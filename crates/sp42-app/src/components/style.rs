/// Map a `wiki_id` (e.g. `"frwiki"`) to its base URL. Falls back to the
/// French Wikipedia for unrecognised identifiers until multi-wiki support
/// is implemented.
pub fn wiki_base_url(wiki_id: &str) -> &'static str {
    match wiki_id {
        "frwiki" => "https://fr.wikipedia.org",
        "enwiki" => "https://en.wikipedia.org",
        "dewiki" => "https://de.wikipedia.org",
        "testwiki" => "https://test.wikipedia.org",
        _ => "https://fr.wikipedia.org",
    }
}

#[cfg(test)]
mod tests {
    use super::wiki_base_url;

    #[test]
    fn wiki_base_url_resolves_known_wikis() {
        assert_eq!(wiki_base_url("frwiki"), "https://fr.wikipedia.org");
        assert_eq!(wiki_base_url("enwiki"), "https://en.wikipedia.org");
    }

    #[test]
    fn wiki_base_url_falls_back_for_unknown() {
        assert_eq!(wiki_base_url("xxwiki"), "https://fr.wikipedia.org");
    }
}
