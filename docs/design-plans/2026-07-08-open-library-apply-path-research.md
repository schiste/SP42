# Open Library apply-path research (PRD-0009 Layer 3 prerequisite)

**Date:** 2026-07-08
**Author:** Luis Villa (researched by Claude Code)
**Status:** Research note — the findings ADR-0019 (Open Library apply
contract, PRD-0009 resolved Q6(b)) is drafted from. No write code exists or
is enabled; Layers 1–2 (ADR-0018) are unaffected.

## The question

PRD-0009's enrichment lane (Layer 3) needs a **production write mechanism an
ordinary operator account can actually use** before the apply-contract ADR can
be drafted. The PRD deferred with: "the documented REST save `PUT` path is
internal/localhost-only," flagging the mechanism as unknown. This note answers:
what can a normal Open Library account write, through what endpoint, with what
authentication, and under what policy?

## Method

Read the primary sources, not summaries: the official
`internetarchive/openlibrary-client` library (login + save implementation and
README), the **server-side permission code** (infogami `plugins/api/code.py`,
which implements the RESTful save and `save_many`), the website edit-form
handler (`openlibrary/plugins/upstream/addbook.py`, `SaveBookHelper`), the
Writing Bots policy page, and the open rate-limiting issues
(internetarchive/openlibrary#8534, #10585, plus the #11628 `save_many` 403
report).

## Findings

### 1. The PRD's "localhost-only" premise is wrong — the real gate is a usergroup

The RESTful save (`PUT /books/OL…M.json`, `PUT /authors/OL…A.json`) and the
bulk `POST /api/save_many` **do run against production
`https://openlibrary.org`** — the official `openlibrary-client` targets
production by default and the whole `openlibrary-bots` ecosystem writes
through it. The docs' localhost examples are dev-instance examples, not a
deployment restriction.

