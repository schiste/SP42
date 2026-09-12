# Cite-Template Repair Design

## Summary
<!-- TO BE GENERATED after body is written -->

## Definition of Done

1. **Detector**: a malformation detector over parsed cite-template parameters, with
   the rule set defined per-wiki as data (enwiki CS1 rules shipped first; other
   wikis are config, not code). First-cut rules: `url-status` without
   `archive-url` (both branches) and `date`/`year` conflict. (`postscript` was
   dropped from the first cut during design: it is an active CS1 parameter, not
   an obsolete one, so blanket removal is unsafe; it may return later as a
   narrowly-scoped redundant-value rule.)
2. **Integration**: findings ride the `verify_page` page report as repair
   candidates; a standalone read-only scan verb exists as a byproduct of the
   test framework.
3. **Repair flow**: proposals are minimal node-scoped parameter edits — including
   a new *remove-parameter* primitive for `WikitextEditor` — through the
   ADR-0010 propose/confirm path, dry-run default, testwiki-gated writes via the
   PRD-0008 config pattern. The dead-URL branch discovers an existing Wayback
   snapshot (read-only, never Save-Page-Now) and proposes the
   `archive-url`/`archive-date`/`url-status` triple as one edit.
4. **Shared seam**: the propose/confirm spine (preview/execute route pair,
   session+CSRF, dry-run default, testwiki config gating, baserevid anti-drift
   replay) is extracted into a shared repair-proposal contract that both the
   shipped bare-URL repair and the new cite-template repair feed into — rather
   than duplicating PRD-0008's routes a second time.
5. **Surfaces**: both designed now — CLI `preview`/`execute` verbs implemented
   first, PRD-0014 action-row integration as a later phase on the same server
   flow.
6. **Verification**: synthetic + real fixtures (drawn from CS1 maintenance
   categories), node-scoped-diff property test, write-gate test,
   archive-read-only property test.
7. **Recorded boundary**: archive-parameter edits happen only in service of
   repairing a CS1-tracked template malformation, human-confirmed; generic
   dead-link repair remains IABot's territory (see issue #29 close rationale).

## Glossary
<!-- TO BE GENERATED after body is written -->
