use serde::Serialize;

use sp42_core::branding::{CONFIG_PAGE_PREFIX, PROJECT_NAME, PROJECT_SLUG, USER_AGENT};

pub const APP_IDENTIFIER: &str = "org.sp42.desktop";
pub const FRONTEND_DIST: &str = "../../sp42-app/dist";
pub const WINDOW_TITLE: &str = PROJECT_NAME;
pub const SHELL_MODE: &str = "local-native-shell";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFormat {
    Text,
    Markdown,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NativeShellContract {
    pub product_name: &'static str,
    pub project_slug: &'static str,
    pub app_identifier: &'static str,
    pub window_title: &'static str,
    pub frontend_dist: &'static str,
    pub shell_mode: &'static str,
    pub primary_surface: &'static str,
    pub shared_surface: &'static str,
    pub launch_notes: Vec<&'static str>,
    pub branding_hint: &'static str,
    pub user_agent: &'static str,
}

#[must_use]
pub fn native_shell_contract() -> NativeShellContract {
    NativeShellContract {
        product_name: PROJECT_NAME,
        project_slug: PROJECT_SLUG,
        app_identifier: APP_IDENTIFIER,
        window_title: WINDOW_TITLE,
        frontend_dist: FRONTEND_DIST,
        shell_mode: SHELL_MODE,
        primary_surface: "desktop operator summary + patrol session digest + shell state",
        shared_surface: "shared with browser/CLI/server contract reports",
        launch_notes: vec![
            "local-first shell contract; no network bootstrap required",
            "mirrors the shared desktop operator summary and session digest story",
            "keeps the Tauri wrapper thin while the Rust desktop surface owns behavior",
            "builds against the browser dist when available, but remains contract-driven without it",
        ],
        branding_hint: CONFIG_PAGE_PREFIX,
        user_agent: USER_AGENT,
    }
}

#[must_use]
pub fn render_shell_bootstrap() -> String {
    render_shell_bootstrap_with_format(ShellFormat::Text)
}

#[must_use]
pub fn render_shell_bootstrap_with_format(format: ShellFormat) -> String {
    let contract = native_shell_contract();

    match format {
        ShellFormat::Text => render_shell_bootstrap_text(&contract),
        ShellFormat::Markdown => render_shell_bootstrap_markdown(&contract),
        ShellFormat::Json => render_shell_bootstrap_json(&contract),
    }
}

pub fn parse_shell_format(args: impl IntoIterator<Item = String>) -> Result<ShellFormat, String> {
    let mut args = args.into_iter();
    let mut format = ShellFormat::Text;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--format requires a value".to_string())?;
                format = parse_shell_format_value(&value)?;
            }
            "--help" | "-h" => {
                return Err(render_shell_help());
            }
            _ => return Err(format!("unsupported argument: {arg}")),
        }
    }

    Ok(format)
}

fn parse_shell_format_value(value: &str) -> Result<ShellFormat, String> {
    match value {
        "text" => Ok(ShellFormat::Text),
        "markdown" => Ok(ShellFormat::Markdown),
        "json" => Ok(ShellFormat::Json),
        _ => Err(format!("unsupported output format: {value}")),
    }
}

fn render_shell_bootstrap_text(contract: &NativeShellContract) -> String {
    [
        format!("{PROJECT_NAME} Tauri native shell"),
        format!("product_name={}", contract.product_name),
        format!("project_slug={}", contract.project_slug),
        format!("identifier={}", contract.app_identifier),
        format!("window_title={}", contract.window_title),
        format!("frontend_dist={}", contract.frontend_dist),
        format!("shell_mode={}", contract.shell_mode),
        format!("primary_surface={}", contract.primary_surface),
        format!("shared_surface={}", contract.shared_surface),
        format!("branding_hint={}", contract.branding_hint),
        format!("user_agent={}", contract.user_agent),
        format!("launch_notes={}", contract.launch_notes.join(" | ")),
    ]
    .join("\n")
}

fn render_shell_bootstrap_markdown(contract: &NativeShellContract) -> String {
    [
        format!("# {PROJECT_NAME} native shell"),
        format!(
            "- `product_name`: `{}`\n- `project_slug`: `{}`\n- `identifier`: `{}`\n- `window_title`: `{}`\n- `frontend_dist`: `{}`\n- `shell_mode`: `{}`",
            contract.product_name,
            contract.project_slug,
            contract.app_identifier,
            contract.window_title,
            contract.frontend_dist,
            contract.shell_mode,
        ),
        format!(
            "- `primary_surface`: {}\n- `shared_surface`: {}\n- `branding_hint`: `{}`\n- `user_agent`: `{}`",
            contract.primary_surface,
            contract.shared_surface,
            contract.branding_hint,
            contract.user_agent,
        ),
        format!(
            "- `launch_notes`:\n{}",
            contract
                .launch_notes
                .iter()
                .map(|note| format!("  - {note}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    ]
    .join("\n\n")
}

fn render_shell_bootstrap_json(contract: &NativeShellContract) -> String {
    serde_json::to_string_pretty(contract).unwrap_or_else(|error| {
        format!(
            "{{\"error\":\"failed to serialize native shell contract: {error}\"}}"
        )
    })
}

fn render_shell_help() -> String {
    [
        "SP42 native shell bootstrap".to_string(),
        "Usage: sp42-desktop-tauri [--format text|markdown|json]".to_string(),
        "The native shell is local-first and mirrors the shared desktop operator story.".to_string(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{
        APP_IDENTIFIER, FRONTEND_DIST, ShellFormat, native_shell_contract, parse_shell_format,
        render_shell_bootstrap_with_format,
    };

    #[test]
    fn shell_contract_uses_shared_branding() {
        let contract = native_shell_contract();

        assert_eq!(contract.product_name, "SP42");
        assert_eq!(contract.project_slug, "sp42");
        assert_eq!(contract.app_identifier, APP_IDENTIFIER);
        assert_eq!(contract.frontend_dist, FRONTEND_DIST);
        assert!(contract.primary_surface.contains("operator summary"));
    }

    #[test]
    fn renders_text_bootstrap() {
        let output = render_shell_bootstrap_with_format(ShellFormat::Text);

        assert!(output.contains("SP42 Tauri native shell"));
        assert!(output.contains("shell_mode=local-native-shell"));
        assert!(output.contains("primary_surface=desktop operator summary"));
    }

    #[test]
    fn renders_markdown_bootstrap() {
        let output = render_shell_bootstrap_with_format(ShellFormat::Markdown);

        assert!(output.contains("# SP42 native shell"));
        assert!(output.contains("`identifier`: `org.sp42.desktop`"));
        assert!(output.contains("launch_notes"));
    }

    #[test]
    fn renders_json_bootstrap() {
        let output = render_shell_bootstrap_with_format(ShellFormat::Json);
        let parsed: Value = serde_json::from_str(&output).expect("json should parse");

        assert_eq!(parsed["product_name"], "SP42");
        assert_eq!(parsed["app_identifier"], "org.sp42.desktop");
        assert_eq!(parsed["shell_mode"], "local-native-shell");
    }

    #[test]
    fn parses_shell_format_flag() {
        let format = parse_shell_format([
            "--format".to_string(),
            "markdown".to_string(),
        ])
        .expect("format should parse");

        assert!(matches!(format, ShellFormat::Markdown));
    }
}
