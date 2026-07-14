# GA Evidence Appendix Renderer Implementation Plan — Phase 1: crate + builder core

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Create `crates/sp42-assessment` with a pure `PageVerificationReport → wikitext GA appendix` builder whose wording invariants are pinned by tests (PRD-0016).

**Architecture:** New flat domain crate depending on `sp42-citation` for report contracts. One pure builder function (`ga_appendix.rs`), all GA-facing English in `copy.rs`. No I/O, no inference, no `ReportDocument` (the builder emits wikitext directly per the design sketch).

**Tech Stack:** Rust 1.96 (edition 2024, workspace-inherited lints incl. `clippy::pedantic = deny`), plain `std` only — no new dependencies. Tests are in-module `#[cfg(test)]` with fixture-helper functions (house style, cf. `crates/sp42-citation/src/citation_page_report.rs:212`).

**Scope:** Phase 1 of 3 from `docs/design-plans/2026-07-10-ga-appendix-renderer.md` (sketch phase 4 is staged behind PRD-0015 and excluded).

**Codebase verified:** 2026-07-12 (two codebase-investigator passes; key facts inline below)

**Verified contract facts this plan relies on:**
- `PageVerificationReport { wiki_id, rev_id: u64, title, findings: Vec<CitationFinding>, skipped: Vec<SkippedRef>, extraction_failures: Vec<BlockFailure>, stats: PageVerificationStats }` — `crates/sp42-citation/src/citation/page.rs:168`. Stats has **no** `supported_unlocated` and **no** `verified_at` (the sketch's upstream notes are not landed) — derive the grounded/unconfirmed split from `findings`; footer date is the shell-injected render date.
- `CitationFinding` — `crates/sp42-citation/src/citation/verify.rs:170`, **complete required-field list** (several have no `Default`): `kind: CitationFindingKind` (single variant, `CitationFindingKind::default()` works), `verdict: CitationVerdict` (`Judged(SupportLevel) | SourceUnavailable`, serialized as flat `Verdict`), `grounding_status: GroundingStatus` (`Located | LocatedFuzzy | Unlocated | NotApplicable`), `source_unavailable_reason: Option<SourceUnavailableReason>` (`Unreachable | Unusable`), `unusable_reason: Option<BodyUsabilityReason>`, `agreement: PanelAgreement { panel_size: u8, winner_votes: u8 }`, `passage: Option<LocatedPassage { quote, offset }>`, `provenance: SourceProvenance { url: url::Url, content_hash, fetched_at, http_status }`, `source_excerpt: Option<String>`, `metadata: Option<CitoidMetadata>`, `grounding: GroundingAssertion` (enum, no neutral value: `LocatedQuote { quote, source_hash, offset } | SourceFetched { source_hash }` — fixtures use `GroundingAssertion::SourceFetched { source_hash: String::new() }` unless the case under test needs a located quote), `use_site_ordinal: u32`, `ref_id: String`, `claim: String`, `preceding_context: Vec<String>` (fixtures: `Vec::new()`), `archive_of: Option<url::Url>`, `is_bare_url_ref: bool`, `schema_version: u32` (fixtures: `1`). The renderer reads `grounding_status`/`passage`, never `grounding` — the assertion arm is fixture plumbing only.
- `SupportLevel` = `Supported | Partial | NotSupported` (`verdict.rs:23`). `SkippedRef { ref_id, reason: SkippedReason (single variant NonUrlSource), block_ordinal }`; `BlockFailure { block_ordinal, reason: String }` — reason strings embed raw `cite_ref-…` ids (must be rewritten).
- `BodyUsabilityReason` variants (`crates/sp42-citation/src/citation/body_classifier.rs:34`): `Ok, JsonLdLeak, CssLeak, AntiBotChallenge, WaybackRedirectNotice, WaybackChrome, AmazonStub, ShortBody, PdfBody, ViewerShell, NavChromePaywall`.
- No `chrono`/`time` in the workspace; `sp42-live` has private `days_from_civil` helpers — write our own tiny UTC date formatter (Task 3).
- CI gate: `cargo xtask ci-all`; layering: add crate to the `LAYER` map in `scripts/check-layering.sh` (line ~44-77) as `domain`; commits are conventional (`feat(assessment): …`), enforced by `.husky/commit-msg`.

**Design note carried from the 2026-07-12 GA smoketest session:** `PanelAgreement` is a disclosure the report carries; PRD-0016 requires every carried disclosure to render. Disagreement lines therefore carry a low-confidence annotation when the winning verdict lacks a panel majority (`winner_votes * 2 <= panel_size`). This is the one rendering element not literally enumerated in the PRD's sublist spec — flag it in the PR description for the Editor to strike if unwanted.

---

## Task 1: Crate scaffold + workspace registration (infrastructure)

**Files:**
- Create: `crates/sp42-assessment/Cargo.toml`
- Create: `crates/sp42-assessment/src/lib.rs`
- Modify: `Cargo.toml` (root; `members` list, lines 2-23 — insert alphabetically after `crates/sp42-app`)
- Modify: `scripts/check-layering.sh` (`LAYER` map, lines ~44-77 — insert alphabetically)
- Modify: `.github/CODEOWNERS` (crate lines block)

**Step 1: Create the crate files**

`crates/sp42-assessment/Cargo.toml`:
```toml
[package]
name = "sp42-assessment"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
version.workspace = true

[dependencies]
sp42-citation = { path = "../sp42-citation" }

[dev-dependencies]
serde_json.workspace = true
url.workspace = true

[lints]
workspace = true
```

`crates/sp42-assessment/src/lib.rs`:
```rust
//! Assessment-domain policy: GA-review-shaped rendering over the references
//! domain's verification reports (PRD-0016).
//!
//! The one export is a pure builder from `PageVerificationReport` to a plain
//! wikitext evidence appendix a Good-article reviewer pastes onto
//! `Talk:Article/GAn`. No I/O, no inference, no wiki writes.

pub mod copy;
pub mod ga_appendix;

pub use ga_appendix::render_ga_appendix;
```

(Compile stub: until Tasks 2-5 land, `copy` and `ga_appendix` are created in the same commit as empty `//! module` files with the items added by their tasks. To keep every commit green, create `src/copy.rs` and `src/ga_appendix.rs` now containing only their module doc comment and a `pub use`-free body, and make `lib.rs` declare only `pub mod copy;` + `pub mod ga_appendix;` — add the `pub use` re-export in Task 5 when the function exists.)

**Step 2: Register the crate**

- Root `Cargo.toml`: add `"crates/sp42-assessment",` to `members` (alphabetical, after `crates/sp42-app`).
- `scripts/check-layering.sh`: add `"sp42-assessment": "domain",` to the `LAYER` map (it is a Python dict inside the script — match the neighboring `"sp42-citation": "domain",` line exactly, alphabetical order).
- `.github/CODEOWNERS`: add `/crates/sp42-assessment/            @schiste` alongside the existing crate lines.

**Step 3: Verify operationally**

```sh
cargo check -p sp42-assessment
./scripts/check-layering.sh
```
Expected: both succeed; layering reports sp42-assessment as domain with only platform/domain deps.

**Step 4: Commit**

```sh
git add crates/sp42-assessment Cargo.toml scripts/check-layering.sh .github/CODEOWNERS
git commit -m "feat(assessment): scaffold sp42-assessment domain crate (PRD-0016)"
```

---

## Task 2: `copy.rs` — the reader-facing copy module

**Files:**
- Modify: `crates/sp42-assessment/src/copy.rs`

All GA-facing English lives here (PRD-0016 "criterion copy lives in one module"). No logic beyond `match`es from contract enums to strings. Public within the crate; `ga_appendix.rs` is the only consumer.

**Step 1: Write the failing test** (in-module `#[cfg(test)]` at the bottom of `copy.rs`)

```rust
#[cfg(test)]
mod tests {
    use sp42_citation::{BodyUsabilityReason, SupportLevel};

    #[test]
    fn verdict_copy_never_leaks_contract_identifiers() {
        for level in [SupportLevel::Partial, SupportLevel::NotSupported] {
            let text = super::disagreement_verdict(level);
            assert!(!text.contains("NotSupported") && !text.contains("Partial"));
            assert!(!text.to_lowercase().contains("fail"), "mismatch framing, not failure: {text}");
        }
    }

    #[test]
    fn unusable_reason_copy_covers_every_variant() {
        for reason in [
            BodyUsabilityReason::JsonLdLeak,
            BodyUsabilityReason::CssLeak,
            BodyUsabilityReason::AntiBotChallenge,
            BodyUsabilityReason::WaybackRedirectNotice,
            BodyUsabilityReason::WaybackChrome,
            BodyUsabilityReason::AmazonStub,
            BodyUsabilityReason::ShortBody,
            BodyUsabilityReason::PdfBody,
            BodyUsabilityReason::ViewerShell,
            BodyUsabilityReason::NavChromePaywall,
        ] {
            let text = super::unusable_reason(reason);
            assert!(!text.is_empty());
            assert!(!text.contains('_'), "no snake_case tokens: {text}");
        }
    }
}
```

Check first that `SupportLevel`, `BodyUsabilityReason`, `GroundingStatus` are exported from `sp42_citation`'s crate root (`crates/sp42-citation/src/lib.rs`); if any is only at a module path, use that path — do not add new re-exports upstream in this task.

**Step 2: Run to verify it fails**

```sh
cargo test -p sp42-assessment
```
Expected: compile error (functions missing).

**Step 3: Implement**

```rust
//! Every GA-facing English string in the appendix, in one place (PRD-0016:
//! reader-facing vocabulary; enables later localization / {{GAList}} idiom
//! swap as a copy change, not an architecture change).

use sp42_citation::{BodyUsabilityReason, GroundingStatus, SupportLevel};

/// Appendix and section headings (plain wikitext, no transclusions).
pub const APPENDIX_HEADING: &str = "== SP42 evidence appendix ==";
pub const CRITERION_2_HEADING: &str =
    "=== Criterion 2 (verifiable) — [[Wikipedia:Good article criteria|GA criteria]] ===";
pub const BUCKET_DISAGREEMENTS: &str = "==== Claim–source disagreements ====";
pub const BUCKET_RECOVERED: &str = "==== Supported via archive copy (citation update suggested) ====";
pub const BUCKET_DEAD_LINKS: &str = "==== Dead links (no archive copy found) ====";
pub const BUCKET_UNREADABLE: &str = "==== Sources the tool could not read (tool limitation — the citations may be fine) ====";
pub const BUCKET_UNCONFIRMED: &str = "==== Unconfirmed supports (judged supported, quote not re-located) ====";
pub const BUCKET_SUPPORTED: &str = "==== Supported spot-checks ====";
pub const BUCKET_SKIPPED: &str = "==== Not machine-verified (book and offline sources) ====";
pub const BUCKET_EXTRACTION_FAILURES: &str = "==== Refs the tool could not process ====";

/// The positive "assessed by SP42" honesty line (2b only in the MVP).
pub const ASSESSED_LINE: &str = "''This appendix carries evidence for criterion 2b \
(inline citations support the text) only; all other criteria and sub-criteria were \
not assessed by the tool.''";

/// Provenance-footer framing line.
pub const FRAMING_LINE: &str = "This is a tool-generated evidence appendix; the \
criteria judgments and the review outcome are the reviewer's.";

/// "What is this?" explainer target (repo-hosted for the MVP; Phase 3 creates it).
pub const EXPLAINER_URL: &str =
    "https://github.com/schiste/SP42/blob/main/docs/domains/assessment/what-is-this-appendix.md";

/// Reader-facing verdict for a disagreement line (PRD-0014 mismatch framing).
#[must_use]
pub fn disagreement_verdict(level: SupportLevel) -> &'static str {
    match level {
        SupportLevel::NotSupported => "the source and this claim disagree — the panel found no support for the claim in the source",
        SupportLevel::Partial => "the source only partially supports this claim",
        SupportLevel::Supported => "the source supports this claim",
    }
}

/// Grounding annotation for non-exact grounding (renders wherever it lands).
#[must_use]
pub fn grounding_annotation(status: GroundingStatus) -> Option<&'static str> {
    match status {
        GroundingStatus::Unlocated | GroundingStatus::NotApplicable => {
            Some("the supporting quote could not be re-located in the source")
        }
        GroundingStatus::LocatedFuzzy => {
            Some("the supporting quote matched the source only approximately")
        }
        GroundingStatus::Located => None,
    }
}

/// No-quote verdict wording (ADR-0007: never fabricate a passage).
pub const NO_PASSAGE_LINE: &str = "no supporting passage was found in the source";

/// Label for `source_excerpt` context — never presented as evidence.
pub const EXCERPT_CONTEXT_LABEL: &str = "context the tool read (not evidence)";

/// Low-confidence annotation when the panel's winning verdict lacks a majority.
pub const PANEL_SPLIT_LINE: &str =
    "the review panel split on this reading — treat as low-confidence";

/// Repair-handle annotation for lines carrying `archive_of`.
pub const ARCHIVE_HANDLE_PREFIX: &str = "verified against an archive copy of";

/// Reader-facing reason a fetched source could not be used.
#[must_use]
pub fn unusable_reason(reason: BodyUsabilityReason) -> &'static str {
    match reason {
        BodyUsabilityReason::PdfBody => "a PDF the tool cannot read",
        BodyUsabilityReason::ViewerShell => "an interactive viewer page with no readable text",
        BodyUsabilityReason::NavChromePaywall => "a paywall or registration page",
        BodyUsabilityReason::ShortBody => "the page returned too little readable text",
        BodyUsabilityReason::AntiBotChallenge => "an anti-bot challenge page",
        BodyUsabilityReason::WaybackRedirectNotice | BodyUsabilityReason::WaybackChrome => {
            "an archive page without readable article content"
        }
        BodyUsabilityReason::JsonLdLeak | BodyUsabilityReason::CssLeak => {
            "page code instead of article text"
        }
        BodyUsabilityReason::AmazonStub => "a storefront stub page",
        BodyUsabilityReason::Ok => "a page the panel could not use",
    }
}

/// Generic unreadable wording when the report carries no specific reason.
pub const UNUSABLE_GENERIC: &str = "a page the tool fetched but could not use";

/// Skip reason (single contract variant today: non-URL source).
pub const SKIPPED_NON_URL: &str =
    "cites a book or offline source the tool does not verify";
```

Adjust wording freely at review time — the *shape* (every string here, none inline in the builder) is the requirement. If `rustfmt` or `clippy::pedantic` complains (long literals are fine; `#[must_use]` on fns returning `&'static str` is required by pedantic), fix per the lint text.

**Step 4: Run tests**

```sh
cargo test -p sp42-assessment && cargo clippy -p sp42-assessment --all-targets -- -D warnings
```
Expected: PASS, no clippy warnings.

**Step 5: Commit**

```sh
git add crates/sp42-assessment/src/copy.rs crates/sp42-assessment/src/lib.rs
git commit -m "feat(assessment): reader-facing copy module for the GA appendix"
```

---

## Task 3: Builder helpers — `escape_verbatim`, `ref_label`, `format_utc_date`

**Files:**
- Modify: `crates/sp42-assessment/src/ga_appendix.rs`

**Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod helper_tests {
    use super::{escape_verbatim, format_utc_date, ref_label};

    #[test]
    fn escape_neutralizes_templates_refs_and_nowiki_terminators() {
        let hostile = r#"See {{Infobox}} and <ref>x</ref> then </nowiki>{{evil}} after"#;
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
```

**Step 2: Run to verify failure** — `cargo test -p sp42-assessment` → compile error.

**Step 3: Implement**

```rust
//! Pure builder: `PageVerificationReport` → plain-wikitext GA evidence
//! appendix (PRD-0016). No I/O, no inference; deterministic given the report
//! plus the shell-injected render timestamp.

use crate::copy;

/// Escape one verbatim field for safe embedding in wikitext (PRD-0016 hard
/// safety rule): entity-encode `&`, `<`, `>` inside the content — which makes
/// an embedded `</nowiki>` terminator inert — then wrap in `<nowiki>` so
/// braces, brackets, and pipes stay display-only.
fn escape_verbatim(text: &str) -> String {
    let inner = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<nowiki>{inner}</nowiki>")
}

/// Reader-facing ref label derived from the stable cite id (PRD-0016: the
/// report carries no rendered marker; never print the raw `cite_ref-…` id).
/// Named MediaWiki refs produce `cite_ref-<name>_<seq>-<use>`; unnamed refs
/// produce `cite_ref-<n>`. `ordinal` is the finding's `use_site_ordinal`.
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
```

(`clippy::pedantic` will demand `#[must_use]` is not required on private fns; it may flag arithmetic style — resolve per lint text without changing behavior, the test pins correctness.)

**Step 4: Run tests** — `cargo test -p sp42-assessment` → PASS.

**Step 5: Commit**

```sh
git add crates/sp42-assessment/src/ga_appendix.rs
git commit -m "feat(assessment): appendix escaping, ref-label, and date helpers"
```

---

## Task 4: Bucketing — verdict partitions, grounding annotates

**Files:**
- Modify: `crates/sp42-assessment/src/ga_appendix.rs`

**Step 1: Write the failing test** — the PRD's verdict×grounding matrix, every finding in exactly one bucket:

```rust
#[cfg(test)]
mod bucket_tests {
    use super::{Bucket, bucket_for};
    use sp42_citation::{
        CitationFinding, CitationVerdict, GroundingStatus, SourceUnavailableReason, SupportLevel,
    };

    // Fixture helper: house style is programmatic construction with defaults
    // (cf. citation_page_report.rs:212). Reuse via a shared `fn finding()` in
    // this module; fields not under test take neutral values.
    fn finding(
        verdict: CitationVerdict,
        grounding: GroundingStatus,
        archived: bool,
        unavailable: Option<SourceUnavailableReason>,
    ) -> CitationFinding {
        // Build the full struct literally here (all fields; the complete
        // required-field list is in this plan's header, source of truth
        // crates/sp42-citation/src/citation/verify.rs:170). Neutral values:
        // kind: CitationFindingKind::default(),
        // grounding: GroundingAssertion::SourceFetched { source_hash: String::new() },
        // provenance url https://example.org/a, empty strings, None options,
        // preceding_context: Vec::new(), agreement: PanelAgreement::new(3, 3),
        // schema_version: 1; set archive_of to
        // Some("https://web.archive.org/x".parse().unwrap()) when `archived`.
        todo!("write out the full literal in implementation")
    }

    #[test]
    fn verdict_partitions_and_grounding_annotates() {
        use CitationVerdict as V;
        use GroundingStatus as G;
        use SupportLevel as L;
        let cases = [
            // (verdict, grounding, archived, unavailable_reason) -> bucket
            (V::Judged(L::NotSupported), G::NotApplicable, false, None, Bucket::Disagreement),
            (V::Judged(L::Partial), G::Located, false, None, Bucket::Disagreement),
            // Non-exact grounding on Partial stays a disagreement (annotated).
            (V::Judged(L::Partial), G::Unlocated, false, None, Bucket::Disagreement),
            // Archive-backed disagreement stays a disagreement (with handle).
            (V::Judged(L::NotSupported), G::NotApplicable, true, None, Bucket::Disagreement),
            // Supported + exact + archive -> recovered.
            (V::Judged(L::Supported), G::Located, true, None, Bucket::Recovered),
            // Supported + exact, no archive -> spot-check record.
            (V::Judged(L::Supported), G::Located, false, None, Bucket::Supported),
            // Grounding caveat wins the bucket, archive annotates.
            (V::Judged(L::Supported), G::LocatedFuzzy, true, None, Bucket::Unconfirmed),
            (V::Judged(L::Supported), G::Unlocated, false, None, Bucket::Unconfirmed),
            (V::Judged(L::Supported), G::NotApplicable, false, None, Bucket::Unconfirmed),
            (V::SourceUnavailable, G::NotApplicable, false, Some(SourceUnavailableReason::Unreachable), Bucket::DeadLink),
            (V::SourceUnavailable, G::NotApplicable, false, Some(SourceUnavailableReason::Unusable), Bucket::Unreadable),
            // Legacy record with no reason: dead link (the conservative read).
            (V::SourceUnavailable, G::NotApplicable, false, None, Bucket::DeadLink),
        ];
        for (verdict, grounding, archived, unavailable, expected) in cases {
            let f = finding(verdict, grounding, archived, unavailable);
            assert_eq!(bucket_for(&f), expected, "{verdict:?}/{grounding:?}/archived={archived}");
        }
    }
}
```

**Step 2: Run to verify failure.**

**Step 3: Implement**

```rust
/// The consequence-ordered criterion-2 sublists (PRD-0016). Verdict
/// partitions; grounding and `archive_of` annotate. Every finding lands in
/// exactly one bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Disagreement,
    Recovered,
    DeadLink,
    Unreadable,
    Unconfirmed,
    Supported,
}

fn bucket_for(finding: &CitationFinding) -> Bucket {
    match finding.verdict {
        CitationVerdict::Judged(SupportLevel::NotSupported | SupportLevel::Partial) => {
            Bucket::Disagreement
        }
        CitationVerdict::Judged(SupportLevel::Supported) => {
            if finding.grounding_status == GroundingStatus::Located {
                if finding.archive_of.is_some() {
                    Bucket::Recovered
                } else {
                    Bucket::Supported
                }
            } else {
                Bucket::Unconfirmed
            }
        }
        CitationVerdict::SourceUnavailable => match finding.source_unavailable_reason {
            Some(SourceUnavailableReason::Unusable) => Bucket::Unreadable,
            Some(SourceUnavailableReason::Unreachable) | None => Bucket::DeadLink,
        },
    }
}
```

Imports: `use sp42_citation::{CitationFinding, CitationVerdict, GroundingStatus, SourceUnavailableReason, SupportLevel};` — again, check the actual export paths from `sp42_citation`'s root and adjust (module paths, not new upstream re-exports).

**Step 4: Run tests** → PASS. **Step 5: Commit** — `feat(assessment): consequence-order bucketing for appendix findings`.

---

## Task 5: Line renderers, stats line, section assembly — `render_ga_appendix`

**Files:**
- Modify: `crates/sp42-assessment/src/ga_appendix.rs`
- Modify: `crates/sp42-assessment/src/lib.rs` (add `pub use ga_appendix::render_ga_appendix;`)

**Step 1: Write the failing tests.** Build one fixture report exercising every bucket (reuse/extend the Task 4 `finding()` helper; move it to a shared `#[cfg(test)] mod fixtures` in `ga_appendix.rs`), plus `skipped` (one `SkippedRef`) and `extraction_failures` (one `BlockFailure` whose `reason` embeds a raw `cite_ref-…` id, e.g. `"ref cite_ref-64 has no resolvable claim text"`). Assert:

```rust
#[test]
fn appendix_renders_the_full_criterion_2_structure() {
    let report = fixtures::full_report();
    let out = super::render_ga_appendix(&report, 1_783_886_599_386, "0.1.0");
    // Section + heading structure, consequence order.
    let idx = |needle: &str| out.find(needle).unwrap_or_else(|| panic!("missing: {needle}"));
    assert!(idx(copy::BUCKET_DISAGREEMENTS) < idx(copy::BUCKET_RECOVERED));
    assert!(idx(copy::BUCKET_RECOVERED) < idx(copy::BUCKET_DEAD_LINKS));
    assert!(idx(copy::BUCKET_DEAD_LINKS) < idx(copy::BUCKET_UNREADABLE));
    assert!(idx(copy::BUCKET_UNREADABLE) < idx(copy::BUCKET_UNCONFIRMED));
    assert!(idx(copy::BUCKET_UNCONFIRMED) < idx(copy::BUCKET_SUPPORTED));
    assert!(idx(copy::BUCKET_SUPPORTED) < idx(copy::BUCKET_SKIPPED));
    // Honesty arms.
    assert!(out.contains(copy::ASSESSED_LINE));
    assert!(out.contains(copy::FRAMING_LINE) && out.contains(copy::EXPLAINER_URL));
    assert!(out.contains("2026-07-12"), "footer render date");
    assert!(out.contains("rev 12345"), "footer rev_id");
    // Stats line states the grounded/unconfirmed split within supported.
    assert!(out.contains("of them unconfirmed"));
}

#[test]
fn no_raw_contract_identifiers_anywhere() {
    let out = super::render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
    for token in [
        "NotSupported", "SourceUnavailable", "Unlocated", "LocatedFuzzy",
        "NotApplicable", "cite_ref-", "ShortBody", "PdfBody", "snake_case",
        "not_supported", "source_unavailable",
    ] {
        assert!(!out.contains(token), "raw identifier leaked: {token}");
    }
}

#[test]
fn no_pass_fail_wording_in_the_assembled_appendix() {
    // DoD #3: the wording invariant binds the whole output, not just the
    // copy module. Word-level scan ("passage" is PRD-mandated wording and
    // must not trip it).
    let out = super::render_ga_appendix(&fixtures::full_report(), 0, "0.1.0")
        .to_lowercase();
    let banned = ["pass", "passed", "passes", "fail", "failed", "fails", "failure"];
    for word in out.split(|c: char| !c.is_ascii_alphabetic()) {
        assert!(!banned.contains(&word), "pass/fail wording leaked: {word}");
    }
}

#[test]
fn unusable_reasons_wire_to_their_own_findings() {
    // DoD #8: distinct reasons render distinctly — the assembly must thread
    // each finding's own reason, not just have correct copy. full_report
    // carries one PdfBody and one ViewerShell unreadable finding.
    let out = super::render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
    assert!(out.contains(copy::unusable_reason(BodyUsabilityReason::PdfBody)));
    assert!(out.contains(copy::unusable_reason(BodyUsabilityReason::ViewerShell)));
}

#[test]
fn rendering_is_deterministic() {
    let report = fixtures::full_report();
    let a = super::render_ga_appendix(&report, 1_700_000_000_000, "0.1.0");
    let b = super::render_ga_appendix(&report, 1_700_000_000_000, "0.1.0");
    assert_eq!(a, b);
}

#[test]
fn hostile_verbatim_fields_render_inert() {
    // Claim discussing {{Infobox}}, excerpt embedding </nowiki>, quote with a
    // ref tag (the PRD's malicious fixture, one hostile field per kind).
    let report = fixtures::hostile_report();
    let out = super::render_ga_appendix(&report, 0, "0.1.0");
    // Count opens and closes: every <nowiki> the renderer opens, it closes,
    // and no embedded terminator adds an extra close.
    assert_eq!(out.matches("<nowiki>").count(), out.matches("</nowiki>").count());
    // The embedded terminator survives only entity-encoded.
    assert!(out.contains("&lt;/nowiki&gt;"));
    // The ref tag never appears live.
    assert!(!out.contains("<ref>"));
}

#[test]
fn no_quote_disagreement_states_no_passage_and_excerpt_is_labeled_context() {
    let report = fixtures::full_report(); // its NotSupported finding: passage None, source_excerpt Some
    let out = super::render_ga_appendix(&report, 0, "0.1.0");
    assert!(out.contains(copy::NO_PASSAGE_LINE));
    assert!(out.contains(copy::EXCERPT_CONTEXT_LABEL));
}

#[test]
fn archive_handle_renders_in_every_bucket_that_carries_it() {
    let out = super::render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
    // full_report has archive_of on a disagreement, a recovered, and an
    // unconfirmed finding; the handle text must appear three times.
    assert_eq!(out.matches(copy::ARCHIVE_HANDLE_PREFIX).count(), 3);
}

#[test]
fn skips_and_extraction_failures_render_with_rewritten_ids() {
    let out = super::render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
    assert!(out.contains(copy::SKIPPED_NON_URL));
    // BlockFailure.reason "ref cite_ref-64 has no resolvable claim text"
    // renders with the raw id rewritten (covered by the identifier scan too;
    // assert the line survived the rewrite rather than being dropped).
    assert!(out.contains("has no resolvable claim text"));
}

#[test]
fn panel_split_annotation_renders_on_minority_verdicts_only() {
    // full_report: one disagreement with PanelAgreement::new(3, 1), one with (3, 2).
    let out = super::render_ga_appendix(&fixtures::full_report(), 0, "0.1.0");
    assert_eq!(out.matches(copy::PANEL_SPLIT_LINE).count(), 1);
}

#[test]
fn bundled_ref_supported_lines_are_distinguishable() {
    // Two Supported findings sharing ref_id but different provenance URLs.
    let out = super::render_ga_appendix(&fixtures::bundled_ref_report(), 0, "0.1.0");
    assert!(out.contains("https://example.org/one") && out.contains("https://example.org/two"));
}
```

**Step 2: Run to verify failure.**

**Step 3: Implement.** Public entry:

```rust
/// Render the GA evidence appendix (PRD-0016) — pure and deterministic.
///
/// `rendered_at_ms` is the shell-injected render time (the report contract
/// carries no verification timestamp today; the footer labels the date as the
/// render date). `sp42_version` is the shell's crate version.
#[must_use]
pub fn render_ga_appendix(
    report: &PageVerificationReport,
    rendered_at_ms: i64,
    sp42_version: &str,
) -> String
```

Note for the copy module given the pass/fail output scan: no copy string may use the standalone words pass/passed/fail/failed/failure (e.g. "refs the tool could not process", never "processing failed"); the Task 2 copy above already complies — keep it so. `BlockFailure.reason` strings come from the contract and could theoretically carry these words; the fixture's reason string ("has no resolvable claim text") does not, and if a future contract reason does, the copy module owns the rewrite (same seam as the cite-id rewrite). The same discipline binds fixture-authored text: keep the banned words (and `/pass/`-style URL segments) out of `full_report()`'s claims, excerpts, and URLs — the scan runs over the whole assembled output.

The Task-4/Task-5 shared fixture `full_report()` must include **two** unreadable findings with distinct reasons (`PdfBody`, `ViewerShell`) for the wiring test above.

Assembly rules (each as a small private fn; findings iterate in report order within each bucket — stable, keyed by `use_site_ordinal`):

- **Header**: `copy::APPENDIX_HEADING`, blank line, `copy::ASSESSED_LINE`.
- **Criterion-2 heading**, then the **stats line** in evidence phrasing, deriving the supported split: `let unconfirmed_supported = findings where Judged(Supported) && grounding_status != Located`. Example: `Of {refs_seen} references, {use_sites_verified} citation use-sites were machine-checked: {supported} supported ({n} of them unconfirmed), {partial} partially supported, {not_supported} where claim and source disagree, {dead} dead links, {unusable} sources the tool could not read; {skipped} book/offline refs and {extraction_failures} unprocessable refs were not checked.` — every number from `report.stats` except the derived split.
- **Buckets in order** (`Disagreement, Recovered, DeadLink, Unreadable, Unconfirmed, Supported`), each rendered only when non-empty, as `* ` list items:
  - *Disagreement*: `* {ref_label}: {copy::disagreement_verdict(level)}. Claim: {escape_verbatim(claim)}.` then ` The panel located: {escape_verbatim(quote)}.` when `passage` is `Some`, else ` {copy::NO_PASSAGE_LINE}` (capitalize in copy or in code — pick one, test pins it); when `source_excerpt` is `Some` and `passage` is `None`: ` {copy::EXCERPT_CONTEXT_LABEL}: {escape_verbatim(excerpt truncated to 200 chars on a char boundary)}.`; grounding annotation via `copy::grounding_annotation` when `Some` and level is `Partial`; ` ({copy::PANEL_SPLIT_LINE})` when `winner_votes * 2 <= panel_size`; source link ` — [{provenance.url}]`; archive handle ` ({copy::ARCHIVE_HANDLE_PREFIX} [{archive_of}])` when carried.
  - *Recovered*: `* {ref_label}: supported via an archive copy — update the citation to [{archive_of}] (live link: [{provenance.url}]). Claim: {escape_verbatim(claim prefix ≤120 chars)}.`
  - *DeadLink*: `* {ref_label}: the source could not be fetched (link may be dead): [{provenance.url}]` — dead URL only, no archive candidates (the contract preserves none).
  - *Unreadable*: `* {ref_label}: the tool fetched [{provenance.url}] but read {copy::unusable_reason(reason)}` (fall back to `copy::UNUSABLE_GENERIC` when `unusable_reason` is `None`) `— the citation may be fine.`
  - *Unconfirmed*: `* {ref_label}: judged supported, but {copy::grounding_annotation(status)}. Claim: {escape_verbatim(claim prefix)}. — [{provenance.url}]` plus archive handle when carried.
  - *Supported*: compact one-liner — `* {ref_label}: supported — {escape_verbatim(claim prefix ≤80 chars)} — [{provenance.url}] (quote located)`. No quotes (they stay in the CLI/structured rendering); the URL makes bundled-ref lines distinguishable.
- **Skipped** (when non-empty): `* {ref_label(ref_id, ordinal from position)}: {copy::SKIPPED_NON_URL}` — note `SkippedRef` has no `use_site_ordinal`; use `block_ordinal` for the fallback index.
- **Extraction failures** (when non-empty): render `BlockFailure.reason` through a `sanitize_reason` helper that rewrites every `cite_ref-<token>` substring (token = maximal run of non-whitespace after the prefix) to `ref_label(token-with-prefix, i)` — plain string scan, no regex dependency.
- **Footer**: `----` rule, then `''{article title} at revision {rev_id} · rendered {format_utc_date(rendered_at_ms)} (render date, not verification date) · SP42 {sp42_version} · {copy::FRAMING_LINE} [{copy::EXPLAINER_URL} What is this?]''`.
- Claim-prefix truncation helper: char-boundary safe (`.chars().take(n)`), append `…` when truncated — truncate **before** escaping.

Book citations (PRD sublist 8): the contract carries no distinct book-outcome records today (PRD-0009's lanes are unmerged; books appear as `SkippedRef`/`NonUrlSource`) — the skipped list *is* the book disclosure for the MVP. Record this in a code comment on the skipped renderer citing PRD-0016 sublist 8.

**Step 4: Run the full gate**

```sh
cargo test -p sp42-assessment && cargo clippy -p sp42-assessment --all-targets -- -D warnings && cargo fmt --check -p sp42-assessment
```
Expected: PASS.

**Step 5: Commit**

```sh
git add crates/sp42-assessment
git commit -m "feat(assessment): GA evidence appendix renderer (PRD-0016 criterion-2 MVP)"
```

---

## Task 6: Phase gate

**Step 1: Workspace-wide checks** (Tauri step will fail on this host for lack of GTK — known environment gap; run the steps individually):

```sh
cargo build --workspace --all-targets --profile ci
cargo test --workspace --profile ci
cargo clippy --workspace --all-targets --all-features --profile ci -- -D warnings
cargo doc --workspace --no-deps --profile ci
./scripts/check-layering.sh
```
Expected: all green (the new crate is additive; nothing upstream changed).

**Step 2: Commit any fallout fixes** (fmt/clippy only) — `chore(assessment): phase-1 gate fixes` — or nothing if clean.
