//! Data-display primitives and compact report patterns.

use leptos::prelude::*;

use super::layout::{Density, Size, State, Tone};
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
    state: ScoreTextState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScoreTextState {
    #[default]
    WithIcon,
    TextOnly,
}

impl ScoreTextProps {
    #[must_use]
    pub const fn new(score: i32) -> Self {
        Self {
            score,
            tone: ScoreTone::for_score(score),
            size: Size::Medium,
            state: ScoreTextState::WithIcon,
        }
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub const fn with_state(mut self, state: ScoreTextState) -> Self {
        self.state = state;
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
        .state
        .eq(&ScoreTextState::WithIcon)
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
    size: Size,
}

impl DeltaTextProps {
    #[must_use]
    pub fn new(delta: i32) -> Self {
        Self {
            delta,
            suffix: String::new(),
            size: Size::Small,
        }
    }

    #[must_use]
    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
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
            size.text_class_name()
        ])>
            {value}
        </span>
    }
}

pub use delta_text as DeltaText;
pub struct ScoreButtonProps {
    score: i32,
    expanded: State,
    title: String,
    on_click: Option<Callback<leptos::ev::MouseEvent>>,
}

impl ScoreButtonProps {
    #[must_use]
    pub fn new(score: i32) -> Self {
        Self {
            score,
            expanded: State::default(),
            title: String::new(),
            on_click: None,
        }
    }

    #[must_use]
    pub fn with_state(mut self, expanded: impl Into<State>) -> Self {
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
            class="sp42-context-score-button"
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
        <div class="sp42-score-details-panel">
            <div class="sp42-score-details-summary">
                <span>"Score details"</span>
            </div>
            <ul class="sp42-score-details-list">{children()}</ul>
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
        "sp42-score-details-weight sp42-score-high"
    } else {
        "sp42-score-details-weight sp42-score-low"
    };
    let note = props
        .note
        .map(|note| view! { <div class="sp42-score-details-note">{note}</div> }.into_any());

    view! {
        <li class="sp42-score-details-item">
            <div class="sp42-score-details-line">
                <span class="sp42-score-details-signal">{props.signal}</span>
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

pub struct ResultListProps {
    children: Children,
    density: Density,
}

impl ResultListProps {
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
pub fn result_list(props: ResultListProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div class=class_names(&["sp42-result-list", props.density.class_name()])>
            {children()}
        </div>
    }
}

pub use result_list as ResultList;

pub struct ResultCardProps {
    children: Children,
    tone: Tone,
    density: Density,
}

impl ResultCardProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            tone: Tone::default(),
            density: Density::Compact,
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    #[must_use]
    pub const fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self
    }
}

#[must_use]
pub fn result_card(props: ResultCardProps) -> impl IntoView {
    let children = props.children;

    view! {
        <article class=class_names(&[
            "sp42-result-card",
            result_tone_class_name(props.tone),
            props.density.class_name()
        ])>
            {children()}
        </article>
    }
}

pub use result_card as ResultCard;

pub struct ResultCardHeaderProps {
    leading: Children,
    actions: Option<Children>,
}

