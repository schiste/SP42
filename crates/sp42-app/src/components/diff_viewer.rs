use leptos::prelude::*;
use sp42_core::{DiffLineSpan, DiffMode, DiffSegment, DiffSegmentKind, InlineSpan, StructuredDiff};

/// Action triggered from the diff context menu.
#[derive(Debug, Clone)]
pub struct TagAction {
    pub text: String,
}

/// Describes whether a segment should be rendered or collapsed into a separator.
#[derive(Clone)]
enum SegmentVisibility {
    Visible(usize),
    Separator(usize),
}

/// Walk the segments array and decide what to show.
///
/// A segment is *visible* when:
/// - It is Insert or Delete (always visible), **or**
/// - It is Equal and within `context_lines` positions of a non-Equal segment.
///
/// Consecutive hidden Equal segments are collapsed into a single separator.
fn compute_visibility(segments: &[DiffSegment], context_lines: usize) -> Vec<SegmentVisibility> {
    let len = segments.len();
    let mut visible = vec![false; len];

    for (i, seg) in segments.iter().enumerate() {
        if seg.kind != DiffSegmentKind::Equal {
            visible[i] = true;
            for j in i.saturating_sub(context_lines)..=i {
                visible[j] = true;
            }
            for j in i..=(i + context_lines).min(len - 1) {
                visible[j] = true;
            }
        }
    }

    let mut result = Vec::new();
    let mut hidden_count: usize = 0;

    for i in 0..len {
        if visible[i] {
            if hidden_count > 0 {
                result.push(SegmentVisibility::Separator(hidden_count));
                hidden_count = 0;
            }
            result.push(SegmentVisibility::Visible(i));
        } else {
            hidden_count += 1;
        }
    }
    if hidden_count > 0 {
        result.push(SegmentVisibility::Separator(hidden_count));
    }
    result
}

#[derive(Clone)]
struct SegmentData {
    kind: DiffSegmentKind,
    text: String,
    before: Option<DiffLineSpan>,
    after: Option<DiffLineSpan>,
    inline_highlights: Vec<InlineSpan>,
}

