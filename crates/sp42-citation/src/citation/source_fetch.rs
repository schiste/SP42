//! Source-text recovery and a first-cut HTML→text extractor (ported from wikiharness
//! `source-fetch.ts` / `html.ts`).
//!
//! The grounded bytes are the extracted *text* of the source body. [`html_to_text`] is a
//! dependency-light first-cut extractor (strip comments/declarations, drop
//! script/style/noscript, separate block elements, strip remaining tags, decode entities,
//! collapse whitespace). It is deliberately conservative: an entity or boundary it misses
//! fails **closed** — a quote that fails to locate is suppressed (ADR-0007 §5), never a
//! false `Supported`. A production-grade readability/main-content extractor (the
//! wikiharness `HtmlExtractor`/Defuddle analog, ADR-0011) is a noted follow-up — see
//! `docs/implementation-notes/ADR-CHANGE-NOTES.md`.

use std::sync::LazyLock;

use regex::Regex;

/// Chars up to and including the preamble before it is trusted as a real Wayback banner.
const MIN_PREFIX: usize = 200;
/// Chars of remaining content required for the recovered slice to be worth taking.
const MIN_REMAINDER: usize = 500;

static WAYBACK_PREAMBLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"The Wayback Machine - https://web\.archive\.org/[^\s]+").expect("valid regex")
});
static HTML_DOC_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*<(?:!doctype|html|head|body|\?xml)").expect("valid regex")
});
static HTML_CLOSE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)</(?:p|div|span|a|body|html|table|tr|td|li|ul|ol|h[1-6]|article|section)>")
        .expect("valid regex")
});

static COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--[\s\S]*?-->").expect("valid regex"));
static UNTERMINATED_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--[\s\S]*$").expect("valid regex"));
static DECLARATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<![^>]*>").expect("valid regex"));
static SCRIPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<script\b.*?</script>").expect("valid regex"));
static STYLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<style\b.*?</style>").expect("valid regex"));
static NOSCRIPT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<noscript\b.*?</noscript>").expect("valid regex"));
static BLOCK_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)</?(?:p|div|br|hr|li|ul|ol|tr|td|th|h[1-6]|section|article|header|footer|table|blockquote|pre|figure|figcaption|aside|nav|main|dd|dt|dl)\b[^>]*>",
    )
    .expect("valid regex")
});
static ANY_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid regex"));
static ENTITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"&(#[xX][0-9a-fA-F]+|#[0-9]+|[a-zA-Z][a-zA-Z0-9]+);").expect("valid regex")
});
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));

/// Salvage the inner article from a source whose Wayback banner survived extraction.
///
/// Returns the trimmed remainder after the banner when the banner is far enough in
/// (>= 200 chars) and enough content follows (>= 500 chars); otherwise the input
/// unchanged.
#[must_use]
pub fn recover_wayback_body(text: &str) -> String {
    let Some(matched) = WAYBACK_PREAMBLE.find(text) else {
        return text.to_string();
    };
    let cut = matched.end();
    if text[..cut].chars().count() < MIN_PREFIX {
        return text.to_string();
    }
    let remainder = text[cut..].trim();
    if remainder.chars().count() < MIN_REMAINDER {
        return text.to_string();
    }
    remainder.to_string()
}

/// Decide whether a body should be HTML-extracted, from its content type and content.
///
/// An `html`/`xml` content type is trusted; any other declared type is treated as
/// non-HTML; with no content type, an actual markup signature is required.
#[must_use]
pub fn looks_like_html(content_type: &str, body: &str) -> bool {
    let content_type = content_type.to_lowercase();
    if content_type.contains("html") || content_type.contains("xml") {
        return true;
    }
    if !content_type.is_empty() {
        return false;
    }
    HTML_DOC_PREFIX.is_match(body) || HTML_CLOSE_TAG.is_match(body)
}

/// Extract readable text from HTML (first-cut; see the module docs for limitations).
#[must_use]
pub fn html_to_text(html: &str) -> String {
    let mut text = COMMENT.replace_all(html, " ").into_owned();
    text = UNTERMINATED_COMMENT.replace_all(&text, " ").into_owned();
    text = DECLARATION.replace_all(&text, " ").into_owned();
    text = SCRIPT.replace_all(&text, " ").into_owned();
    text = STYLE.replace_all(&text, " ").into_owned();
    text = NOSCRIPT.replace_all(&text, " ").into_owned();
    text = BLOCK_TAG.replace_all(&text, " ").into_owned();
    text = ANY_TAG.replace_all(&text, "").into_owned();
    let decoded = decode_entities(&text);
    WHITESPACE.replace_all(&decoded, " ").trim().to_string()
}

/// Decode HTML character references — numeric (decimal and hex, full coverage) plus a
/// curated set of common named entities. An unknown named entity is left intact.
fn decode_entities(text: &str) -> String {
    ENTITY
        .replace_all(text, |caps: &regex::Captures| decode_one(&caps[1]))
        .into_owned()
}

fn decode_one(inner: &str) -> String {
    if let Some(hex) = inner
        .strip_prefix("#x")
        .or_else(|| inner.strip_prefix("#X"))
    {
        return u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .map_or_else(|| format!("&{inner};"), String::from);
    }
    if let Some(dec) = inner.strip_prefix('#') {
        return dec
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map_or_else(|| format!("&{inner};"), String::from);
    }
    named_entity(inner).map_or_else(|| format!("&{inner};"), String::from)
}

