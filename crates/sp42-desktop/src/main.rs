use std::process::ExitCode;

use futures::executor::block_on;
use sp42_devtools::{
    DevContextOptions, DevOperatorSurface, DevOperatorSurfaceOptions, DevWorkbenchOptions,
    build_default_dev_operator_surface, render_dev_transport_lines,
};
use sp42_patrol::{
    PatrolOperatorSummary, PatrolSessionDigest, render_patrol_operator_summary_markdown,
    render_patrol_operator_summary_text, render_patrol_scenario_markdown,
    render_patrol_scenario_text, render_patrol_session_digest_markdown,
    render_patrol_session_digest_text, render_shell_state_markdown, render_shell_state_text,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Text,
    Markdown,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DesktopOptions {
    format: OutputFormat,
}

#[derive(Debug, Clone)]
struct DesktopConsoleSnapshot {
    surface: DevOperatorSurface,
    transport_lines: Vec<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<String, String> {
    let options = parse_options(std::env::args().skip(1))?;
    run_with_format(options.format)
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<DesktopOptions, String> {
    let mut args = args.into_iter();
    let mut format = OutputFormat::Text;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--format requires a value".to_string())?;
                format = match value.as_str() {
                    "text" => OutputFormat::Text,
                    "markdown" => OutputFormat::Markdown,
                    "json" => OutputFormat::Json,
                    _ => return Err(format!("unsupported output format: {value}")),
                };
            }
            _ => return Err(format!("unsupported argument: {arg}")),
        }
    }

    Ok(DesktopOptions { format })
}

fn run_with_format(format: OutputFormat) -> Result<String, String> {
    let snapshot = build_console_snapshot()?;

    match format {
        OutputFormat::Text => Ok(render_text_snapshot(&snapshot)),
        OutputFormat::Markdown => Ok(render_markdown_snapshot(&snapshot)),
        OutputFormat::Json => render_json_snapshot(&snapshot),
    }
}

fn render_text_snapshot(snapshot: &DesktopConsoleSnapshot) -> String {
    let surface = &snapshot.surface;
    [
        format!(
            "{} native operator console",
            sp42_core::branding::PROJECT_NAME
        ),
        render_desktop_summary(&surface.operator_summary, &surface.session_digest),
        render_patrol_operator_summary_text(&surface.operator_summary),
        render_patrol_session_digest_text(&surface.session_digest),
        render_shell_state_text(&surface.shell_state),
        render_patrol_scenario_text(&surface.report),
        render_markdown_section("Transport", &snapshot.transport_lines.join("\n")),
    ]
    .join("\n\n")
}

fn render_markdown_snapshot(snapshot: &DesktopConsoleSnapshot) -> String {
    let surface = &snapshot.surface;
    [
        render_markdown_section(
            "Desktop summary",
            &render_desktop_summary(&surface.operator_summary, &surface.session_digest),
        ),
        render_patrol_operator_summary_markdown(&surface.operator_summary),
        render_patrol_session_digest_markdown(&surface.session_digest),
        render_markdown_section(
            "Shell state",
            &render_shell_state_markdown(&surface.shell_state),
        ),
        render_patrol_scenario_markdown(&surface.report),
        render_markdown_section("Transport", &snapshot.transport_lines.join("\n")),
    ]
    .join("\n\n")
}

fn render_json_snapshot(snapshot: &DesktopConsoleSnapshot) -> Result<String, String> {
    let surface = &snapshot.surface;
    serde_json::to_string_pretty(&serde_json::json!({
        "project": sp42_core::branding::PROJECT_NAME,
        "desktop_summary": render_desktop_summary_value(&surface.operator_summary, &surface.session_digest),
        "operator_summary": &surface.operator_summary,
        "session_digest": &surface.session_digest,
        "shell_state": &surface.shell_state,
        "report": &surface.report,
        "transport_lines": &snapshot.transport_lines,
    }))
    .map_err(|error| error.to_string())
}

fn render_desktop_summary(
    operator_summary: &PatrolOperatorSummary,
    session_digest: &PatrolSessionDigest,
) -> String {
    let selected = operator_summary.selected.as_ref().map_or_else(
        || "selected=none".to_string(),
        |selected| {
            format!(
                "selected_rev={} title=\"{}\" score={} signals={}",
                selected.rev_id, selected.title, selected.score, selected.signals
            )
        },
    );
    let workbench = operator_summary.workbench.as_ref().map_or_else(
        || "action_workbench=none".to_string(),
        |workbench| {
            format!(
                "action_workbench_rev={} requests={} training_rows={}",
                workbench.rev_id,
                workbench.request_labels.len(),
                workbench.training_rows
            )
        },
    );
    let severity_counts = operator_summary
        .severity_counts
        .iter()
        .map(|count| format!("{:?}={}", count.severity, count.count))
        .collect::<Vec<_>>()
        .join(" ");
    let available_sections = operator_summary
        .section_overview
        .iter()
        .filter(|section| section.available)
        .count();

    format!(
        "desktop operator surface wiki={} readiness={:?} queue_depth={} sections={}/{} session_sections={} {selected} {workbench} severity_counts=[{}]",
        operator_summary.wiki_id,
        operator_summary.readiness,
        operator_summary.queue_depth,
        available_sections,
        operator_summary.section_overview.len(),
        session_digest.sections.len(),
        severity_counts
    )
}

