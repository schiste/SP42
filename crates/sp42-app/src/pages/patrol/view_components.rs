use std::collections::HashMap;

use leptos::prelude::*;
use sp42_core::{MediaDiffReport, QueuedEdit, SessionActionKind, StructuredDiff};
use sp42_patrol::LiveOperatorView;
use sp42_ui::{
    ActionBarShell, ActionBarShellProps, Button, ButtonProps, ButtonSurface, DataPanel,
    DataPanelProps, Density, FullscreenOverlay, FullscreenOverlayProps, Gap, Heading, HeadingLevel,
    HeadingProps, Inline, InlineProps, KeyboardShortcutModal, KeyboardShortcutModalProps,
    LoadingRegion, LoadingRegionProps, MetaText, MetaTextProps, ResultCard, ResultCardProps,
    ResultList, ResultListProps, ShortcutDefinition, Size, Spacer, SplitWorkArea,
    SplitWorkAreaProps, Stack, StackProps, StatusBadge, StatusBadgeProps, StatusBar,
    StatusBarProps, StatusDot, StatusDotProps, StatusRegion, StatusRegionProps, Text, TextElement,
    TextInput, TextInputProps, TextProps, Tone, Width, WorkspaceMain, WorkspaceMainProps,
};

use crate::components::action_bar::ActionBar;
use crate::components::context_header::ContextHeader;
use crate::components::diff_viewer::{DiffViewer, EditAction, TagAction};
use crate::components::media_diff_gallery::MediaDiffGallery;
use crate::components::queue_column::QueueColumn;
use crate::components::{
    PatrolScenarioPanel, PatrolSessionDigestPanel, ShellStatePanel, ui_children,
};

#[component]
pub(super) fn AuthRequiredView(
    bridge_mode: String,
    has_token: bool,
    bootstrap_attempted: ReadSignal<bool>,
    bootstrap_error: ReadSignal<Option<String>>,
    set_bootstrap_attempted: WriteSignal<bool>,
    load_action: Action<(), ()>,
) -> impl IntoView {
    StatusRegion(StatusRegionProps::new(ui_children(move || {
        view! {
            {Heading(
                HeadingProps::new(ui_children(|| view! { "Authentication required" }.into_any()))
                    .with_level(HeadingLevel::One)
                    .with_size(Size::Large)
            )}
            {move || {
                if bootstrap_attempted.get() {
                    Text(
                        TextProps::new(ui_children(move || {
                            view! {
                                {bootstrap_error
                                    .get()
                                    .unwrap_or_else(|| "Auto-bootstrap did not produce an authenticated session.".to_string())}
                            }
                            .into_any()
                        }))
                        .with_tone(Tone::Warning)
                        .with_element(TextElement::Paragraph),
                    )
                    .into_any()
                } else {
                    Text(
                        TextProps::new(ui_children(|| {
                            view! { "Bootstrapping session from local token bridge..." }.into_any()
                        }))
                        .with_tone(Tone::Muted)
                        .with_element(TextElement::Paragraph),
                    )
                    .into_any()
                }
            }}
            {Stack(
                StackProps::new(ui_children(move || view! {
                    {MetaText(MetaTextProps::new(ui_children(move || {
                        view! { {format!("Bridge mode: {bridge_mode}")} }.into_any()
                    })))}
                    {MetaText(MetaTextProps::new(ui_children(move || {
                        view! {
                            {format!("Local token: {}", if has_token { "present" } else { "missing" })}
                        }
                        .into_any()
                    })))}
                }.into_any()))
                .with_gap(Gap::XSmall)
            )}
            {(!has_token).then(|| {
                Text(
                    TextProps::new(ui_children(|| {
                        view! {
                            "No WIKIMEDIA_ACCESS_TOKEN found. Create a .env.wikimedia.local file with your token and restart the server."
                        }
                        .into_any()
                    }))
                    .with_tone(Tone::Danger)
                    .with_size(Size::Small)
                    .with_element(TextElement::Paragraph),
                )
            })}
            {Inline(InlineProps::new(ui_children(move || view! {
                {Button(
                    ButtonProps::new("Bootstrap session")
                        .with_tone(Tone::Accent)
                        .on_click(move |_| {
                            set_bootstrap_attempted.set(false);
                            load_action.dispatch_local(());
                        })
                )}
                {Button(ButtonProps::new("Retry").on_click(move |_| {
                    load_action.dispatch_local(());
                }))}
            }.into_any())).with_gap(Gap::Small))}
        }
        .into_any()
    })))
}

