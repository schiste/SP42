#!/usr/bin/env bash
# Cross-document consistency gate.
#
# This used to assert that nine fixed literal strings still existed in README.md
# and docs/STATUS.md. That is a presence check, not a consistency check: it
# cannot detect either of the two failure modes it was nominally guarding.
#
#   1. A document making a claim that is no longer true. (CONTRIBUTING.md
#      asserted the supply-chain gate was "currently red" for months after
#      deny.toml made it green — the literal was still present, so it passed.)
#   2. Two documents contradicting each other. (GOVERNANCE.md permitted
#      self-merge while CONSTITUTION.md Art. 8.3 prohibited it outright. Both
#      sentences existed, so both checks passed.)
#
# The checks below are therefore assertions about *relationships* — a claim that
# must agree with the code, a claim that must agree with another document, a
# manifest that must match reality — rather than assertions that a particular
# sentence is still typed. Where a relationship cannot be verified, that is
# reported as an explicit allowlist entry with a reason, so the gap is visible
# instead of silently unchecked.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

violations=0

fail() {
  printf '  %s\n' "$1" >&2
  violations=$((violations + 1))
}

require_line() {
  local file="$1" needle="$2"
  if ! grep -Fq -- "$needle" "$file"; then
    fail "missing required line in ${file}: ${needle}"
  fi
}

forbid_line() {
  local file="$1" needle="$2" why="$3"
  if grep -Fq -- "$needle" "$file"; then
    fail "${file} still asserts: ${needle} — ${why}"
  fi
}

# ── 1. README/STATUS headline claims stay in lockstep ───────────────────────
# Retained from the previous version: these are genuine "must not silently
# change" anchors, and the README deliberately delegates the timeline to STATUS.
require_line "README.md" '- Live Wikimedia integration is still gated by external credentials and verification'
require_line "README.md" '- Multi-user production auth is not implemented yet'
require_line "README.md" '[docs/STATUS.md](docs/STATUS.md)'

require_line "docs/STATUS.md" 'The offline patrol engine is now effectively complete for local development:'
require_line "docs/STATUS.md" 'Coordination and shared runtime state are now effectively complete for local development:'
require_line "docs/STATUS.md" 'Target shells are now effectively complete for local development and include an interactive patrol rail:'
require_line "docs/STATUS.md" 'Live Wikimedia integration is still gated by external credentials and verification:'
require_line "docs/STATUS.md" 'PWA packaging and offline installability are now effectively complete for local development:'

require_line "docs/platform/DEVELOPER_SURFACE.md" '- it includes a PWA shell for installability, update activation, iOS guidance, and offline-safe shell behavior'
require_line "docs/platform/DEVELOPER_SURFACE.md" '- PWA shell, offline fallback, manifest shortcuts, and telemetry surfaces'

# ── 2. No document may assert a known-stale fact ───────────────────────────
# Each entry here is a claim that has been true at some point and would mislead
# a reader now. Removing the sentence is the fix; re-adding it is the regression.
forbid_line "CONTRIBUTING.md" 'currently red on `main`' \
  'the supply-chain gate is green — deny.toml records the three unfixable advisories with reasons'
forbid_line "CONTRIBUTING.md" 'pushing requires `SP42_SKIP_GIT_HOOKS=1`' \
  'no push currently requires bypassing the hooks'

# ── 3. Constitution must not contradict itself or other documents ───────────
# Art. 8.3 (no self-merge) vs the older Governance carve-out.
if grep -Eq 'Self-merge is not allowed for (protected|release)' GOVERNANCE.md; then
  fail 'GOVERNANCE.md permits a self-merge carve-out that CONSTITUTION.md Art. 8.3 prohibits outright'
fi
require_line "CONSTITUTION.md" 'Self-merge prohibited'

