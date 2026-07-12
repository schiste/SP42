# Article Citation Extractor Implementation Plan — Phase 4: Page Orchestrator

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Fan the existing per-use-site verifier over the extracted use-sites with source-fetch dedupe, and assemble a `PageVerificationReport`. One bad ref never sinks the page.

**Architecture:** Pure `sp42-core` orchestrator. Reuses `verify_citation_use_site` unchanged except for one additive `prefetched` option on `VerifyOptions` so the orchestrator can fetch each distinct URL once and share the body. Reuses `map_with_concurrency`. Tested with `StubHttpClient`/`StubModelClient`.

**Tech Stack:** Rust, `tokio`/`futures`, `std::collections::HashMap`.

**Scope:** Phase 4 of 6.

**Codebase verified:** 2026-06-24
- `VerifyOptions { include_metadata, concurrency, params, repair_turn }` — `#[derive(Debug, Clone)]`, **no lifetime param**, no serde (`verify.rs:247`). Adding a borrowed `&FetchedSource` would force a lifetime onto every call site, so use an **owned** option.
- `FetchedSource { text: String, status: u16 }` is a **private** struct (`verify.rs:611`); `fetch_source<C>(client, url) -> Result<FetchedSource, CitationVerificationError>` is private (`verify.rs`). Both must be promoted to `pub(crate)` (and `FetchedSource` to `pub`, `Clone`) for `page.rs` to reuse.
- `verify_citation_use_site(fetch, model, clock, panel, request, context, use_site_ordinal, options)` takes `options: VerifyOptions` **by value** (`verify.rs:699`).
- `map_with_concurrency(items, limit, f)` where `f: Fn(T, usize) -> Fut` (`concurrency.rs:17`).
- `CitationFinding.verdict: CitationVerdict` (`verify.rs:147`). **Confirmed shape** (`verdict.rs:35`): `enum CitationVerdict { Judged(SupportLevel), SourceUnavailable }` where `enum SupportLevel { Supported, Partial, NotSupported }`. The stats match (Task 3) and the regression assertion (Task 1) use this two-level form — NOT flat `Supported`/`Partial` variants.
- Tests in core are sync `#[test]` + `futures::executor::block_on`; stubs `StubHttpClient`, `StubModelClient`, `FixedClock` come from `sp42-types` (`traits.rs`, `model.rs`).

---

## Task 1: Promote `FetchedSource` / `fetch_source`; add `prefetched` option

**Files:**
- Modify: `crates/sp42-core/src/citation/verify.rs`

**Step 1: Make the fetch primitives reusable**

- Change `struct FetchedSource { … }` (≈ line 611) to:

```rust
/// A fetched source body plus the HTTP status it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedSource {
    pub text: String,
    pub status: u16,
}
```

- Change `async fn fetch_source<C>(…)` to `pub(crate) async fn fetch_source<C>(…)` (signature otherwise unchanged).
- Re-export `FetchedSource` from `lib.rs` (add to the `citation::verify` re-export block).

**Step 2: Add the additive option**

In `VerifyOptions` (≈ line 247) add a field and update `Default`:

```rust
pub struct VerifyOptions {
    pub include_metadata: bool,
    pub concurrency: usize,
    pub params: SamplingParams,
    pub repair_turn: bool,
    /// Pre-fetched source body. When `Some`, `verify_citation_use_site` uses it
    /// instead of fetching — lets the page orchestrator fetch each distinct URL
    /// once. `None` (the default) preserves the byte-identical single-claim path.
    pub prefetched: Option<FetchedSource>,
}
```

In `impl Default for VerifyOptions`, add `prefetched: None,`.

**Step 3: Use the prefetched body in the verifier**

Find where `verify_citation_use_site` calls `fetch_source` (search `fetch_source(` within the function). Replace the single fetch with:

```rust
let fetched = match &options.prefetched {
    Some(source) => source.clone(),
    None => fetch_source(fetch_client, request.source_url.as_str()).await?,
};
```

Then use `fetched.text` / `fetched.status` exactly where the previous `FetchedSource` binding was used. Leave all downstream logic unchanged.

**Step 4: Write a regression test — single-claim path unchanged + prefetch honored**

