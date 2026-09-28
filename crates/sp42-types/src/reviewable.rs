//! Reviewable-item identity — ADR-0030.
//!
//! One opaque, globally unique, revision-independent identity for the *subject*
//! under review. It names a page, draft, nomination, or investigation case; it
//! does **not** name a particular revision, a review lane, or a state.
//!
//! Three properties are load-bearing and each is forced by a decision made
//! elsewhere in the stack:
//!
//! - **Revision-independent.** A verdict recorded about a page stays that same
//!   item's history after the page is edited. ADR-0027 §4's recency-ordered
//!   query and ADR-0028 §2's G4 chaining are both questions about a subject
//!   across time.
//! - **Globally unique across wikis.** `wiki_id` is a component of the value, so
//!   a record written on one wiki is unambiguous when read on another and no
//!   lookup has to guess a project first.
//! - **Lane-agnostic.** `NPP`, `AfD`, and `CCI` are legitimately concurrent on one
//!   page, so the lane dimension lives in ADR-0027's per-track `states` map. An
//!   identity that encoded a lane would mint three identities for one subject.

use serde::{Deserialize, Serialize};

/// The subject vocabulary. Closed core, open extension.
///
/// `Custom(String)` keeps a new lane from requiring a platform-type edit, the
/// same typed-but-open shape ADR-0026 §4 chose for `IntakeField` and ADR-0027 §1
/// chose for `LifecycleState.key`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewableItemKind {
    /// A wiki page, in any namespace.
    Page,
    /// A draft.
    Draft,
    /// A nomination (e.g. `AfC`).
    Nomination,
    /// An investigation case (e.g. `CCI`).
    InvestigationCase,
    /// A subject kind this platform type does not know about.
    ///
    /// The escape hatch, and the reason a domain adding a lane does not have to
    /// edit a shared type.
    Custom(String),
}

impl ReviewableItemKind {
    /// The stable wire spelling of this kind, used inside the encoded id form.
    #[must_use]
    pub fn as_wire_str(&self) -> &str {
        match self {
            Self::Page => "page",
            Self::Draft => "draft",
            Self::Nomination => "nomination",
            Self::InvestigationCase => "investigation_case",
            Self::Custom(name) => name.as_str(),
        }
    }
}

/// Errors from constructing or decoding a [`ReviewableItemId`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReviewableItemIdError {
    /// A wiki id is required.
    #[error("reviewable item id requires a non-empty wiki_id")]
    EmptyWikiId,
    /// A local id is required.
    #[error("reviewable item id requires a non-empty local_id")]
    EmptyLocalId,
    /// `:` separates fields in the encoded form, so it cannot appear in a value.
    #[error("reviewable item local_id must not contain ':' (got {0:?})")]
    ReservedDelimiterInLocalId(String),
    /// A `Custom` kind must name something.
    #[error("reviewable item kind requires a non-empty name")]
    EmptyCustomKind,
    /// The encoded form was not recognized.
    #[error("unrecognised reviewable item id: {0:?}")]
    Decode(String),
    /// The encoded form names a version this build cannot read.
    #[error(
        "unsupported reviewable item id version in {0:?} (this build reads {REVIEWABLE_ITEM_ID_VERSION})"
    )]
    UnsupportedVersion(String),
}

/// The encoding version this build writes.
///
/// Bumped when the wire form changes; decoding refuses an unknown version rather
/// than guessing (CONSTITUTION §9.2).
pub const REVIEWABLE_ITEM_ID_VERSION: &str = "v1";

const ENCODED_PREFIX: &str = "reviewable:v1:";
const FIELD_SEPARATOR: char = ':';

