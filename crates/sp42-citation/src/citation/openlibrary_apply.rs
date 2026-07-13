//! The Open Library apply mechanism (ADR-0025). **The write lane is
//! disabled**: no server route or CLI surface calls [`execute_enrichment_apply`]
//! until ADR-0025's enablement gate passes (live form spike, upstream
//! courtesy contact, capability report) — until then this module is
//! mechanism + fixture tests only, and [`ENRICHMENT_WRITE_LANE_ENABLED`]
//! is the constant any future surface must check.
//!
//! Mechanics, per the ADR:
//! - **Session** (Decision 2): `POST /account/login` with the operator's own
//!   IA S3 keys → session cookie. Per-operator, never shared.
//! - **Lane selection** (Decision 1): attempt the REST `PUT`; a 403 is
//!   infogami's pre-mutation permission refusal, so the same apply falls
//!   back to the form lane and the discovered lane is cached per session by
//!   the caller. No knob, no membership probe.
//! - **Drift** (Decision 3): re-read the record before writing; a moved
//!   `revision` refuses. The REST→form fallback re-runs the re-read.
//! - **Form adapter** (Decision 4): GET the edit form, replay every field it
//!   carries with only the confirmed field changed, and treat any surprise
//!   as contract drift — refuse, never guess. After a submit, read the
//!   record back and verify exactly the proposed value landed.
//!
//! The form-lane field naming (`edition--{field}--0`, infogami's `--`
//! unflatten convention) is **adapter contract v0**: pinned by synthetic
//! fixtures until the ADR-0025 enablement spike replays the real form and
//! refreshes them.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::citation::enrich::{EnrichmentProposal, OpenLibraryRecord, parse_record};
use crate::types::{HttpMethod, HttpRequest, HttpResponse};
use sp42_types::HttpClient;

/// Decode common HTML entities in form field values. Handles `&amp;`, `&lt;`,
/// `&gt;`, `&quot;`, `&#123;` (decimal), and `&#x1a;` (hex) forms.
fn decode_html_entities(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            let mut entity = String::new();
            while let Some(&next) = chars.peek() {
                if next == ';' {
                    chars.next(); // consume the semicolon
                    break;
                }
                entity.push(chars.next().expect("peek succeeded, next should not fail"));
            }
            match entity.as_str() {
                "amp" => result.push('&'),
                "lt" => result.push('<'),
                "gt" => result.push('>'),
                "quot" => result.push('"'),
                "apos" => result.push('\''),
                other if other.starts_with('#') => {
                    // Decimal: &#123; or hex: &#x1a;
                    let num_str = if other.starts_with("#x") || other.starts_with("#X") {
                        &other[2..]
                    } else {
                        &other[1..]
                    };
                    if let Ok(code) = if other.starts_with("#x") || other.starts_with("#X") {
                        u32::from_str_radix(num_str, 16)
                    } else {
                        num_str.parse::<u32>()
                    } {
                        if let Some(ch) = char::from_u32(code) {
                            result.push(ch);
                        } else {
                            result.push('&');
                            result.push_str(other);
                            result.push(';');
                        }
                    } else {
                        result.push('&');
                        result.push_str(other);
                        result.push(';');
                    }
                }
                _ => {
                    result.push('&');
                    result.push_str(&entity);
                    result.push(';');
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// ADR-0025 Decision 6: the write lane ships disabled. Any future surface
/// that wires [`execute_enrichment_apply`] to an operator action must gate
/// on this constant, which flips only in the PR that records the enablement
/// gate (live form spike + upstream contact + capability report).
pub const ENRICHMENT_WRITE_LANE_ENABLED: bool = false;

/// The Open Library origin every apply-lane request targets.
pub const OPEN_LIBRARY_ORIGIN: &str = "https://openlibrary.org";

/// The origin parsed once, for the (unreachable) fallback when a built URL
/// somehow fails to parse — keeps the builders panic-free.
static ORIGIN_BASE: LazyLock<Url> =
    LazyLock::new(|| OPEN_LIBRARY_ORIGIN.parse().expect("static origin parses"));

/// The operator's Open Library session: the cookie from `/account/login`,
/// held per-operator (ADR-0025 Decision 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenLibrarySession {
    /// The `Cookie:` header value subsequent requests carry.
    pub cookie: String,
}

/// The lane an operator session discovered (ADR-0025 Decision 1). Cached by
/// the caller per session; a stale cache is benign in both directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyLane {
    /// `PUT /books/OL…M.json` — API-usergroup members.
    Rest,
    /// The website edit form — every logged-in account.
    Form,
}

/// Build the (state-changing POST) S3-key login request. The keys are the
/// operator's own, minted at archive.org; SP42 never holds a shared key.
#[must_use]
pub fn build_login_request(access_key: &str, secret_key: &str) -> HttpRequest {
    let url = format!("{OPEN_LIBRARY_ORIGIN}/account/login")
        .parse()
        .unwrap_or_else(|_| ORIGIN_BASE.clone());
    let body = serde_json::json!({ "access": access_key, "secret": secret_key });
    HttpRequest {
        method: HttpMethod::Post,
        url,
        headers: BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        body: body.to_string().into_bytes(),
    }
}

/// Parse a login response into a session: a 2xx with a `set-cookie` header
/// carrying the `session` cookie. `None` = login failed, lane stays
/// proposal-only.
#[must_use]
pub fn parse_login_response(response: &HttpResponse) -> Option<OpenLibrarySession> {
    if !(200..300).contains(&response.status) {
        return None;
    }
    let set_cookie = response.headers.get("set-cookie")?;
    let session_pair = set_cookie
        .split(',')
        .flat_map(|part| part.split(';'))
        .map(str::trim)
        .find(|pair| pair.starts_with("session="))?;
    Some(OpenLibrarySession {
        cookie: session_pair.to_string(),
    })
}

/// Build the (read-only GET) raw-record read: the drift re-read, the propose
/// base, and the post-apply read-back all use this.
#[must_use]
pub fn build_record_request(record_key: &str) -> HttpRequest {
    let url = format!("{OPEN_LIBRARY_ORIGIN}{record_key}.json")
        .parse()
        .unwrap_or_else(|_| ORIGIN_BASE.clone());
    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::new(),
        body: Vec::new(),
    }
}