Add to the existing `verify.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn prefetched_source_skips_http_fetch() {
        use sp42_types::{FixedClock, StubHttpClient};
        // Empty HTTP queue: if the verifier tried to fetch, it would error.
        let http = StubHttpClient::new([]);
        let model = StubModelClient::new([Ok(completion(
            r#"{"verdict": "SUPPORTED", "quote": "cats purr"}"#,
        ))]);
        let request = CitationVerificationRequest {
            wiki_id: "enwiki".into(),
            rev_id: 1,
            title: "Cats".into(),
            claim: "Cats purr.".into(),
            source_url: url::Url::parse("https://example.test/a").unwrap(),
        };
        let mut options = VerifyOptions { repair_turn: false, ..VerifyOptions::default() };
        options.prefetched = Some(FetchedSource { text: "cats purr".into(), status: 200 });
        let outcome = block_on(verify_citation_use_site(
            &http, &model, &FixedClock::new(0), &[model_ref()],
            &request, None, 0, options,
        ))
        .expect("verifies from prefetched source");
        // CitationVerdict is `Judged(SupportLevel)` | `SourceUnavailable`
        // (verify with `rg -n "enum CitationVerdict" -A6 crates/sp42-core/src/citation/verdict.rs`).
        assert_eq!(
            outcome.finding.verdict,
            crate::citation::verdict::CitationVerdict::Judged(
                crate::citation::verdict::SupportLevel::Supported
            )
        );
    }
```

(Use the file's existing helpers for `completion(...)` and a `ModelRef`; if the existing `ModelRef` helper is named `model()` reuse it instead of `model_ref()`. Confirm `CitationVerdict::Supported` is the correct variant via the `rg` in "Codebase verified".)

**Step 5: Run + commit**

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core prefetched_source_skips_http_fetch -- --exact
PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core --lib citation::verify   # confirm existing tests still pass
git add crates/sp42-core/src/citation/verify.rs crates/sp42-core/src/lib.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): additive prefetched option on VerifyOptions

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

---

## Task 2: `verify_page` orchestrator — failing test

**Files:**
- Modify: `crates/sp42-core/src/citation/page.rs`
- Modify: `crates/sp42-core/src/lib.rs` — re-export `verify_page`.

**Step 1: Add the function signature (stub) and the test**

In `page.rs`. Phase 1 already added `use crate::citation::extract::{BlockFailure, SkippedRef};` and `use crate::citation::verify::CitationFinding;` — **extend the extract import** rather than re-importing `BlockFailure`:

```rust
// extend the existing line to include ExtractOutcome:
use crate::citation::extract::{BlockFailure, ExtractOutcome, SkippedRef};
use crate::citation::concurrency::map_with_concurrency;
use crate::citation::verify::{fetch_source, verify_citation_use_site, FetchedSource, VerifyOptions};
use crate::citation::verdict::{CitationVerdict, SupportLevel};
use sp42_types::{Clock, HttpClient, ModelClient, ModelRef};
use std::collections::HashMap;

/// Verify every use-site in `extract` against its source. Fetches each distinct
/// source URL once (shared via the prefetched option), fans the existing
/// per-use-site verifier with bounded concurrency, and assembles a read-only
/// report. A per-use-site error becomes an extraction-failure entry, never a
/// top-level error.
pub async fn verify_page<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    page: &PageVerificationRequest,
    extract: ExtractOutcome,
    options: VerifyOptions,
) -> PageVerificationReport
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let _ = (fetch_client, model_client, clock, panel, page, &extract, &options);
    PageVerificationReport {
        wiki_id: page.wiki_id.clone(),
        rev_id: page.rev_id,
        title: page.title.clone(),
        findings: Vec::new(),
        skipped: extract.skipped,
        extraction_failures: extract.failures,
        stats: PageVerificationStats::default(),
    }
}
```

Add the test module:

