use super::{
    Align, BadgeHeaderProps, ButtonProps, ButtonState, ButtonSurface, ButtonType, CardProps,
    CheckboxProps, CodeBlockProps, ContextBarProps, ContextShellProps, DeltaTextProps, DeltaTone,
    Density, DiffBadgeProps, DiffBodyProps, DiffContextMenuItemProps, DiffContextMenuProps,
    DiffEditPanelProps, DiffEmptyCellProps, DiffHunkHeaderProps, DiffHunkProps,
    DiffInlineMarkProps, DiffLineProps, DiffLineState, DiffModeLabelProps, DiffRowsProps,
    DiffSeparatorProps, DiffSplitHeaderProps, DiffSplitRowProps, DiffStateProps, DiffStatsBarProps,
    DiffTone, DiffViewerShellProps, DisclosureProps, EmptyStateProps, ErrorStateProps, FieldProps,
    FilterDisclosureProps, Gap, GridColumns, GridProps, HeadingLevel, HeadingProps, InlineProps,
    InlineState, Justify, LinkProps, MediaCardProps, MediaGalleryPanelProps, MediaGroupProps,
    MediaPreviewProps, ModalProps, NavigationItemProps, NavigationItemState, NavigationPaneProps,
    PanelProps, RenderedHighlightTone, ScoreButtonProps, ScoreDetailItemProps,
    ScoreDetailsPanelProps, ScoreTextProps, ScoreTextState, ScoreTone, ScrollStackProps,
    SectionHeaderProps, SelectOption, SelectProps, SignatureBlockProps, Size, SpinnerProps,
    StackProps, State, StatusBadgeProps, Surface, TextElement, TextFamily, TextInputProps,
    TextInputType, TextListItemProps, TextListProps, TextOverflow, TextProps, TextWeight, Tone,
    ToolbarProps, ValueState, Width, badge_header, button, card, card_header, checkbox, code_block,
    context_bar, context_shell, delta_text, diff_badge, diff_body, diff_context_menu,
    diff_context_menu_item, diff_edit_panel, diff_empty_cell, diff_hunk, diff_hunk_header,
    diff_inline_mark, diff_line, diff_mode_label, diff_rows, diff_separator, diff_split_header,
    diff_split_row, diff_state, diff_stats_bar, diff_viewer_shell, disclosure, empty_state,
    error_state, field, filter_disclosure, grid, heading, inline, link, media_card,
    media_gallery_panel, media_group, media_preview, modal, navigation_item, navigation_pane,
    panel, rendered_html_host, score_button, score_detail_item, score_details_panel, score_text,
    scroll_stack, section_header, select, separator, signature_block, spacer, spinner, stack,
    status_badge, text, text_input, text_list, text_list_item, toolbar,
};
use leptos::{
    html,
    prelude::{Children, IntoAny, IntoView, NodeRef},
};

fn child() -> Children {
    Box::new(|| ().into_any())
}

fn view_is_constructible(_: impl IntoView) {}

#[test]
fn status_badge_tone_maps_to_design_system_class() {
    let badge = StatusBadgeProps::new("Ready").with_tone(Tone::Success);

    assert!(badge.class_name().contains("sp42-status-badge-success"));
    assert!(!badge.class_name().contains("style="));
}

#[test]
fn button_variants_are_composable_classes() {
    let button = ButtonProps::new("Rollback")
        .with_tone(Tone::Danger)
        .with_size(Size::Large)
        .with_density(Density::Comfortable)
        .with_state(ButtonState::Recommended);

    let class_name = button.class_name();

    assert!(class_name.contains("sp42-button"));
    assert!(class_name.contains("sp42-button-danger"));
    assert!(class_name.contains("sp42-size-large"));
    assert!(class_name.contains("sp42-density-comfortable"));
    assert!(class_name.contains("sp42-button-recommended"));
}

#[test]
fn grid_class_captures_layout_choices() {
    let grid = GridProps::new(Box::new(|| ().into_any()))
        .with_columns(GridColumns::AutoFit)
        .with_gap(Gap::Large);

    let class_name = grid.class_name();

    assert!(class_name.contains("sp42-grid-auto-fit"));
    assert!(class_name.contains("sp42-gap-lg"));
}

