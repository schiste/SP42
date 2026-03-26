use leptos::prelude::*;
use sp42_core::QueuedEdit;

#[component]
pub fn QueueColumn(
    queue: Vec<QueuedEdit>,
    selected_index: ReadSignal<usize>,
    set_selected_index: WriteSignal<usize>,
) -> impl IntoView {
    view! {
        <nav
            role="navigation"
            aria-label="Edit queue"
            style="overflow-y:auto;min-height:0;background:#0b1324;border-inline-end:1px solid rgba(148,163,184,.12);"
        >
            {queue
                .into_iter()
                .enumerate()
                .map(|(index, item)| {
                    let score = item.score.total;
                    let title = item.event.title.clone();
                    let is_patrolled = item.event.is_patrolled.is_enabled();
                    let (tier_bg, tier_icon) = score_tier(score);
                    let opacity = if is_patrolled { "opacity:0.5;" } else { "" };
                    view! {
                        <button
                            style=move || {
                                format!(
                                    "display:grid;gap:2px;width:100%;padding:7px 10px;\
                                     border:none;border-block-end:1px solid rgba(148,163,184,.12);\
                                     text-align:start;cursor:pointer;\
                                     font:inherit;color:#eff4ff;min-height:44px;\
                                     background:{};transition:background 120ms;{opacity}",
                                    if selected_index.get() == index {
                                        "#111b2e"
                                    } else {
                                        "transparent"
                                    },
                                )
                            }
                            on:click=move |_| set_selected_index.set(index)
                            aria-pressed=move || (selected_index.get() == index).to_string()
                        >
                            <div style="display:flex;align-items:center;gap:7px;">
                                <span style=format!(
                                    "font-weight:700;font-size:13px;color:{tier_bg};",
                                )>
                                    {format!("{score}")}
                                </span>
                                <span style="font-size:11px;color:#8b9fc0;">{tier_icon}</span>
                                {if is_patrolled {
                                    view! { <span style="font-size:10px;color:#22c55e;">"\u{2713}P"</span> }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </div>
                            <div style="font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;">
                                {title}
                            </div>
                        </button>
                    }
                })
                .collect_view()}
        </nav>
    }
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
