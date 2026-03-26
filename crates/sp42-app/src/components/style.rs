/// Shared UI helpers for runtime-computed values that cannot live in CSS.

/// Map a composite risk score to a (color, icon) pair for consistent tier
/// rendering across the queue column and context sidebar.
pub fn score_tier(score: i32) -> (&'static str, &'static str) {
    if score >= 70 {
        ("#ef4444", "!!")
    } else if score >= 30 {
        ("#f59e0b", "?")
    } else {
        ("#22c55e", "\u{2713}")
    }
}

/// Map a `wiki_id` (e.g. `"frwiki"`) to its base URL. Falls back to the
/// French Wikipedia for unrecognised identifiers until multi-wiki support
/// is implemented.
pub fn wiki_base_url(wiki_id: &str) -> &'static str {
    match wiki_id {
        "frwiki" => "https://fr.wikipedia.org",
        "enwiki" => "https://en.wikipedia.org",
        "dewiki" => "https://de.wikipedia.org",
        _ => "https://fr.wikipedia.org",
    }
}

#[cfg(test)]
mod tests {
    use super::{score_tier, wiki_base_url};

    #[test]
    fn score_tier_maps_thresholds() {
        assert_eq!(score_tier(70).0, "#ef4444");
        assert_eq!(score_tier(30).0, "#f59e0b");
        assert_eq!(score_tier(0).0, "#22c55e");
    }

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
