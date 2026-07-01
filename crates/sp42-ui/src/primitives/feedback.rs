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
        let mut class_name = String::from("sp42-badge sp42-status-badge");
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
        <span class=class_names(&["sp42-spinner", props.size.spinner_class_name()]) role="status" aria-live="polite">
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

#[derive(Clone, Copy)]
pub enum ToneState {
    Static(Tone),
    Signal(Signal<Tone>),
}

impl Default for ToneState {
    fn default() -> Self {
        Self::Static(Tone::default())
    }
}

impl ToneState {
    #[must_use]
    pub fn get(self) -> Tone {
        match self {
            Self::Static(tone) => tone,
            Self::Signal(tone) => tone.get(),
        }
    }
}

impl From<Tone> for ToneState {
    fn from(value: Tone) -> Self {
        Self::Static(value)
    }
}

impl From<Signal<Tone>> for ToneState {
    fn from(value: Signal<Tone>) -> Self {
        Self::Signal(value)
    }
}

pub struct StatusRegionProps {
    children: Children,
    tone: Tone,
}

impl StatusRegionProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            tone: Tone::Muted,
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

#[must_use]
pub fn status_region(props: StatusRegionProps) -> impl IntoView {
    let children = props.children;

    view! {
        <section class=class_names(&["sp42-status-region", tone_region_class_name(props.tone)]) role="status" aria-live="polite">
            {children()}
        </section>
    }
}

pub use status_region as StatusRegion;

pub struct LoadingRegionProps {
    label: String,
}

impl LoadingRegionProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

#[must_use]
pub fn loading_region(props: LoadingRegionProps) -> impl IntoView {
    view! {
        <section class="sp42-status-region" role="status" aria-live="polite">
            {Spinner(SpinnerProps::new(props.label.clone()))}
            <p>{props.label}</p>
        </section>
    }
}

pub use loading_region as LoadingRegion;

pub struct StatusDotProps {
    tone: ToneState,
    label: String,
}

impl StatusDotProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            tone: ToneState::default(),
            label: label.into(),
        }
    }

    #[must_use]
    pub fn with_tone(mut self, tone: impl Into<ToneState>) -> Self {
        self.tone = tone.into();
        self
    }
}

#[must_use]
pub fn status_dot(props: StatusDotProps) -> impl IntoView {
    let tone = props.tone;

    view! {
        <span
            class=move || class_names(&["sp42-status-dot", tone_dot_class_name(tone.get())])
            role="status"
            aria-label=props.label
        ></span>
    }
}

pub use status_dot as StatusDot;

#[must_use]
fn tone_region_class_name(tone: Tone) -> &'static str {
    match tone {
        Tone::Danger => "sp42-status-region-danger",
        Tone::Warning => "sp42-status-region-warning",
        Tone::Success => "sp42-status-region-success",
        Tone::Accent => "sp42-status-region-accent",
        Tone::Info => "sp42-status-region-info",
        Tone::Default | Tone::Muted | Tone::Subtle => "sp42-status-region-muted",
    }
}

#[must_use]
fn tone_dot_class_name(tone: Tone) -> &'static str {
    match tone {
        Tone::Danger => "sp42-status-dot-danger",
        Tone::Warning => "sp42-status-dot-warning",
        Tone::Success => "sp42-status-dot-success",
        Tone::Accent => "sp42-status-dot-accent",
        Tone::Info => "sp42-status-dot-info",
        Tone::Default | Tone::Muted | Tone::Subtle => "sp42-status-dot-muted",
    }
}