fn render_desktop_summary_value(
    operator_summary: &PatrolOperatorSummary,
    session_digest: &PatrolSessionDigest,
) -> serde_json::Value {
    serde_json::json!({
        "wiki_id": &operator_summary.wiki_id,
        "readiness": format!("{:?}", operator_summary.readiness),
        "queue_depth": operator_summary.queue_depth,
        "selected": operator_summary.selected.as_ref().map(|selected| serde_json::json!({
            "wiki_id": &operator_summary.wiki_id,
            "rev_id": selected.rev_id,
            "title": &selected.title,
            "score": selected.score,
            "signals": selected.signals,
        })),
        "severity_counts": operator_summary
            .severity_counts
            .iter()
            .map(|count| serde_json::json!({
                "severity": format!("{:?}", count.severity),
                "count": count.count,
            }))
            .collect::<Vec<_>>(),
        "available_sections": operator_summary
            .section_overview
            .iter()
            .filter(|section| section.available)
            .count(),
        "section_count": operator_summary.section_overview.len(),
        "session": serde_json::json!({
            "wiki_id": &session_digest.wiki_id,
            "readiness": format!("{:?}", session_digest.readiness),
            "queue_depth": session_digest.queue_depth,
            "findings": session_digest.findings.len(),
            "sections": session_digest.sections.len(),
        }),
    })
}

fn build_console_snapshot() -> Result<DesktopConsoleSnapshot, String> {
    let surface = block_on(build_default_dev_operator_surface(
        &DevOperatorSurfaceOptions {
            stream_event_id_prefix: "desktop-fixture".to_string(),
            context: Some(DevContextOptions::default()),
            workbench: Some(DevWorkbenchOptions {
                token: "desktop-local-token".to_string(),
                actor: "SP42-desktop".to_string(),
                note: Some("desktop shell".to_string()),
            }),
            action_note: Some("desktop shell".to_string()),
        },
    ))
    .map_err(|error| error.to_string())?;
    let transport_lines = render_dev_transport_lines(&surface);

    Ok(DesktopConsoleSnapshot {
        surface,
        transport_lines,
    })
}

fn render_markdown_section(title: &str, body: &str) -> String {
    if body.trim().is_empty() {
        format!("## {title}\n\n_Empty_")
    } else {
        format!("## {title}\n\n{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputFormat, parse_options, run_with_format};

    #[test]
    fn desktop_preview_renders_summary() {
        let summary = run_with_format(OutputFormat::Text).expect("desktop shell should render");

        assert!(summary.contains("SP42 native operator console"));
        assert!(summary.contains("desktop operator surface wiki=frwiki"));
        assert!(summary.contains("Patrol operator summary"));
        assert!(summary.contains("Patrol session digest"));
        assert!(summary.contains("Patrol report"));
        assert!(summary.contains("[Queue] available=true"));
        assert!(summary.contains("[Coordination] available=true"));
        assert!(summary.contains("Transport"));
    }

    #[test]
    fn desktop_parses_markdown_format_flag() {
        let options = parse_options(["--format".to_string(), "markdown".to_string()])
            .expect("format should parse");

        assert_eq!(options.format, OutputFormat::Markdown);
    }

    #[test]
    fn desktop_renders_markdown_report() {
        let summary =
            run_with_format(OutputFormat::Markdown).expect("markdown desktop shell should render");

        assert!(summary.contains("## Desktop summary"));
        assert!(summary.contains("# Patrol operator summary"));
        assert!(summary.contains("# Patrol session digest"));
        assert!(summary.contains("# Patrol report"));
        assert!(summary.contains("## Queue"));
        assert!(summary.contains("## Workbench"));
        assert!(summary.contains("## Backlog"));
        assert!(summary.contains("## Coordination"));
        assert!(summary.contains("## Stream"));
        assert!(summary.contains("## Transport"));
    }

    #[test]
    fn desktop_renders_json_report() {
        let summary =
            run_with_format(OutputFormat::Json).expect("json desktop shell should render");

        assert!(summary.contains("\"project\":"));
        assert!(summary.contains("\"desktop_summary\":"));
        assert!(summary.contains("\"operator_summary\":"));
        assert!(summary.contains("\"session_digest\":"));
        assert!(summary.contains("\"report\":"));
        assert!(summary.contains("\"transport_lines\":"));
    }
}
