//! Overlay and disclosure primitives.

use leptos::prelude::*;

use super::layout::{Density, ValueState};
use super::util::class_names;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModalSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ModalSize {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Small => "sp42-modal-sm",
            Self::Medium => "sp42-modal-md",
            Self::Large => "sp42-modal-lg",
        }
    }
}

pub struct ModalProps {
    title: String,
    children: Children,
    footer: Option<Children>,
    size: ModalSize,
}

impl ModalProps {
    #[must_use]
    pub fn new(title: impl Into<String>, children: Children) -> Self {
        Self {
            title: title.into(),
            children,
            footer: None,
            size: ModalSize::default(),
        }
    }

    #[must_use]
    pub fn with_footer(mut self, footer: Children) -> Self {
        self.footer = Some(footer);
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: ModalSize) -> Self {
        self.size = size;
        self
    }
}

#[must_use]
pub fn modal(props: ModalProps) -> impl IntoView {
    let children = props.children;
    let title = props.title;
    let aria_label = title.clone();
    let footer = props
        .footer
        .map(|footer| view! { <footer class="sp42-modal-footer">{footer()}</footer> }.into_any());

    view! {
        <div class="modal-backdrop">
            <section
                class=class_names(&["modal", "sp42-modal", props.size.class_name()])
                role="dialog"
                aria-modal="true"
                aria-label=aria_label
            >
                <header class="sp42-modal-header">
                    <h2>{title}</h2>
                </header>
                <div class="sp42-modal-body">{children()}</div>
                {footer}
            </section>
        </div>
    }
}

pub use modal as Modal;

pub struct DisclosureProps {
    summary: String,
    children: Children,
    open: bool,
    density: Density,
}

impl DisclosureProps {
    #[must_use]
    pub fn new(summary: impl Into<String>, children: Children) -> Self {
        Self {
            summary: summary.into(),
            children,
            open: false,
            density: Density::default(),
        }
    }

    #[must_use]
    pub const fn open(mut self) -> Self {
        self.open = true;
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }
}

#[must_use]
pub fn disclosure(props: DisclosureProps) -> impl IntoView {
    let children = props.children;

    view! {
        <details class=class_names(&["sp42-disclosure", props.density.class_name()]) open=props.open>
            <summary>{props.summary}</summary>
            <div class="sp42-disclosure-body">{children()}</div>
        </details>
    }
}

pub use disclosure as Disclosure;

pub struct FilterDisclosureProps {
    summary: ValueState,
    children: Children,
}

impl FilterDisclosureProps {
    #[must_use]
    pub fn new(summary: impl Into<ValueState>, children: Children) -> Self {
        Self {
            summary: summary.into(),
            children,
        }
    }
}

#[must_use]
pub fn filter_disclosure(props: FilterDisclosureProps) -> impl IntoView {
    let children = props.children;
    let summary = props.summary;

    view! {
        <details class="filter-bar-details">
            <summary class="filter-summary">
                {move || summary.get()}
            </summary>
            <div class="filter-bar">{children()}</div>
        </details>
    }
}

pub use filter_disclosure as FilterDisclosure;
