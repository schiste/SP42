use leptos::prelude::*;
use sp42_core::FlagState;
use sp42_live::LiveOperatorQuery;

/// Filter parameters sent as query string to `/operator/live/{wiki_id}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatrolFilterParams {
    pub query: LiveOperatorQuery,
    pub selected_index: Option<usize>,
    pub group_edits: bool,
}

impl Default for PatrolFilterParams {
    fn default() -> Self {
        Self {
            query: LiveOperatorQuery::default(),
            selected_index: None,
            group_edits: false,
        }
    }
}

impl PatrolFilterParams {
    #[must_use]
    pub fn to_query_string(&self) -> String {
        let mut pairs = self
            .query
            .to_query_pairs()
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>();
        if let Some(idx) = self.selected_index {
            pairs.push(format!("selected_index={idx}"));
        }
        pairs.join("&")
    }
}

const NAMESPACE_OPTIONS: &[(i32, &str)] = &[
    (0, "Main"),
    (1, "Talk"),
    (2, "User"),
    (3, "User talk"),
    (4, "Project"),
    (6, "File"),
    (10, "Template"),
    (14, "Category"),
];

#[component]
pub fn FilterBar(
    filters: ReadSignal<PatrolFilterParams>,
    set_filters: WriteSignal<PatrolFilterParams>,
    next_continue: ReadSignal<Option<String>>,
    /// The active wiki's resolved default namespaces — what the server uses for an
    /// unfiltered query. Shown as "checked when no explicit selection" so the
    /// checkboxes match server behavior for configured wikis. Codex review #90.
    default_namespaces: ReadSignal<Vec<i32>>,
) -> impl IntoView {
    macro_rules! update_filter {
        ($body:expr) => {{
            let updater: Box<dyn Fn(&mut PatrolFilterParams)> = Box::new($body);
            let mut current = filters.get();
            current.query.rccontinue = None;
            updater(&mut current);
            set_filters.set(current);
        }};
    }

    let load_older = move |_| {
        if let Some(token) = next_continue.get() {
            let mut current = filters.get();
            current.query.rccontinue = Some(token);
            set_filters.set(current);
        }
    };

    let checkbox_class = "filter-checkbox";
    let label_class = "filter-label";
    let select_class = "filter-select";

    let summary_text = move || {
        let f = filters.get();
        let mut parts = vec![format!("{} edits", f.query.limit)];
        if f.query.unpatrolled_only.is_enabled() {
            parts.push("unpatrolled".to_string());
        }
        if !f.query.include_minor.is_enabled() {
            parts.push("no minor".to_string());
        }
        if f.query.include_bots.is_enabled() {
            parts.push("+ bots".to_string());
        }
        if let Some(ref tag) = f.query.tag_filter {
            parts.push(format!("tag:{tag}"));
        }
        parts.join(", ")
    };

    view! {
        <details class="filter-bar-details">
            <summary class="filter-summary">
                "Filters: " {summary_text}
            </summary>
            <div class="filter-bar">

            <label class=label_class>
                "Limit:"
                <select
                    class=select_class
                    on:change=move |ev| {
                        let value: u16 = event_target_value(&ev).parse().unwrap_or(15);
                        update_filter!(move |f| f.query.limit = value);
                    }
                >
                    <option value="15" selected=move || filters.get().query.limit == 15>"15"</option>
                    <option value="25" selected=move || filters.get().query.limit == 25>"25"</option>
                    <option value="50" selected=move || filters.get().query.limit == 50>"50"</option>
                </select>
            </label>

            <span class="filter-separator">"|"</span>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().query.unpatrolled_only.is_enabled()
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.query.unpatrolled_only = FlagState::from(checked));
                    }
                />
                "Unpatrolled only"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || !filters.get().query.include_minor.is_enabled()
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.query.include_minor = FlagState::from(!checked));
                    }
                />
                "Hide minor"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().query.include_bots.is_enabled()
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.query.include_bots = FlagState::from(checked));
                    }
                />
                "Bots"
            </label>

            <span class="filter-separator">"|"</span>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().query.include_registered.is_enabled()
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.query.include_registered = FlagState::from(checked));
                    }
                />
                "Registered"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().query.include_anonymous.is_enabled()
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.query.include_anonymous = FlagState::from(checked));
                    }
                />
                "Anonymous"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().query.include_temporary.is_enabled()
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.query.include_temporary = FlagState::from(checked));
                    }
                />
                "Temporary"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || !filters.get().query.include_new_pages.is_enabled()
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.query.include_new_pages = FlagState::from(!checked));
                    }
                />
                "Hide new pages"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().group_edits
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.group_edits = checked);
                    }
                />
                "Group edits"
            </label>

            <span class="filter-separator">"|"</span>

            <label class=label_class>
                "Min score:"
                <select
                    class=select_class
                    on:change=move |ev| {
                        let value: i32 = event_target_value(&ev).parse().unwrap_or(0);
                        update_filter!(move |f| f.query.min_score = if value == 0 { None } else { Some(value) });
                    }
                >
                    <option value="0" selected=move || filters.get().query.min_score.is_none()>"0"</option>
                    <option value="10" selected=move || filters.get().query.min_score == Some(10)>"10"</option>
                    <option value="20" selected=move || filters.get().query.min_score == Some(20)>"20"</option>
                    <option value="30" selected=move || filters.get().query.min_score == Some(30)>"30"</option>
                    <option value="50" selected=move || filters.get().query.min_score == Some(50)>"50"</option>
                    <option value="70" selected=move || filters.get().query.min_score == Some(70)>"70"</option>
                </select>
            </label>

            <label class=label_class>
                "Tag:"
                <input
                    type="text"
                    class=select_class
                    style="width:100px;"
                    placeholder="e.g. mw-reverted"
                    prop:value=move || filters.get().query.tag_filter.unwrap_or_default()
                    on:change=move |ev| {
                        let value = event_target_input_value(&ev);
                        update_filter!(move |f| {
                            f.query.tag_filter = if value.trim().is_empty() {
                                None
                            } else {
                                Some(value.trim().to_string())
                            };
                        });
                    }
                />
            </label>

            <span class="filter-separator">"|"</span>

            {NAMESPACE_OPTIONS
                .iter()
                .map(|&(ns, name)| {
                    let is_active = move || {
                        let namespaces = filters.get().query.namespaces;
                        if namespaces.is_empty() {
                            default_namespaces.get().contains(&ns)
                        } else {
                            namespaces.contains(&ns)
                        }
                    };
                    view! {
                        <label class=label_class style="font-size:11px;">
                            <input
                                type="checkbox"
                                class=checkbox_class
                                prop:checked=is_active
                                on:change=move |ev| {
                                    let checked = event_target_checked(&ev);
                                    update_filter!(move |f| {
                                        if f.query.namespaces.is_empty() {
                                            f.query.namespaces = default_namespaces.get_untracked();
                                        }
                                        let list = &mut f.query.namespaces;
                                        if checked && !list.contains(&ns) {
                                            list.push(ns);
                                            list.sort_unstable();
                                        } else if !checked {
                                            list.retain(|&n| n != ns);
                                        }
                                    });
                                }
                            />
                            {name}
                        </label>
                    }
                })
                .collect_view()}

            <div class="flex-spacer"></div>

            <button
                class="btn btn-ghost btn-compact"
                disabled=move || next_continue.get().is_none()
                on:click=load_older
            >
                "Load older \u{25b8}"
            </button>
            </div>
        </details>
    }
}

