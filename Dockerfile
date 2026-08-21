# syntax=docker/dockerfile:1.7

FROM rust:1.96.0-bookworm AS builder

ARG TRUNK_VERSION=0.21.14

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        git \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked --version "${TRUNK_VERSION}"

WORKDIR /src
COPY . .

# The repository build task compiles both the Axum server and the Trunk/Wasm app.
ENV SP42_USE_SCCACHE=0

RUN --mount=type=cache,id=sp42-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=sp42-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=sp42-target,target=/src/target \
    ./scripts/build-web-release.sh \
    && mkdir -p /out/bin /out/dist \
    && cp target/release/sp42-server /out/bin/sp42-server \
    && cp -a target/dist/sp42-app /out/dist/sp42-app

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 sp42 \
    && useradd --uid 10001 --gid sp42 --system --no-create-home sp42

WORKDIR /opt/sp42

COPY --from=builder --chown=sp42:sp42 /out/bin/sp42-server ./bin/sp42-server
COPY --from=builder --chown=sp42:sp42 /out/dist/sp42-app ./dist/sp42-app
COPY --chown=sp42:sp42 configs ./configs
COPY --chown=sp42:sp42 schemas ./schemas

RUN mkdir -p /var/lib/sp42 \
    && chown sp42:sp42 /var/lib/sp42

WORKDIR /var/lib/sp42

# SP42_DEPLOYMENT_MODE is intentionally not set here: the server requires it
# explicitly (local, vps, or desktop; see docs/platform/RUNTIME_CONFIGURATION.md)
# and refuses to start without it, so this image can't silently boot with the
# local-only dev-auth bootstrap enabled. Pass it at `docker run` time.
ENV SP42_BIND_ADDR=127.0.0.1:8788 \
    SP42_APP_DIST_DIR=/opt/sp42/dist/sp42-app \
    SP42_WIKI_CONFIG_DIR=/opt/sp42/configs \
    SP42_RUNTIME_DIR=/var/lib/sp42 \
    RUST_LOG=info

USER 10001:10001

EXPOSE 8788
VOLUME ["/var/lib/sp42"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:8788/healthz"]

ENTRYPOINT ["/opt/sp42/bin/sp42-server"]