The actual restriction is **server-side permission**: in infogami's API plugin
(`infogami/plugins/api/code.py`), *every* API write — the single-document PUT
and `save_many` alike — runs `can_write()`, which passes only when the
logged-in account is a member of **`/usergroup/api`** or `/usergroup/admin`.
An ordinary account gets `403 Permission Denied` (exactly what
openlibrary#11628 reports for `save_many`). There is no volume carve-out: the
machine-API lane is closed to ordinary accounts even for a single edit.

Corrected statement for the PRD/ADR: *the REST save path is
production-real but usergroup-gated; it is not localhost-only.* The PRD's
conclusion (don't assume it) stands; its reason changes.

### 2. Ordinary accounts write through the website form path

Human editors — the wiki model the PRD leans on — save through
`POST /books/OL…M/edit` (handler `book_edit` → `SaveBookHelper`; same shape
for `/works/OL…W/edit`). Verified properties from the handler source:

- **Permission: a logged-in account + per-record `web.ctx.site.can_write`** —
  the ordinary editability every human editor already has, no usergroup.
- **Field contract:** flattened form fields with `edition.*` / `work.*`
  prefixes (`SaveBookHelper.process_input` splits them); a work edit rides an
  edition edit only when the work key matches.
- **Edit comment:** `_comment` field, committed with `action="edit-book"` —
  attribution and comment survive exactly as PRD-0009 requires.
- **No CSRF token** in the current handler (nothing to mint, but also a
  contract that could change under us — see risks).
- **Versioning:** the form carries a `v` (revision) parameter and the handler
  loads that revision, but there is **no explicit refuse-on-conflict**; drift
  protection must be SP42-side (compare the record's `revision`/`last_modified`
  read at proposal time against a re-read immediately before apply, and refuse
  if moved — the ADR-0010 discipline implemented by the client, not the
  server).

### 3. The API usergroup is obtainable, by a human process, framed for bots

The sanctioned path into `/usergroup/api`: open a GitHub issue / write to the
admins describing what the account will do; bulk-cleanup accounts are expected
to be **separate accounts named `…Bot`**, and the recommended tooling is
`openlibrary-client`. Membership is public — `GET /usergroup/api.json` lists
members — so SP42 can **capability-probe without a write**: read the
usergroup, check whether the operator's key is in it.

For SP42 this is a plausible *per-operator opt-in* (an operator doing
sustained enrichment asks for membership, possibly via a paired bot account),
not an assumption the product can make for every operator.

### 4. Authentication (either lane): session cookie via `/account/login`

`POST /account/login` accepts form-encoded username/password **or** JSON
Internet Archive **S3 keys** (`access`/`secret` — any account can mint them at
`archive.org/account/s3.php`); either yields the session cookie all writes
ride. S3 keys are the better operator story (no password storage, revocable),
are what `openlibrary-client` uses, and remain strictly **per-operator**
(PRD-0009 resolved Q1).

### 5. Rate limits: none published; policy is an open upstream discussion

There are no published write limits; formal rate-limit policy is an open
upstream issue (openlibrary#8534, #10585 — the only number in play is a
*proposal* to limit unidentified traffic). Etiquette expectations: identify
via User-Agent, keep frequency low. SP42's posture already covers this: the
guarded fetch edge's UA and pacing (ADR-0015), ≤3 concurrency on third-party
REST, and Layer 3 being operator-confirmed one-field-at-a-time makes writes
human-paced by construction.

## Implication for the apply-contract ADR

*(Adopted: ADR-0019 records this contract.)* The ADR can be drafted with a
**two-lane apply contract**, both per-operator:

1. **Default lane — authenticated form POST** (`/books/OL…M/edit`): works for
   every ordinary operator account today. SP42 renders the proposal, the
   operator confirms, SP42 replays the record's current fields + the confirmed
   field change + `_comment` through the form contract, with an SP42-side
   refuse-on-drift (re-read `revision` before apply). This is exactly the
   "controlled browser/form-backed submit path" PRD-0009 reserved.
2. **Privileged lane — REST `PUT` with `_comment`** for operators whose
   account is in `/usergroup/api`: cleaner JSON contract, the officially
   recommended machine lane. (As adopted, ADR-0019 selects the lane from the
   server's own pre-mutation 403 answer with per-session caching, rather
   than reading the `/usergroup/api.json` membership list — the list read
   remains a valid display-only capability hint, but the write path never
   depends on it.)

Both lanes keep Layer 3 proposal-only until the ADR lands and the form-lane
spike (below) passes.

## Risks / open items for the ADR

- **The form contract is not a published API.** Field names come from
  templates and can change without notice; the ADR must treat the form lane
  as a versioned adapter with fixture-replay tests and a fail-closed posture
  (any contract drift → refuse and fall back to proposal-only).
- **One-time live spike needed** (manual, testable account): confirm the form
  POST's exact field set for a minimal single-field edit, confirm `_comment`
  lands in history, confirm behavior when `v` is stale (merge vs overwrite) —
  this decides how strict the SP42-side drift refusal must be.
- **CSRF could be added upstream** at any time; the adapter must read the
  edit form first (GET) and replay any hidden fields it finds, rather than
  posting blind.
- **Community posture:** SP42 assisted edits are operator-confirmed, not bulk;
  still, telling the Open Library team about SP42 (the same channel as the
  usergroup request) is cheap goodwill and may yield a sanctioned path — do it
  before enabling any write lane (mirrors PRD-0008's frwiki-gate posture).
- `/api/import` remains out of scope (enrich-existing-only, resolved Q2).

## Sources

- `internetarchive/openlibrary-client` — `olclient/openlibrary.py` (login via
  `POST /account/login` with credentials or S3 keys; `PUT /books/{olid}.json`
  with `_comment`; `POST /api/save_many`; default host
  `https://openlibrary.org`) and README (S3 key configuration).
- `internetarchive/infogami` — `infogami/plugins/api/code.py` (`can_write()`
  gating both PUT and `save_many` on `/usergroup/api` ∪ `/usergroup/admin`).
- `internetarchive/openlibrary` — `openlibrary/plugins/upstream/addbook.py`
  (`book_edit`/`work_edit`/`SaveBookHelper`: logged-in + per-record
  `can_write`, `edition.*`/`work.*` field split, `_comment`,
  `action="edit-book"`, `v` revision parameter, no CSRF token, no server-side
  conflict refusal); issues #11628 (`save_many` 403 in production), #8534 and
  #10585 (rate-limit policy open).
- Open Library *Writing Bots* documentation (API-usergroup grant process, bot
  account naming, `openlibrary-client` recommendation).
