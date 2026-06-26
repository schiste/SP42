#!/usr/bin/env bash
# Enforce CONSTITUTION Article 5.3 prohibited patterns on NEWLY ADDED Rust lines.
#
# Scans only added lines (diff '+') so it never fails retroactively on legacy
# code — new code is held to the standard, existing violations are left for
# deliberate cleanup. Reports file:line for each hit.
#
# Usage:
#   check-forbidden-patterns.sh --staged          # added lines in the index (pre-commit)
#   check-forbidden-patterns.sh --range A..B       # added lines in a commit range (pre-push/CI)
#   check-forbidden-patterns.sh                    # defaults to --staged
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="--staged"
range=""
case "${1:-}" in
  --staged) mode="--staged" ;;
  --range) mode="--range"; range="${2:-}"; [[ -n "$range" ]] || { echo "--range needs A..B" >&2; exit 2; } ;;
  "") mode="--staged" ;;
  *) echo "usage: $0 [--staged | --range A..B]" >&2; exit 2 ;;
esac

if [[ "$mode" == "--staged" ]]; then
  diff_cmd=(git diff --cached --unified=0 --no-color --)
else
  diff_cmd=(git diff --unified=0 --no-color "$range" --)
fi

# An added line is a test-context line when its file path looks like a test file.
is_test_path() {
  case "$1" in
    */tests/*|*_test.rs|*_tests.rs|*/test_fixtures.rs|*/tests.rs) return 0 ;;
    *) return 1 ;;
  esac
}

# Does the line carry an issue reference (#123 or a URL)? Used to permit
# annotated allow/ignore/TODO per Article 5.3 ("without approved issue link").
has_issue_ref() { [[ "$1" =~ \#[0-9]+ || "$1" == *http* ]]; }

# File content at the revision being scanned (staged index, or the head of the
# range) so test-context detection matches what is actually committed/pushed.
cat_ref() {
  if [[ "$mode" == "--staged" ]]; then
    git show ":$1" 2>/dev/null
  else
    git show "${range#*..}:$1" 2>/dev/null
  fi
}

# Is line `$2` of file `$1` inside a `#[cfg(test)]` module block? Used to exempt
# inline unit-test code (which commonly uses unwrap()) from the production rules.
in_test_module() {
  local lines
  lines=" $(cat_ref "$1" | perl -e '
    my @l=<STDIN>; my ($in,$d,$pend)=(0,0,0); my @out;
    for my $i (0..$#l){ my $ln=$i+1; my $s=$l[$i];
      if($in){ $d += ($s=~tr/{//)-($s=~tr/}//); push @out,$ln; $in=0 if $d<=0; }
      elsif($pend){ if($s=~/\bmod\b.*\{/){ $in=1; $d=($s=~tr/{//)-($s=~tr/}//); push @out,$ln; $pend=0; } elsif($s=~/\S/){ $pend=0; } }
      if(!$in && !$pend && $s=~/#\[cfg\(test\)\]/){ $pend=1; }
    }
    print "@out";') "
  [[ "$lines" == *" $2 "* ]]
}

# Does a `// SAFETY:` note sit on line `$2` or within the 3 lines above it?
has_safety_context() {
  local lo=$(( $2 > 3 ? $2 - 3 : 1 ))
  cat_ref "$1" | sed -n "${lo},${2}p" | grep -q '// SAFETY:'
}

violations=0
file=""
newline=0
is_rs=0

emit() { printf '  %s:%s  %s\n' "$file" "$1" "$2" >&2; violations=$((violations + 1)); }

while IFS= read -r line; do
  case "$line" in
    "+++ "*)
      # +++ b/path/to/file.rs   (or /dev/null for deletions)
      path="${line#+++ }"; path="${path#b/}"
      file="$path"
      [[ "$file" == *.rs ]] && is_rs=1 || is_rs=0
      continue ;;
    "@@ "*)
      # @@ -a,b +c,d @@  -> next added line number is c
      hunk="${line#@@ -}"; plus="${hunk#*+}"; start="${plus%%[ ,]*}"
      newline="$start"
      continue ;;
    "+"*)
      [[ "$is_rs" == "1" ]] || continue
      content="${line:1}"
      n="$newline"
      newline=$((newline + 1))

      # .unwrap() in production code -> use expect("...") or ?
      # Exempt test files (by path) and inline `#[cfg(test)]` modules.
      if [[ "$content" == *".unwrap()"* ]] && ! is_test_path "$file" && ! in_test_module "$file" "$n"; then
        emit "$n" 'unwrap() in production code — use expect("…") or ?'
      fi
      # dbg! left in source
      if [[ "$content" == *"dbg!("* ]]; then
        emit "$n" 'dbg!() left in source'
      fi
      # TODO/FIXME without an issue link
      if [[ "$content" == *TODO* || "$content" == *FIXME* ]] && ! has_issue_ref "$content"; then
        emit "$n" 'TODO/FIXME without an issue link (#123 or URL)'
      fi
      # #[allow(...)] without an issue link
      if [[ "$content" == *"#[allow("* ]] && ! has_issue_ref "$content"; then
        emit "$n" '#[allow(…)] without an approved issue link'
      fi
      # #[ignore] without an issue link
      if [[ "$content" == *"#[ignore"* ]] && ! has_issue_ref "$content"; then
        emit "$n" '#[ignore] without an issue link'
      fi
      # unsafe without a // SAFETY: note on the same line or the 3 lines above
      if [[ "$content" =~ (^|[^[:alnum:]_])unsafe([^[:alnum:]_]|$) ]] \
        && [[ "$content" != *"// SAFETY:"* ]] \
        && ! has_safety_context "$file" "$n"; then
        emit "$n" 'unsafe without a // SAFETY: justification'
      fi
      continue ;;
    *)
      continue ;;
  esac
done < <("${diff_cmd[@]}" 2>/dev/null)

if (( violations > 0 )); then
  echo "" >&2
  echo "SP42 forbidden-pattern check failed: $violations issue(s) in added lines (Constitution §5.3)." >&2
  exit 1
fi
printf 'SP42 forbidden-pattern check passed.\n'
