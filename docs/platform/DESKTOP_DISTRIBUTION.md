# SP42 Desktop Distribution

SP42 uses Tauri as the single desktop shell for macOS, Windows, and Linux. The
desktop shell loads the Trunk-built browser bundle from `target/dist/sp42-app`.

## Backend Modes

The desktop app supports two backend modes:

- `SP42_DESKTOP_BACKEND_MODE=sidecar` starts the bundled `sp42-server` sidecar
  and injects its loopback API URL into the frontend. This is the default.
- `SP42_DESKTOP_BACKEND_MODE=remote` keeps the desktop app as a native shell but
  points the frontend at a deployed backend. Set
  `SP42_DESKTOP_REMOTE_BACKEND_URL=https://sp42.<project>.wmcloud.org` or
  `SP42_PUBLIC_BASE_URL`.

Sidecar mode uses:

```env
SP42_DEPLOYMENT_MODE=desktop
SP42_DESKTOP_SIDECAR_BIND_ADDR=127.0.0.1:8788
SP42_RUNTIME_DIR=<platform app data dir>/runtime
```

## Local macOS Build

Prerequisites:

- Rust toolchain used by the workspace
- `wasm32-unknown-unknown` Rust target
- `trunk`
- `cargo tauri`
- Xcode or Xcode Command Line Tools on macOS

Build the unsigned macOS app and DMG:

```sh
./scripts/build-desktop.sh --platform macos
```

For a faster compile-only desktop check:

```sh
./scripts/build-desktop.sh --platform macos --debug
```

The Tauri sidecar is prepared by:

```sh
crates/sp42-desktop/scripts/prepare-tauri-build.sh --release
```

That script builds the browser bundle, builds `sp42-server`, and copies the
server binary to `crates/sp42-desktop/src-tauri/binaries/sp42-server-<target>`,
which is the naming convention Tauri expects for sidecars.
Use `--locked`, `--frozen`, or `--offline` when preparing release artifacts that
must enforce the workspace lockfile/cache state.

## Signing And Notarization

The current repository is configured for unsigned artifacts first:

- macOS bundle identifier: `org.sp42.desktop`
- macOS entitlements: `crates/sp42-desktop/src-tauri/Entitlements.plist`
- macOS hardened runtime: enabled in `tauri.conf.json`
- Signing identity/provider: left as `null` in config and expected through
  Tauri/Apple environment variables once credentials exist

For signed macOS distribution, configure:

```env
APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)"
APPLE_PROVIDER_SHORT_NAME="<TEAMID or provider short name>"
APPLE_ID="<apple-id@example.org>"
APPLE_PASSWORD="<app-specific-password>"
APPLE_TEAM_ID="<TEAMID>"
```

Then run the release build without `--no-sign` once certificates and notarization
credentials are available.

## Cross-Platform Releases

`.github/workflows/desktop-release.yml` builds native unsigned artifacts first:

- macOS: `.app` and `.dmg`
- Windows: `.msi` and NSIS setup executable
- Linux: `.deb` and AppImage

The workflow intentionally builds on each target OS instead of cross-compiling
Windows installers from macOS/Linux. Signed releases are a later step:

- macOS: Developer ID certificate and notarization
- Windows: code signing certificate
- Linux: package signing or AppImage GPG signing

References:

- Tauri prerequisites: https://v2.tauri.app/start/prerequisites/
- Tauri distribution: https://v2.tauri.app/distribute/
- Tauri sidecars: https://v2.tauri.app/develop/sidecar/
- Tauri Windows installers: https://v2.tauri.app/distribute/windows-installer/
