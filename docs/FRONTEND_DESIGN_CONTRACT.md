# SP42 Frontend Design Contract

*Binding design specification for the patrol interface. The back office
(telemetry, debug panels, coordination internals) may be technical and
information-dense without polish. The patrol surface must be fast, obvious,
and effortless.*

*This document has the same standing as CONSTITUTION.md. Violations block
merge.*

---

## 1. Design North Star

**One sentence:** A patroller reviewing their 200th edit of the day should
feel faster, not tired.

**Metric:** time-per-edit. Every design decision is evaluated against one
question: *does this make the next edit faster or slower to process?* If a
feature looks good but adds a click, a dialog, or a layout shift, it fails.

---

## 2. The Patrol Loop

The entire frontend exists to serve this loop:

```
SEE  ->  ASSESS  ->  DECIDE  ->  ACT  ->  NEXT
 |         |           |          |         |
queue    diff +      risk       key      auto-
feed     context    signals    press    advance
```

The loop must complete in **under 5 seconds for obvious vandalism** and
**under 15 seconds for ambiguous edits**. These are wall-clock targets
measured from the moment an edit appears at the top of the queue.

**Rule 2.1** Every element on the patrol screen must serve exactly one stage
of this loop. If it serves none, it does not belong on the patrol screen.

**Rule 2.2** The loop stages follow logical reading order (inline-start to
inline-end, block-start to block-end). In LTR locales the eye moves left to
right; in RTL locales it moves right to left. The layout adapts
automatically via CSS logical properties — the information sequence is always
queue -> diff -> context -> actions regardless of script direction.

**Rule 2.3** Advancing to the next edit after an action requires zero
interaction. The queue auto-advances and the diff panel updates without a
page transition, modal, confirmation dialog, or scroll reset.

---

## 3. Visual Identity — Anti-Patterns

SP42 has a functional identity, not a decorative one. The interface
communicates through structure and data, not through visual effects.

### 3.1 Banned Patterns

The following patterns are explicitly prohibited on the patrol surface.
These are the hallmarks of generic AI-generated design and must be rejected
in review:

| Pattern | Why it is banned |
|---------|-----------------|
| Gradient blobs / radial gradient backgrounds | Decorative, not informational. Wastes GPU paint cycles. Obscures text contrast. |
| Glassmorphism (`backdrop-filter: blur`) | Expensive compositing. Makes text behind the element unreadable. Purely cosmetic. |
| Border radius > 8px on functional elements | Wastes corner space. 2-6px is sufficient for softening; 1.5rem rounds are decorative. |
| Oversized hero sections | The patrol screen has no "hero." Every pixel is working content. |
| Background illustrations, SVG blob shapes | Decorative. Competes with diff content for attention. |
| Gradient text | Unreadable at small sizes. Breaks with high-contrast modes. |
| Shadow stacking (multiple `box-shadow` layers) | A single subtle shadow is sufficient for elevation. Multiple shadows are cosmetic. |
| Badge/pill overuse (> 3 badges per component) | Information overload. If everything is badged, nothing is. |
| "Glow" effects (`box-shadow` with colored spread) | Decorative. Breaks dark-mode contrast ratios. |
| Bounce/spring/elastic animations | Playful, not professional. Slows perceived performance. |
| Skeleton shimmer loaders in the patrol loop | The patrol loop does not wait. Old content stays until new content is ready. |
| Emojis as status indicators | Ambiguous across platforms. Use text labels or geometric icons. |
| Purple/teal/indigo accent gradients | The default AI color palette. SP42 uses a constrained blue-grey palette with semantic accents (Section 8). |

**Rule 3.1** If a CSS property exists solely for decoration and removing it
would not reduce the user's ability to understand or interact with the
interface, it must be removed.

### 3.2 Permitted Visual Techniques

| Technique | When permitted |
|-----------|--------------|
| Solid background color per surface level | Always (see Section 8) |
| Single `box-shadow` for panel elevation | On floating elements (action bar, dropdowns) |
| `border` (1px solid) for panel separation | Always |
| `border-radius: 2-6px` | On buttons, inputs, badges, cards |
| `opacity` changes | For disabled states and hover feedback |
| `transition` on `opacity` and `background-color` | For hover/focus states, max 120ms |
| CSS `outline` for focus indicators | Always (required for a11y) |

---

## 4. Proportional System — Golden Ratio

