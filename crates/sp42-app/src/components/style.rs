use sp42_core::{ScoreTier, score_tier};
use sp42_ui::ScoreTone;

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

#[must_use]
pub fn score_tone_for_score(score: i32) -> ScoreTone {
    match score_tier(score) {
        ScoreTier::Low => ScoreTone::Low,
        ScoreTier::Medium => ScoreTone::Medium,
        ScoreTier::High => ScoreTone::High,
    }
}

#[cfg(test)]
mod tests {
    use super::{score_tone_for_score, wiki_base_url};
    use sp42_ui::ScoreTone;

    #[test]
    fn wiki_base_url_resolves_known_wikis() {
        assert_eq!(wiki_base_url("frwiki"), "https://fr.wikipedia.org");
        assert_eq!(wiki_base_url("enwiki"), "https://en.wikipedia.org");
    }

    #[test]
    fn wiki_base_url_falls_back_for_unknown() {
        assert_eq!(wiki_base_url("xxwiki"), "https://fr.wikipedia.org");
    }

    #[test]
    fn score_tone_uses_domain_score_tier() {
        assert_eq!(score_tone_for_score(12), ScoreTone::Low);
        assert_eq!(score_tone_for_score(42), ScoreTone::Medium);
        assert_eq!(score_tone_for_score(90), ScoreTone::High);
    }
}
