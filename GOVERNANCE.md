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

## Amendments

Governance changes should be proposed through pull requests. Changes that alter
the Constitution follow the amendment process in `CONSTITUTION.md`.
