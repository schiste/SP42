use leptos::prelude::*;
use sp42_core::{MediaDiffEntry, MediaDiffKind, MediaDiffReport};

#[component]
pub fn MediaDiffGallery(report: Option<MediaDiffReport>, loading: Signal<bool>) -> impl IntoView {
    view! {
        <aside
            aria-label="Media diff"
            class="panel"
            style="display:grid;grid-template-rows:auto 1fr;min-width:0;min-height:0;overflow:hidden;"
        >
            <div
                style="display:flex;align-items:center;justify-content:space-between;gap:10px;\
                       padding:10px 12px;border-block-end:1px solid var(--border);"
            >
                <div style="display:grid;gap:2px;">
                    <strong style="font-size:13px;">"Media diff"</strong>
                    <span style="font-size:11px;color:var(--muted);">
                        "Explicit file/gallery references added or removed in wikitext"
                    </span>
                </div>
                {move || {
                    if loading.get() {
                        view! { <span style="font-size:11px;color:var(--muted);">"Loading…"</span> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
            </div>
            <div style="overflow:auto;padding:12px;display:grid;gap:12px;align-content:start;">
                {move || {
                    if loading.get() {
                        return view! {
                            <div style="color:var(--muted);font-size:12px;">"Loading image changes…"</div>
                        }.into_any();
                    }

                    let Some(report) = report.clone() else {
                        return view! {
                            <div style="color:var(--muted);font-size:12px;">
                                "No media diff is available for this edit."
                            </div>
                        }.into_any();
                    };

                    let added = entries_for_kind(&report, MediaDiffKind::Added);
                    let removed = entries_for_kind(&report, MediaDiffKind::Removed);
                    let changed = entries_for_kind(&report, MediaDiffKind::Changed);

                    if added.is_empty() && removed.is_empty() && changed.is_empty() {
                        return view! {
                            <div style="color:var(--muted);font-size:12px;">
                                "No image additions or removals detected."
                            </div>
                        }.into_any();
                    }

                    view! {
                        {render_group("Added", "var(--success)", added)}
                        {render_group("Removed", "var(--danger)", removed)}
                        {render_group("Changed usage", "var(--warning)", changed)}
                    }.into_any()
                }}
            </div>
        </aside>
    }
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
    accent: &'static str,
    entries: Vec<MediaDiffEntry>,
) -> leptos::prelude::AnyView {
    if entries.is_empty() {
        return view! { <span></span> }.into_any();
    }

    view! {
        <section style="display:grid;gap:8px;">
            <div style="display:flex;align-items:center;justify-content:space-between;gap:10px;">
                <strong style=format!("font-size:12px;color:{accent};")>{title}</strong>
                <span style="font-size:11px;color:var(--muted);">{entries.len()}</span>
            </div>
            <div style="display:grid;gap:10px;">
                {entries.into_iter().map(render_entry_card).collect_view()}
            </div>
        </section>
    }
    .into_any()
}

fn render_entry_card(entry: MediaDiffEntry) -> leptos::prelude::AnyView {
    let title = entry.display_title.clone();
    let page_href = entry.page_url.as_ref().map(ToString::to_string);
    let preview_src = entry.preview_url.as_ref().map(ToString::to_string);
    let usage_summary = usage_summary_line(&entry);

    view! {
        <article
            style="display:grid;gap:8px;padding:10px;border:1px solid var(--border-light);\
                   border-radius:var(--radius-sm);background:var(--panel-deep);"
        >
            {if let Some(src) = preview_src {
                view! {
                    <img
                        src=src
                        alt=title.clone()
                        loading="lazy"
                        style="width:100%;aspect-ratio:4/3;object-fit:cover;border-radius:var(--radius-sm);\
                               background:var(--row-tint);"
                    />
                }.into_any()
            } else {
                view! {
                    <div
                        style="display:grid;place-items:center;aspect-ratio:4/3;border-radius:var(--radius-sm);\
                               background:var(--row-tint);color:var(--muted);font-size:11px;"
                    >
                        "Preview unavailable"
                    </div>
                }.into_any()
            }}
            <div style="display:grid;gap:4px;">
                {if let Some(href) = page_href {
                    view! {
                        <a href=href target="_blank" rel="noopener" style="color:var(--text);font-weight:700;">
                            {title}
                        </a>
                    }.into_any()
                } else {
                    view! { <strong style="color:var(--text);">{title}</strong> }.into_any()
                }}
                <div style="font-size:11px;color:var(--muted);">{usage_summary}</div>
                {render_signature_block("Before", entry.before_signatures.clone())}
                {render_signature_block("After", entry.after_signatures.clone())}
            </div>
        </article>
    }
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
        return view! { <span></span> }.into_any();
    }

    view! {
        <div style="display:grid;gap:2px;">
            <span style="font-size:10px;font-weight:700;text-transform:uppercase;color:var(--muted);">
                {label}
            </span>
            <span style="font-size:11px;color:var(--text);line-height:1.45;">
                {signatures.join(" · ")}
            </span>
        </div>
    }.into_any()
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
