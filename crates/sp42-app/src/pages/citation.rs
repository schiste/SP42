use leptos::prelude::*;
use sp42_core::{
    BodyUsabilityReason, CitationFinding, CitationVerdict, DevAuthBootstrapRequest,
    GroundingStatus, PageVerificationReport, PanelAgreement, SourceUnavailableReason, SupportLevel,
};
use sp42_reporting::{
    ReportSection, page_verification_report_to_document, render_page_verification_text,
};

use crate::components::{StatusBadge, StatusTone};
use crate::platform::auth::{bootstrap_dev_auth_session, fetch_dev_auth_session_status};
use crate::platform::citation::fetch_page_report;
use crate::platform::config::configured_default_wiki_id;

#[component]
pub fn CitationSurface() -> impl IntoView {
    let (wiki_id, set_wiki_id) = signal(configured_default_wiki_id());
    let (title, set_title) = signal(String::new());
    let (rev, set_rev) = signal(String::new());
    let (report, set_report) = signal(None::<PageVerificationReport>);
    let (load_error, set_load_error) = signal(None::<String>);
    let (loading, set_loading) = signal(false);

    let load_action = Action::new_local(move |_: &()| {
        let wiki = wiki_id.get_untracked();
        let article_title = title.get_untracked();
        let rev_input = rev.get_untracked();
        async move {
            let trimmed_title = article_title.trim().to_string();
            if trimmed_title.is_empty() {
                set_load_error.set(Some(
                    "Enter an article title before verifying citations.".to_string(),
                ));
                set_report.set(None);
                return;
            }
            // A blank revision means "latest"; the server resolves 0 to a concrete id.
            let trimmed_rev = rev_input.trim();
            let rev_id = if trimmed_rev.is_empty() {
                0
            } else {
                match trimmed_rev.parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => {
                        set_load_error.set(Some(
                            "Revision must be a number, or leave it blank for the latest."
                                .to_string(),
                        ));
                        set_report.set(None);
                        return;
                    }
                }
            };

            set_loading.set(true);
            set_load_error.set(None);

            // The verify-page route is session+CSRF gated, so ensure a session
            // before calling it — Citations works standalone (no dependency on
            // having visited Patrol first) and survives the ~30-min expiry.
            // Prefer an existing session: in desktop/VPS mode the session is a
            // real OAuth session and the local dev-token bootstrap is rejected, so
            // only fall back to bootstrap when no session is active. Either path
            // refreshes the CSRF token that post_json_bytes attaches.
            let have_session = matches!(
                fetch_dev_auth_session_status().await,
                Ok(status) if status.authenticated
            );
            if !have_session {
                let bootstrap = DevAuthBootstrapRequest {
                    username: String::new(),
                    scopes: Vec::new(),
                    expires_at_ms: None,
                };
                match bootstrap_dev_auth_session(&bootstrap).await {
                    Ok(status) if status.authenticated => {}
                    Ok(_) => {
                        set_report.set(None);
                        set_load_error.set(Some(
                            "Could not start an authenticated session — check the local Wikimedia token (.env.wikimedia.local)."
                                .to_string(),
                        ));
                        set_loading.set(false);
                        return;
                    }
                    Err(error) => {
                        set_report.set(None);
                        set_load_error.set(Some(format!("Auth bootstrap failed: {error}")));
                        set_loading.set(false);
                        return;
                    }
                }
            }

            match fetch_page_report(&wiki, &trimmed_title, rev_id).await {
                Ok(next_report) => set_report.set(Some(next_report)),
                Err(error) => {
                    set_report.set(None);
                    set_load_error.set(Some(error));
                }
            }
            set_loading.set(false);
        }
    });

    view! {
        <section class="article-workspace">
            <form
                class="article-command-bar"
                on:submit=move |ev| {
                    ev.prevent_default();
                    load_action.dispatch_local(());
                }
            >
                <div class="article-command-title">
                    <span class="section-header">"Citation Review"</span>
                    <strong>"Verify every citation on a revision"</strong>
                </div>
                <label class="article-field">
                    <span>"Wiki"</span>
                    <input
                        class="article-input article-input-short"
                        type="text"
                        prop:value=move || wiki_id.get()
                        on:input=move |ev| set_wiki_id.set(input_value(&ev))
                    />
                </label>
                <label class="article-field article-field-title">
                    <span>"Title"</span>
                    <input
                        class="article-input"
                        type="text"
                        placeholder="Article title"
                        prop:value=move || title.get()
                        on:input=move |ev| set_title.set(input_value(&ev))
                    />
                </label>
                <label class="article-field">
                    <span>"Revision"</span>
                    <input
                        class="article-input article-input-short"
                        type="text"
                        placeholder="latest"
                        prop:value=move || rev.get()
                        on:input=move |ev| set_rev.set(input_value(&ev))
                    />
                </label>
                <button class="btn btn-compact btn-success" type="submit" disabled=move || loading.get()>
                    {move || if loading.get() { "Verifying" } else { "Verify" }}
                </button>
            </form>

            {move || {
                if let Some(error) = load_error.get() {
                    return view! {
                        <div class="article-state article-state-error">{error}</div>
                    }.into_any();
                }
                if let Some(current_report) = report.get() {
                    return view! {
                        <PageReportView report=current_report />
                    }.into_any();
                }
                view! {
                    <div class="article-state">
                        "Verify a revision to see per-citation verdicts, the evidence located in each source, and which citations need attention."
                    </div>
                }.into_any()
            }}
        </section>
    }
}

