#!/usr/bin/env bash
# Layer check (ADR-0013). Reads `cargo metadata` and fails on any workspace edge
# that violates the platform/domain/shell dependency direction:
#
#     platform ◄─ domains ◄─ shells
#
#   - platform crates must NOT depend on a domain or shell crate
#   - domain crates must NOT depend on a shell crate (domain→domain is allowed)
#   - shells may depend on anything
#
# Each crate is mapped to a layer by name below. Once crates are relocated into
# crates/{platform,domains,shells}/ (migration phase 5) this can switch to a
# folder-based tag; until then the explicit map is the single source of truth and
# encodes the TARGET taxonomy so new violations are caught during migration.
#
# `sp42-core` is the documented hybrid exemption: it is being split into
# sp42-platform + sp42-patrol + sp42-citation (ADR-0013). Until that lands it is
# exempt both as a dependency source and target. Remove the exemption when it is
# retired (migration phase 5/6).
#
# Default is ENFORCE (exit non-zero on violation). Set SP42_LAYER_ENFORCE=0 for a
# non-failing (warn) run.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

command -v cargo >/dev/null 2>&1 || {
  printf 'SP42 layer check: `cargo` not found on PATH.\n' >&2
  exit 1
}

SP42_LAYER_ENFORCE="${SP42_LAYER_ENFORCE:-1}" python3 - <<'PY'
import json, os, subprocess, sys

enforce = os.environ.get("SP42_LAYER_ENFORCE", "1") == "1"

meta = json.loads(
    subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"])
)
members = {p["name"]: p for p in meta["packages"]}

# Target taxonomy (ADR-0013). Update this map as crates are extracted/relocated.
LAYER = {
    # platform
    "sp42-types": "platform",
    "sp42-platform": "platform",
    "sp42-coordination": "platform",
    "sp42-wiki": "platform",
    "sp42-inference": "platform",
    "sp42-live": "platform",
    "sp42-reporting": "platform",
    # shells
    "sp42-cli": "shell",
    "sp42-app": "shell",
    "sp42-desktop": "shell",
    "sp42-server": "shell",
    "sp42-devtools": "shell",
    # tooling (exempt)
    "xtask": "tooling",
    # domains
    "sp42-citation": "domain",
    "sp42-patrol": "domain",
    # hybrid — all code extracted; sp42-core is now a pure re-export facade,
    # retired in the relocation slice. Kept exempt until then.
    "sp42-core": "hybrid",
}

# Lower rank = lower layer. A crate may depend only on layers at or below its own
# rank; depending on a HIGHER layer is the violation.
RANK = {"platform": 0, "domain": 1, "shell": 2}
EXEMPT_AS_SOURCE = {"tooling", "hybrid"}
EXEMPT_AS_TARGET = {"tooling", "hybrid"}

violations, notes = [], []
for name in sorted(members):
    src = LAYER.get(name)
    if src is None:
        notes.append(f"untagged crate (add to LAYER map): {name}")
        continue
    if src in EXEMPT_AS_SOURCE:
        continue
    for dep in members[name]["dependencies"]:
        dname = dep["name"]
        if dname not in members:
            continue  # external crate
        dst = LAYER.get(dname)
        if dst is None or dst in EXEMPT_AS_TARGET:
            continue
        if RANK[dst] > RANK[src]:
            violations.append(
                f"{name} ({src}) -> {dname} ({dst})"
                f"  [forbidden: {src} must not depend on {dst}]"
            )

print("== SP42 layer check (ADR-0013) ==")
print("  layers: platform <- domains <- shells")
print("  exemptions: sp42-core (hybrid, pending split), xtask (tooling)")
for n in notes:
    print("  note:", n)

if notes and enforce:
    print("\n  every workspace crate must be tagged in the LAYER map.", file=sys.stderr)
    sys.exit(1)

if violations:
    print(f"\n  {len(violations)} layering violation(s):")
    for v in violations:
        print("   x", v)
    if enforce:
        print("\nSP42 layer check failed.", file=sys.stderr)
        sys.exit(1)
    print("\n  (warn mode: not failing — set SP42_LAYER_ENFORCE=1 to enforce)")
else:
    print("\n  no layering violations.")
print("\nSP42 layer check passed.")
PY
