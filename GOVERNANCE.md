# Governance

SP42 is maintainer-led open source. Contributors are welcome, but the project is
still alpha software and its architecture, security model, and deployment path
are intentionally controlled.

## Roles

- **Maintainers** own project direction, review standards, releases, deployment,
  security triage, and repository settings.
- **Contributors** propose issues, documentation, tests, and code through pull
  requests.
- **Reviewers** may provide technical review, but merge authority remains with
  maintainers unless explicitly delegated.

## Decision Making

Most changes are accepted through normal pull request review. Maintainers decide
whether a PR is in scope, sufficiently tested, and aligned with the project
constitution.

Changes require an Architecture Decision Record when they affect:

- crate boundaries or shared contracts
- runtime configuration or deployment behavior
- authentication, authorization, cookies, CSRF, or token handling
- scoring policy, ranking behavior, or Wikimedia action semantics
- desktop packaging or release distribution
- persistent storage formats or public APIs

ADRs live in `docs/adr/` and should explain context, decision, alternatives,
and consequences.

A change also requires a Product Requirements Document when it changes an
operator- or editor-facing capability, workflow, visible decision, or
scoring/ranking/Wikimedia action semantics. PRDs live in `docs/prd/` and record
user-facing intent plus definition of done.

Public API or contract changes do not automatically require a PRD. Use an ADR
for structural contract changes; add a PRD only when the contract changes what
operators, editors, or external integrations can do or rely on.

## Collaboration Direction

SP42 is git-and-PR-based today, but a standing goal is that **non-expert
collaboration on detection rules, evaluation datasets, and evaluation runs
should not require git**. This extends the contributor-pool reasoning in
[ADR-0001 §1](docs/adr/0001-foundational-decisions.md) (a full-Rust codebase
narrows who can contribute code; non-code contribution must stay broad).
Authoring a rule, contributing or correcting a labeled case, and running an
evaluation should eventually be possible in-product, with the repository/CI path
and the in-product path sharing one enforcement mechanism (see PRD-0007's
embeddable verdict, callable from both repo/CI and the product).

This is **partially implemented, not merely aspirational**: SP42 already has a
canonical on-wiki public storage convention for personal and shared documents
(`sp42-core::wiki_storage` / `public_documents`, surfaced at
`/operator/storage/*`), and it already models shared **rule sets** and
**training datasets** as on-wiki document kinds — chosen for auditability,
transparency, and because data with no reason to be private is easiest to manage
in the open. A wiki page is a natural zero-git editing surface, so that
convention is the candidate substrate for the collaboration above. What is still
missing is the in-product authoring lifecycle, typed/validated training-dataset
documents, revision-pinned reads, and an authenticated write path (the last
gated on live Wikimedia integration, Phase 4) — so until those land, all
contribution remains PR-based as described below, and the reproducibility-pinned
evaluation corpus stays in a Git host in the interim (PRD-0007). Validity is
enforced not at wiki edit time (the wiki cannot run SP42's schema validator) but
by an SP42-side adoption gate that only admits a validated, pinned revision.

## Pull Request Rules

- External contributors work through forks or branches and open pull requests.
- `main` should remain releasable.
- Pull requests should be small, focused, and reviewable.
- Pull request descriptions must include validation notes, even when the only
  validation is a documented reason why a check was not run.
- Maintainers may ask for tests, docs, or a narrower scope before review.
- Self-merge is not allowed for protected files or release/deployment changes.

## Protected Areas

The following areas need maintainer review before merge:

- `.github/`
- `CONSTITUTION.md`, `GOVERNANCE.md`, `SECURITY.md`, `CONTRIBUTING.md`
- `crates/sp42-server/`
- authentication/session/runtime configuration code
- desktop packaging and Tauri configuration
- deployment scripts and VPS packaging
- schemas, configs, and ADRs

Maintainers enforce these review boundaries through branch protection, review
policy, and release/deployment access controls.

## Contributor Issue Labels

Use `good first issue` only for work that can be completed without deployment
credentials, architectural authority, signing certificates, Wikimedia Cloud VPS
access, or private maintainer context.

## Access And Secrets

Write access, release access, Wikimedia Cloud VPS access, signing credentials,
and any other secrets are maintainer-only by default. Contributors should be
able to build and test locally without access to production infrastructure.

## Releases

Releases are cut by maintainers. Unsigned artifacts may be produced by CI first;
signed desktop artifacts and VPS deployments require explicit maintainer
approval.

Before any release commit or tag, maintainers run:

```sh
./scripts/audit-release-tree.sh
```

The audit fails if generated build output is tracked, Tauri sidecar binaries are
not ignored, local runtime directories are not ignored, or non-ignored untracked
files are present.

Maintainers should install the repository hooks with:

```sh
./scripts/install-git-hooks.sh
```

The repository uses local Husky-compatible hooks under `.husky/`, not GitHub
Actions, for this pre-commit/pre-push enforcement. The `pre-push` hook runs
`./scripts/ci-all.sh`, so pushes are intentionally slower than normal commits.

## Amendments

Governance changes should be proposed through pull requests. Changes that alter
the Constitution follow the amendment process in `CONSTITUTION.md`.
