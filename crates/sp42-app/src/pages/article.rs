use leptos::prelude::*;
use sp42_core::{ArticleInventory, ArticleReference, MediaReference};

use crate::platform::article::fetch_article_inventory;
use crate::platform::config::configured_default_wiki_id;

#[component]
pub fn ArticleSurface() -> impl IntoView {
    let (wiki_id, set_wiki_id) = signal(configured_default_wiki_id());
    let (title, set_title) = signal(String::new());
    let (inventory, set_inventory) = signal(None::<ArticleInventory>);
    let (load_error, set_load_error) = signal(None::<String>);
    let (loading, set_loading) = signal(false);

    let load_action = Action::new_local(move |_: &()| {
        let wiki = wiki_id.get_untracked();
        let article_title = title.get_untracked();
        async move {
            let trimmed_title = article_title.trim().to_string();
            if trimmed_title.is_empty() {
                set_load_error.set(Some(
                    "Enter an article title before loading inventory.".to_string(),
                ));
                set_inventory.set(None);
                return;
            }

            set_loading.set(true);
            set_load_error.set(None);
            match fetch_article_inventory(&wiki, &trimmed_title).await {
                Ok(next_inventory) => set_inventory.set(Some(next_inventory)),
                Err(error) => {
                    set_inventory.set(None);
                    set_load_error.set(Some(error));
                }
            }
            set_loading.set(false);
        }
    });

    view! {
        <section class="article-workspace">
            <form
                class="article-command-bar"
                on:submit=move |ev| {
                    ev.prevent_default();
                    load_action.dispatch_local(());
                }
            >
                <div class="article-command-title">
                    <span class="section-header">"Article Workspace"</span>
                    <strong>"Current page inventory"</strong>
                </div>
                <label class="article-field">
                    <span>"Wiki"</span>
                    <input
                        class="article-input article-input-short"
                        type="text"
                        prop:value=move || wiki_id.get()
                        on:input=move |ev| set_wiki_id.set(input_value(&ev))
                    />
                </label>
                <label class="article-field article-field-title">
                    <span>"Title"</span>
                    <input
                        class="article-input"
                        type="text"
                        placeholder="Article title"
                        prop:value=move || title.get()
                        on:input=move |ev| set_title.set(input_value(&ev))
                    />
                </label>
                <button class="btn btn-compact btn-success" type="submit" disabled=move || loading.get()>
                    {move || if loading.get() { "Loading" } else { "Load" }}
                </button>
            </form>

            {move || {
                if let Some(error) = load_error.get() {
                    return view! {
                        <div class="article-state article-state-error">{error}</div>
                    }.into_any();
                }
                if let Some(next_inventory) = inventory.get() {
                    return view! {
                        <ArticleInventoryView inventory=next_inventory />
                    }.into_any();
                }
                view! {
                    <div class="article-state">
                        "Load an article to see sections, citations, templates, categories, media references, and cross-project links."
                    </div>
                }.into_any()
            }}
        </section>
    }
}

#[component]
fn ArticleInventoryView(inventory: ArticleInventory) -> impl IntoView {
    let summary = inventory_summary(&inventory);
    view! {
        <div class="article-inventory">
            <header class="article-inventory-header">
                <div>
                    <span class="section-header">{inventory.wiki_id.clone()}</span>
                    <h1>{inventory.title.clone()}</h1>
                </div>
                <div class="article-stat-grid">
                    {summary
                        .into_iter()
                        .map(|(label, value)| view! {
                            <div class="article-stat">
                                <span>{label}</span>
                                <strong>{value}</strong>
                            </div>
                        })
                        .collect_view()}
                </div>
            </header>

            <div class="article-panels">
                <InventoryPanel title="Sections".to_string() count=inventory.section_count>
                    <CompactList values=inventory.section_headings.clone() empty="No section headings detected.".to_string() />
                </InventoryPanel>
                <InventoryPanel title="References".to_string() count=inventory.references.len()>
                    <ReferenceList references=inventory.references.clone() />
                </InventoryPanel>
                <InventoryPanel title="Templates".to_string() count=inventory.templates.len()>
                    <CompactList values=inventory.templates.clone() empty="No templates detected.".to_string() />
                </InventoryPanel>
                <InventoryPanel title="Categories".to_string() count=inventory.categories.len()>
                    <CompactList values=inventory.categories.clone() empty="No categories detected.".to_string() />
                </InventoryPanel>
                <InventoryPanel title="Media".to_string() count=inventory.media_references.len()>
                    <MediaList references=inventory.media_references.clone() />
                </InventoryPanel>
                <InventoryPanel title="Cross-project links".to_string() count=inventory.interwiki_links.len()>
                    <CompactList values=inventory.interwiki_links.clone() empty="No interwiki or sister-project links detected.".to_string() />
                </InventoryPanel>
            </div>

            <section class="article-notes">
                <span class="section-header">"Readiness Notes"</span>
                {inventory.notes
                    .into_iter()
                    .map(|note| view! { <p>{note}</p> })
                    .collect_view()}
            </section>
        </div>
    }
}

