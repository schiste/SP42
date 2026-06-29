//! Typed design-system primitives for SP42's Leptos UI.
//!
//! These functions own presentation choices through semantic variants. Callers
//! pass behavior, text, and children, but never raw CSS classes or inline style.

use leptos::{html, prelude::*};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonTone {
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl ButtonTone {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Neutral => "",
            Self::Accent => "btn-accent",
            Self::Success => "btn-success",
            Self::Warning => "btn-warning",
            Self::Danger => "btn-danger",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonEmphasis {
    #[default]
    Solid,
    Subtle,
    Ghost,
}

impl ButtonEmphasis {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Solid => "",
            Self::Subtle => "btn-subtle",
            Self::Ghost => "btn-ghost",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonType {
    #[default]
    Button,
    Submit,
    Reset,
}

impl ButtonType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Submit => "submit",
            Self::Reset => "reset",
        }
    }
}

pub struct ButtonProps {
    label: String,
    tone: ButtonTone,
    size: Size,
    density: Density,
    emphasis: ButtonEmphasis,
    button_type: ButtonType,
    disabled: ControlState,
    recommended: bool,
    title: String,
    aria_label: String,
    aria_keyshortcuts: String,
    on_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl ButtonProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tone: ButtonTone::default(),
            size: Size::default(),
            density: Density::default(),
            emphasis: ButtonEmphasis::default(),
            button_type: ButtonType::default(),
            disabled: ControlState::default(),
            recommended: false,
            title: String::new(),
            aria_label: String::new(),
            aria_keyshortcuts: String::new(),
            on_click: None,
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: ButtonTone) -> Self {
        self.tone = tone;
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }

    #[must_use]
    pub const fn with_emphasis(mut self, emphasis: ButtonEmphasis) -> Self {
        self.emphasis = emphasis;
        self
    }

    #[must_use]
    pub const fn with_type(mut self, button_type: ButtonType) -> Self {
        self.button_type = button_type;
        self
    }

    #[must_use]
    pub fn with_disabled(mut self, disabled: impl Into<ControlState>) -> Self {
        self.disabled = disabled.into();
        self
    }

    #[must_use]
    pub const fn recommended(mut self) -> Self {
        self.recommended = true;
        self
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    #[must_use]
    pub fn with_aria_label(mut self, aria_label: impl Into<String>) -> Self {
        self.aria_label = aria_label.into();
        self
    }

    #[must_use]
    pub fn with_keyshortcuts(mut self, aria_keyshortcuts: impl Into<String>) -> Self {
        self.aria_keyshortcuts = aria_keyshortcuts.into();
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
    pub fn class_name(&self) -> String {
        let mut class_name = String::from("btn");
        push_class(&mut class_name, self.tone.class_name());
        push_class(&mut class_name, self.size.class_name());
        push_class(&mut class_name, self.density.class_name());
        push_class(&mut class_name, self.emphasis.class_name());
        if self.recommended {
            push_class(&mut class_name, "btn-recommended");
        }
        class_name
    }
}

#[must_use]
pub fn button(props: ButtonProps) -> impl IntoView {
    let class_name = props.class_name();
    let button_type = props.button_type.as_str();
    let disabled = props.disabled;
    let on_click = props.on_click;

    view! {
        <button
            type=button_type
            class=class_name
            title=props.title
            aria-label=props.aria_label
            aria-keyshortcuts=props.aria_keyshortcuts
            disabled=move || disabled.get()
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

pub use button as Button;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusTone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Accent,
    Danger,
}

impl StatusTone {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Neutral => "sp42-status-badge-neutral",
            Self::Info => "sp42-status-badge-info",
            Self::Success => "sp42-status-badge-success",
            Self::Warning => "sp42-status-badge-warning",
            Self::Accent => "sp42-status-badge-accent",
            Self::Danger => "sp42-status-badge-danger",
        }
    }
}

pub struct StatusBadgeProps {
    label: String,
    tone: StatusTone,
    size: Size,
}