#[component]
pub(super) fn HelpModal(
    show_help: ReadSignal<bool>,
    set_show_help: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        {move || {
            if !show_help.get() {
                return ().into_any();
            }
            KeyboardShortcutModal(
                KeyboardShortcutModalProps::new(
                    "Keyboard Shortcuts",
                    vec![
                        ShortcutDefinition::new("Rollback", "R"),
                        ShortcutDefinition::new("Undo", "U"),
                        ShortcutDefinition::new("Patrol", "P"),
                        ShortcutDefinition::new("Skip", "S"),
                        ShortcutDefinition::new("Previous edit", "\u{2191}"),
                        ShortcutDefinition::new("Next edit", "\u{2193}"),
                        ShortcutDefinition::new("This help", "?"),
                        ShortcutDefinition::new("Back-office", "Ctrl+Shift+D"),
                    ],
                )
                .on_close(move |_| set_show_help.set(false)),
            )
            .into_any()
        }}
    }
}

#[component]
pub(super) fn BackofficeModal(
    show_backoffice: ReadSignal<bool>,
    set_show_backoffice: WriteSignal<bool>,
    view_data: ReadSignal<Option<LiveOperatorView>>,
) -> impl IntoView {
    view! {
        {move || {
            if !show_backoffice.get() {
                return ().into_any();
            }
            let view = view_data.get();
            FullscreenOverlay(FullscreenOverlayProps::new(ui_children(move || {
                view! {
                    {Inline(
                        InlineProps::new(ui_children(move || view! {
                            {Heading(
                                HeadingProps::new(ui_children(|| view! { "Back-office" }.into_any()))
                                    .with_size(Size::Medium)
                            )}
                            {Spacer()}
                            {Button(
                                ButtonProps::new("Close (Esc)")
                                    .on_click(move |_| set_show_backoffice.set(false))
                            )}
                        }.into_any()))
                        .with_gap(Gap::Medium)
                    )}
                        {if let Some(ref view) = view {
                            let history_entries = view.action_history.entries.clone();
                            view! {
                                <PatrolScenarioPanel report=view.scenario_report.clone() />
                                <PatrolSessionDigestPanel report=view.scenario_report.clone() />
                                <ShellStatePanel model=view.shell_state.clone() />
                                <ActionHistoryPanel history_entries=history_entries />
                            }.into_any()
                        } else {
                            Text(
                                TextProps::new(ui_children(|| {
                                    view! { "Load the patrol queue first." }.into_any()
                                }))
                                .with_tone(Tone::Muted)
                                .with_element(TextElement::Paragraph),
                            )
                            .into_any()
                        }}
                }.into_any()
            })))
            .into_any()
        }}
    }
}

#[component]
pub(super) fn ActionHistoryPanel(
    history_entries: Vec<sp42_core::ActionExecutionLogEntry>,
) -> impl IntoView {
    if history_entries.is_empty() {
        return ().into_any();
    }

    let history_count = history_entries.len();
    DataPanel(
        DataPanelProps::new(
            "Action History",
            ui_children(move || {
                view! {
                    {ResultList(ResultListProps::new(ui_children(move || view! {
                        {history_entries.into_iter().map(|entry| {
                    let label = entry.kind.label().to_string();
                    let status_tone = if entry.accepted { Tone::Success } else { Tone::Danger };
                    let status_text = if entry.accepted { "OK" } else { "Failed" };
                    let detail = entry.error.or(entry.api_code).unwrap_or_default();
                    ResultCard(ResultCardProps::new(ui_children(move || {
                        view! {
                            {Inline(InlineProps::new(ui_children(move || view! {
                                {Text(
                                    TextProps::new(ui_children(move || view! { {label} }.into_any()))
                                        .with_weight(sp42_ui::TextWeight::Bold)
                                )}
                                {MetaText(MetaTextProps::new(ui_children(move || {
                                    view! { {format!("r{}", entry.rev_id)} }.into_any()
                                })))}
                                {StatusBadge(
                                    StatusBadgeProps::new(status_text).with_tone(status_tone)
                                )}
                                {(!detail.is_empty()).then(|| {
                                    Text(
                                        TextProps::new(ui_children(move || {
                                            view! { {detail} }.into_any()
                                        }))
                                        .with_tone(Tone::Warning)
                                        .with_size(Size::XSmall),
                                    )
                                })}
                            }.into_any())).with_gap(Gap::Small))}
                        }
                        .into_any()
                    })))
                        }).collect_view()}
                    }.into_any())))}
                }
                .into_any()
            }),
        )
        .with_count(history_count.to_string()),
    )
    .into_any()
}