impl ResultCardHeaderProps {
    #[must_use]
    pub fn new(leading: Children) -> Self {
        Self {
            leading,
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
pub fn result_card_header(props: ResultCardHeaderProps) -> impl IntoView {
    let leading = props.leading;
    let actions = props.actions.map(|actions| {
        view! { <div class="sp42-result-card-actions">{actions()}</div> }.into_any()
    });

    view! {
        <header class="sp42-result-card-header">
            <div class="sp42-result-card-title">{leading()}</div>
            {actions}
        </header>
    }
}

pub use result_card_header as ResultCardHeader;

pub struct ResultDisclosureProps {
    summary: Children,
    children: Children,
    tone: Tone,
    open: State,
}

impl ResultDisclosureProps {
    #[must_use]
    pub fn new(summary: Children, children: Children) -> Self {
        Self {
            summary,
            children,
            tone: Tone::default(),
            open: State::default(),
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    #[must_use]
    pub fn with_state(mut self, open: impl Into<State>) -> Self {
        self.open = open.into();
        self
    }
}

#[must_use]
pub fn result_disclosure(props: ResultDisclosureProps) -> impl IntoView {
    let summary = props.summary;
    let children = props.children;
    let open = props.open;

    view! {
        <details
            class=class_names(&["sp42-result-disclosure", result_tone_class_name(props.tone)])
            open=move || open.get()
        >
            <summary>{summary()}</summary>
            <div class="sp42-result-disclosure-body">{children()}</div>
        </details>
    }
}

pub use result_disclosure as ResultDisclosure;

pub struct MetaTextProps {
    children: Children,
    density: Density,
}

impl MetaTextProps {
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
pub fn meta_text(props: MetaTextProps) -> impl IntoView {
    let children = props.children;

    view! {
        <div class=class_names(&["sp42-meta-text", props.density.class_name()])>
            {children()}
        </div>
    }
}

pub use meta_text as MetaText;

pub struct EmptyTextProps {
    message: String,
}

impl EmptyTextProps {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[must_use]
pub fn empty_text(props: EmptyTextProps) -> impl IntoView {
    view! { <p class="sp42-empty-text">{props.message}</p> }
}

pub use empty_text as EmptyText;

pub struct NotesPanelProps {
    title: String,
    children: Children,
}

impl NotesPanelProps {
    #[must_use]
    pub fn new(title: impl Into<String>, children: Children) -> Self {
        Self {
            title: title.into(),
            children,
        }
    }
}

#[must_use]
pub fn notes_panel(props: NotesPanelProps) -> impl IntoView {
    let children = props.children;

    view! {
        <section class="sp42-notes-panel">
            <span class="sp42-section-label">{props.title}</span>
            {children()}
        </section>
    }
}

pub use notes_panel as NotesPanel;

pub struct RawReportDisclosureProps {
    summary: String,
    text: String,
}

impl RawReportDisclosureProps {
    #[must_use]
    pub fn new(summary: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            text: text.into(),
        }
    }
}

#[must_use]
pub fn raw_report_disclosure(props: RawReportDisclosureProps) -> impl IntoView {
    view! {
        <details class="sp42-raw-report">
            <summary>{props.summary}</summary>
            <pre class="sp42-raw-report-body">{props.text}</pre>
        </details>
    }
}

pub use raw_report_disclosure as RawReportDisclosure;

pub struct EvidenceDisclosureProps {
    summary: String,
    children: Children,
}

impl EvidenceDisclosureProps {
    #[must_use]
    pub fn new(summary: impl Into<String>, children: Children) -> Self {
        Self {
            summary: summary.into(),
            children,
        }
    }
}

#[must_use]
pub fn evidence_disclosure(props: EvidenceDisclosureProps) -> impl IntoView {
    let children = props.children;

    view! {
        <details class="sp42-evidence-disclosure">
            <summary>{props.summary}</summary>
            {children()}
        </details>
    }
}

pub use evidence_disclosure as EvidenceDisclosure;

pub struct EvidenceBlockProps {
    label: String,
    text: String,
    tone: Tone,
}

impl EvidenceBlockProps {
    #[must_use]
    pub fn new(label: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            text: text.into(),
            tone: Tone::default(),
        }
    }

    #[must_use]
    pub const fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

#[must_use]
pub fn evidence_block(props: EvidenceBlockProps) -> impl IntoView {
    view! {
        <div class="sp42-evidence-block">
            <span>{props.label}</span>
            <blockquote class=class_names(&[
                "sp42-evidence-quote",
                result_tone_class_name(props.tone)
            ])>
                {props.text}
            </blockquote>
        </div>
    }
}

pub use evidence_block as EvidenceBlock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutDefinition {
    pub label: String,
    pub keys: String,
}

impl ShortcutDefinition {
    #[must_use]
    pub fn new(label: impl Into<String>, keys: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            keys: keys.into(),
        }
    }
}

pub struct ShortcutListProps {
    shortcuts: Vec<ShortcutDefinition>,
}

impl ShortcutListProps {
    #[must_use]
    pub fn new(shortcuts: Vec<ShortcutDefinition>) -> Self {
        Self { shortcuts }
    }
}

#[must_use]
pub fn shortcut_list(props: ShortcutListProps) -> impl IntoView {
    view! {
        <div class="sp42-shortcut-list">
            {props
                .shortcuts
                .into_iter()
                .map(|shortcut| ShortcutRow(ShortcutRowProps::new(shortcut.label, shortcut.keys)))
                .collect_view()}
        </div>
    }
}

pub use shortcut_list as ShortcutList;

pub struct ShortcutRowProps {
    label: String,
    keys: String,
}

impl ShortcutRowProps {
    #[must_use]
    pub fn new(label: impl Into<String>, keys: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            keys: keys.into(),
        }
    }
}

#[must_use]
pub fn shortcut_row(props: ShortcutRowProps) -> impl IntoView {
    view! {
        <div class="sp42-shortcut-row">
            <span>{props.label}</span>
            <kbd>{props.keys}</kbd>
        </div>
    }
}

pub use shortcut_row as ShortcutRow;

#[must_use]
fn result_tone_class_name(tone: Tone) -> &'static str {
    match tone {
        Tone::Success => "sp42-tone-success",
        Tone::Warning => "sp42-tone-warning",
        Tone::Danger => "sp42-tone-danger",
        Tone::Info => "sp42-tone-info",
        Tone::Accent => "sp42-tone-accent",
        Tone::Muted => "sp42-tone-muted",
        Tone::Subtle => "sp42-tone-subtle",
        Tone::Default => "sp42-tone-default",
    }
}
