#[cfg(target_arch = "wasm32")]
fn main() {
    sp42_app::run_app();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
