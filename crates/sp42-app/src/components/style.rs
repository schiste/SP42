/// Shared UI helpers for runtime-computed values that cannot live in CSS.

/// Map a composite risk score to a (color, icon) pair for consistent tier
/// rendering across the queue column and context sidebar.
///
/// The colour is a CSS custom property reference (not a literal) so the tier
/// follows the active Codex theme — see `static/style.css`.
pub fn score_tier(score: i32) -> (&'static str, &'static str) {
    if score >= 70 {
        ("var(--danger)", "!!")
    } else if score >= 30 {
        ("var(--warning)", "?")
    } else {
        ("var(--success)", "\u{2713}")
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
        "testwiki" => "https://test.wikipedia.org",
        _ => "https://fr.wikipedia.org",
    }
}

#[cfg(test)]
mod tests {
    use super::{score_tier, wiki_base_url};

    #[test]
    fn score_tier_maps_thresholds() {
        assert_eq!(score_tier(70).0, "var(--danger)");
        assert_eq!(score_tier(30).0, "var(--warning)");
        assert_eq!(score_tier(0).0, "var(--success)");
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
