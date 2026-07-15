use leptos::{html, prelude::*};
use sp42_core::{
    DiffHunkKind, DiffLineSpan, DiffMarker, DiffMode, DiffMoveRole, DiffSegment, DiffSegmentKind,
    InlineSpan, StructuredDiff,
};
use sp42_ui::{
    Button, ButtonProps, ButtonSurface, Card, CardProps, Density, DiffBadge, DiffBadgeProps,
    DiffBody, DiffBodyProps, DiffContextMenu, DiffContextMenuItem, DiffContextMenuItemProps,
    DiffContextMenuProps, DiffEditPanel, DiffEditPanelProps, DiffEmptyCell, DiffEmptyCellProps,
    DiffHunk, DiffHunkHeader, DiffHunkHeaderProps, DiffHunkProps, DiffInlineMark,
    DiffInlineMarkProps, DiffLine, DiffLineProps, DiffLineState, DiffModeLabel, DiffModeLabelProps,
    DiffRows, DiffRowsProps, DiffSeparator, DiffSeparatorProps, DiffSplitHeader,
    DiffSplitHeaderProps, DiffSplitRow, DiffSplitRowProps, DiffState, DiffStateProps, DiffStatsBar,
    DiffStatsBarProps, DiffTone, DiffViewerShell, DiffViewerShellProps, Gap, Grid, GridColumns,
    GridProps, RenderedHighlightTone, RenderedHtmlHost, RenderedHtmlHostProps, SectionHeader,
    SectionHeaderProps, Size, Spacer, Stack, StackProps, Surface, Text, TextProps, Tone,
};

#[cfg(target_arch = "wasm32")]
use crate::components::rendered_highlight::find_rendered_highlight_matches;
use crate::components::rendered_highlight::{
    RenderedHighlightPhrase, RenderedHighlightSource, collect_rendered_highlight_phrases,
};
use crate::components::rendered_hunk_preview::{
    RenderedHunkContext, RenderedHunkPreviewController, create_rendered_hunk_preview_controller,
};

