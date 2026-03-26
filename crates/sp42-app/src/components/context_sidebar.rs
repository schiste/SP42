use leptos::prelude::*;
use sp42_core::{
    CoordinationRoomSummary, CoordinationStateSummary, DevAuthCapabilityReport, EditorIdentity,
    LiveOperatorView, PatrolScenarioReadiness, PatrolScenarioReport, PatrolSessionDigest,
    QueuedEdit, ReportSeverity, ScoringContext,
};

use super::style::{SECTION_HEADER, score_tier, wiki_base_url};

#[component]
pub fn ContextSidebar(
    view: LiveOperatorView,
    edit: Option<QueuedEdit>,
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

    view! {
        <aside
            role="complementary"
            aria-label="Edit context"
            aria-live="polite"
            style="overflow-y:auto;min-height:0;padding:10px;display:grid;gap:17px;align-content:start;\
                   border-inline-start:1px solid rgba(148,163,184,.12);"
        >
            {score_section(&edit)}
            {user_section(&edit)}
            {metadata_section(&edit, &view.scoring_context)}
            {signals_section(&edit)}
            {capabilities_section(&view.capabilities)}
            {scenario_readiness_section(&view.scenario_report)}
            {session_digest_section(&view.session_digest)}
            {coordination_section(&edit, &view.coordination_room, &view.coordination_state)}
        </aside>
    }
    .into_any()
}

fn score_section(edit: &QueuedEdit) -> impl IntoView {
    let score = edit.score.total;
    let (tier_color, tier_icon) = score_tier(score);
    view! {
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
    }
}

fn user_section(edit: &QueuedEdit) -> impl IntoView {
    let user_label = user_display(&edit.event.performer);
    let user_type = user_type_label(&edit.event.performer);
    view! {
        <div style="display:grid;gap:4px;">
            <div style="font-size:13px;font-weight:700;color:#eff4ff;word-break:break-all;">
                {user_label}
            </div>
            <div style="font-size:11px;color:#8b9fc0;">
                {user_type}
            </div>
        </div>
    }
}

fn metadata_section(edit: &QueuedEdit, scoring_context: &Option<ScoringContext>) -> impl IntoView {
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
    let base = wiki_base_url(&edit.event.wiki_id);
    let article_url = format!(
        "{}/wiki/{}",
        base,
        edit.event.title.replace(' ', "_")
    );
    let diff_url = format!("{base}/w/index.php?diff={rev_id}&oldid={old_rev_id}");
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

    view! {
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
    }
}

fn signals_section(edit: &QueuedEdit) -> impl IntoView {
    view! {
        <div style="display:grid;gap:3px;">
            <div style=SECTION_HEADER>
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
    }
}

fn capabilities_section(capabilities: &DevAuthCapabilityReport) -> impl IntoView {
    let can_rollback = capabilities.capabilities.moderation.can_rollback;
    let can_patrol = capabilities.capabilities.moderation.can_patrol;
    let can_undo = capabilities.capabilities.editing.can_undo;
    view! {
        <div style="display:grid;gap:3px;font-size:11px;color:#8b9fc0;">
            <div style=SECTION_HEADER>
                "Capabilities"
            </div>
            <div>{format!("rollback: {}", if can_rollback { "yes" } else { "no" })}</div>
            <div>{format!("patrol: {}", if can_patrol { "yes" } else { "no" })}</div>
            <div>{format!("undo: {}", if can_undo { "yes" } else { "no" })}</div>
        </div>
    }
}

fn session_digest_section(digest: &PatrolSessionDigest) -> impl IntoView {
    if digest.explanation_lines.is_empty() {
        return view! { <span></span> }.into_any();
    }

    let headline = digest_headline(digest);

    view! {
        <details style="display:grid;gap:3px;">
            <summary style=format!("{SECTION_HEADER}cursor:pointer;")>
                "Session Digest"
            </summary>
            <p style="margin:0;font-size:12px;color:#eff4ff;">{headline}</p>
            <ul style="margin:0;padding-inline-start:17px;">
                {digest.explanation_lines.iter().map(|line| view! {
                    <li style="font-size:11px;color:#8b9fc0;line-height:1.4;">
                        {line.clone()}
                    </li>
                }).collect_view()}
            </ul>
        </details>
    }
    .into_any()
}