All spatial relationships in the patrol interface derive from the golden
ratio (phi = 1.618). This produces visual harmony without arbitrary values.

### 4.1 Base Unit

The base spatial unit is **4px**. All spacing, sizing, and layout dimensions
are multiples of this unit, scaled by powers of phi.

| Token | Formula | Value | Usage |
|-------|---------|-------|-------|
| `--space-3xs` | 4 * phi^-3 | ~1px | Hairline borders |
| `--space-2xs` | 4 * phi^-2 | ~2px | Tight internal padding |
| `--space-xs` | 4 * phi^-1 | ~2.5px | Icon-to-label gap |
| `--space-s` | 4 * phi^0 | 4px | Minimum padding |
| `--space-m` | 4 * phi^1 | ~6.5px | Standard internal gap |
| `--space-l` | 4 * phi^2 | ~10px | Component padding |
| `--space-xl` | 4 * phi^3 | ~17px | Section gap |
| `--space-2xl` | 4 * phi^4 | ~27px | Panel padding |
| `--space-3xl` | 4 * phi^5 | ~44px | Major section margin |

In practice, round to the nearest pixel: 1, 2, 3, 4, 7, 10, 17, 27, 44.

### 4.2 Layout Column Ratios

The three-column patrol layout uses golden-ratio proportions:

```
Queue : Diff : Context = 1 : phi^2 : 1

Concrete at 1440px viewport:
  Queue    = 220px   (1 part)
  Diff     = 576px   (phi^2 ~ 2.618 parts, flexible)
  Context  = 220px   (1 part)
  Gaps     = 2 * 10px
```

The diff panel always gets the golden-ratio-dominant share of horizontal
space. On narrow viewports the ratio adjusts but the diff panel remains
the largest element.

### 4.3 Type Scale

Font sizes follow a modular scale based on phi with a 13px base (the diff
content size):

| Token | Formula | Value | Usage |
|-------|---------|-------|-------|
| `--type-xs` | 13 / phi^2 | ~5px | — (below minimum, unused) |
| `--type-s` | 13 / phi | ~8px | — (below minimum, unused) |
| `--type-base` | 13px | 13px | Diff content, queue items |
| `--type-m` | 13 * phi^0.5 | ~16.5px | Section headers, score display |
| `--type-l` | 13 * phi | ~21px | Panel titles (rare) |

No font size below 11px is used (Rule 6.2 override on the scale).

### 4.4 Action Bar Height

The action bar height is the base unit scaled to a golden proportion of the
viewport:

```
Action bar height = --space-3xl = 44px
```

This is phi^5 * 4px, which happens to be the standard touch-target minimum
(44x44px per WCAG 2.5.8).

**Rule 4.1** No spacing value in the patrol interface may be an arbitrary
number. Every space, padding, margin, and gap must map to a token in the
golden-ratio scale. Ad hoc values like `padding: 12px 18px` are rejected
in review.

---

## 5. Layout

### 5.1 Patrol View (Desktop, >= 1024px)

Three-column layout, fixed to viewport (no page-level scrolling):

```
+--session-bar-------------------------------------------+
|  SP42  wiki:frwiki  user:Example  [o]  [gear]          |
+------------+-------------------------+-----------------+
|            |                         |                 |
|   Queue    |       Diff panel        |    Context      |
|   (start)  |       (center)         |    (end)        |
|            |                         |                 |
|  ~220px    |       flexible          |    ~220px       |
|  ratio: 1  |       ratio: phi^2     |    ratio: 1     |
|            |                         |                 |
+------------+-------------------------+-----------------+
|        Action bar (full width, pinned to bottom)       |
+--------------------------------------------------------+
```

- **Queue column**: edit list with score, title, timestamp. Selected row
  highlighted. Scrollable independently.
- **Diff panel**: the only scrollable content the user focuses on. Takes the
  golden-ratio share of horizontal space.
- **Context sidebar**: user info, warning level, account age, ML score,
  signal breakdown. Static for the selected edit — no scrolling expected.
- **Action bar**: pinned to bottom. Always visible. Never scrolls out of
  view. Height: 44px.

**Rule 5.1** The queue, diff, and context panels must render simultaneously.
No tabs, no accordions, no "click to expand." If the user needs to click to
see the diff, the layout is wrong.

**Rule 5.2** The diff panel never has horizontal scroll. Long lines wrap or
are truncated with an expand affordance. Horizontal scrolling in a diff is
the number one complaint about every existing tool.

