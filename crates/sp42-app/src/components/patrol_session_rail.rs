use leptos::prelude::*;
use sp42_core::{
    DevAuthActionTokenAvailability, DevAuthCapabilityReport, DevAuthProbeAcceptance, QueuedEdit,
    ReviewWorkbench, SessionActionExecutionRequest, SessionActionKind, build_review_workbench,
    build_session_action_execution_requests,
};

use crate::platform::auth::execute_dev_auth_action;

use super::{ActionHistoryPanel, StatusBadge, StatusTone, TelemetryPanel};

const REQUEST_PREVIEW_TOKEN_PLACEHOLDER: &str = "<bridge-token-redacted>";

#[component]
pub fn PatrolSessionRail(config: sp42_core::WikiConfig, queue: Vec<QueuedEdit>) -> impl IntoView {
    let queue_len = queue.len();
    let initial_index = (!queue.is_empty()).then_some(0usize);
    let (selected_index, set_selected_index) = signal(initial_index);
    let (note, set_note) = signal("inspect selected edit".to_string());
    let (capability_report, set_capability_report) = signal(None::<DevAuthCapabilityReport>);
    let (execution_status, set_execution_status) = signal("No action executed yet.".to_string());
    let (action_history_refresh_tick, set_action_history_refresh_tick) = signal(0_u64);

    let preview_queue = queue.clone();
    let action_queue = queue.clone();
    let rollback_queue = queue.clone();
    let patrol_queue = queue.clone();
    let undo_queue = queue.clone();
    let badge_queue = queue.clone();
    let wiki_id = config.wiki_id.clone();

    let capability_action = Action::new_local(move |_: &()| {
        let set_capability_report = set_capability_report;
        let wiki_id = wiki_id.clone();
        async move {
            let report = match crate::platform::debug::fetch_dev_auth_capabilities(&wiki_id).await {
                Ok(report) => report,
                Err(error) => DevAuthCapabilityReport {
                    checked: true,
                    wiki_id,
                    error: Some(error),
                    acceptance: DevAuthProbeAcceptance {
                        profile_accepted: false,
                        userinfo_accepted: false,
                    },
                    token_availability: DevAuthActionTokenAvailability {
                        csrf_token_available: false,
                        patrol_token_available: false,
                        rollback_token_available: false,
                    },
                    ..DevAuthCapabilityReport::default()
                },
            };
            set_capability_report.set(Some(report));
        }
    });

    let execute_action = Action::new_local(move |request: &SessionActionExecutionRequest| {
        let request = request.clone();
        let set_execution_status = set_execution_status;
        let set_action_history_refresh_tick = set_action_history_refresh_tick;
        async move {
            let message = match execute_dev_auth_action(&request).await {
                Ok(response) => format_session_action_response(&response),
                Err(error) => format!("Execution error: {error}"),
            };
            set_execution_status.set(message);
            set_action_history_refresh_tick.update(|tick| *tick = tick.saturating_add(1));
        }
    });

    let narrative_lines = Memo::new(move |_| {
        session_story_lines(
            &config,
            &preview_queue,
            selected_index.get(),
            &note.get(),
            capability_report.get().as_ref(),
        )
    });

    let execution_lines = Memo::new(move |_| {
        session_execution_lines(
            &action_queue,
            selected_index.get(),
            &note.get(),
            capability_report.get().as_ref(),
            execution_status.get(),
        )
    });

    let status_badges = move || {
        session_badges(
            queue_len,
            selected_index.get(),
            capability_report.get().as_ref(),
            execution_status.get(),
        )
    };

    Effect::new(move |_| {
        capability_action.dispatch_local(());
    });

    view! {
        <section style="display:grid;gap:17px;">
            <header style="display:grid;gap:7px;">
                <div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;">
                    <StatusBadge label="Patrol Rail".to_string() tone=StatusTone::Accent />
                    {status_badges()
                        .into_iter()
                        .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                        .collect_view()}
                </div>
                <p style="margin:0;color:#8b9fc0;line-height:1.6;">
                    "Choose the live edit, frame the note, check capabilities, and send the action from one rail."
                </p>
            </header>

            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:10px;">
                <article
                    style="display:grid;gap:10px;padding:17px;border-radius:4px;border:1px solid rgba(148,163,184,.16);background:rgba(8,15,29,.52);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Selected Edit"</h3>
                        <div style="display:flex;gap:4px;flex-wrap:wrap;">
                            <button
                                style="min-height:44px"
                                aria-label="Previous edit"
                                on:click=move |_| shift_selection(&set_selected_index, selected_index.get(), queue_len, -1)
                                disabled=move || !can_shift(selected_index.get(), queue_len, -1)
                            >
                                "Previous"
                            </button>
                            <button
                                style="min-height:44px"
                                aria-label="Next edit"
                                on:click=move |_| shift_selection(&set_selected_index, selected_index.get(), queue_len, 1)
                                disabled=move || !can_shift(selected_index.get(), queue_len, 1)
                            >
                                "Next"
                            </button>
                        </div>
                    </div>
                    <div style="display:grid;gap:7px;grid-template-columns:repeat(auto-fit,minmax(120px,1fr));">
                        {queue
                            .iter()
                            .enumerate()
                            .map(|(index, item)| {
                                view! {
                                    <button
                                        on:click=move |_| set_selected_index.set(Some(index))
                                        aria-pressed=move || selected_index.get() == Some(index)
                                        style=move || {
                                            if selected_index.get() == Some(index) {
                                                "text-align:start;border:1px solid rgba(143,183,255,.5);background:rgba(143,183,255,.14);"
                                            } else {
                                                "text-align:start;"
                                            }
                                        }
                                    >
                                        {format!("#{} {} • {}", index + 1, item.event.rev_id, item.event.title)}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>
                    <label style="display:grid;gap:4px;">
                        <span>"Action Note"</span>
                        <input
                            type="text"
                            prop:value=move || note.get()
                            on:input=move |ev| set_note.set(event_target_value(&ev))
                        />
                    </label>
                    <div style="display:flex;gap:7px;flex-wrap:wrap;">
                        {move || {
                            selected_item_badges(&badge_queue, selected_index.get())
                                .into_iter()
                                .map(|(label, tone)| view! { <StatusBadge label=label tone=tone /> })
                                .collect_view()
                        }}
                    </div>
                </article>

                <article
                    aria-live="polite"
                    aria-label="Action controls and execution status"
                    style="display:grid;gap:10px;padding:17px;border-radius:4px;border:1px solid rgba(148,163,184,.16);background:rgba(8,15,29,.52);"
                >
                    <div style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;">
                        <h3 style="margin:0;font-size:1rem;">"Action Rail"</h3>
                        <div role="toolbar" aria-label="Patrol actions" style="display:flex;gap:4px;flex-wrap:wrap;">
                            <button
                                style="min-height:44px"
                                on:click=move |_| {
                                    capability_action.dispatch_local(());
                                }
                                disabled=move || capability_action.pending().get()
                            >
                                "Refresh Access"
                            </button>
                            <button
                                style="min-height:44px"
                                aria-keyshortcuts="r"
                                on:click=move |_| execute_selected_action(&rollback_queue, selected_index.get(), &note.get(), SessionActionKind::Rollback, &execute_action)
                                disabled=move || action_disabled(capability_report.get().as_ref(), selected_index.get(), queue_len, SessionActionKind::Rollback, execute_action.pending().get())
                            >
                                "Rollback"
                            </button>
                            <button
                                style="min-height:44px"
                                aria-keyshortcuts="p"
                                on:click=move |_| execute_selected_action(&patrol_queue, selected_index.get(), &note.get(), SessionActionKind::Patrol, &execute_action)
                                disabled=move || action_disabled(capability_report.get().as_ref(), selected_index.get(), queue_len, SessionActionKind::Patrol, execute_action.pending().get())
                            >
                                "Patrol"
                            </button>
                            <button
                                style="min-height:44px"
                                aria-keyshortcuts="u"
                                on:click=move |_| execute_selected_action(&undo_queue, selected_index.get(), &note.get(), SessionActionKind::Undo, &execute_action)
                                disabled=move || action_disabled(capability_report.get().as_ref(), selected_index.get(), queue_len, SessionActionKind::Undo, execute_action.pending().get())
                            >
                                "Undo"
                            </button>
                        </div>
                    </div>

                    <TelemetryPanel
                        title="Action Intent".to_string()
                        description="Queue selection, patrol intent, and session-action request details.".to_string()
                        tone=StatusTone::Info
                        badges=session_badges(queue_len, selected_index.get(), capability_report.get().as_ref(), execution_status.get())
                        lines=narrative_lines.get()
                    />

                    <TelemetryPanel
                        title="Live Result".to_string()
                        description="Live response from the localhost bridge, with capability-aware action gating and pending-state feedback.".to_string()
                        tone=StatusTone::Accent
                        badges=execution_badges(capability_report.get().as_ref(), execution_status.get(), execute_action.pending().get())
                        lines=execution_lines.get()
                    />

                    <ActionHistoryPanel refresh_tick=action_history_refresh_tick />
                </article>
            </div>
        </section>
    }
}

#[must_use]
pub fn session_badges(
    queue_len: usize,
    selected_index: Option<usize>,
    capability_report: Option<&DevAuthCapabilityReport>,
    execution_status: String,
) -> Vec<(String, StatusTone)> {
    let mut badges = vec![
        (
            format!("{} queue", queue_len),
            if queue_len == 0 {
                StatusTone::Warning
            } else {
                StatusTone::Success
            },
        ),
        (
            selected_index
                .map(|index| format!("selected #{}", index + 1))
                .unwrap_or_else(|| "no selection".to_string()),
            if selected_index.is_some() {
                StatusTone::Info
            } else {
                StatusTone::Warning
            },
        ),
    ];

    badges.push((
        if capability_report.is_some() {
            "capabilities loaded".to_string()
        } else {
            "capabilities pending".to_string()
        },
        if capability_report.is_some() {
            StatusTone::Success
        } else {
            StatusTone::Warning
        },
    ));

    badges.push((
        if execution_status.starts_with("Execution error") {
            "execution error".to_string()
        } else if execution_status.starts_with("No action executed yet") {
            "idle".to_string()
        } else {
            "execution updated".to_string()
        },
        if execution_status.starts_with("Execution error") {
            StatusTone::Warning
        } else if execution_status.starts_with("No action executed yet") {
            StatusTone::Neutral
        } else {
            StatusTone::Accent
        },
    ));

    badges
}

#[must_use]
pub fn session_story_lines(
    config: &sp42_core::WikiConfig,
    queue: &[QueuedEdit],
    selected_index: Option<usize>,
    note: &str,
    capability_report: Option<&DevAuthCapabilityReport>,
) -> Vec<String> {
    let Some(item) = selected_queue_item(queue, selected_index) else {
        return vec!["No queue item selected.".to_string()];
    };

    let actor = capability_report
        .and_then(|report| report.username.clone())
        .unwrap_or_else(|| "SP42-session".to_string());

    let mut lines = vec![
        format!(
            "wiki={} rev={} title=\"{}\"",
            config.wiki_id, item.event.rev_id, item.event.title
        ),
        format!(
            "score={} signals={}",
            item.score.total,
            item.score.contributions.len()
        ),
    ];

    match build_review_workbench(
        config,
        item,
        REQUEST_PREVIEW_TOKEN_PLACEHOLDER,
        &actor,
        Some(note),
    ) {
        Ok(workbench) => {
            lines.extend(review_workbench_lines(&workbench, capability_report));
        }
        Err(error) => {
            lines.push(format!("action rail error: {error}"));
        }
    }

    lines
}

#[must_use]
pub fn session_execution_lines(
    queue: &[QueuedEdit],
    selected_index: Option<usize>,
    note: &str,
    capability_report: Option<&DevAuthCapabilityReport>,
    execution_status: String,
) -> Vec<String> {
    let Some(item) = selected_queue_item(queue, selected_index) else {
        return vec![execution_status];
    };

    let mut lines = vec![
        format!(
            "selected rev={} title=\"{}\"",
            item.event.rev_id, item.event.title
        ),
        execution_status,
    ];

    if let Ok(requests) = build_session_action_execution_requests(item, Some(note)) {
        for request in requests {
            let allowed = is_action_allowed(capability_report, request.kind);
            lines.push(format!(
                "{:?} allowed={} {} {}",
                request.kind, allowed, request.wiki_id, request.rev_id
            ));
        }
    } else {
        lines.push("session action request generation failed.".to_string());
    }

    lines
}

#[must_use]
pub fn execution_badges(
    capability_report: Option<&DevAuthCapabilityReport>,
    execution_status: String,
    pending: bool,
) -> Vec<(String, StatusTone)> {
    vec![
        (
            if pending {
                "pending".to_string()
            } else {
                "ready".to_string()
            },
            if pending {
                StatusTone::Info
            } else {
                StatusTone::Success
            },
        ),
        (
            if capability_report.is_some() {
                "bridge checked".to_string()
            } else {
                "bridge idle".to_string()
            },
            if capability_report.is_some() {
                StatusTone::Success
            } else {
                StatusTone::Warning
            },
        ),
        (
            if execution_status.starts_with("Execution error") {
                "failed".to_string()
            } else if execution_status.starts_with("No action executed yet") {
                "waiting".to_string()
            } else {
                "updated".to_string()
            },
            if execution_status.starts_with("Execution error") {
                StatusTone::Warning
            } else if execution_status.starts_with("No action executed yet") {
                StatusTone::Neutral
            } else {
                StatusTone::Accent
            },
        ),
    ]
}

fn review_workbench_lines(
    workbench: &ReviewWorkbench,
    capability_report: Option<&DevAuthCapabilityReport>,
) -> Vec<String> {
    let mut lines = vec![
        format!(
            "action rev={} title=\"{}\"",
            workbench.rev_id, workbench.title
        ),
        format!(
            "request_preview_rows={}",
            workbench.training_jsonl.lines().count()
        ),
        format!(
            "request_table_rows={}",
            workbench.training_csv.lines().skip(1).count()
        ),
    ];

    for request in &workbench.requests {
        let allowed = match request.label.as_str() {
            "rollback" => {
                capability_report.is_none_or(|value| value.capabilities.moderation.can_rollback)
            }
            "patrol" => {
                capability_report.is_none_or(|value| value.capabilities.moderation.can_patrol)
            }
            "undo" => capability_report.is_none_or(|value| value.capabilities.editing.can_undo),
            _ => true,
        };

        lines.push(format!(
            "{} allowed={} {:?} {}",
            request.label, allowed, request.method, request.url
        ));
    }

    lines
}

fn is_action_allowed(
    capability_report: Option<&DevAuthCapabilityReport>,
    kind: SessionActionKind,
) -> bool {
    capability_report.is_none_or(|report| match kind {
        SessionActionKind::Rollback => report.capabilities.moderation.can_rollback,
        SessionActionKind::Patrol => report.capabilities.moderation.can_patrol,
        SessionActionKind::Undo => report.capabilities.editing.can_undo,
    })
}

fn action_disabled(
    capability_report: Option<&DevAuthCapabilityReport>,
    selected_index: Option<usize>,
    queue_len: usize,
    kind: SessionActionKind,
    pending: bool,
) -> bool {
    pending
        || selected_index.is_none()
        || queue_len == 0
        || !is_action_allowed(capability_report, kind)
}

fn execute_selected_action<O: 'static>(
    queue: &[QueuedEdit],
    selected_index: Option<usize>,
    note: &str,
    kind: SessionActionKind,
    execute_action: &Action<SessionActionExecutionRequest, O>,
) {
    let Some(item) = selected_queue_item(queue, selected_index) else {
        return;
    };

    if let Some(request) = session_action_request_for_kind(item, Some(note), kind) {
        execute_action.dispatch_local(request);
    }
}

fn session_action_request_for_kind(
    item: &QueuedEdit,
    note: Option<&str>,
    kind: SessionActionKind,
) -> Option<SessionActionExecutionRequest> {
    build_session_action_execution_requests(item, note)
        .ok()?
        .into_iter()
        .find(|request| request.kind == kind)
}

fn selected_queue_item(queue: &[QueuedEdit], selected_index: Option<usize>) -> Option<&QueuedEdit> {
    selected_index.and_then(|index| queue.get(index))
}

fn format_session_action_response(response: &sp42_core::SessionActionExecutionResponse) -> String {
    format!(
        "{:?} accepted={} actor={} rev={} message={}",
        response.kind,
        response.accepted,
        response.actor.as_deref().unwrap_or("unknown"),
        response.rev_id,
        response.message.as_deref().unwrap_or("none")
    )
}

fn can_shift(selected_index: Option<usize>, queue_len: usize, delta: isize) -> bool {
    let Some(index) = selected_index else {
        return false;
    };
    match delta.cmp(&0) {
        std::cmp::Ordering::Less => index > 0,
        std::cmp::Ordering::Equal => false,
        std::cmp::Ordering::Greater => index + 1 < queue_len,
    }
}

fn shift_selection(
    set_selected_index: &WriteSignal<Option<usize>>,
    selected_index: Option<usize>,
    queue_len: usize,
    delta: isize,
) {
    let Some(index) = selected_index else {
        return;
    };
    let next = match delta.cmp(&0) {
        std::cmp::Ordering::Less => index.saturating_sub(1),
        std::cmp::Ordering::Equal => index,
        std::cmp::Ordering::Greater => (index + 1).min(queue_len.saturating_sub(1)),
    };
    set_selected_index.set(Some(next));
}

fn selected_item_badges(
    queue: &[QueuedEdit],
    selected_index: Option<usize>,
) -> Vec<(String, StatusTone)> {
    let Some(item) = selected_queue_item(queue, selected_index) else {
        return vec![("No selection".to_string(), StatusTone::Warning)];
    };

    vec![
        (format!("rev {}", item.event.rev_id), StatusTone::Info),
        (
            format!("score {}", item.score.total),
            score_tone(item.score.total),
        ),
        (
            format!("signals {}", item.score.contributions.len()),
            StatusTone::Neutral,
        ),
    ]
}

fn score_tone(score: i32) -> StatusTone {
    if score >= 80 {
        StatusTone::Warning
    } else if score >= 40 {
        StatusTone::Accent
    } else {
        StatusTone::Success
    }
}

#[cfg(test)]
mod tests {
    use super::{
        action_disabled, can_shift, execution_badges, score_tone, selected_item_badges,
        session_action_request_for_kind, session_badges, session_execution_lines,
        session_story_lines, shift_selection,
    };
    use sp42_core::{
        EditEvent, EditorIdentity, QueuedEdit, ScoringConfig, SessionActionKind, parse_wiki_config,
        score_edit,
    };

    fn sample_queue() -> Vec<QueuedEdit> {
        let event = EditEvent {
            wiki_id: "frwiki".to_string(),
            title: "Example".to_string(),
            namespace: 0,
            rev_id: 123_456,
            old_rev_id: Some(123_455),
            performer: EditorIdentity::Registered {
                username: "Tester".to_string(),
            },
            timestamp_ms: 1_710_000_000_000,
            is_bot: false.into(),
            is_minor: false.into(),
            is_new_page: false.into(),
            tags: vec![],
            comment: Some("example".to_string()),
            byte_delta: 42,
            is_patrolled: false.into(),
        };
        let score = score_edit(&event, &ScoringConfig::default()).expect("score should compute");

        vec![QueuedEdit { event, score }]
    }

    #[test]
    fn badges_reflect_queue_and_execution_state() {
        let badges = session_badges(1, Some(0), None, "No action executed yet.".to_string());
        assert!(badges.iter().any(|(label, _)| label == "1 queue"));
        assert!(badges.iter().any(|(label, _)| label == "selected #1"));
    }

    #[test]
    fn story_and_execution_lines_include_selection() {
        let queue = sample_queue();
        let config =
            parse_wiki_config(include_str!("../../../configs/frwiki.yaml")).expect("config");

        let story = session_story_lines(&config, &queue, Some(0), "note", None);
        let execution = session_execution_lines(
            &queue,
            Some(0),
            "note",
            None,
            "No action executed yet.".to_string(),
        );

        assert!(story.iter().any(|line| line.contains("review rev=123456")));
        assert!(
            execution
                .iter()
                .any(|line| line.contains("selected rev=123456"))
        );
    }

    #[test]
    fn helper_tones_and_controls_behave() {
        let badges = selected_item_badges(&sample_queue(), Some(0));
        assert!(badges.iter().any(|(label, _)| label == "rev 123456"));
        assert_eq!(score_tone(90), StatusTone::Warning);
        assert!(can_shift(Some(0), 1, -1) == false);
        assert!(action_disabled(None, Some(0), 1, SessionActionKind::Rollback, false) == true);
        let _ = session_action_request_for_kind(
            &sample_queue()[0],
            Some("note"),
            SessionActionKind::Patrol,
        );
        let _ = execution_badges(None, "Execution error: x".to_string(), true);
    }
}
