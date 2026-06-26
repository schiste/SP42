use leptos::prelude::*;
use sp42_core::{
    CitationFinding, CitationVerdict, DevAuthBootstrapRequest, GroundingStatus,
    PageVerificationReport, SourceUnavailableReason, SupportLevel,
};
use sp42_reporting::{
    ReportDocument, ReportSection, page_verification_report_to_document,
    render_page_verification_text,
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
                        "Verify a revision to see per-citation verdicts, grounding status, and source availability."
                    </div>
                }.into_any()
            }}
        </section>
    }
}

#[component]
fn PageReportView(report: PageVerificationReport) -> impl IntoView {
    // The shared transform drives the stats lead + the simple sections and the
    // full-text pane (identical to the CLI). Findings are rendered structurally
    // below so each source URL is a real link.
    let report_text = render_page_verification_text(&report);
    let ReportDocument {
        title,
        lead_lines,
        sections,
    } = page_verification_report_to_document(&report);
    // Skipped / extraction-failure sections render fine as text; Findings get cards.
    let other_sections: Vec<ReportSection> = sections
        .into_iter()
        .filter(|section| section.name != "Findings")
        .collect();
    let mut findings = report.findings;
    findings.sort_by_key(|finding| finding.use_site_ordinal);
    let finding_count = findings.len();

    view! {
        <div class="article-inventory">
            <header class="article-inventory-header">
                <div>
                    <span class="section-header">{title}</span>
                </div>
                <ul style="margin:0;padding-inline-start:17px;color:#eff4ff;display:grid;gap:4px;">
                    {lead_lines
                        .into_iter()
                        .map(|line| view! { <li>{line}</li> })
                        .collect_view()}
                </ul>
            </header>

            <section class="article-panel">
                <header
                    class="article-panel-header"
                    style="display:flex;align-items:center;justify-content:space-between;gap:7px;flex-wrap:wrap;"
                >
                    <StatusBadge label="Findings".to_string() tone=StatusTone::Accent />
                    <StatusBadge
                        label=format!("{finding_count} citation(s)")
                        tone=StatusTone::Info
                    />
                </header>
                <div class="article-reference-list">
                    {findings
                        .into_iter()
                        .map(|finding| view! { <FindingCard finding=finding /> })
                        .collect_view()}
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
                <summary style="cursor:pointer;font-weight:700;">"Full text report"</summary>
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
    let grounding = grounding_label(finding.grounding_status);
    let reason = unavailable_label(&finding);
    let ordinal = finding.use_site_ordinal;
    let ref_id = finding.ref_id.clone();
    let url = finding.provenance.url.to_string();
    let archive = finding.archive_of.as_ref().map(ToString::to_string);
    let claim = finding.claim.clone();

    let heading = if ref_id.is_empty() {
        format!("#{ordinal}")
    } else {
        format!("#{ordinal} {ref_id}")
    };

    view! {
        <article class="article-reference">
            <div class="article-reference-top">
                <strong>{heading}</strong>
                <StatusBadge label=verdict_label tone=verdict_tone />
            </div>
            <div class="article-reference-meta">
                {grounding.map(|value| view! { <span>{format!("grounding: {value}")}</span> })}
                {reason.map(|value| view! { <span>{value}</span> })}
            </div>
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
            <p>{claim}</p>
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
            ("not supported".to_string(), StatusTone::Accent)
        }
        CitationVerdict::SourceUnavailable => ("source unavailable".to_string(), StatusTone::Info),
    }
}

fn grounding_label(status: GroundingStatus) -> Option<&'static str> {
    match status {
        GroundingStatus::Located => Some("located"),
        GroundingStatus::LocatedFuzzy => Some("located (fuzzy)"),
        GroundingStatus::Unlocated => Some("unlocated"),
        GroundingStatus::NotApplicable => None,
    }
}

fn unavailable_label(finding: &CitationFinding) -> Option<String> {
    match finding.source_unavailable_reason? {
        SourceUnavailableReason::Unreachable => Some("unreachable (dead link)".to_string()),
        SourceUnavailableReason::Unusable => Some(match finding.unusable_reason {
            Some(reason) => format!("unusable: {reason:?}"),
            None => "unusable".to_string(),
        }),
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
