//! Diff viewer primitives and rendered HTML hosts.

use leptos::{html, prelude::*};

use super::util::{class_names, push_class};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffTone {
    Insert,
    Delete,
    #[default]
    Equal,
}

impl DiffTone {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Insert => "diff-insert",
            Self::Delete => "diff-delete",
            Self::Equal => "diff-equal",
        }
    }

    #[must_use]
    pub const fn inline_class_name(self) -> &'static str {
        match self {
            Self::Insert => "sp42-diff-inline-insert",
            Self::Delete => "sp42-diff-inline-delete",
            Self::Equal => "",
        }
    }
}

pub struct DiffViewerShellProps {
    aria_label: String,
    children: Children,
}

impl DiffViewerShellProps {
    #[must_use]
    pub fn new(aria_label: impl Into<String>, children: Children) -> Self {
        Self {
            aria_label: aria_label.into(),
            children,
        }
    }
}

#[must_use]
pub fn diff_viewer_shell(props: DiffViewerShellProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div role="main" aria-label=props.aria_label class="diff-viewer">
            {children()}
        </div>
    }
}

pub use diff_viewer_shell as DiffViewerShell;

pub struct DiffStateProps {
    aria_label: String,
    message: String,
}

impl DiffStateProps {
    #[must_use]
    pub fn new(aria_label: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            aria_label: aria_label.into(),
            message: message.into(),
        }
    }
}

#[must_use]
pub fn diff_state(props: DiffStateProps) -> impl IntoView {
    view! {
        <div role="main" aria-label=props.aria_label class="sp42-diff-state">
            <p>{props.message}</p>
        </div>
    }
}

pub use diff_state as DiffState;

pub struct DiffStatsBarProps {
    children: Children,
}

impl DiffStatsBarProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn diff_stats_bar(props: DiffStatsBarProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="diff-stats">{children()}</div> }
}

pub use diff_stats_bar as DiffStatsBar;

pub struct DiffModeLabelProps {
    label: String,
}

impl DiffModeLabelProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

#[must_use]
pub fn diff_mode_label(props: DiffModeLabelProps) -> impl IntoView {
    view! { <span class="sp42-diff-mode-label">{props.label}</span> }
}

pub use diff_mode_label as DiffModeLabel;

pub struct DiffBodyProps {
    children: Children,
}

impl DiffBodyProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn diff_body(props: DiffBodyProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-diff-body">{children()}</div> }
}

pub use diff_body as DiffBody;

pub struct DiffHunkProps {
    children: Children,
}

impl DiffHunkProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn diff_hunk(props: DiffHunkProps) -> impl IntoView {
    let children = props.children;

    view! { <section class="sp42-diff-hunk">{children()}</section> }
}

pub use diff_hunk as DiffHunk;

pub struct DiffHunkHeaderProps {
    title: String,
    section_label: String,
    children: Children,
}

impl DiffHunkHeaderProps {
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        section_label: impl Into<String>,
        children: Children,
    ) -> Self {
        Self {
            title: title.into(),
            section_label: section_label.into(),
            children,
        }
    }
}

#[must_use]
pub fn diff_hunk_header(props: DiffHunkHeaderProps) -> impl IntoView {
    let children = props.children;

    view! {
        <header class="sp42-diff-hunk-header">
            <div>
                <strong>{props.title}</strong>
                <span>{props.section_label}</span>
                {children()}
            </div>
        </header>
    }
}

pub use diff_hunk_header as DiffHunkHeader;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffBadgeTone {
    #[default]
    Neutral,
    Accent,
}

impl DiffBadgeTone {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Neutral => "sp42-diff-badge-neutral",
            Self::Accent => "sp42-diff-badge-accent",
        }
    }
}

pub struct DiffBadgeProps {
    label: String,
    tone: DiffBadgeTone,
}

impl DiffBadgeProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tone: DiffBadgeTone::default(),
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: DiffBadgeTone) -> Self {
        self.tone = tone;
        self
    }
}

#[must_use]
pub fn diff_badge(props: DiffBadgeProps) -> impl IntoView {
    view! {
        <span class=class_names(&["sp42-diff-badge", props.tone.class_name()])>
            {props.label}
        </span>
    }
}

pub use diff_badge as DiffBadge;

pub struct DiffRowsProps {
    children: Children,
}

impl DiffRowsProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn diff_rows(props: DiffRowsProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-diff-rows">{children()}</div> }
}

pub use diff_rows as DiffRows;

pub struct DiffSeparatorProps {
    label: String,
}

impl DiffSeparatorProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

#[must_use]
pub fn diff_separator(props: DiffSeparatorProps) -> impl IntoView {
    view! { <div class="diff-separator">{props.label}</div> }
}

