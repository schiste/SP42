fn main() {
    ensure_debug_sidecar_placeholder();
    tauri_build::build();
}

fn ensure_debug_sidecar_placeholder() {
    if std::env::var("PROFILE").ok().as_deref() != Some("debug") {
        return;
    }

    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let extension = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let sidecar = std::path::Path::new("binaries").join(format!("sp42-server-{target}{extension}"));
    if sidecar.exists() {
        return;
    }

    if let Some(parent) = sidecar.parent() {
        std::fs::create_dir_all(parent).expect("failed to create debug sidecar directory");
    }
    let placeholder = "SP42 debug sidecar placeholder. Run crates/sp42-desktop/scripts/prepare-tauri-build.sh before launching the desktop app.\n";
    std::fs::write(&sidecar, placeholder).expect("failed to write debug sidecar placeholder");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mut permissions = std::fs::metadata(&sidecar)
            .expect("failed to stat debug sidecar placeholder")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&sidecar, permissions)
            .expect("failed to chmod debug sidecar placeholder");
    }
}
