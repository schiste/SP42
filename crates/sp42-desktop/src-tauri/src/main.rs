mod shell;

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_none() {
        println!("{}", shell::render_shell_bootstrap());
        return;
    }

    let format = match shell::parse_shell_format(args) {
        Ok(format) => format,
        Err(help_or_error) => {
            println!("{help_or_error}");
            return;
        }
    };

    println!("{}", shell::render_shell_bootstrap_with_format(format));
}