**Rule 5.3** The layout is CSS-only (grid/flex with logical properties). No
JavaScript-driven layout, resize handles, or drag-to-reorder. Layout must
render correctly before Wasm loads. Layout must reverse correctly when
`dir="rtl"` is set.

### 5.2 Patrol View (Tablet / Mobile, < 1024px)

Single-column stack: queue (collapsed to current-edit summary) -> diff ->
context (collapsed to badges) -> action bar (pinned bottom).

The queue becomes a swipe gesture (inline-start = skip, inline-end = expand
queue list). Swipe direction reverses in RTL.

**Rule 5.4** The mobile layout removes no information — it collapses and
re-layers it. A mobile patroller can still see every signal, just not
simultaneously.

---

## 6. Information Hierarchy

### 6.1 The Five-Second Scan

When a patroller's eye lands on a new edit, they scan in this order (based
on eye-tracking patterns and Huggle/AntiVandal user interviews):

1. **Score** — How suspicious is this? (0-100, color-coded)
2. **User identity** — Anonymous IP? New account? Trusted editor?
3. **Diff** — What changed? (red deletions, green additions)
4. **Article title** — What page?
5. **Signals** — Which scoring rules fired? (only if the score is ambiguous)

**Rule 6.1** Elements are sized and positioned in this priority order. The
score is the largest/most prominent element in the context panel. The user
identity is next. Signals are smallest — they are detail, not headlines.

### 6.2 Score Visualization

Scores map to three visual tiers:

| Range | Color | Meaning | Action hint | a11y icon |
|-------|-------|---------|-------------|-----------|
| 70-100 | Red/warm | High risk | Likely rollback | `!!` double-bang |
| 30-69 | Amber/neutral | Review needed | Read the diff | `?` question |
| 0-29 | Green/cool | Low risk | Likely patrol/skip | Checkmark |

**Rule 6.2** These tiers use background tint on the queue row, not just text
color. Color must be perceivable by colorblind users — use both hue AND
luminance contrast. Never rely on red/green alone. The a11y icon column is
mandatory — it appears alongside the score in all display modes.

**Rule 6.3** The score number is always visible alongside the color. Color
supplements the number; it never replaces it.

### 6.3 Diff Rendering

- **Deletions**: red background (`--diff-delete-bg`) with inline-start border.
- **Additions**: green background (`--diff-add-bg`) with inline-start border.
- **Unchanged context**: 3 lines above and below each change, collapsed
  otherwise.
- **Character-level highlighting within changed lines**: stronger
  tint on the specific characters that differ, not just the whole line.

**Rule 6.4** Diff rendering uses a monospace font. Variable-width fonts in
diffs cause misalignment that wastes patroller time.

**Rule 6.5** Empty diffs (page moves, protection changes, etc.) display a
clear "no content change" indicator, not a blank panel.

**Rule 6.6** Diff content is rendered with `unicode-bidi: plaintext` and
the `dir` attribute set per-line based on content detection. A diff
containing Arabic text within an English article must display each line in
the correct direction.

---

## 7. Actions

### 7.1 Action Bar

The action bar is a single row of buttons, always visible, pinned to the
bottom of the viewport:

```
[R Rollback] [U Undo] [P Patrol] [S Skip] [W Warn v] [... More v]
```

**Rule 7.1** Primary actions (Rollback, Undo, Patrol, Skip) are always
visible. Secondary actions (Warn, Report, Tag) live behind a "More" menu.

**Rule 7.2** Each primary action button shows its keyboard shortcut on the
button face. The shortcut is a single key press, not a chord. Suggested:
`R` = Rollback, `U` = Undo, `P` = Patrol, `S` = Skip.

**Rule 7.3** Destructive actions (Rollback) have a distinct visual weight
(filled button, warmer color). Non-destructive actions (Patrol, Skip) are
outlined or muted.

### 7.2 Action Feedback

After an action executes:

- The button briefly shows a checkmark or X (120ms).
- The queue auto-advances to the next edit.
- The diff panel updates.
- No confirmation dialog. No toast notification. No modal.

**Rule 7.4** No confirmation dialogs for standard actions. Rollback on
Wikipedia is already reversible (undo the rollback). Adding "Are you sure?"
halves throughput for zero safety gain.

**Rule 7.5** If an action fails (network error, capability mismatch), the
button shows an error state and the action bar displays a single-line error
message. The error does not block interaction with other actions or the
queue.