#[component]
fn PageReportView(report: PageVerificationReport) -> impl IntoView {
    let stats = report.stats.clone();
    let title = report.title.clone();
    let wiki = report.wiki_id.clone();
    let rev = report.rev_id;

    // The verdict tally as a scannable row of chips; a zero count stays neutral so
    // the page only "lights up" red/amber when there is actually something to act on.
    let chips = vec![
        ("Supported", stats.supported, StatusTone::Success),
        ("Partial", stats.partial, StatusTone::Warning),
        ("Not supported", stats.not_supported, StatusTone::Danger),
        ("Unavailable", stats.source_unavailable, StatusTone::Info),
    ];
    let meta_line = format!(
        "{refs} refs · {verified} use-sites verified · unavailable: {dead} dead, {unusable} unusable · {skipped} skipped · {failures} extraction failures",
        refs = stats.refs_seen,
        verified = stats.use_sites_verified,
        dead = stats.source_unavailable_unreachable,
        unusable = stats.source_unavailable_unusable,
        skipped = stats.skipped,
        failures = stats.extraction_failures,
    );

    // Problem-first ordering: the actionable findings (unsupported, unverified
    // support, dead links, partials) float to the top regardless of document
    // position, so a reviewer is not hunting through dozens of clean citations.
    let mut findings = report.findings.clone();
    findings.sort_by_key(|finding| (severity_rank(finding), finding.use_site_ordinal));
    let total = findings.len();
    let problem_count = findings
        .iter()
        .filter(|finding| is_problem(finding))
        .count();
    let findings = StoredValue::new(findings);
    let (problems_only, set_problems_only) = signal(false);
    // A toggle only earns its place when it changes the view: some — but not all —
    // findings are problems.
    let show_toggle = problem_count > 0 && problem_count < total;

    // Skipped / extraction-failure sections render fine as the shared text lines.
    let other_sections: Vec<ReportSection> = page_verification_report_to_document(&report)
        .sections
        .into_iter()
        .filter(|section| section.name != "Findings")
        .collect();
    let report_text = render_page_verification_text(&report);

    view! {
        <div class="article-inventory">
            <header class="article-inventory-header">
                <div>
                    <span class="section-header">{wiki}" · rev "{rev}</span>
                    <h1>{title}</h1>
                </div>
                <div style="display:flex;gap:6px;flex-wrap:wrap;">
                    {chips
                        .into_iter()
                        .map(|(label, count, tone)| {
                            let tone = if count == 0 { StatusTone::Neutral } else { tone };
                            view! { <StatusBadge label=format!("{label} {count}") tone=tone /> }
                        })
                        .collect_view()}
                </div>
            </header>
            <p class="article-reference-meta" style="margin:0;">{meta_line}</p>

            <section class="article-panel">
                <header
                    class="article-panel-header"
                    style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;"
                >
                    <StatusBadge label="Findings".to_string() tone=StatusTone::Accent />
                    <div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap;">
                        <StatusBadge
                            label=format!("{problem_count} need attention")
                            tone=if problem_count == 0 { StatusTone::Success } else { StatusTone::Warning }
                        />
                        {show_toggle.then(|| view! {
                            <button
                                class="btn btn-compact"
                                on:click=move |_| set_problems_only.update(|value| *value = !*value)
                            >
                                {move || if problems_only.get() {
                                    format!("Show all ({total})")
                                } else {
                                    format!("Problems only ({problem_count})")
                                }}
                            </button>
                        })}
                    </div>
                </header>
                <div class="article-reference-list">
                    {move || {
                        let only = problems_only.get();
                        findings.with_value(|all| {
                            all.iter()
                                .filter(|finding| !only || is_problem(finding))
                                .cloned()
                                .map(|finding| view! { <FindingCard finding=finding /> })
                                .collect_view()
                        })
                    }}
                </div>
            </section>

            <div class="article-panels">
                {other_sections
                    .into_iter()
                    .map(|section| view! { <ReportSectionCard section=section /> })
                    .collect_view()}
            </div>

            <details
                style="padding:10px 17px;border-radius:4px;border:1px solid rgba(148,163,184,.14);background:rgba(8,15,29,.58);"
            >
                <summary style="cursor:pointer;font-weight:700;">"Raw text report"</summary>
                <pre
                    style="margin:.75rem 0 0;overflow:auto;white-space:pre-wrap;word-break:break-word;font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;font-size:.9rem;line-height:1.55;color:#eff4ff;"
                >{report_text}</pre>
            </details>
        </div>
    }
}

