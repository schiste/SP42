#!/usr/bin/env bash
# Architecture map generator (companion to the ADR-0013 layer check).
#
# Regenerates docs/platform/architecture.md from three real sources, so the
# diagram cannot drift into fiction:
#
#   1. `cargo metadata`            — the actual workspace crates + dependency edges
#   2. scripts/check-layering.sh   — the layer taxonomy (single source of truth;
#                                    the LAYER map is parsed out of that script)
#   3. docs/**/adr, docs/**/prd    — ADRs and PRDs; a document is linked to a
#                                    crate when it names that crate
#
# Usage:
#   scripts/generate-architecture-map.sh           # rewrite docs/platform/architecture.md
#   scripts/generate-architecture-map.sh --check   # fail if the committed file is stale
#
# Output is deterministic (sorted, no timestamps) so --check is a plain diff.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

command -v cargo >/dev/null 2>&1 || {
  printf 'SP42 architecture map: `cargo` not found on PATH.\n' >&2
  exit 1
}

mode="${1:-generate}"

SP42_ARCHMAP_MODE="$mode" python3 - <<'PY'
import json, os, re, subprocess, sys

OUT = "docs/platform/architecture.md"
OUT_DIR = os.path.dirname(OUT)
check = os.environ.get("SP42_ARCHMAP_MODE") == "--check"

# ── 1. Layer taxonomy, parsed from check-layering.sh (single source of truth) ──
layer_src = open("scripts/check-layering.sh").read()
LAYER = dict(
    re.findall(r'^\s*"([a-z0-9-]+)":\s*"(platform|domain|shell|hybrid|tooling)",',
               layer_src, re.M)
)
if not LAYER:
    sys.exit("could not parse LAYER map out of scripts/check-layering.sh")

# Domain crates belong to a docs/domains/<domain>/ folder. Small and explicit;
# extend when a new domain crate lands (the generator fails loudly if a domain
# crate is missing here).
DOMAIN_OF = {
    "sp42-patrol": "patrolling",
    "sp42-citation": "references",
    "sp42-assessment": "assessment",
}

# Curated per-crate notes for the decision-coverage table — the place for
# caveats the mechanical sources can't express, with a citation. Links are
# relative to docs/platform/.
NOTES = {
    "sp42-core": "Hybrid exemption — re-export facade being split into"
                 " platform/domain crates and retired"
                 " ([ADR-0013](adr/0013-layered-platform-domain-architecture.md))",
    "sp42-inference": "Still depends on `sp42-core`; edge disappears when the"
                      " facade is retired"
                      " ([ADR-0013](adr/0013-layered-platform-domain-architecture.md))",
    "sp42-wiki": "Still depends on `sp42-core`; edge disappears when the"
                 " facade is retired"
                 " ([ADR-0013](adr/0013-layered-platform-domain-architecture.md))",
}

# ── 2. Workspace crates + dependency edges ──
meta = json.loads(
    subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"])
)
crates = sorted(p["name"] for p in meta["packages"] if p["name"] != "xtask")
crate_set = set(crates)
for c in crates:
    if c not in LAYER:
        sys.exit(f"crate {c} has no layer in scripts/check-layering.sh — add it there first")
    if LAYER[c] == "domain" and c not in DOMAIN_OF:
        sys.exit(f"domain crate {c} missing from DOMAIN_OF in {sys.argv[0] if sys.argv else 'generator'}")

edges = sorted({
    (p["name"], d["name"])
    for p in meta["packages"]
    for d in p["dependencies"]
    if d["name"] in crate_set and p["name"] in crate_set and d["kind"] is None
})
# Everything depends on sp42-types; drawing those edges buries the signal.
drawn_edges = [(a, b) for a, b in edges if b != "sp42-types"]

# ── 3. ADRs and PRDs ──
mention_re = re.compile(r"(?<![\w-])(sp42-[a-z0-9]+(?:-[a-z0-9]+)*)(?![\w-])")

def scan_doc(path):
    text = open(path).read()
    title_m = re.search(r"^# ((?:ADR|PRD)-\d{4}): (.+)$", text, re.M)
    # ADRs carry **Status:**, PRDs carry **State:** (see docs/process/prd-template.md)
    status_m = re.search(r"^\*\*(?:Status|State):\*\*\s*(.+)$", text, re.M)
    mentions = sorted({m for m in mention_re.findall(text) if m in crate_set})
    return {
        "id": title_m.group(1) if title_m else None,
        "title": title_m.group(2).strip() if title_m else None,
        "status": status_m.group(1).strip() if status_m else "—",
        "crates": mentions,
        "path": path,
        "adr_refs": sorted({f"ADR-{n}" for n in re.findall(r"ADR-(\d{4})", text)}),
    }