fn coordination_section(
    edit: &QueuedEdit,
    room: &Option<CoordinationRoomSummary>,
    state: &Option<CoordinationStateSummary>,
) -> impl IntoView {
    let Some(room) = room else {
        return view! { <span></span> }.into_any();
    };

    let operators = room.presence_count;
    let claims = room.claim_count;
    let recent_actions = room.recent_action_count;
    let claimed_by_other = is_claimed_by_other(edit.event.rev_id, state);

    view! {
        <div style="display:grid;gap:3px;">
            <div style=SECTION_HEADER>
                "Coordination"
            </div>
            <div style="font-size:12px;color:#8b9fc0;">
                {format!(
                    "{} operator(s) online, {} claimed, {} recent actions",
                    operators, claims, recent_actions,
                )}
            </div>
            {if claimed_by_other {
                view! {
                    <div style="font-size:11px;color:#f59e0b;font-weight:700;">
                        "This edit is claimed by another operator"
                    </div>
                }.into_any()
            } else {
                view! { <span></span> }.into_any()
            }}
        </div>
    }
    .into_any()
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
            <div style=SECTION_HEADER>
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

fn digest_headline(digest: &PatrolSessionDigest) -> String {
    digest
        .operator_summary
        .notes
        .first()
        .cloned()
        .unwrap_or_else(|| "Session digest".to_string())
}

fn is_claimed_by_other(rev_id: u64, state: &Option<CoordinationStateSummary>) -> bool {
    state
        .as_ref()
        .map(|s| s.claims.iter().any(|c| c.rev_id == rev_id))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use sp42_core::{
        CoordinationStateSummary, EditClaim, PatrolOperatorSummary, PatrolScenarioReadiness,
        PatrolSessionDigest,
    };

    use super::{digest_headline, is_claimed_by_other};

    fn empty_digest() -> PatrolSessionDigest {
        PatrolSessionDigest {
            wiki_id: "frwiki".to_string(),
            queue_depth: 0,
            readiness: PatrolScenarioReadiness::Ready,
            selected: None,
            findings: Vec::new(),
            severity_counts: Vec::new(),
            operator_summary: PatrolOperatorSummary {
                wiki_id: "frwiki".to_string(),
                readiness: PatrolScenarioReadiness::Ready,
                queue_depth: 0,
                selected: None,
                severity_counts: Vec::new(),
                section_overview: Vec::new(),
                workbench: None,
                notes: Vec::new(),
            },
            sections: Vec::new(),
            workbench: None,
            explanation_lines: Vec::new(),
        }
    }

    #[test]
    fn digest_headline_uses_first_operator_note() {
        let mut digest = empty_digest();
        digest.operator_summary.notes = vec!["inspect diff".to_string(), "extra".to_string()];

        assert_eq!(digest_headline(&digest), "inspect diff");
    }

    #[test]
    fn digest_headline_falls_back_when_no_notes() {
        let digest = empty_digest();

        assert_eq!(digest_headline(&digest), "Session digest");
    }

    #[test]
    fn is_claimed_by_other_detects_matching_rev_id() {
        let state = Some(CoordinationStateSummary {
            wiki_id: "frwiki".to_string(),
            claims: vec![EditClaim {
                wiki_id: "frwiki".to_string(),
                rev_id: 42,
                actor: "other-user".to_string(),
            }],
            presence: Vec::new(),
            flagged_edits: Vec::new(),
            score_deltas: Vec::new(),
            race_resolutions: Vec::new(),
            recent_actions: Vec::new(),
        });

        assert!(is_claimed_by_other(42, &state));
        assert!(!is_claimed_by_other(99, &state));
    }

    #[test]
    fn is_claimed_by_other_returns_false_without_state() {
        assert!(!is_claimed_by_other(42, &None));
    }
}
