# Bare-URL Repair MVP Implementation Plan — Phase 7: Records and fold-backs

> **For Claude:** REQUIRED SUB-SKILL: Use ed3d-plan-and-execute:executing-an-implementation-plan to implement this plan task-by-task.

**Goal:** The paper trail matches the code: PRD-0008 gains the "Proposed CLI surface" section, the thin propose/confirm contract ADR is drafted as **ADR-0010**, the "PRDs contain proposed CLI surfaces" convention issue is prepared (and filed **only with the operator's explicit go-ahead**), and `docs/STATUS.md` records the slice. `scripts/check-doc-consistency.sh` stays green.

**Architecture:** Docs only — no code. ADR numbering verified: merged ADRs run through 0005; PRD-0008 itself records that the citation-verification branch holds unmerged drafts through ADR-0009 and that this ADR takes its number at draft time ⇒ **0010**.

**Scope:** Phase 7 of 7. Depends on Phases 1–6 (records describe what shipped — verify the flag/route names against the code as built before writing them down).

**Codebase verified:** 2026-06-09, branch `louie/bare-url-repair` @ `2ed57b3`.

---

**Working directory for every command:** `/var/home/louie/Projects/Volunteering-Consulting/SP42/.worktrees/bare-url-repair`

### Task 1: PRD-0008 "Proposed CLI surface" section

**Files:**
- Modify: `docs/domains/references/prd/0008-bare-url-repair.md`

**Step 1: Insert the section**

PRD-0008's headings are: `## Scope boundary`, `## Problem`, `## Proposal`, `## Definition of Done`, `## Alternatives`, `## Risks`, `## Open questions`. Insert the new section **between `## Proposal` and `## Definition of Done`**:

````markdown
## Proposed CLI surface

The MVP's operator surface is two mutually-exclusive CLI flag-modes over the
dev bridge (ADR-0002), following the house flag-mode pattern (decided by the
Editor mid-design, 2026-06-09):

```text
--bare-url-preview --title <T> --rev <N> [--wiki <ID>] [--bridge-base-url <URL>] [--format text|json|markdown]
--bare-url-execute --title <T> --rev <N> --ordinal <K> [--wiki <ID>] [--action-note <summary>] [--bridge-base-url <URL>] [--format text|json|markdown]
```

- `--bare-url-preview` calls `POST /dev/citation/bare-url-proposals` and
  renders the revision's `{proposals, declined}`; it is read-only and needs
  no session.
- `--bare-url-execute` re-fetches the proposals, selects ordinal `<K>`, and
  replays exactly that proposal against `POST /dev/citation/bare-url-apply`
  under the operator's bridge session (bootstrap + CSRF token). The fresh
  fetch re-anchors the locator; the server's anti-drift re-check and
  `baserevid` guard refuse on any race (`node-drift` / `node-out-of-range`,
  zero writes).
- `--wiki` defaults to `testwiki`, the only wiki the MVP enables.
- `--action-note` wins over the default edit summary `SP42: bare-URL repair`.
- Declined references render with their reason codes (`metadata-unavailable`,
  `no-usable-title`) so the operator sees why a bare URL kept its finding.
````

**Step 2: Verify section accuracy against the built code**

```bash
grep -n '"--bare-url-preview"\|"--bare-url-execute"\|"--wiki"\|"--ordinal"' crates/sp42-cli/src/main.rs
grep -n "DEV_CITATION_BARE_URL" crates/sp42-core/src/routes.rs
```

Expected: every flag and route named in the section exists. If Phase 6 changed any name, the PRD text follows the code.

**Step 3: Commit**

```bash
git add docs/domains/references/prd/0008-bare-url-repair.md
git commit -m "docs: fold the proposed CLI surface back into PRD-0008"
```

### Task 2: ADR-0010 — operator-confirmed content proposals

**Files:**
- Create: `docs/platform/adr/0010-operator-confirmed-content-proposals.md`

**Step 1: Create the ADR** (template: ADR-0003's structure — Status/Date/Author header, Context, Decision, Consequences, Non-Goals):

```markdown
# ADR-0010: Operator-confirmed content proposals (propose/confirm)

**Status:** Proposed
**Date:** 2026-06-09
**Author:** Luis Villa

Spawned by PRD-0008 (bare-URL repair), which reserved this number at draft
time because the citation-verification series holds unmerged drafts through
ADR-0009.

## Context

PRD-0008 needs SP42 to *generate* article content (a filled citation
template) rather than only tag or revert it. Nothing generated may reach a
wiki without the operator seeing and confirming the exact bytes. ADR-0003
already gives us drift-guarded, node-anchored edits; ADR-0002 gives us a
bridge session that keeps tokens out of client processes. What was missing
is the contract for how a generated edit travels between "proposed" and
"applied".

## Decision

1. **Two-step propose/confirm.** A read-only proposal route computes
   `{proposals, declined}` for a revision; a separate authenticated apply
   route performs the write. Nothing is written during proposal generation.
2. **Proposals are replayable edit payloads.** Each proposal carries the
   ADR-0003 `WikitextNodeLocator` (kind, ordinal, `expected_text` anchor)
   plus the complete `replacement_wikitext`. Apply replays that payload
   verbatim — the server re-runs the anti-drift re-check and sends
   `baserevid`, so a changed article refuses (`node-drift` /
   `node-out-of-range`, HTTP 400 with `http_status: 409` in the body, zero
   wiki writes) rather than guessing.
3. **Declines are structured outcomes, not errors.** A reference that cannot
   get a usable proposal (`metadata-unavailable`, `no-usable-title`) stays a
   finding; one junk URL never fails a whole proposal response.
4. **Per-wiki presence gate.** A wiki opts in by naming the template in its
   config (`WikiTemplates.bare_url_citation`); the same check guards both
   routes (`bare-url-repair-not-enabled`). Production configs simply omit
   the key.
5. **Wire contracts live in `sp42-core`.** Request/response/proposal types
   are shared serde types (the `action_contracts` precedent), so the server
   and every shell speak one contract.

## Consequences

- Any future "SP42 writes content" feature (citation repair from the
  verification pipeline, template fixes, typo repair) can reuse the same
  propose/confirm shape: locator + replacement + verbatim replay.
- The apply path inherits ADR-0003's guarantees wholesale; there is no new
  write machinery to audit.
- The proposal payload is self-describing, so a CLI, browser, or desktop
  shell can render a faithful "before/after" without server round-trips.
- Replaying stale proposals is safe by construction (refusal, not
  mis-targeting), at the cost of operators occasionally re-running preview.

## Non-Goals

- Batch or automatic application of proposals — every apply is one
  operator-confirmed payload.
- Production-wiki enablement (testwiki only in the MVP; frwiki enablement is
  a follow-on with per-wiki template/language mapping).
- Action-history logging for bare-URL applies (MVP omission, noted in the
  implementation plan).
```

**Step 2: Verify doc consistency**

```bash
bash scripts/check-doc-consistency.sh
```

Expected: exits 0.

**Step 3: Commit**

```bash
git add docs/platform/adr/0010-operator-confirmed-content-proposals.md
git commit -m "docs: draft ADR-0010 operator-confirmed content proposals"
```

### Task 3: STATUS.md bullet

**Files:**
- Modify: `docs/STATUS.md`

**Step 1: Add the bullet**

In `docs/STATUS.md`, the Phase 5 section ends with the ADR-0003 bullet:

```markdown
- node-anchored wikitext editing (ADR-0003) is implemented: a `WikitextEditor`
  contract with a Parsoid-backed adapter; `InlineEdit` accepts an optional
  node locator, and the literal fallback refuses ambiguous matches
```

Immediately after it, add:

```markdown
- bare-URL reference repair (PRD-0008) has a testwiki-gated propose/confirm
  slice: Citoid-backed citation proposals and verbatim-replay applies over
  `/dev/citation/*` bridge routes, with CLI preview/execute flag-modes
  (ADR-0010); the live test.wikipedia.org repair gate remains manual
```

(Do **not** modify any existing line — `check-doc-consistency.sh` matches several of them exactly.)

**Step 2: Verify**

```bash
bash scripts/check-doc-consistency.sh
```

Expected: exits 0.

**Step 3: Commit**

```bash
git add docs/STATUS.md
git commit -m "docs: record the bare-URL repair slice in STATUS"
```

### Task 4: The PRD-convention issue — draft, then STOP for the operator

The design records the Editor's instruction to file a GitHub issue proposing
a "PRDs contain proposed CLI surfaces" convention for `docs/process/prd-protocol.md`.
**Filing a GitHub issue is an outward-facing artifact: do not file it
autonomously.** Draft it, then ask the operator.

**Step 1: Prepare the issue draft**

Title:

```text
PRD convention: add a "Proposed CLI surface" section expectation to docs/process/prd-protocol.md
```

Body:

```markdown
PRD-0008 (bare-URL repair) folded its operator-facing CLI surface back into
the PRD as a `## Proposed CLI surface` section: the flag-modes, their
required/optional arguments, which bridge routes they drive, and how
refusals surface to the operator.

Proposal: make this a documented convention in `docs/process/prd-protocol.md` — a PRD
that introduces or changes an operator-facing capability with a CLI-reachable
surface should carry a "Proposed CLI surface" section while in
Draft/Discussion, kept in sync with the shipped flags by the closing PR.

Why:
- The CLI surface *is* user-facing intent for CLI-first features, so it
  belongs in the PRD next to the Definition of Done, not only in code.
- Reviewers can react to flag semantics (defaults, mutual exclusions,
  output formats) before implementation hardens them.
- The PRD lifecycle already requires Definition-of-Done items to map to
  observables; a recorded CLI surface gives those items a concrete shape.

First instance: PRD-0008's section (added with the bare-URL repair MVP);
see also the flag-mode precedents in the CLI.
```

**Step 2: STOP — ask the operator (REQUIRED)**

Use AskUserQuestion: present the draft and ask Luis to either (a) file it
himself (web or `gh issue create --repo schiste/SP42`), or (b) explicitly
authorize you to run the `gh issue create` command with this exact title and
body. Do not proceed past this step without one of those outcomes. If the
operator defers, leave Task 4 incomplete and note it in the phase summary —
Tasks 1–3 and 5 do not depend on it.

**Step 3: Record the issue URL in the PRD discussion trail**

Once the issue exists at `https://github.com/schiste/SP42/issues/<N>`,
append to the **end of the `## Proposed CLI surface` section** added in
Task 1:

```markdown
The "PRDs contain proposed CLI surfaces" convention proposal for
`docs/process/prd-protocol.md` is tracked in
https://github.com/schiste/SP42/issues/<N>.
```

(substituting the real issue number).

**Step 4: Commit**

```bash
git add docs/domains/references/prd/0008-bare-url-repair.md
git commit -m "docs: link the PRD CLI-surface convention issue from PRD-0008"
```

### Task 5: Final branch verification

**Step 1: Run the focused suite**

```bash
bash scripts/check-doc-consistency.sh
cargo fmt --all
./scripts/check-focused.sh
```

Expected: doc consistency green; `cargo fmt` produces no diff (if it does,
inspect, include the formatting in a `style:` commit, and re-run); the
focused check/test/trunk pipeline passes.

**Step 2: Confirm the branch state**

```bash
git status --short
git log --oneline main..HEAD
```

Expected: clean tree; the commit list covers Phases 1–7 (Citoid lift, config
gate, core module, both routes, CLI flag-modes, docs).

**Done means:** tests green workspace-wide for the touched crates, clippy
clean with `-D warnings`, doc-consistency green, and the only outstanding
item being (at most) the operator-gated issue filing from Task 4 — plus the
PRD's live test.wikipedia.org repair, which is explicitly **outside this
plan** and runs manually with an operator at the keyboard.
