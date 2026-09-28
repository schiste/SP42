# Intake / Lifecycle / Gates Implementation Plan — Phase 1: ADR-0030 reviewable item identity

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** Implement ADR-0030 — `ReviewableItemId`, the opaque, globally unique,
revision-independent, lane-agnostic identity that ADR-0027's lifecycle record is
keyed on and ADR-0028/0029 key their verdicts on. Nothing else in the chain is
constructible without it.

**Architecture:** New module `crates/sp42-types/src/reviewable.rs` holding the
type and its versioned codec, re-exported flat from `lib.rs`. Plus a thin
resolution surface in `sp42-platform`. No storage, no engine, no I/O.

**Tech Stack:** `serde`, `thiserror` (both already `sp42-types` deps), `proptest`
(already a `sp42-platform` dev-dependency) for the round-trip property ADR-0030
requires.

**Scope:** Phase 1 of 8. Depends on Phase 0 Task 1 only for the ability to consult
the registry; the identity type itself needs nothing from Phase 0.

**Codebase verified:** 2026-09-28.

- `crates/sp42-types/src/lib.rs` is 21 lines: `#![forbid(unsafe_code)]`, four
  `pub mod` (errors, model, traits, transport), then flat `pub use` re-exports.
  Module-per-boundary, not per-type; callers write `use sp42_types::Clock`, never
  `use sp42_types::traits::Clock`.
- `crates/sp42-types/src/model.rs:1-12` states the house rule for this crate: a
  provider/vendor dependency must never enter it; features depend on the trait.
- `sp42-coordination/src/codec.rs` (msgpack via `rmp-serde`) is the codec
  precedent, and `codec.rs:142` holds a `proptest!` round-trip block.
- The repo's persistence-versioning precedent is a `const …_SCHEMA_VERSION: u32`
  plus a `{project, version, …}` envelope — see
  `sp42-citation/src/citation/storage.rs:31-33` and
  `sp42-platform/src/wiki_storage.rs:112-121`.
- Additive-serialization precedent, and the exact wording to mirror:
  `sp42-platform/src/types.rs:105-112` — *"Additive and serde-back-compatible: old
  snapshots deserialize to `None` and `None` serializes to nothing."* Used with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.

---

## Task 1: The type

**Files:**
- Create: `crates/sp42-types/src/reviewable.rs`
- Modify: `crates/sp42-types/src/lib.rs` (add `pub mod reviewable;` + re-exports)

**Step 1: Declare the type per ADR-0030 §1.**

```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReviewableItemId {
    kind: ReviewableItemKind,
    wiki_id: String,
    local_id: String,
    v: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ReviewableItemKind {
    Page,
    Draft,
    Nomination,
    InvestigationCase,
    Custom(String),
}
```