/// An opaque, globally unique, revision-independent subject identity.
///
/// Opaque: callers compare and store it, never pattern-match its internals to
/// infer meaning. The fields are private for that reason — see ADR-0030 §2, where
/// `kind` is explicitly descriptive rather than load-bearing, and a rule that
/// needed to know the kind must go through a domain-owned field instead.
///
/// The `Ord` derivation is deliberate: the lifecycle store and ADR-0027 §4's
/// recency query need a total order, and CONSTITUTION §1.4 requires ordering to be
/// explicit rather than an accident of declaration order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReviewableItemId {
    kind: ReviewableItemKind,
    wiki_id: String,
    local_id: String,
}

impl ReviewableItemId {
    /// Mint an identity for a subject.
    ///
    /// Minting is an admission-time concern (ADR-0030 §3) and idempotent — this
    /// constructor is pure and has no notion of "already exists"; the store that
    /// mints is responsible for resolving a repeat to the same id.
    ///
    /// # Errors
    ///
    /// Returns [`ReviewableItemIdError`] if `wiki_id` or `local_id` is empty, if
    /// `local_id` contains the field separator, or if a [`ReviewableItemKind::Custom`]
    /// carries an empty name.
    pub fn new(
        kind: ReviewableItemKind,
        wiki_id: impl Into<String>,
        local_id: impl Into<String>,
    ) -> Result<Self, ReviewableItemIdError> {
        let kind = match kind {
            ReviewableItemKind::Custom(name) if name.is_empty() => {
                return Err(ReviewableItemIdError::EmptyCustomKind);
            }
            other => other,
        };
        let wiki_id = wiki_id.into();
        if wiki_id.is_empty() {
            return Err(ReviewableItemIdError::EmptyWikiId);
        }
        let local_id = local_id.into();
        if local_id.is_empty() {
            return Err(ReviewableItemIdError::EmptyLocalId);
        }
        if local_id.contains(FIELD_SEPARATOR) {
            return Err(ReviewableItemIdError::ReservedDelimiterInLocalId(local_id));
        }
        Ok(Self {
            kind,
            wiki_id,
            local_id,
        })
    }

    /// Convenience constructor for the most common kind.
    ///
    /// # Errors
    ///
    /// Returns [`ReviewableItemIdError::EmptyWikiId`] when `wiki_id` is empty.
    pub fn page(wiki_id: impl Into<String>, page_id: i64) -> Result<Self, ReviewableItemIdError> {
        Self::new(ReviewableItemKind::Page, wiki_id, page_id.to_string())
    }

    /// The subject kind. Descriptive only — see the type docs.
    #[must_use]
    pub fn kind(&self) -> &ReviewableItemKind {
        &self.kind
    }

    /// The wiki this subject lives on.
    #[must_use]
    pub fn wiki_id(&self) -> &str {
        &self.wiki_id
    }

    /// The kind-local identifier: a page id for `Page`, and so on.
    #[must_use]
    pub fn local_id(&self) -> &str {
        &self.local_id
    }

    /// The versioned, delimiter-joined wire form.
    ///
    /// `reviewable:v1:<kind>:<wiki_id>:<local_id>`
    ///
    /// Single-string rather than nested so it round-trips through `FileStorage`'s
    /// hex-encoded key names and through `Storage` keys generally, and so a
    /// stored id stays readable to whoever inspects the store.
    #[must_use]
    pub fn to_encoded(&self) -> String {
        format!(
            "{ENCODED_PREFIX}{}{FIELD_SEPARATOR}{}{FIELD_SEPARATOR}{}",
            self.kind.as_wire_str(),
            self.wiki_id,
            self.local_id
        )
    }

