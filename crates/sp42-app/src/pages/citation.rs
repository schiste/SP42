use leptos::prelude::*;
use sp42_citation::{
    FindingGroup, finding_is_problem, page_verification_report_to_document, panel_agreement_label,
    render_page_verification_text, source_unavailable_detail,
};
use sp42_core::{
    BareUrlApplyRequest, CitationConcernKind, CitationFinding, CitationVerdict, CitoidMetadata,
    DevAuthBootstrapRequest, GroundingStatus, PageVerificationReport, ReverifyFindingRequest,
    SessionActionExecutionRequest, SessionActionKind, SupportLevel, parse_page_target,
};
use sp42_reporting::ReportSection;
use sp42_ui::{
    Button, ButtonProps, ButtonSurface, ButtonType, CommandBar, CommandBarProps, CommandTitle,
    CommandTitleProps, DataPanel, DataPanelProps, Density, DiffEditPanel, DiffEditPanelProps,
    EvidenceBlock, EvidenceBlockProps, EvidenceDisclosure, EvidenceDisclosureProps, Field,
    FieldProps, Gap, Inline, InlineProps, InventoryHeader, InventoryHeaderProps, InventoryShell,
    InventoryShellProps, Link, LinkProps, MetaText, MetaTextProps, PageShell, PageShellProps,
    PanelGrid, PanelGridProps, RawReportDisclosure, RawReportDisclosureProps, ResultCard,
    ResultCardHeader, ResultCardHeaderProps, ResultCardProps, ResultDisclosure,
    ResultDisclosureProps, ResultList, ResultListProps, Select, SelectOption, SelectProps, Stack,
    StackProps, StatusBadge, StatusBadgeProps, StatusRegion, StatusRegionProps, Text, TextElement,
    TextInput, TextInputProps, TextProps, TextWeight, Tone, Width,
};

use crate::components::style::wiki_base_url;
use crate::components::ui_children;
use crate::platform::auth::{
    bootstrap_dev_auth_session, execute_dev_auth_action, fetch_dev_auth_session_status,
};
use crate::platform::citation::{
    apply_bare_url_proposal, fetch_bare_url_proposals, fetch_page_report, reverify_finding,
};
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
                        view! {
                            <FindingCardContainer
                                finding=finding
                                article_url=article_url
                                wiki_id=wiki_id.clone()
                                title=title.clone()
                                rev_id=rev_id
                            />
                        }
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

/// Owns the mutable, re-verifiable copy of one finding (PRD-0014): renders the
/// read-only `FindingCard` plus, beneath it, the action row for
/// `Partial`/`NotSupported` verdicts. Re-verify replaces the signal in place,
/// so both re-render with the fresh result without touching the page-level
/// report signal.
#[component]
fn FindingCardContainer(
    finding: CitationFinding,
    article_url: Option<String>,
    wiki_id: String,
    title: String,
    rev_id: u64,
) -> impl IntoView {
    let current = RwSignal::new(finding);

    view! {
        {move || view! { <FindingCard finding=current.get() article_url=article_url.clone() /> }}
        {move || {
            finding_has_action_row(&current.get()).then(|| {
                view! {
                    <FindingActionRow
                        finding=current
                        wiki_id=wiki_id.clone()
                        title=title.clone()
                        rev_id=rev_id
                    />
                }
            })
        }}
    }
}

/// Which of the four action-row controls is currently open. `None` of them
/// pre-selected — the row starts fully closed (PRD-0014 `DoD`: no default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenPanel {
    None,
    EditText,
    FixCitation,
    FlagCitation,
}

/// Which flow "Fix citation" routes to (PRD-0014, Resolved question 1):
/// repair (PRD-0008) when the finding traces to an existing `<ref>`, insert
/// (PRD-0012) when it doesn't — a use-site with no ref to replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixCitationRoute {
    Replace,
    Insert,
}

fn fix_citation_route(ref_id: &str) -> FixCitationRoute {
    if ref_id.is_empty() {
        FixCitationRoute::Insert
    } else {
        FixCitationRoute::Replace
    }
}