impl StatusBadgeProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            tone: StatusTone::default(),
            size: Size::default(),
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: StatusTone) -> Self {
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
        push_class(&mut class_name, self.tone.class_name());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextTone {
    #[default]
    Default,
    Muted,
    Subtle,
    Accent,
    Success,
    Warning,
    Danger,
}

impl TextTone {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Default => "sp42-text-default",
            Self::Muted => "text-muted",
            Self::Subtle => "sp42-text-subtle",
            Self::Accent => "text-accent",
            Self::Success => "text-success",
            Self::Warning => "text-warning",
            Self::Danger => "text-danger",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextSize {
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
}

impl TextSize {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::XSmall => "sp42-text-xs",
            Self::Small => "sp42-text-sm",
            Self::Medium => "sp42-text-md",
            Self::Large => "sp42-text-lg",
        }
    }
}

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
            Self::Truncate => "truncate",
            Self::ClampTwo => "sp42-text-clamp-two",
            Self::PreserveLines => "sp42-text-preserve-lines",
        }
    }
}

pub struct TextProps {
    children: Children,
    tone: TextTone,
    size: TextSize,
    weight: TextWeight,
    element: TextElement,
    mono: bool,
    overflow: TextOverflow,
}

impl TextProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            tone: TextTone::default(),
            size: TextSize::default(),
            weight: TextWeight::default(),
            element: TextElement::default(),
            mono: false,
            overflow: TextOverflow::default(),
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: TextTone) -> Self {
        self.tone = tone;
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: TextSize) -> Self {
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
    pub const fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    #[must_use]
    pub const fn truncate(mut self) -> Self {
        self.overflow = TextOverflow::Truncate;
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
            self.tone.class_name(),
            self.size.class_name(),
            self.weight.class_name(),
        ]);
        if self.mono {
            push_class(&mut class_name, "mono");
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeadingSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl HeadingSize {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Small => "sp42-heading-sm",
            Self::Medium => "sp42-heading-md",
            Self::Large => "sp42-heading-lg",
        }
    }
}

pub struct HeadingProps {
    children: Children,
    level: HeadingLevel,
    size: HeadingSize,
    tone: TextTone,
    align: Align,
}

impl HeadingProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            level: HeadingLevel::default(),
            size: HeadingSize::default(),
            tone: TextTone::default(),
            align: Align::Start,
        }
    }

    #[must_use]
    pub const fn with_level(mut self, level: HeadingLevel) -> Self {
        self.level = level;
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: HeadingSize) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: TextTone) -> Self {
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
            self.size.class_name(),
            self.tone.class_name(),
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
        <header class=class_names(&["sp42-section-header", props.density.class_name()])>
            <span class="section-header">{props.title}</span>
            {actions}
        </header>
    }
}

pub use section_header as SectionHeader;

pub struct FieldProps {
    label: String,
    control: Children,
    hint: String,
    error: String,
    id: String,
    required: bool,
    density: Density,
}

impl FieldProps {
    #[must_use]
    pub fn new(label: impl Into<String>, control: Children) -> Self {
        Self {
            label: label.into(),
            control,
            hint: String::new(),
            error: String::new(),
            id: String::new(),
            required: false,
            density: Density::default(),
        }
    }

    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    #[must_use]
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = error.into();
        self
    }

    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }
}

#[must_use]
pub fn field(props: FieldProps) -> impl IntoView {
    let control = props.control;
    let required = props
        .required
        .then(|| view! { <span class="sp42-field-required">"*"</span> }.into_any());
    let hint = (!props.hint.is_empty())
        .then(|| view! { <p class="sp42-field-hint">{props.hint}</p> }.into_any());
    let error = (!props.error.is_empty())
        .then(|| view! { <p class="sp42-field-error">{props.error}</p> }.into_any());

    view! {
        <label for=props.id class=class_names(&["sp42-field", props.density.class_name()])>
            <span class="sp42-field-label">{props.label}{required}</span>
            {control()}
            {hint}
            {error}
        </label>
    }
}

