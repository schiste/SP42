//! Navigation and context-shell primitives.

use leptos::prelude::*;

use super::data_display::ScoreTone;
use super::layout::State;
use super::util::{class_names, push_class};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavigationItemState {
    #[default]
    Default,
    Subdued,
}

impl NavigationItemState {
    #[must_use]
    pub const fn is_subdued(self) -> bool {
        matches!(self, Self::Subdued)
    }
}

pub struct NavigationPaneProps {
    aria_label: String,
    heading: String,
    children: Children,
}

impl NavigationPaneProps {
    #[must_use]
    pub fn new(
        aria_label: impl Into<String>,
        heading: impl Into<String>,
        children: Children,
    ) -> Self {
        Self {
            aria_label: aria_label.into(),
            heading: heading.into(),
            children,
        }
    }
}

#[must_use]
pub fn navigation_pane(props: NavigationPaneProps) -> impl IntoView {
    let children = props.children;

    view! {
        <nav role="navigation" aria-label=props.aria_label class="sp42-navigation-pane">
            <div class="sp42-navigation-header">{props.heading}</div>
            <div class="sp42-navigation-scroll">{children()}</div>
        </nav>
    }
}

pub use navigation_pane as NavigationPane;

pub struct NavigationItemProps {
    children: Children,
    selected: State,
    state: NavigationItemState,
    tone: ScoreTone,
    on_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl NavigationItemProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            selected: State::default(),
            state: NavigationItemState::default(),
            tone: ScoreTone::default(),
            on_click: None,
        }
    }

    #[must_use]
    pub fn with_selected(mut self, selected: impl Into<State>) -> Self {
        self.selected = selected.into();
        self
    }

    #[must_use]
    pub const fn with_state(mut self, state: NavigationItemState) -> Self {
        self.state = state;
        self
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: ScoreTone) -> Self {
        self.tone = tone;
        self
    }

    #[must_use]
    pub fn on_click<F>(mut self, on_click: F) -> Self
    where
        F: Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
    {
        self.on_click = Some(Callback::new(on_click));
        self
    }

    #[must_use]
    pub fn class_name(&self, selected: bool) -> String {
        navigation_item_class_name(selected, self.state, self.tone)
    }
}

#[must_use]
pub fn navigation_item(props: NavigationItemProps) -> impl IntoView {
    let children = props.children;
    let selected = props.selected;
    let state = props.state;
    let tone = props.tone;
    let on_click = props.on_click;

    view! {
        <button
            type="button"
            class=move || {
                navigation_item_class_name(selected.get(), state, tone)
            }
            aria-pressed=move || selected.get().to_string()
            on:click=move |ev| {
                if let Some(callback) = on_click {
                    callback.run(ev);
                }
            }
        >
            {children()}
        </button>
    }
}

pub use navigation_item as NavigationItem;

#[must_use]
fn navigation_item_class_name(
    selected: bool,
    state: NavigationItemState,
    tone: ScoreTone,
) -> String {
    let mut class_name = String::from("sp42-navigation-item");
    if selected {
        push_class(&mut class_name, "sp42-nav-item-selected");
        push_class(&mut class_name, tone.class_name());
    }
    if state.is_subdued() {
        push_class(&mut class_name, "sp42-nav-item-subdued");
    }
    class_name
}

pub struct ContextShellProps {
    children: Children,
}

impl ContextShellProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn context_shell(props: ContextShellProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-context-header-shell">{children()}</div> }
}

pub use context_shell as ContextShell;

pub struct ContextBarProps {
    children: Children,
}

impl ContextBarProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn context_bar(props: ContextBarProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-context-header">{children()}</div> }
}

pub use context_bar as ContextBar;

pub struct WorkspaceShellProps {
    children: Children,
}

impl WorkspaceShellProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn workspace_shell(props: WorkspaceShellProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-workspace-shell">{children()}</div> }
}

pub use workspace_shell as WorkspaceShell;

pub struct WorkspaceNavProps {
    aria_label: String,
    children: Children,
}

impl WorkspaceNavProps {
    #[must_use]
    pub fn new(aria_label: impl Into<String>, children: Children) -> Self {
        Self {
            aria_label: aria_label.into(),
            children,
        }
    }
}

#[must_use]
pub fn workspace_nav(props: WorkspaceNavProps) -> impl IntoView {
    let children = props.children;

    view! { <nav class="sp42-workspace-nav" aria-label=props.aria_label>{children()}</nav> }
}

pub use workspace_nav as WorkspaceNav;

pub struct WorkspaceBrandProps {
    label: String,
}

impl WorkspaceBrandProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

#[must_use]
pub fn workspace_brand(props: WorkspaceBrandProps) -> impl IntoView {
    view! { <div class="sp42-workspace-brand">{props.label}</div> }
}

pub use workspace_brand as WorkspaceBrand;

pub struct WorkspaceTabsProps {
    aria_label: String,
    children: Children,
}

impl WorkspaceTabsProps {
    #[must_use]
    pub fn new(aria_label: impl Into<String>, children: Children) -> Self {
        Self {
            aria_label: aria_label.into(),
            children,
        }
    }
}

#[must_use]
pub fn workspace_tabs(props: WorkspaceTabsProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div class="sp42-workspace-tabs" role="tablist" aria-label=props.aria_label>
            {children()}
        </div>
    }
}

pub use workspace_tabs as WorkspaceTabs;

pub struct WorkspaceTabProps {
    label: String,
    selected: State,
    on_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl WorkspaceTabProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            selected: State::default(),
            on_click: None,
        }
    }

    #[must_use]
    pub fn with_selected(mut self, selected: impl Into<State>) -> Self {
        self.selected = selected.into();
        self
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
pub fn workspace_tab(props: WorkspaceTabProps) -> impl IntoView {
    let selected = props.selected;
    let on_click = props.on_click;

    view! {
        <button
            type="button"
            class=move || class_names(&[
                "sp42-workspace-tab",
                if selected.get() { "sp42-workspace-tab-active" } else { "" }
            ])
            aria-selected=move || selected.get().to_string()
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

pub use workspace_tab as WorkspaceTab;

pub struct WorkspaceInlineFormProps {
    children: Children,
    on_submit: Option<Callback<leptos::ev::SubmitEvent>>,
}

impl WorkspaceInlineFormProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            on_submit: None,
        }
    }

    #[must_use]
    pub fn on_submit<F>(mut self, on_submit: F) -> Self
    where
        F: Fn(leptos::ev::SubmitEvent) + Send + Sync + 'static,
    {
        self.on_submit = Some(Callback::new(on_submit));
        self
    }
}

#[must_use]
pub fn workspace_inline_form(props: WorkspaceInlineFormProps) -> impl IntoView {
    let children = props.children;
    let on_submit = props.on_submit;

    view! {
        <form
            class="sp42-workspace-inline-form"
            on:submit=move |ev| {
                if let Some(callback) = on_submit {
                    callback.run(ev);
                }
            }
        >
            {children()}
        </form>
    }
}

pub use workspace_inline_form as WorkspaceInlineForm;

pub struct WorkspaceSessionProps {
    children: Children,
}

impl WorkspaceSessionProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn workspace_session(props: WorkspaceSessionProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-workspace-session">{children()}</div> }
}

pub use workspace_session as WorkspaceSession;

pub struct WorkspaceBodyProps {
    children: Children,
}

impl WorkspaceBodyProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn workspace_body(props: WorkspaceBodyProps) -> impl IntoView {
    let children = props.children;

    view! { <main class="sp42-workspace-body">{children()}</main> }
}

pub use workspace_body as WorkspaceBody;