pub use diff_separator as DiffSeparator;

pub struct DiffSplitHeaderProps {
    before_label: String,
    after_label: String,
}

impl DiffSplitHeaderProps {
    #[must_use]
    pub fn new(before_label: impl Into<String>, after_label: impl Into<String>) -> Self {
        Self {
            before_label: before_label.into(),
            after_label: after_label.into(),
        }
    }
}

#[must_use]
pub fn diff_split_header(props: DiffSplitHeaderProps) -> impl IntoView {
    view! {
        <div class="sp42-diff-split-header">
            <div>{props.before_label}</div>
            <div>{props.after_label}</div>
        </div>
    }
}

pub use diff_split_header as DiffSplitHeader;

pub struct DiffSplitRowProps {
    children: Children,
}

impl DiffSplitRowProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn diff_split_row(props: DiffSplitRowProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-diff-split-row">{children()}</div> }
}

pub use diff_split_row as DiffSplitRow;

pub struct DiffEmptyCellProps;

#[must_use]
pub fn diff_empty_cell(_: DiffEmptyCellProps) -> impl IntoView {
    view! { <div class="sp42-diff-empty-cell"></div> }
}

pub use diff_empty_cell as DiffEmptyCell;

pub struct DiffLineProps {
    tone: DiffTone,
    aria_label: String,
    prefix: String,
    before_label: Option<String>,
    after_label: Option<String>,
    line_label: Option<String>,
    framed: bool,
    children: Children,
    on_context_menu: Option<Callback<leptos::ev::MouseEvent>>,
    on_double_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl DiffLineProps {
    #[must_use]
    pub fn new(
        tone: DiffTone,
        prefix: impl Into<String>,
        aria_label: impl Into<String>,
        children: Children,
    ) -> Self {
        Self {
            tone,
            aria_label: aria_label.into(),
            prefix: prefix.into(),
            before_label: None,
            after_label: None,
            line_label: None,
            framed: false,
            children,
            on_context_menu: None,
            on_double_click: None,
        }
    }

    #[must_use]
    pub fn with_before_label(mut self, label: impl Into<String>) -> Self {
        self.before_label = Some(label.into());
        self
    }

    #[must_use]
    pub fn with_after_label(mut self, label: impl Into<String>) -> Self {
        self.after_label = Some(label.into());
        self
    }

    #[must_use]
    pub fn with_line_label(mut self, label: impl Into<String>) -> Self {
        self.line_label = Some(label.into());
        self
    }

    #[must_use]
    pub const fn framed(mut self) -> Self {
        self.framed = true;
        self
    }

    #[must_use]
    pub fn on_context_menu<F>(mut self, on_context_menu: F) -> Self
    where
        F: Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
    {
        self.on_context_menu = Some(Callback::new(on_context_menu));
        self
    }

    #[must_use]
    pub fn on_double_click<F>(mut self, on_double_click: F) -> Self
    where
        F: Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
    {
        self.on_double_click = Some(Callback::new(on_double_click));
        self
    }
}

#[must_use]
pub fn diff_line(props: DiffLineProps) -> impl IntoView {
    let children = props.children;
    let on_context_menu = props.on_context_menu;
    let on_double_click = props.on_double_click;
    let mut class_name = String::from("diff-line");
    if props.framed {
        push_class(&mut class_name, "sp42-diff-line-framed");
    }

    let line_label = props.line_label.map(|label| {
        view! {
            <span class="diff-line-num" aria-hidden="true">{label}</span>
        }
        .into_any()
    });
    let before_label = props.before_label.map(|label| {
        view! {
            <span class="diff-line-num" aria-hidden="true">{label}</span>
        }
        .into_any()
    });
    let after_label = props.after_label.map(|label| {
        view! {
            <span class="diff-line-num" aria-hidden="true">{label}</span>
        }
        .into_any()
    });

    view! {
        <div
            class=class_name
            aria-label=props.aria_label
            on:contextmenu=move |ev| {
                if let Some(callback) = on_context_menu {
                    callback.run(ev);
                }
            }
            on:dblclick=move |ev| {
                if let Some(callback) = on_double_click {
                    callback.run(ev);
                }
            }
        >
            {line_label}
            {before_label}
            {after_label}
            <span class="sp42-diff-prefix" aria-hidden="true">{props.prefix}</span>
            <pre class=props.tone.class_name() dir="auto">{children()}</pre>
        </div>
    }
}

pub use diff_line as DiffLine;

pub struct DiffInlineMarkProps {
    tone: DiffTone,
    text: String,
}

impl DiffInlineMarkProps {
    #[must_use]
    pub fn new(tone: DiffTone, text: impl Into<String>) -> Self {
        Self {
            tone,
            text: text.into(),
        }
    }
}

#[must_use]
pub fn diff_inline_mark(props: DiffInlineMarkProps) -> AnyView {
    let class_name = props.tone.inline_class_name();
    if class_name.is_empty() {
        view! { <span>{props.text}</span> }.into_any()
    } else {
        view! { <mark class=class_names(&["sp42-diff-inline-mark", class_name])>{props.text}</mark> }
            .into_any()
    }
}

pub use diff_inline_mark as DiffInlineMark;

pub struct DiffEditPanelProps {
    textarea_id: String,
    value: String,
    actions: Children,
    on_keydown: Option<Callback<leptos::ev::KeyboardEvent>>,
}

impl DiffEditPanelProps {
    #[must_use]
    pub fn new(
        textarea_id: impl Into<String>,
        value: impl Into<String>,
        actions: Children,
    ) -> Self {
        Self {
            textarea_id: textarea_id.into(),
            value: value.into(),
            actions,
            on_keydown: None,
        }
    }

