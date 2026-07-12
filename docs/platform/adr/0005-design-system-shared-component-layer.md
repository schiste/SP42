# ADR-0005: Design system and shared component layer (`sp42-ui`)

**Status:** Proposed
**Date:** 2026-06-07
**Author:** SP42
**Summary:** A presentation-only `sp42-ui` crate is the single source of the design system (tokens, atoms, primitives) that shells consume; it takes no domain dependency, and the design contract is enforced mechanically rather than by prose.

**Implementation note (2026-07-10):** Partially implemented. The `sp42-ui`
crate, its primitives, and `theme.rs` shipped, but the token single source of
truth this ADR centers on — a bundled `tokens.css` with closed `Color`/`Space`/
`Type`/`Icon` atom enums — has not landed (`scripts/check-design-system.sh`
still gates `static/style.css`). Status stays Proposed until the token layer
ships or the decision is revised.

**Layer note (2026-07-11, #128):** `sp42-ui` is classified **platform** in the
layer check, not a shell — it is a reusable presentation library, and the
classification mechanically enforces this ADR's rule that `sp42-ui` takes no
domain dependency (§"Shells depend on the typed API": a domain-typed component
like `ActionBar` lives in the consuming shell). The layer check now fails if
`sp42-ui` ever gains a domain or shell dependency.

## Context

`docs/platform/FRONTEND_DESIGN_CONTRACT.md` is a binding design spec, but it is prose with
no implementation — §18 explicitly leaves the "Leptos component API" out of scope.
That gap is filled by accident: the patrol UI has three sources of visual truth
that disagree. The contract names tokens (`--surface-base`, `--space-*`);
`sp42-ui/static/style.css` uses different names (`--bg`, `--danger`) and has no
spacing/type scale; components hardcode the rest.

Measured today: **176** inline `style=` attributes, **43** hex/`rgba()` literals
in `.rs`, **33** stringly-typed `class="btn …"` usages, and **44** distinct colour
values against the contract's ≤20 cap (the excess is one hue at 5–7 alphas). The
mandatory golden-ratio scales (Rule 4.1, 9.3) exist nowhere; `--font-sans` is
referenced but never defined; `StatusBadge` invents tones that contradict
`--success`. The contract is therefore unenforceable and every component
re-derives values that drift apart.

This is an ADR-only change to internal architecture; no operator-facing behaviour
changes, so no PRD.

**Contract amendments.** This ADR carries two substantive amendments to the
binding `FRONTEND_DESIGN_CONTRACT.md`, made in this PR under its Article-12
process (Rule 19.4): (1) Rule 16.2 is amended to permit exactly one
network-internal, Trunk-bundled token stylesheet (`tokens.css`), with "external"
defined as network-external; (2) §17's custom-property cap is raised from `<40`
to `<=52` (the asserted total for the now-mandatory scales and diff tokens). The
≤20 colour cap is unchanged. Both are recorded as dated amendment notes in the
contract and gate on the same approval that accepts this ADR; until then this ADR
and the amendments are Proposed together.

## Decision

Create **`sp42-ui`**, a presentation-only crate that is the single source of
visual truth, structured as **atoms → primitives → patterns**. Every shell
(`sp42-app`, `sp42-desktop`) renders only through it. `sp42-ui` is a *web*
source of truth, not a universal one: `sp42-cli` is deliberately not a Leptos
consumer and shares only the medium-independent semantic layer (`Tier` /
`StatusTone`, which lives below `sp42-ui`), rendering it as ANSI — so one
threshold definition serves every shell.

### Design principles

- **Single source of truth.** One atom catalog defines every colour, size, type,
  and icon; the three drifting sources collapse into it, and nothing downstream
  redefines a value.
- **One reason to change.** `sp42-ui` owns the look; domain crates own behaviour;
  shells own domain-bound composition.
- **Extend by adding, not editing.** A new state, icon, or shell is a new enum
  variant or a new consumer — never an edit to existing call sites or a forked
  stylesheet.
- **Narrow, typed surfaces.** Primitives expose specific props, with **no
  `class`/`style` passthrough** — the catch-all escape hatch is removed.
- **Shells depend on the typed API, not on CSS.** `sp42-ui` depends on `leptos`
  and **nothing in the domain** (no `sp42-core`/`sp42-server`/shells); data enters
  via plain props. So `Button`/`ScoreDisplay` live in `sp42-ui`, while `ActionBar`
  (which speaks `SessionActionKind`) stays in `sp42-app` and composes them.

```text
shared contracts -> domain crates -> shell crates -> compose sp42-ui
                                       (sp42-ui has no domain deps)
```

### Enforcement-first

Our only gate is local git hooks (no server CI). So each rule is designed into the
cheapest enforcement tier: **(1) unrepresentable** in the type system (preferred —
no bypass), else **(2) auto-rejected** by `check-design-system.sh`. Tier 3
(review) is not used: a rule that needs it is redesigned or dropped. This is the
whole point of `sp42-ui` — it moves the contract from prose into types and checks.

### Atoms

Each category is one centralized set: a CSS custom-property in the bundled
`tokens.css`, plus a closed Rust enum (`Color::AccentRiskHigh`, `Space::M`,
`Icon::RiskHigh`) that renders to `var(--…)` or inline SVG. Style values are built
*only* from these enums, so a hex, a `12px`, or a hand-typed glyph cannot be passed
where an atom is expected (tier 1). Canonical names follow the contract; old names
stay as aliases during migration.

- **Colour** — surfaces, text, accents, and diff (§8); diff colours become tokens.
- **Type + font** — `--type-base|m|l` (golden-ratio); the font is **universal by
  stack, not by file**: one `--font-ui` (system-ui + Noto fallbacks) and
  `--font-mono`, no bundled/web font (Rule 9.1). Fixes `--font-sans`.
- **Icon** — closed `Icon` enum, **≤10** (§16.3), each a vendored **inline SVG**
  from **Material Symbols** (Outlined 400; Apache-2.0), pinned in
  `atoms/icon/icons.toml`. Not the variable font (Rule 9.1/16.4; ~7.9 MB). Static,
  so no motion rules needed; RTL flip lives here once.
- **Space / Radius / Elevation / Motion / Focus** — the golden-ratio space scale
  (Rule 4.1); `radius-sm|md` (>8px inexpressible); one shadow (no stacking); one
  ≤120ms motion token (decorative animation has no atom to reference); the focus
  ring (Rule 11.2), always applied.

### Primitives

Typed Leptos components with exhaustive variant enums; the base classes (`.btn`,
`.badge`, …) are private to `sp42-ui`, so shells cannot reference them.

```rust
// before: view! { <button class="btn btn-danger">"R Rollback"</button> }
view! { <Button variant=ButtonVariant::Danger>"R Rollback"</Button> }
```

Catalog: `Button`, `Badge`/`StatusBadge`, `Panel`, `Card`, `ScoreDisplay`
(consumes a domain `Tier`; `sp42-ui` maps `Tier` → `Color`+`Icon`),
`Field`/`TextInput`/`Select`,
`SectionHeader`, `Spinner`, `Modal`, `ContextMenu`. Each encodes the contract
(focus ring, logical properties, no decorative animation — replacing the
contract-violating `.btn-recommended` pulse).

## Enforcement

**Gate (hooks-only, developer-owned).** `pre-commit` runs the fast grep-based
`check-design-system.sh` **diff-scoped**: a banned pattern on an *added* line
blocks; pre-existing violations only warn. `pre-push` runs it via `xtask ci-all`
in **baseline mode**: blocks if a whole-tree count *rose* above the committed
baseline (catching `--no-verify`), else warns. So new violations are blocked,
legacy debt only shrinks, and no one is blocked by code they did not touch.
`SP42_SKIP_GIT_HOOKS=1` to bypass is a governance violation (Rule 19.2).

**Bans → mechanism** (`check-design-system.sh` mirrors `check-focused.sh`):

| Ban | Tier | Mechanism |
|---|---|---|
| Hex/`rgba()` or color/length in `style=`, in shells | 2 | grep `sp42-app`/`sp42-desktop` |
| Stringly-typed base class / `class`/`style` passthrough | 1 | classes private; primitives expose no passthrough |
| Off-scale colour/space/type/glyph | 1 | `Color`/`Space`/`Type`/`Icon` enums |
| Icon font, or icon outside the set | 2 | grep `@font-face`/`material-symbols`/`.woff2`; manifest `source` must be `material-symbols:` |
| Extra stylesheet / CDN / `@import` / physical CSS prop | 2 | grep (extends Rule 19.3) |
| `sp42-ui` depends on a domain crate | 2 | grep `sp42-ui/Cargo.toml` |

**Budgets** (asserted; baselines ratchet to caps):

| Budget | Today | Cap |
|---|---|---|
| Custom properties in `tokens.css` | 31 | ≤ 52 (31 + 9 space + 3 type + 6 diff) |
| Distinct colours, excl. diff | ~32 | ≤ 20 |
| Color literals outside `tokens.css` | 44 | 0 |
| Inline `style=` w/ color/length · stringly classes · icons | 176 · 33 · — | 0 · 0 · ≤10 |

## Migration

PR #84 is the vehicle for the full deployment, not a token-only staging PR. The
work may still land as small commits, but the PR merge gate is the final clean
state:

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

Implementation order: (1) Scaffold `sp42-ui`, move `style.css` in (aliased), and
wire the mechanical check. (2) Add the space/type/diff tokens. (3) Land the
score→severity *semantic* in a Leptos-free home, not in `sp42-ui`: add `Tier`,
`score_tier(score) -> Tier`, and `StatusTone` (with their tests) to the
scoring/domain layer (`sp42-core` now, a `sp42-types`/scoring-policy slice later)
so the Leptos-free `sp42-cli` shares one definition — a tier threshold is scoring
policy, not a widget (Scoring Constitution §14.3). `sp42-ui` then maps `Tier` →
`Color`+`Icon` and `sp42-cli` maps `Tier` → ANSI/glyph. Move `tone_colors` into
`sp42-ui`; route `wiki_base_url` to `sp42-wiki` (not `sp42-ui` — it is a domain
concern, and `sp42-ui` takes no domain deps). Fix `StatusBadge` and
`--font-sans`. (4) Land primitives and patterns, deleting superseded CSS/inline
styles as the app migrates. The final check must fail on any remaining page or
app-local styling, rather than relying on review.

## Alternatives

- **Status quo / consolidate-tokens-only** — leaves stringly-typed usage and 176
  inline styles; contract stays unenforceable. Rejected.
- **Module in `sp42-app`, not a crate** — lighter, but `sp42-desktop` is a known
  second consumer and only a crate boundary stops a forked stylesheet. Rejected.
- **`class`/`style` passthrough** — the one thing grep can't catch; reopens the
  drift door. Rejected; one-offs add a reviewed variant instead.
- **External UI kit / Tailwind / icon font** — network requests + web fonts,
  against Rule 16.1/9.1 and ADR-0001 (all-Rust). Rejected.

## Consequences

- One place to change any visual value; the contract becomes enforceable.
- Most violations are compile errors (no maintenance, no bypass) — the right
  default for a hooks-only gate.
- `sp42-desktop` and future shells inherit the system for free.
- Cost: a new crate, temporary aliases, a shrinking debt baseline, and slightly
  more friction for genuinely new visuals (a reviewed `sp42-ui` change) — by design.

## Non-Goals

- No change to the contract's *rules* beyond the two reconciliations above.
- No redesign, no light mode, no back-office polish, no server CI (future option).
- No domain logic in `sp42-ui`.
