//! Citation verification (PRD-0001): the pure, deterministic heart plus the
//! injected-edge orchestration for fetching a source and asking a model panel
//! whether it supports a claim.
//!
//! Layering (Constitution Art. 2.3 — side effects only at the edges):
//! - Pure `sp42-citation` logic: the verdict types, the anti-fabrication locator,
//!   measured-agreement voting, the body-usability GIGO gate, the verifier
//!   prompt, the model-response parser, URL helpers, source-text
//!   recovery/extraction, the Citoid metadata sidecar, bounded concurrency, the
//!   grounding/assemble gate and per-model edge, and the content-addressed
//!   snapshot/verdict store.
//! - I/O is confined to the `async` functions, which are generic over the
//!   injected `HttpClient` / `Storage` / `Clock` traits.
//!
//! Verdict semantics → ADR-0007; request/response contract → ADR-0008; model
//! panel + measured agreement → ADR-0006; source-snapshot storage → ADR-0009.

pub mod body_classifier;
pub mod citoid;
pub mod concurrency;
pub mod extract;
pub mod locate_quote;
pub mod openlibrary;
pub mod page;
pub mod parsing;
pub mod prompts;
pub mod search_inside;
pub mod segment;
pub mod source_fetch;
pub mod storage;
pub mod urls;
pub mod verdict;
pub mod verify;
pub mod voting;