#[test]
fn primitive_variant_classes_are_semantic() {
    assert_eq!(Density::Compact.class_name(), "sp42-density-compact");
    assert_eq!(Density::Normal.class_name(), "sp42-density-normal");
    assert_eq!(
        Density::Comfortable.class_name(),
        "sp42-density-comfortable"
    );
    assert_eq!(Size::Small.class_name(), "sp42-size-small");
    assert_eq!(Size::Medium.class_name(), "sp42-size-medium");
    assert_eq!(Size::Large.class_name(), "sp42-size-large");
    assert_eq!(Width::Auto.class_name(), "sp42-control-auto");
    assert_eq!(Width::Short.class_name(), "sp42-control-short");
    assert_eq!(Width::Medium.class_name(), "sp42-control-medium");
    assert_eq!(Width::Full.class_name(), "sp42-control-full");
    assert_eq!(Surface::Default.class_name(), "sp42-surface-default");
    assert_eq!(Surface::Subtle.class_name(), "sp42-surface-subtle");
    assert_eq!(Surface::Raised.class_name(), "sp42-surface-raised");
    assert_eq!(Surface::Accent.class_name(), "sp42-surface-accent");
    assert_eq!(Surface::Success.class_name(), "sp42-surface-success");
    assert_eq!(Surface::Warning.class_name(), "sp42-surface-warning");
    assert_eq!(Surface::Danger.class_name(), "sp42-surface-danger");
    assert_eq!(Gap::None.class_name(), "sp42-gap-none");
    assert_eq!(Gap::XSmall.class_name(), "sp42-gap-xs");
    assert_eq!(Gap::Small.class_name(), "sp42-gap-sm");
    assert_eq!(Gap::Medium.class_name(), "sp42-gap-md");
    assert_eq!(Gap::Large.class_name(), "sp42-gap-lg");
    assert_eq!(Gap::XLarge.class_name(), "sp42-gap-xl");
    assert_eq!(Align::Start.class_name(), "sp42-align-start");
    assert_eq!(Align::Center.class_name(), "sp42-align-center");
    assert_eq!(Align::End.class_name(), "sp42-align-end");
    assert_eq!(Align::Stretch.class_name(), "sp42-align-stretch");
    assert_eq!(Align::Baseline.class_name(), "sp42-align-baseline");
    assert_eq!(Justify::Start.class_name(), "sp42-justify-start");
    assert_eq!(Justify::Center.class_name(), "sp42-justify-center");
    assert_eq!(Justify::End.class_name(), "sp42-justify-end");
    assert_eq!(Justify::Between.class_name(), "sp42-justify-between");
}

#[test]
fn control_variant_classes_are_semantic() {
    assert_eq!(Tone::Default.button_class_name(), "");
    assert_eq!(Tone::Accent.button_class_name(), "sp42-button-accent");
    assert_eq!(Tone::Success.button_class_name(), "sp42-button-success");
    assert_eq!(Tone::Warning.button_class_name(), "sp42-button-warning");
    assert_eq!(Tone::Danger.button_class_name(), "sp42-button-danger");
    assert_eq!(ButtonSurface::Solid.class_name(), "");
    assert_eq!(ButtonSurface::Subtle.class_name(), "sp42-button-subtle");
    assert_eq!(ButtonSurface::Ghost.class_name(), "sp42-button-ghost");
    assert_eq!(ButtonType::Button.as_str(), "button");
    assert_eq!(ButtonType::Submit.as_str(), "submit");
    assert_eq!(ButtonType::Reset.as_str(), "reset");
    assert_eq!(
        Tone::Default.status_class_name(),
        "sp42-status-badge-neutral"
    );
    assert_eq!(Tone::Info.status_class_name(), "sp42-status-badge-info");
    assert_eq!(
        Tone::Success.status_class_name(),
        "sp42-status-badge-success"
    );
    assert_eq!(
        Tone::Warning.status_class_name(),
        "sp42-status-badge-warning"
    );
    assert_eq!(Tone::Accent.status_class_name(), "sp42-status-badge-accent");
    assert_eq!(Tone::Danger.status_class_name(), "sp42-status-badge-danger");
    assert_eq!(GridColumns::One.class_name(), "sp42-grid-one");
    assert_eq!(GridColumns::Two.class_name(), "sp42-grid-two");
    assert_eq!(GridColumns::Three.class_name(), "sp42-grid-three");
    assert_eq!(GridColumns::Four.class_name(), "sp42-grid-four");
    assert_eq!(GridColumns::AutoFit.class_name(), "sp42-grid-auto-fit");
    assert_eq!(Size::Small.modal_class_name(), "sp42-modal-sm");
    assert_eq!(Size::Medium.modal_class_name(), "sp42-modal-md");
    assert_eq!(Size::Large.modal_class_name(), "sp42-modal-lg");
    assert_eq!(Size::Small.spinner_class_name(), "sp42-spinner-sm");
    assert_eq!(Size::Medium.spinner_class_name(), "sp42-spinner-md");
    assert_eq!(Size::Large.spinner_class_name(), "sp42-spinner-lg");
}