/// The record document with exactly the proposal's field replaced — the
/// verbatim replay both lanes write (ADR-0025 Decision 3: apply never
/// recomputes the change).
#[must_use]
pub fn apply_proposal_to_record(
    record: &OpenLibraryRecord,
    proposal: &EnrichmentProposal,
) -> Value {
    let mut raw = record.raw.clone();
    if let Value::Object(map) = &mut raw {
        map.insert(proposal.field.clone(), proposal.proposed.clone());
    }
    raw
}

/// Build the REST-lane apply: `PUT {key}.json` of the full updated document
/// plus `_comment`, under the operator's session.
#[must_use]
pub fn build_rest_apply_request(
    session: &OpenLibrarySession,
    record: &OpenLibraryRecord,
    proposal: &EnrichmentProposal,
) -> HttpRequest {
    let mut body = apply_proposal_to_record(record, proposal);
    if let Value::Object(map) = &mut body {
        map.insert(
            "_comment".to_string(),
            Value::String(proposal.comment.clone()),
        );
    }
    let url = format!("{OPEN_LIBRARY_ORIGIN}{key}.json", key = record.key)
        .parse()
        .unwrap_or_else(|_| ORIGIN_BASE.clone());
    HttpRequest {
        method: HttpMethod::Put,
        url,
        headers: BTreeMap::from([
            ("content-type".to_string(), "application/json".to_string()),
            ("cookie".to_string(), session.cookie.clone()),
        ]),
        body: body.to_string().into_bytes(),
    }
}

/// Build the (read-only GET) edit-form fetch for the form lane.
#[must_use]
pub fn build_edit_form_request(session: &OpenLibrarySession, record_key: &str) -> HttpRequest {
    let url = format!("{OPEN_LIBRARY_ORIGIN}{record_key}/edit")
        .parse()
        .unwrap_or_else(|_| ORIGIN_BASE.clone());
    HttpRequest {
        method: HttpMethod::Get,
        url,
        headers: BTreeMap::from([("cookie".to_string(), session.cookie.clone())]),
        body: Vec::new(),
    }
}

/// The parsed edit form: where it posts and every field it carries, in
/// document order. Replaying all of them (with one changed) is what keeps
/// the full-record form save from blanking anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditForm {
    /// The form's `action` path (posted back on the same origin).
    pub action_path: String,
    /// Every field the form carries: `(name, value)`, later entries
    /// overriding earlier ones on submit-encode is NOT assumed — names are
    /// kept in order and posted as-is.
    pub fields: Vec<(String, String)>,
}

static FORM_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<form\b[^>]*\baction="([^"]+)"[^>]*>"#).expect("valid regex")
});
static INPUT_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<(input|textarea|select)\b[^>]*>").expect("valid regex"));
static ATTR_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\bname="([^"]*)""#).expect("valid regex"));
static ATTR_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\bvalue="([^"]*)""#).expect("valid regex"));
static ATTR_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\btype="([^"]*)""#).expect("valid regex"));
static TEXTAREA_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<textarea\b[^>]*\bname="([^"]*)"[^>]*>(.*?)</textarea>"#)
        .expect("valid regex")
});
static SELECT_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<select\b[^>]*\bname="([^"]*)"[^>]*>(.*?)</select>"#).expect("valid regex")
});
static SELECTED_OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<option\b[^>]*\bselected\b[^>]*\bvalue="([^"]*)""#).expect("valid regex")
});
static OPTION_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<option\b[^>]*\bvalue="([^"]*)""#).expect("valid regex"));
static RADIO_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<input\b[^>]*\btype="radio"[^>]*>"#).expect("valid regex"));

