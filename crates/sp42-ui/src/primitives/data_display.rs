//! Data-display primitives and compact report patterns.

use leptos::prelude::*;

use super::layout::{ControlState, Density, Size};
use super::typography::TextSize;
use super::util::class_names;

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
