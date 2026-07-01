use leptos::prelude::*;
use sp42_core::{DevAuthCapabilityReport, SessionActionKind};
use sp42_live::{LiveOperatorActionPreflight, LiveOperatorActionRecommendation};
use sp42_ui::{
    Button, ButtonProps, ButtonState, ButtonSurface, ButtonType, Density, Size, Spacer, Text,
    TextProps, Tone, Toolbar, ToolbarProps,
};

use super::ui_children;

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

    Toolbar(
        ToolbarProps::new(
            "Patrol actions",
            ui_children(move || {
                view! {
                    {Button(recommend_when(
                        ButtonProps::new("R Rollback")
                            .with_tone(Tone::Danger)
                            .with_type(ButtonType::Button)
                            .with_title(rollback_title)
                            .with_keyshortcuts("r")
                            .with_disabled(Signal::derive(move || {
                                !rollback_available || !has_selection.get() || action_pending.get()
                            }))
                            .on_click(move |_| on_action.set(Some(SessionActionKind::Rollback))),
                        recommended == Some(SessionActionKind::Rollback),
                    ))}

                    {Button(recommend_when(
                        ButtonProps::new("U Undo")
                            .with_type(ButtonType::Button)
                            .with_title(undo_title)
                            .with_keyshortcuts("u")
                            .with_disabled(Signal::derive(move || {
                                !undo_available || !has_selection.get() || action_pending.get()
                            }))
                            .on_click(move |_| on_action.set(Some(SessionActionKind::Undo))),
                        recommended == Some(SessionActionKind::Undo),
                    ))}

                    {Button(recommend_when(
                        ButtonProps::new("P Patrol")
                            .with_tone(Tone::Success)
                            .with_type(ButtonType::Button)
                            .with_title(patrol_title)
                            .with_keyshortcuts("p")
                            .with_disabled(Signal::derive(move || {
                                !patrol_available || !has_selection.get() || action_pending.get()
                            }))
                            .on_click(move |_| on_action.set(Some(SessionActionKind::Patrol))),
                        recommended == Some(SessionActionKind::Patrol),
                    ))}

                    {Button(
                        ButtonProps::new("S Skip")
                            .with_type(ButtonType::Button)
                            .with_surface(ButtonSurface::Ghost)
                            .with_keyshortcuts("s")
                            .with_disabled(Signal::derive(move || !has_selection.get()))
                            .on_click(move |_| on_skip.set(true)),
                    )}

                    {Spacer()}
                    {Text(
                        TextProps::new(ui_children(move || {
                            view! {
                                {move || {
                                    if action_pending.get() {
                                        "Executing...".to_string()
                                    } else {
                                        preflight_notes.clone()
                                    }
                                }}
                            }
                            .into_any()
                        }))
                        .with_tone(Tone::Muted)
                        .with_size(Size::XSmall),
                    )}
                }
                .into_any()
            }),
        )
        .with_density(Density::Compact),
    )
}

fn recommend_when(props: ButtonProps, recommended: bool) -> ButtonProps {
    if recommended {
        props.with_state(ButtonState::Recommended)
    } else {
        props
    }
}

fn find_recommendation(
    preflight: &LiveOperatorActionPreflight,
    kind: SessionActionKind,
) -> Option<LiveOperatorActionRecommendation> {
    preflight
        .recommendations
        .iter()
        .find(|r| r.kind == kind)
        .cloned()
}

fn tooltip_from_reasons(recommendation: Option<&LiveOperatorActionRecommendation>) -> String {
    recommendation
        .map(|r| r.reasons.join("; "))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use sp42_core::SessionActionKind;
    use sp42_live::{
        LiveOperatorActionPreflight, LiveOperatorActionRecommendation, LiveOperatorRetryClass,
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
