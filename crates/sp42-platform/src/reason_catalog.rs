//! Per-project, per-language reason catalog — ADR-0031 §4.
//!
//! The catalog is where a reason's *wording* lives. It is keyed by the same
//! machine-legible [`ReasonCode`] the verdict carries, which is what lets the
//! verdict stay language-independent while the explanation does not.
//!
//! The load-time rule is the one that matters: a catalog missing an entry for a
//! code the project's rulesets can produce is a **load error**, not a runtime
//! fallback to the bare identifier. A project may not ship a ruleset whose
//! reasons render as machine codes, because that is the failure mode where a
//! reviewer is shown `citation_formatting_complaint` instead of an explanation.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use sp42_types::{Reason, ReasonCode, ReasonError, ReasonParam, ReasonParams};

/// The shipped frwiki/English catalog, embedded at compile time.
///
/// Embedded with `include_str!` so a malformed catalog is a build failure
/// rather than something discovered on a wiki that happens to be configured.
const FRWIKI_EN_CATALOG_YAML: &str = include_str!("../../../configs/reason-catalog/frwiki/en.yaml");

/// Why a catalog could not be loaded, or a reason could not be rendered.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ReasonCatalogError {
    /// The catalog file did not parse.
    #[error("reason catalog is not valid YAML: {message}")]
    Parse { message: String },
    /// The catalog's own metadata is wrong.
    #[error("reason catalog {found:?} does not belong to {expected:?}")]
    WrongCatalog { expected: String, found: String },
    /// A code in the file is not a usable identifier.
    #[error("reason catalog entry {code:?}: {source}")]
    InvalidCode {
        code: String,
        #[source]
        source: ReasonError,
    },
    /// A template references a placeholder that was not declared in `params`.
    #[error(
        "reason catalog entry {code:?} uses placeholder {{{placeholder}}} but does not declare it"
    )]
    UndeclaredPlaceholder { code: String, placeholder: String },
    /// A declared parameter is never used by the template.
    #[error("reason catalog entry {code:?} declares unused parameter {parameter:?}")]
    UnusedParameter { code: String, parameter: String },
    /// **The load-time rule.** A configured ruleset can produce a code this
    /// catalog does not cover, so a reviewer would see the bare identifier.
    #[error("reason catalog for {wiki_id}/{language} is missing {missing:?}")]
    MissingCode {
        wiki_id: String,
        language: String,
        missing: Vec<String>,
    },
    /// A reason's placeholder had no bound parameter at render time.
    #[error("reason {code:?} needs parameter {parameter:?}, which was not supplied")]
    UnboundParameter { code: String, parameter: String },
    /// A bound parameter is not used by the template.
    #[error("reason {code:?} supplied unused parameter {parameter:?}")]
    ExtraneousParameter { code: String, parameter: String },
}

/// One catalog entry: the wording, and the placeholders that wording uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogEntry {
    template: String,
    params: BTreeSet<String>,
}

impl CatalogEntry {
    /// Every `{name}` placeholder in the template, in sorted order.
    ///
    /// A `{` with no matching `}` is ignored rather than treated as opening a
    /// placeholder that runs to the end of the string: a stray brace in prose
    /// should not silently swallow the rest of the template.
    fn template_placeholders(template: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let bytes = template.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'{' {
                index += 1;
                continue;
            }
            let Some(offset) = template[index + 1..].find('}') else {
                break;
            };
            found.insert(template[index + 1..index + 1 + offset].to_string());
            index += 2 + offset;
        }
        found
    }
}

/// The on-disk shape. Private: callers load through [`load_reason_catalog`],
/// which validates.
#[derive(Debug, Deserialize)]
struct CatalogDocument {
    wiki_id: String,
    language: String,
    reasons: BTreeMap<String, CatalogEntryDocument>,
}

#[derive(Debug, Deserialize)]
struct CatalogEntryDocument {
    template: String,
    #[serde(default)]
    params: Vec<String>,
}

/// A loaded, validated catalog for one project and language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonCatalog {
    wiki_id: String,
    language: String,
    entries: BTreeMap<ReasonCode, CatalogEntry>,
}

