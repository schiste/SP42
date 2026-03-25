use std::fs;
use std::path::Path;

fn main() {
    let static_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    for asset in ["manifest.json", "sw.js", "offline.html"] {
        println!(
            "cargo:rerun-if-changed={}",
            static_dir.join(asset).display()
        );
    }

    let env_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(".env.wikimedia.local");
    println!("cargo:rerun-if-changed={}", env_path.display());

    let Ok(contents) = fs::read_to_string(&env_path) else {
        return;
    };

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        let value = parse_value(raw_value.trim());

        match key {
            "WIKIMEDIA_CLIENT_APPLICATION_KEY" => {
                println!("cargo:rustc-env=SP42_WIKIMEDIA_CLIENT_APPLICATION_KEY={value}");
            }
            "WIKIMEDIA_OAUTH_CALLBACK_URL" => {
                println!("cargo:rustc-env=SP42_WIKIMEDIA_OAUTH_CALLBACK_URL={value}");
            }
            _ => {}
        }
    }
}

fn parse_value(raw_value: &str) -> String {
    if raw_value.len() >= 2 {
        let first = raw_value.as_bytes()[0];
        let last = raw_value.as_bytes()[raw_value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return raw_value[1..raw_value.len() - 1].to_string();
        }
    }

    raw_value.to_string()
}