    /// Parse the wire form. Refuses an unknown version rather than guessing.
    ///
    /// # Errors
    ///
    /// Returns [`ReviewableItemIdError::UnsupportedVersion`] when the encoded
    /// version is not the one this build reads, and
    /// [`ReviewableItemIdError::Decode`] when the layout is malformed or a
    /// field fails the same validation [`Self::new`] applies.
    pub fn from_encoded(encoded: &str) -> Result<Self, ReviewableItemIdError> {
        let decode_err = || ReviewableItemIdError::Decode(encoded.to_string());

        let after_namespace = encoded.strip_prefix("reviewable:").ok_or_else(decode_err)?;

        // A version this build does not know is its own failure: guessing at the
        // layout would silently misread a foreign id, which is worse than
        // refusing it (CONSTITUTION §9.2).
        if !after_namespace.starts_with(REVIEWABLE_ITEM_ID_VERSION) {
            return Err(ReviewableItemIdError::UnsupportedVersion(
                encoded.to_string(),
            ));
        }
        let rest = after_namespace
            .strip_prefix(REVIEWABLE_ITEM_ID_VERSION)
            .and_then(|tail| tail.strip_prefix(':'))
            .ok_or_else(decode_err)?;

        let mut parts = rest.split(FIELD_SEPARATOR);
        let (Some(kind), Some(wiki_id), Some(local_id), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(decode_err());
        };

        let kind = match kind {
            "page" => ReviewableItemKind::Page,
            "draft" => ReviewableItemKind::Draft,
            "nomination" => ReviewableItemKind::Nomination,
            "investigation_case" => ReviewableItemKind::InvestigationCase,
            "" => return Err(decode_err()),
            other => ReviewableItemKind::Custom(other.to_string()),
        };

        Self::new(kind, wiki_id, local_id)
    }

    /// A short human-readable description, for logs and audit rows.
    ///
    /// Pure and clock-free (CONSTITUTION §1.4): identity is the only input.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{}/{}", self.wiki_id, self.kind.as_wire_str())
    }
}

impl std::fmt::Display for ReviewableItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_encoded())
    }
}

impl std::str::FromStr for ReviewableItemId {
    type Err = ReviewableItemIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_encoded(s)
    }
}

impl Serialize for ReviewableItemId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_encoded())
    }
}

