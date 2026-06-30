//! Layout primitives and shared presentation variants.

use leptos::prelude::*;

use super::util::{class_names, push_class};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlState {
    Static(bool),
    Signal(Signal<bool>),
}

impl Default for ControlState {
    fn default() -> Self {
        Self::Static(false)
    }
}

impl ControlState {
    #[must_use]
    pub fn get(self) -> bool {
        match self {
            Self::Static(value) => value,
            Self::Signal(value) => value.get(),
        }
    }
}

impl From<bool> for ControlState {
    fn from(value: bool) -> Self {
        Self::Static(value)
    }
}

impl From<Signal<bool>> for ControlState {
    fn from(value: Signal<bool>) -> Self {
        Self::Signal(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueState {
    Static(String),
    Signal(Signal<String>),
}

impl Default for ValueState {
    fn default() -> Self {
        Self::Static(String::new())
    }
}

impl ValueState {
    #[must_use]
    pub fn get(&self) -> String {
        match self {
            Self::Static(value) => value.clone(),
            Self::Signal(value) => value.get(),
        }
    }
}

impl From<String> for ValueState {
    fn from(value: String) -> Self {
        Self::Static(value)
    }
}

impl From<&str> for ValueState {
    fn from(value: &str) -> Self {
        Self::Static(value.to_string())
    }
}

impl From<Signal<String>> for ValueState {
    fn from(value: Signal<String>) -> Self {
        Self::Signal(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Comfortable,
}

impl Density {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Compact => "sp42-density-compact",
            Self::Normal => "sp42-density-normal",
            Self::Comfortable => "sp42-density-comfortable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Small => "sp42-size-small",
            Self::Medium => "sp42-size-medium",
            Self::Large => "sp42-size-large",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlWidth {
    #[default]
    Auto,
    Short,
    Medium,
    Full,
}

impl ControlWidth {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Auto => "sp42-control-auto",
            Self::Short => "sp42-control-short",
            Self::Medium => "sp42-control-medium",
            Self::Full => "sp42-control-full",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Surface {
    #[default]
    Default,
    Subtle,
    Raised,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Surface {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Default => "sp42-surface-default",
            Self::Subtle => "sp42-surface-subtle",
            Self::Raised => "sp42-surface-raised",
            Self::Accent => "sp42-surface-accent",
            Self::Success => "sp42-surface-success",
            Self::Warning => "sp42-surface-warning",
            Self::Danger => "sp42-surface-danger",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gap {
    None,
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
    XLarge,
}

impl Gap {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::None => "sp42-gap-none",
            Self::XSmall => "sp42-gap-xs",
            Self::Small => "sp42-gap-sm",
            Self::Medium => "sp42-gap-md",
            Self::Large => "sp42-gap-lg",
            Self::XLarge => "sp42-gap-xl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
    Baseline,
}

impl Align {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Start => "sp42-align-start",
            Self::Center => "sp42-align-center",
            Self::End => "sp42-align-end",
            Self::Stretch => "sp42-align-stretch",
            Self::Baseline => "sp42-align-baseline",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    Between,
}

impl Justify {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Start => "sp42-justify-start",
            Self::Center => "sp42-justify-center",
            Self::End => "sp42-justify-end",
            Self::Between => "sp42-justify-between",
        }
    }
}

pub struct PanelProps {
    children: Children,
    surface: Surface,
    density: Density,
}

impl PanelProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            surface: Surface::default(),
            density: Density::default(),
        }
    }

    #[must_use]
    pub const fn with_surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        class_names(&[
            "panel",
            self.surface.class_name(),
            self.density.class_name(),
        ])
    }
}

#[must_use]
pub fn panel(props: PanelProps) -> impl IntoView {
    let class_name = props.class_name();
    let children = props.children;

    view! {
        <section class=class_name>{children()}</section>
    }
}

pub use panel as Panel;

pub struct CardProps {
    children: Children,
    surface: Surface,
    density: Density,
}

impl CardProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            surface: Surface::Subtle,
            density: Density::default(),
        }
    }

    #[must_use]
    pub const fn with_surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        class_names(&["card", self.surface.class_name(), self.density.class_name()])
    }
}

