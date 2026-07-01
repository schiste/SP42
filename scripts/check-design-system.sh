#!/usr/bin/env bash
# Hard design-system boundary gate.
#
# sp42-ui owns presentation. sp42-app pages own behavior and domain
# composition. This script scans the tree and fails on page-owned styling,
# presentation literals outside sp42-ui, app CSS, unprefixed selectors in the
# shared stylesheet, or sp42-ui dependencies back into app/domain crates.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

python3 - <<'PY'
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path.cwd()


def git_files() -> list[Path]:
    raw = subprocess.check_output(["git", "ls-files"], text=True)
    return [Path(line) for line in raw.splitlines() if line]


def read_lines(path: Path) -> list[str]:
    return (ROOT / path).read_text(encoding="utf-8", errors="replace").splitlines()


violations: list[tuple[Path, int, str, str]] = []


def fail(path: Path, line_no: int, message: str, line: str = "") -> None:
    violations.append((path, line_no, message, line.strip()))


FILES = git_files()

PAGE_FILES = [
    path
    for path in FILES
    if str(path).startswith("crates/sp42-app/src/pages/") and path.suffix == ".rs"
]

STYLE_ATTR = re.compile(r"\bstyle\s*=")
CLASS_ATTR = re.compile(r"\bclass\s*=")
CSS_LENGTH = re.compile(r"(?<![A-Za-z0-9_])\d+(?:\.\d+)?(?:px|rem|em|vh|vw|vmin|vmax|ch|lh)\b")
FONT_LITERAL = re.compile(r"\b(?:font-size|font-weight|font-family|line-height|letter-spacing)\b")
SEMANTIC_CLASS_ALLOW = "sp42-design-allow: semantic-class"

for path in PAGE_FILES:
    lines = read_lines(path)
    for idx, line in enumerate(lines):
        line_no = idx + 1
        if STYLE_ATTR.search(line):
            fail(path, line_no, "page code may not use style=; compose sp42-ui components instead", line)
        if CLASS_ATTR.search(line):
            allow_start = max(0, idx - 2)
            allowed = any(SEMANTIC_CLASS_ALLOW in lines[i] for i in range(allow_start, idx + 1))
            if not allowed:
                fail(
                    path,
                    line_no,
                    "page code may not use raw class= without an explicit semantic-class allowance",
                    line,
                )
        if CSS_LENGTH.search(line) or FONT_LITERAL.search(line):
            fail(
                path,
                line_no,
                "page code may not contain spacing/font literals; add a typed sp42-ui prop or variant",
                line,
            )

APP_CSS_SUFFIXES = {".css", ".scss", ".sass", ".less"}
for path in FILES:
    if str(path).startswith("crates/sp42-app/") and path.suffix in APP_CSS_SUFFIXES:
        fail(path, 1, "sp42-app may not own CSS files; move presentation to sp42-ui/static/style.css")

SOURCE_EXTENSIONS = {
    ".css",
    ".html",
    ".js",
    ".jsx",
    ".json",
    ".less",
    ".rs",
    ".sass",
    ".scss",
    ".ts",
    ".tsx",
    ".xml",
}
COLOR_LITERAL = re.compile(r"(?<![&A-Za-z0-9_])#[0-9A-Fa-f]{3}(?:[0-9A-Fa-f]{3})?(?:[0-9A-Fa-f]{2})?\b|rgba?\s*\(")
COLOR_SCAN_SKIP_PREFIXES = (
    ".github/",
    "crates/sp42-ui/",
    "crates/sp42-desktop/src-tauri/icons/",
    "docs/",
    "scripts/",
    "target/",
)

for path in FILES:
    path_text = str(path)
    if path.suffix not in SOURCE_EXTENSIONS:
        continue
    if any(path_text.startswith(prefix) for prefix in COLOR_SCAN_SKIP_PREFIXES):
        continue
    for line_no, line in enumerate(read_lines(path), start=1):
        if COLOR_LITERAL.search(line):
            fail(path, line_no, "color literals outside sp42-ui are forbidden", line)


def strip_css_comments_from_line(line: str, in_comment: bool) -> tuple[str, bool]:
    output: list[str] = []
    i = 0
    while i < len(line):
        if in_comment:
            end = line.find("*/", i)
            if end == -1:
                return "".join(output), True
            i = end + 2
            in_comment = False
            continue

        start = line.find("/*", i)
        if start == -1:
            output.append(line[i:])
            break
        output.append(line[i:start])
        i = start + 2
        in_comment = True
    return "".join(output), in_comment


STYLE_PATH = Path("crates/sp42-ui/static/style.css")
CLASS_SELECTOR = re.compile(r"(?<![A-Za-z0-9_-])\.([A-Za-z_][A-Za-z0-9_-]*)")

in_comment = False
for line_no, raw_line in enumerate(read_lines(STYLE_PATH), start=1):
    line, in_comment = strip_css_comments_from_line(raw_line, in_comment)
    for match in CLASS_SELECTOR.finditer(line):
        selector = match.group(1)
        if not selector.startswith("sp42-"):
            fail(
                STYLE_PATH,
                line_no,
                "sp42-ui stylesheet selectors must be private sp42-* internals",
                raw_line,
            )

UI_CARGO = Path("crates/sp42-ui/Cargo.toml")
DENIED_WORKSPACE_DEP = re.compile(r"^[\"']?(sp42-[A-Za-z0-9_-]+)[\"']?\s*=")
section = ""
for line_no, raw_line in enumerate(read_lines(UI_CARGO), start=1):
    line = raw_line.split("#", 1)[0].strip()
    if not line:
        continue
    if line.startswith("[") and line.endswith("]"):
        section = line.strip("[]")
        continue
    if section.endswith("dependencies"):
        match = DENIED_WORKSPACE_DEP.match(line)
        if match:
            fail(
                UI_CARGO,
                line_no,
                f"sp42-ui must not depend on workspace crate {match.group(1)}",
                raw_line,
            )

if violations:
    print("SP42 design-system gate failed:", file=sys.stderr)
    for path, line_no, message, line in violations:
        print(f"  {path}:{line_no}: {message}", file=sys.stderr)
        if line:
            print(f"    {line}", file=sys.stderr)
    print(f"\n{len(violations)} design-system violation(s).", file=sys.stderr)
    sys.exit(1)

print("SP42 design-system gate passed.")
PY
