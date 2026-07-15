# ADR-0026: Diff-render HTML sanitization reliance

**Status:** Proposed
**Date:** 2026-07-15
**Author:** Luis Villa (drafted by Claude Code)

## Context

The diff viewer renders a revision section's HTML into the DOM via
`set_inner_html` (`crates/sp42-app/src/components/diff_viewer.rs`,
`RenderedHtmlPane`). That HTML is MediaWiki `action=parse` output, fetched
server-side in `render_revision_section_side`
(`crates/sp42-server/src/revision_artifacts.rs`) and stored verbatim in the
rendered-hunk cache before it reaches the client.

Constitution **10.2** states: *"No `eval`, no `innerHTML` equivalent with
untrusted content. Leptos auto-escapes. Diff rendering uses sanitized
allowlist."* The current implementation has **no local allowlist** on this
path — it relies on MediaWiki's own parser sanitization (which strips
`<script>`, event-handler attributes, and disallowed tags before emitting
parser HTML).

This is a gap against the letter of 10.2. It is not a known live exploit,
because the content originates from MediaWiki's parser rather than from raw
attacker-controlled markup, but 10.2 calls for the allowlist precisely so the
render sink does not depend on an upstream sanitizer we do not control.

## Decision

Accept, for now, reliance on MediaWiki's upstream parser sanitization for
diff-render HTML, as a **documented, bounded deviation** from Constitution
10.2 rather than a silent one.

Conditions on the deviation:

1. **Single source.** The deviation covers only HTML obtained from the
   MediaWiki `action=parse` API over the authenticated fetch path. Any other
   HTML source reaching an `innerHTML`-equivalent sink is out of scope and
   still requires an allowlist.
2. **Edge-sanitize when revisited.** If the reliance is ever tightened, the
   allowlist (e.g. `ammonia`) is applied at the server fetch edge in
   `render_revision_section_side`, before caching — not in the wasm client —
   so the cache only ever holds sanitized HTML and the sanitizer stays out of
   the wasm bundle.
3. **Sinks are annotated.** Both the wasm sink and the server fetch edge carry
   comments pointing at this ADR so the deviation is discoverable from the
   code, not only from governance docs.

## Consequences

- The `set_inner_html` call in `RenderedHtmlPane` is a knowing, documented
  exception; reviewers should not treat its presence as an unreviewed 10.2
  violation, nor "fix" it by removing the call.
- Security posture now depends explicitly on MediaWiki's parser sanitization.
  If that assumption is ever in doubt (e.g. rendering content from a source
  with weaker sanitization), this ADR must be revisited and the edge
  allowlist added.
- Follow-up (not scheduled): add an `ammonia`-based allowlist at the fetch
  edge to close the 10.2 gap fully and remove the deviation.
