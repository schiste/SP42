//! Typography primitives and inline text helpers.

use leptos::prelude::*;

use super::layout::{Align, Density, Size, Tone};
use super::util::{class_names, push_class};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextWeight {
    #[default]
    Regular,
    Medium,
    Bold,
}

impl TextWeight {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Regular => "sp42-weight-regular",
            Self::Medium => "sp42-weight-medium",
            Self::Bold => "sp42-weight-bold",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextFamily {
    #[default]
    Default,
    Mono,
}

impl TextFamily {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Default => "",
            Self::Mono => "sp42-mono",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextElement {
    #[default]
    Span,
    Paragraph,
    Strong,
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextOverflow {
    #[default]
    Normal,
    Truncate,
    ClampTwo,
    PreserveLines,
}

impl TextOverflow {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Normal => "",
            Self::Truncate => "sp42-truncate",
            Self::ClampTwo => "sp42-text-clamp-two",
            Self::PreserveLines => "sp42-text-preserve-lines",
        }
    }
}

pub struct TextProps {
    children: Children,
    tone: Tone,
    size: Size,
    weight: TextWeight,
    element: TextElement,
    family: TextFamily,
    overflow: TextOverflow,
}

impl TextProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            tone: Tone::default(),
            size: Size::default(),
            weight: TextWeight::default(),
            element: TextElement::default(),
            family: TextFamily::default(),
            overflow: TextOverflow::default(),
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
    pub const fn with_weight(mut self, weight: TextWeight) -> Self {
        self.weight = weight;
        self
    }

    #[must_use]
    pub const fn with_element(mut self, element: TextElement) -> Self {
        self.element = element;
        self
    }

    #[must_use]
    pub const fn with_family(mut self, family: TextFamily) -> Self {
        self.family = family;
        self
    }

    #[must_use]
    pub const fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        let mut class_name = class_names(&[
            "sp42-text",
            self.tone.text_class_name(),
            self.size.text_class_name(),
            self.weight.class_name(),
        ]);
        push_class(&mut class_name, self.family.class_name());
        push_class(&mut class_name, self.overflow.class_name());
        class_name
    }
}

#[must_use]
pub fn text(props: TextProps) -> AnyView {
    let class_name = props.class_name();
    let children = props.children;
    let content = children();

    match props.element {
        TextElement::Span => view! { <span class=class_name>{content}</span> }.into_any(),
        TextElement::Paragraph => view! { <p class=class_name>{content}</p> }.into_any(),
        TextElement::Strong => view! { <strong class=class_name>{content}</strong> }.into_any(),
        TextElement::Code => view! { <code class=class_name>{content}</code> }.into_any(),
    }
}

pub use text as Text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeadingLevel {
    One,
    #[default]
    Two,
    Three,
    Four,
    Five,
    Six,
}

pub struct HeadingProps {
    children: Children,
    level: HeadingLevel,
    size: Size,
    tone: Tone,
    align: Align,
}

impl HeadingProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            level: HeadingLevel::default(),
            size: Size::default(),
            tone: Tone::default(),
            align: Align::Start,
        }
    }

    #[must_use]
    pub const fn with_level(mut self, level: HeadingLevel) -> Self {
        self.level = level;
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    #[must_use]
    pub const fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        class_names(&[
            "sp42-heading",
            self.size.heading_class_name(),
            self.tone.text_class_name(),
            self.align.class_name(),
        ])
    }
}

#[must_use]
pub fn heading(props: HeadingProps) -> AnyView {
    let class_name = props.class_name();
    let children = props.children;
    let content = children();

    match props.level {
        HeadingLevel::One => view! { <h1 class=class_name>{content}</h1> }.into_any(),
        HeadingLevel::Two => view! { <h2 class=class_name>{content}</h2> }.into_any(),
        HeadingLevel::Three => view! { <h3 class=class_name>{content}</h3> }.into_any(),
        HeadingLevel::Four => view! { <h4 class=class_name>{content}</h4> }.into_any(),
        HeadingLevel::Five => view! { <h5 class=class_name>{content}</h5> }.into_any(),
        HeadingLevel::Six => view! { <h6 class=class_name>{content}</h6> }.into_any(),
    }
}

pub use heading as Heading;

pub struct SectionHeaderProps {
    title: String,
    actions: Option<Children>,
    density: Density,
}

impl SectionHeaderProps {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            actions: None,
            density: Density::Compact,
        }
    }

    #[must_use]
    pub fn with_actions(mut self, actions: Children) -> Self {
        self.actions = Some(actions);
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }
}

#[must_use]
pub fn section_header(props: SectionHeaderProps) -> impl IntoView {
    let actions = props
        .actions
        .map(|actions| view! { <div class="sp42-section-actions">{actions()}</div> }.into_any());

    view! {
        <header class=class_names(&["sp42-section-label", props.density.class_name()])>
            <span class="sp42-section-label">{props.title}</span>
            {actions}
        </header>
    }
}

pub use section_header as SectionHeader;
pub struct LinkProps {
    label: String,
    href: String,
    external: bool,
    size: Size,
}

impl LinkProps {
    #[must_use]
    pub fn new(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            external: false,
            size: Size::Small,
        }
    }

    #[must_use]
    pub const fn external(mut self) -> Self {
        self.external = true;
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
}

#[must_use]
pub fn link(props: LinkProps) -> AnyView {
    let class_name = class_names(&["sp42-link", props.size.text_class_name()]);

    if props.external {
        view! {
            <a href=props.href target="_blank" rel="noopener noreferrer" class=class_name>
                {props.label}
            </a>
        }
        .into_any()
    } else {
        view! {
            <a href=props.href class=class_name>
                {props.label}
            </a>
        }
        .into_any()
    }
}

pub use link as Link;
