use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildMode {
    Dev,
    Ci,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopPlatform {
    Host,
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy)]
struct BuildOptions {
    mode: BuildMode,
    clean: bool,
    locked: bool,
    frozen: bool,
    offline: bool,
}

#[derive(Debug, Clone, Copy)]
struct DesktopOptions {
    build: BuildOptions,
    platform: DesktopPlatform,
}

#[derive(Debug, Default, Clone)]
struct ChildEnv {
    vars: Vec<(OsString, OsString)>,
    removals: Vec<OsString>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_help();
        return Ok(());
    };
    let root = repo_root()?;

    match command.as_str() {
        "build-local" => build_local(&root, parse_build_options(args, BuildMode::Dev, false)?),
        "build-server" => build_server(&root, parse_build_options(args, BuildMode::Dev, false)?),
        "build-frontend" => {
            build_frontend(&root, parse_build_options(args, BuildMode::Release, true)?)
        }
        "build-release" | "build-web-release" => {
            build_web_release(&root, parse_build_options(args, BuildMode::Release, true)?)
        }
        "package-vps" => package_vps(&root, parse_build_options(args, BuildMode::Release, true)?),
        "build-desktop" => build_desktop(&root, parse_desktop_options(args)?),
        "ci-all" => ci_all(&root, parse_build_options(args, BuildMode::Ci, true)?),
        "timings" => timings(&root, parse_build_options(args, BuildMode::Ci, true)?),
        "--help" | "-h" | "help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unsupported xtask command: {other}")),
    }
}

fn print_help() {
    println!(
        "\
SP42 build tasks

Usage:
  cargo run -p xtask -- <command> [options]

Commands:
  build-local        Fast local host + wasm build
  build-server       Build only sp42-server
  build-frontend     Build the Trunk browser bundle
  build-web-release  Build the deployable server + browser bundle
  package-vps        Build and package a Wikimedia Cloud VPS artifact
  build-desktop      Build desktop shell targets for the current host
  ci-all             Run the full workspace CI build/test/lint/doc flow
  timings            Generate Cargo timings reports

Common options:
  --clean            Purge generated artifacts, including target/, before building
  --debug            Use dev profile
  --ci               Use ci profile
  --release          Use release profile
  --locked           Pass --locked to Cargo/Trunk
  --unlocked         Disable the command default --locked behavior
  --frozen           Pass --frozen
  --offline          Pass --offline

Desktop options:
  --platform host|macos|windows|linux
"
    );
}

fn parse_build_options<I>(
    args: I,
    default_mode: BuildMode,
    locked_default: bool,
) -> Result<BuildOptions, String>
where
    I: IntoIterator<Item = String>,
{
    let mut options = BuildOptions {
        mode: default_mode,
        clean: false,
        locked: locked_default,
        frozen: false,
        offline: false,
    };

    for arg in args {
        match arg.as_str() {
            "--clean" => options.clean = true,
            "--debug" => {
                options.mode = BuildMode::Dev;
                if locked_default {
                    options.locked = false;
                }
            }
            "--ci" => options.mode = BuildMode::Ci,
            "--release" => options.mode = BuildMode::Release,
            "--locked" => options.locked = true,
            "--unlocked" => options.locked = false,
            "--frozen" => options.frozen = true,
            "--offline" => options.offline = true,
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unsupported option: {arg}")),
        }
    }

    Ok(options)
}

fn parse_desktop_options<I>(args: I) -> Result<DesktopOptions, String>
where
    I: IntoIterator<Item = String>,
{
    let mut raw = args.into_iter().peekable();
    let mut build_args = Vec::new();
    let mut platform = DesktopPlatform::Host;

    while let Some(arg) = raw.next() {
        if arg == "--platform" {
            let value = raw
                .next()
                .ok_or_else(|| "--platform requires a value".to_string())?;
            platform = parse_desktop_platform(&value)?;
        } else {
            build_args.push(arg);
        }
    }

    Ok(DesktopOptions {
        build: parse_build_options(build_args, BuildMode::Dev, false)?,
        platform,
    })
}

