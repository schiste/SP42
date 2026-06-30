//! Navigation and context-shell primitives.

use leptos::prelude::*;

use super::data_display::ScoreTone;
use super::layout::State;
use super::util::push_class;

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
        <nav role="navigation" aria-label=props.aria_label class="queue-column">
            <div class="queue-header">{props.heading}</div>
            <div class="queue-scroll">{children()}</div>
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
    let mut class_name = String::from("queue-item");
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

    view! { <div class="context-header-shell">{children()}</div> }
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

    view! { <div class="context-header">{children()}</div> }
}

pub use context_bar as ContextBar;
