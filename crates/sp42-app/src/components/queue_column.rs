use leptos::prelude::*;
use sp42_core::QueuedEdit;

use super::style::score_tier;

#[component]
pub fn QueueColumn(
    queue: Vec<QueuedEdit>,
    selected_index: ReadSignal<usize>,
    set_selected_index: WriteSignal<usize>,
) -> impl IntoView {
    view! {
        <nav role="navigation" aria-label="Edit queue" class="queue-column">
            {if queue.is_empty() {
                view! {
                    <div class="grid-center text-muted" style="padding:17px;font-size:12px;">
                        <p style="margin:0 0 4px;font-weight:700;">"No edits in queue"</p>
                        <p style="margin:0;">"Try adjusting your filters."</p>
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
                    let (tier_color, tier_icon) = score_tier(score);
                    view! {
                        <button
                            class="queue-item"
                            style=move || {
                                let bg = if selected_index.get() == index { "var(--selected)" } else { "transparent" };
                                let op = if is_patrolled { "opacity:0.5;" } else { "" };
                                format!("background:{bg};{op}")
                            }
                            on:click=move |_| set_selected_index.set(index)
                            aria-pressed=move || (selected_index.get() == index).to_string()
                        >
                            <div style="display:flex;align-items:center;gap:7px;">
                                <span style=format!("font-weight:700;font-size:13px;color:{tier_color};")>
                                    {score.to_string()}
                                </span>
                                <span class="text-muted" style="font-size:11px;">{tier_icon}</span>
                                {if is_patrolled {
                                    view! { <span class="text-success" style="font-size:10px;">"\u{2713}P"</span> }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </div>
                            <div class="truncate" style="font-size:12px;">
                                {title}
                            </div>
                        </button>
                    }
                })
                .collect_view()}
                </div> }.into_any()
            }}
        </nav>
    }
}
