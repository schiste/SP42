use leptos::prelude::*;
use sp42_core::{
    DiffHunkKind, DiffLineSpan, DiffMarker, DiffMode, DiffMoveRole, DiffSegment, DiffSegmentKind,
    InlineSpan, StructuredDiff,
};

/// Action triggered from the diff context menu.
#[derive(Debug, Clone)]
pub struct TagAction {
    pub text: String,
}

/// Reserved for future inline diff editing affordances.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EditAction {
    pub original_text: String,
    pub new_text: String,
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

#[derive(Clone)]
struct HunkData {
    kind: DiffHunkKind,
    before: Option<DiffLineSpan>,
    after: Option<DiffLineSpan>,
    before_section: Option<String>,
    after_section: Option<String>,
    markers: Vec<DiffMarker>,
    notes: Vec<String>,
    move_group: Option<usize>,
    move_role: Option<DiffMoveRole>,
    segments: Vec<SegmentData>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffDisplayMode {
    SideBySide,
    Unified,
}

#[derive(Clone)]
struct SideBySideRow {
    left: Option<SideBySideCell>,
    right: Option<SideBySideCell>,
}

#[derive(Clone)]
struct SideBySideCell {
    kind: DiffSegmentKind,
    text: String,
    line_label: String,
    inline_highlights: Vec<InlineSpan>,
}

#[component]
pub fn DiffViewer(
    diff: Option<StructuredDiff>,
    #[prop(optional)] on_tag: Option<WriteSignal<Option<TagAction>>>,
    #[prop(optional)] on_edit: Option<WriteSignal<Option<EditAction>>>,
) -> impl IntoView {
    let (menu_pos, set_menu_pos) = signal(None::<(i32, i32, String)>);
    let (editing_line, set_editing_line) = signal(None::<(usize, String)>);
    let _ = (&on_edit, &editing_line, &set_editing_line);

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
    let (display_mode, set_display_mode) = signal(DiffDisplayMode::SideBySide);
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
    let hunk_data: Vec<HunkData> = diff
        .hunks
        .into_iter()
        .map(|hunk| HunkData {
            kind: hunk.kind,
            before: hunk.before,
            after: hunk.after,
            before_section: hunk.section.before,
            after_section: hunk.section.after,
            markers: hunk.markers,
            notes: hunk.notes,
            move_group: hunk.move_group,
            move_role: hunk.move_role,
            segments: hunk
                .segments
                .into_iter()
                .map(|segment| SegmentData {
                    kind: segment.kind,
                    text: segment.text,
                    before: segment.before,
                    after: segment.after,
                    inline_highlights: segment.inline_highlights,
                })
                .collect(),
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
                {if diff_mode == DiffMode::Lines {
                    view! {
                        <div style="display:flex;gap:4px;margin-inline-start:auto;">
                            <button
                                class="btn btn-ghost btn-compact"
                                style:opacity=move || if display_mode.get() == DiffDisplayMode::SideBySide { "1" } else { "0.7" }
                                on:click=move |_| set_display_mode.set(DiffDisplayMode::SideBySide)
                            >
                                "Side by side"
                            </button>
                            <button
                                class="btn btn-ghost btn-compact"
                                style:opacity=move || if display_mode.get() == DiffDisplayMode::Unified { "1" } else { "0.7" }
                                on:click=move |_| set_display_mode.set(DiffDisplayMode::Unified)
                            >
                                "Unified"
                            </button>
                        </div>
                    }.into_any()
                } else {
                    view! { <span style="margin-inline-start:auto;"></span> }.into_any()
                }}

                <button
                    class="btn btn-ghost btn-compact"
                    style="padding:2px 8px;font-size:11px;"
                    on:click=move |_| set_show_full.update(|v| *v = !*v)
                >
                    {move || if show_full.get() { "Show changes only" } else { "Show full diff" }}
                </button>
            </div>

            <div style="padding:10px;">
                {move || {
                    let has_menu = on_tag.is_some();
                    let render = |seg: &SegmentData, line: usize| {
                        render_segment_data(seg, line, diff_mode, has_menu, set_menu_pos, editing_line, set_editing_line, on_edit)
                    };
                    if diff_mode == DiffMode::Lines && display_mode.get() == DiffDisplayMode::SideBySide {
                        if show_full.get() {
                            render_side_by_side_rows(
                                build_side_by_side_rows(&seg_data),
                                diff_mode,
                                has_menu,
                                set_menu_pos,
                            )
                        } else if !hunk_data.is_empty() {
                            hunk_data
                                .iter()
                                .enumerate()
                                .map(|(index, hunk)| {
                                    render_hunk_side_by_side(
                                        hunk,
                                        index + 1,
                                        diff_mode,
                                        has_menu,
                                        set_menu_pos,
                                    )
                                })
                                .collect_view()
                                .into_any()
                        } else {
                            render_side_by_side_rows(
                                build_side_by_side_rows(&seg_data),
                                diff_mode,
                                has_menu,
                                set_menu_pos,
                            )
                        }
                    } else if show_full.get() {
                        seg_data
                            .iter()
                            .enumerate()
                            .map(|(idx, seg)| render(seg, idx + 1))
                            .collect_view()
                            .into_any()
                    } else if !hunk_data.is_empty() {
                        hunk_data
                            .iter()
                            .enumerate()
                            .map(|(index, hunk)| {
                                render_hunk(
                                    hunk,
                                    index + 1,
                                    diff_mode,
                                    has_menu,
                                    set_menu_pos,
                                    editing_line,
                                    set_editing_line,
                                    on_edit,
                                )
                            })
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

fn render_hunk(
    hunk: &HunkData,
    ordinal: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
    editing_line: ReadSignal<Option<(usize, String)>>,
    set_editing_line: WriteSignal<Option<(usize, String)>>,
    on_edit: Option<WriteSignal<Option<EditAction>>>,
) -> leptos::tachys::view::any_view::AnyView {
    view! {
        <section
            style="display:grid;gap:6px;margin-block-end:12px;padding:7px 0 0;\
                   border-block-start:1px solid var(--border-light);"
        >
            {render_hunk_header(hunk, ordinal)}
            <div style="display:grid;gap:2px;">
                {hunk
                    .segments
                    .iter()
                    .enumerate()
                    .map(|(index, segment)| {
                        let fallback = hunk
                            .before
                            .as_ref()
                            .map_or_else(
                                || hunk.after.as_ref().map_or(ordinal, |span| span.start_line + index),
                                |span| span.start_line + index,
                            );
                        render_segment_data(
                            segment,
                            fallback,
                            diff_mode,
                            has_menu,
                            set_menu_pos,
                            editing_line,
                            set_editing_line,
                            on_edit,
                        )
                    })
                    .collect_view()}
            </div>
        </section>
    }
    .into_any()
}

fn render_hunk_side_by_side(
    hunk: &HunkData,
    ordinal: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
) -> leptos::tachys::view::any_view::AnyView {
    let rows = build_side_by_side_rows(&hunk.segments, diff_mode);

    view! {
        <section
            style="display:grid;gap:6px;margin-block-end:12px;padding:7px 0 0;\
                   border-block-start:1px solid var(--border-light);"
        >
            {render_hunk_header(hunk, ordinal)}
            {render_side_by_side_rows(rows, diff_mode, has_menu, set_menu_pos)}
        </section>
    }
    .into_any()
}

fn render_hunk_header(hunk: &HunkData, ordinal: usize) -> leptos::tachys::view::any_view::AnyView {
    let title = hunk_title(hunk, ordinal);
    let section_label = hunk_section_label(hunk);
    let marker_badges = hunk
        .markers
        .iter()
        .map(diff_marker_label)
        .collect::<Vec<_>>();
    let move_badge = hunk.move_role.map(|role| match role {
        DiffMoveRole::Source => "Moved from here",
        DiffMoveRole::Target => "Moved to here",
    });
    let note = hunk.notes.first().cloned();

    view! {
        <header style="display:grid;gap:4px;padding:0 0 1px;">
            <div style="display:flex;align-items:center;gap:6px;flex-wrap:wrap;min-height:20px;">
                <strong style="font-size:11px;text-transform:uppercase;letter-spacing:0.06em;color:var(--muted);line-height:1.1;">
                    {title}
                </strong>
                <span style="font-size:10px;color:var(--subtle);line-height:1.2;">
                    {section_label}
                </span>
                {move || {
                    move_badge
                        .map(|badge| {
                            view! {
                                <span
                                    style="padding:1px 5px;border-radius:999px;font-size:9px;line-height:1.2;\
                                           background:rgba(59,130,246,.12);color:#bfdbfe;border:1px solid rgba(59,130,246,.22);"
                                >
                                    {badge}
                                </span>
                            }
                            .into_any()
                        })
                        .unwrap_or_else(|| view! { <span></span> }.into_any())
                }}
                {marker_badges
                    .iter()
                    .map(|label| {
                        let label = (*label).to_string();
                        view! {
                            <span
                                style="padding:1px 5px;border-radius:999px;font-size:9px;line-height:1.2;\
                                       background:rgba(248,250,252,.05);color:var(--muted);border:1px solid var(--border-light);"
                            >
                                {label}
                            </span>
                        }
                    })
                    .collect_view()}
            </div>
            {move || {
                note.clone()
                    .map(|text| {
                        view! {
                            <div style="font-size:10px;line-height:1.3;color:var(--muted);">
                                {text}
                            </div>
                        }
                        .into_any()
                    })
                    .unwrap_or_else(|| view! { <span></span> }.into_any())
            }}
        </header>
    }
    .into_any()
}

fn build_side_by_side_rows(segments: &[SegmentData], diff_mode: DiffMode) -> Vec<SideBySideRow> {
    let mut rows = Vec::new();
    let mut index = 0usize;

    while index < segments.len() {
        match segments[index].kind {
            DiffSegmentKind::Equal => {
                let left_cells = expand_segment_cells(&segments[index], false, diff_mode);
                let right_cells = expand_segment_cells(&segments[index], true, diff_mode);
                let row_count = left_cells.len().max(right_cells.len());
                for row_index in 0..row_count {
                    rows.push(SideBySideRow {
                        left: left_cells.get(row_index).cloned(),
                        right: right_cells.get(row_index).cloned(),
                    });
                }
                index += 1;
            }
            DiffSegmentKind::Delete | DiffSegmentKind::Insert => {
                let run_start = index;
                while index < segments.len() && segments[index].kind != DiffSegmentKind::Equal {
                    index += 1;
                }
                let change_run = &segments[run_start..index];
                let left_cells = change_run
                    .iter()
                    .filter(|segment| segment.kind == DiffSegmentKind::Delete)
                    .flat_map(|segment| expand_segment_cells(segment, false, diff_mode))
                    .collect::<Vec<_>>();
                let right_cells = change_run
                    .iter()
                    .filter(|segment| segment.kind == DiffSegmentKind::Insert)
                    .flat_map(|segment| expand_segment_cells(segment, true, diff_mode))
                    .collect::<Vec<_>>();
                let row_count = left_cells.len().max(right_cells.len());

                for row_index in 0..row_count {
                    rows.push(SideBySideRow {
                        left: left_cells.get(row_index).cloned(),
                        right: right_cells.get(row_index).cloned(),
                    });
                }
            }
        }
    }

    rows
}

fn render_side_by_side_rows(
    rows: Vec<SideBySideRow>,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
) -> leptos::tachys::view::any_view::AnyView {
    view! {
        <div style="display:grid;gap:2px;">
            <div
                style="display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:10px;\
                       padding:0 0 6px;border-block-end:1px solid var(--border-light);"
            >
                <div style="font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:0.08em;color:var(--muted);">
                    "Before"
                </div>
                <div style="font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:0.08em;color:var(--muted);">
                    "After"
                </div>
            </div>
            {rows
                .into_iter()
                .enumerate()
                .map(|(index, row)| render_side_by_side_row(&row, index + 1, diff_mode, has_menu, set_menu_pos))
                .collect_view()}
        </div>
    }
    .into_any()
}

fn render_side_by_side_row(
    row: &SideBySideRow,
    fallback_line_num: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
) -> leptos::tachys::view::any_view::AnyView {
    view! {
        <div style="display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:10px;">
            {render_side_by_side_cell(
                row.left.as_ref(),
                fallback_line_num,
                diff_mode,
                has_menu,
                set_menu_pos,
                false,
            )}
            {render_side_by_side_cell(
                row.right.as_ref(),
                fallback_line_num,
                diff_mode,
                has_menu,
                set_menu_pos,
                true,
            )}
        </div>
    }
    .into_any()
}

fn render_side_by_side_cell(
    cell: Option<&SideBySideCell>,
    _fallback_line_num: usize,
    _diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
    allow_insert_menu: bool,
) -> leptos::tachys::view::any_view::AnyView {
    let Some(cell) = cell else {
        return view! {
            <div
                style="min-height:1.2em;border-radius:var(--radius-sm);background:rgba(255,255,255,0.02);\
                       border:1px solid rgba(255,255,255,0.03);"
            ></div>
        }
        .into_any();
    };

    let (class, prefix) = match cell.kind {
        DiffSegmentKind::Delete => ("diff-delete", "-"),
        DiffSegmentKind::Insert => ("diff-insert", "+"),
        DiffSegmentKind::Equal => ("diff-equal", " "),
    };
    let aria = match cell.kind {
        DiffSegmentKind::Delete => "Removed: ",
        DiffSegmentKind::Insert => "Added: ",
        DiffSegmentKind::Equal => "",
    };
    let aria_text = format!("{aria}{}", cell.text.trim_end());
    let text = cell.text.clone();
    let has_highlights = !cell.inline_highlights.is_empty();
    let menu_text = text.clone();
    let is_insert = allow_insert_menu && cell.kind == DiffSegmentKind::Insert;

    let contextmenu = move |ev: leptos::ev::MouseEvent| {
        if is_insert && has_menu {
            ev.prevent_default();
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
        <div
            class="diff-line"
            aria-label=aria_text
            on:contextmenu=contextmenu
            style="border-radius:var(--radius-sm);overflow:hidden;"
        >
            <span
                class="diff-line-num"
                aria-hidden="true"
                style="width:56px;font-family:var(--font-mono);"
            >
                {cell.line_label.clone()}
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
                    cell
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

fn hunk_title(hunk: &HunkData, ordinal: usize) -> String {
    let base = match hunk.kind {
        DiffHunkKind::Modification => format!("Hunk {ordinal}"),
        DiffHunkKind::Addition => format!("Addition {ordinal}"),
        DiffHunkKind::Removal => format!("Removal {ordinal}"),
    };

    if let Some(group) = hunk.move_group {
        format!("{base} · move #{group}")
    } else {
        base
    }
}

fn expand_segment_cells(
    segment: &SegmentData,
    use_after_span: bool,
    diff_mode: DiffMode,
) -> Vec<SideBySideCell> {
    let span = if use_after_span {
        segment.after.as_ref()
    } else {
        segment.before.as_ref()
    };
    let line_labels = build_line_labels(span, diff_mode);
    let text_lines = split_text_lines(&segment.text);
    let highlight_lines = split_inline_highlights_by_line(&segment.inline_highlights);
    let cell_count = line_labels
        .len()
        .max(text_lines.len())
        .max(highlight_lines.len());

    (0..cell_count.max(1))
        .map(|index| SideBySideCell {
            kind: segment.kind,
            text: text_lines.get(index).cloned().unwrap_or_default(),
            line_label: line_labels.get(index).cloned().unwrap_or_default(),
            inline_highlights: highlight_lines.get(index).cloned().unwrap_or_default(),
        })
        .collect()
}

fn build_line_labels(span: Option<&DiffLineSpan>, diff_mode: DiffMode) -> Vec<String> {
    match diff_mode {
        DiffMode::Chars => Vec::new(),
        DiffMode::Lines => match span {
            Some(span) => (0..span.line_count)
                .map(|offset| (span.start_line + offset).to_string())
                .collect(),
            None => Vec::new(),
        },
    }
}

fn split_text_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    text.split_inclusive('\n')
        .map(ToString::to_string)
        .collect::<Vec<_>>()
}

fn split_inline_highlights_by_line(spans: &[InlineSpan]) -> Vec<Vec<InlineSpan>> {
    if spans.is_empty() {
        return Vec::new();
    }

    let mut lines = vec![Vec::new()];

    for span in spans {
        let pieces = split_text_lines(&span.text);
        for (index, piece) in pieces.into_iter().enumerate() {
            if index > 0 {
                lines.push(Vec::new());
            }
            if !piece.is_empty() {
                lines.last_mut().expect("line bucket").push(InlineSpan {
                    kind: span.kind,
                    text: piece,
                });
            }
        }
    }

    while lines.last().is_some_and(Vec::is_empty) && lines.len() > 1 {
        lines.pop();
    }

    lines
}

fn hunk_section_label(hunk: &HunkData) -> String {
    match (
        hunk.before_section.as_deref(),
        hunk.after_section.as_deref(),
    ) {
        (Some(before), Some(after)) if before == after => before.to_string(),
        (Some(before), Some(after)) => format!("{before} → {after}"),
        (Some(before), None) => before.to_string(),
        (None, Some(after)) => after.to_string(),
        (None, None) => "Lead".to_string(),
    }
}

fn diff_marker_label(marker: &DiffMarker) -> &'static str {
    match marker {
        DiffMarker::References => "refs",
        DiffMarker::Category => "categories",
        DiffMarker::Interwiki => "interwiki",
        DiffMarker::Template => "templates",
        DiffMarker::Media => "media",
        DiffMarker::Heading => "heading",
    }
}

fn render_segment_data(
    segment: &SegmentData,
    fallback_line_num: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
    editing_line: ReadSignal<Option<(usize, String)>>,
    set_editing_line: WriteSignal<Option<(usize, String)>>,
    on_edit: Option<WriteSignal<Option<EditAction>>>,
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

    let line_idx = fallback_line_num;
    let original_for_edit = text.clone();
    let has_edit = on_edit.is_some();

    view! {
        {move || {
            if let Some((edit_idx, _)) = editing_line.get() {
                if edit_idx == line_idx {
                    let orig = text.clone();
                    let cancel = move |_| set_editing_line.set(None);
                    let save = {
                        let orig = orig.clone();
                        move |_: leptos::ev::Event| {
                            #[cfg(target_arch = "wasm32")]
                            {
                                use wasm_bindgen::JsCast;
                                let new_text = web_sys::window()
                                    .and_then(|w| w.document())
                                    .and_then(|d| d.get_element_by_id(&format!("sp42-edit-{line_idx}")))
                                    .and_then(|el| el.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
                                    .map(|ta| ta.value())
                                    .unwrap_or_default();
                                if let Some(on_edit) = on_edit {
                                    on_edit.set(Some(EditAction {
                                        original_text: orig.trim_end().to_string(),
                                        new_text: new_text.trim_end().to_string(),
                                    }));
                                }
                                set_editing_line.set(None);
                            }
                        }
                    };
                    let keydown = {
                        let save_fn = save.clone();
                        move |ev: leptos::ev::KeyboardEvent| {
                            if ev.key() == "Escape" {
                                set_editing_line.set(None);
                            }
                            if ev.key() == "Enter" && ev.ctrl_key() {
                                ev.prevent_default();
                                save_fn(ev.into());
                            }
                        }
                    };
                    return view! {
                        <div class="diff-edit-container">
                            <textarea
                                id=format!("sp42-edit-{line_idx}")
                                class="diff-edit-textarea"
                                rows="4"
                                on:keydown=keydown
                            >
                                {text.trim_end().to_string()}
                            </textarea>
                            <div class="diff-edit-actions">
                                <button class="btn btn-success btn-compact" on:click=move |e| save(e.into())>
                                    "Save edit (Ctrl+Enter)"
                                </button>
                                <button class="btn btn-ghost btn-compact" on:click=cancel>
                                    "Cancel (Esc)"
                                </button>
                            </div>
                        </div>
                    }.into_any();
                }
            }

            let contextmenu = {
                let menu_text = menu_text.clone();
                move |ev: leptos::ev::MouseEvent| {
                    if is_insert && has_menu {
                        ev.prevent_default();
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
                }
            };
            let dblclick = {
                let original_for_edit = original_for_edit.clone();
                move |_: leptos::ev::MouseEvent| {
                    if has_edit {
                        set_editing_line
                            .set(Some((line_idx, original_for_edit.trim_end().to_string())));
                    }
                }
            };

            view! {
                <div class="diff-line" aria-label=aria_text.clone() on:contextmenu=contextmenu on:dblclick=dblclick>
                    <span class="diff-line-num" aria-hidden="true" style="width:56px;font-family:var(--font-mono);">
                        {before_line.clone()}
                    </span>
                    <span class="diff-line-num" aria-hidden="true" style="width:56px;font-family:var(--font-mono);">
                        {after_line.clone()}
                    </span>
                    <span style="width:10px;color:var(--subtle);flex-shrink:0;user-select:none;">{prefix}</span>
                    <pre class=class dir="auto" style="margin:0;flex:1;white-space:pre-wrap;word-break:break-all;unicode-bidi:plaintext;">
                        {if has_highlights {
                            segment.inline_highlights.iter().map(|span| {
                                let hs = match span.kind {
                                    DiffSegmentKind::Delete => "background:rgba(239,68,68,.35);border-radius:2px;",
                                    DiffSegmentKind::Insert => "background:rgba(34,197,94,.35);border-radius:2px;",
                                    DiffSegmentKind::Equal => "",
                                };
                                let t = span.text.clone();
                                if hs.is_empty() { view! { <span>{t}</span> }.into_any() }
                                else { view! { <mark style=hs>{t}</mark> }.into_any() }
                            }).collect_view().into_any()
                        } else {
                            view! { <span>{text.clone()}</span> }.into_any()
                        }}
                    </pre>
                </div>
            }.into_any()
        }}
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

    use super::{
        SegmentData, SegmentVisibility, build_side_by_side_rows, compute_visibility,
        format_line_label,
    };

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

    fn segment_data(
        kind: DiffSegmentKind,
        text: &str,
        before: Option<(usize, usize)>,
        after: Option<(usize, usize)>,
    ) -> SegmentData {
        SegmentData {
            kind,
            text: text.to_string(),
            before: before.map(|(start_line, line_count)| DiffLineSpan {
                start_line,
                line_count,
            }),
            after: after.map(|(start_line, line_count)| DiffLineSpan {
                start_line,
                line_count,
            }),
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

    #[test]
    fn side_by_side_rows_align_asymmetrical_change_runs_by_line() {
        let rows = build_side_by_side_rows(
            &[
                segment_data(
                    DiffSegmentKind::Delete,
                    "old a\nold b\n",
                    Some((10, 2)),
                    None,
                ),
                segment_data(DiffSegmentKind::Insert, "new a\n", None, Some((10, 1))),
                segment_data(
                    DiffSegmentKind::Insert,
                    "new b\nnew c\n",
                    None,
                    Some((11, 2)),
                ),
            ],
            DiffMode::Lines,
        );

        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0].left.as_ref().map(|cell| cell.line_label.as_str()),
            Some("10")
        );
        assert_eq!(
            rows[0].right.as_ref().map(|cell| cell.line_label.as_str()),
            Some("10")
        );
        assert_eq!(
            rows[1].left.as_ref().map(|cell| cell.line_label.as_str()),
            Some("11")
        );
        assert_eq!(
            rows[1].right.as_ref().map(|cell| cell.line_label.as_str()),
            Some("11")
        );
        assert!(rows[2].left.is_none());
        assert_eq!(
            rows[2].right.as_ref().map(|cell| cell.line_label.as_str()),
            Some("12")
        );
    }

    #[test]
    fn side_by_side_rows_split_equal_segments_into_aligned_lines() {
        let rows = build_side_by_side_rows(
            &[segment_data(
                DiffSegmentKind::Equal,
                "same a\nsame b\n",
                Some((4, 2)),
                Some((8, 2)),
            )],
            DiffMode::Lines,
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].left.as_ref().map(|cell| cell.line_label.as_str()),
            Some("4")
        );
        assert_eq!(
            rows[0].right.as_ref().map(|cell| cell.line_label.as_str()),
            Some("8")
        );
        assert_eq!(
            rows[1].left.as_ref().map(|cell| cell.line_label.as_str()),
            Some("5")
        );
        assert_eq!(
            rows[1].right.as_ref().map(|cell| cell.line_label.as_str()),
            Some("9")
        );
    }
}