#[component]
pub fn DiffViewer(
    diff: Option<StructuredDiff>,
    #[prop(optional)] on_tag: Option<WriteSignal<Option<TagAction>>>,
) -> impl IntoView {
    let (menu_pos, set_menu_pos) = signal(None::<(i32, i32, String)>);

    let Some(diff) = diff else {
        return view! {
            <div role="main" aria-label="Diff viewer" class="grid-center text-muted" style="height:100%;">
                <p>"No diff available for this edit."</p>
            </div>
        }
        .into_any();
    };

    if diff.segments.is_empty() {
        return view! {
            <div role="main" aria-label="Diff viewer" class="grid-center text-muted" style="height:100%;">
                <p>"No content change (page move, protection, or tag-only edit)."</p>
            </div>
        }
        .into_any();
    }

    let stats_added = diff.stats.insert_segments;
    let stats_removed = diff.stats.delete_segments;
    let stats_unchanged = diff.stats.equal_segments;
    let diff_mode = diff.mode;
    let mode_label = match diff_mode {
        DiffMode::Lines => "line diff",
        DiffMode::Chars => "character diff",
    };

    let (show_full, set_show_full) = signal(false);
    let collapsed_plan = compute_visibility(&diff.segments, 3);

    let seg_data: Vec<SegmentData> = diff
        .segments
        .into_iter()
        .map(|s| SegmentData {
            kind: s.kind,
            text: s.text,
            before: s.before,
            after: s.after,
            inline_highlights: s.inline_highlights,
        })
        .collect();

    view! {
        <div role="main" aria-label="Diff viewer" class="diff-viewer">
            <div class="diff-stats">
                <span class="text-success">
                    {format!("+{stats_added} added")}
                </span>
                <span class="text-danger">
                    {format!("-{stats_removed} removed")}
                </span>
                <span>
                    {format!("{stats_unchanged} unchanged")}
                </span>
                <span
                    class="diff-mode"
                    style="text-transform:uppercase;letter-spacing:0.04em;"
                >
                    {mode_label}
                </span>

                <button
                    class="btn btn-ghost btn-compact"
                    style="margin-inline-start:auto;padding:2px 8px;font-size:11px;"
                    on:click=move |_| set_show_full.update(|v| *v = !*v)
                >
                    {move || if show_full.get() { "Show changes only" } else { "Show full diff" }}
                </button>
            </div>

            <div style="padding:10px;">
                {move || {
                    let has_menu = on_tag.is_some();
                    let render = |seg: &SegmentData, line: usize| {
                        render_segment_data(seg, line, diff_mode, has_menu, set_menu_pos)
                    };
                    if show_full.get() {
                        seg_data
                            .iter()
                            .enumerate()
                            .map(|(idx, seg)| render(seg, idx + 1))
                            .collect_view()
                            .into_any()
                    } else {
                        collapsed_plan
                            .iter()
                            .map(|vis| match vis {
                                SegmentVisibility::Separator(n) => {
                                    let n = *n;
                                    view! {
                                        <div class="diff-separator">
                                            {format!("... {n} unchanged lines ...")}
                                        </div>
                                    }
                                    .into_any()
                                }
                                SegmentVisibility::Visible(idx) => {
                                    render(&seg_data[*idx], *idx + 1)
                                }
                            })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
            {move || {
                let Some((x, y, text)) = menu_pos.get() else {
                    return view! { <span></span> }.into_any();
                };
                let dismiss = move |_| set_menu_pos.set(None);
                let citation_click = {
                    let text = text.clone();
                    move |_| {
                        if let Some(on_tag) = on_tag {
                            on_tag.set(Some(TagAction { text: text.clone() }));
                        }
                        set_menu_pos.set(None);
                    }
                };
                view! {
                    <div class="context-menu-backdrop" on:click=dismiss>
                        <div
                            class="context-menu"
                            style=format!("left:{x}px;top:{y}px;")
                            on:click=move |ev| ev.stop_propagation()
                        >
                            <button class="context-menu-item" on:click=citation_click>
                                "Citation needed"
                            </button>
                        </div>
                    </div>
                }.into_any()
            }}
        </div>
    }
    .into_any()
}

fn render_segment_data(
    segment: &SegmentData,
    fallback_line_num: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
) -> leptos::tachys::view::any_view::AnyView {
    let (class, prefix) = match segment.kind {
        DiffSegmentKind::Delete => ("diff-delete", "-"),
        DiffSegmentKind::Insert => ("diff-insert", "+"),
        DiffSegmentKind::Equal => ("diff-equal", " "),
    };
    let aria = match segment.kind {
        DiffSegmentKind::Delete => "Removed: ",
        DiffSegmentKind::Insert => "Added: ",
        DiffSegmentKind::Equal => "",
    };
    let aria_text = format!("{aria}{}", segment.text.trim_end());
    let text = segment.text.clone();
    let is_insert = segment.kind == DiffSegmentKind::Insert;
    let menu_text = text.clone();
    let before_line = format_line_label(segment.before.as_ref(), diff_mode, fallback_line_num);
    let after_line = format_line_label(segment.after.as_ref(), diff_mode, fallback_line_num);

    let has_highlights = !segment.inline_highlights.is_empty();

    let contextmenu = move |ev: leptos::ev::MouseEvent| {
        if is_insert && has_menu {
            ev.prevent_default();
            // Try to get selected text, fall back to full segment text
            #[cfg(target_arch = "wasm32")]
            {
                use wasm_bindgen::JsCast;
                let selection = web_sys::window()
                    .and_then(|w| {
                        let f = js_sys::Reflect::get(&w, &"getSelection".into()).ok()?;
                        let f = f.dyn_into::<js_sys::Function>().ok()?;
                        let sel = f.call0(&w).ok()?;
                        let text = sel.as_string().or_else(|| {
                            sel.dyn_ref::<js_sys::Object>()
                                .map(|o| o.to_string().as_string().unwrap_or_default())
                        })?;
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(text)
                        }
                    })
                    .unwrap_or_else(|| menu_text.trim().to_string());
                set_menu_pos.set(Some((ev.client_x(), ev.client_y(), selection)));
            }
        }
    };

    view! {
        <div class="diff-line" aria-label=aria_text on:contextmenu=contextmenu>
            <span
                class="diff-line-num"
                aria-hidden="true"
                style="width:56px;font-family:var(--font-mono);"
            >
                {before_line}
            </span>
            <span
                class="diff-line-num"
                aria-hidden="true"
                style="width:56px;font-family:var(--font-mono);"
            >
                {after_line}
            </span>
            <span style="width:10px;color:var(--subtle);flex-shrink:0;user-select:none;">
                {prefix}
            </span>
            <pre
                class=class
                dir="auto"
                style="margin:0;flex:1;white-space:pre-wrap;word-break:break-all;unicode-bidi:plaintext;"
            >
                {if has_highlights {
                    segment
                        .inline_highlights
                        .iter()
                        .map(|span| {
                            let highlight_style = match span.kind {
                                DiffSegmentKind::Delete => "background:rgba(239,68,68,.35);border-radius:2px;",
                                DiffSegmentKind::Insert => "background:rgba(34,197,94,.35);border-radius:2px;",
                                DiffSegmentKind::Equal => "",
                            };
                            let t = span.text.clone();
                            if highlight_style.is_empty() {
                                view! { <span>{t}</span> }.into_any()
                            } else {
                                view! { <mark style=highlight_style>{t}</mark> }.into_any()
                            }
                        })
                        .collect_view()
                        .into_any()
                } else {
                    view! { <span>{text}</span> }.into_any()
                }}
            </pre>
        </div>
    }
    .into_any()
}

fn format_line_label(
    span: Option<&DiffLineSpan>,
    diff_mode: DiffMode,
    fallback_line_num: usize,
) -> String {
    match diff_mode {
        DiffMode::Lines => match span {
            Some(span) if span.line_count > 1 => {
                format!(
                    "{}-{}",
                    span.start_line,
                    span.start_line + span.line_count - 1
                )
            }
            Some(span) => span.start_line.to_string(),
            None => String::new(),
        },
        DiffMode::Chars => fallback_line_num.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use sp42_core::{DiffLineSpan, DiffMode, DiffSegment, DiffSegmentKind};

    use super::{SegmentVisibility, compute_visibility, format_line_label};

    fn segment(kind: DiffSegmentKind) -> DiffSegment {
        DiffSegment {
            kind,
            text: match kind {
                DiffSegmentKind::Equal => "same\n".to_string(),
                DiffSegmentKind::Delete => "old\n".to_string(),
                DiffSegmentKind::Insert => "new\n".to_string(),
            },
            before: None,
            after: None,
            inline_highlights: Vec::new(),
        }
    }

    #[test]
    fn compute_visibility_collapses_distant_equal_runs() {
        let segments = vec![
            segment(DiffSegmentKind::Equal),
            segment(DiffSegmentKind::Equal),
            segment(DiffSegmentKind::Delete),
            segment(DiffSegmentKind::Insert),
            segment(DiffSegmentKind::Equal),
            segment(DiffSegmentKind::Equal),
            segment(DiffSegmentKind::Equal),
            segment(DiffSegmentKind::Equal),
        ];

        let visibility = compute_visibility(&segments, 1);

        assert!(matches!(visibility[0], SegmentVisibility::Separator(1)));
        assert!(matches!(visibility[1], SegmentVisibility::Visible(1)));
        assert!(matches!(visibility[2], SegmentVisibility::Visible(2)));
        assert!(matches!(visibility[3], SegmentVisibility::Visible(3)));
        assert!(matches!(visibility[4], SegmentVisibility::Visible(4)));
        assert!(matches!(visibility[5], SegmentVisibility::Separator(3)));
    }

    #[test]
    fn format_line_label_uses_real_line_ranges_for_line_diffs() {
        let span = DiffLineSpan {
            start_line: 12,
            line_count: 3,
        };

        assert_eq!(format_line_label(Some(&span), DiffMode::Lines, 99), "12-14");
        assert_eq!(
            format_line_label(
                Some(&DiffLineSpan {
                    start_line: 7,
                    line_count: 1,
                }),
                DiffMode::Lines,
                99
            ),
            "7"
        );
        assert_eq!(format_line_label(None, DiffMode::Lines, 99), "");
    }

    #[test]
    fn format_line_label_falls_back_for_char_diffs() {
        let span = DiffLineSpan {
            start_line: 12,
            line_count: 3,
        };

        assert_eq!(format_line_label(Some(&span), DiffMode::Chars, 4), "4");
        assert_eq!(format_line_label(None, DiffMode::Chars, 4), "4");
    }
}