pub use field as Field;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextInputType {
    #[default]
    Text,
    Search,
    Url,
    Email,
    Password,
    Number,
}

impl TextInputType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Search => "search",
            Self::Url => "url",
            Self::Email => "email",
            Self::Password => "password",
            Self::Number => "number",
        }
    }
}

pub struct TextInputProps {
    id: String,
    name: String,
    value: ValueState,
    placeholder: String,
    input_type: TextInputType,
    disabled: ControlState,
    required: bool,
    density: Density,
    width: ControlWidth,
    on_input: Option<Callback<leptos::ev::Event>>,
    on_change: Option<Callback<leptos::ev::Event>>,
}

impl TextInputProps {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            value: ValueState::default(),
            placeholder: String::new(),
            input_type: TextInputType::default(),
            disabled: ControlState::default(),
            required: false,
            density: Density::default(),
            width: ControlWidth::default(),
            on_input: None,
            on_change: None,
        }
    }

    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    #[must_use]
    pub fn with_value(mut self, value: impl Into<ValueState>) -> Self {
        self.value = value.into();
        self
    }

    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    #[must_use]
    pub const fn with_type(mut self, input_type: TextInputType) -> Self {
        self.input_type = input_type;
        self
    }

    #[must_use]
    pub fn with_disabled(mut self, disabled: impl Into<ControlState>) -> Self {
        self.disabled = disabled.into();
        self
    }

    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }

    #[must_use]
    pub const fn with_width(mut self, width: ControlWidth) -> Self {
        self.width = width;
        self
    }

    #[must_use]
    pub fn on_input<F>(mut self, on_input: F) -> Self
    where
        F: Fn(leptos::ev::Event) + Send + Sync + 'static,
    {
        self.on_input = Some(Callback::new(on_input));
        self
    }

    #[must_use]
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: Fn(leptos::ev::Event) + Send + Sync + 'static,
    {
        self.on_change = Some(Callback::new(on_change));
        self
    }
}

#[must_use]
pub fn text_input(props: TextInputProps) -> impl IntoView {
    let disabled = props.disabled;
    let on_input = props.on_input;
    let on_change = props.on_change;
    let value = props.value;

    view! {
        <input
            id=props.id
            name=props.name
            type=props.input_type.as_str()
            class=class_names(&["sp42-input", props.density.class_name(), props.width.class_name()])
            prop:value=move || value.get()
            placeholder=props.placeholder
            disabled=move || disabled.get()
            required=props.required
            on:input=move |ev| {
                if let Some(callback) = on_input {
                    callback.run(ev);
                }
            }
            on:change=move |ev| {
                if let Some(callback) = on_change {
                    callback.run(ev);
                }
            }
        />
    }
}

pub use text_input as TextInput;

pub struct SelectOption {
    value: String,
    label: String,
    disabled: bool,
}

impl SelectOption {
    #[must_use]
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
        }
    }

    #[must_use]
    pub const fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

pub struct SelectProps {
    id: String,
    name: String,
    value: ValueState,
    options: Vec<SelectOption>,
    disabled: ControlState,
    density: Density,
    width: ControlWidth,
    on_change: Option<Callback<leptos::ev::Event>>,
}

impl SelectProps {
    #[must_use]
    pub fn new(id: impl Into<String>, options: Vec<SelectOption>) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            value: ValueState::default(),
            options,
            disabled: ControlState::default(),
            density: Density::default(),
            width: ControlWidth::default(),
            on_change: None,
        }
    }

    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    #[must_use]
    pub fn with_value(mut self, value: impl Into<ValueState>) -> Self {
        self.value = value.into();
        self
    }

    #[must_use]
    pub fn with_disabled(mut self, disabled: impl Into<ControlState>) -> Self {
        self.disabled = disabled.into();
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }

    #[must_use]
    pub const fn with_width(mut self, width: ControlWidth) -> Self {
        self.width = width;
        self
    }

    #[must_use]
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: Fn(leptos::ev::Event) + Send + Sync + 'static,
    {
        self.on_change = Some(Callback::new(on_change));
        self
    }
}

