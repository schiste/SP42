use leptos::prelude::*;
use sp42_core::{EditorIdentity, QueuedEdit};

use super::style::score_tier;

#[component]
pub fn QueueColumn(
    queue: Vec<QueuedEdit>,
    selected_index: ReadSignal<usize>,
    set_selected_index: WriteSignal<usize>,
) -> impl IntoView {
    let count = queue.len();
    view! {
        <nav role="navigation" aria-label="Edit queue" class="queue-column">
            <div class="queue-header">
                {format!("Queue ({count})")}
            </div>
            <div class="queue-scroll">
                {if queue.is_empty() {
                    view! {
                        <div class="grid-center text-muted" style="padding:17px;font-size:12px;">
                            <p style="margin:0 0 4px;font-weight:700;">"No edits in queue"</p>
                            <p style="margin:0;">"Adjust filters or load older."</p>
                        </div>
                    }.into_any()
                } else {
                    view! { <div>
                {queue
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let score = item.score.total;
                        let title = item.event.title.clone();
                        let is_patrolled = item.event.is_patrolled.is_enabled();
                        let (tier_color, _tier_icon) = score_tier(score);
                        let user = match &item.event.performer {
                            EditorIdentity::Registered { username } => username.clone(),
                            EditorIdentity::Anonymous { label } => label.clone(),
                            EditorIdentity::Temporary { label } => label.clone(),
                        };
                        let delta = item.event.byte_delta;
                        let delta_str = if delta > 0 { format!("+{delta}") } else { delta.to_string() };
                        let delta_color = if delta > 0 { "var(--success)" } else if delta < 0 { "var(--danger)" } else { "var(--muted)" };

                        view! {
                            <button
                                class="queue-item"
                                style=move || {
                                    if selected_index.get() == index {
                                        format!("border-inline-start:3px solid {tier_color};background:var(--selected);{}", if is_patrolled { "opacity:0.5;" } else { "" })
                                    } else {
                                        format!("border-inline-start:3px solid transparent;{}", if is_patrolled { "opacity:0.5;" } else { "" })
                                    }
                                }
                                on:click=move |_| set_selected_index.set(index)
                                aria-pressed=move || (selected_index.get() == index).to_string()
                            >
                                <div class="queue-item-top">
                                    <span class="queue-score" style=format!("color:{tier_color};")>
                                        {score.to_string()}
                                    </span>
                                    <span class="queue-title">{title}</span>
                                </div>
                                <div class="queue-item-meta">
                                    <span>{user}</span>
                                    <span style=format!("color:{delta_color};")>{delta_str}</span>
                                    {if is_patrolled {
                                        view! { <span class="text-success">"P"</span> }.into_any()
                                    } else {
                                        view! { <span></span> }.into_any()
                                    }}
                                </div>
                            </button>
                        }
                    })
                    .collect_view()}
                    </div> }.into_any()
                }}
            </div>
        </nav>
    }
}
