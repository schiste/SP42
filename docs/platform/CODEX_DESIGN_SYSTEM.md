# Using the Wikimedia Codex design system

_Last updated: 2026-06-29._

SP42's web UI is styled to align with [Wikimedia Codex](https://doc.wikimedia.org/codex/latest/),
the Wikimedia Foundation's design system. This page records **how** SP42 uses
Codex, what it deliberately does not adopt, and the decisions a future
contributor needs to understand the divergences. It complements — and does not
replace — the binding [FRONTEND_DESIGN_CONTRACT.md](FRONTEND_DESIGN_CONTRACT.md)
and [ADR-0005](adr/0005-design-system-shared-component-layer.md).

## Why align with Codex at all

SP42 is a Wikipedia patrolling tool. Looking and behaving like a first-party
Wikimedia tool lowers the trust and learning cost for the editors who use it,
and lets us inherit Codex's accessibility and color decisions instead of
re-deriving them. Codex is also a moving target maintained by WMF; aligning to
its tokens means we can track it over time rather than drifting.

## The hard constraint: tokens, not components

Codex ships three separable things:

| Codex artifact | SP42 uses it? | Why |
| --- | --- | --- |
| Design **tokens** (`@wikimedia/codex-design-tokens`) | **Yes** | Plain CSS custom properties / values — framework-agnostic. |
| CSS-only component conventions | **Partly** | Class/markup patterns we mirror by hand (buttons, badges, fields). |
| **Vue** component library (`@wikimedia/codex`) | **No** | SP42's UI is Leptos (Rust → WASM). Vue components cannot mount in it. |
| Icons (`@wikimedia/codex-icons`) | Not yet | SVG paths are portable; adopt opportunistically if/when needed. |

The Vue incompatibility is the load-bearing fact: "conform to Codex" for SP42
means **the token/CSS path only**. We do not, and cannot without a rewrite,
consume Codex components directly.

## How the tokens are wired

The single source of truth is the `:root` block in
[`crates/sp42-ui/static/style.css`](../../crates/sp42-ui/static/style.css).

- SP42 keeps its **own semantic variable names** (`--bg`, `--text`, `--accent`,
  `--danger`, `--panel`, `--border`, `--radius-sm`, …). Every component reads
  these names and nothing else.
- Those names are **pointed at Codex token values** (`theme-wikimedia-ui`).
  `:root` carries the Codex **dark** values; `:root[data-theme="light"]` carries
  the Codex **light** values.
- Because the indirection layer is the only thing that changed, components were
  largely untouched by adoption, and a future Codex version bump only edits this
  one block.

Values were captured from the published Codex `theme-wikimedia-ui` token set
(`@wikimedia/codex-design-tokens`, v2.6.0 — `theme-wikimedia-ui.css` for light,
`theme-wikimedia-ui-mode-dark.css` for dark). Each SP42 token notes the Codex
token it derives from in a trailing comment, so the mapping is auditable.

### Updating to a newer Codex

1. Pull the new `theme-wikimedia-ui.css` / `…-mode-dark.css` from the published
   `@wikimedia/codex-design-tokens` package.
2. Re-map the values in the two `:root` blocks, keeping SP42's variable names.
3. Re-verify both themes (`trunk serve`, toggle light/dark) and the
   FRONTEND_DESIGN_CONTRACT checks.

Components should **not** need to change — if they do, a hardcoded value has
leaked past the token layer and should be routed through a variable instead.

## What we adopted, concretely

- **Dark default, light opt-in.** Dark stays SP42's default; light is a toggle
  in the workspace nav. The choice persists in `localStorage` (`sp42-theme`) and
  is applied as `data-theme` on the document root by the
  [`theme`](../../crates/sp42-ui/src/theme.rs) module. Shells restore theme
  state with `restore_theme()` and render the shared `ThemeToggle`; label,
  title, click handling, and visual classes stay inside `sp42-ui`. A small
  inline script in [`index.html`](../../index.html) applies the saved theme
  before first paint to avoid a flash. Codex ships both modes from one token
  source, so supporting both was nearly free.
