// Bundles the sp42-app browser frontend into this crate's compiled output.
//
// Opt-in via SP42_BUNDLE_FRONTEND=1 so a plain `cargo build` for local backend
// development stays fast and doesn't require trunk/wasm32 tooling. Toolforge's
// build service only runs `cargo build --release` (no Trunk/wasm buildpack
// exists there), so this is how the frontend gets built as part of that single
// buildpack invocation instead of shipping a prebuilt bundle through git.
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=SP42_BUNDLE_FRONTEND");
    if env::var("SP42_BUNDLE_FRONTEND").as_deref() != Ok("1") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .join("..")
        .join("..")
        .canonicalize()
        .expect("resolve workspace root");

    run("rustup", &["target", "add", "wasm32-unknown-unknown"], &workspace_root, &[]);

    if Command::new("trunk").arg("--version").output().is_err() {
        println!("cargo:warning=trunk not found, installing via `cargo install trunk --locked`");
        run("cargo", &["install", "trunk", "--locked"], &workspace_root, &[]);
    }

    // Isolate Trunk's internal `cargo build --target wasm32-unknown-unknown` from
    // the outer `cargo build --release` invocation currently running this build
    // script: sharing a target dir across nested cargo invocations can deadlock
    // on cargo's build-directory lock.
    let wasm_target_dir = workspace_root.join("target").join("wasm-bootstrap");
    run(
        "trunk",
        &[
            "build",
            "--config",
            workspace_root.join("Trunk.toml").to_str().expect("utf8 path"),
            "--cargo-profile",
            "web-release",
            "--release",
        ],
        &workspace_root,
        &[("CARGO_TARGET_DIR", wasm_target_dir.to_str().expect("utf8 path"))],
    );
}

fn run(program: &str, args: &[&str], cwd: &Path, extra_env: &[(&str, &str)]) {
    println!("cargo:warning=running: {program} {}", args.join(" "));
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to spawn {program}: {error}"));
    assert!(status.success(), "{program} {} exited with {status}", args.join(" "));
}