/// An action row renders only for `Partial`/`NotSupported` findings;
/// `Supported`/`SourceUnavailable` findings stay read-only (PRD-0014 `DoD`).
fn finding_has_action_row(finding: &CitationFinding) -> bool {
    matches!(
        finding.verdict,
        CitationVerdict::Judged(SupportLevel::Partial | SupportLevel::NotSupported)
    )
}

/// The concern kind SP42 suggests from `finding.verdict` (PRD-0014) — a
/// suggestion only; the operator may override to any wiki-configured kind
/// regardless of verdict (Resolved question 5).
fn suggested_concern_kind(finding: &CitationFinding) -> Option<CitationConcernKind> {
    match finding.verdict {
        CitationVerdict::Judged(SupportLevel::Partial) => Some(CitationConcernKind::PartialSupport),
        CitationVerdict::Judged(SupportLevel::NotSupported) => {
            Some(CitationConcernKind::FailedVerification)
        }
        _ => None,
    }
}

fn concern_kind_options() -> Vec<SelectOption> {
    vec![
        SelectOption::new(
            CitationConcernKind::PartialSupport.label(),
            "Partial support (span)",
        ),
        SelectOption::new(
            CitationConcernKind::FailedVerification.label(),
            "Failed verification (after ref)",
        ),
    ]
}

fn parse_concern_kind(value: &str) -> Option<CitationConcernKind> {
    match value {
        "partial-support" => Some(CitationConcernKind::PartialSupport),
        "failed-verification" => Some(CitationConcernKind::FailedVerification),
        _ => None,
    }
}

/// Which concern kind "Confirm flag" applies (PRD-0014, Resolved question 5):
/// the operator's selection always wins when present and parseable — SP42
/// only suggests, never decides, so a value the operator chose (including a
/// deliberate override away from the suggestion) takes priority over
/// `finding.verdict`'s default.
fn resolve_concern_kind(
    selected: Option<&str>,
    suggested: Option<CitationConcernKind>,
) -> Option<CitationConcernKind> {
    selected.and_then(parse_concern_kind).or(suggested)
}