fn parse_desktop_platform(value: &str) -> Result<DesktopPlatform, String> {
    match value {
        "host" => Ok(DesktopPlatform::Host),
        "macos" => Ok(DesktopPlatform::Macos),
        "windows" => Ok(DesktopPlatform::Windows),
        "linux" => Ok(DesktopPlatform::Linux),
        _ => Err(format!("unsupported desktop platform: {value}")),
    }
}

fn build_local(root: &Path, options: BuildOptions) -> Result<(), String> {
    prepare_build(root, options)?;
    let cargo = cargo_bin()?;
    let env = build_env(root, options.mode)?;

    let mut host_args = vec![
        "build".to_string(),
        "-p".to_string(),
        "sp42-core".to_string(),
        "-p".to_string(),
        "sp42-server".to_string(),
        "-p".to_string(),
        "sp42-cli".to_string(),
        "-p".to_string(),
        "sp42-desktop".to_string(),
    ];
    append_cargo_profile_flags(&mut host_args, options.mode, false);
    append_repro_flags(&mut host_args, &options);
    run_command(&cargo, &host_args, root, &env)?;

    let mut wasm_args = vec![
        "build".to_string(),
        "-p".to_string(),
        "sp42-app".to_string(),
        "--target".to_string(),
        "wasm32-unknown-unknown".to_string(),
    ];
    append_cargo_profile_flags(&mut wasm_args, options.mode, true);
    append_repro_flags(&mut wasm_args, &options);
    run_command(&cargo, &wasm_args, root, &env)
}

fn build_server(root: &Path, options: BuildOptions) -> Result<(), String> {
    prepare_build(root, options)?;
    let cargo = cargo_bin()?;
    let env = build_env(root, options.mode)?;
    let mut args = vec![
        "build".to_string(),
        "-p".to_string(),
        "sp42-server".to_string(),
    ];
    append_cargo_profile_flags(&mut args, options.mode, false);
    append_repro_flags(&mut args, &options);
    run_command(&cargo, &args, root, &env)
}

fn build_frontend(root: &Path, options: BuildOptions) -> Result<(), String> {
    prepare_build(root, options)?;
    let env = build_env(root, options.mode)?;
    trunk_build(root, options, &env)
}

fn build_web_release(root: &Path, mut options: BuildOptions) -> Result<(), String> {
    options.mode = BuildMode::Release;
    prepare_build(root, options)?;
    build_server_without_prepare(root, options)?;
    let env = build_env(root, BuildMode::Release)?;
    trunk_build(root, options, &env)
}

fn package_vps(root: &Path, mut options: BuildOptions) -> Result<(), String> {
    options.mode = BuildMode::Release;
    build_web_release(root, options)?;

    let package_root = root.join("dist").join("sp42-vps");
    if package_root.exists() {
        fs::remove_dir_all(&package_root)
            .map_err(|error| format!("failed to remove old VPS package: {error}"))?;
    }

    fs::create_dir_all(package_root.join("bin"))
        .map_err(|error| format!("failed to create VPS package bin dir: {error}"))?;
    fs::create_dir_all(package_root.join("dist"))
        .map_err(|error| format!("failed to create VPS package dist dir: {error}"))?;
    fs::create_dir_all(package_root.join("deploy"))
        .map_err(|error| format!("failed to create VPS package deploy dir: {error}"))?;

    copy_file(
        &root
            .join("target")
            .join("release")
            .join(server_binary_name()),
        &package_root.join("bin").join(server_binary_name()),
    )?;
    copy_dir_recursive(
        &frontend_dist_dir(root),
        &package_root.join("dist").join("sp42-app"),
    )?;
    copy_dir_recursive(&root.join("configs"), &package_root.join("configs"))?;
    copy_dir_recursive(&root.join("schemas"), &package_root.join("schemas"))?;
    write_vps_templates(&package_root)?;

    println!("SP42 VPS package written to {}", package_root.display());
    Ok(())
}