#[component]
fn FindingCard(finding: CitationFinding) -> impl IntoView {
    let (verdict_label, verdict_tone) = verdict_meta(&finding);
    let grounding = grounding_flag(finding.grounding_status);
    let agreement = agreement_label(finding.agreement);
    let reason = unavailable_label(&finding);
    let ordinal = finding.use_site_ordinal;
    let ref_id = finding.ref_id.clone();
    let url = finding.provenance.url.to_string();
    let archive = finding.archive_of.as_ref().map(ToString::to_string);
    let claim = finding.claim.clone();
    // The verbatim passage located in the source — the evidence behind the
    // verdict, and the single most useful thing for a reviewer to read.
    let quote = finding
        .passage
        .as_ref()
        .map(|passage| passage.quote.clone());

    let heading = if ref_id.is_empty() {
        format!("#{ordinal}")
    } else {
        format!("#{ordinal} {ref_id}")
    };

    view! {
        <article class="article-reference">
            <div class="article-reference-top">
                <strong>{heading}</strong>
                <div style="display:flex;gap:6px;flex-wrap:wrap;justify-content:flex-end;">
                    {agreement.map(|label| view! {
                        <StatusBadge label=label tone=StatusTone::Neutral />
                    })}
                    <StatusBadge label=verdict_label tone=verdict_tone />
                </div>
            </div>

            {grounding.map(|(label, tone)| view! {
                <div class="article-reference-meta">
                    <StatusBadge label=label.to_string() tone=tone />
                </div>
            })}

            <p style="color:#eff4ff;">
                <span style="color:#8b9fc0;">"Claim: "</span>
                {claim}
            </p>

            {quote.map(|text| view! {
                <blockquote style="margin:2px 0;padding:6px 10px;border-inline-start:3px solid rgba(61,185,125,.5);background:rgba(15,23,42,.58);color:#eff4ff;font-size:12px;line-height:1.5;">
                    <span style="display:block;color:#8b9fc0;font-size:10px;text-transform:uppercase;letter-spacing:.06em;margin-bottom:3px;">
                        "Evidence located in source"
                    </span>
                    {text}
                </blockquote>
            })}

            {reason.map(|value| view! { <div class="article-reference-meta">{value}</div> })}

            <div class="article-reference-meta">
                <a
                    href=url.clone()
                    target="_blank"
                    rel="noopener noreferrer"
                    style="color:#7cc4ff;word-break:break-all;"
                >
                    {url.clone()}
                </a>
            </div>
            {archive.map(|value| view! {
                <div class="article-reference-meta">
                    "via archive of "
                    <a
                        href=value.clone()
                        target="_blank"
                        rel="noopener noreferrer"
                        style="color:#7cc4ff;word-break:break-all;"
                    >
                        {value.clone()}
                    </a>
                </div>
            })}
        </article>
    }
}