/// A curated common-named-entity table; `None` for anything outside it (kept intact).
fn named_entity(name: &str) -> Option<char> {
    let ch = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "middot" => '·',
        "deg" => '°',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "euro" => '€',
        "pound" => '£',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "times" => '×',
        "divide" => '÷',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "agrave" => 'à',
        "aacute" => 'á',
        "acirc" => 'â',
        "atilde" => 'ã',
        "auml" => 'ä',
        "aring" => 'å',
        "ccedil" => 'ç',
        "egrave" => 'è',
        "eacute" => 'é',
        "ecirc" => 'ê',
        "euml" => 'ë',
        "igrave" => 'ì',
        "iacute" => 'í',
        "icirc" => 'î',
        "iuml" => 'ï',
        "ntilde" => 'ñ',
        "ograve" => 'ò',
        "oacute" => 'ó',
        "ocirc" => 'ô',
        "otilde" => 'õ',
        "ouml" => 'ö',
        "oslash" => 'ø',
        "ugrave" => 'ù',
        "uacute" => 'ú',
        "ucirc" => 'û',
        "uuml" => 'ü',
        "yacute" => 'ý',
        "szlig" => 'ß',
        "Agrave" => 'À',
        "Aacute" => 'Á',
        "Eacute" => 'É',
        "Egrave" => 'È',
        "Ccedil" => 'Ç',
        "Ntilde" => 'Ñ',
        "Ouml" => 'Ö',
        "Uuml" => 'Ü',
        _ => return None,
    };
    Some(ch)
}

#[cfg(test)]
mod tests {
    use super::{html_to_text, looks_like_html, recover_wayback_body};

    fn long(text: &str, repeats: usize) -> String {
        text.repeat(repeats)
    }

    #[test]
    fn recovers_inner_body_after_a_real_banner() {
        let prefix = long("preamble navigation chrome ", 10); // > 200 chars before banner
        let banner = "The Wayback Machine - https://web.archive.org/web/2020/x";
        let body = long("Real article prose. ", 40); // > 500 chars after
        let input = format!("{prefix}{banner} {body}");
        let recovered = recover_wayback_body(&input);
        assert!(recovered.starts_with("Real article prose."));
        assert!(!recovered.contains("Wayback Machine"));
    }

    #[test]
    fn no_banner_is_unchanged() {
        let text = "Just a normal article with no archive banner at all.";
        assert_eq!(recover_wayback_body(text), text);
    }

    #[test]
    fn short_remainder_is_unchanged() {
        let prefix = long("preamble navigation chrome ", 10);
        let banner = "The Wayback Machine - https://web.archive.org/web/2020/x";
        let input = format!("{prefix}{banner} short tail");
        assert_eq!(recover_wayback_body(&input), input);
    }

    #[test]
    fn banner_at_index_zero_is_unchanged() {
        let banner = "The Wayback Machine - https://web.archive.org/web/2020/x ";
        let body = long("Real article prose. ", 40);
        let input = format!("{banner}{body}");
        assert_eq!(recover_wayback_body(&input), input);
    }

    #[test]
    fn looks_like_html_by_content_type() {
        assert!(looks_like_html("text/html; charset=utf-8", ""));
        assert!(looks_like_html("application/xml", ""));
        assert!(!looks_like_html("text/plain", "if a < b and c > d then ok"));
    }

    #[test]
    fn looks_like_html_without_content_type_needs_a_signature() {
        assert!(looks_like_html("", "<!doctype html><p>Hello</p>"));
        assert!(looks_like_html("", "<p>x</p>"));
        assert!(!looks_like_html("", "use the <ref> tag to cite a source"));
    }

    #[test]
    fn extracts_basic_prose_and_keeps_inline_words_whole() {
        assert_eq!(
            html_to_text("<!doctype html><p>Hello <b>world</b></p>"),
            "Hello world"
        );
        // Inline tags must not split a word.
        assert_eq!(html_to_text("wo<b>r</b>d"), "word");
    }

    #[test]
    fn drops_script_and_style_content() {
        let html = "<style>.x{color:red}</style><p>Visible</p><script>alert(1<2)</script>";
        assert_eq!(html_to_text(html), "Visible");
    }

    #[test]
    fn strips_comments_and_collapses_whitespace() {
        let html = "<p>A</p>\n\n  <!-- hidden -->  <p>B</p>";
        assert_eq!(html_to_text(html), "A B");
    }

    #[test]
    fn decodes_named_numeric_and_hex_entities() {
        assert_eq!(html_to_text("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(html_to_text("caf&eacute;"), "café");
        assert_eq!(html_to_text("caf&#233;"), "café");
        assert_eq!(html_to_text("caf&#xE9;"), "café");
        // An unknown named entity is left intact (fail-closed).
        assert_eq!(html_to_text("a&notareal;b"), "a&notareal;b");
    }

    #[test]
    fn separates_block_elements_with_space() {
        assert_eq!(html_to_text("<li>one</li><li>two</li>"), "one two");
    }
}
