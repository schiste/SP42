use leptos::prelude::*;
use sp42_citation::{
    FindingGroup, finding_is_problem, page_verification_report_to_document, panel_agreement_label,
    render_page_verification_text, source_unavailable_detail,
};
use sp42_core::{
    CitationFinding, CitoidMetadata, DevAuthBootstrapRequest, GroundingStatus,
    PageVerificationReport, parse_page_target,
};
use sp42_reporting::ReportSection;

use crate::components::style::wiki_base_url;
use crate::components::{StatusBadge, StatusTone};
use crate::platform::auth::{bootstrap_dev_auth_session, fetch_dev_auth_session_status};
use crate::platform::citation::fetch_page_report;
use crate::platform::config::{is_local_deployment, selected_wiki_id};

#[component]
pub fn CitationSurface() -> impl IntoView {
    let (wiki_id, set_wiki_id) = signal(selected_wiki_id());
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
            if article_title.trim().is_empty() {
                set_load_error.set(Some(
                    "Enter an article title or paste a wiki URL before verifying citations."
                        .to_string(),
                ));
                set_report.set(None);
                return;
            }
            // Accept a pasted /wiki/ or index.php URL, not just a bare title — the
            // server's action API treats a URL as a literal (missing) title.
            let target = parse_page_target(&article_title);
            // The Revision field overrides any oldid in a pasted URL; left blank it
            // falls back to that oldid, or 0 (latest, resolved server-side).
            let trimmed_rev = rev_input.trim();
            let rev_id = if trimmed_rev.is_empty() {
                target.rev_id
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

            // The verify-page route is session+CSRF gated. Ensuring a session via
            // the local dev-token bootstrap is a LOCAL-mode convenience only — in
            // vps/desktop that route is forbidden (403, not 401), so the global
            // 401 handler would never re-gate. Outside local mode we skip the
            // bootstrap entirely and let the verify call below 401, which drops the
            // user back to the login gate. Codex review #90.
            if is_local_deployment() {
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
            }

            match fetch_page_report(&wiki, &target.title, rev_id).await {
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
                        placeholder="Article title or URL"
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
        "{refs} refs · {verified} use-sites verified · unavailable: {dead} dead, {unusable} unreadable · {skipped} skipped · {failures} extraction failures",
        refs = stats.refs_seen,
        verified = stats.use_sites_verified,
        dead = stats.source_unavailable_unreachable,
        unusable = stats.source_unavailable_unusable,
        skipped = stats.skipped,
        failures = stats.extraction_failures,
    );

    // Bucket findings into actionable groups (problems first). Each non-empty group
    // becomes a labelled, collapsible section, so the eye scans section headers —
    // not a verdict column buried inside per-card detail. Document order is kept
    // within each group.
    let mut findings = report.findings.clone();
    findings.sort_by_key(|finding| finding.use_site_ordinal);
    let total = findings.len();
    let problem_count = findings
        .iter()
        .filter(|finding| finding_is_problem(finding))
        .count();
    let grouped: Vec<(FindingGroup, Vec<CitationFinding>)> = FindingGroup::ALL
        .into_iter()
        .filter_map(|group| {
            let items: Vec<CitationFinding> = findings
                .iter()
                .filter(|finding| FindingGroup::of(finding) == group)
                .cloned()
                .collect();
            (!items.is_empty()).then_some((group, items))
        })
        .collect();

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
                    <span class="section-header">{wiki.clone()}" · rev "{rev}</span>
                    <h1>{title.clone()}</h1>
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
            <p class="article-reference-meta" style="margin:0;">
                {format!("{problem_count} of {total} citation(s) need attention")}
            </p>

            <div style="display:grid;gap:10px;">
                {grouped
                    .into_iter()
                    .map(|(group, items)| view! {
                        <FindingGroupSection
                            group=group
                            findings=items
                            wiki_id=wiki.clone()
                            title=title.clone()
                            rev_id=rev
                        />
                    })
                    .collect_view()}
            </div>

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

/// One verdict-group section: a collapsible block whose header names the group and
/// count, color-coded by a left border. Problem groups open by default; the
/// confirmed-`Supported` group starts collapsed.
#[component]
fn FindingGroupSection(
    group: FindingGroup,
    findings: Vec<CitationFinding>,
    wiki_id: String,
    title: String,
    rev_id: u64,
) -> impl IntoView {
    let count = findings.len();
    let open = !group.collapsed_by_default();
    let tone = group_tone(group);
    let border = group_border(group);
    let group_title = group.title().to_string();
    let hint = group.hint();

    view! {
        <details
            open=open
            style=format!(
                "border:1px solid var(--border);border-inline-start:3px solid {border};border-radius:6px;background:var(--panel-inner);padding:10px 12px;"
            )
        >
            <summary style="cursor:pointer;display:flex;align-items:center;gap:8px;flex-wrap:wrap;">
                <StatusBadge label=format!("{group_title} · {count}") tone=tone />
                {hint.map(|text| view! {
                    <span class="article-reference-meta">{text}</span>
                })}
            </summary>
            <div class="article-reference-list" style="margin-top:10px;">
                {findings
                    .into_iter()
                    .map(|finding| {
                        let article_url =
                            article_anchor_url(&wiki_id, &title, rev_id, &finding.ref_id);
                        view! { <FindingCard finding=finding article_url=article_url /> }
                    })
                    .collect_view()}
            </div>
        </details>
    }
}

/// A deep link to the citation's inline marker on the verified revision, so an
/// editor can jump from a finding to where the claim sits in the article. `ref_id`
/// is the `cite_ref-…` anchor MediaWiki renders for the `[n]` marker; `None` when
/// the finding has no page ref (the standalone single-claim path).
fn article_anchor_url(wiki_id: &str, title: &str, rev_id: u64, ref_id: &str) -> Option<String> {
    if ref_id.is_empty() {
        return None;
    }
    let mut url = url::Url::parse(&format!("{}/w/index.php", wiki_base_url(wiki_id))).ok()?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("title", title);
        if rev_id != 0 {
            query.append_pair("oldid", &rev_id.to_string());
        }
    }
    url.set_fragment(Some(ref_id));
    Some(url.to_string())
}

#[component]
fn FindingCard(finding: CitationFinding, article_url: Option<String>) -> impl IntoView {
    // The verdict is carried by the enclosing group section + color, so the card
    // leads with the claim (the thing to check) and keeps the long evidence quote
    // behind a disclosure so rows stay compact while scanning.
    let agreement = panel_agreement_label(finding.agreement);
    let reason = source_unavailable_detail(&finding);
    // A located-fuzzy match is the one grounding nuance the group title does not
    // capture (it lives in the Supported/Partial group but is weaker), so flag it.
    let fuzzy = matches!(finding.grounding_status, GroundingStatus::LocatedFuzzy);
    let ordinal = finding.use_site_ordinal;
    let ref_id = finding.ref_id.clone();
    let url = finding.provenance.url.to_string();
    let archive = finding.archive_of.as_ref().map(ToString::to_string);
    let claim = finding.claim.clone();
    let quote = finding
        .passage
        .as_ref()
        .map(|passage| passage.quote.clone());
    let excerpt = finding.source_excerpt.clone();
    let source_meta = finding.metadata.as_ref().and_then(metadata_line);
    let has_evidence = quote.is_some() || excerpt.is_some();

    view! {
        <article class="article-reference">
            <div style="display:flex;justify-content:space-between;gap:8px;align-items:baseline;">
                <div style="min-width:0;">
                    <strong>{format!("#{ordinal}")}</strong>
                    {(!ref_id.is_empty()).then(|| view! {
                        <span style="font-size:10px;color:#8b9fc0;margin-inline-start:6px;overflow-wrap:anywhere;">
                            {ref_id.clone()}
                        </span>
                    })}
                </div>
                <div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap;justify-content:flex-end;">
                    {fuzzy.then(|| view! {
                        <StatusBadge label="fuzzy match".to_string() tone=StatusTone::Warning />
                    })}
                    {agreement.map(|label| view! {
                        <StatusBadge label=label tone=StatusTone::Neutral />
                    })}
                </div>
            </div>

            <p style="color:#eff4ff;margin:0;">{claim}</p>

            {article_url.map(|href| view! {
                <div class="article-reference-meta">
                    <a
                        href=href
                        target="_blank"
                        rel="noopener noreferrer"
                        style="color:#7cc4ff;font-weight:600;"
                    >
                        "↗ show citation in article"
                    </a>
                </div>
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

            // Citoid bibliographic context — always visible, since it helps identify
            // a source even when the tool could not read it.
            {source_meta.map(|line| view! {
                <div class="article-reference-meta">{format!("source: {line}")}</div>
            })}

            {has_evidence.then(|| view! {
                <details>
                    <summary style="cursor:pointer;font-size:10px;color:#8b9fc0;text-transform:uppercase;letter-spacing:.06em;">
                        "Source evidence"
                    </summary>
                    {quote.map(|text| view! {
                        <div style="margin:6px 0 0;">
                            <span style="display:block;color:#8b9fc0;font-size:10px;text-transform:uppercase;letter-spacing:.06em;margin-bottom:2px;">
                                "Located quote"
                            </span>
                            <blockquote style="margin:0;padding:6px 10px;border-inline-start:3px solid rgba(61,185,125,.5);background:rgba(15,23,42,.58);color:#eff4ff;font-size:12px;line-height:1.5;">
                                {text}
                            </blockquote>
                        </div>
                    })}
                    {excerpt.map(|text| view! {
                        <div style="margin:6px 0 0;">
                            <span style="display:block;color:#8b9fc0;font-size:10px;text-transform:uppercase;letter-spacing:.06em;margin-bottom:2px;">
                                "Source text (what the panel read)"
                            </span>
                            <blockquote style="margin:0;padding:6px 10px;border-inline-start:3px solid rgba(148,163,184,.3);background:rgba(15,23,42,.58);color:#cdd7ea;font-size:12px;line-height:1.5;white-space:pre-wrap;">
                                {text}
                            </blockquote>
                        </div>
                    })}
                </details>
            })}

        // Tier-2 (deferred): per-finding disposition for editors — "Looks right /
        // Disagree (because …)" capturing verdict-quality feedback — slots in at the
        // card footer here; persistence is a separate change.
        </article>
    }
}

/// A one-line bibliographic summary from Citoid metadata (title · author ·
/// publication · date), or `None` when no field is present.
fn metadata_line(meta: &CitoidMetadata) -> Option<String> {
    let parts: Vec<String> = [
        meta.title.as_ref(),
        meta.author.as_ref(),
        meta.publication.as_ref(),
        meta.published.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(String::clone)
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Section-header tone for a finding group.
fn group_tone(group: FindingGroup) -> StatusTone {
    match group {
        FindingGroup::NotSupported => StatusTone::Danger,
        FindingGroup::Unverified | FindingGroup::Partial => StatusTone::Warning,
        FindingGroup::DeadLink => StatusTone::Info,
        FindingGroup::Unreadable => StatusTone::Neutral,
        FindingGroup::VerifiedViaArchive => StatusTone::Accent,
        FindingGroup::Supported => StatusTone::Success,
    }
}

/// Left-border accent color for a group section (matches [`group_tone`]).
fn group_border(group: FindingGroup) -> &'static str {
    match group {
        FindingGroup::NotSupported => "#ef4444",
        FindingGroup::Unverified | FindingGroup::Partial => "#f59e0b",
        FindingGroup::DeadLink => "#3b82f6",
        FindingGroup::Unreadable => "#4f6280",
        FindingGroup::VerifiedViaArchive => "#8fb7ff",
        FindingGroup::Supported => "#22c55e",
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