#[must_use]
pub fn select(props: SelectProps) -> impl IntoView {
    let disabled = props.disabled;
    let on_change = props.on_change;
    let selected_value = props.value;
    let class_name = class_names(&[
        "sp42-select",
        props.density.class_name(),
        props.width.class_name(),
    ]);

    view! {
        <select
            id=props.id
            name=props.name
            class=class_name
            disabled=move || disabled.get()
            on:change=move |ev| {
                if let Some(callback) = on_change {
                    callback.run(ev);
                }
            }
        >
            {props
                .options
                .into_iter()
                .map(|option| {
                    let option_value = option.value.clone();
                    let selected_value = selected_value.clone();
                    view! {
                        <option
                            value=option.value
                            selected=move || option_value == selected_value.get()
                            disabled=option.disabled
                        >
                            {option.label}
                        </option>
                    }
                })
                .collect_view()}
        </select>
    }
}

pub use select as Select;

pub struct CheckboxProps {
    id: String,
    name: String,
    label: String,
    checked: ControlState,
    disabled: ControlState,
    density: Density,
    on_change: Option<Callback<leptos::ev::Event>>,
}

impl CheckboxProps {
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            label: label.into(),
            checked: ControlState::default(),
            disabled: ControlState::default(),
            density: Density::Compact,
            on_change: None,
        }
    }

    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    #[must_use]
    pub fn with_checked(mut self, checked: impl Into<ControlState>) -> Self {
        self.checked = checked.into();
        self
    }

    #[must_use]
    pub fn with_disabled(mut self, disabled: impl Into<ControlState>) -> Self {
        self.disabled = disabled.into();
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }

    #[must_use]
    pub fn on_change<F>(mut self, on_change: F) -> Self
    where
        F: Fn(leptos::ev::Event) + Send + Sync + 'static,
    {
        self.on_change = Some(Callback::new(on_change));
        self
    }
}

#[must_use]
pub fn checkbox(props: CheckboxProps) -> impl IntoView {
    let checked = props.checked;
    let disabled = props.disabled;
    let id = props.id;
    let input_id = id.clone();
    let on_change = props.on_change;

    view! {
        <label for=id class=class_names(&["sp42-checkbox-field", props.density.class_name()])>
            <input
                id=input_id
                name=props.name
                type="checkbox"
                class="sp42-checkbox"
                prop:checked=move || checked.get()
                disabled=move || disabled.get()
                on:change=move |ev| {
                    if let Some(callback) = on_change {
                        callback.run(ev);
                    }
                }
            />
            <span>{props.label}</span>
        </label>
    }
}

pub use checkbox as Checkbox;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpinnerSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl SpinnerSize {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Small => "sp42-spinner-sm",
            Self::Medium => "sp42-spinner-md",
            Self::Large => "sp42-spinner-lg",
        }
    }
}

pub struct SpinnerProps {
    label: String,
    size: SpinnerSize,
}

impl SpinnerProps {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            size: SpinnerSize::default(),
        }
    }

    #[must_use]
    pub const fn with_size(mut self, size: SpinnerSize) -> Self {
        self.size = size;
        self
    }
}

