use leptos::prelude::*;
use sp42_core::{
    DevAuthCapabilityReport, EditorIdentity, PatrolScenarioReadiness, PatrolScenarioReport,
    QueuedEdit, ReportSeverity, ScoringContext,
};

#[component]
pub fn ContextSidebar(
    edit: Option<QueuedEdit>,
    scoring_context: Option<ScoringContext>,
    capabilities: DevAuthCapabilityReport,
    scenario_report: PatrolScenarioReport,
) -> impl IntoView {
    let Some(edit) = edit else {
        return view! {
            <aside
                role="complementary"
                aria-label="Edit context"
                style="padding:10px;color:#8b9fc0;border-inline-start:1px solid rgba(148,163,184,.12);"
            >
                <p>"Select an edit to see context."</p>
            </aside>
        }
        .into_any();
    };

    let score = edit.score.total;
    let (tier_color, tier_icon) = score_tier(score);
    let user_label = user_display(&edit.event.performer);
    let user_type = user_type_label(&edit.event.performer);
    let byte_delta = edit.event.byte_delta;
    let delta_color = if byte_delta > 0 {
        "#22c55e"
    } else if byte_delta < 0 {
        "#ef4444"
    } else {
        "#8b9fc0"
    };
    let delta_str = if byte_delta > 0 {
        format!("+{byte_delta}")
    } else {
        format!("{byte_delta}")
    };

    let comment = edit
        .event
        .comment
        .clone()
        .unwrap_or_else(|| "(no edit summary)".to_string());

    let rev_id = edit.event.rev_id;
    let old_rev_id = edit.event.old_rev_id.unwrap_or(0);
    let article_url = format!(
        "https://fr.wikipedia.org/wiki/{}",
        edit.event.title.replace(' ', "_")
    );
    let diff_url = format!("https://fr.wikipedia.org/w/index.php?diff={rev_id}&oldid={old_rev_id}");

    let liftwing = scoring_context
        .as_ref()
        .and_then(|ctx| ctx.liftwing_risk)
        .map(|p| format!("{:.0}%", p * 100.0))
        .unwrap_or_else(|| "n/a".to_string());

    let warning = scoring_context
        .as_ref()
        .and_then(|ctx| ctx.user_risk.as_ref())
        .map(|risk| format!("{}", risk.warning_level))
        .unwrap_or_else(|| "none".to_string());

    let can_rollback = capabilities.capabilities.moderation.can_rollback;
    let can_patrol = capabilities.capabilities.moderation.can_patrol;
    let can_undo = capabilities.capabilities.editing.can_undo;

    view! {
        <aside
            role="complementary"
            aria-label="Edit context"
            aria-live="polite"
            style="overflow-y:auto;min-height:0;padding:10px;display:grid;gap:17px;align-content:start;\
                   border-inline-start:1px solid rgba(148,163,184,.12);"
        >
            // Score (largest element per Rule 6.1)
            <div style="text-align:center;">
                <div style=format!(
                    "font-size:44px;font-weight:700;line-height:1;color:{tier_color};",
                )>
                    {format!("{score}")}
                </div>
                <div style="font-size:13px;color:#8b9fc0;margin-top:4px;">
                    {tier_icon} " risk score"
                </div>
            </div>

            // User identity
            <div style="display:grid;gap:4px;">
                <div style="font-size:13px;font-weight:700;color:#eff4ff;word-break:break-all;">
                    {user_label}
                </div>
                <div style="font-size:11px;color:#8b9fc0;">
                    {user_type}
                </div>
            </div>

            // Edit metadata
            <div style="display:grid;gap:4px;font-size:12px;color:#8b9fc0;">
                <div>
                    "Bytes: "
                    <span style=format!("color:{delta_color};font-weight:700;")>
                        {delta_str}
                    </span>
                </div>
                <div style="font-style:italic;overflow:hidden;text-overflow:ellipsis;">
                    {comment}
                </div>
                <div>
                    {format!("rev {rev_id}")}
                </div>
                <div style="display:flex;gap:10px;">
                    <a
                        href=article_url
                        target="_blank"
                        rel="noopener"
                        style="color:#3b82f6;font-size:12px;text-decoration:none;"
                    >
                        "View on wiki"
                    </a>
                    <a
                        href=diff_url
                        target="_blank"
                        rel="noopener"
                        style="color:#3b82f6;font-size:12px;text-decoration:none;"
                    >
                        "View diff on wiki"
                    </a>
                </div>
                <div>"LiftWing: " {liftwing}</div>
                <div>"Warning: " {warning}</div>
            </div>

            // Signal breakdown
            <div style="display:grid;gap:3px;">
                <div style="font-size:11px;font-weight:700;color:#8b9fc0;text-transform:uppercase;letter-spacing:.1em;">
                    "Signals"
                </div>
                {edit
                    .score
                    .contributions
                    .iter()
                    .map(|sig| {
                        let color = if sig.weight > 0 { "#ef4444" } else { "#22c55e" };
                        let sign = if sig.weight > 0 { "+" } else { "" };
                        view! {
                            <div style="display:flex;justify-content:space-between;font-size:12px;">
                                <span style="color:#8b9fc0;">
                                    {format!("{}", sig.signal)}
                                </span>
                                <span style=format!("color:{color};font-weight:700;")>
                                    {format!("{sign}{}", sig.weight)}
                                </span>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>

            // Capabilities
            <div style="display:grid;gap:3px;font-size:11px;color:#8b9fc0;">
                <div style="font-weight:700;text-transform:uppercase;letter-spacing:.1em;">
                    "Capabilities"
                </div>
                <div>{format!("rollback: {}", if can_rollback { "yes" } else { "no" })}</div>
                <div>{format!("patrol: {}", if can_patrol { "yes" } else { "no" })}</div>
                <div>{format!("undo: {}", if can_undo { "yes" } else { "no" })}</div>
            </div>

            // Scenario readiness & findings
            {scenario_readiness_section(&scenario_report)}
        </aside>
    }
    .into_any()
}

fn score_tier(score: i32) -> (&'static str, &'static str) {
    if score >= 70 {
        ("#ef4444", "!!")
    } else if score >= 30 {
        ("#f59e0b", "?")
    } else {
        ("#22c55e", "\u{2713}")
    }
}

fn user_display(performer: &EditorIdentity) -> String {
    match performer {
        EditorIdentity::Registered { username } => username.clone(),
        EditorIdentity::Anonymous { label } => label.clone(),
        EditorIdentity::Temporary { label } => format!("{label} (temp)"),
    }
}

fn user_type_label(performer: &EditorIdentity) -> &'static str {
    match performer {
        EditorIdentity::Registered { .. } => "Registered user",
        EditorIdentity::Anonymous { .. } => "Anonymous (IP)",
        EditorIdentity::Temporary { .. } => "Temporary account",
    }
}

fn scenario_readiness_section(report: &PatrolScenarioReport) -> impl IntoView {
    let (readiness_color, readiness_label) = match report.readiness {
        PatrolScenarioReadiness::Ready => ("#22c55e", "Ready"),
        PatrolScenarioReadiness::Limited => ("#f59e0b", "Limited"),
        PatrolScenarioReadiness::Blocked => ("#ef4444", "Blocked"),
    };

    let blockers = report
        .findings
        .iter()
        .filter(|f| f.severity == ReportSeverity::Blocker)
        .count();
    let warnings = report
        .findings
        .iter()
        .filter(|f| f.severity == ReportSeverity::Warning)
        .count();

    let findings_view: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.severity != ReportSeverity::Info)
        .map(|f| {
            let color = match f.severity {
                ReportSeverity::Blocker => "#ef4444",
                ReportSeverity::Warning => "#f59e0b",
                ReportSeverity::Info => "#8b9fc0",
            };
            view! {
                <div style=format!("font-size:11px;color:{color};")>
                    {format!("{}: {}", f.code, f.message)}
                </div>
            }
        })
        .collect();

    view! {
        <div style="display:grid;gap:3px;">
            <div style="font-size:11px;font-weight:700;color:#8b9fc0;text-transform:uppercase;letter-spacing:.1em;">
                "Scenario"
            </div>
            <div style="display:flex;align-items:center;gap:7px;">
                <span
                    style=format!(
                        "width:10px;height:10px;border-radius:4px;background:{readiness_color};display:inline-block;",
                    )
                ></span>
                <span style=format!("font-size:12px;color:{readiness_color};font-weight:700;")>
                    {readiness_label}
                </span>
                <span style="font-size:11px;color:#8b9fc0;">
                    {format!(
                        "{} sections available",
                        report.sections.iter().filter(|s| s.available).count(),
                    )}
                </span>
            </div>
            {if blockers > 0 || warnings > 0 {
                view! {
                    <div style="font-size:11px;color:#8b9fc0;">
                        {format!(
                            "{} blocker(s), {} warning(s)",
                            blockers,
                            warnings,
                        )}
                    </div>
                }
                    .into_any()
            } else {
                view! { <span></span> }.into_any()
            }}
            {findings_view.into_iter().collect_view()}
        </div>
    }
}
