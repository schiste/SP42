use sp42_core::{ArticleInventory, routes};

#[cfg(target_arch = "wasm32")]
use super::{config::api_url, http::get_bytes};

#[cfg(target_arch = "wasm32")]
pub async fn fetch_article_inventory(
    wiki_id: &str,
    title: &str,
) -> Result<ArticleInventory, String> {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("title", title)
        .finish();
    let url = routes::with_optional_query(routes::operator_article_path(wiki_id), &query);
    let bytes = get_bytes(&api_url(&url), "fetch article inventory").await?;

    serde_json::from_slice(&bytes).map_err(|error| format!("parse article inventory: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_article_inventory(
    _wiki_id: &str,
    _title: &str,
) -> Result<ArticleInventory, String> {
    Err("Article inventory fetch is only available in the browser runtime.".to_string())
}
