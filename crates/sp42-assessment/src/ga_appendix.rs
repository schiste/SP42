//! Pure builder: `PageVerificationReport` → plain-wikitext GA evidence appendix.
//! (PRD-0016). No I/O, no inference; deterministic given the report
//! plus the shell-injected render timestamp.

/// Escape one verbatim field for safe embedding in wikitext (PRD-0016 hard
/// safety rule): entity-encode `&`, `<`, `>` inside the content — which makes
/// an embedded `</nowiki>` terminator inert — then wrap in `<nowiki>` so
/// braces, brackets, and pipes stay display-only.
#[allow(dead_code)]
fn escape_verbatim(text: &str) -> String {
    let inner = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<nowiki>{inner}</nowiki>")
}

/// Reader-facing ref label derived from the stable cite id (PRD-0016: the
/// report carries no rendered marker; never print the raw `cite_ref-…` id).
/// Named `MediaWiki` refs produce `cite_ref-<name>_<seq>-<use>`; unnamed refs
/// produce `cite_ref-<n>`. The `ordinal` is the finding's `use_site_ordinal`.
#[allow(dead_code)]
fn ref_label(ref_id: &str, ordinal: u32) -> String {
    let fallback = format!("ref #{}", ordinal + 1);
    let Some(rest) = ref_id.strip_prefix("cite_ref-") else {
        return fallback;
    };
    if rest.chars().all(|c| c.is_ascii_digit()) {
        return fallback;
    }
    // Strip the trailing `-<use>` then the trailing `_<seq>`; what remains is
    // the ref name. Any parse miss falls back to the ordinal.
    let Some((rest, use_idx)) = rest.rsplit_once('-') else {
        return fallback;
    };
    if !use_idx.chars().all(|c| c.is_ascii_digit()) {
        return fallback;
    }
    let Some((name, seq)) = rest.rsplit_once('_') else {
        return fallback;
    };
    if name.is_empty() || !seq.chars().all(|c| c.is_ascii_digit()) {
        return fallback;
    }
    format!("ref \"{}\"", name.replace('_', " "))
}

/// `YYYY-MM-DD` (UTC) from epoch milliseconds. Civil-from-days per Howard
/// Hinnant's algorithm — the workspace carries no date crate, and the footer
/// needs only a date (cf. the private helpers in `sp42-live`).
#[allow(dead_code)]
fn format_utc_date(epoch_ms: i64) -> String {
    let days = epoch_ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod helper_tests {
    use super::{escape_verbatim, format_utc_date, ref_label};

    #[test]
    fn escape_neutralizes_templates_refs_and_nowiki_terminators() {
        let hostile = r"See {{Infobox}} and <ref>x</ref> then </nowiki>{{evil}} after";
        let escaped = escape_verbatim(hostile);
        assert!(escaped.starts_with("<nowiki>") && escaped.ends_with("</nowiki>"));
        let inner = &escaped["<nowiki>".len()..escaped.len() - "</nowiki>".len()];
        // The terminator case: no literal `</nowiki>` may survive inside the wrapper.
        assert!(!inner.contains("</nowiki>"));
        // Angle brackets are entity-encoded so no tag (ref, nowiki) is live.
        assert!(!inner.contains('<') && !inner.contains('>'));
        // Content is preserved (entity-decoded form still names the template).
        assert!(inner.contains("{{Infobox}}"));
    }

    #[test]
    fn escape_round_trips_preexisting_entities_faithfully() {
        // `&lt;` in the source text must not collapse into a live `<`.
        assert_eq!(
            escape_verbatim("a &lt; b"),
            "<nowiki>a &amp;lt; b</nowiki>"
        );
    }

    #[test]
    fn ref_label_derives_names_and_falls_back_to_ordinal() {
        // Named ref: cite_ref-<name>_<seq>-<use>
        assert_eq!(ref_label("cite_ref-Lux_history_1-0", 4), "ref \"Lux history\"");
        // Unnamed ref: cite_ref-<n> — n is internal, use the per-report ordinal.
        assert_eq!(ref_label("cite_ref-6", 4), "ref #5");
        // Unparseable / empty id: ordinal fallback, never the raw id.
        assert_eq!(ref_label("", 0), "ref #1");
    }

    #[test]
    fn utc_date_formats_from_epoch_ms() {
        assert_eq!(format_utc_date(1_783_886_599_386), "2026-07-12");
        assert_eq!(format_utc_date(0), "1970-01-01");
    }
}
