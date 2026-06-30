//! Interactive controls and form primitives.

use leptos::prelude::*;

use super::layout::{ControlState, ControlWidth, Density, Size, ValueState};
use super::util::{class_names, push_class};

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
