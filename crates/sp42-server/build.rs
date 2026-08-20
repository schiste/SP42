// Bundles the sp42-app browser frontend into this crate's compiled output.
//
// Opt-in via SP42_BUNDLE_FRONTEND=1 so a plain `cargo build` for local backend
// development stays fast and doesn't require trunk/wasm32 tooling. Toolforge's
// build service only runs `cargo build --release` (no Trunk/wasm buildpack
// exists there), so this is how the frontend gets built as part of that single
// buildpack invocation instead of shipping a prebuilt bundle through git.
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const TRUNK_VERSION: &str = "0.21.14";
// SHA-256 of trunk-x86_64-unknown-linux-gnu.tar.gz for TRUNK_VERSION, from the
// upstream release's published .sha256 sidecar file. Update alongside TRUNK_VERSION.
const TRUNK_TARBALL_SHA256: &str =
    "f2b4680cd239693a646a2795e4633c625328d7b2a044fbe749fa3a2fe9e7036b";

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

    let tools_dir = workspace_root.join("target").join("tools");
    let trunk_path = tools_dir.join("trunk");
    if !trunk_path.is_file() {
        println!("cargo:warning=trunk not found, downloading prebuilt {TRUNK_VERSION} release binary");
        std::fs::create_dir_all(&tools_dir).expect("create target/tools dir");
        let url = format!(
            "https://github.com/trunk-rs/trunk/releases/download/v{TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz"
        );
        let tarball = tools_dir.join("trunk.tar.gz");
        run(
            "curl",
            &["-fsSL", "-o", tarball.to_str().expect("utf8 path"), &url],
            &workspace_root,
            &[],
        );
        run(
            "sh",
            &[
                "-c",
                &format!(
                    "echo '{TRUNK_TARBALL_SHA256}  {}' | sha256sum -c -",
                    tarball.display()
                ),
            ],
            &workspace_root,
            &[],
        );
        run(
            "tar",
            &["-xzf", tarball.to_str().expect("utf8 path"), "-C", tools_dir.to_str().expect("utf8 path")],
            &workspace_root,
            &[],
        );
        assert!(trunk_path.is_file(), "trunk binary missing after download");
    }
    let path_with_tools = prepend_to_path(&tools_dir);

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
        &[
            ("CARGO_TARGET_DIR", wasm_target_dir.to_str().expect("utf8 path").to_string()),
            ("PATH", path_with_tools.to_string_lossy().into_owned()),
        ],
    );
}

fn prepend_to_path(dir: &Path) -> OsString {
    let current = env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(env::split_paths(&current));
    env::join_paths(paths).expect("build PATH")
}

fn run(program: &str, args: &[&str], cwd: &Path, extra_env: &[(&str, String)]) {
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
