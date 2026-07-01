//! Overlay and disclosure primitives.

use leptos::prelude::*;

use super::controls::{Button, ButtonProps, ButtonSurface};
use super::data_display::{ShortcutDefinition, ShortcutList, ShortcutListProps};
use super::layout::{Density, Size, State, ValueState};
use super::util::class_names;

pub struct ModalProps {
    title: String,
    children: Children,
    footer: Option<Children>,
    size: Size,
}

impl ModalProps {
    #[must_use]
    pub fn new(title: impl Into<String>, children: Children) -> Self {
        Self {
            title: title.into(),
            children,
            footer: None,
            size: Size::default(),
        }
    }

    #[must_use]
    pub fn with_footer(mut self, footer: Children) -> Self {
        self.footer = Some(footer);
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
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
        <div class="sp42-modal-backdrop">
            <section
                class=class_names(&["sp42-modal", props.size.modal_class_name()])
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
    open: State,
    density: Density,
}

impl DisclosureProps {
    #[must_use]
    pub fn new(summary: impl Into<String>, children: Children) -> Self {
        Self {
            summary: summary.into(),
            children,
            open: State::default(),
            density: Density::default(),
        }
    }

    #[must_use]
    pub fn with_state(mut self, open: impl Into<State>) -> Self {
        self.open = open.into();
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
    let open = props.open;

    view! {
        <details class=class_names(&["sp42-disclosure", props.density.class_name()]) open=move || open.get()>
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

pub struct KeyboardShortcutModalProps {
    title: String,
    shortcuts: Vec<ShortcutDefinition>,
    on_close: Option<Callback<leptos::ev::MouseEvent>>,
}

impl KeyboardShortcutModalProps {
    #[must_use]
    pub fn new(title: impl Into<String>, shortcuts: Vec<ShortcutDefinition>) -> Self {
        Self {
            title: title.into(),
            shortcuts,
            on_close: None,
        }
    }

    #[must_use]
    pub fn on_close<F>(mut self, on_close: F) -> Self
    where
        F: Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
    {
        self.on_close = Some(Callback::new(on_close));
        self
    }
}

#[must_use]
pub fn keyboard_shortcut_modal(props: KeyboardShortcutModalProps) -> impl IntoView {
    let close_backdrop = props.on_close;
    let close_button = props.on_close;
    let title = props.title;
    let aria_label = title.clone();

    view! {
        <div
            class="sp42-modal-backdrop"
            on:click=move |ev| {
                if let Some(callback) = &close_backdrop {
                    callback.run(ev);
                }
            }
        >
            <section
                class="sp42-modal sp42-modal-sm"
                role="dialog"
                aria-modal="true"
                aria-label=aria_label
                on:click=move |ev| ev.stop_propagation()
            >
                <header class="sp42-modal-header">
                    <h2>{title}</h2>
                </header>
                {ShortcutList(ShortcutListProps::new(props.shortcuts))}
                <footer class="sp42-modal-footer">
                    {Button(
                        ButtonProps::new("Close")
                            .with_surface(ButtonSurface::Ghost)
                            .on_click(move |ev| {
                                if let Some(callback) = &close_button {
                                    callback.run(ev);
                                }
                            })
                    )}
                </footer>
            </section>
        </div>
    }
}

pub use keyboard_shortcut_modal as KeyboardShortcutModal;

pub struct FullscreenOverlayProps {
    children: Children,
}

impl FullscreenOverlayProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn fullscreen_overlay(props: FullscreenOverlayProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div class="sp42-fullscreen-overlay">
            <div class="sp42-fullscreen-overlay-inner">{children()}</div>
        </div>
    }
}

pub use fullscreen_overlay as FullscreenOverlay;