use super::ui_children;

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
    #[prop(optional)] wiki_id: Option<String>,
    #[prop(optional)] rev_id: Option<u64>,
    #[prop(optional)] old_rev_id: Option<u64>,
    #[prop(optional)] on_tag: Option<WriteSignal<Option<TagAction>>>,
    #[prop(optional)] on_edit: Option<WriteSignal<Option<EditAction>>>,
) -> impl IntoView {
    let (menu_pos, set_menu_pos) = signal(None::<(i32, i32, String)>);
    let (editing_line, set_editing_line) = signal(None::<(usize, String)>);
    let rendered_hunks = create_rendered_hunk_preview_controller();
    let _ = (&on_edit, &editing_line, &set_editing_line);

    let Some(diff) = diff else {
        return DiffState(DiffStateProps::new(
            "Diff viewer",
            "No diff available for this edit.",
        ))
        .into_any();
    };

    if diff.segments.is_empty() {
        return DiffState(DiffStateProps::new(
            "Diff viewer",
            "No content change (page move, protection, or tag-only edit).",
        ))
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
    let rendered_context = old_rev_id.and_then(|old_rev_id| {
        Some(RenderedHunkContext::new(
            wiki_id.clone()?,
            rev_id?,
            old_rev_id,
        ))
    });

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

    DiffViewerShell(DiffViewerShellProps::new(
        "Diff viewer",
        ui_children(move || {
            view! {
            {render_diff_stats_bar(
                stats_added,
                stats_removed,
                stats_unchanged,
                diff_mode,
                mode_label,
                show_full,
                set_show_full,
                display_mode,
                set_display_mode,
            )}

            {DiffBody(DiffBodyProps::new(ui_children(move || view! {
                {move || {
                    let has_menu = on_tag.is_some();
                    let render = |seg: &SegmentData, line: usize| {
                        render_segment_data(seg, line, diff_mode, has_menu, set_menu_pos, editing_line, set_editing_line, on_edit)
                    };
                    if diff_mode == DiffMode::Lines && display_mode.get() == DiffDisplayMode::SideBySide {
                        if show_full.get() {
                            render_side_by_side_rows(
                                build_side_by_side_rows(&seg_data, diff_mode),
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
                                        index,
                                        diff_mode,
                                        has_menu,
                                        set_menu_pos,
                                        rendered_context.clone(),
                                        rendered_hunks,
                                    )
                                })
                                .collect_view()
                                .into_any()
                        } else {
                            render_side_by_side_rows(
                                build_side_by_side_rows(&seg_data, diff_mode),
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
                                    index,
                                    diff_mode,
                                    has_menu,
                                    set_menu_pos,
                                    editing_line,
                                    set_editing_line,
                                    on_edit,
                                    rendered_context.clone(),
                                    rendered_hunks,
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
                                    DiffSeparator(DiffSeparatorProps::new(format!(
                                        "... {n} unchanged lines ..."
                                    )))
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
            }.into_any())))}
            {move || {
                let Some((x, y, text)) = menu_pos.get() else {
                    return ().into_any();
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
                DiffContextMenu(
                    DiffContextMenuProps::new(
                        x,
                        y,
                        ui_children(move || view! {
                            {DiffContextMenuItem(
                                DiffContextMenuItemProps::new("Citation needed")
                                    .on_click(citation_click)
                            )}
                        }.into_any()),
                    )
                    .on_backdrop_click(dismiss),
                )
                .into_any()
            }}
        }
        .into_any()
        }),
    ))
    .into_any()
}

fn render_diff_stats_bar(
    stats_added: usize,
    stats_removed: usize,
    stats_unchanged: usize,
    diff_mode: DiffMode,
    mode_label: &'static str,
    show_full: ReadSignal<bool>,
    set_show_full: WriteSignal<bool>,
    display_mode: ReadSignal<DiffDisplayMode>,
    set_display_mode: WriteSignal<DiffDisplayMode>,
) -> AnyView {
    DiffStatsBar(DiffStatsBarProps::new(ui_children(move || {
        view! {
            {Text(
                TextProps::new(ui_children(move || {
                    view! { {format!("+{stats_added} added")} }.into_any()
                }))
                .with_tone(Tone::Success),
            )}
            {Text(
                TextProps::new(ui_children(move || {
                    view! { {format!("-{stats_removed} removed")} }.into_any()
                }))
                .with_tone(Tone::Danger),
            )}
            {Text(
                TextProps::new(ui_children(move || {
                    view! { {format!("{stats_unchanged} unchanged")} }.into_any()
                }))
                .with_tone(Tone::Muted),
            )}
            {DiffModeLabel(DiffModeLabelProps::new(mode_label))}
            {render_display_mode_controls(diff_mode, display_mode, set_display_mode)}
            {move || {
                let label = if show_full.get() {
                    "Show changes only"
                } else {
                    "Show full diff"
                };
                Button(
                    ButtonProps::new(label)
                        .with_surface(ButtonSurface::Ghost)
                        .with_density(Density::Compact)
                        .with_size(Size::Small)
                        .on_click(move |_| set_show_full.update(|value| *value = !*value)),
                )
                .into_any()
            }}
        }
        .into_any()
    })))
    .into_any()
}

fn render_display_mode_controls(
    diff_mode: DiffMode,
    display_mode: ReadSignal<DiffDisplayMode>,
    set_display_mode: WriteSignal<DiffDisplayMode>,
) -> AnyView {
    if diff_mode != DiffMode::Lines {
        return Spacer().into_any();
    }

    sp42_ui::Toolbar(sp42_ui::ToolbarProps::new(
        "Diff display mode",
        ui_children(move || {
            view! {
                {move || {
                    let emphasis = if display_mode.get() == DiffDisplayMode::SideBySide {
                        ButtonSurface::Subtle
                    } else {
                        ButtonSurface::Ghost
                    };
                    Button(
                        ButtonProps::new("Side by side")
                            .with_surface(emphasis)
                            .with_density(Density::Compact)
                            .with_size(Size::Small)
                            .on_click(move |_| set_display_mode.set(DiffDisplayMode::SideBySide)),
                    )
                    .into_any()
                }}
                {move || {
                    let emphasis = if display_mode.get() == DiffDisplayMode::Unified {
                        ButtonSurface::Subtle
                    } else {
                        ButtonSurface::Ghost
                    };
                    Button(
                        ButtonProps::new("Unified")
                            .with_surface(emphasis)
                            .with_density(Density::Compact)
                            .with_size(Size::Small)
                            .on_click(move |_| set_display_mode.set(DiffDisplayMode::Unified)),
                    )
                    .into_any()
                }}
            }
            .into_any()
        }),
    ))
    .into_any()
}

fn render_hunk(
    hunk: &HunkData,
    ordinal: usize,
    hunk_index: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
    editing_line: ReadSignal<Option<(usize, String)>>,
    set_editing_line: WriteSignal<Option<(usize, String)>>,
    on_edit: Option<WriteSignal<Option<EditAction>>>,
    rendered_context: Option<RenderedHunkContext>,
    rendered_hunks: RenderedHunkPreviewController,
) -> leptos::tachys::view::any_view::AnyView {
    let rendered_toggle =
        render_rendered_hunk_toggle(hunk_index, rendered_context.clone(), rendered_hunks);
    let hunk_data = hunk.clone();
    let preview_hunk = hunk_data.clone();

    DiffHunk(DiffHunkProps::new(ui_children(move || {
        view! {
            {render_hunk_header(&hunk_data, ordinal, rendered_toggle)}
            {render_rendered_hunk_preview(
                &preview_hunk,
                hunk_index,
                rendered_context.clone(),
                rendered_hunks,
            )}
            {DiffRows(DiffRowsProps::new(ui_children(move || view! {
                {hunk_data
                    .segments
                    .iter()
                    .enumerate()
                    .map(|(index, segment)| {
                        let fallback = hunk_data
                            .before
                            .as_ref()
                            .map_or_else(
                                || hunk_data.after.as_ref().map_or(ordinal, |span| span.start_line + index),
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
            }.into_any())))}
        }
        .into_any()
    })))
    .into_any()
}

fn render_hunk_side_by_side(
    hunk: &HunkData,
    ordinal: usize,
    hunk_index: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
    rendered_context: Option<RenderedHunkContext>,
    rendered_hunks: RenderedHunkPreviewController,
) -> leptos::tachys::view::any_view::AnyView {
    let rows = build_side_by_side_rows(&hunk.segments, diff_mode);
    let rendered_toggle =
        render_rendered_hunk_toggle(hunk_index, rendered_context.clone(), rendered_hunks);
    let hunk_data = hunk.clone();
    let preview_hunk = hunk_data.clone();

    DiffHunk(DiffHunkProps::new(ui_children(move || {
        view! {
            {render_hunk_header(&hunk_data, ordinal, rendered_toggle)}
            {render_rendered_hunk_preview(
                &preview_hunk,
                hunk_index,
                rendered_context.clone(),
                rendered_hunks,
            )}
            {render_side_by_side_rows(rows, diff_mode, has_menu, set_menu_pos)}
        }
        .into_any()
    })))
    .into_any()
}

fn render_rendered_hunk_toggle(
    hunk_index: usize,
    rendered_context: Option<RenderedHunkContext>,
    rendered_hunks: RenderedHunkPreviewController,
) -> leptos::tachys::view::any_view::AnyView {
    let Some(context) = rendered_context else {
        return ().into_any();
    };

    let toggle_context = context.clone();

    view! {
        {move || {
            let label = if rendered_hunks.is_expanded(hunk_index) {
                "Hide rendered"
            } else {
                "Show rendered"
            };
            Button(
                ButtonProps::new(label)
                    .with_surface(ButtonSurface::Ghost)
                    .with_density(Density::Compact)
                    .with_size(Size::Small)
                    .on_click({
                        let toggle_context = toggle_context.clone();
                        move |_| rendered_hunks.toggle(toggle_context.clone(), hunk_index)
                    }),
            )
            .into_any()
        }}
    }
    .into_any()
}

fn render_rendered_hunk_preview(
    hunk: &HunkData,
    hunk_index: usize,
    rendered_context: Option<RenderedHunkContext>,
    rendered_hunks: RenderedHunkPreviewController,
) -> leptos::tachys::view::any_view::AnyView {
    if rendered_context.is_none() {
        return ().into_any();
    }

    let before_highlights = collect_rendered_highlight_phrases(
        rendered_highlight_sources(hunk),
        DiffSegmentKind::Delete,
    );
    let after_highlights = collect_rendered_highlight_phrases(
        rendered_highlight_sources(hunk),
        DiffSegmentKind::Insert,
    );

    view! {
        {move || {
            if !rendered_hunks.is_expanded(hunk_index) {
                return ().into_any();
            }

            if rendered_hunks.is_loading(hunk_index) {
                Card(
                    CardProps::new(ui_children(|| view! {
                        {Text(
                            TextProps::new(ui_children(|| view! { "Rendering hunk context..." }.into_any()))
                                .with_tone(Tone::Muted)
                                .with_size(Size::XSmall)
                        )}
                    }.into_any()))
                    .with_density(Density::Compact),
                )
                .into_any()
            } else if let Some(error) = rendered_hunks.error(hunk_index) {
                Card(
                    CardProps::new(ui_children(move || view! {
                        {Text(
                            TextProps::new(ui_children(|| view! { "Rendered preview unavailable" }.into_any()))
                                .with_tone(Tone::Danger)
                                .with_size(Size::XSmall)
                        )}
                        {Text(
                            TextProps::new(ui_children(move || view! { {error} }.into_any()))
                                .with_tone(Tone::Muted)
                                .with_size(Size::XSmall)
                        )}
                    }.into_any()))
                    .with_density(Density::Compact)
                    .with_surface(Surface::Danger),
                )
                .into_any()
            } else {
                let Some(preview) = rendered_hunks.preview(hunk_index) else {
                    return ().into_any();
                };

                let warnings = preview.warnings.clone();
                let before = preview.before.clone();
                let after = preview.after.clone();
                let before_highlights = before_highlights.clone();
                let after_highlights = after_highlights.clone();
                Card(
                    CardProps::new(ui_children(move || view! {
                        {Grid(
                            GridProps::new(ui_children(move || view! {
                                {Stack(
                                    StackProps::new(ui_children(move || view! {
                                        {SectionHeader(SectionHeaderProps::new(format!(
                                            "Before · {}",
                                            before.section_label.clone()
                                        )))}
                                        {Card(
                                            CardProps::new(ui_children(move || view! {
                                                <RenderedHtmlPane
                                                    html=before.html.clone()
                                                    highlight_phrases=before_highlights.clone()
                                                    highlight_tone=RenderedHighlightTone::Remove
                                                />
                                            }.into_any()))
                                            .with_density(Density::Compact)
                                        )}
                                    }.into_any()))
                                    .with_gap(Gap::Small)
                                )}
                                {Stack(
                                    StackProps::new(ui_children(move || view! {
                                        {SectionHeader(SectionHeaderProps::new(format!(
                                            "After · {}",
                                            after.section_label.clone()
                                        )))}
                                        {Card(
                                            CardProps::new(ui_children(move || view! {
                                                <RenderedHtmlPane
                                                    html=after.html.clone()
                                                    highlight_phrases=after_highlights.clone()
                                                    highlight_tone=RenderedHighlightTone::Add
                                                />
                                            }.into_any()))
                                            .with_density(Density::Compact)
                                        )}
                                    }.into_any()))
                                    .with_gap(Gap::Small)
                                )}
                            }.into_any()))
                            .with_columns(GridColumns::Two)
                        )}
                        {warnings
                            .into_iter()
                            .map(|warning| {
                                Text(
                                    TextProps::new(ui_children(move || view! { {warning} }.into_any()))
                                        .with_tone(Tone::Muted)
                                        .with_size(Size::XSmall),
                                )
                            })
                            .collect_view()}
                    }.into_any()))
                    .with_density(Density::Compact),
                )
                .into_any()
            }
        }}
    }
    .into_any()
}

fn rendered_highlight_sources(
    hunk: &HunkData,
) -> impl Iterator<Item = RenderedHighlightSource<'_>> {
    hunk.segments.iter().map(|segment| RenderedHighlightSource {
        kind: segment.kind,
        text: &segment.text,
        inline_highlights: &segment.inline_highlights,
    })
}

#[component]
fn RenderedHtmlPane(
    html: String,
    highlight_phrases: Vec<RenderedHighlightPhrase>,
    highlight_tone: RenderedHighlightTone,
) -> impl IntoView {
    let container_ref = NodeRef::<html::Div>::new();
    let highlight_class = highlight_tone.class_name();

    Effect::new(move |_| {
        if let Some(container) = container_ref.get() {
            // ACCEPTED DEVIATION from Constitution 10.2 ("Diff rendering uses
            // sanitized allowlist"): `html` is MediaWiki `action=parse` output
            // fetched server-side in `render_revision_section_side`. It is NOT
            // run through a local allowlist; we rely on MediaWiki's parser
            // sanitization upstream. This reliance is deliberate and documented
            // (see ADR-0026) — do not "fix" it by removing set_inner_html. If a
            // local allowlist is added, do it at the server fetch edge before
            // caching, not here in wasm.
            container.set_inner_html(&html);
            #[cfg(target_arch = "wasm32")]
            use wasm_bindgen::JsCast;
            #[cfg(target_arch = "wasm32")]
            apply_rendered_highlights(
                container.unchecked_into::<web_sys::Element>(),
                &highlight_phrases,
                highlight_class,
            );
        }
    });

    RenderedHtmlHost(RenderedHtmlHostProps::new(container_ref))
}

fn render_hunk_header(
    hunk: &HunkData,
    ordinal: usize,
    rendered_toggle: leptos::tachys::view::any_view::AnyView,
) -> leptos::tachys::view::any_view::AnyView {
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

    DiffHunkHeader(DiffHunkHeaderProps::new(
        title,
        section_label,
        ui_children(move || {
            view! {
                {move_badge
                    .map(|badge| {
                        DiffBadge(
                            DiffBadgeProps::new(badge)
                                .with_tone(Tone::Accent)
                        )
                        .into_any()
                    })
                    .unwrap_or_else(|| ().into_any())}
                {marker_badges
                    .iter()
                    .map(|label| {
                        DiffBadge(DiffBadgeProps::new((*label).to_string()))
                    })
                    .collect_view()}
                {note
                    .map(|text| {
                        Text(
                            TextProps::new(ui_children(move || view! { {text} }.into_any()))
                                .with_tone(Tone::Muted)
                                .with_size(Size::XSmall)
                        )
                        .into_any()
                    })
                    .unwrap_or_else(|| ().into_any())}
                {rendered_toggle}
            }
            .into_any()
        }),
    ))
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
    DiffRows(DiffRowsProps::new(ui_children(move || {
        view! {
            {DiffSplitHeader(DiffSplitHeaderProps::new("Before", "After"))}
            {rows
                .into_iter()
                .enumerate()
                .map(|(index, row)| render_side_by_side_row(&row, index + 1, diff_mode, has_menu, set_menu_pos))
                .collect_view()}
        }
        .into_any()
    })))
    .into_any()
}

fn render_side_by_side_row(
    row: &SideBySideRow,
    fallback_line_num: usize,
    diff_mode: DiffMode,
    has_menu: bool,
    set_menu_pos: WriteSignal<Option<(i32, i32, String)>>,
) -> leptos::tachys::view::any_view::AnyView {
    let row = row.clone();

    DiffSplitRow(DiffSplitRowProps::new(ui_children(move || {
        view! {
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
        }
        .into_any()
    })))
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
        return DiffEmptyCell(DiffEmptyCellProps).into_any();
    };

    let (tone, prefix) = match cell.kind {
        DiffSegmentKind::Delete => (DiffTone::Delete, "-"),
        DiffSegmentKind::Insert => (DiffTone::Insert, "+"),
        DiffSegmentKind::Equal => (DiffTone::Equal, " "),
    };
    let aria = match cell.kind {
        DiffSegmentKind::Delete => "Removed: ",
        DiffSegmentKind::Insert => "Added: ",
        DiffSegmentKind::Equal => "",
    };
    let aria_text = format!("{aria}{}", cell.text.trim_end());
    let text = cell.text.clone();
    let inline_highlights = cell.inline_highlights.clone();
    let menu_text = text.clone();
    let is_insert = allow_insert_menu && cell.kind == DiffSegmentKind::Insert;
    let line_label = cell.line_label.clone();

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

    DiffLine(
        DiffLineProps::new(
            tone,
            prefix,
            aria_text,
            ui_children(move || render_inline_diff_content(text, inline_highlights)),
        )
        .with_line_label(line_label)
        .with_state(DiffLineState::Framed)
        .on_context_menu(contextmenu),
    )
    .into_any()
}

fn render_inline_diff_content(text: String, inline_highlights: Vec<InlineSpan>) -> AnyView {
    if inline_highlights.is_empty() {
        return DiffInlineMark(DiffInlineMarkProps::new(DiffTone::Equal, text));
    }

    inline_highlights
        .into_iter()
        .map(|span| DiffInlineMark(DiffInlineMarkProps::new(inline_diff_tone(&span), span.text)))
        .collect_view()
        .into_any()
}

fn inline_diff_tone(span: &InlineSpan) -> DiffTone {
    if span.text.trim().is_empty() {
        DiffTone::Equal
    } else {
        diff_tone(span.kind)
    }
}

fn diff_tone(kind: DiffSegmentKind) -> DiffTone {
    match kind {
        DiffSegmentKind::Delete => DiffTone::Delete,
        DiffSegmentKind::Insert => DiffTone::Insert,
        DiffSegmentKind::Equal => DiffTone::Equal,
    }
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

#[cfg(target_arch = "wasm32")]
fn apply_rendered_highlights(
    root: web_sys::Element,
    phrases: &[RenderedHighlightPhrase],
    highlight_class: &str,
) {
    #[allow(unused_imports)]
    use wasm_bindgen::JsCast;

    if phrases.is_empty() {
        return;
    }
    let Some(document) = crate::platform::globals::browser_document() else {
        return;
    };

    let mut text_nodes = Vec::new();
    collect_text_nodes(root.as_ref(), &mut text_nodes);

    for text_node in text_nodes {
        let Some(parent) = text_node.parent_element() else {
            continue;
        };
        if matches!(
            parent.tag_name().as_str(),
            "SCRIPT" | "STYLE" | "NOSCRIPT" | "MARK"
        ) {
            continue;
        }
        let original = text_node.data();
        let matches = find_rendered_highlight_matches(&original, phrases);
        if matches.is_empty() {
            continue;
        }

        let fragment = document.create_document_fragment();
        let mut cursor = 0usize;
        for (start, end) in matches {
            if start > cursor {
                let text = document.create_text_node(&original[cursor..start]);
                let _ = fragment.append_child(text.as_ref());
            }
            if let Ok(mark) = document.create_element("mark") {
                let _ = mark.set_attribute("class", highlight_class);
                mark.set_text_content(Some(&original[start..end]));
                let _ = fragment.append_child(mark.as_ref());
            }
            cursor = end;
        }
        if cursor < original.len() {
            let text = document.create_text_node(&original[cursor..]);
            let _ = fragment.append_child(text.as_ref());
        }
        let _ = parent.replace_child(fragment.as_ref(), text_node.as_ref());
    }
}

#[cfg(target_arch = "wasm32")]
fn collect_text_nodes(node: &web_sys::Node, nodes: &mut Vec<web_sys::Text>) {
    use wasm_bindgen::JsCast;

    if node.node_type() == web_sys::Node::TEXT_NODE {
        if let Ok(text) = node.clone().dyn_into::<web_sys::Text>() {
            nodes.push(text);
        }
        return;
    }

    let mut child = node.first_child();
    while let Some(current) = child {
        let next = current.next_sibling();
        collect_text_nodes(&current, nodes);
        child = next;
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
    let (tone, prefix) = match segment.kind {
        DiffSegmentKind::Delete => (DiffTone::Delete, "-"),
        DiffSegmentKind::Insert => (DiffTone::Insert, "+"),
        DiffSegmentKind::Equal => (DiffTone::Equal, " "),
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

    let highlights = segment.inline_highlights.clone();

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
                    let save_click = save.clone();
                    return DiffEditPanel(
                        DiffEditPanelProps::new(
                            format!("sp42-edit-{line_idx}"),
                            text.trim_end().to_string(),
                            ui_children(move || view! {
                                {Button(
                                    ButtonProps::new("Save edit (Ctrl+Enter)")
                                        .with_tone(Tone::Success)
                                        .with_density(Density::Compact)
                                        .on_click(move |e| save_click(e.into()))
                                )}
                                {Button(
                                    ButtonProps::new("Cancel (Esc)")
                                        .with_surface(ButtonSurface::Ghost)
                                        .with_density(Density::Compact)
                                        .on_click(cancel)
                                )}
                            }.into_any()),
                        )
                        .on_keydown(keydown),
                    ).into_any();
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
            let line_text = text.clone();
            let line_highlights = highlights.clone();

            DiffLine(
                DiffLineProps::new(
                    tone,
                    prefix,
                    aria_text.clone(),
                    ui_children(move || render_inline_diff_content(line_text, line_highlights)),
                )
                .with_before_label(before_line.clone())
                .with_after_label(after_line.clone())
                .on_context_menu(contextmenu)
                .on_double_click(dblclick),
            )
            .into_any()
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
    use sp42_ui::DiffTone;

    use super::{
        SegmentData, SegmentVisibility, build_side_by_side_rows, compute_visibility,
        format_line_label, inline_diff_tone,
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

    #[test]
    fn whitespace_only_inline_spans_are_not_emphasized() {
        assert_eq!(
            inline_diff_tone(&sp42_core::InlineSpan {
                kind: DiffSegmentKind::Delete,
                text: "               ".to_string(),
            }),
            DiffTone::Equal
        );
    }
}