#[test]
fn text_and_score_variant_classes_are_semantic() {
    assert_eq!(Tone::Default.text_class_name(), "sp42-text-default");
    assert_eq!(Tone::Muted.text_class_name(), "text-muted");
    assert_eq!(Tone::Subtle.text_class_name(), "sp42-text-subtle");
    assert_eq!(Tone::Accent.text_class_name(), "text-accent");
    assert_eq!(Tone::Success.text_class_name(), "text-success");
    assert_eq!(Tone::Warning.text_class_name(), "text-warning");
    assert_eq!(Tone::Danger.text_class_name(), "text-danger");
    assert_eq!(Size::XSmall.text_class_name(), "sp42-text-xs");
    assert_eq!(Size::Small.text_class_name(), "sp42-text-sm");
    assert_eq!(Size::Medium.text_class_name(), "sp42-text-md");
    assert_eq!(Size::Large.text_class_name(), "sp42-text-lg");
    assert_eq!(TextWeight::Regular.class_name(), "sp42-weight-regular");
    assert_eq!(TextWeight::Medium.class_name(), "sp42-weight-medium");
    assert_eq!(TextWeight::Bold.class_name(), "sp42-weight-bold");
    assert_eq!(TextOverflow::Normal.class_name(), "");
    assert_eq!(TextOverflow::Truncate.class_name(), "truncate");
    assert_eq!(TextOverflow::ClampTwo.class_name(), "sp42-text-clamp-two");
    assert_eq!(
        TextOverflow::PreserveLines.class_name(),
        "sp42-text-preserve-lines"
    );
    assert_eq!(Size::Small.heading_class_name(), "sp42-heading-sm");
    assert_eq!(Size::Medium.heading_class_name(), "sp42-heading-md");
    assert_eq!(Size::Large.heading_class_name(), "sp42-heading-lg");
    assert_eq!(ScoreTone::for_score(12), ScoreTone::Low);
    assert_eq!(ScoreTone::for_score(42), ScoreTone::Medium);
    assert_eq!(ScoreTone::for_score(90), ScoreTone::High);
    assert_eq!(ScoreTone::Low.icon(), "\u{2713}");
    assert_eq!(ScoreTone::Medium.icon(), "?");
    assert_eq!(ScoreTone::High.icon(), "!!");
    assert_eq!(DeltaTone::for_delta(2), DeltaTone::Positive);
    assert_eq!(DeltaTone::for_delta(-2), DeltaTone::Negative);
    assert_eq!(DeltaTone::for_delta(0), DeltaTone::Neutral);
}

