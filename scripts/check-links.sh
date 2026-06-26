#!/usr/bin/env bash
# Verify intra-repo Markdown links resolve. Internal links only — external URLs
# (http/https/mailto/…) are deliberately out of scope so the gate is
# deterministic and never flaky on network or placeholder URLs.
#
# Checks every relative link target (any file type, plus directories), not just
# .md, so a broken link to a script/config/image is caught too.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

printf '== markdown link check (internal links) ==\n'
python3 - <<'PY'
import os, re, subprocess, sys

# [text](target) and [text](target "title"); target may be <wrapped>.
LINK = re.compile(r'\]\(\s*(<[^>]+>|[^)\s]+)')
SKIP = ("http://", "https://", "mailto:", "tel:", "//", "#")

def is_external(t: str) -> bool:
    if t.startswith(SKIP):
        return True
    return bool(re.match(r'^[a-zA-Z][a-zA-Z0-9+.-]*:', t))  # any URL scheme

files = [f for f in subprocess.check_output(["git", "ls-files", "*.md"]).decode().split("\n") if f]
broken = 0
for f in files:
    d = os.path.dirname(f)
    in_code = False
    with open(f, encoding="utf-8") as fh:
        for i, line in enumerate(fh, 1):
            stripped = line.lstrip()
            if stripped.startswith("```") or stripped.startswith("~~~"):
                in_code = not in_code   # toggle fenced code block
                continue
            if in_code:
                continue
            scan = re.sub(r'`[^`]*`', '', line)   # ignore inline code spans
            for m in LINK.finditer(scan):
                t = m.group(1).strip()
                if t.startswith("<") and t.endswith(">"):
                    t = t[1:-1]
                if is_external(t):
                    continue
                t = t.split("#", 1)[0]          # drop fragment
                if not t:
                    continue                     # pure in-page anchor
                target = os.path.normpath(os.path.join(d, t))
                if not os.path.exists(target):
                    print(f"BROKEN {f}:{i} -> {m.group(1)}", file=sys.stderr)
                    broken += 1

print(f"checked {len(files)} markdown file(s)")
if broken:
    print(f"SP42 link check failed: {broken} broken internal link(s).", file=sys.stderr)
    sys.exit(1)
print("all internal links resolve")
PY
printf 'SP42 link check passed.\n'