fn verdict_meta(finding: &CitationFinding) -> (String, StatusTone) {
    match finding.verdict {
        CitationVerdict::Judged(SupportLevel::Supported) => {
            ("supported".to_string(), StatusTone::Success)
        }
        CitationVerdict::Judged(SupportLevel::Partial) => {
            ("partial".to_string(), StatusTone::Warning)
        }
        CitationVerdict::Judged(SupportLevel::NotSupported) => {
            ("not supported".to_string(), StatusTone::Danger)
        }
        CitationVerdict::SourceUnavailable => ("source unavailable".to_string(), StatusTone::Info),
    }
}

/// A support-class verdict (the panel judged the source to back the claim).
fn is_support(verdict: CitationVerdict) -> bool {
    matches!(
        verdict,
        CitationVerdict::Judged(SupportLevel::Supported | SupportLevel::Partial)
    )
}

/// Ordering weight for problem-first sorting — lower sorts earlier. The tiers,
/// most urgent first: refuted (`NotSupported`), support whose quote could not be
/// located (unverified), a dead live link, a partial, an unusable source (tool
/// limitation), and finally a grounded `Supported`.
fn severity_rank(finding: &CitationFinding) -> u8 {
    match finding.verdict {
        CitationVerdict::Judged(SupportLevel::NotSupported) => 0,
        _ if is_support(finding.verdict)
            && finding.grounding_status == GroundingStatus::Unlocated =>
        {
            1
        }
        CitationVerdict::SourceUnavailable
            if finding.source_unavailable_reason == Some(SourceUnavailableReason::Unreachable) =>
        {
            2
        }
        CitationVerdict::Judged(SupportLevel::Partial) => 3,
        CitationVerdict::SourceUnavailable => 4,
        CitationVerdict::Judged(SupportLevel::Supported) => 5,
    }
}

/// A finding a reviewer likely needs to act on: refuted, unverified support, a
/// dead link, or a partial. Unusable sources (PDF/paywall) and grounded support
/// are not surfaced by the "problems only" filter.
fn is_problem(finding: &CitationFinding) -> bool {
    severity_rank(finding) <= 3
}

/// The measured panel agreement, shown only when it carries information (a panel
/// of at least two models — ADR-0006 §3).
fn agreement_label(agreement: PanelAgreement) -> Option<String> {
    agreement
        .is_meaningful()
        .then(|| format!("{}/{} agree", agreement.winner_votes, agreement.panel_size))
}

/// The grounding axis as a reviewer-facing flag. Crucially, an `Unlocated`
/// support is surfaced as *unverified* (amber) rather than hidden behind a green
/// verdict — the panel claimed support but the quote was not found in the source.
fn grounding_flag(status: GroundingStatus) -> Option<(&'static str, StatusTone)> {
    match status {
        GroundingStatus::Located => Some(("quote located in source", StatusTone::Success)),
        GroundingStatus::LocatedFuzzy => Some(("quote located (fuzzy match)", StatusTone::Warning)),
        GroundingStatus::Unlocated => Some((
            "unverified — quote not found in source",
            StatusTone::Warning,
        )),
        GroundingStatus::NotApplicable => None,
    }
}

