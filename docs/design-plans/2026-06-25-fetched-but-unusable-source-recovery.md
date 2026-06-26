# Fetched-but-Unusable Source Detection & Recovery Design

## Summary

SP42 verifies Wikipedia citations by fetching the cited source, extracting body text, and running that text through a body-usability gate before passing anything to a language-model panel. The gate's current design sees only extracted text, so several real-world source shapes pass as "usable" even though no model can evaluate them. They fall into three categories: pages behind paywalls or registration walls (which cause the model to confabulate a `partial` verdict); raw PDF bytes; and **host-specific "special-case" sources** whose generic extraction returns a viewer shell or chrome rather than content — Google Books is the canonical example, but it is one of a large class (other JavaScript book/document readers, embedded viewers, and database front-ends commonly cited on Wikipedia). The fix is to widen the gate's view — a new unified entry point accepts URL, content-type, and body text together, so it can detect PDFs by magic bytes or MIME type and match known special-case hosts against a small, **extensible host-rule table** (seeded with Google Books) before any text-shape analysis is attempted.

The design is split into two sequential pieces. Piece 1 (Phases 1–4, the immediate deliverable) adds these detectors and extends the reason enum to name the specific unusable kind (`PdfBody`, `ViewerShell`, `NavChromePaywall`). That specific reason is threaded onto the per-citation finding record so the reviewer report can distinguish a tool limitation from a generic unreadable body — without touching the existing verdict taxonomy, because a 2xx-but-unusable body already routes through the `SourceUnavailable` path. Piece 2 (Phases 5–8, future work) adds recovery: Wayback Availability API fallback for dead links, in-crate PDF text extraction, and **per-host source adapters** — a Google Books snippet lookup and an arXiv HTML-twin fetch are the first two instances of an extensible recovery class, not one-offs. Piece 2 is gated on ADR-0012, which must codify the recovery-fetch policy before any recovery code is written.

## Definition of Done

This design covers two sequential pieces. **Piece 1 (detection + reason precision)** is the immediate deliverable; **Piece 2 (recovery)** is future work gated by a new fetch-edge ADR (ADR-0012) and a separate implementation plan.

