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
use sp42_ui::{
    Button, ButtonProps, ButtonType, CommandBar, CommandBarProps, CommandTitle, CommandTitleProps,
    DataPanel, DataPanelProps, Density, EvidenceBlock, EvidenceBlockProps, EvidenceDisclosure,
    EvidenceDisclosureProps, Field, FieldProps, Gap, Inline, InlineProps, InventoryHeader,
    InventoryHeaderProps, InventoryShell, InventoryShellProps, Link, LinkProps, MetaText,
    MetaTextProps, PageShell, PageShellProps, PanelGrid, PanelGridProps, RawReportDisclosure,
    RawReportDisclosureProps, ResultCard, ResultCardHeader, ResultCardHeaderProps, ResultCardProps,
    ResultDisclosure, ResultDisclosureProps, ResultList, ResultListProps, StatusBadge,
    StatusBadgeProps, StatusRegion, StatusRegionProps, Text, TextElement, TextInput,
    TextInputProps, TextProps, Tone, Width,
};

use crate::components::style::wiki_base_url;
use crate::components::ui_children;
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

    PageShell(PageShellProps::new(ui_children(move || {
        view! {
            {CommandBar(
                CommandBarProps::new(ui_children(move || view! {
                    {CommandTitle(CommandTitleProps::new(
                        "Citation Review",
                        "Verify every citation on a revision",
                    ))}
                    {Field(FieldProps::new(
                        "Wiki",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("citation-wiki")
                                    .with_value(Signal::derive(move || wiki_id.get()))
                                    .with_width(Width::Short)
                                    .with_density(Density::Compact)
                                    .on_input(move |ev| set_wiki_id.set(input_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Field(FieldProps::new(
                        "Title",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("citation-title")
                                    .with_value(Signal::derive(move || title.get()))
                                    .with_placeholder("Article title or URL")
                                    .with_width(Width::Full)
                                    .with_density(Density::Compact)
                                    .on_input(move |ev| set_title.set(input_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Field(FieldProps::new(
                        "Revision",
                        ui_children(move || view! {
                            {TextInput(
                                TextInputProps::new("citation-revision")
                                    .with_value(Signal::derive(move || rev.get()))
                                    .with_placeholder("latest")
                                    .with_width(Width::Short)
                                    .with_density(Density::Compact)
                                    .on_input(move |ev| set_rev.set(input_value(&ev)))
                            )}
                        }.into_any()),
                    ))}
                    {Button(
                        ButtonProps::new("Verify")
                            .with_type(ButtonType::Submit)
                            .with_tone(Tone::Success)
                            .with_density(Density::Compact)
                            .with_disabled(Signal::derive(move || loading.get()))
                    )}
                }.into_any()))
                .on_submit(move |ev| {
                    ev.prevent_default();
                    load_action.dispatch_local(());
                })
            )}

            {move || {
                if let Some(error) = load_error.get() {
                    return StatusRegion(
                        StatusRegionProps::new(ui_children(move || view! { {error} }.into_any()))
                            .with_tone(Tone::Danger),
                    )
                    .into_any();
                }
                if let Some(current_report) = report.get() {
                    return view! {
                        <PageReportView report=current_report />
                    }.into_any();
                }
                StatusRegion(StatusRegionProps::new(ui_children(|| {
                    view! {
                        "Verify a revision to see per-citation verdicts, the evidence located in each source, and which citations need attention."
                    }
                    .into_any()
                })))
                .into_any()
            }}
        }
        .into_any()
    })))
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
        ("Supported", stats.supported, Tone::Success),
        ("Partial", stats.partial, Tone::Warning),
        ("Not supported", stats.not_supported, Tone::Danger),
        ("Unavailable", stats.source_unavailable, Tone::Info),
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

    InventoryShell(InventoryShellProps::new(ui_children(move || {
        view! {
            {InventoryHeader(
                InventoryHeaderProps::new(format!("{wiki} · rev {rev}"), title.clone())
                    .with_actions(ui_children(move || view! {
                        {Inline(InlineProps::new(ui_children(move || view! {
                            {chips
                                .into_iter()
                                .map(|(label, count, tone)| {
                                    let tone = if count == 0 { Tone::Default } else { tone };
                                    StatusBadge(
                                        StatusBadgeProps::new(format!("{label} {count}"))
                                            .with_tone(tone),
                                    )
                                })
                                .collect_view()}
                        }.into_any())).with_gap(Gap::Small))}
                    }.into_any()))
            )}
            {MetaText(MetaTextProps::new(ui_children(move || view! { {meta_line.clone()} }.into_any())))}
            {MetaText(MetaTextProps::new(ui_children(move || {
                view! { {format!("{problem_count} of {total} citation(s) need attention")} }.into_any()
            })))}

            {ResultList(ResultListProps::new(ui_children(move || view! {
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
            }.into_any())))}

            {PanelGrid(PanelGridProps::new(ui_children(move || view! {
                {other_sections
                    .into_iter()
                    .map(|section| view! { <ReportSectionCard section=section /> })
                    .collect_view()}
            }.into_any())))}

            {RawReportDisclosure(RawReportDisclosureProps::new("Raw text report", report_text))}
        }
        .into_any()
    })))
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
    let group_title = group.title().to_string();
    let hint = group.hint();

    ResultDisclosure(
        ResultDisclosureProps::new(
            ui_children(move || {
                view! {
                    {StatusBadge(
                        StatusBadgeProps::new(format!("{group_title} · {count}"))
                            .with_tone(tone),
                    )}
                    {hint.map(|text| {
                        MetaText(MetaTextProps::new(ui_children(move || view! { {text} }.into_any())))
                    })}
                }
                .into_any()
            }),
            ui_children(move || {
                view! {
                    {ResultList(ResultListProps::new(ui_children(move || view! {
                {findings
                    .into_iter()
                    .map(|finding| {
                        let article_url =
                            article_anchor_url(&wiki_id, &title, rev_id, &finding.ref_id);
                        view! { <FindingCard finding=finding article_url=article_url /> }
                    })
                    .collect_view()}
                    }.into_any())))}
                }
                .into_any()
            }),
        )
        .with_tone(tone)
        .with_state(open),
    )
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

    ResultCard(ResultCardProps::new(ui_children(move || {
        view! {
            {ResultCardHeader(
                ResultCardHeaderProps::new(ui_children(move || {
                    view! {
                        <strong>{format!("#{ordinal}")}</strong>
                        {(!ref_id.is_empty()).then(|| {
                            MetaText(MetaTextProps::new(ui_children(move || {
                                view! { {ref_id.clone()} }.into_any()
                            })))
                        })}
                    }
                    .into_any()
                }))
                .with_actions(ui_children(move || {
                    view! {
                        {fuzzy.then(|| {
                            StatusBadge(
                                StatusBadgeProps::new("fuzzy match").with_tone(Tone::Warning),
                            )
                        })}
                        {agreement.map(|label| {
                            StatusBadge(StatusBadgeProps::new(label).with_tone(Tone::Default))
                        })}
                    }
                    .into_any()
                }))
            )}

            {Text(
                TextProps::new(ui_children(move || view! { {claim.clone()} }.into_any()))
                    .with_element(TextElement::Paragraph)
            )}

            {article_url.map(|href| {
                MetaText(MetaTextProps::new(ui_children(move || {
                    view! {
                        {Link(LinkProps::new("show citation in article", href).external())}
                    }
                    .into_any()
                })))
            })}

            {reason.map(|value| {
                MetaText(MetaTextProps::new(ui_children(move || view! { {value} }.into_any())))
            })}

            {MetaText(MetaTextProps::new(ui_children(move || {
                view! {
                    {Link(LinkProps::new(url.clone(), url.clone()).external())}
                }
                .into_any()
            })))}
            {archive.map(|value| {
                MetaText(MetaTextProps::new(ui_children(move || {
                    view! {
                        "via archive of "
                        {Link(LinkProps::new(value.clone(), value).external())}
                    }
                    .into_any()
                })))
            })}

            {source_meta.map(|line| {
                MetaText(MetaTextProps::new(ui_children(move || {
                    view! { {format!("source: {line}")} }.into_any()
                })))
            })}

            {has_evidence.then(|| {
                EvidenceDisclosure(
                    EvidenceDisclosureProps::new(
                        "Source evidence",
                        ui_children(move || {
                            view! {
                                {quote.map(|text| {
                                    EvidenceBlock(
                                        EvidenceBlockProps::new("Located quote", text)
                                            .with_tone(Tone::Success),
                                    )
                                })}
                                {excerpt.map(|text| {
                                    EvidenceBlock(EvidenceBlockProps::new(
                                        "Source text (what the panel read)",
                                        text,
                                    ))
                                })}
                            }
                            .into_any()
                        }),
                    ),
                )
            })}
        }
        .into_any()
    })))
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
fn group_tone(group: FindingGroup) -> Tone {
    match group {
        FindingGroup::NotSupported => Tone::Danger,
        FindingGroup::Unverified | FindingGroup::Partial => Tone::Warning,
        FindingGroup::DeadLink => Tone::Info,
        FindingGroup::Unreadable => Tone::Default,
        FindingGroup::VerifiedViaArchive => Tone::Accent,
        FindingGroup::Supported => Tone::Success,
    }
}

#[component]
fn ReportSectionCard(section: ReportSection) -> impl IntoView {
    let tone = if section.available {
        Tone::Success
    } else {
        Tone::Info
    };
    let name = section.name;
    let lines = section.summary_lines;
    let count = lines.len();
    let availability = if tone == Tone::Success {
        "available"
    } else {
        "unavailable"
    };

    DataPanel(
        DataPanelProps::new(
            name,
            ui_children(move || {
                view! {
                    {ResultList(ResultListProps::new(ui_children(move || {
                        view! {
                            {lines
                                .into_iter()
                                .map(|line| {
                                    ResultCard(ResultCardProps::new(ui_children(move || {
                                        view! { <span>{line}</span> }.into_any()
                                    })))
                                })
                                .collect_view()}
                        }
                        .into_any()
                    })))}
                }
                .into_any()
            }),
        )
        .with_count(format!("{count} line(s) · {availability}")),
    )
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