def collect(glob_dirs, kind):
    docs = []
    for d in glob_dirs:
        if not os.path.isdir(d):
            continue
        for f in sorted(os.listdir(d)):
            if not f.endswith(".md"):
                continue
            doc = scan_doc(os.path.join(d, f))
            if doc["id"] is None or not doc["id"].startswith(kind):
                continue
            doc["domain"] = d.split("/")[2] if d.startswith("docs/domains/") else None
            docs.append(doc)
    return sorted(docs, key=lambda d: d["id"])

adr_dirs = ["docs/platform/adr"] + sorted(
    os.path.join("docs/domains", dom, "adr")
    for dom in os.listdir("docs/domains")
    if os.path.isdir(os.path.join("docs/domains", dom))
)
prd_dirs = sorted(
    os.path.join("docs/domains", dom, "prd")
    for dom in os.listdir("docs/domains")
    if os.path.isdir(os.path.join("docs/domains", dom))
)
adrs = collect(adr_dirs, "ADR")
prds = collect(prd_dirs, "PRD")

adrs_of = {c: [a for a in adrs if c in a["crates"]] for c in crates}
prds_of = {c: [p for p in prds if c in p["crates"]] for c in crates}

def rel(path):
    return os.path.relpath(path, OUT_DIR)

def doc_link(doc):
    return f"[{doc['id']}]({rel(doc['path'])})"

# ── 4. Mermaid diagrams ──
# Layer-level overview: one node per layer (domains individually), edges
# aggregated from the real crate edges, labelled with the dependency count.
def group_of(c):
    return DOMAIN_OF[c] if LAYER[c] == "domain" else LAYER[c]

group_edges = {}
for a, b in edges:  # full edge set, including sp42-types targets
    ga, gb = group_of(a), group_of(b)
    if ga != gb:
        group_edges[(ga, gb)] = group_edges.get((ga, gb), 0) + 1

group_crates = {}
for c in crates:
    group_crates.setdefault(group_of(c), []).append(c)

GROUP_LABEL = {
    "shell": "Shells — composition roots",
    "hybrid": "sp42-core — hybrid, being retired (ADR-0013)",
    "platform": "Platform — mechanisms, primitives, contracts",
}
GROUP_CLASS = {"shell": "shell", "hybrid": "hybrid", "platform": "platform"}

def group_node(g):
    label = GROUP_LABEL.get(g, f"{g} domain")
    n = len(group_crates[g])
    if g != "hybrid":
        label += f"<br/>{n} crate{'s' if n > 1 else ''}"
    return f'G_{g}["{label}"]:::{GROUP_CLASS.get(g, "domain")}'

o = ["flowchart LR"]
order = ["shell"] + sorted(g for g in group_crates if LAYER[group_crates[g][0]] == "domain") \
        + [g for g in ("hybrid", "platform") if g in group_crates]
for g in order:
    o.append(f"  {group_node(g)}")
for (ga, gb), n in sorted(group_edges.items()):
    o.append(f'  G_{ga} -->|{n} dep{"s" if n > 1 else ""}| G_{gb}')
o += [
    "  classDef shell fill:#fef3c7,stroke:#b45309,color:#111",
    "  classDef domain fill:#dcfce7,stroke:#15803d,color:#111",
    "  classDef platform fill:#dbeafe,stroke:#1d4ed8,color:#111",
    "  classDef hybrid fill:#fee2e2,stroke:#b91c1c,color:#111,stroke-dasharray: 4 3",
]
overview = "\n".join(o)

# Crate-level map:
def node_id(c):
    return c.replace("-", "_")

def node(c):
    ids = [a["id"] for a in adrs_of[c]]
    if not ids:
        label = c
    elif len(ids) <= 3:
        label = f"{c}<br/>{', '.join(ids)}"
    else:
        label = f"{c}<br/>{ids[0]} … +{len(ids) - 1} more ADRs"
    return f'{node_id(c)}["{label}"]:::{LAYER[c]}'

m = ["flowchart TB"]
by_layer = {}
for c in crates:
    by_layer.setdefault(LAYER[c], []).append(c)

m.append('  subgraph SHELLS["Shells — composition roots"]')
for c in by_layer.get("shell", []):
    m.append(f"    {node(c)}")
m.append("  end")

m.append('  subgraph DOMAINS["Domains — policy, config, workflow"]')
for dom in sorted({DOMAIN_OF[c] for c in by_layer.get("domain", [])}):
    m.append(f'    subgraph DOM_{dom}["{dom}"]')
    for c in by_layer.get("domain", []):
        if DOMAIN_OF[c] == dom:
            m.append(f"      {node(c)}")
    m.append("    end")
m.append("  end")

