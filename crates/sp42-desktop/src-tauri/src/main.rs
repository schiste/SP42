#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod desktop;
mod shell;

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if shell::is_contract_invocation(&args) {
        match shell::render_shell_bootstrap_from_args(args) {
            Ok(output) => println!("{output}"),
            Err(help_or_error) => println!("{help_or_error}"),
        }
        return;
    }

    if let Err(error) = desktop::run() {
        eprintln!("failed to run SP42 desktop app: {error}");
        std::process::exit(1);
    }
}
