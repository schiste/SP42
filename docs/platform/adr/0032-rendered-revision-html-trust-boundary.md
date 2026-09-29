# ADR-0032: Rendered-revision HTML trust boundary

**Status:** Accepted
**Date:** 2026-09-28
**Author:** Christophe Henner (drafted with Claude)
**Summary:** Rendered wiki-revision HTML crosses a trust boundary at the server fetch edge and must pass an allowlist sanitizer before it can reach any review surface; the sanitizer lives in `sp42-fetch` and the value is carried by a construction-only type.

## Context

The rendered-hunk-preview surface fetches the HTML that MediaWiki's
`action=parse&prop=text` produces for an **arbitrary revision**, and the browser
shell renders it with `Element::set_inner_html` (`sp42-app`'s
`RenderedHtmlPane`).

That markup is authored by whoever made the revision. The reviewer's browser
parses and executes it, in a surface that holds a live Wikimedia bearer token and
sits on the same origin as the operator's local API.

The Constitution (Art. 10.2) has always claimed *"Diff rendering uses sanitized
allowlist."* As of this ADR, no sanitizer existed anywhere in the workspace: the
claim was documentation of an intent, not of a control. The existing SSRF guard
(`sp42-fetch/src/resolver.rs`, ADR-0015) validated the *destination* of a fetch;
nothing validated what came back.

Two properties make the absence load-bearing rather than theoretical:

- **Scope.** ADR-0014 commits SP42 to *any* Wikimedia project, so the parser
  posture of the origin wiki is not a fixed, audited input.
- **Privilege.** The victim is a moderator whose session can act on the wiki.

`revision_artifacts.rs` — the file carrying this pipeline — also sat at 26%
line coverage, the lowest of any substantial module, so the boundary was neither
guarded nor observed.

## Decision

Sanitize at the **server fetch edge**, with an allowlist, exactly once per
response, before the value enters any in-memory or on-the-wire structure.

1. **`sp42-fetch` owns the sanitizer** (`sp42_fetch::sanitize_rendered_html`,
   built on `ammonia` 4.2). It already owns "what may safely cross this edge" via
   the SSRF resolver; connection and content are two halves of one boundary, and
   the crate is native-only so the sanitizer is reachable from every producer.

2. **`RenderedHunkSide::html` becomes private.** The type is constructed only via
   `RenderedHunkSide::sanitized(label, html, missing)`, which makes the
   "already sanitized" obligation visible at each call site instead of being a
   convention a struct literal can silently break. `html()` is the only reader.

3. **The policy is allowlist-only and deliberately narrow** (see Consequence 2).

4. **The browser keeps `set_inner_html`.** It is now a documented second stage
   rather than the only barrier.

## Why the sanitizer is not in `sp42-platform`

`sp42-platform` owns the `RenderedHunkSide` type, so a `newtype`-style
`SanitizedHtml` wrapper would be the more natural home. It is deliberately not:

```
sp42-app -> sp42-core -> sp42-platform
```

`sp42-platform` is in the browser/wasm dependency chain. `ammonia` is built on
`html5ever`, and adding an HTML parser there would add a serialized HTML5 parser
to the wasm bundle, against a ceiling (Art. 5.2 / `check-wasm-size.sh`) that
currently has roughly 9 KiB of gzip headroom. The type therefore *requires*
pre-sanitized input by construction and documentation rather than by
re-validating, and the sanitization itself happens natively at the fetch edge.

This is a real cost: the invariant is enforced by a private field and a
`#[must_use]` constructor, not by the type system refusing bad input. The
coverage tests in `sp42-fetch/src/html.rs` are what keep the policy honest, and a
future AD-wasm change (moving `sp42-platform` out of the wasm chain) is the
trigger to revisit this and introduce a validated newtype.

## Alternatives considered

**Drop the rendered-HTML pane; show structured text only.** Strictly safer, and
it removes the class of bug rather than defending against it. Rejected because
the rendered pane is a shipped operator-facing feature and a core part of what
makes a diff reviewable — removing it is a product regression, and ADR-0010's
"operator-confirmed proposals" workflow depends on reading rendered context.

**Sanitize in the browser, client-side.** Rejected. `sp42-app` would then need
`html5ever` in the wasm bundle — breaching the size ceiling — and the CLI and MCP
consumers of the same route would receive unsanitized HTML, leaving the hole open
for every non-browser shell.

**Hand-rolled allowlist (string/regex stripping).** Rejected as a
known-bypass class. Sanitizing HTML by ad-hoc string manipulation diverges from
browser parsing behaviour, and that divergence *is* the vulnerability.
`ammonia` re-serializes from a real HTML5 parse, so malformed and mis-nested
hostile input is normalized rather than passed through.

**Rely on Content-Security-Policy alone.** Rejected. CSP is defense in depth and
is added separately (see the CSP work); it does not remove the need to sanitize,
because the Tauri desktop shell must also tolerate the markup.

## Consequences

1. **Art. 10.2 becomes true.** The allowlist it describes now exists, is tested,
   and sits on the only path that produces the value.

2. **The policy, stated explicitly.** Preserved: structure, headings, lists,
   tables, inline text semantics, links, images, and MathML (Wikipedia renders
   formulas as MathML; dropping it would corrupt most science-article diffs).
   Also preserved: `data-mw`, which the reference-extraction lane reads back and
   which is inert in a browser.

   Deliberately dropped: `style` attributes and `<style>` elements (an
   exfiltration channel via `background:url(//attacker/…)` and a UI-redress
   channel — a review surface is not a layout engine); all `on*` handlers;
   `javascript:` / `data:` / `vbscript:` URLs; `iframe` / `object` / `embed` /
   `form` / `base` / `meta` / `link`; HTML comments (conditional-comment
   payloads).

   Surviving links are forced to `rel="noopener noreferrer"`, so a clicked
   reference cannot script or navigate the reviewing operator's app.

3. **Output is bounded** at 4 MiB, mirroring the fetch edge's response cap
   (ADR-0015): sanitizer cost is linear in input, so an unbounded hostile
   revision is a server-side CPU/DoS concern. Truncation is tag-structure-aware
   and lands only on a document boundary, so the result stays well-formed.

4. **Accepted residual risk.** A surviving `<img src>` or `<a href>` still causes
   the browser to contact whatever host the revision names. This is inherent to
   rendering a diff; it is recorded rather than "fixed", because dropping images
   would break image-bearing articles. Mitigation is the CSP work plus the
   already-enforced `rel` on links.

5. **Sanitization is idempotent** (asserted by test), so the server-side allowlist
   and any future client-side defense compose without double-transforming output.

6. **New dependency: `ammonia` 4.2.0** (Art. 7.2 disclosure) — MIT OR Apache-2.0,
   both already in the `deny.toml` allowlist; 16.9M lifetime downloads, actively
   maintained (last release 2026-09-17), one logical crate over `html5ever`.
   It adds a second `html5ever` to the tree (0.40 alongside the 0.36 that
   `parsoid` pulls); `deny.toml` has `multiple-versions = "warn"`, so this is
   visible and non-blocking. Transitive count is 12, well under the Art. 7.2
   50-transitive lead-approval threshold. It is native-only, so it does not
   affect the wasm budget.

7. **Coverage improves where it was weakest.** The new tests in
   `sp42-fetch/src/html.rs` cover script/style/handler/scheme/active-content
   removal, MathML and structure preservation, truncation well-formedness, and
   totality on malformed input. CSP hardening is a separate, follow-on change.