# The Constitution's testing tiers must not promise a gate that does not exist.
# Art. 1.2 names an `integration` feature; if a crate ever declares it, the
# disclaimer above it must be updated in the same change.
if grep -rqs --include='Cargo.toml' 'integration' crates xtask; then
  if grep -Fq 'Not yet implemented' CONSTITUTION.md; then
    fail 'an `integration` feature now exists but CONSTITUTION.md Art. 1.2 still says the tier is not implemented'
  fi
fi

# Art. 6.2 must name the file the traits actually live in.
if [[ -f crates/sp42-platform/src/traits.rs ]] && grep -Fq 'declare' crates/sp42-platform/src/traits.rs 2>/dev/null; then
  fail 'crates/sp42-platform/src/traits.rs holds declarations again — CONSTITUTION.md Art. 6.2 needs updating'
fi

# ── 4. Documented commands must exist ───────────────────────────────────────
# CONTRIBUTING and README tell contributors to run scripts. A renamed or deleted
# script is a broken instruction, and this catches it without anyone reading
# both documents.
for doc in README.md CONTRIBUTING.md docs/STATUS.md; do
  # Extract `./scripts/<name>.sh` references and check each one is executable.
  grep -oE '\./scripts/[A-Za-z0-9._-]+\.sh' "$doc" 2>/dev/null | sort -u | while read -r script; do
    if [[ ! -f "$script" ]]; then
      printf '  %s references %s, which does not exist\n' "$doc" "$script" >&2
      exit 1
    fi
    if [[ ! -x "$script" ]]; then
      printf '  %s references %s, which is not executable\n' "$doc" "$script" >&2
      exit 1
    fi
  done || exit 1
done

# ── 5. Every workspace crate is documented ─────────────────────────────────
# The README's Repository Layout is the map contributors navigate by. A crate
# that exists but is absent from it is invisible to anyone using the docs.
while read -r crate_dir; do
  crate="$(basename "$crate_dir")"
  if ! grep -Fq "crates/${crate}" README.md; then
    fail "crate ${crate} exists but is not mentioned in README.md's Repository Layout"
  fi
done < <(/usr/bin/find crates -mindepth 1 -maxdepth 1 -type d | sort)

# ── 6. ADR status discipline ───────────────────────────────────────────────
# Every ADR needs a Summary line (adr-protocol.md), and an Accepted ADR must
# carry the four sections Art. 4.1 requires: context, decision, alternatives,
# consequences.
#
# ART_4_1_GRANDFATHERED lists the accepted ADRs that predate this check and are
# missing the Alternatives section. They are listed rather than fixed because
# the honest fix is to record what was actually considered, and for a decision
# already taken that history is not recoverable — inventing an Alternatives
# section now would be fabricating a decision record, which is the precise thing
# an ADR exists to prevent. Removing an entry from this list requires either
# adding the section or an ADR explaining why it does not apply.
ART_4_1_GRANDFATHERED=" 0001 0002 0010 0011 0016 0018 0020 0021 0022 0023 "

while read -r adr; do
  adr_id="$(basename "$adr" | sed -nE 's/^([0-9]{4})-.*/\1/p')"
  if [[ -z "$adr_id" ]]; then
    continue
  fi
  if ! grep -Fq '**Summary:**' "$adr"; then
    fail "${adr} has no '**Summary:**' line (required by docs/process/adr-protocol.md)"
  fi
  if grep -Fq '**Status:** Accepted' "$adr"; then
    for section in '## Context' '## Decision' '## Alternatives' '## Consequences'; do
      if grep -Fq "$section" "$adr"; then
        continue
      fi
      if [[ "$section" == '## Alternatives' && "$ART_4_1_GRANDFATHERED" == *" $adr_id "* ]]; then
        continue
      fi
      fail "${adr} is Accepted but has no '${section}' section (required by CONSTITUTION.md Art. 4.1)"
    done
  fi
done < <(/usr/bin/find docs -path '*/adr/*' -name '[0-9]*.md' | sort)

if (( violations > 0 )); then
  printf '\nSP42 docs/status consistency check FAILED: %d issue(s).\n' "$violations" >&2
  exit 1
fi

printf 'SP42 docs/status consistency checks passed.\n'
