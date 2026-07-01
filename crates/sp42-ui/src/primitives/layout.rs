//! Layout primitives and shared presentation variants.

use leptos::prelude::*;

use super::util::{class_names, push_class};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Static(bool),
    Signal(Signal<bool>),
}

impl Default for State {
    fn default() -> Self {
        Self::Static(false)
    }
}

impl State {
    #[must_use]
    pub fn get(self) -> bool {
        match self {
            Self::Static(value) => value,
            Self::Signal(value) => value.get(),
        }
    }
}

impl From<bool> for State {
    fn from(value: bool) -> Self {
        Self::Static(value)
    }
}

impl From<Signal<bool>> for State {
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
    XSmall,
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    #[must_use]
    pub const fn class_name(self) -> &'static str {
        match self {
            Self::XSmall => "sp42-size-xs",
            Self::Small => "sp42-size-small",
            Self::Medium => "sp42-size-medium",
            Self::Large => "sp42-size-large",
        }
    }

    #[must_use]
    pub const fn text_class_name(self) -> &'static str {
        match self {
            Self::XSmall => "sp42-text-xs",
            Self::Small => "sp42-text-sm",
            Self::Medium => "sp42-text-md",
            Self::Large => "sp42-text-lg",
        }
    }

    #[must_use]
    pub const fn heading_class_name(self) -> &'static str {
        match self {
            Self::XSmall | Self::Small => "sp42-heading-sm",
            Self::Medium => "sp42-heading-md",
            Self::Large => "sp42-heading-lg",
        }
    }

    #[must_use]
    pub const fn modal_class_name(self) -> &'static str {
        match self {
            Self::XSmall | Self::Small => "sp42-modal-sm",
            Self::Medium => "sp42-modal-md",
            Self::Large => "sp42-modal-lg",
        }
    }

    #[must_use]
    pub const fn spinner_class_name(self) -> &'static str {
        match self {
            Self::XSmall | Self::Small => "sp42-spinner-sm",
            Self::Medium => "sp42-spinner-md",
            Self::Large => "sp42-spinner-lg",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Width {
    #[default]
    Auto,
    Short,
    Medium,
    Full,
}

impl Width {
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
pub enum Tone {
    #[default]
    Default,
    Muted,
    Subtle,
    Info,
    Accent,
    Success,
    Warning,
    Danger,
}

impl Tone {
    #[must_use]
    pub const fn button_class_name(self) -> &'static str {
        match self {
            Self::Accent => "sp42-button-accent",
            Self::Success => "sp42-button-success",
            Self::Warning => "sp42-button-warning",
            Self::Danger => "sp42-button-danger",
            Self::Default | Self::Muted | Self::Subtle | Self::Info => "",
        }
    }

    #[must_use]
    pub const fn status_class_name(self) -> &'static str {
        match self {
            Self::Info => "sp42-status-badge-info",
            Self::Success => "sp42-status-badge-success",
            Self::Warning => "sp42-status-badge-warning",
            Self::Accent => "sp42-status-badge-accent",
            Self::Danger => "sp42-status-badge-danger",
            Self::Default | Self::Muted | Self::Subtle => "sp42-status-badge-neutral",
        }
    }

    #[must_use]
    pub const fn text_class_name(self) -> &'static str {
        match self {
            Self::Default | Self::Info => "sp42-text-default",
            Self::Muted => "sp42-text-muted",
            Self::Subtle => "sp42-text-subtle",
            Self::Accent => "sp42-text-accent",
            Self::Success => "sp42-text-success",
            Self::Warning => "sp42-text-warning",
            Self::Danger => "sp42-text-danger",
        }
    }

    #[must_use]
    pub const fn diff_badge_class_name(self) -> &'static str {
        match self {
            Self::Accent => "sp42-diff-badge-accent",
            Self::Default
            | Self::Muted
            | Self::Subtle
            | Self::Info
            | Self::Success
            | Self::Warning
            | Self::Danger => "sp42-diff-badge-neutral",
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
pub enum InlineState {
    #[default]
    Wrap,
    NoWrap,
}

impl InlineState {
    #[must_use]
    pub const fn wraps(self) -> bool {
        matches!(self, Self::Wrap)
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
            "sp42-panel",
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
        class_names(&[
            "sp42-card",
            self.surface.class_name(),
            self.density.class_name(),
        ])
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
    state: InlineState,
}

impl InlineProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            gap: Gap::default(),
            align: Align::Center,
            justify: Justify::default(),
            state: InlineState::default(),
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
    pub const fn with_state(mut self, state: InlineState) -> Self {
        self.state = state;
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
        if self.state.wraps() {
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

pub struct PageShellProps {
    children: Children,
}

impl PageShellProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn page_shell(props: PageShellProps) -> impl IntoView {
    let children = props.children;

    view! { <section class="sp42-page-shell">{children()}</section> }
}

pub use page_shell as PageShell;

pub struct CommandBarProps {
    children: Children,
    on_submit: Option<Callback<leptos::ev::SubmitEvent>>,
}

impl CommandBarProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
            on_submit: None,
        }
    }

    #[must_use]
    pub fn on_submit<F>(mut self, on_submit: F) -> Self
    where
        F: Fn(leptos::ev::SubmitEvent) + Send + Sync + 'static,
    {
        self.on_submit = Some(Callback::new(on_submit));
        self
    }
}

#[must_use]
pub fn command_bar(props: CommandBarProps) -> impl IntoView {
    let children = props.children;
    let on_submit = props.on_submit;

    view! {
        <form
            class="sp42-command-bar"
            on:submit=move |ev| {
                if let Some(callback) = on_submit {
                    callback.run(ev);
                }
            }
        >
            {children()}
        </form>
    }
}

pub use command_bar as CommandBar;

pub struct CommandTitleProps {
    eyebrow: String,
    title: String,
}

impl CommandTitleProps {
    #[must_use]
    pub fn new(eyebrow: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            eyebrow: eyebrow.into(),
            title: title.into(),
        }
    }
}