```rust
#[cfg(test)]
mod orchestrator_tests {
    use super::*;
    use crate::citation::extract::extract_use_sites;
    use crate::wikitext_editor::{BlockKind, BlockRef, ParsoidBlock};
    use sp42_types::{FixedClock, HttpResponse, StubHttpClient, StubModelClient};

    fn page() -> PageVerificationRequest {
        PageVerificationRequest { wiki_id: "enwiki".into(), title: "Cats".into(), rev_id: 1 }
    }

    fn model_ref() -> ModelRef {
        ModelRef::new("test", "m", "m")
    }

    // Two use-sites citing the SAME url. With dedupe, exactly one fetch.
    fn two_use_sites_same_url() -> ExtractOutcome {
        let b = ParsoidBlock {
            text: "Cats purr. Cats sleep.".into(),
            section_path: vec!["Behaviour".into()],
            refs: vec![
                BlockRef { offset: 10, ref_id: "r1".into(),
                    source_urls: vec![url::Url::parse("https://s.test/x").unwrap()],
                    ref_text: "[1]".into(), named: false },
                BlockRef { offset: 22, ref_id: "r2".into(),
                    source_urls: vec![url::Url::parse("https://s.test/x").unwrap()],
                    ref_text: "[2]".into(), named: false },
            ],
            block_kind: BlockKind::Paragraph,
            block_ordinal: 0,
        };
        extract_use_sites(&[b], &page())
    }

    #[test]
    fn dedupes_fetches_and_verifies_each_use_site() {
        use futures::executor::block_on;
        // EXACTLY ONE http response: proves the shared URL is fetched once.
        let http = StubHttpClient::new([Ok(HttpResponse {
            status: 200,
            headers: std::collections::BTreeMap::new(),
            body: b"cats purr and sleep".to_vec(),
        })]);
        // One model completion per use-site (panel size 1, repair off).
        let model = StubModelClient::new([
            Ok(/* completion helper */ super::tests::completion(
                r#"{"verdict":"SUPPORTED","quote":"cats purr"}"#)),
            Ok(super::tests::completion(
                r#"{"verdict":"SUPPORTED","quote":"sleep"}"#)),
        ]);
        let options = VerifyOptions { repair_turn: false, concurrency: 2, ..VerifyOptions::default() };
        let report = block_on(verify_page(
            &http, &model, &FixedClock::new(0), &[model_ref()],
            &page(), two_use_sites_same_url(), options,
        ));
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.stats.use_sites_verified, 2);
        // If a second fetch had been attempted the queue would be empty → error path.
        assert!(report.extraction_failures.is_empty());
    }
}
```

Notes for the executor:
- `HttpResponse` is `{ status: u16, headers: BTreeMap<String, String>, body: Vec<u8> }` (confirmed in `sp42-types/src/traits.rs`); the literal above matches. If a field name differs in the current tree, adjust.
- `completion(...)` is a helper in `verify.rs`'s test module. If it isn't reachable as `super::tests::completion`, copy the one-line helper into this test module (build a `ModelCompletion` with the JSON as content) rather than reaching across modules.