/// The action row beneath a finding's card (PRD-0014): three equal-weight
/// actions (edit article text / fix citation / flag citation) plus a fourth,
/// always-available Re-verify control. None is pre-selected or defaulted;
/// each opens its own propose/preview step inline, terminating in the same
/// operator-confirmed apply every other action in this domain uses.
#[component]
fn FindingActionRow(
    finding: RwSignal<CitationFinding>,
    wiki_id: String,
    title: String,
    rev_id: u64,
) -> impl IntoView {
    let (open, set_open) = signal(OpenPanel::None);
    let (status, set_status) = signal(String::new());
    let (busy, set_busy) = signal(false);
    // The revision subsequent Edit/Fix/Flag actions from this row anchor to. Seeded with the
    // report's revision, but a successful Re-verify (which runs against the *latest* revision)
    // advances it to the revision actually verified — otherwise a later confirmed action would
    // carry a stale `baserevid` and could hit an edit conflict or anchor to the wrong revision.
    let (base_rev_id, set_base_rev_id) = signal(rev_id);
    let ordinal = finding.get_untracked().use_site_ordinal;
    let edit_textarea_id = format!("sp42-citation-edit-{ordinal}");
    let reason_textarea_id = format!("sp42-citation-reason-{ordinal}");
    let concern_select_id = format!("sp42-citation-concern-{ordinal}");

    Stack(StackProps::new(ui_children(move || {
        view! {
            {
                let wiki_id = wiki_id.clone();
                let title = title.clone();
                Inline(InlineProps::new(ui_children(move || {
                    let wiki_id = wiki_id.clone();
                    let title = title.clone();
                    view! {
                        {Button(
                            ButtonProps::new("Edit article text")
                                .with_surface(ButtonSurface::Subtle)
                                .with_density(Density::Compact)
                                .with_disabled(Signal::derive(move || busy.get()))
                                .on_click(move |_| {
                                    set_status.set(String::new());
                                    set_open.set(if open.get_untracked() == OpenPanel::EditText {
                                        OpenPanel::None
                                    } else {
                                        OpenPanel::EditText
                                    });
                                }),
                        )}
                        {Button(
                            ButtonProps::new("Fix citation")
                                .with_surface(ButtonSurface::Subtle)
                                .with_density(Density::Compact)
                                .with_disabled(Signal::derive(move || busy.get()))
                                .on_click(move |_| {
                                    set_status.set(String::new());
                                    set_open.set(if open.get_untracked() == OpenPanel::FixCitation {
                                        OpenPanel::None
                                    } else {
                                        OpenPanel::FixCitation
                                    });
                                }),
                        )}
                        {Button(
                            ButtonProps::new("Flag citation")
                                .with_surface(ButtonSurface::Subtle)
                                .with_density(Density::Compact)
                                .with_disabled(Signal::derive(move || busy.get()))
                                .on_click(move |_| {
                                    set_status.set(String::new());
                                    set_open.set(if open.get_untracked() == OpenPanel::FlagCitation {
                                        OpenPanel::None
                                    } else {
                                        OpenPanel::FlagCitation
                                    });
                                }),
                        )}
                        {Button(
                            ButtonProps::new("Re-verify")
                                .with_surface(ButtonSurface::Ghost)
                                .with_density(Density::Compact)
                                .with_disabled(Signal::derive(move || busy.get()))
                                .on_click(move |_| {
                                    let current = finding.get_untracked();
                                    let ref_id = current.ref_id.clone();
                                    if ref_id.is_empty() {
                                        set_status.set(
                                            "Re-verify needs an existing citation reference on the page."
                                                .to_string(),
                                        );
                                        return;
                                    }
                                    let request = ReverifyFindingRequest {
                                        wiki_id: wiki_id.clone(),
                                        title: title.clone(),
                                        rev_id: 0,
                                        ref_id,
                                        // Pin the exact use-site so a ref with several source URLs
                                        // re-verifies this card's source, not the first extracted.
                                        use_site_ordinal: Some(current.use_site_ordinal),
                                    };
                                    set_busy.set(true);
                                    set_status
                                        .set("Re-verifying against the current article state...".to_string());
                                    wasm_bindgen_futures::spawn_local(async move {
                                        match reverify_finding(&request).await {
                                            Ok(response) => {
                                                // Advance the baserevid for later actions to the
                                                // revision we actually verified against.
                                                set_base_rev_id.set(response.rev_id);
                                                finding.set(response.finding);
                                                set_status.set("Re-verified.".to_string());
                                            }
                                            Err(error) => set_status.set(format!("Re-verify error: {error}")),
                                        }
                                        set_busy.set(false);
                                    });
                                }),
                        )}
                    }
                    .into_any()
                })).with_gap(Gap::Small))
            }

            {move || {
                (!status.get().is_empty()).then(|| {
                    MetaText(MetaTextProps::new(ui_children(move || {
                        view! { {status.get()} }.into_any()
                    })))
                })
            }}

            {
                let wiki_id = wiki_id.clone();
                let title = title.clone();
                move || {
                    let wiki_id = wiki_id.clone();
                    let title = title.clone();
                    match open.get() {
                    OpenPanel::EditText => {
                        let edit_textarea_id = edit_textarea_id.clone();
                        let edit_id_for_save = edit_textarea_id.clone();
                        view! {
                            {MetaText(MetaTextProps::new(ui_children(|| {
                                view! {
                                    "The operator authors the replacement text — SP42 does not suggest one."
                                }
                                    .into_any()
                            })))}
                            {DiffEditPanel(
                                // Starts empty (PRD-0014 DoD) — SP42 never
                                // authors claim content in this domain, not
                                // even by pre-filling a starting point.
                                DiffEditPanelProps::new(edit_textarea_id, String::new(), ui_children(move || {
                                    let wiki_id = wiki_id.clone();
                                    let title = title.clone();
                                    let edit_id_for_save = edit_id_for_save.clone();
                                    view! {
                                        {Button(
                                            ButtonProps::new("Save edit")
                                                .with_tone(Tone::Success)
                                                .with_density(Density::Compact)
                                                .with_disabled(Signal::derive(move || busy.get()))
                                                .on_click(move |_| {
                                                    let claim = finding.get_untracked().claim.clone();
                                                    let replacement = textarea_value(&edit_id_for_save);
                                                    if replacement.trim().is_empty() {
                                                        set_status.set(
                                                            "Enter the replacement text before saving."
                                                                .to_string(),
                                                        );
                                                        return;
                                                    }
                                                    let request = SessionActionExecutionRequest {
                                                        wiki_id: wiki_id.clone(),
                                                        kind: SessionActionKind::InlineEdit,
                                                        rev_id: base_rev_id.get_untracked(),
                                                        title: Some(title.clone()),
                                                        target_user: None,
                                                        undo_after_rev_id: None,
                                                        summary: Some("SP42: inline edit".to_string()),
                                                        selected_text: Some(claim),
                                                        batch_rev_ids: None,
                                                        replacement_text: Some(replacement),
                                                        node_locator: None,
                                                        concern_kind: None,
                                                        reason: None,
                                                    };
                                                    set_busy.set(true);
                                                    set_status.set("Saving edit...".to_string());
                                                    wasm_bindgen_futures::spawn_local(async move {
                                                        match execute_dev_auth_action(&request).await {
                                                            Ok(response) if response.accepted => {
                                                                set_status.set(
                                                                    "Edit saved. Click Re-verify to check \
                                                                     whether it resolved the mismatch."
                                                                        .to_string(),
                                                                );
                                                                set_open.set(OpenPanel::None);
                                                            }
                                                            Ok(response) => {
                                                                set_status.set(format!(
                                                                    "Edit rejected: {}",
                                                                    response.message.unwrap_or_default()
                                                                ));
                                                            }
                                                            Err(error) => {
                                                                set_status.set(format!("Edit error: {error}"));
                                                            }
                                                        }
                                                        set_busy.set(false);
                                                    });
                                                }),
                                        )}
                                        {Button(
                                            ButtonProps::new("Cancel")
                                                .with_surface(ButtonSurface::Ghost)
                                                .with_density(Density::Compact)
                                                .on_click(move |_| set_open.set(OpenPanel::None)),
                                        )}
                                    }
                                    .into_any()
                                })),
                            )}
                        }
                            .into_any()
                    }
                    OpenPanel::FixCitation => {
                        view! {
                            <FixCitationPanel
                                finding=finding
                                wiki_id=wiki_id.clone()
                                title=title.clone()
                                rev_id=base_rev_id.get_untracked()
                                on_close=move || set_open.set(OpenPanel::None)
                                set_status=set_status
                            />
                        }
                            .into_any()
                    }
                    OpenPanel::FlagCitation => {
                        let current = finding.get();
                        let suggested = suggested_concern_kind(&current);
                        let suggested_value =
                            suggested.map(CitationConcernKind::label).unwrap_or_default();
                        let concern_select_id = concern_select_id.clone();
                        let concern_select_for_field = concern_select_id.clone();
                        let concern_select_for_confirm = concern_select_id.clone();
                        let reason_textarea_id = reason_textarea_id.clone();
                        let reason_id_for_confirm = reason_textarea_id.clone();
                        Stack(StackProps::new(ui_children(move || {
                            view! {
                                {Field(FieldProps::new(
                                    "Concern kind",
                                    ui_children(move || {
                                        view! {
                                            {Select(
                                                SelectProps::new(
                                                    concern_select_for_field.clone(),
                                                    concern_kind_options(),
                                                )
                                                    .with_value(suggested_value),
                                            )}
                                        }
                                            .into_any()
                                    }),
                                ))}
                                {DiffEditPanel(
                                    DiffEditPanelProps::new(
                                        reason_textarea_id,
                                        String::new(),
                                        ui_children(move || {
                                            let wiki_id = wiki_id.clone();
                                            let title = title.clone();
                                            let concern_select_for_confirm =
                                                concern_select_for_confirm.clone();
                                            let reason_id_for_confirm = reason_id_for_confirm.clone();
                                            view! {
                                                {Button(
                                                    ButtonProps::new("Confirm flag")
                                                        .with_tone(Tone::Warning)
                                                        .with_density(Density::Compact)
                                                        .with_disabled(Signal::derive(move || busy.get()))
                                                        .on_click(move |_| {
                                                            let current = finding.get_untracked();
                                                            let kind = resolve_concern_kind(
                                                                select_value(&concern_select_for_confirm)
                                                                    .as_deref(),
                                                                suggested_concern_kind(&current),
                                                            );
                                                            let Some(kind) = kind else {
                                                                set_status.set(
                                                                    "Choose a concern kind before confirming."
                                                                        .to_string(),
                                                                );
                                                                return;
                                                            };
                                                            if current.claim.trim().is_empty() {
                                                                set_status.set(
                                                                    "This finding has no claim text to flag."
                                                                        .to_string(),
                                                                );
                                                                return;
                                                            }
                                                            let reason = textarea_value(&reason_id_for_confirm);
                                                            let request = SessionActionExecutionRequest {
                                                                wiki_id: wiki_id.clone(),
                                                                kind: SessionActionKind::FlagCitation,
                                                                rev_id: base_rev_id.get_untracked(),
                                                                title: Some(title.clone()),
                                                                target_user: None,
                                                                undo_after_rev_id: None,
                                                                summary: None,
                                                                selected_text: Some(current.claim.clone()),
                                                                batch_rev_ids: None,
                                                                replacement_text: None,
                                                                node_locator: None,
                                                                concern_kind: Some(kind),
                                                                reason: (!reason.trim().is_empty())
                                                                    .then_some(reason),
                                                            };
                                                            set_busy.set(true);
                                                            set_status.set("Flagging citation...".to_string());
                                                            wasm_bindgen_futures::spawn_local(async move {
                                                                match execute_dev_auth_action(&request).await {
                                                                    Ok(response) if response.accepted => {
                                                                        set_status
                                                                            .set("Citation flagged.".to_string());
                                                                        set_open.set(OpenPanel::None);
                                                                    }
                                                                    Ok(response) => {
                                                                        set_status.set(format!(
                                                                            "Flag rejected: {}",
                                                                            response.message.unwrap_or_default()
                                                                        ));
                                                                    }
                                                                    Err(error) => {
                                                                        set_status
                                                                            .set(format!("Flag error: {error}"));
                                                                    }
                                                                }
                                                                set_busy.set(false);
                                                            });
                                                        }),
                                                )}
                                                {Button(
                                                    ButtonProps::new("Cancel")
                                                        .with_surface(ButtonSurface::Ghost)
                                                        .with_density(Density::Compact)
                                                        .on_click(move |_| set_open.set(OpenPanel::None)),
                                                )}
                                            }
                                                .into_any()
                                        }),
                                    ),
                                )}
                                {MetaText(MetaTextProps::new(ui_children(|| {
                                    view! {
                                        "Optional reason above (threaded into the template's reason= parameter)."
                                    }
                                        .into_any()
                                })))}
                            }
                            .into_any()
                        })).with_gap(Gap::Small))
                            .into_any()
                    }
                    OpenPanel::None => ().into_any(),
                }
                }
            }
        }
        .into_any()
    })).with_gap(Gap::Small))
}