/// Parse the edition/work edit form out of the page, fail-closed: `None`
/// for anything that is not recognizably the edit form SP42 knows how to
/// replay — a login redirect body, a redesigned page, an unparseable form,
/// or any widget whose state cannot be faithfully represented.
#[must_use]
pub fn parse_edit_form(html: &str, record_key: &str) -> Option<EditForm> {
    // The form must post back to the record's own /edit path.
    let expected_action = format!("{record_key}/edit");
    let form_open = FORM_OPEN
        .captures_iter(html)
        .find(|caps| caps[1].trim_end_matches('#') == expected_action)?;
    let form_start = form_open.get(0)?.end();
    let form_end = html[form_start..]
        .find("</form>")
        .map(|offset| form_start + offset)?;
    let form_html = &html[form_start..form_end];

    let mut fields: Vec<(String, String)> = Vec::new();

    // Selects first: capture the selected option if one exists, else the
    // first option (which the browser would submit). PR 148 P2: replay
    // browser-default option instead of refusing.
    for caps in SELECT_BLOCK.captures_iter(form_html) {
        let name = caps[1].to_string();
        let options = &caps[2];
        let value = if let Some(selected) = SELECTED_OPTION.captures(options) {
            decode_html_entities(&selected[1])
        } else if let Some(first_option) = OPTION_VALUE.captures(options) {
            // No explicit selection: use the first option (browser default).
            decode_html_entities(&first_option[1])
        } else {
            // No options in the select: contract drift.
            return None;
        };
        fields.push((name, value));
    }

    // Textareas: preserve exact content including leading/trailing whitespace.
    // PR 148 P2: preserve textarea contents when replaying.
    for caps in TEXTAREA_BLOCK.captures_iter(form_html) {
        let content = &caps[2];
        fields.push((caps[1].to_string(), decode_html_entities(content)));
    }

    // Radio button groups: capture the checked radio's value if one is checked.
    // PR 148 P2: handle radio controls before enabling form fallback.
    // We track which radio groups we've seen to handle multiple radios.
    let mut radio_groups_seen = std::collections::HashSet::new();
    for caps in RADIO_TAG.captures_iter(form_html) {
        let tag = caps.get(0).map(|m| m.as_str())?;
        let Some(name) = ATTR_NAME.captures(tag).map(|c| c[1].to_string()) else {
            continue;
        };
        // Only process if this radio is checked and we haven't seen this group yet.
        if tag.to_lowercase().contains("checked") && !radio_groups_seen.contains(&name) {
            radio_groups_seen.insert(name.clone());
            let value = ATTR_VALUE
                .captures(tag)
                .map(|c| decode_html_entities(&c[1]))
                .unwrap_or_default();
            fields.push((name, value));
        }
    }

    // Regular inputs (text, hidden, etc.). Skip submit buttons and handled radios.
    for caps in INPUT_TAG.captures_iter(form_html) {
        if &caps[1].to_lowercase() != "input" {
            continue; // textarea/select handled above
        }
        let tag = caps.get(0).map(|m| m.as_str())?;
        let Some(name) = ATTR_NAME.captures(tag).map(|c| c[1].to_string()) else {
            continue; // nameless inputs are not submitted
        };
        let input_type = ATTR_TYPE
            .captures(tag)
            .map_or_else(|| "text".to_string(), |c| c[1].to_lowercase());
        match input_type.as_str() {
            // Buttons and radios submit nothing unless clicked/selected (handled above).
            "submit" | "button" | "image" | "reset" | "radio" => {}
            // Checkboxes and file inputs are contract drift (fail-closed).
            "checkbox" | "file" => return None,
            _ => {
                let value = ATTR_VALUE
                    .captures(tag)
                    .map(|c| decode_html_entities(&c[1]))
                    .unwrap_or_default();
                // PR 148 P2: decode form values before replaying.
                fields.push((name, value));
            }
        }
    }

    if fields.is_empty() {
        return None;
    }
    Some(EditForm {
        action_path: expected_action,
        fields,
    })
}

/// The form-lane input name for a proposal's field — adapter contract v0
/// (infogami's `--` unflatten convention). Confirmed/adjusted by the ADR-0025
/// enablement spike before any live write.
///
/// PR 148 P2: Open Library's current edition template renders ISBNs through
/// the identifier editor (`edition--identifiers--...`), not the direct field.
#[must_use]
pub fn form_field_name(field: &str) -> String {
    // ISBN fields live in the identifier-list editor, not as direct fields.
    match field {
        "isbn_13" | "isbn_10" => format!("edition--identifiers--{field}"),
        _ => format!("edition--{field}--0"),
    }
}

/// Apply the proposal to a parsed form, fail-closed (ADR-0025 Decision 4):
/// the target field's (empty) slot and the `_comment` slot must both already
/// exist in the form — a form without them is not the form this adapter
/// version knows, and inventing fields risks a mangled public record.
/// Returns the ready-to-post form, or `None` on contract drift.
#[must_use]
pub fn fill_edit_form(form: &EditForm, proposal: &EnrichmentProposal) -> Option<EditForm> {
    let target = form_field_name(&proposal.field);
    let proposed_value = proposal.proposed_form_value()?;
    let mut filled = form.clone();
    let mut target_hit = false;
    let mut comment_hit = false;
    for (name, value) in &mut filled.fields {
        if *name == target {
            // The slot must be an OPEN gap in the form too, mirroring the
            // raw-record check in propose_from_candidate.
            if !value.trim().is_empty() {
                return None;
            }
            value.clone_from(&proposed_value);
            target_hit = true;
        }
        if name == "_comment" {
            value.clone_from(&proposal.comment);
            comment_hit = true;
        }
    }
    (target_hit && comment_hit).then_some(filled)
}