**Step 2: Run to verify it fails**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core dedupes_fetches_and_verifies_each_use_site -- --exact`
Expected: FAIL (stub returns empty findings).

---

## Task 3: Implement `verify_page`

**Files:**
- Modify: `crates/sp42-core/src/citation/page.rs`

**Step 1: Replace the stub body**

```rust
pub async fn verify_page<C, M>(
    fetch_client: &C,
    model_client: &M,
    clock: &dyn Clock,
    panel: &[ModelRef],
    page: &PageVerificationRequest,
    extract: ExtractOutcome,
    options: VerifyOptions,
) -> PageVerificationReport
where
    C: HttpClient + ?Sized,
    M: ModelClient + ?Sized,
{
    let ExtractOutcome { use_sites, skipped, failures } = extract;
    // refs_seen = every ref we encountered: those that became use-sites, those
    // skipped (non-URL), and those that failed extraction.
    let refs_seen = use_sites.len() + skipped.len() + failures.len();

    // Pre-bind shared refs OUTSIDE the closures so the spawned futures capture
    // plain `&`/`&dyn` (Copy) references, not re-borrows of locals — mirrors the
    // panel fan-out in verify.rs and avoids fighting the borrow checker.
    let fetch_client: &C = fetch_client;
    let model_client: &M = model_client;
    let clock: &dyn Clock = clock;
    let panel: &[ModelRef] = panel;

    // 1. Dedupe: distinct source URLs, fetched once each.
    let mut distinct: Vec<String> = Vec::new();
    for us in &use_sites {
        let u = us.request.source_url.to_string();
        if !distinct.contains(&u) {
            distinct.push(u);
        }
    }
    let concurrency = options.concurrency;
    let fetched_list = map_with_concurrency(distinct.clone(), concurrency, |url, _| async move {
        (url.clone(), fetch_source(fetch_client, &url).await.ok())
    })
    .await;
    let mut bodies: HashMap<String, FetchedSource> = HashMap::new();
    for (url, source) in fetched_list {
        if let Some(source) = source {
            bodies.insert(url, source);
        }
    }

    // 2. Fan verify over use-sites, sharing the prefetched body.
    let mut extraction_failures = failures;
    let mut findings = Vec::new();
    let bodies_ref = &bodies;
    let options_ref = &options;
    let results = map_with_concurrency(use_sites, concurrency, |us, _| async move {
        let mut opts = options_ref.clone();
        opts.prefetched = bodies_ref.get(&us.request.source_url.to_string()).cloned();
        let outcome = verify_citation_use_site(
            fetch_client, model_client, clock, panel,
            &us.request, Some(&us.context), us.use_site_ordinal, opts,
        )
        .await;
        (us.ref_id, us.block_ordinal, outcome)
    })
    .await;

    for (ref_id, block_ordinal, outcome) in results {
        match outcome {
            Ok(o) => findings.push(o.finding),
            Err(error) => extraction_failures.push(BlockFailure {
                block_ordinal,
                reason: format!("verify failed for {ref_id}: {error}"),
            }),
        }
    }

    // 3. Stats.
    let mut stats = PageVerificationStats {
        refs_seen,
        use_sites_verified: findings.len(),
        skipped: skipped.len(),
        extraction_failures: extraction_failures.len(),
        ..PageVerificationStats::default()
    };
    for f in &findings {
        match f.verdict {
            CitationVerdict::Judged(SupportLevel::Supported) => stats.supported += 1,
            CitationVerdict::Judged(SupportLevel::Partial) => stats.partial += 1,
            CitationVerdict::Judged(SupportLevel::NotSupported) => stats.not_supported += 1,
            CitationVerdict::SourceUnavailable => stats.source_unavailable += 1,
        }
    }

    PageVerificationReport {
        wiki_id: page.wiki_id.clone(),
        rev_id: page.rev_id,
        title: page.title.clone(),
        findings,
        skipped,
        extraction_failures,
        stats,
    }
}
```

Executor notes:
- `BlockFailure`, `CitationVerdict`, `SupportLevel` are imported in the Task 2 Step 1 import block — do not re-`use` them.
- The match arms use the confirmed `CitationVerdict::Judged(SupportLevel::…)` / `SourceUnavailable` shape and are exhaustive as written.
- The shared refs are pre-bound above both `map_with_concurrency` calls so each future captures plain `&`/`&dyn` references; this matches the panel fan-out in `verify.rs`. If the borrow checker still objects, the cause is almost always a future outliving the borrow — keep the `.await` inside `verify_page`'s frame (it is) and do not `tokio::spawn`.
- `us.block_ordinal` requires the `block_ordinal` field added to `CitationUseSite` in Phase 1 / populated in Phase 3 — confirm it is present.

**Step 2: Run the orchestrator test**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core dedupes_fetches_and_verifies_each_use_site -- --exact`
Expected: PASS.

**Step 3: Re-export, clippy, fmt, commit**

```bash
# add verify_page to the citation::page re-export in lib.rs
PATH="$HOME/.cargo/bin:$PATH" cargo clippy -p sp42-core --all-targets -- -D warnings && cargo fmt -p sp42-core
PATH="$HOME/.cargo/bin:$PATH" cargo test -p sp42-core --lib citation
git add crates/sp42-core/src/citation/page.rs crates/sp42-core/src/lib.rs
SP42_SKIP_GIT_HOOKS=1 git commit -m "feat(citation): verify_page orchestrator with fetch dedupe

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_015NtcEzaj8J373GGsv3FC7y"
```

**Done when:** `verify_page` fetches a shared URL once, produces one finding per use-site, isolates per-use-site errors into `extraction_failures`, tallies stats, and carries `skipped`/`failures` through; the single-claim path (`prefetched: None`) is unchanged; clippy clean.