/// A human-readable reason for a `SourceUnavailable` verdict: a dead link (with
/// its HTTP status when known) or a fetched-but-unreadable body (with the
/// classifier detail). `None` for any other verdict.
fn unavailable_label(finding: &CitationFinding) -> Option<String> {
    match finding.source_unavailable_reason? {
        SourceUnavailableReason::Unreachable => {
            let status = finding
                .provenance
                .http_status
                .map_or_else(String::new, |status| format!(" (HTTP {status})"));
            Some(format!("source unavailable — dead link{status}"))
        }
        SourceUnavailableReason::Unusable => Some(format!(
            "source unavailable — {}",
            finding
                .unusable_reason
                .map_or("could not read content", humanize_unusable)
        )),
    }
}

/// Plain-language rendering of a body-classifier reason (the `{:?}` debug form is
/// not reviewer-facing).
fn humanize_unusable(reason: BodyUsabilityReason) -> &'static str {
    match reason {
        BodyUsabilityReason::Ok => "usable",
        BodyUsabilityReason::JsonLdLeak => "page returned metadata, not article text",
        BodyUsabilityReason::CssLeak => "page returned a stylesheet, not article text",
        BodyUsabilityReason::AntiBotChallenge => "blocked by an anti-bot challenge",
        BodyUsabilityReason::WaybackRedirectNotice => "archive returned a redirect notice",
        BodyUsabilityReason::WaybackChrome => "archive returned only toolbar chrome",
        BodyUsabilityReason::AmazonStub => "Amazon boilerplate, not article text",
        BodyUsabilityReason::ShortBody => "body too short to verify",
        BodyUsabilityReason::PdfBody => "PDF — not machine-readable here",
        BodyUsabilityReason::ViewerShell => "JavaScript viewer shell, no readable text",
        BodyUsabilityReason::NavChromePaywall => "paywall / sign-in wall",
    }
}

#[component]
fn ReportSectionCard(section: ReportSection) -> impl IntoView {
    let tone = if section.available {
        StatusTone::Success
    } else {
        StatusTone::Info
    };

    view! {
        <article class="article-panel">
            <header
                class="article-panel-header"
                style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;"
            >
                <StatusBadge label=section.name.clone() tone=tone />
                <StatusBadge
                    label=format!("{} line(s)", section.summary_lines.len())
                    tone=StatusTone::Info
                />
            </header>
            <ul class="article-list">
                {section
                    .summary_lines
                    .into_iter()
                    .map(|line| view! { <li>{line}</li> })
                    .collect_view()}
            </ul>
        </article>
    }
}