impl ReasonCatalog {
    /// Load and validate a catalog against the codes a project's rulesets can
    /// actually produce.
    ///
    /// `required_codes` is what makes the load-time rule enforceable: the caller
    /// knows the rulesets' declared vocabularies, so a gap between what a
    /// ruleset can say and what the catalog can say is caught here rather than
    /// by a reviewer on a live wiki.
    ///
    /// # Errors
    ///
    /// Returns [`ReasonCatalogError::Parse`] if the YAML is malformed;
    /// [`ReasonCatalogError::WrongCatalog`] if the file is for another project
    /// or language; [`ReasonCatalogError::InvalidCode`] for a code that is not
    /// lower-case `snake_case`; [`ReasonCatalogError::UndeclaredPlaceholder`] or
    /// [`ReasonCatalogError::UnusedParameter`] when a template and its declared
    /// `params` disagree; and [`ReasonCatalogError::MissingCode`] — the
    /// load-time rule — when a code in `required_codes` has no entry.
    pub fn load(
        yaml: &str,
        expected_wiki_id: &str,
        expected_language: &str,
        required_codes: &BTreeSet<ReasonCode>,
    ) -> Result<Self, ReasonCatalogError> {
        let document: CatalogDocument =
            serde_yaml::from_str(yaml).map_err(|error| ReasonCatalogError::Parse {
                message: error.to_string(),
            })?;

        if document.wiki_id != expected_wiki_id {
            return Err(ReasonCatalogError::WrongCatalog {
                expected: expected_wiki_id.to_string(),
                found: document.wiki_id,
            });
        }
        if document.language != expected_language {
            return Err(ReasonCatalogError::WrongCatalog {
                expected: expected_language.to_string(),
                found: document.language,
            });
        }

        let mut entries: BTreeMap<ReasonCode, CatalogEntry> = BTreeMap::new();
        for (raw_code, raw) in document.reasons {
            ReasonCode::validate_name(&raw_code).map_err(|source| {
                ReasonCatalogError::InvalidCode {
                    code: raw_code.clone(),
                    source,
                }
            })?;
            let code = ReasonCode::declared(raw_code.clone());

            let declared: BTreeSet<String> = raw.params.iter().cloned().collect();
            let used = CatalogEntry::template_placeholders(&raw.template);

            // A placeholder the template uses but does not declare would render
            // as a literal `{name}` to a reviewer; an entry that declares a
            // parameter it never uses is a typo that would hide a missing
            // binding. Both are schema errors, not runtime surprises.
            if let Some(placeholder) = used.difference(&declared).next() {
                return Err(ReasonCatalogError::UndeclaredPlaceholder {
                    code: raw_code,
                    placeholder: placeholder.clone(),
                });
            }
            if let Some(parameter) = declared.difference(&used).next() {
                return Err(ReasonCatalogError::UnusedParameter {
                    code: raw_code,
                    parameter: parameter.clone(),
                });
            }

            entries.insert(
                code,
                CatalogEntry {
                    template: raw.template,
                    params: used,
                },
            );
        }

        let missing: Vec<String> = required_codes
            .iter()
            .filter(|code| !entries.contains_key(*code))
            .map(|code| code.as_str().to_string())
            .collect();
        if !missing.is_empty() {
            return Err(ReasonCatalogError::MissingCode {
                wiki_id: expected_wiki_id.to_string(),
                language: expected_language.to_string(),
                missing,
            });
        }

        Ok(Self {
            wiki_id: document.wiki_id,
            language: document.language,
            entries,
        })
    }

    /// The project this catalog is for.
    #[must_use]
    pub fn wiki_id(&self) -> &str {
        &self.wiki_id
    }

    /// The language this catalog is for.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Every code this catalog can render.
    pub fn codes(&self) -> impl Iterator<Item = &ReasonCode> {
        self.entries.keys()
    }

    /// Whether this catalog can render `code`.
    #[must_use]
    pub fn covers(&self, code: &ReasonCode) -> bool {
        self.entries.contains_key(code)
    }

    /// Render a reason's text.
    ///
    /// Pure: no clock, no I/O, no locale lookup beyond this catalog. A missing
    /// binding is an error rather than an empty string, so a half-rendered
    /// sentence can never reach an operator.
    ///
    /// # Errors
    ///
    /// Returns [`ReasonCatalogError::MissingCode`] when the catalog has no entry
    /// for the reason's code, [`ReasonCatalogError::UnboundParameter`] when a
    /// placeholder the template needs has no bound value, and
    /// [`ReasonCatalogError::ExtraneousParameter`] when a bound value the
    /// template never uses is supplied.
    pub fn render(&self, reason: &Reason) -> Result<String, ReasonCatalogError> {
        let entry =
            self.entries
                .get(&reason.code)
                .ok_or_else(|| ReasonCatalogError::MissingCode {
                    wiki_id: self.wiki_id.clone(),
                    language: self.language.clone(),
                    missing: vec![reason.code.as_str().to_string()],
                })?;

        for parameter in &entry.params {
            if reason.params.get(parameter).is_none() {
                return Err(ReasonCatalogError::UnboundParameter {
                    code: reason.code.as_str().to_string(),
                    parameter: parameter.clone(),
                });
            }
        }
        for (parameter, _) in reason.params.iter() {
            if !entry.params.contains(parameter) {
                return Err(ReasonCatalogError::ExtraneousParameter {
                    code: reason.code.as_str().to_string(),
                    parameter: parameter.to_string(),
                });
            }
        }

        let mut out = entry.template.clone();
        for (parameter, value) in reason.params.iter() {
            out = out.replace(
                &ReasonParam::placeholder(parameter),
                &value.diagnostic_value(),
            );
        }
        Ok(out)
    }
}

