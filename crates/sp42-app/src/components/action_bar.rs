use leptos::prelude::*;
use sp42_core::{DevAuthCapabilityReport, LiveOperatorActionPreflight, SessionActionKind};

#[component]
pub fn ActionBar(
    preflight: LiveOperatorActionPreflight,
    capabilities: DevAuthCapabilityReport,
    has_selection: Signal<bool>,
    action_pending: Signal<bool>,
    on_action: WriteSignal<Option<SessionActionKind>>,
    on_skip: WriteSignal<bool>,
) -> impl IntoView {
    let recommended = preflight.recommended_kind;

    let rollback = find_recommendation(&preflight, SessionActionKind::Rollback);
    let patrol = find_recommendation(&preflight, SessionActionKind::Patrol);
    let undo = find_recommendation(&preflight, SessionActionKind::Undo);

    // Use preflight availability when present, fall back to raw capabilities
    let rollback_available = rollback
        .as_ref()
        .map_or(capabilities.capabilities.moderation.can_rollback, |r| {
            r.available
        });
    let patrol_available = patrol
        .as_ref()
        .map_or(capabilities.capabilities.moderation.can_patrol, |r| {
            r.available
        });
    let undo_available = undo
        .as_ref()
        .map_or(capabilities.capabilities.editing.can_undo, |r| r.available);

    let rollback_title = tooltip_from_reasons(rollback.as_ref());
    let patrol_title = tooltip_from_reasons(patrol.as_ref());
    let undo_title = tooltip_from_reasons(undo.as_ref());

    let preflight_notes = preflight.notes.join(" ");

    let rollback_class = if recommended == Some(SessionActionKind::Rollback) {
        "btn btn-danger btn-recommended"
    } else {
        "btn btn-danger"
    };
    let undo_class = if recommended == Some(SessionActionKind::Undo) {
        "btn btn-recommended"
    } else {
        "btn"
    };
    let patrol_class = if recommended == Some(SessionActionKind::Patrol) {
        "btn btn-success btn-recommended"
    } else {
        "btn btn-success"
    };

    view! {
        <div role="toolbar" aria-label="Patrol actions" class="action-bar">
            <button
                class=rollback_class
                title=rollback_title
                aria-keyshortcuts="r"
                disabled=move || !rollback_available || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Rollback))
            >
                "R Rollback"
            </button>

            <button
                class=undo_class
                title=undo_title
                aria-keyshortcuts="u"
                disabled=move || !undo_available || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Undo))
            >
                "U Undo"
            </button>

            <button
                class=patrol_class
                title=patrol_title
                aria-keyshortcuts="p"
                disabled=move || !patrol_available || !has_selection.get() || action_pending.get()
                on:click=move |_| on_action.set(Some(SessionActionKind::Patrol))
            >
                "P Patrol"
            </button>

            <button
                class="btn btn-ghost"
                aria-keyshortcuts="s"
                disabled=move || !has_selection.get()
                on:click=move |_| on_skip.set(true)
            >
                "S Skip"
            </button>

            <div class="flex-spacer"></div>
            <div class="text-muted" style="font-size:11px;">
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
