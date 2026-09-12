# Citation Envelope Replay Regression Suite Design

## Summary
<!-- TO BE GENERATED after body is written -->

## Definition of Done

- A Rust binary that replays each fixture pair (`SnapshotEnvelope` + `VerdictEnvelope`) through
  the current pipeline (`assemble_citation_finding`), reconstructing `ModelVerdict` from the
  stored `ModelVote` list, and compares the rebuilt `VerdictEnvelope` against the stored one.
- Two modes: check (default, used by CI — reports mismatches and exits nonzero) and an explicit
  regenerate mode that rewrites fixtures when a change is intentional.
- A thin `scripts/replay-citation-envelopes.sh` wrapper (`--check`) wired into `ci.yml`'s
  `checks` job, matching the architecture-map gate idiom.
- A starter set of ~8-12 handwritten synthetic envelope-pair fixtures under a new
  `fixtures/citation-replay/` dir, covering all four verdict outcomes (`supported`, `partial`,
  `not_supported`, `source_unavailable`) and located/unlocated grounding — structured so real
  fetched-page pairs can be added later without a format change.
- Out of scope: PRD-0007's model-quality benchmarking, and the separate in-progress eval-corpus
  labeling effort (`docs/CITATION_EVAL_CORPUS.md`, `scripts/eval-corpus/`).

## Glossary
<!-- TO BE GENERATED after body is written -->