### 7.3 Keyboard-First

**Rule 7.6** Every action in the patrol loop is reachable by keyboard without
modifier keys. The patrol loop can be operated entirely from the right hand
on a QWERTY layout without reaching for the mouse.

**Rule 7.7** The keyboard shortcut layer activates when the diff panel has
focus (the default state after queue advance). Typing in a text input (search,
filter) suspends shortcuts. `Escape` returns focus to the diff panel.

**Rule 7.8** Arrow keys (Up/Down) navigate the queue. Enter selects an edit.
This allows queue browsing without mouse.

---

## 8. Color System

The patrol interface uses a constrained dark palette. All colors are
specified as CSS custom properties (design tokens). The palette is
deliberately narrow — limiting choice prevents visual noise.

### 8.1 Surfaces

| Token | Value | Usage |
|-------|-------|-------|
| `--surface-base` | `#08111f` | Page background |
| `--surface-panel` | `#0b1324` | Queue, context, action bar |
| `--surface-elevated` | `#111b2e` | Diff panel, active queue row |
| `--surface-hover` | `#162038` | Hover state on queue rows |
| `--surface-border` | `rgba(148, 163, 184, 0.14)` | Panel borders |

### 8.2 Text

| Token | Value | Usage |
|-------|-------|-------|
| `--text-primary` | `#eff4ff` | Main text, diff content |
| `--text-secondary` | `#8b9fc0` | Labels, metadata, timestamps |
| `--text-muted` | `#4f6280` | Disabled, placeholder |

### 8.3 Semantic Accents

| Token | Value | Usage |
|-------|-------|-------|
| `--accent-risk-high` | `#ef4444` | Score 70-100, rollback button |
| `--accent-risk-medium` | `#f59e0b` | Score 30-69 |
| `--accent-risk-low` | `#22c55e` | Score 0-29, patrol action |
| `--accent-info` | `#3b82f6` | Neutral info, links, focus rings |
| `--accent-brand` | `#8fb7ff` | SP42 branding (session bar only) |

### 8.4 Diff

| Token | Value | Usage |
|-------|-------|-------|
| `--diff-add-bg` | `rgba(34, 197, 94, 0.12)` | Addition line background |
| `--diff-add-border` | `#22c55e` | Addition inline-start border |
| `--diff-add-highlight` | `rgba(34, 197, 94, 0.25)` | Character-level add highlight |
| `--diff-del-bg` | `rgba(239, 68, 68, 0.12)` | Deletion line background |
| `--diff-del-border` | `#ef4444` | Deletion inline-start border |
| `--diff-del-highlight` | `rgba(239, 68, 68, 0.25)` | Character-level deletion highlight |

**Rule 8.1** The patrol interface does not offer a light mode. Dark mode is
the only mode. Patrollers work in sustained sessions; light backgrounds cause
eye fatigue. This is a deliberate product decision, not a missing feature.

**Rule 8.2** No color is used for decoration. Every colored element encodes
information (risk level, action type, state change). If an element is colored,
it must answer "what does this color mean?"

**Rule 8.3** All text on all surfaces must maintain a contrast ratio of at
least **4.5:1** per WCAG AA. All large text (>= 18px or >= 14px bold) must
maintain at least **3:1**. Interactive elements must maintain at least
**3:1** against adjacent non-interactive elements.

**Rule 8.4** The total number of distinct color values used in the patrol
surface (excluding diff content) must not exceed **20**. This number
includes every surface, text, accent, and border value. Adding a color
requires removing one or justifying the addition in the PR description.

---

## 9. Typography

### 9.1 Zero Web Fonts

**Rule 9.1** The patrol interface loads exactly zero web fonts. No Google
Fonts, no self-hosted font files, no `@font-face` declarations. Font
loading blocks rendering, introduces FOUT/FOIT, and adds network requests.
System fonts render instantly because they are already on the user's device.

### 9.2 Font Stacks

Two font stacks, both system-only:

**UI chrome** (labels, buttons, badges, metadata):

```css
font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI",
             Roboto, "Noto Sans", "Liberation Sans", sans-serif;
```

The `"Noto Sans"` entry ensures CJK and RTL coverage on Linux, where
system-ui may not include these scripts.

**Diff content** (code, wikitext, edit content):

```css
font-family: ui-monospace, SFMono-Regular, "Cascadia Code",
             "Liberation Mono", Menlo, Monaco, Consolas, monospace;
```