#[must_use]
pub fn card(props: CardProps) -> impl IntoView {
    let class_name = props.class_name();
    let children = props.children;

    view! {
        <article class=class_name>{children()}</article>
    }
}

pub use card as Card;

pub struct StackProps {
    children: Children,
    gap: Gap,
    align: Align,
}

impl StackProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            gap: Gap::default(),
            align: Align::Stretch,
        }
    }

    #[must_use]
    pub const fn with_gap(mut self, gap: Gap) -> Self {
        self.gap = gap;
        self
    }

    #[must_use]
    pub const fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        class_names(&["sp42-stack", self.gap.class_name(), self.align.class_name()])
    }
}

#[must_use]
pub fn stack(props: StackProps) -> impl IntoView {
    let class_name = props.class_name();
    let children = props.children;

    view! {
        <div class=class_name>{children()}</div>
    }
}

pub use stack as Stack;

pub struct InlineProps {
    children: Children,
    gap: Gap,
    align: Align,
    justify: Justify,
    wrap: bool,
}

impl InlineProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            gap: Gap::default(),
            align: Align::Center,
            justify: Justify::default(),
            wrap: true,
        }
    }

    #[must_use]
    pub const fn with_gap(mut self, gap: Gap) -> Self {
        self.gap = gap;
        self
    }

    #[must_use]
    pub const fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    #[must_use]
    pub const fn with_justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    #[must_use]
    pub const fn without_wrap(mut self) -> Self {
        self.wrap = false;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        let mut class_name = class_names(&[
            "sp42-inline",
            self.gap.class_name(),
            self.align.class_name(),
            self.justify.class_name(),
        ]);
        if self.wrap {
            push_class(&mut class_name, "sp42-wrap");
        }
        class_name
    }
}

#[must_use]
pub fn inline(props: InlineProps) -> impl IntoView {
    let class_name = props.class_name();
    let children = props.children;

    view! {
        <div class=class_name>{children()}</div>
    }
}

pub use inline as Inline;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridColumns {
    #[default]
    One,
    Two,
    Three,
    Four,
    AutoFit,
}

impl GridColumns {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::One => "sp42-grid-one",
            Self::Two => "sp42-grid-two",
            Self::Three => "sp42-grid-three",
            Self::Four => "sp42-grid-four",
            Self::AutoFit => "sp42-grid-auto-fit",
        }
    }
}

pub struct GridProps {
    children: Children,
    columns: GridColumns,
    gap: Gap,
    align: Align,
}

impl GridProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            columns: GridColumns::default(),
            gap: Gap::default(),
            align: Align::Stretch,
        }
    }

    #[must_use]
    pub const fn with_columns(mut self, columns: GridColumns) -> Self {
        self.columns = columns;
        self
    }

    #[must_use]
    pub const fn with_gap(mut self, gap: Gap) -> Self {
        self.gap = gap;
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
            "sp42-grid",
            self.columns.class_name(),
            self.gap.class_name(),
            self.align.class_name(),
        ])
    }
}

#[must_use]
pub fn grid(props: GridProps) -> impl IntoView {
    let class_name = props.class_name();
    let children = props.children;

    view! {
        <div class=class_name>{children()}</div>
    }
}

pub use grid as Grid;

pub struct ToolbarProps {
    aria_label: String,
    children: Children,
    density: Density,
}

impl ToolbarProps {
    #[must_use]
    pub fn new(aria_label: impl Into<String>, children: Children) -> Self {
        Self {
            aria_label: aria_label.into(),
            children,
            density: Density::Compact,
        }
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }
}

#[must_use]
pub fn toolbar(props: ToolbarProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div
            role="toolbar"
            aria-label=props.aria_label
            class=class_names(&["sp42-toolbar", props.density.class_name()])
        >
            {children()}
        </div>
    }
}

pub use toolbar as Toolbar;

#[must_use]
pub fn spacer() -> impl IntoView {
    view! { <div class="flex-spacer"></div> }
}

pub use spacer as Spacer;

#[must_use]
pub fn separator() -> impl IntoView {
    view! { <span class="sp42-separator">"|"</span> }
}

pub use separator as Separator;