fn build_desktop(root: &Path, options: DesktopOptions) -> Result<(), String> {
    validate_desktop_platform(options.platform)?;
    prepare_build(root, options.build)?;

    let cargo = cargo_bin()?;
    let env = build_env(root, options.build.mode)?;

    let mut desktop_args = vec![
        "build".to_string(),
        "-p".to_string(),
        "sp42-desktop".to_string(),
    ];
    append_cargo_profile_flags(&mut desktop_args, options.build.mode, false);
    append_repro_flags(&mut desktop_args, &options.build);
    run_command(&cargo, &desktop_args, root, &env)?;

    let mut tauri_contract_args = vec![
        "build".to_string(),
        "--manifest-path".to_string(),
        root.join("crates")
            .join("sp42-desktop")
            .join("src-tauri")
            .join("Cargo.toml")
            .display()
            .to_string(),
    ];
    append_cargo_profile_flags(&mut tauri_contract_args, options.build.mode, false);
    append_repro_flags(&mut tauri_contract_args, &options.build);
    run_command(&cargo, &tauri_contract_args, root, &env)
}

fn ci_all(root: &Path, options: BuildOptions) -> Result<(), String> {
    prepare_build(root, options)?;
    let cargo = cargo_bin()?;
    let env = build_env(root, BuildMode::Ci)?;

    let mut build_args = vec![
        "build".to_string(),
        "--workspace".to_string(),
        "--all-targets".to_string(),
        "--profile".to_string(),
        "ci".to_string(),
    ];
    append_repro_flags(&mut build_args, &options);
    run_command(&cargo, &build_args, root, &env)?;

    let mut test_args = vec![
        "test".to_string(),
        "--workspace".to_string(),
        "--profile".to_string(),
        "ci".to_string(),
    ];
    append_repro_flags(&mut test_args, &options);
    run_command(&cargo, &test_args, root, &env)?;

    let mut clippy_args = vec![
        "clippy".to_string(),
        "--workspace".to_string(),
        "--all-targets".to_string(),
        "--all-features".to_string(),
        "--profile".to_string(),
        "ci".to_string(),
    ];
    append_repro_flags(&mut clippy_args, &options);
    clippy_args.push("--".to_string());
    clippy_args.push("-D".to_string());
    clippy_args.push("warnings".to_string());
    run_command(&cargo, &clippy_args, root, &env)?;

    let mut doc_args = vec![
        "doc".to_string(),
        "--workspace".to_string(),
        "--no-deps".to_string(),
        "--profile".to_string(),
        "ci".to_string(),
    ];
    append_repro_flags(&mut doc_args, &options);
    run_command(&cargo, &doc_args, root, &env)?;

    let mut wasm_args = vec![
        "build".to_string(),
        "-p".to_string(),
        "sp42-app".to_string(),
        "--target".to_string(),
        "wasm32-unknown-unknown".to_string(),
        "--profile".to_string(),
        "ci".to_string(),
    ];
    append_repro_flags(&mut wasm_args, &options);
    run_command(&cargo, &wasm_args, root, &env)?;

    trunk_build(root, options, &env)?;

    let mut tauri_contract_args = vec![
        "build".to_string(),
        "--manifest-path".to_string(),
        root.join("crates")
            .join("sp42-desktop")
            .join("src-tauri")
            .join("Cargo.toml")
            .display()
            .to_string(),
        "--profile".to_string(),
        "ci".to_string(),
    ];
    append_repro_flags(&mut tauri_contract_args, &options);
    run_command(&cargo, &tauri_contract_args, root, &env)
}

fn timings(root: &Path, options: BuildOptions) -> Result<(), String> {
    prepare_build(root, options)?;
    let cargo = cargo_bin()?;
    let env = build_env(root, BuildMode::Ci)?;

    let mut host_args = vec![
        "build".to_string(),
        "--workspace".to_string(),
        "--all-targets".to_string(),
        "--profile".to_string(),
        "ci".to_string(),
        "--timings".to_string(),
    ];
    append_repro_flags(&mut host_args, &options);
    run_command(&cargo, &host_args, root, &env)?;

    let mut wasm_args = vec![
        "build".to_string(),
        "-p".to_string(),
        "sp42-app".to_string(),
        "--target".to_string(),
        "wasm32-unknown-unknown".to_string(),
        "--profile".to_string(),
        "ci".to_string(),
        "--timings".to_string(),
    ];
    append_repro_flags(&mut wasm_args, &options);
    run_command(&cargo, &wasm_args, root, &env)
}

fn build_server_without_prepare(root: &Path, options: BuildOptions) -> Result<(), String> {
    let cargo = cargo_bin()?;
    let env = build_env(root, options.mode)?;
    let mut args = vec![
        "build".to_string(),
        "-p".to_string(),
        "sp42-server".to_string(),
    ];
    append_cargo_profile_flags(&mut args, options.mode, false);
    append_repro_flags(&mut args, &options);
    run_command(&cargo, &args, root, &env)
}