### 9.3 Size Rules

**Rule 9.2** No font size below 11px anywhere in the patrol interface. Dense
does not mean unreadable.

**Rule 9.3** Font sizes use the golden-ratio type scale tokens from
Section 4.3. Ad hoc `font-size` values are rejected in review.

**Rule 9.4** Line height for diff content is 1.4. For UI labels, 1.2.
Tighter line heights in the queue list (1.15) are acceptable to maximize
visible edits.

---

## 10. Internationalization (i18n)

SP42 targets every Wikipedia language edition. The interface must work in
any script direction and any language without code changes.

### 10.1 Text

**Rule 10.1** No user-visible string is hardcoded in view/component code.
Every label, button text, status message, placeholder, error message, and
tooltip is sourced from a translation layer that can be swapped per locale.

**Rule 10.2** The translation layer supports:
- Plural forms (CLDR rules: zero, one, two, few, many, other)
- Interpolation (named parameters, not positional: `{count}` not `%d`)
- Gender-neutral defaults
- Strings up to 3x the English length without layout breakage (German and
  Finnish routinely expand text by 2-3x)

**Rule 10.3** Translation keys are namespaced by surface:
`patrol.queue.empty`, `patrol.action.rollback`, `backoffice.debug.title`.
Patrol-surface keys are translated first; back-office keys may remain
English-only initially.

### 10.2 Script Direction

**Rule 10.4** The `<html>` element's `dir` attribute is set dynamically based
on the active wiki's primary script. When patrolling ar.wikipedia.org, the
entire interface is RTL. When patrolling en.wikipedia.org, it is LTR.

**Rule 10.5** All CSS uses **logical properties exclusively**. Physical
properties are banned:

| Banned | Required replacement |
|--------|---------------------|
| `margin-left` | `margin-inline-start` |
| `margin-right` | `margin-inline-end` |
| `padding-left` | `padding-inline-start` |
| `padding-right` | `padding-inline-end` |
| `text-align: left` | `text-align: start` |
| `text-align: right` | `text-align: end` |
| `float: left` | `float: inline-start` |
| `border-left` | `border-inline-start` |
| `left:` / `right:` (positioning) | `inset-inline-start` / `inset-inline-end` |

**Rule 10.6** Flexbox and grid layouts use logical axis names. `row` is
the inline axis (flips in RTL). `justify-content` operates on the inline
axis and `align-items` on the block axis — no additional RTL overrides are
needed when logical properties are used consistently.

**Rule 10.7** Icons with directional meaning (arrows, chevrons, "back"
indicators) are flipped in RTL contexts via `transform: scaleX(-1)` or by
providing RTL-specific SVG variants. A left-pointing back arrow in LTR
becomes a right-pointing back arrow in RTL.

### 10.3 Mixed-Direction Content

Wikipedia articles frequently contain mixed-direction text (an Arabic
article quoting an English source, a Hebrew article with Latin taxonomy
names). The diff panel must handle this:

**Rule 10.8** Diff lines set `dir="auto"` by default, allowing the browser's
Unicode BiDi algorithm to determine direction from the first strong
character. When explicit direction is known (e.g., from wiki markup), it
overrides the auto value.

**Rule 10.9** The queue list displays article titles in their native script.
An Arabic article title renders right-to-left within a queue row even if
the interface locale is LTR.

### 10.4 Numbers, Dates, and Times

**Rule 10.10** All dates and times use `Intl.DateTimeFormat` (or the Rust
equivalent via `js_sys`) with the active locale. No hardcoded date formats.
"2 minutes ago" relative times use `Intl.RelativeTimeFormat`.

**Rule 10.11** Numbers use `Intl.NumberFormat` with the active locale.
Arabic-Indic numerals for Arabic locale, Western numerals for English. The
score display (0-100) always uses Western numerals for cross-locale
consistency in the queue column, but context-panel metadata uses locale
numerals.

---

## 11. Accessibility (a11y)

### 11.1 Keyboard Navigation

**Rule 11.1** All interactive elements are keyboard-navigable via Tab order.
The tab order follows the patrol loop: queue -> diff -> context -> actions.

**Rule 11.2** Focus indicators are a 2px solid `--accent-info` outline with
a 2px offset. The default browser outline is never suppressed via
`outline: none`. Custom focus styles must be at least as visible as the
browser default.

**Rule 11.3** Skip-navigation links are provided: "Skip to diff", "Skip to
actions". These are visually hidden until focused.

