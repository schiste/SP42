use leptos::prelude::*;

/// Filter parameters sent as query string to `/operator/live/{wiki_id}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatrolFilterParams {
    pub limit: u16,
    pub include_bots: bool,
    pub unpatrolled_only: bool,
    pub include_minor: bool,
    pub include_registered: bool,
    pub include_anonymous: bool,
    pub include_temporary: bool,
    pub include_new_pages: bool,
    pub namespaces: Option<Vec<i32>>,
    pub min_score: Option<i32>,
    pub tag_filter: Option<String>,
    pub rccontinue: Option<String>,
    pub selected_index: Option<usize>,
}

impl Default for PatrolFilterParams {
    fn default() -> Self {
        Self {
            limit: 15,
            include_bots: false,
            unpatrolled_only: true,
            include_minor: true,
            include_registered: false,
            include_anonymous: true,
            include_temporary: true,
            include_new_pages: true,
            namespaces: None,
            min_score: None,
            tag_filter: None,
            rccontinue: None,
            selected_index: None,
        }
    }
}

impl PatrolFilterParams {
    #[must_use]
    pub fn to_query_string(&self) -> String {
        let mut pairs = Vec::new();
        pairs.push(format!("limit={}", self.limit));
        if self.include_bots {
            pairs.push("include_bots=true".to_string());
        }
        if self.unpatrolled_only {
            pairs.push("unpatrolled_only=true".to_string());
        }
        if !self.include_minor {
            pairs.push("include_minor=false".to_string());
        }
        if !self.include_registered {
            pairs.push("include_registered=false".to_string());
        }
        if !self.include_anonymous {
            pairs.push("include_anonymous=false".to_string());
        }
        if !self.include_temporary {
            pairs.push("include_temporary=false".to_string());
        }
        if !self.include_new_pages {
            pairs.push("include_new_pages=false".to_string());
        }
        if let Some(ref ns) = self.namespaces {
            let ns_str: Vec<String> = ns.iter().map(ToString::to_string).collect();
            pairs.push(format!("namespaces={}", ns_str.join(",")));
        }
        if let Some(score) = self.min_score {
            pairs.push(format!("min_score={score}"));
        }
        if let Some(ref tag) = self.tag_filter {
            pairs.push(format!("tag_filter={tag}"));
        }
        if let Some(ref token) = self.rccontinue {
            pairs.push(format!("rccontinue={token}"));
        }
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

const DEFAULT_NAMESPACES: &[i32] = &[0, 2, 4, 6, 10, 14];

#[component]
pub fn FilterBar(
    filters: ReadSignal<PatrolFilterParams>,
    set_filters: WriteSignal<PatrolFilterParams>,
    next_continue: ReadSignal<Option<String>>,
) -> impl IntoView {
    macro_rules! update_filter {
        ($body:expr) => {{
            let updater: Box<dyn Fn(&mut PatrolFilterParams)> = Box::new($body);
            let mut current = filters.get();
            current.rccontinue = None;
            updater(&mut current);
            set_filters.set(current);
        }};
    }

    let load_older = move |_| {
        if let Some(token) = next_continue.get() {
            let mut current = filters.get();
            current.rccontinue = Some(token);
            set_filters.set(current);
        }
    };

    let checkbox_class = "filter-checkbox";
    let label_class = "filter-label";
    let select_class = "filter-select";

    let summary_text = move || {
        let f = filters.get();
        let mut parts = vec![format!("{} edits", f.limit)];
        if f.unpatrolled_only {
            parts.push("unpatrolled".to_string());
        }
        if !f.include_minor {
            parts.push("no minor".to_string());
        }
        if f.include_bots {
            parts.push("+ bots".to_string());
        }
        if let Some(ref tag) = f.tag_filter {
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
                        update_filter!(move |f| f.limit = value);
                    }
                >
                    <option value="15" selected=move || filters.get().limit == 15>"15"</option>
                    <option value="25" selected=move || filters.get().limit == 25>"25"</option>
                    <option value="50" selected=move || filters.get().limit == 50>"50"</option>
                </select>
            </label>

            <span class="filter-separator">"|"</span>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().unpatrolled_only
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.unpatrolled_only = checked);
                    }
                />
                "Unpatrolled only"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || !filters.get().include_minor
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_minor = !checked);
                    }
                />
                "Hide minor"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().include_bots
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_bots = checked);
                    }
                />
                "Bots"
            </label>

            <span class="filter-separator">"|"</span>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().include_registered
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_registered = checked);
                    }
                />
                "Registered"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().include_anonymous
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_anonymous = checked);
                    }
                />
                "Anonymous"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || filters.get().include_temporary
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_temporary = checked);
                    }
                />
                "Temporary"
            </label>

            <label class=label_class>
                <input
                    type="checkbox"
                    class=checkbox_class
                    prop:checked=move || !filters.get().include_new_pages
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_new_pages = !checked);
                    }
                />
                "Hide new pages"
            </label>

            <span class="filter-separator">"|"</span>

            <label class=label_class>
                "Min score:"
                <select
                    class=select_class
                    on:change=move |ev| {
                        let value: i32 = event_target_value(&ev).parse().unwrap_or(0);
                        update_filter!(move |f| f.min_score = if value == 0 { None } else { Some(value) });
                    }
                >
                    <option value="0" selected=move || filters.get().min_score.is_none()>"0"</option>
                    <option value="10" selected=move || filters.get().min_score == Some(10)>"10"</option>
                    <option value="20" selected=move || filters.get().min_score == Some(20)>"20"</option>
                    <option value="30" selected=move || filters.get().min_score == Some(30)>"30"</option>
                    <option value="50" selected=move || filters.get().min_score == Some(50)>"50"</option>
                    <option value="70" selected=move || filters.get().min_score == Some(70)>"70"</option>
                </select>
            </label>

            <label class=label_class>
                "Tag:"
                <input
                    type="text"
                    class=select_class
                    style="width:100px;"
                    placeholder="e.g. mw-reverted"
                    prop:value=move || filters.get().tag_filter.unwrap_or_default()
                    on:change=move |ev| {
                        let value = event_target_input_value(&ev);
                        update_filter!(move |f| {
                            f.tag_filter = if value.trim().is_empty() {
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
                        filters
                            .get()
                            .namespaces
                            .as_ref()
                            .map_or(DEFAULT_NAMESPACES.contains(&ns), |list| list.contains(&ns))
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
                                        let list = f
                                            .namespaces
                                            .get_or_insert_with(|| DEFAULT_NAMESPACES.to_vec());
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
            limit: 50,
            include_bots: true,
            unpatrolled_only: true,
            include_minor: false,
            include_registered: false,
            include_anonymous: false,
            include_temporary: false,
            include_new_pages: false,
            namespaces: Some(vec![0, 2]),
            min_score: Some(30),
            tag_filter: Some("mw-reverted".to_string()),
            rccontinue: Some("20260325|abc".to_string()),
            selected_index: Some(3),
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
