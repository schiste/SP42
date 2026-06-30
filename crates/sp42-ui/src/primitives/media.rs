//! Media comparison and preview primitives.

use leptos::prelude::*;

use super::layout::{State, Tone};
use super::util::class_names;

pub struct MediaGalleryPanelProps {
    aria_label: String,
    title: String,
    description: String,
    loading: State,
    children: Children,
}

impl MediaGalleryPanelProps {
    #[must_use]
    pub fn new(
        aria_label: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        children: Children,
    ) -> Self {
        Self {
            aria_label: aria_label.into(),
            title: title.into(),
            description: description.into(),
            loading: State::default(),
            children,
        }
    }

    #[must_use]
    pub fn with_state(mut self, loading: impl Into<State>) -> Self {
        self.loading = loading.into();
        self
    }
}

#[must_use]
pub fn media_gallery_panel(props: MediaGalleryPanelProps) -> impl IntoView {
    let children = props.children;
    let loading = props.loading;

    view! {
        <aside aria-label=props.aria_label class="sp42-media-panel">
            <header class="sp42-media-panel-header">
                <div class="sp42-media-panel-title">
                    <strong>{props.title}</strong>
                    <span>{props.description}</span>
                </div>
                {move || {
                    if loading.get() {
                        view! { <span class="sp42-media-loading">"Loading..."</span> }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }
                }}
            </header>
            <div class="sp42-media-panel-body">{children()}</div>
        </aside>
    }
}

pub use media_gallery_panel as MediaGalleryPanel;

pub struct MediaGroupProps {
    title: String,
    count: usize,
    tone: Tone,
    children: Children,
}

impl MediaGroupProps {
    #[must_use]
    pub fn new(title: impl Into<String>, count: usize, children: Children) -> Self {
        Self {
            title: title.into(),
            count,
            tone: Tone::Default,
            children,
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

#[must_use]
pub fn media_group(props: MediaGroupProps) -> impl IntoView {
    let children = props.children;

    view! {
        <section class="sp42-media-group">
            <header>
                <strong class=class_names(&["sp42-media-group-title", props.tone.text_class_name()])>
                    {props.title}
                </strong>
                <span>{props.count}</span>
            </header>
            <div class="sp42-media-group-list">{children()}</div>
        </section>
    }
}

pub use media_group as MediaGroup;

pub struct MediaCardProps {
    children: Children,
}

impl MediaCardProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn media_card(props: MediaCardProps) -> impl IntoView {
    let children = props.children;

    view! { <article class="sp42-media-card">{children()}</article> }
}

pub use media_card as MediaCard;

pub struct MediaPreviewProps {
    title: String,
    src: Option<String>,
}

impl MediaPreviewProps {
    #[must_use]
    pub fn new(title: impl Into<String>, src: Option<String>) -> Self {
        Self {
            title: title.into(),
            src,
        }
    }
}

#[must_use]
pub fn media_preview(props: MediaPreviewProps) -> impl IntoView {
    if let Some(src) = props.src {
        view! {
            <img
                src=src
                alt=props.title
                loading="lazy"
                class="sp42-media-preview"
            />
        }
        .into_any()
    } else {
        view! {
            <div class="sp42-media-preview sp42-media-preview-empty">
                "Preview unavailable"
            </div>
        }
        .into_any()
    }
}

pub use media_preview as MediaPreview;

pub struct SignatureBlockProps {
    label: String,
    value: String,
}

impl SignatureBlockProps {
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[must_use]
pub fn signature_block(props: SignatureBlockProps) -> impl IntoView {
    view! {
        <div class="sp42-signature-block">
            <span>{props.label}</span>
            <span>{props.value}</span>
        </div>
    }
}

pub use signature_block as SignatureBlock;