### 11.2 Screen Readers

**Rule 11.4** The patrol layout uses ARIA landmark roles:
- Queue: `role="navigation"` with `aria-label="Edit queue"`
- Diff: `role="main"` with `aria-label="Diff viewer"`
- Context: `role="complementary"` with `aria-label="Edit context"`
- Actions: `role="toolbar"` with `aria-label="Patrol actions"`

**Rule 11.5** When a queue row is selected, the screen reader announces:
article title, score, and user identity. This uses `aria-live="polite"` on
the context panel.

**Rule 11.6** Action buttons include `aria-keyshortcuts` attributes matching
their keyboard shortcut (`aria-keyshortcuts="r"` for Rollback).

**Rule 11.7** Diff additions and deletions are annotated with
`aria-label="Added: {text}"` and `aria-label="Removed: {text}"` on their
container elements, so screen readers can distinguish them without relying
on color.

### 11.3 Motion & Vestibular

**Rule 11.8** When `prefers-reduced-motion: reduce` is active, all
transitions are instant (duration: 0ms). The queue advance animation is
suppressed. No content is lost.

### 11.4 Color & Contrast

**Rule 11.9** Score tiers (Section 6.2) always show the a11y icon column
(`!!` / `?` / checkmark) alongside color coding. Color is never the sole
channel of information.

**Rule 11.10** High-contrast mode (`prefers-contrast: more`) increases all
border widths to 2px and all text contrast to at least 7:1 (WCAG AAA).

### 11.5 Touch Targets

**Rule 11.11** All interactive elements have a minimum touch target of
44x44px (WCAG 2.5.8). In the action bar, buttons meet this by default
(Section 4.4). In the queue list, rows are at least 44px tall even if the
text content would allow shorter.

---

## 12. Motion & Transitions

**Rule 12.1** The only animation in the patrol interface is the queue advance
transition: a 120ms ease-out on the queue list when the current edit
completes. No easing curves longer than 150ms.

**Rule 12.2** No loading spinners in the patrol loop. The diff panel shows
the previous diff until the new one is ready, then swaps instantly. A
skeleton is acceptable only on first load, and only if
`prefers-reduced-motion` is not `reduce`.

**Rule 12.3** No layout shifts after initial render. Elements do not resize,
reflow, or reposition as data loads. If content is async, its container is
pre-sized.

**Rule 12.4** `transition` properties are limited to `opacity`,
`background-color`, and `transform`. No transitions on `width`, `height`,
`margin`, `padding`, or `top`/`left` — these trigger layout reflow.

---

## 13. State Communication

### 13.1 Connection State

A single status indicator at the inline-end of the session bar shows:

| State | Indicator | Color |
|-------|-----------|-------|
| Connected, stream active | Solid dot | `--accent-risk-low` |
| Connected, no new edits | Hollow dot | `--text-secondary` |
| Reconnecting | Pulsing dot (suppressed with `prefers-reduced-motion`) | `--accent-risk-medium` |
| Disconnected | X mark | `--accent-risk-high` |

**Rule 13.1** Connection state uses a 12px indicator, not a banner. Banners
consume vertical space and create layout shifts.

### 13.2 Session State

The current username and session status appear in a compact session bar
(28px height, `--space-xl` padding) above the three-column layout. This bar
also contains the wiki selector and the settings gear.

**Rule 13.2** The session bar is not part of the patrol loop. It is ambient
information. It must never compete with the diff panel for attention.

---

## 14. Zero States

**Rule 14.1** When the queue is empty, the queue panel shows "No edits
pending" (translated) with the connection indicator. It does not show
illustrations, mascots, or motivational messages.

**Rule 14.2** When offline (service worker serving cached shell), all
panels show a single-line offline indicator where live data would be:
`Offline — cached shell only` (translated). No fake data, no stale previews.

**Rule 14.3** When the session is not authenticated, the patrol view
is replaced by a single-screen auth prompt. No partial patrol UI with
disabled buttons — either you are in or you are not.

---

## 15. Back Office vs. Patrol Surface

The application has two distinct surfaces:

