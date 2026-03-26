use leptos::prelude::*;
use sp42_core::{LiveOperatorActionPreflight, SessionActionKind};

#[component]
pub fn ActionBar(
    preflight: LiveOperatorActionPreflight,
    has_selection: Signal<bool>,
    action_pending: Signal<bool>,
    on_action: WriteSignal<Option<SessionActionKind>>,
    on_skip: WriteSignal<bool>,
) -> impl IntoView {
    let recommended = preflight.recommended_kind;

    let rollback = find_recommendation(&preflight, SessionActionKind::Rollback);
    let patrol = find_recommendation(&preflight, SessionActionKind::Patrol);
    let undo = find_recommendation(&preflight, SessionActionKind::Undo);

    let rollback_available = rollback.as_ref().is_some_and(|r| r.available);
    let patrol_available = patrol.as_ref().is_some_and(|r| r.available);
    let undo_available = undo.as_ref().is_some_and(|r| r.available);

    let rollback_title = tooltip_from_reasons(rollback.as_ref());
    let patrol_title = tooltip_from_reasons(patrol.as_ref());
    let undo_title = tooltip_from_reasons(undo.as_ref());

    let preflight_notes = preflight.notes.join(" ");

    let btn_base = "min-height:44px;padding:4px 17px;border:1px solid rgba(148,163,184,.18);\
                    border-radius:4px;font:inherit;font-size:13px;font-weight:700;\
                    cursor:pointer;transition:opacity 120ms;";

    let ring = "box-shadow:0 0 0 2px rgba(143,183,255,.5);";

    view! {
        <div
            role="toolbar"
            aria-label="Patrol actions"
            style="display:flex;align-items:center;gap:7px;padding:0 10px;\
                   background:#0b1324;border-block-start:1px solid rgba(148,163,184,.18);"
        >
            // Rollback — destructive, filled red-tinted
            <button
                style=format!(
                    "{btn_base}background:rgba(239,68,68,.18);color:#fecaca;border-color:rgba(239,68,68,.3);{}",
                    if recommended == Some(SessionActionKind::Rollback) { ring } else { "" },
                )
                title=rollback_title
                aria-keyshortcuts="r"
                disabled=move || !rollback_available || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Rollback))
            >
                "R Rollback"
            </button>

            // Undo — neutral outlined
            <button
                style=format!(
                    "{btn_base}background:transparent;color:#eff4ff;{}",
                    if recommended == Some(SessionActionKind::Undo) { ring } else { "" },
                )
                title=undo_title
                aria-keyshortcuts="u"
                disabled=move || !undo_available || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Undo))
            >
                "U Undo"
            </button>

            // Patrol — green-tinted
            <button
                style=format!(
                    "{btn_base}background:rgba(34,197,94,.14);color:#bbf7d0;border-color:rgba(34,197,94,.3);{}",
                    if recommended == Some(SessionActionKind::Patrol) { ring } else { "" },
                )
                title=patrol_title
                aria-keyshortcuts="p"
                disabled=move || !patrol_available || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Patrol))
            >
                "P Patrol"
            </button>

            // Skip — muted outlined
            <button
                style=format!(
                    "{btn_base}background:transparent;color:#8b9fc0;",
                )
                aria-keyshortcuts="s"
                disabled=move || !has_selection.get()
                on:click=move |_| on_skip.set(true)
            >
                "S Skip"
            </button>

            // Spacer
            <div style="flex:1;"></div>

            // Status area (right side)
            <div style="font-size:11px;color:#8b9fc0;">
                {move || {
                    if action_pending.get() {
                        "Executing...".to_string()
                    } else {
                        preflight_notes.clone()
                    }
                }}
            </div>
        </div>
    }
}

fn find_recommendation(
    preflight: &LiveOperatorActionPreflight,
    kind: SessionActionKind,
) -> Option<sp42_core::LiveOperatorActionRecommendation> {
    preflight
        .recommendations
        .iter()
        .find(|r| r.kind == kind)
        .cloned()
}

fn tooltip_from_reasons(
    recommendation: Option<&sp42_core::LiveOperatorActionRecommendation>,
) -> String {
    recommendation
        .map(|r| r.reasons.join("; "))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use sp42_core::{
        LiveOperatorActionPreflight, LiveOperatorActionRecommendation, LiveOperatorRetryClass,
        SessionActionKind,
    };

    use super::{find_recommendation, tooltip_from_reasons};

    fn test_preflight() -> LiveOperatorActionPreflight {
        LiveOperatorActionPreflight {
            selected_rev_id: Some(123),
            recommended_kind: Some(SessionActionKind::Patrol),
            recommendations: vec![
                LiveOperatorActionRecommendation {
                    kind: SessionActionKind::Rollback,
                    request: None,
                    available: false,
                    recommended: false,
                    retry_class: LiveOperatorRetryClass::Never,
                    reasons: vec!["missing token".to_string()],
                },
                LiveOperatorActionRecommendation {
                    kind: SessionActionKind::Patrol,
                    request: None,
                    available: true,
                    recommended: true,
                    retry_class: LiveOperatorRetryClass::NotNeeded,
                    reasons: vec![
                        "edit is unpatrolled".to_string(),
                        "user can patrol".to_string(),
                    ],
                },
            ],
            notes: vec!["patrol recommended".to_string()],
        }
    }

    #[test]
    fn find_recommendation_returns_matching_kind() {
        let preflight = test_preflight();

        let patrol = find_recommendation(&preflight, SessionActionKind::Patrol);
        assert!(patrol.is_some());
        assert!(patrol.expect("should exist").available);
    }

    #[test]
    fn find_recommendation_returns_none_for_missing_kind() {
        let preflight = test_preflight();

        let undo = find_recommendation(&preflight, SessionActionKind::Undo);
        assert!(undo.is_none());
    }

    #[test]
    fn tooltip_from_reasons_joins_reasons() {
        let preflight = test_preflight();
        let patrol = find_recommendation(&preflight, SessionActionKind::Patrol);

        let tooltip = tooltip_from_reasons(patrol.as_ref());
        assert_eq!(tooltip, "edit is unpatrolled; user can patrol");
    }

    #[test]
    fn tooltip_from_reasons_returns_empty_for_none() {
        let tooltip = tooltip_from_reasons(None);
        assert!(tooltip.is_empty());
    }
}