fn prepare_build(root: &Path, options: BuildOptions) -> Result<(), String> {
    if options.clean {
        clean_workspace(root)?;
    }
    Ok(())
}

fn clean_workspace(root: &Path) -> Result<(), String> {
    for relative in [
        ".tmp",
        ".sp42-runtime",
        "coverage",
        "dist",
        "crates/sp42-app/dist",
        "target/dist",
        "crates/sp42-desktop/src-tauri/target",
        "target",
    ] {
        remove_path_if_exists(&root.join(relative))?;
    }
    println!("SP42 cleanup complete.");
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)
            .map_err(|error| format!("failed to remove {}: {error}", path.display())),
        Ok(_) => fs::remove_file(path)
            .map_err(|error| format!("failed to remove {}: {error}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to inspect {}: {error}", path.display())),
    }
}

fn append_cargo_profile_flags(args: &mut Vec<String>, mode: BuildMode, wasm: bool) {
    match (mode, wasm) {
        (BuildMode::Dev, _) => {}
        (BuildMode::Ci, _) => {
            args.push("--profile".to_string());
            args.push("ci".to_string());
        }
        (BuildMode::Release, true) => {
            args.push("--profile".to_string());
            args.push("web-release".to_string());
        }
        (BuildMode::Release, false) => args.push("--release".to_string()),
    }
}

fn append_repro_flags(args: &mut Vec<String>, options: &BuildOptions) {
    if options.locked {
        args.push("--locked".to_string());
    }
    if options.frozen {
        args.push("--frozen".to_string());
    }
    if options.offline {
        args.push("--offline".to_string());
    }
}

fn trunk_build(root: &Path, options: BuildOptions, envs: &ChildEnv) -> Result<(), String> {
    let trunk = find_executable("trunk").ok_or_else(|| {
        "trunk is required for frontend builds. Install it with: cargo install trunk".to_string()
    })?;
    let mut args = vec![
        "build".to_string(),
        "--config".to_string(),
        root.join("Trunk.toml").display().to_string(),
    ];

    match options.mode {
        BuildMode::Dev => {}
        BuildMode::Ci => {
            args.push("--cargo-profile".to_string());
            args.push("ci".to_string());
        }
        BuildMode::Release => {
            args.push("--cargo-profile".to_string());
            args.push("web-release".to_string());
            args.push("--release".to_string());
        }
    }
    append_repro_flags(&mut args, &options);

    let mut envs = envs.clone();
    envs.vars.push((
        OsString::from("SP42_APP_DIST_DIR"),
        frontend_dist_dir(root).into_os_string(),
    ));
    if let Some(parent) = cargo_bin()?.parent() {
        envs.vars.push((
            OsString::from("PATH"),
            prepend_path(parent, env::var_os("PATH").unwrap_or_default())?,
        ));
    }

    fs::create_dir_all(frontend_dist_dir(root))
        .map_err(|error| format!("failed to create frontend dist dir: {error}"))?;
    run_command(&trunk, &args, root, &envs)
}

