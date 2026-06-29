use leptos::prelude::*;
use sp42_core::{EditorIdentity, QueuedEdit};
use sp42_ui::{
    Align, DeltaText, DeltaTextProps, EmptyState, EmptyStateProps, Gap, Inline, InlineProps,
    NavigationItem, NavigationItemProps, NavigationPane, NavigationPaneProps, ScoreText,
    ScoreTextProps, ScoreTone, Size, Stack, StackProps, StatusBadgeProps, StatusTone, Text,
    TextOverflow, TextProps, TextSize, TextTone,
};

use super::ui_children;

#[component]
pub fn QueueColumn(
    queue: Vec<QueuedEdit>,
    selected_rev_id: Signal<Option<u64>>,
    set_selected_rev_id: WriteSignal<Option<u64>>,
    group_counts: std::collections::HashMap<u64, usize>,
) -> impl IntoView {
    let count = queue.len();
    NavigationPane(NavigationPaneProps::new(
        "Edit queue",
        format!("Queue ({count})"),
        ui_children(move || {
            if queue.is_empty() {
                return view! {
                    {EmptyState(EmptyStateProps::new(
                        "No edits in queue",
                        "Adjust filters or load older.",
                    ))}
                }
                .into_any();
            }

            view! {
                {Stack(
                    StackProps::new(ui_children(move || {
                        view! {
                            {queue
                                .into_iter()
                                .map(|item| {
                        let score = item.score.total;
                        let title = item.event.title.clone();
                        let is_patrolled = item.event.is_patrolled.is_enabled();
                        let user = match &item.event.performer {
                            EditorIdentity::Registered { username } => username.clone(),
                            EditorIdentity::Anonymous { label } => label.clone(),
                            EditorIdentity::Temporary { label } => label.clone(),
                        };
                        let delta = item.event.byte_delta;

                        let rev_id = item.event.rev_id;
                        let group_count = group_counts.get(&rev_id).copied().unwrap_or(1);
                        NavigationItem(navigation_item_props(
                            score,
                            is_patrolled,
                            Signal::derive(move || selected_rev_id.get() == Some(rev_id)),
                            move |_| set_selected_rev_id.set(Some(rev_id)),
                            ui_children(move || {
                                view! {
                                    {Inline(
                                        InlineProps::new(ui_children(move || {
                                            view! {
                                                {ScoreText(
                                                    ScoreTextProps::new(score)
                                                        .without_icon()
                                                        .with_size(Size::Medium),
                                                )}
                                                {Text(
                                                    TextProps::new(ui_children(move || {
                                                        view! { {title} }.into_any()
                                                    }))
                                                    .with_size(TextSize::Small)
                                                    .with_overflow(TextOverflow::ClampTwo),
                                                )}
                                            }
                                            .into_any()
                                        }))
                                        .with_gap(Gap::Small)
                                        .with_align(Align::Baseline)
                                        .without_wrap(),
                                    )}
                                    {Inline(
                                        InlineProps::new(ui_children(move || {
                                            view! {
                                                {Text(
                                                    TextProps::new(ui_children(move || {
                                                        view! { {user} }.into_any()
                                                    }))
                                                    .with_tone(TextTone::Muted)
                                                    .with_size(TextSize::XSmall),
                                                )}
                                                {DeltaText(
                                                    DeltaTextProps::new(delta)
                                                        .with_size(TextSize::XSmall),
                                                )}
                                                {if is_patrolled {
                                                    sp42_ui::StatusBadge(
                                                        StatusBadgeProps::new("P")
                                                            .with_tone(StatusTone::Success)
                                                            .with_size(Size::Small),
                                                    ).into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                                {if group_count > 1 {
                                                    sp42_ui::StatusBadge(
                                                        StatusBadgeProps::new(format!("{group_count} edits"))
                                                            .with_tone(StatusTone::Accent)
                                                            .with_size(Size::Small),
                                                    ).into_any()
                                                } else {
                                                    view! { <span></span> }.into_any()
                                                }}
                                            }
                                            .into_any()
                                        }))
                                        .with_gap(Gap::Small),
                                    )}
                                }
                                .into_any()
                            }),
                        ))
                    })
                    .collect_view()}
                        }
                        .into_any()
                    }))
                    .with_gap(Gap::None),
                )}
            }
            .into_any()
        }),
    ))
}

fn navigation_item_props(
    score: i32,
    is_patrolled: bool,
    selected: Signal<bool>,
    on_click: impl Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
    children: Children,
) -> NavigationItemProps {
    let props = NavigationItemProps::new(children)
        .with_selected(selected)
        .with_tone(ScoreTone::for_score(score))
        .on_click(on_click);
    if is_patrolled { props.subdued() } else { props }
}
