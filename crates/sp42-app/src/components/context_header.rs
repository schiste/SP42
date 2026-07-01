use leptos::prelude::*;
use sp42_core::{EditorIdentity, QueuedEdit};
use sp42_ui::{
    ContextBar, ContextBarProps, ContextShell, ContextShellProps, DeltaText, DeltaTextProps, Gap,
    Inline, InlineProps, Link, LinkProps, ScoreButton, ScoreButtonProps, ScoreDetailItem,
    ScoreDetailItemProps, ScoreDetailsPanel, ScoreDetailsPanelProps, Separator, Size, Spacer, Text,
    TextProps, Tone,
};

use super::{
    style::{score_tone_for_score, wiki_base_url},
    ui_children,
};

/// Compact horizontal bar above the diff showing the selected edit's key info.
#[component]
pub fn ContextHeader(edit: Option<QueuedEdit>) -> impl IntoView {
    let Some(edit) = edit else {
        return ContextShell(ContextShellProps::new(ui_children(|| {
            view! {
                {ContextBar(ContextBarProps::new(ui_children(|| {
                    view! {
                        {Text(
                            TextProps::new(ui_children(|| {
                                view! { "Select an edit to see context." }.into_any()
                            }))
                            .with_tone(Tone::Muted)
                            .with_size(Size::Small),
                        )}
                    }
                    .into_any()
                })))}
            }
            .into_any()
        })))
        .into_any();
    };

    let (show_score_details, set_show_score_details) = signal(false);
    let selected_rev_id = edit.event.rev_id;
    Effect::new(move |_| {
        let _ = selected_rev_id;
        set_show_score_details.set(false);
    });

    let score = edit.score.total;
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
    let score_contributions = edit.score.contributions.clone();

    ContextShell(ContextShellProps::new(ui_children(move || {
        view! {
            {ContextBar(ContextBarProps::new(ui_children(move || {
                view! {
                    {ScoreButton(
                        ScoreButtonProps::new(score)
                            .with_tone(score_tone_for_score(score))
                            .with_state(Signal::derive(move || show_score_details.get()))
                            .with_title("Show score details")
                            .on_click(move |_| {
                                set_show_score_details.update(|open| *open = !*open);
                            }),
                    )}
                    {Separator()}
                    {Inline(
                        InlineProps::new(ui_children(move || {
                            view! {
                                {Text(
                                    TextProps::new(ui_children(move || {
                                        view! { {user_label} }.into_any()
                                    }))
                                    .with_size(Size::Small),
                                )}
                                {Text(
                                    TextProps::new(ui_children(move || {
                                        view! { {format!("({user_type})")} }.into_any()
                                    }))
                                    .with_tone(Tone::Muted)
                                    .with_size(Size::Small),
                                )}
                            }
                            .into_any()
                        }))
                        .with_gap(Gap::XSmall),
                    )}
                    {Separator()}
                    {DeltaText(DeltaTextProps::new(delta).with_suffix(" bytes"))}
                    {if !top_signals.is_empty() {
                        view! {
                            {Separator()}
                            {Text(
                                TextProps::new(ui_children(move || {
                                    view! { {top_signals.join(" · ")} }.into_any()
                                }))
                                .with_tone(Tone::Muted)
                                .with_size(Size::XSmall),
                            )}
                        }
                        .into_any()
                    } else {
                        ().into_any()
                    }}
                    {Spacer()}
                    {Link(LinkProps::new("View on wiki", diff_url).external())}
                }
                .into_any()
            })))}
            {move || {
                if !show_score_details.get() {
                    return ().into_any();
                }
                let details = score_contributions
                    .iter()
                    .filter(|e| e.weight != 0)
                    .map(|entry| {
                        ScoreDetailItem(
                            ScoreDetailItemProps::new(entry.signal.to_string(), entry.weight)
                                .with_note(entry.note.clone()),
                        )
                    })
                    .collect_view();
                ScoreDetailsPanel(ScoreDetailsPanelProps::new(ui_children(move || {
                    view! { {details} }.into_any()
                })))
                .into_any()
            }}
        }
        .into_any()
    })))
    .into_any()
}