Fields are private with accessors. ADR-0030 §1 says opaque, and §2 says `kind` is
descriptive rather than load-bearing — keeping the fields private is what stops a
caller from pattern-matching its way to a policy decision, which §2 explicitly
rules out ("a rule that needed to know the kind should say so through a
domain-owned field"). Derive `Ord` deliberately: the lifecycle store and the
recency query will need a total order, and CONSTITUTION §1.4 requires ordering to
be explicit rather than incidental.

**Step 2: Constructors that cannot mint a nonsense id.** `Page` must have a
non-empty `wiki_id` and `local_id`; reject empties with the module's own error enum
(CONSTITUTION §6.3: one error enum per module, no `anyhow` in a public interface):

```rust
#[derive(Debug, thiserror::Error)]
pub enum ReviewableItemIdError {
    #[error("reviewable item id requires a non-empty wiki_id")]
    EmptyWikiId,
    #[error("reviewable item id requires a non-empty local_id")]
    EmptyLocalId,
    #[error("unrecognised reviewable item id encoding: {0}")]
    Decode(String),
}
```

**Step 3: Round-trip codec, versioned per ADR-0030 §1.** `v` exists so the
serialized form can change without a flag-day migration (CONSTITUTION §9.2). Keep
the wire form a single delimiter-joined string so it round-trips through
`FileStorage`'s hex-encoded key names and through `Storage` keys generally:

```
reviewable:v1:<kind>:<wiki_id>:<local_id>
```

`local_id` may contain `:`? Decide now and pin it with a test — recommend
rejecting `:` in `local_id` at construction, so the encoding stays unambiguous
without escaping.

**Step 4: Serialize transparently as the string form** via
`#[serde(try_from = "String", into = "String")]`, so snapshots and storage
payloads hold a readable id rather than a nested object.

**Step 5: Commit** — `feat(types): ReviewableItemId and its versioned encoding`

---

## Task 2: Pin the invariants with tests

**Files:**
- Modify: `crates/sp42-types/src/reviewable.rs` (inline `#[cfg(test)] mod tests`)

Inline tests, not a `tests/` file — the workspace has ~76 inline modules and
exactly one integration test.

**Step 1: The round-trip property ADR-0030 §Consequences calls for.**

```rust
proptest! {
    #[test]
    fn id_round_trips_through_its_encoding(
        kind in kind_strategy(), wiki in "[a-z]{2,12}", local in "[A-Za-z0-9_\\-]{1,40}",
    ) {
        let id = ReviewableItemId::new(kind, wiki, local).expect("valid");
        let s = id.to_encoded();
        prop_assert_eq!(ReviewableItemId::from_encoded(&s).expect("decode"), id);
    }
}
```

Generate a `Custom(String)` kind at least once in the strategy — it is the escape
hatch and the one most likely to break the delimiter logic.

**Step 2: Case tests** for: empty `wiki_id` rejected, empty `local_id` rejected,
`:` in `local_id` rejected, unknown `v` prefix rejected, truncated encoding
rejected, and a `v1` encoding decoding under a future `v` policy that is at least
*detected* rather than silently misread.

**Step 3: Ordering test** — two ids differing only in `wiki_id` order by wiki
first, so the `Ord` derivation is a deliberate total order and not an accident of
declaration order.

**Step 4: Commit** — `test(types): pin ReviewableItemId encoding round-trip and ordering`

---

## Task 3: Resolution surface

**Files:**
- Create: `crates/sp42-platform/src/reviewable_item.rs`

**Step 1: The pure half.** Per ADR-0030 §4, resolution is a read that can fail and
is never an identity mutation. Start with the parts that need no I/O:

```rust
pub struct ResolvedSubject { pub kind: ReviewableItemKind, pub wiki_id: String,
                            pub locator: SubjectLocator, pub title: String }
```

and a pure `fn describe(item_id: &ReviewableItemId) -> String` for logging/audit
that never touches the network.

**Step 2: The async half takes an injected client, not a crate dependency.**
CONSTITUTION §2.3 forbids `sp42-platform` depending on any I/O crate, and today it
has no `tokio` and no `reqwest`. ADR-0026 §5 says discovery goes "through the
already-hardened `sp42-fetch` edge" — that is satisfied by the *shell* injecting
it, because `GuardedHttpClient` already implements `sp42_types::HttpClient`
(`sp42-fetch/src/client.rs:188`) and `sp42-platform` already takes that trait
(`sp42-platform/src/traits.rs:5`, used as `where C: HttpClient + ?Sized` in
`wiki_storage.rs:427`). Follow that shape exactly. Do **not** add `sp42-fetch` to
`sp42-platform/Cargo.toml` — it drags `reqwest` + `tokio` in and breaks §2.3, and
it would also count against the wasm size ceiling.

**Step 3: Commit** — `feat(platform): reviewable item resolution surface`

---

## Phase 1 exit criteria

- [ ] `proptest` round-trip green, including a `Custom` kind
- [ ] Rejection cases covered: empty fields, delimiter in `local_id`, unknown version
- [ ] `grep -n "tokio\|reqwest" crates/sp42-platform/Cargo.toml` still returns nothing
- [ ] `cargo clippy -- -D warnings` clean; no `unwrap()` in non-test code
- [ ] `./scripts/generate-architecture-map.sh --check` green