#[test]
fn complex_view_variant_classes_are_semantic() {
    assert_eq!(DiffTone::Insert.class_name(), "diff-insert");
    assert_eq!(DiffTone::Delete.class_name(), "diff-delete");
    assert_eq!(DiffTone::Equal.class_name(), "diff-equal");
    assert_eq!(
        DiffTone::Insert.inline_class_name(),
        "sp42-diff-inline-insert"
    );
    assert_eq!(
        DiffTone::Delete.inline_class_name(),
        "sp42-diff-inline-delete"
    );
    assert_eq!(DiffTone::Equal.inline_class_name(), "");
    assert_eq!(
        Tone::Default.diff_badge_class_name(),
        "sp42-diff-badge-neutral"
    );
    assert_eq!(
        Tone::Accent.diff_badge_class_name(),
        "sp42-diff-badge-accent"
    );
    assert_eq!(
        RenderedHighlightTone::Add.class_name(),
        "rendered-hunk-highlight-add"
    );
    assert_eq!(
        RenderedHighlightTone::Remove.class_name(),
        "rendered-hunk-highlight-remove"
    );
}

#[test]
fn prop_builders_compose_expected_class_names() {
    assert!(
        PanelProps::new(child())
            .with_surface(Surface::Raised)
            .with_density(Density::Comfortable)
            .class_name()
            .contains("sp42-surface-raised")
    );
    assert!(
        CardProps::new(child())
            .with_surface(Surface::Accent)
            .with_density(Density::Compact)
            .class_name()
            .contains("sp42-density-compact")
    );
    assert!(
        StackProps::new(child())
            .with_gap(Gap::XLarge)
            .with_align(Align::Baseline)
            .class_name()
            .contains("sp42-align-baseline")
    );
    assert!(
        InlineProps::new(child())
            .with_gap(Gap::XSmall)
            .with_align(Align::End)
            .with_justify(Justify::Between)
            .with_state(InlineState::NoWrap)
            .class_name()
            .contains("sp42-justify-between")
    );
    assert!(
        TextProps::new(child())
            .with_tone(Tone::Danger)
            .with_size(Size::Large)
            .with_weight(TextWeight::Bold)
            .with_overflow(TextOverflow::PreserveLines)
            .with_family(TextFamily::Mono)
            .class_name()
            .contains("mono")
    );
    assert!(
        HeadingProps::new(child())
            .with_level(HeadingLevel::Three)
            .with_size(Size::Large)
            .with_tone(Tone::Accent)
            .with_align(Align::Center)
            .class_name()
            .contains("sp42-heading-lg")
    );
    assert!(
        NavigationItemProps::new(child())
            .with_selected(true)
            .with_state(NavigationItemState::Subdued)
            .with_tone(ScoreTone::High)
            .class_name(true)
            .contains("sp42-score-high")
    );
    assert_eq!(
        DeltaTextProps::new(4).with_suffix("%").formatted_value(),
        "+4%"
    );
    assert_eq!(
        DeltaTextProps::new(-3)
            .with_suffix(" files")
            .formatted_value(),
        "-3 files"
    );
}

#[test]
fn control_components_construct_from_typed_props() {
    view_is_constructible(button(
        ButtonProps::new("Run")
            .with_tone(Tone::Accent)
            .with_size(Size::Small)
            .with_density(Density::Compact)
            .with_surface(ButtonSurface::Subtle)
            .with_type(ButtonType::Submit)
            .with_disabled(State::Static(false))
            .with_title("Run task")
            .with_aria_label("Run task")
            .with_keyshortcuts("Control+Enter")
            .on_click(|_| {}),
    ));
    view_is_constructible(status_badge(
        StatusBadgeProps::new("Ready")
            .with_tone(Tone::Info)
            .with_size(Size::Small),
    ));
    view_is_constructible(field(
        FieldProps::new("Name", child())
            .with_hint("Required")
            .with_error("Missing")
            .with_id("field-id")
            .required()
            .with_density(Density::Comfortable),
    ));
    view_is_constructible(text_input(
        TextInputProps::new("query")
            .with_name("query")
            .with_value(ValueState::from("value"))
            .with_placeholder("Search")
            .with_type(TextInputType::Search)
            .with_disabled(false)
            .required()
            .with_density(Density::Compact)
            .with_width(Width::Full)
            .on_input(|_| {})
            .on_change(|_| {}),
    ));
    view_is_constructible(select(
        SelectProps::new(
            "mode",
            vec![
                SelectOption::new("all", "All"),
                SelectOption::new("archived", "Archived").disabled(),
            ],
        )
        .with_name("mode")
        .with_value(ValueState::from("all"))
        .with_disabled(false)
        .with_density(Density::Normal)
        .with_width(Width::Medium)
        .on_change(|_| {}),
    ));
    view_is_constructible(checkbox(
        CheckboxProps::new("include", "Include")
            .with_name("include")
            .with_checked(true)
            .with_disabled(false)
            .with_density(Density::Compact)
            .on_change(|_| {}),
    ));
}