#[must_use]
pub fn spinner(props: SpinnerProps) -> impl IntoView {
    view! {
        <span class=class_names(&["spinner", props.size.class_name()]) role="status" aria-live="polite">
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

pub struct LinkProps {
    label: String,
    href: String,
    external: bool,
    size: TextSize,
}

impl LinkProps {
    #[must_use]
    pub fn new(label: impl Into<String>, href: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            external: false,
            size: TextSize::Small,
        }
    }

    #[must_use]
    pub const fn external(mut self) -> Self {
        self.external = true;
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }
}

#[must_use]
pub fn link(props: LinkProps) -> AnyView {
    let class_name = class_names(&["sp42-link", props.size.class_name()]);

    if props.external {
        view! {
            <a href=props.href target="_blank" rel="noopener" class=class_name>
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScoreTone {
    #[default]
    Low,
    Medium,
    High,
}

impl ScoreTone {
    #[must_use]
    pub const fn for_score(score: i32) -> Self {
        if score >= 70 {
            Self::High
        } else if score >= 30 {
            Self::Medium
        } else {
            Self::Low
        }
    }

    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::High => "!!",
            Self::Medium => "?",
            Self::Low => "\u{2713}",
        }
    }

    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Low => "sp42-score-low",
            Self::Medium => "sp42-score-medium",
            Self::High => "sp42-score-high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreTextProps {
    score: i32,
    tone: ScoreTone,
    size: Size,
    show_icon: bool,
}

impl ScoreTextProps {
    #[must_use]
    pub const fn new(score: i32) -> Self {
        Self {
            score,
            tone: ScoreTone::for_score(score),
            size: Size::Medium,
            show_icon: true,
        }
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub const fn without_icon(mut self) -> Self {
        self.show_icon = false;
        self
    }

    #[must_use]
    pub fn class_name(&self) -> String {
        class_names(&["sp42-score", self.tone.class_name(), self.size.class_name()])
    }
}

#[must_use]
pub fn score_text(props: ScoreTextProps) -> impl IntoView {
    let icon = props
        .show_icon
        .then(|| view! { <span>{props.tone.icon()}</span> }.into_any());

    view! {
        <span class=props.class_name()>
            <span>{props.score}</span>
            {icon}
        </span>
    }
}

pub use score_text as ScoreText;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeltaTone {
    Positive,
    Negative,
    #[default]
    Neutral,
}

impl DeltaTone {
    #[must_use]
    pub const fn for_delta(delta: i32) -> Self {
        if delta > 0 {
            Self::Positive
        } else if delta < 0 {
            Self::Negative
        } else {
            Self::Neutral
        }
    }

    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Positive => "sp42-delta-positive",
            Self::Negative => "sp42-delta-negative",
            Self::Neutral => "sp42-delta-neutral",
        }
    }
}

pub struct DeltaTextProps {
    delta: i32,
    suffix: String,
    size: TextSize,
}

impl DeltaTextProps {
    #[must_use]
    pub fn new(delta: i32) -> Self {
        Self {
            delta,
            suffix: String::new(),
            size: TextSize::Small,
        }
    }

    #[must_use]
    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub fn formatted_value(&self) -> String {
        let value = if self.delta > 0 {
            format!("+{}", self.delta)
        } else {
            self.delta.to_string()
        };
        format!("{value}{}", self.suffix)
    }
}

#[must_use]
pub fn delta_text(props: DeltaTextProps) -> impl IntoView {
    let delta = props.delta;
    let size = props.size;
    let suffix = props.suffix;
    let value = if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    };
    let value = format!("{value}{suffix}");

    view! {
        <span class=class_names(&[
            "sp42-delta",
            DeltaTone::for_delta(delta).class_name(),
            size.class_name()
        ])>
            {value}
        </span>
    }
}

pub use delta_text as DeltaText;

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
    selected: ControlState,
    subdued: bool,
    tone: ScoreTone,
    on_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl NavigationItemProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            selected: ControlState::default(),
            subdued: false,
            tone: ScoreTone::default(),
            on_click: None,
        }
    }

    #[must_use]
    pub fn with_selected(mut self, selected: impl Into<ControlState>) -> Self {
        self.selected = selected.into();
        self
    }

    #[must_use]
    pub const fn subdued(mut self) -> Self {
        self.subdued = true;
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
        navigation_item_class_name(selected, self.subdued, self.tone)
    }
}

