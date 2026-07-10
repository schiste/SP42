# Attic

Historical snapshots, kept for the record and unmaintained. Documents here
describe the repository **as it was when they were written** — crate names,
module paths, statuses, and instructions may no longer match reality. Nothing
in `attic/` is a contract, a plan of record, or a place to add new work.

A document moves here when the work it drove has shipped and every decision it
contains has been folded into a living home (an ADR, a PRD, or the code). The
living documentation is:

- [docs/](../docs/README.md) — current architecture, domains, process
- [docs/platform/architecture.md](../docs/platform/architecture.md) — generated
  map of crates, layers, ADRs, and PRDs

## Contents

- `implementation-notes/` — research and working notes from the citation
  verification build (2026-06-08/09). The code they describe now lives in
  `crates/sp42-citation` (citation logic) and `crates/sp42-inference` (LLM
  adapter); the ADR amendments they proposed are folded into ADR-0006–0009.
- `implementation-plans/` — phase-by-phase build plans for features that
  shipped: bare-URL repair, node-anchored editing, citation claim context,
  article citation extractor, unusable-source detection.
- `design-plans/` — design plans whose work shipped and whose decisions live
  in the linked ADRs/PRDs. Design plans that still spec unbuilt work remain in
  [docs/design-plans/](../docs/design-plans/).