/// The shipped frwiki/English catalog, validated against `required_codes`.
///
/// # Errors
///
/// Propagates every [`ReasonCatalog::load`] failure, including
/// [`ReasonCatalogError::MissingCode`] when a code a ruleset declares has no
/// entry - so a project cannot ship a ruleset whose reasons would render as bare
/// machine identifiers.
pub fn load_frwiki_english(
    required_codes: &BTreeSet<ReasonCode>,
) -> Result<ReasonCatalog, ReasonCatalogError> {
    ReasonCatalog::load(FRWIKI_EN_CATALOG_YAML, "frwiki", "en", required_codes)
}

/// Helper for a ruleset assembling the codes its arms declare.
#[must_use]
pub fn required_codes<I, S>(codes: I) -> BTreeSet<ReasonCode>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    codes
        .into_iter()
        .map(|code| ReasonCode::declared(code.as_ref().to_string()))
        .collect()
}

/// Convenience for building a reason's parameters from pairs.
#[must_use]
pub fn params_from(pairs: &[(&str, ReasonParam)]) -> ReasonParams {
    pairs
        .iter()
        .fold(ReasonParams::empty(), |acc, (name, value)| {
            acc.with(*name, value.clone())
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        ReasonCatalog, ReasonCatalogError, load_frwiki_english, params_from, required_codes,
    };
    use sp42_types::{Reason, ReasonCode, ReasonParam};

    fn code(name: &str) -> ReasonCode {
        ReasonCode::declared(name)
    }

    /// The codes the shipped catalog is expected to cover, and the codes a
    /// ruleset would declare. Kept adjacent so the two cannot drift apart
    /// without this file changing.
    fn frwiki_declared_codes() -> std::collections::BTreeSet<ReasonCode> {
        required_codes([
            "insufficient_sources",
            "no_outline",
            "original_research",
            "promotional_language",
            "csd_g4_recreated",
            "redirect_to_non_mainspace",
            "redirect_never_linked",
            "merge_consensus",
            "no_consensus",
            "off_topic",
            "citation_formatting_complaint",
            "unverifiable_source",
            "ga_checklist_incomplete",
            "fa_not_yet_ga",
            "dyk_failed_length",
        ])
    }

    #[test]
    fn the_shipped_catalog_covers_what_a_ruleset_declares() {
        let catalog =
            load_frwiki_english(&frwiki_declared_codes()).expect("shipped catalog is complete");
        assert_eq!(catalog.wiki_id(), "frwiki");
        assert_eq!(catalog.language(), "en");
        for declared in frwiki_declared_codes() {
            assert!(
                catalog.covers(&declared),
                "{} declared by a ruleset but absent from the catalog",
                declared.as_str()
            );
        }
    }

    #[test]
    fn a_code_with_no_catalog_entry_is_a_load_error_not_a_fallback() {
        // The load-time rule, and the whole point of ADR-0031 §4. A reviewer must
        // never be shown a bare machine identifier.
        let required = required_codes(["insufficient_sources", "a_code_nobody_wrote_text_for"]);
        let error = load_frwiki_english(&required).expect_err("must not load");
        assert!(
            matches!(&error, ReasonCatalogError::MissingCode { missing, .. }
                if missing == &vec!["a_code_nobody_wrote_text_for".to_string()]),
            "expected MissingCode naming the gap, got {error:?}"
        );
    }

    #[test]
    fn coverage_is_enforced_per_language() {
        // The same declared vocabulary, asked for in a language that has no
        // catalog, must fail rather than fall back to English. This is the
        // property that makes `disallowed_reasons` language-independent.
        let yaml = "wiki_id: frwiki\nlanguage: xx\nreasons: {}\n";
        let error = ReasonCatalog::load(yaml, "frwiki", "xx", &frwiki_declared_codes())
            .expect_err("an untranslated language must not load");
        match error {
            ReasonCatalogError::MissingCode { missing, .. } => {
                assert_eq!(missing.len(), frwiki_declared_codes().len());
            }
            other => panic!("expected MissingCode, got {other:?}"),
        }
    }

    #[test]
    fn a_catalog_for_another_project_or_language_is_rejected() {
        let yaml = "wiki_id: dewiki\nlanguage: de\nreasons: {}\n";
        let error = ReasonCatalog::load(yaml, "frwiki", "de", &BTreeSet::new())
            .expect_err("wrong project must be rejected");
        assert!(matches!(error, ReasonCatalogError::WrongCatalog { .. }));

        let error = ReasonCatalog::load(yaml, "dewiki", "en", &BTreeSet::new())
            .expect_err("wrong language must be rejected");
        assert!(matches!(error, ReasonCatalogError::WrongCatalog { .. }));
    }

    #[test]
    fn a_template_using_an_undeclared_placeholder_is_a_schema_error() {
        let yaml = concat!(
            "wiki_id: frwiki\nlanguage: en\nreasons:\n",
            "  broken:\n",
            "    template: \"saw {count} sources\"\n",
            "    params: []\n",
        );
        let error = ReasonCatalog::load(yaml, "frwiki", "en", &BTreeSet::new())
            .expect_err("undeclared placeholder must be rejected");
        assert!(
            matches!(
                error,
                ReasonCatalogError::UndeclaredPlaceholder { ref placeholder, .. }
                    if placeholder == "count"
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_declared_but_unused_parameter_is_a_schema_error() {
        let yaml = concat!(
            "wiki_id: frwiki\nlanguage: en\nreasons:\n",
            "  broken:\n",
            "    template: \"no placeholders here\"\n",
            "    params: [count]\n",
        );
        let error = ReasonCatalog::load(yaml, "frwiki", "en", &BTreeSet::new())
            .expect_err("unused declaration must be rejected");
        assert!(
            matches!(
                error,
                ReasonCatalogError::UnusedParameter { ref parameter, .. } if parameter == "count"
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn rendering_substitutes_every_declared_parameter() {
        let catalog = load_frwiki_english(&frwiki_declared_codes()).expect("loads");
        let reason = Reason::with_params(
            code("insufficient_sources"),
            params_from(&[
                ("count", ReasonParam::Count(2)),
                ("threshold", ReasonParam::Count(3)),
            ]),
        );
        let rendered = catalog.render(&reason).expect("renders");
        assert_eq!(
            rendered,
            "The article has too few independent sources (2 cited, 3 required)."
        );
    }

    #[test]
    fn rendering_is_total_for_a_parameterless_reason() {
        let catalog = load_frwiki_english(&frwiki_declared_codes()).expect("loads");
        let rendered = catalog
            .render(&Reason::new(code("no_consensus")))
            .expect("renders");
        assert!(!rendered.is_empty());
        assert!(
            !rendered.contains('{'),
            "no placeholder may survive: {rendered}"
        );
    }

    #[test]
    fn a_missing_binding_is_an_error_rather_than_a_blank() {
        let catalog = load_frwiki_english(&frwiki_declared_codes()).expect("loads");
        // `insufficient_sources` declares count and threshold; supply one.
        let reason = Reason::with_params(
            code("insufficient_sources"),
            params_from(&[("count", ReasonParam::Count(2))]),
        );
        let error = catalog.render(&reason).expect_err("must not half-render");
        assert!(
            matches!(
                error,
                ReasonCatalogError::UnboundParameter { ref parameter, .. }
                    if parameter == "threshold"
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn an_unused_supplied_parameter_is_an_error() {
        let catalog = load_frwiki_english(&frwiki_declared_codes()).expect("loads");
        let reason = Reason::with_params(
            code("no_consensus"),
            params_from(&[("stray", ReasonParam::Text("x".into()))]),
        );
        let error = catalog.render(&reason).expect_err("must reject");
        assert!(matches!(
            error,
            ReasonCatalogError::ExtraneousParameter { ref parameter, .. } if parameter == "stray"
        ));
    }

    #[test]
    fn the_disallowed_example_is_present_so_the_blocklist_is_demonstrable() {
        // AFCSTANDARDS forbids citation-formatting complaints as a decline
        // reason. The code has to exist for a ruleset to block it.
        let catalog = load_frwiki_english(&frwiki_declared_codes()).expect("loads");
        assert!(catalog.covers(&code("citation_formatting_complaint")));
    }

    #[test]
    fn invalid_yaml_is_reported_as_a_parse_error() {
        let error = ReasonCatalog::load("not: [valid", "frwiki", "en", &BTreeSet::new())
            .expect_err("must not parse");
        assert!(
            matches!(error, ReasonCatalogError::Parse { .. }),
            "got {error:?}"
        );
    }
}