impl<'de> Deserialize<'de> for ReviewableItemId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::from_encoded(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{ReviewableItemId, ReviewableItemIdError, ReviewableItemKind};

    fn page() -> ReviewableItemId {
        ReviewableItemId::page("frwiki", 4242).expect("valid page id")
    }

    #[test]
    fn encodes_the_documented_wire_form() {
        assert_eq!(
            page().to_encoded(),
            "reviewable:v1:page:frwiki:4242".to_string()
        );
    }

    #[test]
    fn round_trips_through_the_wire_form() {
        let id = page();
        assert_eq!(
            ReviewableItemId::from_encoded(&id.to_encoded()).expect("decodes"),
            id
        );
    }

    #[test]
    fn round_trips_a_custom_kind() {
        let id = ReviewableItemId::new(
            ReviewableItemKind::Custom("book_workshop".to_string()),
            "enwiki",
            "w-77",
        )
        .expect("valid");
        assert_eq!(
            ReviewableItemId::from_encoded(&id.to_encoded()).expect("decodes"),
            id,
            "the escape hatch must survive the delimiter logic"
        );
    }

    #[test]
    fn rejects_empty_fields() {
        assert_eq!(
            ReviewableItemId::new(ReviewableItemKind::Page, "", "1"),
            Err(ReviewableItemIdError::EmptyWikiId)
        );
        assert_eq!(
            ReviewableItemId::new(ReviewableItemKind::Page, "frwiki", ""),
            Err(ReviewableItemIdError::EmptyLocalId)
        );
        assert_eq!(
            ReviewableItemId::new(ReviewableItemKind::Custom(String::new()), "frwiki", "1"),
            Err(ReviewableItemIdError::EmptyCustomKind)
        );
    }

    #[test]
    fn rejects_the_field_delimiter_in_local_id() {
        // ':' separates fields, so allowing it would make the encoding ambiguous
        // and the round-trip test above would be passing by luck.
        assert_eq!(
            ReviewableItemId::new(ReviewableItemKind::Page, "frwiki", "a:b"),
            Err(ReviewableItemIdError::ReservedDelimiterInLocalId(
                "a:b".to_string()
            ))
        );
    }

    #[test]
    fn rejects_malformed_encodings() {
        for bad in [
            "",
            "page:frwiki:1",
            "reviewable:v1:page:frwiki",
            "reviewable:v1:page:frwiki:1:2",
            "reviewable:v2:page:frwiki:1",
            "reviewable:page:frwiki:1",
        ] {
            assert!(
                ReviewableItemId::from_encoded(bad).is_err(),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn ordering_is_by_kind_then_wiki_then_local() {
        // A deliberate total order, not an accident of declaration order:
        // CONSTITUTION §1.4 wants ordering to be explicit.
        let a = ReviewableItemId::new(ReviewableItemKind::Page, "dewiki", "1").expect("valid");
        let b = ReviewableItemId::new(ReviewableItemKind::Page, "frwiki", "1").expect("valid");
        let c = ReviewableItemId::new(ReviewableItemKind::Page, "frwiki", "2").expect("valid");
        let mut ids = vec![c.clone(), b.clone(), a.clone()];
        ids.sort();
        assert_eq!(ids, vec![a, b, c]);
    }

    #[test]
    fn serializes_as_a_readable_string() {
        let json = serde_json::to_string(&page()).expect("serializes");
        assert_eq!(json, "\"reviewable:v1:page:frwiki:4242\"");
        let back: ReviewableItemId = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, page());
    }

    #[test]
    fn display_and_from_str_agree_with_the_encoding() {
        let id = page();
        assert_eq!(id.to_string(), id.to_encoded());
        assert_eq!(
            id.to_string().parse::<ReviewableItemId>().expect("parses"),
            id
        );
    }

    #[test]
    fn describe_is_clock_free_and_carries_wiki_and_kind() {
        assert_eq!(page().describe(), "frwiki/page");
    }

    /// A kind strategy that exercises the `Custom` escape hatch hard, since that
    /// is the variant most likely to break the delimiter logic.
    fn kind_strategy() -> impl Strategy<Value = ReviewableItemKind> {
        prop_oneof![
            3 => Just(ReviewableItemKind::Page),
            2 => Just(ReviewableItemKind::Draft),
            2 => Just(ReviewableItemKind::Nomination),
            2 => Just(ReviewableItemKind::InvestigationCase),
            3 => "[a-z_]{1,12}".prop_map(ReviewableItemKind::Custom),
        ]
    }

    // ADR-0030 §Consequences: "Pinned by: round-trip property tests over the
    // versioned encoding." CONSTITUTION §1.2 asks for property tests on every
    // commit, and its codec rule is "round-trip is identity".
    proptest! {
        #[test]
        fn id_round_trips_through_its_wire_form(
            kind in kind_strategy(),
            wiki in "[a-z]{2,12}",
            local in "[A-Za-z0-9_-]{1,40}",
        ) {
            let id = ReviewableItemId::new(kind, wiki, local).expect("strategy stays valid");
            let encoded = id.to_encoded();
            prop_assert_eq!(
                ReviewableItemId::from_encoded(&encoded).expect("decodes"),
                id.clone(),
            );
            // and through serde, since that is how it actually reaches storage
            let json = serde_json::to_string(&id).expect("serializes");
            let back: ReviewableItemId = serde_json::from_str(&json).expect("deserializes");
            prop_assert_eq!(back, id);
        }

        /// The encoding is injective: two distinct ids never encode alike.
        /// Without this, a round-trip test passes happily while two different
        /// subjects share one identity - which is exactly the failure that
        /// fragments ADR-0027's transition history.
        #[test]
        fn distinct_ids_encode_distinctly(
            a in (kind_strategy(), "[a-z]{2,12}", "[A-Za-z0-9_-]{1,20}"),
            b in (kind_strategy(), "[a-z]{2,12}", "[A-Za-z0-9_-]{1,20}"),
        ) {
            let mk = |(k, w, l)| ReviewableItemId::new(k, w, l).expect("valid");
            let (a, b) = (mk(a), mk(b));
            if a != b {
                prop_assert_ne!(a.to_encoded(), b.to_encoded());
            }
        }
    }
}
