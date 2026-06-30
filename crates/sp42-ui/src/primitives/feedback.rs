//! Feedback, status, loading, empty, and error states.

use leptos::prelude::*;

use super::layout::{Size, Tone};
use super::util::{class_names, push_class};

pub struct StatusBadgeProps {
    label: String,
    tone: Tone,
    size: Size,
}

impl StatusBadgeProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tone: Tone::default(),
            size: Size::default(),
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        let mut class_name = String::from("badge sp42-status-badge");
        push_class(&mut class_name, self.tone.status_class_name());
        push_class(&mut class_name, self.size.class_name());
        class_name
    }
}

#[must_use]
pub fn status_badge(props: StatusBadgeProps) -> impl IntoView {
    let class_name = props.class_name();

    view! {
        <span class=class_name>{props.label}</span>
    }
}

pub use status_badge as StatusBadge;

pub struct SpinnerProps {
    label: String,
    size: Size,
}

impl SpinnerProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            size: Size::default(),
        }
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
}

#[must_use]
pub fn spinner(props: SpinnerProps) -> impl IntoView {
    view! {
        <span class=class_names(&["spinner", props.size.spinner_class_name()]) role="status" aria-live="polite">
            <span class="sp42-visually-hidden">{props.label}</span>
        </span>
    }
}

pub use spinner as Spinner;

pub struct EmptyStateProps {
    title: String,
    message: String,
    actions: Option<Children>,
}

impl EmptyStateProps {
    #[must_use]
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            actions: None,
        }
    }

    #[must_use]
    pub fn with_actions(mut self, actions: Children) -> Self {
        self.actions = Some(actions);
        self
    }
}

#[must_use]
pub fn empty_state(props: EmptyStateProps) -> impl IntoView {
    let actions = props
        .actions
        .map(|actions| view! { <div class="sp42-state-actions">{actions()}</div> }.into_any());

    view! {
        <section class="sp42-state sp42-empty-state">
            <h2>{props.title}</h2>
            <p>{props.message}</p>
            {actions}
        </section>
    }
}

pub use empty_state as EmptyState;

pub struct ErrorStateProps {
    title: String,
    message: String,
    actions: Option<Children>,
}

impl ErrorStateProps {
    #[must_use]
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            actions: None,
        }
    }

    #[must_use]
    pub fn with_actions(mut self, actions: Children) -> Self {
        self.actions = Some(actions);
        self
    }
}

#[must_use]
pub fn error_state(props: ErrorStateProps) -> impl IntoView {
    let actions = props
        .actions
        .map(|actions| view! { <div class="sp42-state-actions">{actions()}</div> }.into_any());

    view! {
        <section class="sp42-state sp42-error-state" role="alert">
            <h2>{props.title}</h2>
            <p>{props.message}</p>
            {actions}
        </section>
    }
}

pub use error_state as ErrorState;
