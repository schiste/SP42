use leptos::prelude::*;
use sp42_core::{MediaDiffEntry, MediaDiffKind, MediaDiffReport};
use sp42_ui::{
    Link, LinkProps, MediaCard, MediaCardProps, MediaGalleryPanel, MediaGalleryPanelProps,
    MediaGroup, MediaGroupProps, MediaPreview, MediaPreviewProps, ScrollStack, ScrollStackProps,
    SignatureBlock, SignatureBlockProps, Size, Stack, StackProps, Text, TextProps, TextWeight,
    Tone,
};

use super::ui_children;

#[component]
pub fn MediaDiffGallery(report: Option<MediaDiffReport>, loading: Signal<bool>) -> impl IntoView {
    MediaGalleryPanel(
        MediaGalleryPanelProps::new(
            "Media diff",
            "Media diff",
            "Explicit file/gallery references added or removed in wikitext",
            ui_children(move || {
                view! {
                    {ScrollStack(ScrollStackProps::new(ui_children(move || view! {
                {move || {
                    if loading.get() {
                        return view! {
                            {Text(
                                TextProps::new(ui_children(|| view! { "Loading image changes..." }.into_any()))
                                    .with_tone(Tone::Muted)
                                    .with_size(Size::Small)
                            )}
                        }.into_any();
                    }

                    let Some(report) = report.clone() else {
                        return view! {
                            {Text(
                                TextProps::new(ui_children(|| view! { "No media diff is available for this edit." }.into_any()))
                                    .with_tone(Tone::Muted)
                                    .with_size(Size::Small)
                            )}
                        }.into_any();
                    };

                    let added = entries_for_kind(&report, MediaDiffKind::Added);
                    let removed = entries_for_kind(&report, MediaDiffKind::Removed);
                    let changed = entries_for_kind(&report, MediaDiffKind::Changed);

                    if added.is_empty() && removed.is_empty() && changed.is_empty() {
                        return view! {
                            {Text(
                                TextProps::new(ui_children(|| view! { "No image additions or removals detected." }.into_any()))
                                    .with_tone(Tone::Muted)
                                    .with_size(Size::Small)
                            )}
                        }.into_any();
                    }

                    view! {
                        {render_group("Added", Tone::Success, added)}
                        {render_group("Removed", Tone::Danger, removed)}
                        {render_group("Changed usage", Tone::Warning, changed)}
                    }.into_any()
                }}
                    }.into_any())))}
                }
                .into_any()
            }),
        )
        .with_state(loading),
    )
}

fn entries_for_kind(report: &MediaDiffReport, kind: MediaDiffKind) -> Vec<MediaDiffEntry> {
    report
        .entries
        .iter()
        .filter(|entry| entry.kind == kind)
        .cloned()
        .collect()
}

fn render_group(
    title: &'static str,
    tone: Tone,
    entries: Vec<MediaDiffEntry>,
) -> leptos::prelude::AnyView {
    if entries.is_empty() {
        return ().into_any();
    }

    MediaGroup(
        MediaGroupProps::new(
            title,
            entries.len(),
            ui_children(move || {
                view! {
                {entries.into_iter().map(render_entry_card).collect_view()}
                }
                .into_any()
            }),
        )
        .with_tone(tone),
    )
    .into_any()
}

fn render_entry_card(entry: MediaDiffEntry) -> leptos::prelude::AnyView {
    let title = entry.display_title.clone();
    let page_href = entry.page_url.as_ref().map(ToString::to_string);
    let preview_src = entry.preview_url.as_ref().map(ToString::to_string);
    let usage_summary = usage_summary_line(&entry);

    MediaCard(MediaCardProps::new(ui_children(move || {
        view! {
            {MediaPreview(MediaPreviewProps::new(title.clone(), preview_src))}
            {Stack(
                StackProps::new(ui_children(move || view! {
                {if let Some(href) = page_href {
                    view! {
                        {Link(LinkProps::new(title, href).external())}
                    }.into_any()
                } else {
                    view! {
                        {Text(
                            TextProps::new(ui_children(move || view! { {title} }.into_any()))
                                .with_weight(TextWeight::Bold)
                        )}
                    }.into_any()
                }}
                {Text(
                    TextProps::new(ui_children(move || view! { {usage_summary} }.into_any()))
                        .with_tone(Tone::Muted)
                        .with_size(Size::XSmall)
                )}
                {render_signature_block("Before", entry.before_signatures.clone())}
                {render_signature_block("After", entry.after_signatures.clone())}
                }.into_any()))
                .with_gap(sp42_ui::Gap::XSmall)
            )}
        }
        .into_any()
    })))
    .into_any()
}

fn usage_summary_line(entry: &MediaDiffEntry) -> String {
    format!(
        "Occurrences: {} → {}",
        entry.before_occurrences, entry.after_occurrences
    )
}

fn render_signature_block(
    label: &'static str,
    signatures: Vec<String>,
) -> leptos::prelude::AnyView {
    if signatures.is_empty() {
        return ().into_any();
    }

    SignatureBlock(SignatureBlockProps::new(label, signatures.join(" · "))).into_any()
}

#[cfg(test)]
mod tests {
    use sp42_core::{MediaDiffEntry, MediaDiffKind, MediaDiffReport};

    use super::{entries_for_kind, usage_summary_line};

    fn sample_report() -> MediaDiffReport {
        MediaDiffReport {
            entries: vec![
                MediaDiffEntry {
                    kind: MediaDiffKind::Added,
                    file_name: "File:Added.jpg".to_string(),
                    display_title: "Added.jpg".to_string(),
                    before_occurrences: 0,
                    after_occurrences: 1,
                    before_signatures: Vec::new(),
                    after_signatures: vec!["thumb | caption".to_string()],
                    preview_url: None,
                    page_url: None,
                },
                MediaDiffEntry {
                    kind: MediaDiffKind::Removed,
                    file_name: "File:Removed.jpg".to_string(),
                    display_title: "Removed.jpg".to_string(),
                    before_occurrences: 1,
                    after_occurrences: 0,
                    before_signatures: vec!["thumb".to_string()],
                    after_signatures: Vec::new(),
                    preview_url: None,
                    page_url: None,
                },
            ],
            notes: Vec::new(),
        }
    }

    #[test]
    fn entries_for_kind_filters_entries() {
        let added = entries_for_kind(&sample_report(), MediaDiffKind::Added);
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].display_title, "Added.jpg");
    }

    #[test]
    fn usage_summary_line_formats_occurrence_delta() {
        let report = sample_report();
        assert_eq!(usage_summary_line(&report.entries[0]), "Occurrences: 0 → 1");
    }
}