    #[must_use]
    pub fn on_keydown<F>(mut self, on_keydown: F) -> Self
    where
        F: Fn(leptos::ev::KeyboardEvent) + Send + Sync + 'static,
    {
        self.on_keydown = Some(Callback::new(on_keydown));
        self
    }
}

#[must_use]
pub fn diff_edit_panel(props: DiffEditPanelProps) -> impl IntoView {
    let actions = props.actions;
    let on_keydown = props.on_keydown;

    view! {
        <div class="diff-edit-container">
            <textarea
                id=props.textarea_id
                class="diff-edit-textarea"
                rows="4"
                prop:value=props.value
                on:keydown=move |ev| {
                    if let Some(callback) = on_keydown {
                        callback.run(ev);
                    }
                }
            />
            <div class="diff-edit-actions">{actions()}</div>
        </div>
    }
}

pub use diff_edit_panel as DiffEditPanel;

pub struct DiffContextMenuProps {
    x: i32,
    y: i32,
    children: Children,
    on_backdrop_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl DiffContextMenuProps {
    #[must_use]
    pub fn new(x: i32, y: i32, children: Children) -> Self {
        Self {
            x,
            y,
            children,
            on_backdrop_click: None,
        }
    }

    #[must_use]
    pub fn on_backdrop_click<F>(mut self, on_backdrop_click: F) -> Self
    where
        F: Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
    {
        self.on_backdrop_click = Some(Callback::new(on_backdrop_click));
        self
    }
}

#[must_use]
pub fn diff_context_menu(props: DiffContextMenuProps) -> impl IntoView {
    let children = props.children;
    let on_backdrop_click = props.on_backdrop_click;

    view! {
        <div
            class="context-menu-backdrop"
            on:click=move |ev| {
                if let Some(callback) = on_backdrop_click {
                    callback.run(ev);
                }
            }
        >
            <div
                class="context-menu"
                style=format!("left:{}px;top:{}px;", props.x, props.y)
                on:click=move |ev| ev.stop_propagation()
            >
                {children()}
            </div>
        </div>
    }
}

pub use diff_context_menu as DiffContextMenu;

pub struct DiffContextMenuItemProps {
    label: String,
    on_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl DiffContextMenuItemProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            on_click: None,
        }
    }

    #[must_use]
    pub fn on_click<F>(mut self, on_click: F) -> Self
    where
        F: Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
    {
        self.on_click = Some(Callback::new(on_click));
        self
    }
}

#[must_use]
pub fn diff_context_menu_item(props: DiffContextMenuItemProps) -> impl IntoView {
    let on_click = props.on_click;

    view! {
        <button
            type="button"
            class="context-menu-item"
            on:click=move |ev| {
                if let Some(callback) = on_click {
                    callback.run(ev);
                }
            }
        >
            {props.label}
        </button>
    }
}

pub use diff_context_menu_item as DiffContextMenuItem;

#[derive(Clone, Copy)]
pub struct RenderedHtmlHostProps {
    node_ref: NodeRef<html::Div>,
}

impl RenderedHtmlHostProps {
    #[must_use]
    pub const fn new(node_ref: NodeRef<html::Div>) -> Self {
        Self { node_ref }
    }
}

#[must_use]
pub fn rendered_html_host(props: RenderedHtmlHostProps) -> impl IntoView {
    view! { <div class="rendered-hunk-html" node_ref=props.node_ref></div> }
}

pub use rendered_html_host as RenderedHtmlHost;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedHighlightTone {
    Add,
    Remove,
}

impl RenderedHighlightTone {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Add => "rendered-hunk-highlight-add",
            Self::Remove => "rendered-hunk-highlight-remove",
        }
    }
}