| | Patrol Surface | Back Office |
|---|---|---|
| **Who** | Patrollers doing edits | Developers, operators, admins |
| **Goal** | Process edits fast | Diagnose, configure, monitor |
| **Design standard** | This contract (binding) | Functional, no binding spec |
| **Density** | Dense but structured | Can be denser, tables OK |
| **Polish** | Every pixel matters | Readable is sufficient |
| **i18n** | Fully translated | English-only acceptable |
| **a11y** | Full WCAG AA | Keyboard-navigable minimum |
| **Components** | Queue, diff, context, actions | Telemetry, debug, coordination, PWA |

**Rule 15.1** The patrol surface and back office are navigable via a
single toggle or route. They never mix. Debug panels never appear on the
patrol screen. Patrol actions never appear on the back office screen.

**Rule 15.2** The patrol surface is the default route. The back office
requires an explicit navigation action (gear icon, `/debug` route, keyboard
shortcut like `Ctrl+Shift+D`).

---

## 16. Asset & Loading Budget

### 16.1 Zero External Requests for UI

**Rule 16.1** The patrol interface makes zero network requests for fonts,
icons, CSS frameworks, or third-party scripts. Everything needed to render
the UI ships in the Wasm bundle and the service worker's cache. The only
network requests are for live data (EventStreams, API calls, WebSocket).

### 16.2 CSS Architecture

**Rule 16.2** All CSS is authored as inline styles or `<style>` blocks
within Leptos components. No external CSS files, no CSS-in-JS runtime, no
PostCSS build step. This eliminates render-blocking stylesheet requests.

**Rule 16.3** Design tokens (Section 4, 8) are defined once in a root
component as CSS custom properties on `:root`. Components reference tokens
only — no hardcoded color values, spacing values, or font sizes in
component styles.

### 16.3 Icon System

**Rule 16.4** Icons are inline SVG or single Unicode characters. No icon
font files, no icon sprite sheets, no external SVG files loaded at runtime.
The patrol surface uses at most 10 distinct icons:

| Icon | Representation | Usage |
|------|---------------|-------|
| Risk high | `!!` text or inline SVG | Score tier |
| Risk medium | `?` text or inline SVG | Score tier |
| Risk low | Checkmark (U+2713) | Score tier |
| Connection status | Dot/X (CSS pseudo-element) | Session bar |
| Chevron | CSS border trick or inline SVG | Queue selection, dropdowns |
| Settings | Gear (inline SVG, ~200 bytes) | Session bar |
| User (anonymous) | IP badge (text) | Context panel |
| User (registered) | Username text | Context panel |
| Action success | Checkmark (U+2713) | Action feedback |
| Action failure | X mark (U+2717) | Action feedback |

---

## 17. Performance Budgets

These supplement CONSTITUTION.md Article 11:

| Metric | Budget | Measurement |
|--------|--------|-------------|
| First diff visible | < 1.5s | Cold start on 4G |
| Queue advance (action -> next diff) | < 300ms | Measured in-app |
| Keyboard shortcut response | < 50ms | Key press to action dispatch |
| Layout shift (CLS) | 0 | After initial render |
| Action bar paint | < 200ms | Time to interactive buttons |
| Queue list render (50 items) | < 100ms | Leptos signal update |
| External requests for UI rendering | 0 | Fonts, icons, CSS, JS libs |
| CSS custom properties (total) | < 40 | Counted at `:root` level |
| Distinct color values (patrol surface) | <= 20 | All surfaces + text + accent |

**Rule 17.1** Performance budgets are tested in CI when the E2E test
harness is in place (Playwright, per Constitution Article 1.2).

---

## 18. What This Contract Does Not Cover

- Back office panel design (telemetry, debug, coordination)
- Onboarding / tutorial flows (future)
- Settings / preferences UI (future)
- Admin-facing features (CSD, AFD, page protection — out of scope)
- Branding beyond the in-app SP42 mark
- Translation file format and build pipeline (implementation detail)
- Specific Leptos component API (implementation detail)

---

## 19. Enforcement

**Rule 19.1** Every pull request that touches patrol-surface components must
include a screenshot or recording demonstrating compliance with this
contract.

**Rule 19.2** Layout shifts, confirmation dialogs, horizontal diff
scrolling, physical CSS properties, hardcoded strings, web font imports,
and border-radius > 8px on the patrol surface are treated as bugs with the
same severity as a test failure.

**Rule 19.3** A CI lint (stylelint or equivalent) enforces the ban on
physical CSS properties (Section 10.5) and the absence of `@font-face`
declarations.

**Rule 19.4** This contract is amended through the same process as
CONSTITUTION.md (Article 12): written proposal, 7-day comment period,
unanimous approval.