#[component]
fn InventoryPanel(title: String, count: usize, children: Children) -> impl IntoView {
    view! {
        <section class="article-panel">
            <header class="article-panel-header">
                <span>{title}</span>
                <strong>{count}</strong>
            </header>
            {children()}
        </section>
    }
}

#[component]
fn CompactList(values: Vec<String>, empty: String) -> impl IntoView {
    if values.is_empty() {
        return view! { <p class="article-empty">{empty}</p> }.into_any();
    }

    view! {
        <ul class="article-list">
            {values
                .into_iter()
                .take(16)
                .map(|value| view! { <li>{value}</li> })
                .collect_view()}
        </ul>
    }
    .into_any()
}

#[component]
fn ReferenceList(references: Vec<ArticleReference>) -> impl IntoView {
    if references.is_empty() {
        return view! {
            <p class="article-empty">"No <ref> tags detected."</p>
        }
        .into_any();
    }

    view! {
        <div class="article-reference-list">
            {references
                .into_iter()
                .take(12)
                .map(|reference| {
                    let name = reference.name.unwrap_or_else(|| "unnamed".to_string());
                    let status = if reference.has_content { "content" } else { "reuse" };
                    view! {
                        <article class="article-reference">
                            <div class="article-reference-top">
                                <strong>{format!("#{} {name}", reference.ordinal)}</strong>
                                <span>{status}</span>
                            </div>
                            <div class="article-reference-meta">
                                {format!(
                                    "{} citation template(s), {} URL(s)",
                                    reference.citation_template_count,
                                    reference.bare_urls.len()
                                )}
                            </div>
                            <p>{reference.preview}</p>
                        </article>
                    }
                })
                .collect_view()}
        </div>
    }
    .into_any()
}

#[component]
fn MediaList(references: Vec<MediaReference>) -> impl IntoView {
    if references.is_empty() {
        return view! {
            <p class="article-empty">"No file, gallery, or template media references detected."</p>
        }
        .into_any();
    }

    view! {
        <ul class="article-list">
            {references
                .into_iter()
                .take(12)
                .map(|reference| view! {
                    <li>
                        <strong>{reference.display_title}</strong>
                        <span>{reference.usage_signature}</span>
                    </li>
                })
                .collect_view()}
        </ul>
    }
    .into_any()
}

fn inventory_summary(inventory: &ArticleInventory) -> Vec<(&'static str, String)> {
    vec![
        ("Bytes", inventory.byte_len.to_string()),
        ("Sections", inventory.section_count.to_string()),
        ("Refs", inventory.reference_count().to_string()),
        (
            "Cite templates",
            inventory.citation_templates.len().to_string(),
        ),
        (
            "Citation-needed",
            inventory.citation_needed_templates.len().to_string(),
        ),
        ("Media", inventory.media_count().to_string()),
    ]
}

#[cfg(target_arch = "wasm32")]
fn input_value(ev: &leptos::ev::Event) -> String {
    use wasm_bindgen::JsCast;

    ev.target()
        .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|element| element.value())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn input_value(_ev: &leptos::ev::Event) -> String {
    String::new()
}
