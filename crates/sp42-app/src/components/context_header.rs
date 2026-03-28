use leptos::prelude::*;
use sp42_core::{EditorIdentity, QueuedEdit};

use super::style::{score_tier, wiki_base_url};

/// Compact horizontal bar above the diff showing the selected edit's key info.
#[component]
pub fn ContextHeader(edit: Option<QueuedEdit>) -> impl IntoView {
    let Some(edit) = edit else {
        return view! {
            <div class="context-header text-muted">"Select an edit to see context."</div>
        }
        .into_any();
    };

    let (show_score_details, set_show_score_details) = signal(false);
    let selected_rev_id = edit.event.rev_id;
    Effect::new(move |_| {
        let _ = selected_rev_id;
        set_show_score_details.set(false);
    });

    let score = edit.score.total;
    let (tier_color, tier_icon) = score_tier(score);
    let user_label = match &edit.event.performer {
        EditorIdentity::Registered { username } => username.clone(),
        EditorIdentity::Anonymous { label } => label.clone(),
        EditorIdentity::Temporary { label } => format!("{label} (temp)"),
    };
    let user_type = match &edit.event.performer {
        EditorIdentity::Registered { .. } => "registered",
        EditorIdentity::Anonymous { .. } => "IP",
        EditorIdentity::Temporary { .. } => "temp",
    };
    let delta = edit.event.byte_delta;
    let delta_color = if delta > 0 {
        "var(--success)"
    } else if delta < 0 {
        "var(--danger)"
    } else {
        "var(--muted)"
    };
    let delta_str = if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    };

    let top_signals: Vec<_> = edit
        .score
        .contributions
        .iter()
        .filter(|s| s.weight != 0)
        .take(3)
        .map(|s| {
            let sign = if s.weight > 0 { "+" } else { "" };
            format!("{} {sign}{}", s.signal, s.weight)
        })
        .collect();

    let base = wiki_base_url(&edit.event.wiki_id);
    let rev_id = edit.event.rev_id;
    let old_rev_id = edit.event.old_rev_id.unwrap_or(0);
    let diff_url = format!("{base}/w/index.php?diff={rev_id}&oldid={old_rev_id}");

    view! {
        <div class="context-header-shell">
            <div class="context-header">
                <button
                    type="button"
                    class="context-score-button"
                    on:click=move |_| {
                        set_show_score_details.update(|open| *open = !*open);
                    }
                    aria-expanded=move || show_score_details.get().to_string()
                    title="Show score details"
                >
                    <span class="context-score" style=format!("color:{tier_color};")>
                        {score.to_string()} " " {tier_icon}
                    </span>
                </button>
                <span class="context-separator">"|"</span>
                <span class="context-user">
                    {user_label} " " <span class="text-muted">{"(" }{user_type}{")"}</span>
                </span>
                <span class="context-separator">"|"</span>
                <span style=format!("color:{delta_color};font-weight:700;")>
                    {delta_str} " bytes"
                </span>
                {if !top_signals.is_empty() {
                    view! {
                        <span class="context-separator">"|"</span>
                        <span class="context-signals">
                            {top_signals.join(" · ")}
                        </span>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
                <div class="flex-spacer"></div>
                <a
                    href=diff_url
                    target="_blank"
                    rel="noopener"
                    class="context-link"
                >
                    "View on wiki"
                </a>
            </div>
            {move || {
                if !show_score_details.get() {
                    return view! { <span></span> }.into_any();
                }
                let details = edit
                    .score
                    .contributions
                    .iter()
                    .filter(|e| e.weight != 0)
                    .map(|entry| {
                        let wp = if entry.weight > 0 { "+" } else { "" };
                        let tone = if entry.weight > 0 { "var(--danger)" } else { "var(--success)" };
                        let note = entry.note.clone();
                        view! {
                            <li class="score-details-item">
                                <div class="score-details-line">
                                    <span class="score-details-signal">{entry.signal.to_string()}</span>
                                    <span class="score-details-weight" style=format!("color:{tone};")>
                                        {format!("{wp}{}", entry.weight)}
                                    </span>
                                </div>
                                {note.map(|v| view! { <div class="score-details-note">{v}</div> })}
                            </li>
                        }
                    })
                    .collect_view();
                view! {
                    <div class="score-details-panel">
                        <div class="score-details-summary">
                            <span>"Score details"</span>
                        </div>
                        <ul class="score-details-list">{details}</ul>
                    </div>
                }.into_any()
            }}
        </div>
    }
    .into_any()
}
