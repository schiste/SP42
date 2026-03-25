# Contributing to SP42

SP42 is still early-stage software. Contributions are welcome, but changes should stay aligned with the current architecture and project constraints.

## Before You Start

- Read [README.md](README.md)
- Read [docs/STATUS.md](docs/STATUS.md)
- Read [CONSTITUTION.md](CONSTITUTION.md)
- Review relevant ADRs in [docs/adr](docs/adr)

## Development Expectations

- Keep secrets and personal Wikimedia credentials out of the repository
- Prefer small, focused pull requests
- Do not introduce new warnings
- Keep shared logic in `sp42-core` when it is genuinely cross-target
- Avoid architecture drift unless the change is explicitly justified

## Local Checks

Run these before opening a pull request:

```sh
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace
```

If your change touches the browser app, also run:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p sp42-app --target wasm32-unknown-unknown
```

## Credentials and Local Auth

If you use the local Wikimedia development bridge:

- keep credentials in `.env.wikimedia.local`
- never commit that file
- never paste raw access tokens into code, docs, issues, or PRs

## Pull Requests

A good PR should include:

- a clear problem statement
- the smallest reasonable implementation
- tests or validation notes
- any documentation updates required by the change

