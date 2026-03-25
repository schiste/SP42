use leptos::prelude::*;

/// Filter parameters sent as query string to `/operator/live/{wiki_id}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatrolFilterParams {
    pub limit: u16,
    pub include_bots: bool,
    pub unpatrolled_only: bool,
    pub include_minor: bool,
    pub namespaces: Option<Vec<i32>>,
    pub min_score: Option<i32>,
    pub rccontinue: Option<String>,
}

impl Default for PatrolFilterParams {
    fn default() -> Self {
        Self {
            limit: 15,
            include_bots: false,
            unpatrolled_only: false,
            include_minor: true,
            namespaces: None,
            min_score: None,
            rccontinue: None,
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
        if let Some(ref ns) = self.namespaces {
            let ns_str: Vec<String> = ns.iter().map(ToString::to_string).collect();
            pairs.push(format!("namespaces={}", ns_str.join(",")));
        }
        if let Some(score) = self.min_score {
            pairs.push(format!("min_score={score}"));
        }
        if let Some(ref token) = self.rccontinue {
            pairs.push(format!("rccontinue={token}"));
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

    let checkbox_style = "accent-color:#3b82f6;width:14px;height:14px;cursor:pointer;";
    let label_style = "display:inline-flex;align-items:center;gap:4px;cursor:pointer;";
    let select_style = "background:#111b2e;color:#eff4ff;border:1px solid rgba(148,163,184,.14);\
                        border-radius:4px;padding:2px 4px;font:inherit;font-size:12px;";

    view! {
        <div style="display:flex;align-items:center;gap:10px;padding:4px 10px;\
                    background:#0b1324;border-block-end:1px solid rgba(148,163,184,.14);\
                    font-size:12px;color:#8b9fc0;flex-wrap:wrap;min-height:34px;">

            // Limit
            <label style=label_style>
                "Limit:"
                <select
                    style=select_style
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

            // Separator
            <span style="color:rgba(148,163,184,.3);">"|"</span>

            // Unpatrolled only
            <label style=label_style>
                <input
                    type="checkbox"
                    style=checkbox_style
                    prop:checked=move || filters.get().unpatrolled_only
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.unpatrolled_only = checked);
                    }
                />
                "Unpatrolled only"
            </label>

            // Hide minor
            <label style=label_style>
                <input
                    type="checkbox"
                    style=checkbox_style
                    prop:checked=move || !filters.get().include_minor
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_minor = !checked);
                    }
                />
                "Hide minor"
            </label>

            // Include bots
            <label style=label_style>
                <input
                    type="checkbox"
                    style=checkbox_style
                    prop:checked=move || filters.get().include_bots
                    on:change=move |ev| {
                        let checked = event_target_checked(&ev);
                        update_filter!(move |f| f.include_bots = checked);
                    }
                />
                "Bots"
            </label>

            <span style="color:rgba(148,163,184,.3);">"|"</span>

            // Min score
            <label style=label_style>
                "Min score:"
                <select
                    style=select_style
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

            <span style="color:rgba(148,163,184,.3);">"|"</span>

            // Namespace toggles
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
                        <label style=format!("{label_style}font-size:11px;")>
                            <input
                                type="checkbox"
                                style=checkbox_style
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

            // Spacer
            <div style="flex:1;"></div>

            // Load older (backlog pagination)
            <button
                style="min-height:27px;padding:2px 10px;border:1px solid rgba(148,163,184,.14);\
                       border-radius:4px;background:transparent;color:#8b9fc0;\
                       font:inherit;font-size:12px;cursor:pointer;"
                disabled=move || next_continue.get().is_none()
                on:click=load_older
            >
                "Load older \u{25b8}"
            </button>
        </div>
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

#[cfg(test)]
mod tests {
    use super::PatrolFilterParams;

    #[test]
    fn default_query_string_contains_limit() {
        let params = PatrolFilterParams::default();
        let qs = params.to_query_string();
        assert!(qs.contains("limit=15"));
        assert!(!qs.contains("include_bots"));
        assert!(!qs.contains("unpatrolled_only"));
        assert!(!qs.contains("include_minor"));
    }

    #[test]
    fn query_string_includes_all_set_params() {
        let params = PatrolFilterParams {
            limit: 50,
            include_bots: true,
            unpatrolled_only: true,
            include_minor: false,
            namespaces: Some(vec![0, 2]),
            min_score: Some(30),
            rccontinue: Some("20260325|abc".to_string()),
        };
        let qs = params.to_query_string();
        assert!(qs.contains("limit=50"));
        assert!(qs.contains("include_bots=true"));
        assert!(qs.contains("unpatrolled_only=true"));
        assert!(qs.contains("include_minor=false"));
        assert!(qs.contains("namespaces=0,2"));
        assert!(qs.contains("min_score=30"));
        assert!(qs.contains("rccontinue=20260325|abc"));
    }
}