fn build_env(root: &Path, mode: BuildMode) -> Result<ChildEnv, String> {
    let mut envs = ChildEnv::default();

    if env::var_os("CARGO_TARGET_DIR").is_none() {
        envs.vars.push((
            OsString::from("CARGO_TARGET_DIR"),
            root.join("target").into_os_string(),
        ));
    }
    if env::var_os("CARGO_BUILD_JOBS").is_none() {
        envs.vars.push((
            OsString::from("CARGO_BUILD_JOBS"),
            available_parallelism().to_string().into(),
        ));
    }
    envs.vars
        .push((OsString::from("CLICOLOR"), OsString::from("0")));
    envs.removals.push(OsString::from("NO_COLOR"));

    if let Some(rustc) = rustup_which("rustc")?
        && env::var_os("RUSTC").is_none()
    {
        envs.vars
            .push((OsString::from("RUSTC"), rustc.into_os_string()));
    }
    if let Some(rustdoc) = rustup_which("rustdoc")?
        && env::var_os("RUSTDOC").is_none()
    {
        envs.vars
            .push((OsString::from("RUSTDOC"), rustdoc.into_os_string()));
    }

    configure_sccache(&mut envs)?;

    match mode {
        BuildMode::Release => {
            if env::var_os("CARGO_INCREMENTAL").is_none() {
                envs.vars
                    .push((OsString::from("CARGO_INCREMENTAL"), OsString::from("0")));
            }
            if env::var_os("SOURCE_DATE_EPOCH").is_none() {
                envs.vars.push((
                    OsString::from("SOURCE_DATE_EPOCH"),
                    source_date_epoch(root).into(),
                ));
            }
            let remap = format!("--remap-path-prefix={}=.", root.display());
            envs.vars.push((
                OsString::from("RUSTFLAGS"),
                append_flag(env::var_os("RUSTFLAGS"), &remap),
            ));
            envs.vars.push((
                OsString::from("RUSTDOCFLAGS"),
                append_flag(env::var_os("RUSTDOCFLAGS"), &remap),
            ));
        }
        BuildMode::Ci => {
            if env::var_os("CARGO_INCREMENTAL").is_none() {
                envs.vars
                    .push((OsString::from("CARGO_INCREMENTAL"), OsString::from("0")));
            }
        }
        BuildMode::Dev => {
            if env::var_os("CARGO_INCREMENTAL").is_none() {
                envs.vars
                    .push((OsString::from("CARGO_INCREMENTAL"), OsString::from("1")));
            }
        }
    }

    Ok(envs)
}

fn configure_sccache(envs: &mut ChildEnv) -> Result<(), String> {
    let requested = env::var("SP42_USE_SCCACHE").unwrap_or_else(|_| "auto".to_string());
    let enabled = match requested.as_str() {
        "auto" => find_executable("sccache").is_some(),
        "1" | "true" | "TRUE" | "yes" | "YES" => {
            if find_executable("sccache").is_none() {
                return Err(
                    "SP42_USE_SCCACHE is enabled but `sccache` is not installed.".to_string(),
                );
            }
            true
        }
        "0" | "false" | "FALSE" | "no" | "NO" => false,
        _ => return Err(format!("Invalid SP42_USE_SCCACHE value: {requested}")),
    };

    if enabled && env::var_os("RUSTC_WRAPPER").is_none() {
        let sccache = find_executable("sccache")
            .ok_or_else(|| "sccache was enabled but could not be resolved".to_string())?;
        envs.vars
            .push((OsString::from("RUSTC_WRAPPER"), sccache.into_os_string()));
        if env::var_os("SCCACHE_IDLE_TIMEOUT").is_none() {
            envs.vars
                .push((OsString::from("SCCACHE_IDLE_TIMEOUT"), OsString::from("0")));
        }
    }

    Ok(())
}

fn validate_desktop_platform(platform: DesktopPlatform) -> Result<(), String> {
    let host = if cfg!(target_os = "macos") {
        DesktopPlatform::Macos
    } else if cfg!(target_os = "windows") {
        DesktopPlatform::Windows
    } else {
        DesktopPlatform::Linux
    };

    if platform == DesktopPlatform::Host || platform == host {
        return Ok(());
    }

    Err(format!(
        "build-desktop currently builds host-native desktop targets only; requested {platform:?} on {host:?}"
    ))
}

