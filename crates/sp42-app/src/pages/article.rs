use leptos::prelude::*;
use sp42_core::{ArticleInventory, ArticleReference, MediaReference};
use sp42_ui::{
    Button, ButtonProps, ButtonType, CommandBar, CommandBarProps, CommandTitle, CommandTitleProps,
    DataPanel, DataPanelProps, Density, EmptyText, EmptyTextProps, Field, FieldProps,
    InventoryHeader, InventoryHeaderProps, InventoryShell, InventoryShellProps, MetaText,
    MetaTextProps, NotesPanel, NotesPanelProps, PageShell, PageShellProps, PanelGrid,
    PanelGridProps, ResultCard, ResultCardHeader, ResultCardHeaderProps, ResultCardProps,
    ResultList, ResultListProps, Size, StatGrid, StatGridProps, StatItem, StatItemProps,
    StatusRegion, StatusRegionProps, Text, TextElement, TextInput, TextInputProps, TextProps, Tone,
    Width,
};

use crate::components::ui_children;
use crate::platform::article::fetch_article_inventory;
use crate::platform::config::selected_wiki_id;

#[component]
pub fn ArticleSurface() -> impl IntoView {
    let (wiki_id, set_wiki_id) = signal(selected_wiki_id());
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

    PageShell(PageShellProps::new(ui_children(move || {
        view! {
            {CommandBar(
                CommandBarProps::new(ui_children(move || view! {
                    {CommandTitle(CommandTitleProps::new(
                        "Article Workspace",
                        "Current page inventory",
                    ))}
                    {Field(FieldProps::new(
                        "Wiki",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("article-wiki")
                                    .with_value(Signal::derive(move || wiki_id.get()))
                                    .with_width(Width::Short)
                                    .with_density(Density::Compact)
                                    .on_input(move |ev| set_wiki_id.set(input_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Field(FieldProps::new(
                        "Title",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("article-title")
                                    .with_value(Signal::derive(move || title.get()))
                                    .with_placeholder("Article title")
                                    .with_width(Width::Full)
                                    .with_density(Density::Compact)
                                    .on_input(move |ev| set_title.set(input_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Button(
                        ButtonProps::new("Load")
                            .with_type(ButtonType::Submit)
                            .with_tone(Tone::Success)
                            .with_density(Density::Compact)
                            .with_disabled(Signal::derive(move || loading.get()))
                    )}
                }.into_any()))
                .on_submit(move |ev| {
                    ev.prevent_default();
                    load_action.dispatch_local(());
                })
            )}

            {move || {
                if let Some(error) = load_error.get() {
                    return StatusRegion(
                        StatusRegionProps::new(ui_children(move || view! { {error} }.into_any()))
                            .with_tone(Tone::Danger),
                    )
                    .into_any();
                }
                if let Some(next_inventory) = inventory.get() {
                    return view! {
                        <ArticleInventoryView inventory=next_inventory />
                    }.into_any();
                }
                StatusRegion(StatusRegionProps::new(ui_children(|| {
                    view! {
                        "Load an article to see sections, citations, templates, categories, media references, and cross-project links."
                    }
                    .into_any()
                })))
                .into_any()
            }}
        }
        .into_any()
    })))
}

#[component]
fn ArticleInventoryView(inventory: ArticleInventory) -> impl IntoView {
    let summary = inventory_summary(&inventory);
    InventoryShell(InventoryShellProps::new(ui_children(move || {
        view! {
            {InventoryHeader(
                InventoryHeaderProps::new(inventory.wiki_id.clone(), inventory.title.clone())
                    .with_actions(ui_children(move || view! {
                        {StatGrid(StatGridProps::new(ui_children(move || view! {
                            {summary
                                .into_iter()
                                .map(|(label, value)| StatItem(StatItemProps::new(label, value)))
                                .collect_view()}
                        }.into_any())))}
                    }.into_any()))
            )}

            {PanelGrid(PanelGridProps::new(ui_children(move || view! {
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
            }.into_any())))}

            {NotesPanel(NotesPanelProps::new("Readiness Notes", ui_children(move || view! {
                {inventory.notes
                    .into_iter()
                    .map(|note| view! { <p>{note}</p> })
                    .collect_view()}
            }.into_any())))}
        }
        .into_any()
    })))
}

#[component]
fn InventoryPanel(title: String, count: usize, children: Children) -> impl IntoView {
    DataPanel(DataPanelProps::new(title, children).with_count(count.to_string()))
}

#[component]
fn CompactList(values: Vec<String>, empty: String) -> impl IntoView {
    if values.is_empty() {
        return EmptyText(EmptyTextProps::new(empty)).into_any();
    }

    ResultList(ResultListProps::new(ui_children(move || {
        view! {
            {values
                .into_iter()
                .take(16)
                .map(|value| {
                    ResultCard(ResultCardProps::new(ui_children(move || {
                        view! { <span>{value}</span> }.into_any()
                    })))
                })
                .collect_view()}
        }
        .into_any()
    })))
    .into_any()
}

#[component]
fn ReferenceList(references: Vec<ArticleReference>) -> impl IntoView {
    if references.is_empty() {
        return EmptyText(EmptyTextProps::new("No <ref> tags detected.")).into_any();
    }

    ResultList(ResultListProps::new(ui_children(move || {
        view! {
            {references
                .into_iter()
                .take(12)
                .map(|reference| {
                    let ordinal = reference.ordinal;
                    let name = reference.name.unwrap_or_else(|| "unnamed".to_string());
                    let status = if reference.has_content { "content" } else { "reuse" };
                    let citation_count = reference.citation_template_count;
                    let url_count = reference.bare_urls.len();
                    let preview = reference.preview;
                    ResultCard(ResultCardProps::new(ui_children(move || {
                        view! {
                            {ResultCardHeader(
                                ResultCardHeaderProps::new(ui_children(move || {
                                    view! { <strong>{format!("#{ordinal} {name}")}</strong> }
                                        .into_any()
                                }))
                                .with_actions(ui_children(move || {
                                    view! { <span>{status}</span> }.into_any()
                                }))
                            )}
                            {MetaText(MetaTextProps::new(ui_children(move || {
                                view! {
                                    {format!("{citation_count} citation template(s), {url_count} URL(s)")}
                                }
                                .into_any()
                            })))}
                            {Text(
                                TextProps::new(ui_children(move || view! { {preview} }.into_any()))
                                    .with_tone(Tone::Muted)
                                    .with_size(Size::Small)
                                    .with_element(TextElement::Paragraph)
                            )}
                        }
                        .into_any()
                    })))
                })
                .collect_view()}
        }
        .into_any()
    })))
    .into_any()
}

#[component]
fn MediaList(references: Vec<MediaReference>) -> impl IntoView {
    if references.is_empty() {
        return EmptyText(EmptyTextProps::new(
            "No file, gallery, or template media references detected.",
        ))
        .into_any();
    }

    ResultList(ResultListProps::new(ui_children(move || {
        view! {
            {references
                .into_iter()
                .take(12)
                .map(|reference| {
                    ResultCard(ResultCardProps::new(ui_children(move || {
                        view! {
                            <strong>{reference.display_title}</strong>
                            <span>{reference.usage_signature}</span>
                        }
                        .into_any()
                    })))
                })
                .collect_view()}
        }
        .into_any()
    })))
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