#[test]
fn layout_components_construct_from_typed_props() {
    view_is_constructible(panel(
        PanelProps::new(child())
            .with_surface(Surface::Default)
            .with_density(Density::Normal),
    ));
    view_is_constructible(card(
        CardProps::new(child())
            .with_surface(Surface::Subtle)
            .with_density(Density::Compact),
    ));
    view_is_constructible(stack(
        StackProps::new(child())
            .with_gap(Gap::Small)
            .with_align(Align::Start),
    ));
    view_is_constructible(inline(
        InlineProps::new(child())
            .with_gap(Gap::Small)
            .with_align(Align::Center)
            .with_justify(Justify::End),
    ));
    view_is_constructible(grid(
        GridProps::new(child())
            .with_columns(GridColumns::Two)
            .with_gap(Gap::Medium)
            .with_align(Align::Stretch),
    ));
    view_is_constructible(text(
        TextProps::new(child())
            .with_tone(Tone::Muted)
            .with_size(Size::Small)
            .with_weight(TextWeight::Medium)
            .with_element(TextElement::Paragraph)
            .with_overflow(TextOverflow::Truncate),
    ));
    view_is_constructible(heading(
        HeadingProps::new(child())
            .with_level(HeadingLevel::One)
            .with_size(Size::Large)
            .with_tone(Tone::Default)
            .with_align(Align::Start),
    ));
    view_is_constructible(section_header(
        SectionHeaderProps::new("Section")
            .with_actions(child())
            .with_density(Density::Compact),
    ));
    view_is_constructible(toolbar(
        ToolbarProps::new("Actions", child()).with_density(Density::Compact),
    ));
}

#[test]
fn state_and_navigation_components_construct_from_typed_props() {
    view_is_constructible(modal(
        ModalProps::new("Dialog", child())
            .with_footer(child())
            .with_size(Size::Large),
    ));
    view_is_constructible(disclosure(
        DisclosureProps::new("More", child())
            .with_state(true)
            .with_density(Density::Compact),
    ));
    view_is_constructible(spinner(SpinnerProps::new("Loading").with_size(Size::Small)));
    view_is_constructible(empty_state(
        EmptyStateProps::new("Empty", "No results").with_actions(child()),
    ));
    view_is_constructible(error_state(
        ErrorStateProps::new("Error", "Retry").with_actions(child()),
    ));
    view_is_constructible(filter_disclosure(FilterDisclosureProps::new(
        "Filters",
        child(),
    )));
    view_is_constructible(spacer());
    view_is_constructible(separator());
    view_is_constructible(link(LinkProps::new("Docs", "/docs").with_size(Size::Small)));
    view_is_constructible(link(
        LinkProps::new("External", "https://example.test").external(),
    ));
    view_is_constructible(score_text(
        ScoreTextProps::new(72).with_state(ScoreTextState::TextOnly),
    ));
    view_is_constructible(delta_text(
        DeltaTextProps::new(5)
            .with_suffix("%")
            .with_size(Size::Large),
    ));
    view_is_constructible(navigation_pane(NavigationPaneProps::new(
        "Queue",
        "Queue",
        child(),
    )));
    view_is_constructible(navigation_item(
        NavigationItemProps::new(child())
            .with_selected(true)
            .with_state(NavigationItemState::Subdued)
            .with_tone(ScoreTone::Medium)
            .on_click(|_| {}),
    ));
    view_is_constructible(context_shell(ContextShellProps::new(child())));
    view_is_constructible(context_bar(ContextBarProps::new(child())));
    view_is_constructible(score_button(
        ScoreButtonProps::new(84)
            .with_state(true)
            .with_title("Score")
            .on_click(|_| {}),
    ));
    view_is_constructible(score_details_panel(ScoreDetailsPanelProps::new(child())));
    view_is_constructible(score_detail_item(
        ScoreDetailItemProps::new("Signal", 5).with_note(Some("Note".to_string())),
    ));
}