if by_layer.get("hybrid"):
    m.append('  subgraph HYBRID["Hybrid — being split and retired (ADR-0013)"]')
    for c in by_layer["hybrid"]:
        m.append(f"    {node(c)}")
    m.append("  end")

m.append('  subgraph PLATFORM["Platform — mechanisms, primitives, contracts"]')
for c in by_layer.get("platform", []):
    m.append(f"    {node(c)}")
m.append("  end")

for a, b in drawn_edges:
    m.append(f"  {node_id(a)} --> {node_id(b)}")

m += [
    "  classDef shell fill:#fef3c7,stroke:#b45309,color:#111",
    "  classDef domain fill:#dcfce7,stroke:#15803d,color:#111",
    "  classDef platform fill:#dbeafe,stroke:#1d4ed8,color:#111",
    "  classDef hybrid fill:#fee2e2,stroke:#b91c1c,color:#111,stroke-dasharray: 4 3",
]
mermaid = "\n".join(m)

# ── 5. Markdown document ──
def crate_row(c):
    a = ", ".join(doc_link(x) for x in adrs_of[c]) or "—"
    p = ", ".join(doc_link(x) for x in prds_of[c]) or "—"
    return f"| `{c}` | {LAYER[c]} | {a} | {p} | {NOTES.get(c, '')} |"

lines = [
    "# SP42 architecture map",
    "",
    "<!-- GENERATED FILE — do not edit by hand. -->",
    "<!-- Regenerate with: scripts/generate-architecture-map.sh -->",
    "<!-- Verify freshness with: scripts/generate-architecture-map.sh --check -->",
    "",
    "The workspace crate graph, layered per",
    "[ADR-0013](adr/0013-layered-platform-domain-architecture.md), annotated with",
    "the ADRs that shaped each crate. Crates and dependency edges come from",
    "`cargo metadata`; layers come from the map in `scripts/check-layering.sh`;",
    "an ADR or PRD is linked to a crate when the document names that crate.",
    "",
    "The dependency invariant (`platform ◄─ domains ◄─ shells`) is *enforced*",
    "by `scripts/check-layering.sh` — this page is the picture, not the police.",
    "",
    "## Layer overview",
    "",
    "One node per layer (domains shown individually); edge labels count the",
    "underlying crate-to-crate dependencies, drawn in full in the next diagram.",
    "",
    "```mermaid",
    overview,
    "```",
    "",
    "## Crate-level map",
    "",
    "```mermaid",
    mermaid,
    "```",
    "",
    "Reading notes:",
    "",
    "- Edges into `sp42-types` are omitted — every crate depends on it.",
    "- `sp42-core` is the documented hybrid exemption from ADR-0013: a re-export",
    "  facade being split into platform/domain crates and retired.",
    "- Node annotations list the ADRs that name the crate.",
    "",
    "## Decision coverage by crate",
    "",
    "| Crate | Layer | ADRs that name it | PRDs that name it | Notes |",
    "|---|---|---|---|---|",
]
lines += [crate_row(c) for c in crates]

lines += [
    "",
    "## ADR index",
    "",
    "| ADR | Title | Status | Home | Crates it names |",
    "|---|---|---|---|---|",
]
for a in adrs:
    home = a["domain"] or "platform"
    names = ", ".join(f"`{c}`" for c in a["crates"]) or "—"
    lines.append(f"| {doc_link(a)} | {a['title']} | {a['status']} | {home} | {names} |")

lines += [
    "",
    "## PRD index",
    "",
    "| PRD | Title | State | Domain | ADRs it references |",
    "|---|---|---|---|---|",
]
for p in prds:
    refs = ", ".join(p["adr_refs"]) or "—"
    lines.append(f"| {doc_link(p)} | {p['title']} | {p['status']} | {p['domain']} | {refs} |")
lines.append("")

content = "\n".join(lines)

if check:
    try:
        current = open(OUT).read()
    except FileNotFoundError:
        sys.exit(f"{OUT} does not exist — run scripts/generate-architecture-map.sh")
    if current != content:
        # Print a unified diff so a CI-only mismatch is diagnosable from the
        # log (local and CI have disagreed before; PR 154).
        import difflib
        diff = difflib.unified_diff(
            current.splitlines(keepends=True),
            content.splitlines(keepends=True),
            fromfile=f"{OUT} (committed)",
            tofile=f"{OUT} (regenerated)",
        )
        sys.stderr.writelines(diff)
        sys.exit(f"{OUT} is stale — run scripts/generate-architecture-map.sh and commit the result")
    print(f"SP42 architecture map: {OUT} is up to date.")
else:
    with open(OUT, "w") as f:
        f.write(content)
    print(f"SP42 architecture map: wrote {OUT} "
          f"({len(crates)} crates, {len(drawn_edges)} edges drawn, "
          f"{len(adrs)} ADRs, {len(prds)} PRDs).")
PY