- **Codex visual conventions:** 2px corner radius, the progressive/destructive/
  success/warning color roles, and CSS-only button/badge/field styling.
- **No values bypass the token layer.** Score tiers, status-badge tones, and
  panel/diff inline styles all resolve through CSS variables, which is what makes
  the light theme render correctly.

## PR #84 design-system migration gate

PR #84 is the vehicle for the full shared design-system cutover, not just a
Codex token refresh. Its merge gate is the clean state ADR-0005 describes:

- `sp42-ui` owns visual presentation: tokens, CSS, primitives, component
  variants, spacing, typography, surfaces, and visual state.
- `sp42-app` owns behavior and domain composition: data loading, actions,
  routing, auth/session state, wiki selection, and assembling `sp42-ui`
  components for app workflows.
- Page modules may not introduce visual presentation. In page code, `style=`,
  raw design classes, color literals, spacing literals, typography literals, and
  page-owned CSS selectors are banned.
- The only allowed styling escape hatch is dynamic runtime positioning inside
  `sp42-ui` internals, for UI that cannot be expressed statically (for example a
  context menu anchored to pointer coordinates). Pages must pass typed data or
  semantic variants into `sp42-ui`; they must not compute CSS strings.

The check for this gate should be mechanical: after the migration, a grep for
page/app-local styling should fail the build rather than rely on review.

## Shared primitive API

`crates/sp42-ui/src/primitives.rs` is the starting point for the shared
component layer. It exposes typed Leptos primitives for the presentation shapes
that currently account for most page-local styling: buttons, status badges,
panels/cards, stack/inline/grid layout, text/headings, section headers, fields,
inputs/selects/checkboxes, modals, disclosures, spinners, and empty/error
states.

The API is intentionally props-first and variant-only. Callers pass semantic
choices such as `tone`, `size`, `density`, `surface`, `align`, `gap`, and
`columns`; they do not pass `class` or `style`. Behavior stays in `sp42-app`
through typed callbacks and signals, while `sp42-ui` converts variants into the
owned CSS classes backed by the token layer.

## Deliberate divergences from Codex

### Fonts / i18n

Codex's typography is, by design, **not** an i18n font solution — and SP42's is.

- Codex sets its base body font to a bare `--font-family-base: sans-serif`,
  deferring script coverage to the OS/skin. Its Latin stack lives in an opt-in
  `--font-family-system-sans` (`-apple-system, Segoe UI, Roboto, Inter, …`) with
  **no CJK fallback**.
- Codex can do this because, inside MediaWiki, the **skin** (Vector) and the
  **Universal Language Selector (ULS)** supply per-language webfonts. The Codex
  token layer carries **no** direction/bidi/lang/script tokens at all.
- SP42 is a **standalone app** — no Vector skin, no ULS. So it keeps the
  `FRONTEND_DESIGN_CONTRACT` Rule 9.2 stack, which adds `"Noto Sans"` /
  `"Liberation Sans"` for CJK/RTL glyph coverage on Linux. Like Codex, it still
  terminates in `sans-serif`, so uncovered scripts fall back to the OS default.

Net: the font stack diverges from `--font-family-system-sans` **on purpose**,
for the environment SP42 runs in, not in conflict with Codex's intent.

### Bidirectionality

Codex treats RTL/LTR as a design-mirroring concern (which elements flip), not a
token concern, and leaves the mechanism to the consuming app. SP42 already
satisfies this independently via CSS logical properties (`*-inline-start`,
`border-block-end`, …) mandated by the FRONTEND_DESIGN_CONTRACT, so no Codex
bidi machinery is needed.

## References

- Codex docs: <https://doc.wikimedia.org/codex/latest/>
- Codex design tokens: <https://doc.wikimedia.org/codex/latest/design-tokens/overview.html>
- [FRONTEND_DESIGN_CONTRACT.md](FRONTEND_DESIGN_CONTRACT.md) — binding design spec (Rule 9.2 fonts, logical properties)
- [ADR-0005 — Design system and shared component layer](adr/0005-design-system-shared-component-layer.md)