#[test]
fn report_and_media_components_construct_from_typed_props() {
    view_is_constructible(badge_header(BadgeHeaderProps::new("Summary", child())));
    view_is_constructible(card_header(
        super::CardHeaderProps::new("Card").with_actions(child()),
    ));
    view_is_constructible(text_list(
        TextListProps::new(child()).with_density(Density::Normal),
    ));
    view_is_constructible(text_list_item(TextListItemProps::new(child())));
    view_is_constructible(code_block(CodeBlockProps::new("let x = 1;")));
    view_is_constructible(scroll_stack(
        ScrollStackProps::new(child()).with_density(Density::Comfortable),
    ));
    view_is_constructible(media_gallery_panel(
        MediaGalleryPanelProps::new("Media", "Images", "2 files", child()).with_state(false),
    ));
    view_is_constructible(media_group(
        MediaGroupProps::new("Added", 2, child()).with_tone(Tone::Success),
    ));
    view_is_constructible(media_card(MediaCardProps::new(child())));
    view_is_constructible(media_preview(MediaPreviewProps::new(
        "Preview",
        Some("/image.png".to_string()),
    )));
    view_is_constructible(media_preview(MediaPreviewProps::new("Preview", None)));
    view_is_constructible(signature_block(SignatureBlockProps::new("SHA", "abc123")));
}

#[test]
fn diff_components_construct_from_typed_props() {
    view_is_constructible(diff_viewer_shell(DiffViewerShellProps::new(
        "Diff",
        child(),
    )));
    view_is_constructible(diff_state(DiffStateProps::new("Diff", "Loading")));
    view_is_constructible(diff_stats_bar(DiffStatsBarProps::new(child())));
    view_is_constructible(diff_mode_label(DiffModeLabelProps::new("Split")));
    view_is_constructible(diff_body(DiffBodyProps::new(child())));
    view_is_constructible(diff_hunk(DiffHunkProps::new(child())));
    view_is_constructible(diff_hunk_header(DiffHunkHeaderProps::new(
        "Hunk",
        "Section",
        child(),
    )));
    view_is_constructible(diff_badge(
        DiffBadgeProps::new("Rendered").with_tone(Tone::Accent),
    ));
    view_is_constructible(diff_rows(DiffRowsProps::new(child())));
    view_is_constructible(diff_separator(DiffSeparatorProps::new("Context")));
    view_is_constructible(diff_split_header(DiffSplitHeaderProps::new(
        "Before", "After",
    )));
    view_is_constructible(diff_split_row(DiffSplitRowProps::new(child())));
    view_is_constructible(diff_empty_cell(DiffEmptyCellProps));
    view_is_constructible(diff_line(
        DiffLineProps::new(DiffTone::Insert, "+", "Added line", child())
            .with_before_label("12")
            .with_after_label("13")
            .with_line_label("line 13")
            .with_state(DiffLineState::Framed)
            .on_context_menu(|_| {})
            .on_double_click(|_| {}),
    ));
    view_is_constructible(diff_inline_mark(DiffInlineMarkProps::new(
        DiffTone::Insert,
        "added",
    )));
    view_is_constructible(diff_inline_mark(DiffInlineMarkProps::new(
        DiffTone::Equal,
        "same",
    )));
    view_is_constructible(diff_edit_panel(
        DiffEditPanelProps::new("edit", "value", child()).on_keydown(|_| {}),
    ));
    view_is_constructible(diff_context_menu(
        DiffContextMenuProps::new(10, 20, child()).on_backdrop_click(|_| {}),
    ));
    view_is_constructible(diff_context_menu_item(
        DiffContextMenuItemProps::new("Copy").on_click(|_| {}),
    ));
    view_is_constructible(rendered_html_host(super::RenderedHtmlHostProps::new(
        NodeRef::<html::Div>::new(),
    )));
}