#[component]
pub(super) fn SessionBar(
    view_data: ReadSignal<Option<LiveOperatorView>>,
    load_error: ReadSignal<Option<String>>,
    action_status: ReadSignal<String>,
    set_show_help: WriteSignal<bool>,
) -> impl IntoView {
    let status_tone = Signal::derive(move || {
        if load_error.get().is_some() {
            Tone::Danger
        } else if view_data.get().is_some() {
            Tone::Success
        } else {
            Tone::Warning
        }
    });

    StatusBar(StatusBarProps::new(ui_children(move || {
        view! {
            {Text(
                TextProps::new(ui_children(|| {
                    view! { {sp42_core::branding::PROJECT_NAME} }.into_any()
                }))
                .with_tone(Tone::Accent)
                .with_weight(sp42_ui::TextWeight::Bold)
            )}
            {move || {
                view_data
                    .get()
                    .map(|view| {
                        view! {
                            {Text(TextProps::new(ui_children(move || {
                                view! { {view.wiki_id.clone()} }.into_any()
                            })))}
                            {Text(TextProps::new(ui_children(move || {
                                view! {
                                    {view.auth.username.clone().unwrap_or_else(|| "-".to_string())}
                                }
                                .into_any()
                            })))}
                            {Text(TextProps::new(ui_children(move || {
                                view! { {format!("{} edits", view.queue.len())} }.into_any()
                            })))}
                        }
                        .into_any()
                    })
                    .unwrap_or_else(|| {
                        Text(
                            TextProps::new(ui_children(|| view! { "loading..." }.into_any()))
                                .with_tone(Tone::Muted),
                        )
                        .into_any()
                    })
            }}
            {Spacer()}
            {StatusDot(
                StatusDotProps::new("Session status").with_tone(status_tone)
            )}
            {move || {
                let status = action_status.get();
                if !status.is_empty() {
                    Text(
                        TextProps::new(ui_children(move || view! { {status} }.into_any()))
                            .with_size(Size::XSmall),
                    )
                    .into_any()
                } else {
                    ().into_any()
                }
            }}
            {Button(
                ButtonProps::new("?")
                    .with_surface(ButtonSurface::Ghost)
                    .with_density(Density::Compact)
                    .with_aria_label("Show keyboard shortcuts")
                    .on_click(move |_| set_show_help.set(true))
            )}
        }
        .into_any()
    })))
}

#[component]
pub(super) fn QueuePane(
    queue: Memo<Vec<QueuedEdit>>,
    selected_rev_id: ReadSignal<Option<u64>>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
    group_rev_ids: ReadSignal<HashMap<u64, Vec<u64>>>,
    load_error: ReadSignal<Option<String>>,
    load_action: Action<(), ()>,
) -> impl IntoView {
    view! {
        {move || {
            let visible_queue = queue.get();
            if !visible_queue.is_empty() {
                view! {
                    <QueueColumn
                        queue=visible_queue
                        selected_rev_id=Signal::derive(move || selected_rev_id.get())
                        set_selected_rev_id=set_selected_rev_id
                        group_counts=group_rev_ids
                            .get_untracked()
                            .iter()
                            .map(|(k, v)| (*k, v.len()))
                            .collect()
                    />
                }
                .into_any()
            } else if let Some(error) = load_error.get() {
                StatusRegion(
                    StatusRegionProps::new(ui_children(move || {
                        view! {
                            {Heading(
                                HeadingProps::new(ui_children(|| {
                                    view! { "Queue unavailable" }.into_any()
                                }))
                                .with_size(Size::Small)
                            )}
                            {Text(
                                TextProps::new(ui_children(move || view! { {error} }.into_any()))
                                    .with_size(Size::Small)
                                    .with_element(TextElement::Paragraph)
                            )}
                            {Button(ButtonProps::new("Retry").on_click(move |_| {
                                load_action.dispatch_local(());
                            }))}
                        }
                        .into_any()
                    }))
                    .with_tone(Tone::Danger),
                )
                .into_any()
            } else {
                LoadingRegion(LoadingRegionProps::new("Loading queue...")).into_any()
            }
        }}
    }
}