/// "Fix citation" (PRD-0014): routes to PRD-0008's bare-URL repair replace
/// flow when `finding.ref_id` is non-empty, or reports the PRD-0012 insert
/// flow as not yet shipped when it's empty (Resolved question 1) — errors
/// informatively rather than hiding the button.
#[component]
fn FixCitationPanel(
    finding: RwSignal<CitationFinding>,
    wiki_id: String,
    title: String,
    rev_id: u64,
    on_close: impl Fn() + Send + Sync + 'static,
    set_status: WriteSignal<String>,
) -> impl IntoView {
    let ref_id = finding.get_untracked().ref_id.clone();
    let route = fix_citation_route(&ref_id);

    match route {
        FixCitationRoute::Insert => {
            Stack(StackProps::new(ui_children(move || {
                view! {
                    {MetaText(MetaTextProps::new(ui_children(|| {
                        view! {
                            "Citation insertion (PRD-0012) hasn't shipped yet, so Fix citation can't propose a citation for this unsourced claim. Use Edit article text or Flag citation instead."
                        }
                            .into_any()
                    })))}
                    {Button(
                        ButtonProps::new("Close")
                            .with_surface(ButtonSurface::Ghost)
                            .with_density(Density::Compact)
                            .on_click(move |_| on_close()),
                    )}
                }
                .into_any()
            })).with_gap(Gap::Small))
            .into_any()
        }
        FixCitationRoute::Replace => {
            let (proposal, set_proposal) = signal(None::<BareUrlApplyRequest>);
            let (preview, set_preview) = signal(None::<(String, String)>);
            let (loading, set_loading) = signal(true);
            let (busy, set_busy) = signal(false);
            let source_url = finding.get_untracked().provenance.url.to_string();

            let wiki_for_load = wiki_id.clone();
            let title_for_load = title.clone();
            let source_url_for_load = source_url.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match fetch_bare_url_proposals(&wiki_for_load, &title_for_load, rev_id).await {
                    Ok(response) => {
                        // A URL can appear on more than one reference; each yields its own
                        // proposal with its own locator. The finding identifies only the source
                        // URL, so if two references cite it, we can't tell which locator this card
                        // belongs to — applying the first would repair the wrong reference. Proceed
                        // only when the match is unique; otherwise refuse and point at the CLI,
                        // which addresses a reference by locator.
                        let mut matching = response
                            .proposals
                            .into_iter()
                            .filter(|p| p.url == source_url_for_load);
                        match (matching.next(), matching.next()) {
                            (Some(found), None) => {
                                set_preview.set(Some((
                                    found.current_anchor.clone(),
                                    found.replacement_wikitext.clone(),
                                )));
                                set_proposal.set(Some(BareUrlApplyRequest {
                                    wiki_id: wiki_for_load.clone(),
                                    title: title_for_load.clone(),
                                    rev_id,
                                    locator: found.locator,
                                    replacement_wikitext: found.replacement_wikitext,
                                    summary: None,
                                }));
                            }
                            (Some(_), Some(_)) => set_status.set(
                                "Fix citation: this page has more than one reference to the same \
                                 bare URL, so SP42 can't tell which one this finding points to. \
                                 Repair it from the CLI, which targets the reference by locator."
                                    .to_string(),
                            ),
                            (None, _) => {}
                        }
                    }
                    Err(error) => set_status.set(format!("Fix citation: {error}")),
                }
                set_loading.set(false);
            });

            let confirm_replace = move |_: leptos::ev::MouseEvent| {
                let Some(request) = proposal.get_untracked() else {
                    return;
                };
                set_busy.set(true);
                set_status.set("Applying bare-URL repair...".to_string());
                wasm_bindgen_futures::spawn_local(async move {
                    match apply_bare_url_proposal(&request).await {
                        Ok(response) if response.accepted => {
                            set_status.set(
                                "Citation repaired. Click Re-verify to check the fresh result."
                                    .to_string(),
                            );
                        }
                        Ok(response) => {
                            set_status.set(format!(
                                "Repair rejected: {}",
                                response.message.unwrap_or_default()
                            ));
                        }
                        Err(error) => set_status.set(format!("Repair error: {error}")),
                    }
                    set_busy.set(false);
                });
            };

            Stack(StackProps::new(ui_children(move || {
                view! {
                    {move || {
                        if loading.get() {
                            return MetaText(MetaTextProps::new(ui_children(|| {
                                view! { "Checking for a bare-URL repair proposal..." }.into_any()
                            })))
                                .into_any();
                        }
                        match preview.get() {
                            Some((before, after)) => {
                                view! {
                                    {EvidenceBlock(EvidenceBlockProps::new("Current", before))}
                                    {EvidenceBlock(
                                        EvidenceBlockProps::new("Proposed", after).with_tone(Tone::Success),
                                    )}
                                    {Button(
                                        ButtonProps::new("Confirm repair")
                                            .with_tone(Tone::Success)
                                            .with_density(Density::Compact)
                                            .with_disabled(Signal::derive(move || busy.get()))
                                            .on_click(confirm_replace),
                                    )}
                                }
                                    .into_any()
                            }
                            None => {
                                MetaText(MetaTextProps::new(ui_children(|| {
                                    view! {
                                        "No bare-URL repair proposal is available for this citation."
                                    }
                                        .into_any()
                                })))
                                    .into_any()
                            }
                        }
                    }}
                    {Button(
                        ButtonProps::new("Close")
                            .with_surface(ButtonSurface::Ghost)
                            .with_density(Density::Compact)
                            .on_click(move |_| on_close()),
                    )}
                }
                .into_any()
            })).with_gap(Gap::Small))
            .into_any()
        }
    }
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
    // Start the evidence expanded when the action row is about to sit right
    // beneath it, so the claim and located passage render side by side
    // instead of behind an extra click (PRD-0014 DoD).
    let evidence_open = finding_has_action_row(&finding);

    ResultCard(ResultCardProps::new(ui_children(move || {
        view! {
            {ResultCardHeader(
                ResultCardHeaderProps::new(ui_children(move || {
                    view! {
                        {Text(
                            TextProps::new(ui_children(move || {
                                view! { {format!("#{ordinal}")} }.into_any()
                            }))
                            .with_weight(TextWeight::Bold)
                        )}
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
                    )
                    .with_open(evidence_open),
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
                                        view! {
                                            {Text(TextProps::new(ui_children(move || {
                                                view! { {line} }.into_any()
                                            })))}
                                        }
                                        .into_any()
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

/// Reads a textarea element's current DOM value by id on demand (e.g. a Save
/// click), rather than tracking every keystroke through a signal — mirrors
/// the patrol rail's `DiffEditPanel` save handler (`diff_viewer.rs`).
#[cfg(target_arch = "wasm32")]
fn textarea_value(id: &str) -> String {
    use wasm_bindgen::JsCast;

    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .and_then(|element| element.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
        .map(|textarea| textarea.value())
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn textarea_value(_id: &str) -> String {
    String::new()
}

/// Reads a select element's current DOM value by id on demand, same rationale as
/// `textarea_value`. `None` when the element isn't found (not yet mounted).
#[cfg(target_arch = "wasm32")]
fn select_value(id: &str) -> Option<String> {
    use wasm_bindgen::JsCast;

    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .and_then(|element| element.dyn_into::<web_sys::HtmlSelectElement>().ok())
        .map(|select| select.value())
}

#[cfg(not(target_arch = "wasm32"))]
fn select_value(_id: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod action_row_tests {
    use super::{
        FixCitationRoute, finding_has_action_row, fix_citation_route, parse_concern_kind,
        resolve_concern_kind, suggested_concern_kind,
    };
    use sp42_core::{
        CitationConcernKind, CitationFinding, CitationFindingKind, CitationVerdict,
        GroundingAssertion, GroundingStatus, PanelAgreement, SourceProvenance, SupportLevel,
    };

    fn finding(verdict: CitationVerdict) -> CitationFinding {
        CitationFinding {
            kind: CitationFindingKind::CitationVerdict,
            verdict,
            grounding_status: GroundingStatus::Located,
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
            source_excerpt: None,
            metadata: None,
            grounding: GroundingAssertion::SourceFetched {
                source_hash: "hash".to_string(),
            },
            use_site_ordinal: 0,
            ref_id: "cite_note-1".to_string(),
            claim: "The article claims 6% growth.".to_string(),
            preceding_context: Vec::new(),
            archive_of: None,
            schema_version: 1,
        }
    }

    #[test]
    fn action_row_shows_only_for_partial_and_not_supported() {
        // PRD-0014 DoD: "An action row renders only for Partial/NotSupported
        // findings; Supported/SourceUnavailable findings remain read-only" —
        // verified over all four verdict fixtures.
        assert!(!finding_has_action_row(&finding(CitationVerdict::Judged(
            SupportLevel::Supported
        ))));
        assert!(finding_has_action_row(&finding(CitationVerdict::Judged(
            SupportLevel::Partial
        ))));
        assert!(finding_has_action_row(&finding(CitationVerdict::Judged(
            SupportLevel::NotSupported
        ))));
        assert!(!finding_has_action_row(&finding(
            CitationVerdict::SourceUnavailable
        )));
    }

    #[test]
    fn suggests_partial_support_for_partial_verdict() {
        let f = finding(CitationVerdict::Judged(SupportLevel::Partial));
        assert_eq!(
            suggested_concern_kind(&f),
            Some(CitationConcernKind::PartialSupport)
        );
    }

    #[test]
    fn suggests_failed_verification_for_not_supported_verdict() {
        let f = finding(CitationVerdict::Judged(SupportLevel::NotSupported));
        assert_eq!(
            suggested_concern_kind(&f),
            Some(CitationConcernKind::FailedVerification)
        );
    }

    #[test]
    fn suggests_nothing_for_supported_or_unavailable_verdicts() {
        assert_eq!(
            suggested_concern_kind(&finding(CitationVerdict::Judged(SupportLevel::Supported))),
            None
        );
        assert_eq!(
            suggested_concern_kind(&finding(CitationVerdict::SourceUnavailable)),
            None
        );
    }

    #[test]
    fn fix_citation_routes_to_replace_when_ref_id_present() {
        // PRD-0014 DoD: "'Fix citation' routes to the replace action when
        // finding.ref_id is non-empty ... verified by tests over both
        // finding shapes."
        assert_eq!(fix_citation_route("cite_note-1"), FixCitationRoute::Replace);
    }

    #[test]
    fn fix_citation_routes_to_insert_when_ref_id_empty() {
        assert_eq!(fix_citation_route(""), FixCitationRoute::Insert);
    }

    #[test]
    fn parse_concern_kind_round_trips_every_variant_label() {
        for kind in [
            CitationConcernKind::PartialSupport,
            CitationConcernKind::FailedVerification,
        ] {
            assert_eq!(parse_concern_kind(kind.label()), Some(kind));
        }
    }

    #[test]
    fn parse_concern_kind_rejects_unknown_value() {
        assert_eq!(parse_concern_kind("unknown-kind"), None);
    }

    #[test]
    fn operator_override_wins_over_suggestion() {
        // PRD-0014 DoD: "the operator can override the suggested
        // CitationConcernKind for any other wiki-configured one before
        // confirming ... the apply payload reflects the operator's choice,
        // not just the suggestion."
        let suggested = Some(CitationConcernKind::PartialSupport);
        let operator_choice = Some("failed-verification");
        assert_eq!(
            resolve_concern_kind(operator_choice, suggested),
            Some(CitationConcernKind::FailedVerification)
        );
    }

    #[test]
    fn falls_back_to_suggestion_when_operator_makes_no_selection() {
        let suggested = Some(CitationConcernKind::FailedVerification);
        assert_eq!(resolve_concern_kind(None, suggested), suggested);
    }

    #[test]
    fn resolves_to_none_when_neither_selected_nor_suggested() {
        assert_eq!(resolve_concern_kind(None, None), None);
    }
}