#[cfg(target_arch = "wasm32")]
fn input_value(ev: &leptos::ev::Event) -> String {
    use wasm_bindgen::JsCast;

    ev.target()
        .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|element| element.value())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn input_value(_ev: &leptos::ev::Event) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::{
        agreement_label, grounding_flag, humanize_unusable, is_problem, severity_rank,
        unavailable_label,
    };
    use crate::components::StatusTone;
    use sp42_core::{
        BodyUsabilityReason, CitationFinding, CitationFindingKind, CitationVerdict,
        GroundingAssertion, GroundingStatus, PanelAgreement, SourceProvenance,
        SourceUnavailableReason, SupportLevel,
    };

    fn finding(verdict: CitationVerdict, grounding: GroundingStatus) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict,
            grounding_status: grounding,
            source_unavailable_reason: None,
            unusable_reason: None,
            agreement: PanelAgreement::new(3, 3),
            passage: None,
            provenance: SourceProvenance {
                url: url::Url::parse("https://example.org/a").expect("test url"),
                content_hash: "hash".to_string(),
                fetched_at: 0,
                http_status: Some(200),
            },
            grounding: GroundingAssertion::SourceFetched {
                source_hash: "hash".to_string(),
            },
            use_site_ordinal: 0,
            ref_id: "ref".to_string(),
            claim: "claim".to_string(),
            preceding_context: Vec::new(),
            archive_of: None,
            schema_version: 1,
        }
    }

    fn unreachable(http_status: Option<u16>) -> CitationFinding {
        let mut f = finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
        );
        f.source_unavailable_reason = Some(SourceUnavailableReason::Unreachable);
        f.provenance.http_status = http_status;
        f
    }

    fn unusable(reason: Option<BodyUsabilityReason>) -> CitationFinding {
        let mut f = finding(
            CitationVerdict::SourceUnavailable,
            GroundingStatus::NotApplicable,
        );
        f.source_unavailable_reason = Some(SourceUnavailableReason::Unusable);
        f.unusable_reason = reason;
        f
    }

    #[test]
    fn severity_orders_problems_before_clean_citations() {
        let not_supported = finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable,
        );
        let unverified = finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Unlocated,
        );
        let supported = finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located,
        );

        // Refuted < unverified-support < dead link < partial < unusable < supported.
        assert!(severity_rank(&not_supported) < severity_rank(&unverified));
        assert!(severity_rank(&unverified) < severity_rank(&unreachable(None)));
        assert!(severity_rank(&unreachable(None)) < severity_rank(&unusable(None)));
        assert!(severity_rank(&unusable(None)) < severity_rank(&supported));
    }

    #[test]
    fn problem_filter_keeps_actionable_drops_clean() {
        assert!(is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::NotSupported),
            GroundingStatus::NotApplicable
        )));
        // A "supported" whose quote did not locate is unverified — still a problem.
        assert!(is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Unlocated
        )));
        assert!(is_problem(&unreachable(None)));
        assert!(is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Partial),
            GroundingStatus::Located
        )));

        // A grounded "supported" and a tool-limited unusable source are not.
        assert!(!is_problem(&finding(
            CitationVerdict::Judged(SupportLevel::Supported),
            GroundingStatus::Located
        )));
        assert!(!is_problem(&unusable(Some(BodyUsabilityReason::PdfBody))));
    }

    #[test]
    fn agreement_shows_only_for_a_real_panel() {
        assert_eq!(
            agreement_label(PanelAgreement::new(3, 2)),
            Some("2/3 agree".to_string())
        );
        // A single-model "panel" carries no agreement signal.
        assert_eq!(agreement_label(PanelAgreement::new(1, 1)), None);
    }

    #[test]
    fn unlocated_support_is_flagged_unverified() {
        let (label, tone) = grounding_flag(GroundingStatus::Unlocated).expect("flag present");
        assert!(label.contains("unverified"));
        assert_eq!(tone, StatusTone::Warning);
        // No quote expected → no flag (would be noise).
        assert!(grounding_flag(GroundingStatus::NotApplicable).is_none());
    }

    #[test]
    fn unavailable_label_carries_http_status_and_reason() {
        assert_eq!(
            unavailable_label(&unreachable(Some(404))),
            Some("source unavailable — dead link (HTTP 404)".to_string())
        );
        let pdf = unavailable_label(&unusable(Some(BodyUsabilityReason::PdfBody)))
            .expect("unusable label");
        assert!(pdf.contains("PDF"));
        // A grounded support has nothing unavailable to report.
        assert_eq!(
            unavailable_label(&finding(
                CitationVerdict::Judged(SupportLevel::Supported),
                GroundingStatus::Located
            )),
            None
        );
    }

    #[test]
    fn humanize_unusable_is_plain_language() {
        assert_eq!(
            humanize_unusable(BodyUsabilityReason::NavChromePaywall),
            "paywall / sign-in wall"
        );
    }
}