#[cfg(target_arch = "wasm32")]
fn event_target_value(ev: &leptos::ev::Event) -> String {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlSelectElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn event_target_value(_ev: &leptos::ev::Event) -> String {
    String::new()
}

#[cfg(target_arch = "wasm32")]
fn event_target_checked(ev: &leptos::ev::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.checked())
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn event_target_checked(_ev: &leptos::ev::Event) -> bool {
    false
}

#[cfg(target_arch = "wasm32")]
fn event_target_input_value(ev: &leptos::ev::Event) -> String {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.value())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn event_target_input_value(_ev: &leptos::ev::Event) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::PatrolFilterParams;
    use sp42_core::FlagState;
    use sp42_live::LiveOperatorQuery;

    #[test]
    fn default_query_string_contains_limit() {
        let params = PatrolFilterParams::default();
        let qs = params.to_query_string();
        assert!(qs.contains("limit=15"));
        assert!(!qs.contains("include_bots"));
        assert!(qs.contains("unpatrolled_only=true"));
        assert!(!qs.contains("include_minor"));
        assert!(qs.contains("include_registered=false"));
        assert!(!qs.contains("include_anonymous"));
        assert!(!qs.contains("include_temporary"));
        assert!(!qs.contains("include_new_pages"));
        assert!(!qs.contains("tag_filter"));
        assert!(!qs.contains("selected_index"));
    }

    #[test]
    fn query_string_includes_all_set_params() {
        let params = PatrolFilterParams {
            query: LiveOperatorQuery {
                limit: 50,
                include_bots: FlagState::Enabled,
                include_minor: FlagState::Disabled,
                include_registered: FlagState::Disabled,
                include_anonymous: FlagState::Disabled,
                include_temporary: FlagState::Disabled,
                include_new_pages: FlagState::Disabled,
                namespaces: vec![0, 2],
                min_score: Some(30),
                tag_filter: Some("mw-reverted".to_string()),
                rccontinue: Some("20260325|abc".to_string()),
                ..LiveOperatorQuery::default()
            },
            selected_index: Some(3),
            group_edits: false,
        };
        let qs = params.to_query_string();
        assert!(qs.contains("limit=50"));
        assert!(qs.contains("include_bots=true"));
        assert!(qs.contains("unpatrolled_only=true"));
        assert!(qs.contains("include_minor=false"));
        assert!(qs.contains("include_registered=false"));
        assert!(qs.contains("include_anonymous=false"));
        assert!(qs.contains("include_temporary=false"));
        assert!(qs.contains("include_new_pages=false"));
        assert!(qs.contains("namespaces=0,2"));
        assert!(qs.contains("min_score=30"));
        assert!(qs.contains("tag_filter=mw-reverted"));
        assert!(qs.contains("rccontinue=20260325|abc"));
        assert!(qs.contains("selected_index=3"));
    }
}