**Piece 1 is done when:**
- A paywall / nav-chrome stub that currently passes the body-usability gate (issue #42) is classified `Unusable` and short-circuits to `SourceUnavailable` **before** any model-panel call — verified by a regression test over a Law360-shaped fixture that asserts zero model-client invocations.
- A PDF source (by `application/pdf` content-type **or** `%PDF-` magic bytes) classifies `Unusable` with a `PdfBody` reason and makes **zero model-panel calls**, deterministically — verified by unit tests, including a PDF mislabeled `text/html`. (Deterministic format detection: a PDF is genuinely unreadable by generic extraction, so short-circuiting it costs no coverage.)
- A known special-case host (Google Books as the seed entry in an extensible host-rule table), matched on the citation's `source_url`, classifies `Unusable` with a `ViewerShell` reason and makes zero model-panel calls — verified by a unit test. (Redirect-target matching is deferred to Piece 2 / ADR-0012: `HttpResponse` exposes no final URL today, and `{{cite}}` `url=` points directly at the host in the overwhelming majority of cases.)
- The specific unusable kind is carried on the finding (`CitationFinding.unusable_reason: Option<BodyUsabilityReason>`), so a tool-limitation (`PdfBody`/`ViewerShell`/`NavChromePaywall`) is distinguishable from a generic unreadable body — verified by serde round-trip tests including back-compat (legacy findings deserialize to `None`). The reviewer report surfaces this kind (Phase 4); the finding-field/serde behavior is the hard, independently-tested requirement, report formatting follows it.
- The nav-chrome/paywall detector is judged on **net usefulness, measured** — not on never being wrong. Success = a net reduction in confabulated verdicts (catches the #42 class, incl. the Law360 case) **without material loss of coverage** on readable sources, with *both* rates measured on a representative fixture sample (real registration walls + real articles) and reported — neither driven to a corner. Some false positives are explicitly acceptable: a wrongly-flagged readable source costs one abstention (the reviewer still sees the citation), and grounding remains the backstop for misses (in #42 grounding held; only the label was wrong).
- No change to the `SourceUnavailableReason` enum or `PageVerificationStats` fields; `cargo test -p sp42-core` and the wasm build of `sp42-app` both pass.

**Piece 2 is done when** (tracked here for direction, specified in a later plan): dead-link citations recover via a Wayback Availability-API lookup when no citation `archive-url` exists (#46); PDF citations recover readable text (#52); and recovery provenance is transparent. Piece 2 requires ADR-0012 (fetch-edge policy) first.

## Glossary

- **Body-usability gate**: The classifier (`classify_body_usability`, `body_classifier.rs`) that runs on fetched content before the model panel is invoked. If the body fails the gate, processing short-circuits to `SourceUnavailable` with no model calls. Currently text-only; Piece 1 extends it to also accept URL and content-type.
- **`BodyUsabilityReason`**: Rust enum identifying *why* a body failed the usability gate (e.g., anti-bot challenge, short body). Piece 1 adds `PdfBody`, `ViewerShell`, and `NavChromePaywall` variants.
- **`CitationFinding`**: The per-citation result record holding the verdict, model votes, grounding status, and source provenance. Piece 1 adds an `unusable_reason` field to it.
- **Confabulated verdict**: A model producing a plausible-sounding but unsupported verdict — specifically, returning `partial` for a paywall stub it cannot actually read. Issue #42 is an instance.
- **`GroundingStatus`**: A field on `CitationFinding` recording whether the source text was found to contain the cited claim, independently of the verdict. Because grounding was already `unlocated` for paywalled sources (#42), autonomous behavior was already safe before Piece 1.
- **Model panel**: The set of language models that vote on whether a source supports a cited claim. Invoked only when the usability gate passes; the central goal of Piece 1 is to prevent unreadable bodies from reaching it.
- **Nav-chrome / paywall stub**: A page from a paywalled or registration-walled site containing navigation and a subscribe/sign-in prompt but no article content — delivered as HTTP 2xx, so currently invisible to the gate.
- **`SourceUnavailable`**: The verdict assigned when a source cannot be evaluated — either `Unreachable` (network/HTTP failure) or `Unusable` (body received but unreadable). The verdict enum is unchanged by Piece 1; only the alongside `unusable_reason` field is new.
- **SSRF guard**: Server-Side Request Forgery protection (`check_fetchable_source_url`, `urls.rs`) preventing the fetch stack from being directed at internal addresses. Recovery fetches in Piece 2 must pass through the same guard.
- **`try_archive_fallback`**: Existing function (`page.rs`) that substitutes a citation-supplied `archive_url` when the live URL is dead, recording `CitationFinding.archive_of`. Piece 2 extends it to query the Wayback Availability API when no `archive_url` is present.
- **Wayback Availability API**: The Internet Archive's public, keyless endpoint (`archive.org/wayback/available`) for querying whether a URL has a saved snapshot and obtaining the closest one. Used in Phase 6 as a dead-link fallback.
- **wasm32 / wasm-safe**: The `wasm32-unknown-unknown` target used by the browser-side `sp42-app` crate, which pulls in `sp42-core`. Code compiled to wasm32 cannot use native dependencies; all Piece 1 work is byte/string inspection and is inherently wasm-safe. Piece 2's PDF extractor must be pure-Rust or `cfg`-excluded from the wasm build.

## Architecture

When SP42 verifies a citation it fetches the cited source, extracts text, and runs a deterministic **body-usability gate** before spending a model panel on the claim. Today the gate (`classify_body_usability`, `crates/sp42-core/src/citation/body_classifier.rs`) only sees the *extracted text* and recognizes seven artifact shapes (JSON-LD/CSS leaks, anti-bot challenges, Wayback notices/chrome, an Amazon stub, and a <300-char short-body floor). Three real-world "fetched 2xx but unreadable" shapes slip through: **paywall/nav-chrome stubs** (issue #42 — these reach the panel and a model confabulates a `partial`), **PDF bytes** (#52), and **Google Books JavaScript viewer shells** (#53).

The `SourceUnavailable` verdict already carries a coarse reason — `SourceUnavailableReason { Unreachable, Unusable }` (`verify.rs:150`) — but that reason is currently chosen purely by HTTP status (`verify.rs:666–668`: 2xx → `Unusable`, else → `Unreachable`), independent of *why* the body was unusable. The rich `BodyUsabilityReason` the classifier computes is discarded (`verify.rs:774` reads only `.usable`).

**Piece 1** consolidates detection into one entry point that sees URL, content-type, and text, adds three detectors, and threads the specific reason onto the finding:

```
classify_source_usability(
    source_url: &str,
    content_type: &str,
    raw_html: Option<&str>,   // pre-extraction HTML; needed for structured paywall markers
    text: Option<&str>,        // extracted article text (post html_to_text)
) -> BodyUsability
```

The classifier sees the **raw HTML** as well as the extracted text, because the deterministic paywall markers (schema.org JSON-LD, `<meta>`, vendor `<script src>`) live in markup that `html_to_text` strips. `FetchedSource` carries `raw_html: Option<String>` (the pre-extraction body for HTML responses, `None` otherwise). Raw HTML is needed only at classification — grounding uses the extracted text — so it is consumed at the gate and not retained downstream (see the #59 note in Additional Considerations). Carrying it now also makes the deferred `dom_smoothie` swap (a raw-HTML consumer) a drop-in.

Dispatch order: (1) PDF — `content_type` contains `application/pdf` or the body starts with `%PDF-` → `PdfBody`; (2) special-case host — `source_url` matches an entry in an **extensible host-rule table** of known unreadable-by-generic-extraction sources, seeded with `books.google.*` → `ViewerShell`; (3) the layered `NavChromePaywall` detector (markers from `raw_html` + prose from `text` + consent guard); (4) otherwise the existing text-shape detectors. `ViewerShell` names the failure mode — a JavaScript/embed viewer shell or chrome returned instead of readable content — and is a *class* (book viewers, embedded readers, database front-ends), not a single site; the host-rule table is the extension point for adding more. Because a detected-unusable 2xx body already routes to `SourceUnavailableReason::Unusable` via the existing http_status logic, **no verdict-enum surgery is required** — the specific kind rides alongside on `CitationFinding.unusable_reason` (option C from brainstorming), where the report renders it and Piece 2's recovery dispatch keys off it.

**Piece 2** (future) adds recovery for the recoverable reasons: extend `try_archive_fallback` (`page.rs`) to query the Wayback Availability API when no citation `archive-url` exists (#46), and recover PDF text in-crate (#52). The "prefer live → recover → none" policy and the requirement that recovery fetches honor the same SSRF/size/redirect guards as live fetches become **ADR-0012**, satisfying issue #34.

## Existing Patterns

This design extends, rather than replaces, the established citation-verification architecture (ADR-0007 semantics, ADR-0008 contract, ADR-0011 article-level verification):

- **Usability gate + reason enum.** `classify_body_usability` already returns `BodyUsability { usable: bool, reason: BodyUsabilityReason }`. Piece 1 adds enum variants and a wider-input entry point following the same shape; it does not introduce a parallel taxonomy.
- **Verdict/reason separation.** The codebase already separates the flat verdict (`Verdict`/`CitationVerdict`) from the reason (`SourceUnavailableReason`) and from `GroundingStatus`. Carrying `unusable_reason` on the finding follows that separation.
- **Short-circuit before panel.** `verify_citation_use_site` (`verify.rs:~774`) already returns `SourceUnavailable` with empty votes when `!usable`. The #42 fix relies on this existing path — the new detector simply makes the stub trip it.
- **Archive fallback.** `try_archive_fallback` (`page.rs`) already recovers dead links from citation-supplied `archive_urls`, marks `CitationFinding.archive_of`, and deliberately refuses to archive `Unusable` sources. Piece 2 extends this function rather than adding a new recovery path.
- **Injected, hardened HTTP.** Fetch goes through the `HttpClient` trait (SSRF guard in `urls.rs::check_fetchable_source_url`, redirect re-check and stream/size caps from #43/#51 live in the client impl). Recovery fetches reuse this; no new fetch stack.
- **Provenance.** `SourceProvenance` records `url`, `content_hash`, `fetched_at`, `http_status`. Recovery transparency reuses `archive_of` and the same provenance discipline.

`sp42-core` compiles to `wasm32` (pulled in by the Leptos/`wasm-bindgen` crate `sp42-app`), so all Piece 1 code is text/URL/byte inspection only — no native deps. Piece 2's PDF extractor must be wasm-safe pure-Rust or `cfg`-gated out of the wasm build.

## Implementation Phases

### Phase 1: Unified usability classifier + reason variants
**Goal:** One entry point that can see URL, content-type, and text; reason enum extended.

**Components:**
- `BodyUsabilityReason` in `body_classifier.rs` — add `PdfBody`, `ViewerShell`, `NavChromePaywall`.
- `classify_source_usability(source_url, content_type, text)` in `body_classifier.rs` — new entry point that dispatches the new URL/content-type detectors then delegates to the existing text-shape detectors. `classify_body_usability` remains the text-shape core.
- `FetchedSource` in `verify.rs` — carry `content_type` and `raw_html: Option<String>` (the pre-extraction body for HTML responses) through. Both are available inside `fetch_source` today and currently discarded.

**Dependencies:** None.

**Done when:** `classify_source_usability` compiles, existing text-shape behavior is unchanged (existing classifier tests still pass), and `FetchedSource` exposes content-type.

### Phase 2: PDF detector + special-case host-rule table
**Goal:** Deterministic classification of PDF bytes and host-specific viewer-shell sources (#52, #53 detection half).

**Components:**
- PDF detector in `body_classifier.rs` — `application/pdf` content-type or `%PDF-` magic prefix → `PdfBody`.
- Host-rule table in `body_classifier.rs` — a small extensible set of host patterns for sources that return a viewer shell/chrome under generic extraction, matched on the final/redirected `source_url` → `ViewerShell`. Seeded with `books.google.*`; structured so further hosts are data, not new code paths.
- Wire `verify_citation_use_site` (`verify.rs:~774`) to call `classify_source_usability` and store the reason for the finding.

**Dependencies:** Phase 1.

**Done when:** Unit tests pass for PDF-by-content-type, PDF-by-magic (incl. `text/html`-mislabeled PDF), and host-rule match on the seed entry (incl. post-redirect host); these bodies no longer reach the model panel.

### Phase 3: Nav-chrome / paywall detector (#42)
**Goal:** Classify paywall/registration stubs as `Unusable`, killing the confabulated `partial`.

**Components:**
- `NavChromePaywall` detector in `body_classifier.rs` — fires only when **both** (a) a paywall **marker** and (b) **no substantial readable article prose**. The prose check (b) is load-bearing; the markers (a) corroborate that missing content is gated, not just an extraction miss. Marker (a) is the union of signals, deterministic-first (informed by prior-art research, 2026-06-25):
  - **schema.org `isAccessibleForFree: false`** in JSON-LD — located by a coarse `<script type="application/ld+json">` block match, then **parsed with `serde_json`** (recursively, incl. nested `hasPart`); not regex-matched inside the JSON. High precision when present.
  - **`<meta property="article:content_tier" content="locked|metered">`** (Open Graph).
  - **paywall-vendor fingerprint** — a `<script>`/`<link>` src whose host is in a small curated set of paywall-platform domains (piano.io, tinypass, zephr, poool, arc-xp, pelcro, evolok, wallkit). Domains are facts (no license entanglement); cross-referenced against miscfilters/antipaywall.txt (GPL-3.0, compatible) for maintenance.
  - **registration-phrase regex** (the original curated set: "subscribe to read", "sign in to continue", …) as the weakest fallback marker.
  Plus a **consent-wall guard**: suppress when the page is a GDPR "accept-or-pay"/cookie-consent shape rather than a paywall (the top false-positive source per BannerClick, IMC 2023). Because (b) is load-bearing, a paywalled page that still ships the full article text reads as high-prose and is **not** flagged — we verify it (coverage win). The conjunction is the starting heuristic, tuned for balance per the net-value stance, not a zero-false-positive corner. (`dom_smoothie`, a wasm-safe pure-Rust readability extractor, is the planned upgrade for signal (b) — deferred; see Additional Considerations.)
- Fixture set under the crate's test data — a handful of real registration walls plus real articles as negative controls, used to tune the threshold.

**Dependencies:** Phase 1.

**Done when:** The Law360-shaped #42 regression fixture classifies `NavChromePaywall`/`Unusable` with **zero** model-client calls; on the representative fixture sample the detector shows a net reduction in confabulations without material coverage loss on the real-article controls (both rates reported, not driven to a zero-false-positive corner); detector unit tests pass.

### Phase 4: Finding surface + report rendering
**Goal:** Surface the specific unusable kind to the reviewer (option C).

**Components:**
- `CitationFinding.unusable_reason: Option<BodyUsabilityReason>` in `verify.rs` — `#[serde(default)]`, populated on short-circuit.
- Report rendering (`sp42-reporting` / page report) — render the kind from findings; `PageVerificationStats` fields unchanged (per-kind counts aggregated from findings, not new stats fields).

**Dependencies:** Phases 2–3.

**Done when:** Serde round-trips with and without `unusable_reason` (legacy → `None`); the reviewer report distinguishes PDF / viewer-shell / paywall from a generic unreadable body; `cargo test -p sp42-core` and the `sp42-app` wasm build pass.

### Phase 5: ADR-0012 — fetch-edge recovery policy (#34) [Piece 2]
**Goal:** Document the "prefer live → recover → none" policy and the guard-reuse requirement before building recovery.

**Components:**
- `docs/adr/0012-*.md` — recovery trigger conditions (which `BodyUsabilityReason`s are recoverable vs terminal), the live→recover→none ordering, and the rule that archive/recovery fetches honor the same SSRF/size/redirect/UA-maxlag guards as live fetches.

**Dependencies:** Phase 4 (reason taxonomy stable).

**Done when:** ADR-0012 accepted; issue #34 closed by it.

### Phase 6: Wayback Availability-API fallback (#46) [Piece 2]
**Goal:** Recover dead links that have no citation `archive-url`.

**Components:**
- Extend `try_archive_fallback` (`page.rs`) — when `Unreachable` and `archive_urls` is empty/exhausted, query `https://archive.org/wayback/available?url=…`, fetch the closest snapshot via the `…id_/` raw form, mark `archive_of`. Keyless; ~1 req/s etiquette + UA with contact.

**Dependencies:** Phase 5.

**Done when:** A dead live URL with a Wayback capture verifies from the archive with `archive_of` set; both-fail still yields `SourceUnavailable`; recovery fetch passes the SSRF/size guards.

### Phase 7: Generic PDF → text recovery (#52) [Piece 2]
**Goal:** Recover readable text from `PdfBody` sources via generic extraction.

**Components:**
- In-crate PDF→text via a wasm-safe pure-Rust crate (`pdf_oxide`/`pdf-extract`) or a `cfg`-gated path; re-run the usability gate + panel on the extracted text.
- Note: some `PdfBody` sources are better recovered by a per-host adapter than by generic extraction (e.g. arXiv `/pdf/` → its HTML twin) — see Phase 8. Detection caveat: do **not** host-special-case arXiv in *detection* — `/abs/` pages are legitimate HTML and must not be flagged; only the content-type/magic PDF detector applies there.

**Dependencies:** Phase 5.

**Done when:** A reachable PDF that supports a claim verifies via extracted text; scanned-image/encrypted PDFs degrade cleanly to `Unusable`; wasm build unaffected.

### Phase 8: Per-host source adapters — extensible recovery class [Piece 2]
**Goal:** Recover host-specific sources that generic extraction can't, via an extensible registry of per-host adapters mirroring the Phase-2 host-rule table. Google Books and arXiv are the first two instances flagged for careful investigation — not a hard commitment to build either.

**Components:**
- A per-host recovery adapter registry — given a `ViewerShell`/`PdfBody` source whose host has an adapter, fetch readable content via that host's preferred path before falling back to generic handling.
- **arXiv HTML-twin adapter** (investigation): recover an arXiv `/pdf/` citation by fetching the HTML rendering (`arxiv.org/html/<id>`, ar5iv fallback) or the `/abs/` abstract — cleaner than PDF extraction, no extraction libs.
- **Google Books snippet adapter** (investigation, likely not built): Books API `volumes/<id>` lookup (id from the URL) for `searchInfo.textSnippet`. Caveats: requires an API key and yields only a 1–2 sentence snippet (marginal grounding). Detection (Phase 2) already removes the misleading verdict, so this is a documented option, not a requirement.

**Dependencies:** Phase 5.

**Done when:** The adapter registry exists with a documented build/skip decision per investigated host; any adapter that ships grounds transparently and configures its own auth/etiquette.

## Additional Considerations

**Implementation scoping.** Piece 1 (Phases 1–4) is the immediate implementation plan. Piece 2 (Phases 5–8) is future work that requires ADR-0012 first and should be its own implementation plan(s); it is included here for direction and contract continuity, not for immediate execution.

**#42 risk framing.** In #42 grounding already held (`grounding_status: unlocated`), so autonomous use was already safe — the only harm was the human-facing `partial` label. Both error costs for the paywall detector are bounded: a false positive costs one abstention on a citation the reviewer can still inspect; a false negative degrades to today's behavior with grounding as the backstop. Because neither error is catastrophic, the detector is tuned for **balance**, not for never being wrong.

**Net-value stance (don't compound over-conservatism).** SP42 already errs conservative — #25 documents the citation port emitting false `Not supported` because it is *too* strict. New gate/detector layers compound that: each "be safe" layer that abstains trades coverage for caution, and stacked together they can make the tool safe and useless. So fuzzy detectors (the paywall heuristic, not the deterministic PDF/host checks) are evaluated on **net value — bad verdicts removed vs. coverage retained — measured and reported**, never on a zero-false-positive corner. If fixture tuning proves insufficient, the fallback is to ship the heuristic behind a flag and tune against real review traffic rather than tighten it blindly.

**Paywall-detection prior art & ceiling (research, 2026-06-25).** HTML-only fetching (no JS execution / headless browser) structurally cannot see interaction-triggered, fingerprint-gated, or JS-rendered walls — automated crawls miss ~45% of them ("Beyond the Crawl," WWW 2025), so ~77% precision/recall is the literature ceiling (Papadopoulos et al., WWW 2020). This is a property of the medium, not timidity, and is exactly why the bounded-error / grounding-as-backstop framing holds. The detector layers cheap deterministic markers (schema.org `isAccessibleForFree`, `article:content_tier`, paywall-vendor fingerprints) ahead of a content heuristic, with a consent-wall guard for the top false-positive source (BannerClick, IMC 2023). The first cut threads **raw HTML** to the gate (the markers live in stripped markup) and uses a sentence-count proxy for the content half. Planned follow-on (deferred dependency, not plumbing): `dom_smoothie` (pure-Rust, wasm32-in-CI) for a principled content-quality signal in place of the sentence-count proxy — a drop-in now that raw HTML is plumbed — bundled with **adopting a real HTML/DOM parser for the citation path**: the crate has no DOM parser today (`html_to_text` is regex-based), so one parser would replace that regex extraction *and* serve the structured marker queries (`<meta>`, vendor `<script src>`) and readability from a single parse. Until then, the first cut keeps the marker extraction lightweight — JSON-LD via `serde_json` (the one signal that truly needs structured parsing), `<meta>`/vendor via targeted matching — and a curated domain-prior list (everywall/ladder-rules, MIT) only if recall plateaus.

**Raw-HTML retention & #59.** The gate now needs the pre-extraction HTML for the structured markers. Raw HTML is consumed at classification and **not retained** afterward (grounding uses the extracted text), so it must not enter the page-prefetch body cache — dropping it post-classification is a small eviction that fits naturally into the #59 evict-after-last-use work. First cut: the single-URL verify path carries `raw_html` transiently; the page path classifies-then-drops. Sources: schema.org paywalled-content guidance; `crates.io/crates/dom_smoothie`; `github.com/everywall/ladder-rules`; `github.com/liamengland1/miscfilters`; BannerClick (arXiv 2310.01108); Beyond the Crawl (arXiv 2502.01608).

**Out of scope.** Reconsidering whether `SourceUnavailable` stays model-votable (ADR-0007 §4 "A3") is explicitly out of scope (per #42). Piece 1 ships the host-rule table seeded with Google Books only; populating it with additional special-case hosts is follow-on work (adding table data, not new code paths). ISBN → Internet Archive book sourcing (the `{{cite book}}`/Citoid ISBN path) is a separate stream — ISBN is not reachable in the pipeline today.