#[must_use]
pub fn command_title(props: CommandTitleProps) -> impl IntoView {
    view! {
        <div class="sp42-command-title">
            <span class="sp42-section-label">{props.eyebrow}</span>
            <strong>{props.title}</strong>
        </div>
    }
}

pub use command_title as CommandTitle;

pub struct InventoryShellProps {
    children: Children,
}

impl InventoryShellProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn inventory_shell(props: InventoryShellProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-inventory-shell">{children()}</div> }
}

pub use inventory_shell as InventoryShell;

pub struct InventoryHeaderProps {
    eyebrow: String,
    title: String,
    actions: Option<Children>,
}

impl InventoryHeaderProps {
    #[must_use]
    pub fn new(eyebrow: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            eyebrow: eyebrow.into(),
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
pub fn inventory_header(props: InventoryHeaderProps) -> impl IntoView {
    let actions = props.actions.map(|actions| actions().into_any());

    view! {
        <header class="sp42-inventory-header">
            <div>
                <span class="sp42-section-label">{props.eyebrow}</span>
                <h1>{props.title}</h1>
            </div>
            {actions}
        </header>
    }
}

pub use inventory_header as InventoryHeader;

pub struct StatGridProps {
    children: Children,
}

impl StatGridProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn stat_grid(props: StatGridProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-stat-grid">{children()}</div> }
}

pub use stat_grid as StatGrid;

pub struct StatItemProps {
    label: String,
    value: String,
}

impl StatItemProps {
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[must_use]
pub fn stat_item(props: StatItemProps) -> impl IntoView {
    view! {
        <div class="sp42-stat-item">
            <span>{props.label}</span>
            <strong>{props.value}</strong>
        </div>
    }
}

pub use stat_item as StatItem;

pub struct PanelGridProps {
    children: Children,
}

impl PanelGridProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn panel_grid(props: PanelGridProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-panel-grid">{children()}</div> }
}

pub use panel_grid as PanelGrid;

pub struct DataPanelProps {
    title: String,
    count: Option<String>,
    children: Children,
}

impl DataPanelProps {
    #[must_use]
    pub fn new(title: impl Into<String>, children: Children) -> Self {
        Self {
            title: title.into(),
            count: None,
            children,
        }
    }

    #[must_use]
    pub fn with_count(mut self, count: impl Into<String>) -> Self {
        self.count = Some(count.into());
        self
    }
}

#[must_use]
pub fn data_panel(props: DataPanelProps) -> impl IntoView {
    let children = props.children;
    let count = props
        .count
        .map(|count| view! { <strong>{count}</strong> }.into_any());

    view! {
        <section class="sp42-data-panel">
            <header class="sp42-data-panel-header">
                <span>{props.title}</span>
                {count}
            </header>
            {children()}
        </section>
    }
}

pub use data_panel as DataPanel;

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

pub struct StatusBarProps {
    children: Children,
}

impl StatusBarProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn status_bar(props: StatusBarProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-status-bar">{children()}</div> }
}

pub use status_bar as StatusBar;

pub struct ActionBarShellProps {
    children: Children,
}

impl ActionBarShellProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn action_bar_shell(props: ActionBarShellProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-action-bar">{children()}</div> }
}

pub use action_bar_shell as ActionBarShell;

pub struct SplitWorkAreaProps {
    primary: Children,
    aside: Option<Children>,
}

impl SplitWorkAreaProps {
    #[must_use]
    pub fn new(primary: Children) -> Self {
        Self {
            primary,
            aside: None,
        }
    }

    #[must_use]
    pub fn with_aside(mut self, aside: Children) -> Self {
        self.aside = Some(aside);
        self
    }
}

#[must_use]
pub fn split_work_area(props: SplitWorkAreaProps) -> impl IntoView {
    let primary = props.primary;
    let has_aside = props.aside.is_some();
    let class_name = class_names(&[
        "sp42-split-work-area",
        if has_aside {
            "sp42-split-work-area-with-aside"
        } else {
            ""
        },
    ]);
    let aside = props
        .aside
        .map(|aside| view! { <aside class="sp42-split-work-aside">{aside()}</aside> }.into_any());

    view! {
        <div class=class_name>
            <div class="sp42-split-work-primary">{primary()}</div>
            {aside}
        </div>
    }
}

pub use split_work_area as SplitWorkArea;

pub struct WorkspaceMainProps {
    children: Children,
}

impl WorkspaceMainProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn workspace_main(props: WorkspaceMainProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-workspace-main">{children()}</div> }
}

pub use workspace_main as WorkspaceMain;

pub struct WorkspaceGridProps {
    children: Children,
    on_keydown: Option<Callback<leptos::ev::KeyboardEvent>>,
}

impl WorkspaceGridProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self {
            children,
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
pub fn workspace_grid(props: WorkspaceGridProps) -> impl IntoView {
    let children = props.children;
    let on_keydown = props.on_keydown;

    view! {
        <div
            tabindex="0"
            class="sp42-workspace-grid"
            on:keydown=move |event| {
                if let Some(callback) = on_keydown {
                    callback.run(event);
                }
            }
        >
            {children()}
        </div>
    }
}

pub use workspace_grid as WorkspaceGrid;

pub struct GateShellProps {
    children: Children,
}

impl GateShellProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn gate_shell(props: GateShellProps) -> impl IntoView {
    let children = props.children;

    view! { <div class="sp42-gate-shell">{children()}</div> }
}

pub use gate_shell as GateShell;

pub struct GateCardProps {
    children: Children,
}

impl GateCardProps {
    #[must_use]
    pub fn new(children: Children) -> Self {
        Self { children }
    }
}

#[must_use]
pub fn gate_card(props: GateCardProps) -> impl IntoView {
    let children = props.children;

    view! { <section class="sp42-gate-card">{children()}</section> }
}

pub use gate_card as GateCard;

#[must_use]
pub fn spacer() -> impl IntoView {
    view! { <div class="sp42-flex-spacer"></div> }
}

pub use spacer as Spacer;

#[must_use]
pub fn separator() -> impl IntoView {
    view! { <span class="sp42-separator">"|"</span> }
}

pub use separator as Separator;