#[must_use]
pub fn navigation_item(props: NavigationItemProps) -> impl IntoView {
    let children = props.children;
    let selected = props.selected;
    let subdued = props.subdued;
    let tone = props.tone;
    let on_click = props.on_click;

    view! {
        <button
            type="button"
            class=move || {
                navigation_item_class_name(selected.get(), subdued, tone)
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
fn navigation_item_class_name(selected: bool, subdued: bool, tone: ScoreTone) -> String {
    let mut class_name = String::from("queue-item");
    if selected {
        push_class(&mut class_name, "sp42-nav-item-selected");
        push_class(&mut class_name, tone.class_name());
    }
    if subdued {
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

pub struct ScoreButtonProps {
    score: i32,
    expanded: ControlState,
    title: String,
    on_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl ScoreButtonProps {
    #[must_use]
    pub fn new(score: i32) -> Self {
        Self {
            score,
            expanded: ControlState::default(),
            title: String::new(),
            on_click: None,
        }
    }

    #[must_use]
    pub fn with_expanded(mut self, expanded: impl Into<ControlState>) -> Self {
        self.expanded = expanded.into();
        self
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
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
pub fn score_button(props: ScoreButtonProps) -> impl IntoView {
    let expanded = props.expanded;
    let on_click = props.on_click;

    view! {
        <button
            type="button"
            class="context-score-button"
            aria-expanded=move || expanded.get().to_string()
            title=props.title
            on:click=move |ev| {
                if let Some(callback) = on_click {
                    callback.run(ev);
                }
            }
        >
            {ScoreText(ScoreTextProps::new(props.score))}
        </button>
    }
}

pub use score_button as ScoreButton;

pub struct ScoreDetailsPanelProps {
    children: Children,
}

impl ScoreDetailsPanelProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn score_details_panel(props: ScoreDetailsPanelProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div class="score-details-panel">
            <div class="score-details-summary">
                <span>"Score details"</span>
            </div>
            <ul class="score-details-list">{children()}</ul>
        </div>
    }
}

pub use score_details_panel as ScoreDetailsPanel;

pub struct ScoreDetailItemProps {
    signal: String,
    weight: i32,
    note: Option<String>,
}

impl ScoreDetailItemProps {
    #[must_use]
    pub fn new(signal: impl Into<String>, weight: i32) -> Self {
        Self {
            signal: signal.into(),
            weight,
            note: None,
        }
    }

    #[must_use]
    pub fn with_note(mut self, note: Option<String>) -> Self {
        self.note = note;
        self
    }
}

#[must_use]
pub fn score_detail_item(props: ScoreDetailItemProps) -> impl IntoView {
    let weight = if props.weight > 0 {
        format!("+{}", props.weight)
    } else {
        props.weight.to_string()
    };
    let weight_class = if props.weight > 0 {
        "score-details-weight sp42-score-high"
    } else {
        "score-details-weight sp42-score-low"
    };
    let note = props
        .note
        .map(|note| view! { <div class="score-details-note">{note}</div> }.into_any());

    view! {
        <li class="score-details-item">
            <div class="score-details-line">
                <span class="score-details-signal">{props.signal}</span>
                <span class=weight_class>{weight}</span>
            </div>
            {note}
        </li>
    }
}

pub use score_detail_item as ScoreDetailItem;

pub struct BadgeHeaderProps {
    badges: Children,
    description: String,
}

impl BadgeHeaderProps {
    #[must_use]
    pub fn new(description: impl Into<String>, badges: Children) -> Self {
        Self {
            badges,
            description: description.into(),
        }
    }
}

#[must_use]
pub fn badge_header(props: BadgeHeaderProps) -> impl IntoView {
    let badges = props.badges;

    view! {
        <header class="sp42-badge-header">
            <div class="sp42-badge-row">{badges()}</div>
            <p>{props.description}</p>
        </header>
    }
}

pub use badge_header as BadgeHeader;

pub struct CardHeaderProps {
    title: String,
    actions: Option<Children>,
}

impl CardHeaderProps {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
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
pub fn card_header(props: CardHeaderProps) -> impl IntoView {
    let actions = props
        .actions
        .map(|actions| view! { <div class="sp42-card-actions">{actions()}</div> }.into_any());

    view! {
        <header class="sp42-card-header">
            <h3>{props.title}</h3>
            {actions}
        </header>
    }
}

pub use card_header as CardHeader;

pub struct TextListProps {
    children: Children,
    density: Density,
}

impl TextListProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
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
pub fn text_list(props: TextListProps) -> impl IntoView {
    let children = props.children;

    view! {
        <ul class=class_names(&["sp42-text-list", props.density.class_name()])>
            {children()}
        </ul>
    }
}

pub use text_list as TextList;

pub struct TextListItemProps {
    children: Children,
}

impl TextListItemProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn text_list_item(props: TextListItemProps) -> impl IntoView {
    let children = props.children;

    view! { <li>{children()}</li> }
}

pub use text_list_item as TextListItem;

pub struct CodeBlockProps {
    text: String,
}

impl CodeBlockProps {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[must_use]
pub fn code_block(props: CodeBlockProps) -> impl IntoView {
    view! { <pre class="sp42-code-block">{props.text}</pre> }
}

pub use code_block as CodeBlock;

pub struct ScrollStackProps {
    children: Children,
    density: Density,
}

impl ScrollStackProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            density: Density::Normal,
        }
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }
}

#[must_use]
pub fn scroll_stack(props: ScrollStackProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div class=class_names(&["sp42-scroll-stack", props.density.class_name()])>
            {children()}
        </div>
    }
}

pub use scroll_stack as ScrollStack;

pub struct MediaGalleryPanelProps {
    aria_label: String,
    title: String,
    description: String,
    loading: ControlState,
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
            loading: ControlState::default(),
            children,
        }
    }

    #[must_use]
    pub fn with_loading(mut self, loading: impl Into<ControlState>) -> Self {
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
    tone: TextTone,
    children: Children,
}

impl MediaGroupProps {
    #[must_use]
    pub fn new(title: impl Into<String>, count: usize, children: Children) -> Self {
        Self {
            title: title.into(),
            count,
            tone: TextTone::Default,
            children,
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: TextTone) -> Self {
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
                <strong class=class_names(&["sp42-media-group-title", props.tone.class_name()])>
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

#[must_use]
fn class_names(names: &[&str]) -> String {
    let mut class_name = String::new();
    for name in names {
        push_class(&mut class_name, name);
    }
    class_name
}

fn push_class(class_name: &mut String, name: &str) {
    if name.is_empty() {
        return;
    }
    if !class_name.is_empty() {
        class_name.push(' ');
    }
    class_name.push_str(name);
}

#[cfg(test)]
mod tests {
    use super::{
        ButtonProps, ButtonTone, Density, Gap, GridColumns, GridProps, Size, StatusBadgeProps,
        StatusTone,
    };
    use leptos::prelude::IntoAny;

    #[test]
    fn status_badge_tone_maps_to_design_system_class() {
        let badge = StatusBadgeProps::new("Ready").with_tone(StatusTone::Success);

        assert!(badge.class_name().contains("sp42-status-badge-success"));
        assert!(!badge.class_name().contains("style="));
    }

    #[test]
    fn button_variants_are_composable_classes() {
        let button = ButtonProps::new("Rollback")
            .with_tone(ButtonTone::Danger)
            .with_size(Size::Large)
            .with_density(Density::Comfortable)
            .recommended();

        let class_name = button.class_name();

        assert!(class_name.contains("btn-danger"));
        assert!(class_name.contains("sp42-size-large"));
        assert!(class_name.contains("sp42-density-comfortable"));
        assert!(class_name.contains("btn-recommended"));
    }

    #[test]
    fn grid_class_captures_layout_choices() {
        let grid = GridProps::new(Box::new(|| ().into_any()))
            .with_columns(GridColumns::AutoFit)
            .with_gap(Gap::Large);

        let class_name = grid.class_name();

        assert!(class_name.contains("sp42-grid-auto-fit"));
        assert!(class_name.contains("sp42-gap-lg"));
    }
}
