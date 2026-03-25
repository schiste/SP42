use leptos::prelude::*;
use sp42_core::{DiffSegment, DiffSegmentKind, StructuredDiff};

// ---------------------------------------------------------------------------
// Visibility helpers
// ---------------------------------------------------------------------------

/// Describes whether a segment should be rendered or collapsed into a separator.
#[derive(Clone)]
enum SegmentVisibility {
    /// Render the segment at `segments[index]`.
    Visible(usize),
    /// Replace a run of consecutive hidden Equal segments with a separator
    /// showing how many lines were hidden.
    Separator(usize),
}

/// Walk the segments array and decide what to show.
///
/// A segment is *visible* when:
/// - It is Insert or Delete (always visible), **or**
/// - It is Equal and within `context_lines` positions of a non-Equal segment.
///
/// Consecutive hidden Equal segments are collapsed into a single
/// [`SegmentVisibility::Separator`].
fn compute_visibility(segments: &[DiffSegment], context_lines: usize) -> Vec<SegmentVisibility> {
    let len = segments.len();
    let mut visible = vec![false; len];

    // Mark all non-Equal segments and their surrounding context as visible.
    for (i, seg) in segments.iter().enumerate() {
        if seg.kind != DiffSegmentKind::Equal {
            visible[i] = true;
            // Context *before* the change
            for j in i.saturating_sub(context_lines)..=i {
                visible[j] = true;
            }
            // Context *after* the change
            for j in i..=(i + context_lines).min(len - 1) {
                visible[j] = true;
            }
        }
    }

    // Build the visibility list, collapsing hidden runs into separators.
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

// ---------------------------------------------------------------------------
// Lightweight, Clone-able segment data for building views reactively.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct SegmentData {
    kind: DiffSegmentKind,
    text: String,
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[component]
pub fn DiffViewer(diff: Option<StructuredDiff>) -> impl IntoView {
    // --- No diff at all ------------------------------------------------
    let Some(diff) = diff else {
        return view! {
            <div
                role="main"
                aria-label="Diff viewer"
                style="display:grid;place-items:center;height:100%;color:#8b9fc0;"
            >
                <p>"No diff available for this edit."</p>
            </div>
        }
        .into_any();
    };

    // --- Empty segments (meta-only edit) --------------------------------
    if diff.segments.is_empty() {
        return view! {
            <div
                role="main"
                aria-label="Diff viewer"
                style="display:grid;place-items:center;height:100%;color:#8b9fc0;"
            >
                <p>"No content change (page move, protection, or tag-only edit)."</p>
            </div>
        }
        .into_any();
    }

    // --- Stats -----------------------------------------------------------
    let stats_added = diff.stats.insert_segments;
    let stats_removed = diff.stats.delete_segments;
    let stats_unchanged = diff.stats.equal_segments;

    // --- Collapsed / expanded toggle ------------------------------------
    let (show_full, set_show_full) = signal(false);

    // Pre-compute the collapsed visibility plan (3 lines of context).
    let collapsed_plan = compute_visibility(&diff.segments, 3);

    // Build lightweight, Clone-able segment data.
    let seg_data: Vec<SegmentData> = diff
        .segments
        .into_iter()
        .map(|s| SegmentData {
            kind: s.kind,
            text: s.text,
        })
        .collect();

    // --- Render -------------------------------------------------------------
    view! {
        <div
            role="main"
            aria-label="Diff viewer"
            style="overflow-y:auto;\
                   font-family:ui-monospace,SFMono-Regular,'Cascadia Code','Liberation Mono',Menlo,Monaco,Consolas,monospace;\
                   font-size:13px;line-height:1.4;"
        >
            // Diff stats summary + toggle
            <div style="display:flex;gap:10px;align-items:center;padding:4px 10px;\
                        font-size:11px;color:#8b9fc0;border-block-end:1px solid rgba(148,163,184,.14);">
                <span style="color:#22c55e;">
                    {format!("+{stats_added} added")}
                </span>
                <span style="color:#ef4444;">
                    {format!("-{stats_removed} removed")}
                </span>
                <span>
                    {format!("{stats_unchanged} unchanged")}
                </span>

                <button
                    style="margin-inline-start:auto;background:none;border:1px solid rgba(148,163,184,.25);\
                           border-radius:4px;padding:2px 8px;color:#8b9fc0;font-size:11px;cursor:pointer;"
                    on:click=move |_| set_show_full.update(|v| *v = !*v)
                >
                    {move || if show_full.get() { "Show changes only" } else { "Show full diff" }}
                </button>
            </div>

            // Diff segments
            <div style="padding:10px;">
                {move || {
                    if show_full.get() {
                        // Expanded: render every segment.
                        seg_data
                            .iter()
                            .enumerate()
                            .map(|(idx, seg)| render_segment_data(seg, idx + 1))
                            .collect_view()
                            .into_any()
                    } else {
                        // Collapsed: render only visible segments + separators.
                        collapsed_plan
                            .iter()
                            .map(|vis| match vis {
                                SegmentVisibility::Separator(n) => {
                                    let n = *n;
                                    view! {
                                        <div style="text-align:center;padding:4px;color:#4f6280;\
                                                    font-size:11px;border-block:1px dashed rgba(148,163,184,.14);">
                                            {format!("... {n} unchanged lines ...")}
                                        </div>
                                    }
                                    .into_any()
                                }
                                SegmentVisibility::Visible(idx) => {
                                    render_segment_data(&seg_data[*idx], *idx + 1)
                                }
                            })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Rendering helper – produces a single diff line from SegmentData
// ---------------------------------------------------------------------------

fn render_segment_data(
    segment: &SegmentData,
    line_num: usize,
) -> leptos::tachys::view::any_view::AnyView {
    let (style, prefix) = match segment.kind {
        DiffSegmentKind::Delete => (
            "background:rgba(239,68,68,.12);border-inline-start:3px solid #ef4444;padding-inline-start:7px;color:#fecaca;",
            "-",
        ),
        DiffSegmentKind::Insert => (
            "background:rgba(34,197,94,.12);border-inline-start:3px solid #22c55e;padding-inline-start:7px;color:#bbf7d0;",
            "+",
        ),
        DiffSegmentKind::Equal => ("padding-inline-start:10px;color:#8b9fc0;", " "),
    };
    let aria = match segment.kind {
        DiffSegmentKind::Delete => "Removed: ",
        DiffSegmentKind::Insert => "Added: ",
        DiffSegmentKind::Equal => "",
    };
    let aria_text = format!("{aria}{}", segment.text.trim_end());
    let text = segment.text.clone();

    view! {
        <div style="display:flex;" aria-label=aria_text>
            <span style="width:44px;text-align:end;padding-inline-end:7px;\
                         color:#4f6280;font-size:12px;user-select:none;\
                         flex-shrink:0;">
                {format!("{line_num}")}
            </span>
            <span style="width:10px;color:#4f6280;flex-shrink:0;user-select:none;">
                {prefix}
            </span>
            <pre style=format!("margin:0;flex:1;white-space:pre-wrap;word-break:break-all;{style}")>
                {text}
            </pre>
        </div>
    }
    .into_any()
}