#[component]
pub(super) fn DiffPane(
    selected_edit: ReadSignal<Option<QueuedEdit>>,
    diff_loading: ReadSignal<bool>,
    current_diff: ReadSignal<Option<StructuredDiff>>,
    current_media_diff: ReadSignal<Option<MediaDiffReport>>,
    media_diff_loading: ReadSignal<bool>,
    set_tag_action: WriteSignal<Option<TagAction>>,
    set_edit_action: WriteSignal<Option<EditAction>>,
) -> impl IntoView {
    WorkspaceMain(WorkspaceMainProps::new(ui_children(move || {
        view! {
            {move || {
                view! { <ContextHeader edit=selected_edit.get() /> }.into_any()
            }}
            {move || {
                let report = current_media_diff.get();
                let show_media_diff = report.as_ref().is_some_and(MediaDiffReport::has_changes);
                let primary = ui_children(move || {
                    view! {
                        {move || {
                            let selected = selected_edit.get();
                            if diff_loading.get() {
                                LoadingRegion(LoadingRegionProps::new("Loading diff...")).into_any()
                            } else {
                                view! {
                                    <DiffViewer
                                        diff=current_diff.get()
                                        wiki_id=selected.as_ref().map(|edit| edit.event.wiki_id.clone()).unwrap_or_default()
                                        rev_id=selected.as_ref().map(|edit| edit.event.rev_id).unwrap_or(0)
                                        old_rev_id=selected.as_ref().and_then(|edit| edit.event.old_rev_id).unwrap_or(0)
                                        on_tag=set_tag_action
                                        on_edit=set_edit_action
                                    />
                                }.into_any()
                            }
                        }}
                    }
                    .into_any()
                });

                if show_media_diff {
                    SplitWorkArea(
                        SplitWorkAreaProps::new(primary).with_aside(ui_children(move || {
                            view! {
                                    <MediaDiffGallery
                                        report=report
                                        loading=Signal::derive(move || media_diff_loading.get())
                                    />
                            }
                            .into_any()
                        })),
                    )
                    .into_any()
                } else {
                    SplitWorkArea(SplitWorkAreaProps::new(primary)).into_any()
                }
            }}
        }
        .into_any()
    })))
}

#[component]
pub(super) fn ActionFooter(
    view_data: ReadSignal<Option<LiveOperatorView>>,
    queue: Memo<Vec<QueuedEdit>>,
    review_note: ReadSignal<String>,
    set_review_note: WriteSignal<String>,
    has_selection: Memo<bool>,
    action_pending: ReadSignal<bool>,
    set_action_trigger: WriteSignal<Option<SessionActionKind>>,
    set_skip_trigger: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        {move || {
            let has_view = view_data.get().is_some();
            let has_queue = !queue.get().is_empty();
            if has_view || has_queue {
                let preflight = view_data
                    .get()
                    .map(|view| view.action_preflight.clone())
                    .unwrap_or_default();
                let capabilities = view_data
                    .get()
                    .map(|view| view.capabilities.clone())
                    .unwrap_or_default();
                ActionBarShell(ActionBarShellProps::new(ui_children(move || {
                    view! {
                        {TextInput(
                            TextInputProps::new("review-note")
                                .with_value(Signal::derive(move || review_note.get()))
                                .with_placeholder("Review note (optional)")
                                .with_aria_label("Review note")
                                .with_width(Width::Full)
                                .with_density(Density::Compact)
                                .on_input(move |ev| {
                                use wasm_bindgen::JsCast;
                                let value = ev.target()
                                    .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
                                    .map(|element| element.value())
                                    .unwrap_or_default();
                                set_review_note.set(value);
                            })
                        )}
                        <ActionBar
                            preflight=preflight
                            capabilities=capabilities
                            has_selection=Signal::derive(move || has_selection.get())
                            action_pending=Signal::derive(move || action_pending.get())
                            on_action=set_action_trigger
                            on_skip=set_skip_trigger
                        />
                    }
                    .into_any()
                })))
                .into_any()
            } else {
                ActionBarShell(ActionBarShellProps::new(ui_children(|| {
                    view! {
                        {Text(
                            TextProps::new(ui_children(|| {
                                view! { "Actions available after queue loads." }.into_any()
                            }))
                            .with_tone(Tone::Muted)
                            .with_size(Size::Small)
                        )}
                    }
                    .into_any()
                })))
                .into_any()
            }
        }}
    }
}