fn write_vps_templates(package_root: &Path) -> Result<(), String> {
    fs::write(
        package_root.join("deploy").join("sp42.env.example"),
        "\
SP42_BIND_ADDR=127.0.0.1:8788
SP42_PUBLIC_BASE_URL=https://sp42.example.wmcloud.org
SP42_APP_DIST_DIR=/opt/sp42/dist/sp42-app
SP42_RUNTIME_DIR=/var/lib/sp42
SP42_DEPLOYMENT_MODE=vps
SP42_SUPERVISOR_WIKIS=frwiki
SP42_INGESTION_POLL_MS=15000
",
    )
    .map_err(|error| format!("failed to write VPS env template: {error}"))?;

    fs::write(
        package_root.join("deploy").join("sp42.service"),
        "\
[Unit]
Description=SP42 Wikimedia patrol workbench
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=sp42
Group=sp42
EnvironmentFile=/etc/sp42/sp42.env
ExecStart=/opt/sp42/bin/sp42-server
Restart=on-failure
RestartSec=5
WorkingDirectory=/opt/sp42

[Install]
WantedBy=multi-user.target
",
    )
    .map_err(|error| format!("failed to write systemd template: {error}"))?;

    fs::write(
        package_root.join("README.md"),
        "\
# SP42 VPS Package

This package contains the host-built `sp42-server` binary, the Trunk browser
bundle, configs, schemas, and starter systemd/environment templates.

Build this package on the same operating system and CPU architecture as the
target Wikimedia Cloud VPS instance unless a dedicated cross-compilation path
has been added.
",
    )
    .map_err(|error| format!("failed to write VPS package README: {error}"))
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::copy(from, to).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            from.display(),
            to.display()
        )
    })?;
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), String> {
    if !from.is_dir() {
        return Err(format!(
            "source directory does not exist: {}",
            from.display()
        ));
    }
    fs::create_dir_all(to)
        .map_err(|error| format!("failed to create {}: {error}", to.display()))?;

    for entry in
        fs::read_dir(from).map_err(|error| format!("failed to read {}: {error}", from.display()))?
    {
        let entry = entry.map_err(|error| format!("failed to read directory entry: {error}"))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else {
            copy_file(&source, &target)?;
        }
    }

    Ok(())
}

fn run_command(
    program: &Path,
    args: &[String],
    root: &Path,
    envs: &ChildEnv,
) -> Result<(), String> {
    println!("$ {} {}", program.display(), args.join(" "));
    let mut command = Command::new(program);
    command.args(args).current_dir(root);
    for (key, value) in &envs.vars {
        command.env(key, value);
    }
    for key in &envs.removals {
        command.env_remove(key);
    }

    let status = command
        .status()
        .map_err(|error| format!("failed to run {}: {error}", program.display()))?;
    exit_status_result(status, program, args)
}

fn exit_status_result(status: ExitStatus, program: &Path, args: &[String]) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} {} exited with {status}",
            program.display(),
            args.join(" ")
        ))
    }
}

fn repo_root() -> Result<PathBuf, String> {
    let manifest_dir =
        env::var("CARGO_MANIFEST_DIR").map_err(|error| format!("CARGO_MANIFEST_DIR: {error}"))?;
    Path::new(&manifest_dir)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest dir has no parent".to_string())
}

fn cargo_bin() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("CARGO_BIN") {
        return Ok(PathBuf::from(value));
    }
    if let Some(path) = rustup_which("cargo")? {
        return Ok(path);
    }
    find_executable("cargo").ok_or_else(|| "cargo was not found in PATH".to_string())
}

fn rustup_which(tool: &str) -> Result<Option<PathBuf>, String> {
    let Some(rustup) = find_executable("rustup") else {
        return Ok(None);
    };
    let output = Command::new(rustup)
        .arg("which")
        .arg(tool)
        .output()
        .map_err(|error| format!("failed to run rustup which {tool}: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn prepend_path(prefix: &Path, current: OsString) -> Result<OsString, String> {
    let mut paths = vec![prefix.to_path_buf()];
    paths.extend(env::split_paths(&current));
    env::join_paths(paths).map_err(|error| format!("failed to build PATH: {error}"))
}

fn frontend_dist_dir(root: &Path) -> PathBuf {
    root.join("target").join("dist").join("sp42-app")
}

fn server_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "sp42-server.exe"
    } else {
        "sp42-server"
    }
}

fn available_parallelism() -> usize {
    std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
}

fn source_date_epoch(root: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("log")
        .arg("-1")
        .arg("--format=%ct")
        .output();

    output.map_or_else(
        |_| "0".to_string(),
        |output| {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if value.is_empty() {
                    "0".to_string()
                } else {
                    value
                }
            } else {
                "0".to_string()
            }
        },
    )
}

fn append_flag(current: Option<OsString>, new_flag: &str) -> OsString {
    let Some(current) = current else {
        return OsString::from(new_flag);
    };
    let current_string = current.to_string_lossy();
    if current_string
        .split_whitespace()
        .any(|flag| flag == new_flag)
    {
        current
    } else if current_string.is_empty() {
        OsString::from(new_flag)
    } else {
        OsString::from(format!("{current_string} {new_flag}"))
    }
}