/// Build the form-lane submit: a form-encoded POST of every replayed field,
/// under the operator's session.
#[must_use]
pub fn build_form_submit_request(session: &OpenLibrarySession, form: &EditForm) -> HttpRequest {
    let url = format!("{OPEN_LIBRARY_ORIGIN}{}", form.action_path)
        .parse()
        .unwrap_or_else(|_| ORIGIN_BASE.clone());
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form.fields.iter().map(|(n, v)| (n.as_str(), v.as_str())))
        .finish();
    HttpRequest {
        method: HttpMethod::Post,
        url,
        headers: BTreeMap::from([
            (
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
            ("cookie".to_string(), session.cookie.clone()),
        ]),
        body: body.into_bytes(),
    }
}

/// `true` iff a form submit response has the known success shape: a
/// redirect whose `location` points back at the record. Anything else is
/// treated as a failed apply (fail-closed), pending the enablement spike.
#[must_use]
pub fn form_submit_succeeded(response: &HttpResponse, record_key: &str) -> bool {
    (300..400).contains(&response.status)
        && response
            .headers
            .get("location")
            .is_some_and(|location| location.contains(record_key))
}

/// `true` iff the record now carries exactly the proposal's value in the
/// proposal's field — the post-apply read-back check (ADR-0025 Decision 4).
#[must_use]
pub fn verify_applied(record: &OpenLibraryRecord, proposal: &EnrichmentProposal) -> bool {
    record.raw.get(&proposal.field) == Some(&proposal.proposed)
}

/// The outcome of one apply attempt. Refusals are first-class outcomes, not
/// errors: they are the discipline working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The write landed and the read-back verified it.
    Applied {
        lane: ApplyLane,
        /// The record revision observed by the read-back.
        new_revision: u64,
    },
    /// The record moved since the proposal (ADR-0025 Decision 3): refused,
    /// re-propose against the current record.
    RefusedDrift {
        base_revision: u64,
        current_revision: u64,
    },
    /// The form no longer matches the adapter's contract (ADR-0025
    /// Decision 4): refused; enrichment is proposal-only until the adapter
    /// is updated.
    RefusedContractDrift { detail: String },
    /// A transport or upstream failure, or a read-back that did not show
    /// the proposed value; nothing verified as written.
    Failed { message: String },
}

/// Read + parse the raw record, as a small shared step.
async fn read_record<C>(client: &C, record_key: &str) -> Result<OpenLibraryRecord, String>
where
    C: HttpClient + ?Sized,
{
    let response = client
        .execute(build_record_request(record_key))
        .await
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&response.status) {
        return Err(format!("record read returned {}", response.status));
    }
    parse_record(&response.body).ok_or_else(|| "record read was unparseable".to_string())
}

/// Execute one confirmed apply (ADR-0025 Decisions 1/3/4): drift re-read,
/// REST attempt with 403 fallback to the form lane, per-session lane cache,
/// and post-apply read-back. **Not wired to any operator surface** — see
/// [`ENRICHMENT_WRITE_LANE_ENABLED`].
///
/// PR 148 P2: enforce the disabled write gate at the apply entry point.
pub async fn execute_enrichment_apply<C>(
    client: &C,
    session: &OpenLibrarySession,
    proposal: &EnrichmentProposal,
    lane_cache: &mut Option<ApplyLane>,
) -> ApplyOutcome
where
    C: HttpClient + ?Sized,
{
    // Enforce the disabled-gate guarantee: if the constant is false, no Open
    // Library write can be issued, not even advisory to future callers.
    if !ENRICHMENT_WRITE_LANE_ENABLED {
        return ApplyOutcome::Failed {
            message: "enrichment write lane is disabled (ADR-0025 Decision 6)".to_string(),
        };
    }

    execute_enrichment_apply_unchecked(client, session, proposal, lane_cache).await
}

/// Internal apply helper without the disabled-gate check. Used by the public
/// function and fixture tests only (see ADR-0025 Decision 6).
async fn execute_enrichment_apply_unchecked<C>(
    client: &C,
    session: &OpenLibrarySession,
    proposal: &EnrichmentProposal,
    lane_cache: &mut Option<ApplyLane>,
) -> ApplyOutcome
where
    C: HttpClient + ?Sized,
{
    // Drift re-read (Decision 3).
    let record = match read_record(client, &proposal.record_key).await {
        Ok(record) => record,
        Err(message) => return ApplyOutcome::Failed { message },
    };
    if record.revision != proposal.base_revision {
        return ApplyOutcome::RefusedDrift {
            base_revision: proposal.base_revision,
            current_revision: record.revision,
        };
    }

    // REST lane first, unless the session already discovered the form lane.
    let mut fell_back = false;
    if *lane_cache != Some(ApplyLane::Form) {
        let response = match client
            .execute(build_rest_apply_request(session, &record, proposal))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return ApplyOutcome::Failed {
                    message: error.to_string(),
                };
            }
        };
        match response.status {
            // Pre-mutation permission refusal: the capability answer.
            // Fall through to the form lane and cache the discovery.
            403 => {
                *lane_cache = Some(ApplyLane::Form);
                fell_back = true;
            }
            status if (200..300).contains(&status) => {
                *lane_cache = Some(ApplyLane::Rest);
                return read_back(client, proposal, ApplyLane::Rest).await;
            }
            status => {
                return ApplyOutcome::Failed {
                    message: format!("REST apply returned {status}"),
                };
            }
        }
    }

    // Form lane. A REST->form fallback restarts the drift window
    // (Decision 3); a cached form lane already holds the fresh read above.
    if fell_back {
        let record = match read_record(client, &proposal.record_key).await {
            Ok(record) => record,
            Err(message) => return ApplyOutcome::Failed { message },
        };
        if record.revision != proposal.base_revision {
            return ApplyOutcome::RefusedDrift {
                base_revision: proposal.base_revision,
                current_revision: record.revision,
            };
        }
    }

    let form_response = match client
        .execute(build_edit_form_request(session, &proposal.record_key))
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return ApplyOutcome::Failed {
                message: error.to_string(),
            };
        }
    };
    if !(200..300).contains(&form_response.status) {
        return ApplyOutcome::Failed {
            message: format!("edit form fetch returned {}", form_response.status),
        };
    }
    let html = String::from_utf8_lossy(&form_response.body);
    let Some(form) = parse_edit_form(&html, &proposal.record_key) else {
        return ApplyOutcome::RefusedContractDrift {
            detail: "edit form was not recognizable".to_string(),
        };
    };
    let Some(filled) = fill_edit_form(&form, proposal) else {
        return ApplyOutcome::RefusedContractDrift {
            detail: "edit form lacked the expected field or comment slot".to_string(),
        };
    };
    let submit = match client
        .execute(build_form_submit_request(session, &filled))
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return ApplyOutcome::Failed {
                message: error.to_string(),
            };
        }
    };
    if !form_submit_succeeded(&submit, &proposal.record_key) {
        return ApplyOutcome::Failed {
            message: format!(
                "form submit did not have the success shape ({})",
                submit.status
            ),
        };
    }
    read_back(client, proposal, ApplyLane::Form).await
}

/// The post-apply read-back (ADR-0025 Decision 4): verify the proposed value
/// actually landed before reporting success.
async fn read_back<C>(client: &C, proposal: &EnrichmentProposal, lane: ApplyLane) -> ApplyOutcome
where
    C: HttpClient + ?Sized,
{
    match read_record(client, &proposal.record_key).await {
        Ok(record) if verify_applied(&record, proposal) => ApplyOutcome::Applied {
            lane,
            new_revision: record.revision,
        },
        Ok(record) => ApplyOutcome::Failed {
            message: format!(
                "read-back at revision {} does not show the proposed value; \
                 possible adapter defect — inspect the record history",
                record.revision
            ),
        },
        Err(message) => ApplyOutcome::Failed {
            message: format!("read-back failed: {message}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApplyLane, ApplyOutcome, ENRICHMENT_WRITE_LANE_ENABLED, EditForm, OpenLibrarySession,
        build_form_submit_request, build_login_request, build_record_request,
        build_rest_apply_request, execute_enrichment_apply, execute_enrichment_apply_unchecked,
        fill_edit_form, form_field_name, form_submit_succeeded, parse_edit_form,
        parse_login_response, verify_applied,
    };
    use crate::citation::enrich::{EnrichmentProposal, parse_record};
    use crate::types::{HttpMethod, HttpResponse};
    use futures::executor::block_on;
    use serde_json::json;
    use sp42_types::StubHttpClient;
    use std::collections::BTreeMap;

    fn session() -> OpenLibrarySession {
        OpenLibrarySession {
            cookie: "session=/people/tester%2C2026-07-08T00%3A00%3A00%2C1234".to_string(),
        }
    }

    fn proposal() -> EnrichmentProposal {
        EnrichmentProposal {
            record_key: "/books/OL7826547M".to_string(),
            base_revision: 7,
            field: "isbn_13".to_string(),
            current: None,
            proposed: json!(["9780140328721"]),
            source: "derived from ISBN-10 0140328726 (record)".to_string(),
            comment: "Add isbn_13 9780140328721 — SP42 assisted edit".to_string(),
        }
    }

    const RECORD_REV7: &str = r#"{"key": "/books/OL7826547M", "revision": 7, "title": "Matilda", "isbn_10": ["0140328726"]}"#;
    const RECORD_REV8_APPLIED: &str = r#"{"key": "/books/OL7826547M", "revision": 8, "title": "Matilda", "isbn_10": ["0140328726"], "isbn_13": ["9780140328721"]}"#;

    const EDIT_FORM_HTML: &str = r#"<html><body>
<form method="post" action="/books/OL7826547M/edit" id="addbook">
<input type="hidden" name="v" value="7"/>
<input type="text" name="edition--title--0" value="Matilda"/>
<input type="text" name="edition--identifiers--isbn_10" value="0140328726"/>
<input type="text" name="edition--identifiers--isbn_13" value=""/>
<textarea name="edition--notes--0"></textarea>
<input type="text" name="_comment" value=""/>
<input type="submit" value="Save"/>
</form></body></html>"#;

    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: BTreeMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn redirect(location: &str) -> HttpResponse {
        HttpResponse {
            status: 303,
            headers: BTreeMap::from([("location".to_string(), location.to_string())]),
            body: Vec::new(),
        }
    }

    #[test]
    fn write_lane_ships_disabled() {
        // ADR-0025 Decision 6: flipped only by the enablement-gate PR.
        // A constant on purpose: this test is the tripwire that makes
        // flipping the gate a visible, reviewed act. (Read through a binding
        // so the tripwire needs no assertions_on_constants allow.)
        let write_lane_enabled = ENRICHMENT_WRITE_LANE_ENABLED;
        assert!(
            !write_lane_enabled,
            "the enrichment write lane must stay disabled until the ADR-0025 enablement gate passes"
        );
    }

    #[test]
    fn login_request_and_session_parse() {
        let request = build_login_request("AK", "SK");
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.url.as_str(),
            "https://openlibrary.org/account/login"
        );
        let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
        assert_eq!(body["access"], "AK");
        assert_eq!(body["secret"], "SK");

        let ok = HttpResponse {
            status: 200,
            headers: BTreeMap::from([(
                "set-cookie".to_string(),
                "session=/people/tester%2C2026; Path=/; HttpOnly".to_string(),
            )]),
            body: Vec::new(),
        };
        let session = parse_login_response(&ok).expect("session");
        assert_eq!(session.cookie, "session=/people/tester%2C2026");
        // A failed login yields no session.
        let denied = HttpResponse {
            status: 401,
            headers: BTreeMap::new(),
            body: Vec::new(),
        };
        assert_eq!(parse_login_response(&denied), None);
    }

    #[test]
    fn rest_apply_request_carries_comment_and_field() {
        let record = parse_record(RECORD_REV7.as_bytes()).expect("record");
        let request = build_rest_apply_request(&session(), &record, &proposal());
        assert_eq!(request.method, HttpMethod::Put);
        assert_eq!(
            request.url.as_str(),
            "https://openlibrary.org/books/OL7826547M.json"
        );
        assert_eq!(
            request.headers.get("cookie").map(String::as_str),
            Some(session().cookie.as_str())
        );
        let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json");
        assert_eq!(body["isbn_13"], json!(["9780140328721"]));
        assert_eq!(
            body["isbn_10"],
            json!(["0140328726"]),
            "untouched fields replay"
        );
        assert_eq!(body["_comment"], json!(proposal().comment));
    }

    #[test]
    fn edit_form_parses_and_fills_fail_closed() {
        let form = parse_edit_form(EDIT_FORM_HTML, "/books/OL7826547M").expect("known form");
        assert_eq!(form.action_path, "/books/OL7826547M/edit");
        // All non-submit fields replayed, including the hidden v.
        assert!(form.fields.iter().any(|(n, v)| n == "v" && v == "7"));
        assert!(
            form.fields
                .iter()
                .any(|(n, v)| n == "edition--title--0" && v == "Matilda")
        );

        let filled = fill_edit_form(&form, &proposal()).expect("fills");
        assert!(
            filled
                .fields
                .iter()
                .any(|(n, v)| n == &form_field_name("isbn_13") && v == "9780140328721")
        );
        assert!(
            filled
                .fields
                .iter()
                .any(|(n, v)| n == "_comment" && v == &proposal().comment)
        );

        // Fail-closed: a form missing the target slot or the comment slot
        // refuses rather than inventing fields.
        let missing_slot =
            EDIT_FORM_HTML.replace("edition--identifiers--isbn_13", "edition--renamed--isbn_13");
        let form = parse_edit_form(&missing_slot, "/books/OL7826547M").expect("parses");
        assert_eq!(fill_edit_form(&form, &proposal()), None);
        // Fail-closed: an occupied target slot is a closed gap.
        let occupied = EDIT_FORM_HTML.replace(
            r#"name="edition--identifiers--isbn_13" value="""#,
            r#"name="edition--identifiers--isbn_13" value="9789999999991""#,
        );
        let form = parse_edit_form(&occupied, "/books/OL7826547M").expect("parses");
        assert_eq!(fill_edit_form(&form, &proposal()), None);
        // Fail-closed: not the edit form at all (login page), or widgets the
        // adapter cannot replay faithfully.
        assert_eq!(
            parse_edit_form("<html>Please log in</html>", "/books/OL7826547M"),
            None
        );
        let with_checkbox = EDIT_FORM_HTML.replace(
            r#"<input type="submit" value="Save"/>"#,
            r#"<input type="checkbox" name="edition--flag"/><input type="submit" value="Save"/>"#,
        );
        assert_eq!(parse_edit_form(&with_checkbox, "/books/OL7826547M"), None);
    }

    #[test]
    fn form_submit_encodes_all_fields_and_checks_success_shape() {
        let form = EditForm {
            action_path: "/books/OL7826547M/edit".to_string(),
            fields: vec![
                ("v".to_string(), "7".to_string()),
                ("_comment".to_string(), "a comment".to_string()),
            ],
        };
        let request = build_form_submit_request(&session(), &form);
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.url.as_str(),
            "https://openlibrary.org/books/OL7826547M/edit"
        );
        let body = String::from_utf8(request.body).expect("utf8");
        assert!(body.contains("v=7"));
        assert!(body.contains("_comment=a+comment"));

        let ok = redirect("https://openlibrary.org/books/OL7826547M/Matilda");
        assert!(form_submit_succeeded(&ok, "/books/OL7826547M"));
        let wrong = response(200, "<html>error</html>");
        assert!(!form_submit_succeeded(&wrong, "/books/OL7826547M"));
    }

    #[test]
    fn rest_lane_applies_and_caches_on_success() {
        // read (drift) -> PUT 200 -> read-back shows the value.
        let client = StubHttpClient::new([
            Ok(response(200, RECORD_REV7)),
            Ok(response(200, r#"{"status": "ok"}"#)),
            Ok(response(200, RECORD_REV8_APPLIED)),
        ]);
        let mut lane_cache = None;
        let outcome = block_on(execute_enrichment_apply_unchecked(
            &client,
            &session(),
            &proposal(),
            &mut lane_cache,
        ));
        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                lane: ApplyLane::Rest,
                new_revision: 8
            }
        );
        assert_eq!(lane_cache, Some(ApplyLane::Rest));
    }

    #[test]
    fn rest_403_falls_back_to_the_form_lane_within_one_apply() {
        // read (drift) -> PUT 403 (capability answer) -> read (drift restart)
        // -> form GET -> form POST redirect -> read-back.
        let client = StubHttpClient::new([
            Ok(response(200, RECORD_REV7)),
            Ok(response(403, r#"{"error": "permission_denied"}"#)),
            Ok(response(200, RECORD_REV7)),
            Ok(response(200, EDIT_FORM_HTML)),
            Ok(redirect("https://openlibrary.org/books/OL7826547M/Matilda")),
            Ok(response(200, RECORD_REV8_APPLIED)),
        ]);
        let mut lane_cache = None;
        let outcome = block_on(execute_enrichment_apply_unchecked(
            &client,
            &session(),
            &proposal(),
            &mut lane_cache,
        ));
        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                lane: ApplyLane::Form,
                new_revision: 8
            }
        );
        assert_eq!(lane_cache, Some(ApplyLane::Form), "discovery cached");
    }

    #[test]
    fn cached_form_lane_skips_the_rest_attempt() {
        // With the lane cached, the request sequence has NO PUT:
        // read (drift) -> form GET -> form POST -> read-back.
        let client = StubHttpClient::new([
            Ok(response(200, RECORD_REV7)),
            Ok(response(200, EDIT_FORM_HTML)),
            Ok(redirect("https://openlibrary.org/books/OL7826547M/Matilda")),
            Ok(response(200, RECORD_REV8_APPLIED)),
        ]);
        let mut lane_cache = Some(ApplyLane::Form);
        let outcome = block_on(execute_enrichment_apply_unchecked(
            &client,
            &session(),
            &proposal(),
            &mut lane_cache,
        ));
        assert!(matches!(
            outcome,
            ApplyOutcome::Applied {
                lane: ApplyLane::Form,
                ..
            }
        ));
    }

    #[test]
    fn moved_revision_refuses_before_any_write() {
        // The record advanced to revision 9 since the proposal: exactly one
        // request (the drift read) is issued, then refusal.
        let client = StubHttpClient::new([Ok(response(
            200,
            r#"{"key": "/books/OL7826547M", "revision": 9, "isbn_10": ["0140328726"]}"#,
        ))]);
        let mut lane_cache = None;
        let outcome = block_on(execute_enrichment_apply_unchecked(
            &client,
            &session(),
            &proposal(),
            &mut lane_cache,
        ));
        assert_eq!(
            outcome,
            ApplyOutcome::RefusedDrift {
                base_revision: 7,
                current_revision: 9
            }
        );
        assert_eq!(lane_cache, None, "no lane discovered by a refusal");
    }

    #[test]
    fn contract_drift_refuses_and_never_posts() {
        // The form fetch returns something unrecognizable: the queue holds
        // NOTHING after it, so any attempted POST would error the test.
        let client = StubHttpClient::new([
            Ok(response(200, RECORD_REV7)),
            Ok(response(403, "denied")),
            Ok(response(200, RECORD_REV7)),
            Ok(response(
                200,
                "<html>a redesigned page with no known form</html>",
            )),
        ]);
        let mut lane_cache = None;
        let outcome = block_on(execute_enrichment_apply_unchecked(
            &client,
            &session(),
            &proposal(),
            &mut lane_cache,
        ));
        assert!(matches!(outcome, ApplyOutcome::RefusedContractDrift { .. }));
    }

    #[test]
    fn read_back_mismatch_is_a_loud_failure_not_success() {
        // The PUT reports 200 but the read-back does not show the value.
        let client = StubHttpClient::new([
            Ok(response(200, RECORD_REV7)),
            Ok(response(200, r#"{"status": "ok"}"#)),
            Ok(response(200, RECORD_REV7)), // unchanged!
        ]);
        let mut lane_cache = None;
        let outcome = block_on(execute_enrichment_apply_unchecked(
            &client,
            &session(),
            &proposal(),
            &mut lane_cache,
        ));
        let ApplyOutcome::Failed { message } = outcome else {
            panic!("expected Failed, got {outcome:?}");
        };
        assert!(message.contains("read-back"));
    }

    #[test]
    fn read_back_verifies_the_exact_proposed_value() {
        let record = parse_record(RECORD_REV8_APPLIED.as_bytes()).expect("record");
        assert!(verify_applied(&record, &proposal()));
        let unchanged = parse_record(RECORD_REV7.as_bytes()).expect("record");
        assert!(!verify_applied(&unchanged, &proposal()));
    }

    #[test]
    fn record_request_targets_the_json_read() {
        assert_eq!(
            build_record_request("/books/OL7826547M").url.as_str(),
            "https://openlibrary.org/books/OL7826547M.json"
        );
    }

    #[test]
    fn decode_html_entities_in_form_values() {
        // PR 148 P2: decode form values before replaying.
        let form_with_entities = r#"<html><body>
<form method="post" action="/books/OL7826547M/edit" id="addbook">
<input type="hidden" name="v" value="7"/>
<input type="text" name="edition--title--0" value="Tom &amp; Jerry"/>
<input type="text" name="edition--isbn_10--0" value="0140328726"/>
<input type="text" name="edition--isbn_13--0" value=""/>
<textarea name="edition--notes--0"></textarea>
<input type="text" name="_comment" value=""/>
<input type="submit" value="Save"/>
</form></body></html>"#;
        let form = parse_edit_form(form_with_entities, "/books/OL7826547M")
            .expect("parses form with entities");
        assert!(
            form.fields
                .iter()
                .any(|(n, v)| n == "edition--title--0" && v == "Tom & Jerry"),
            "entity should be decoded to &"
        );
    }

    #[test]
    fn preserve_textarea_whitespace_in_form() {
        // PR 148 P2: preserve textarea contents when replaying.
        let form_with_textarea = r#"<html><body>
<form method="post" action="/books/OL7826547M/edit">
<textarea name="edition--notes--0">  leading and trailing spaces  </textarea>
<input type="text" name="_comment" value=""/>
<input type="submit" value="Save"/>
</form></body></html>"#;
        let form = parse_edit_form(form_with_textarea, "/books/OL7826547M")
            .expect("parses form with textarea");
        assert!(
            form.fields
                .iter()
                .any(|(n, v)| n == "edition--notes--0" && v == "  leading and trailing spaces  "),
            "textarea whitespace should be preserved"
        );
    }

    #[test]
    fn handle_radio_controls() {
        // PR 148 P2: handle radio controls before enabling form fallback.
        let form_with_radio = r#"<html><body>
<form method="post" action="/books/OL7826547M/edit">
<input type="radio" name="edition--weight_units--0" value="pounds"/>
<input type="radio" name="edition--weight_units--0" value="kg" checked/>
<input type="text" name="_comment" value=""/>
<input type="submit" value="Save"/>
</form></body></html>"#;
        let form =
            parse_edit_form(form_with_radio, "/books/OL7826547M").expect("parses form with radio");
        assert!(
            form.fields
                .iter()
                .any(|(n, v)| n == "edition--weight_units--0" && v == "kg"),
            "checked radio value should be captured"
        );
    }

    #[test]
    fn handle_select_default_option() {
        // PR 148 P2: replay default select values instead of refusing.
        let form_with_select = r#"<html><body>
<form method="post" action="/books/OL7826547M/edit">
<select name="edition--language--0">
<option value="eng">English</option>
<option value="fra">French</option>
</select>
<input type="text" name="_comment" value=""/>
<input type="submit" value="Save"/>
</form></body></html>"#;
        let form = parse_edit_form(form_with_select, "/books/OL7826547M")
            .expect("parses form with select default");
        assert!(
            form.fields
                .iter()
                .any(|(n, v)| n == "edition--language--0" && v == "eng"),
            "first option should be used as browser default"
        );
    }

    #[test]
    fn isbn_form_field_name_targets_identifiers_list() {
        // PR 148 P2: target the form's identifier-list fields.
        assert_eq!(
            form_field_name("isbn_13"),
            "edition--identifiers--isbn_13",
            "isbn_13 should map to identifiers list"
        );
        assert_eq!(
            form_field_name("isbn_10"),
            "edition--identifiers--isbn_10",
            "isbn_10 should map to identifiers list"
        );
        assert_eq!(
            form_field_name("title"),
            "edition--title--0",
            "other fields use direct slot"
        );
    }

    #[test]
    fn gate_disabled_when_write_lane_disabled() {
        // PR 148 P2: enforce the disabled write gate at the apply entry point.
        // With the gate disabled, execute_enrichment_apply returns Failed
        // immediately without issuing any client requests.
        let client = StubHttpClient::new([]);
        let mut lane_cache = None;
        let outcome = block_on(execute_enrichment_apply(
            &client,
            &session(),
            &proposal(),
            &mut lane_cache,
        ));
        assert!(
            matches!(outcome, ApplyOutcome::Failed { message } if message.contains("disabled")),
            "apply should refuse immediately when gate is disabled"
        );
    }
}
