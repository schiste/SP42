# 05 — Wikiharness pure citation-verification algorithms (faithful Rust port spec)

Extracted from wikiharness `packages/core/src/` (TypeScript) on 2026-06-08, including
the `*.test.ts` files so the exact expected behavior / edge cases are pinned. Target:
faithful reimplementation in pure Rust (`sp42-core`). All seven modules below are pure
(no I/O, no clock, no randomness) and unit-tested.

Source files (READ-ONLY, do not modify):
- `packages/core/src/locate-quote.ts` (+ `.test.ts`)
- `packages/core/src/citation/body-classifier.ts` (+ `.test.ts`)
- `packages/core/src/citation/voting.ts` (+ `.test.ts`)
- `packages/core/src/citation/article.ts` (+ `.test.ts`)
- `packages/core/src/citation/prompts.ts` (+ `.test.ts`)
- `packages/core/src/citation/parsing.ts` (+ `.test.ts`)
- `packages/core/src/concurrency.ts` (+ `.test.ts`)

The `Verdict` union (from `packages/core/src/types.ts`):
```ts
export const VERDICTS = ['SUPPORTED', 'PARTIAL', 'NOT_SUPPORTED', 'SOURCE_UNAVAILABLE'] as const;
export type Verdict = (typeof VERDICTS)[number];
```
Note SP42 ADR-0007 uses a two-axis `CitationVerdict` (`Judged(SupportLevel)` |
`SourceUnavailable` | …) — the four wikiharness verdicts map onto that. Keep the
wikiharness scale for the algorithm internals (voting / parsing operate on these four
labels) and adapt at the type boundary.

---

## 1. `locateQuoteInSource` — the anti-fabrication locator

**Signature.** `locateQuoteInSource(quote: string, source: string) -> number | null`.
The return is a **code-unit offset into the ORIGINAL `source`** (JS string length =
UTF-16 code units) at which the (normalized) quote begins, or `null` if not found.
Rust note: JS `.length` and `String.indexOf` count **UTF-16 code units**, and the map
records UTF-16 offsets. For a faithful port either (a) operate on UTF-16 (`encode_utf16`
/ index into a `Vec<u16>`), or (b) decide your own offset convention (byte or char) and
document it — what matters for the gate is the **null vs. some** decision; the exact
offset value only matters if SP42 reuses it as a source-side anchor. The CLAIM-side
anchor SP42 records is `use_site_ordinal`, not this byte offset, so a char/byte offset
convention is acceptable as long as it consistently points at the match start. **The
critical invariant to port is the found/not-found decision and the normalization that
drives it.**

### Algorithm (exact order)
1. `trimmed = quote.trim()`. If `trimmed.is_empty()` → return `null`.
   (Test: `''` and `'   \n\t '` → null.)
2. **Fast path — exact substring:** `let exact = source.indexOf(trimmed)`. If
   `exact != -1` return `exact`. (Note: matches the *trimmed* quote against the *raw*
   source. Test "match at start" → 0; "Nobel Prize" with padded quote → offset 8 via
   this path since the source contains the exact trimmed substring.)
3. **Normalized path:** `normQuote = normalizeForMatch(trimmed)`. If `normQuote` is
   empty → null. Build `{normSource, map} = normalizeWithMap(source)`. `idx =
   normSource.indexOf(normQuote)`. If `idx == -1` → null. Return `map[idx]` (or null if
   out of range — defensive).

### Normalization — `normalizeForMatch(text)` (used for the QUOTE)
Iterate Unicode scalar values of `text.normalize(NFC)`. Maintain `prevSpace` flag.
- If char is whitespace (`\s` with Unicode flag — matches space, tab, newline, CR, FF,
  VT, NBSP and other Unicode whitespace): if `!prevSpace`, append a single `' '` (ASCII
  space), set `prevSpace = true`. Consecutive whitespace collapses to ONE space.
- Else: append `substitute(ch)` (quote substitution, below), `prevSpace = false`.
- Finally `out.trim()` (strips leading/trailing ASCII space).
Net effect: NFC → curly-quote substitution → whitespace runs collapse to single ASCII
space → trim.

### Quote substitutions — `substitute(ch)` (EXACT table, verbatim)
Curly/typographic → straight ASCII. Map (key codepoint → replacement):
```
'‘' U+2018 LEFT SINGLE QUOTE        -> '\''  (straight apostrophe)
'’' U+2019 RIGHT SINGLE QUOTE/APOS  -> '\''
'‚' U+201A SINGLE LOW-9 QUOTE       -> '\''
'‛' U+201B SINGLE HIGH-REV-9 QUOTE  -> '\''
'′' U+2032 PRIME                    -> '\''
'“' U+201C LEFT DOUBLE QUOTE        -> '"'
'”' U+201D RIGHT DOUBLE QUOTE       -> '"'
'„' U+201E DOUBLE LOW-9 QUOTE       -> '"'
'‟' U+201F DOUBLE HIGH-REV-9 QUOTE  -> '"'
'″' U+2033 DOUBLE PRIME             -> '"'
```
Anything not in the table is returned unchanged. NOTE: this substitution applies to
BOTH the quote (in `normalizeForMatch`) and the source (in `normalizeWithMap`).

### Source normalization with offset map — `normalizeWithMap(source)`
Returns `{ text: String, map: Vec<usize> }` where `map[i]` = the original-source offset
(UTF-16 code units in JS) of the i-th char of `text`. Algorithm:
- Iterate chars of `source` (NOT pre-normalized — NFC is applied per "unit" below).
  Track `original` running offset (in JS: `original += ch.length`, i.e. UTF-16 units of
  that scalar). For each char, `start = original` BEFORE advancing.
- **Whitespace:** flush the pending unit (see below); if `!prevSpace`, push `' '` to
  `text` and `start` to `map`, set `prevSpace = true`; continue.
- **Non-whitespace:** `prevSpace = false`; `sub = substitute(ch)`. THEN the
  **base+combining-mark unit** logic:
  - If a unit is already pending AND the current char is a combining mark
    (`\p{M}` — Unicode Mark category): append `sub` to the pending `unit` (do NOT flush;
    do NOT update `unitStart`). This lets a base char + its trailing combining marks NFC
    together as one unit.
  - Else: flush the pending unit, then start a new unit `unit = sub`, `unitStart =
    start`.
- `flushUnit()`: if `unit` is non-empty, run `unit.normalize(NFC)`, and for EACH char of
  the NFC result push that char to `text` and push `unitStart` (the SAME start offset
  for every char produced by this unit) to `map`. Reset `unit = ''`.
- After the loop, `flushUnit()` once more.

**Why per-unit NFC (load-bearing):** HTML→text extraction commonly yields *decomposed*
(NFD) Unicode (base + combining mark as separate code points). Applying NFC to each
base+marks unit recombines them so a decomposed source matches a precomposed (NFC)
quote — without this, a real quote would false-reject. NFC over the whole string would
also recombine, but the per-unit form is what preserves the precise per-char→original
offset map. For Rust, the equivalent is `unicode-normalization` crate's `.nfc()` over
each accumulated `base + Mark*` cluster, recording the cluster's start offset for every
emitted char.

### Case sensitivity — DELIBERATE
Matching is **case-sensitive** (no `.to_lowercase()` anywhere). A model that re-cases a
quote does NOT get a free pass. Test: `locateQuoteInSource('NOBEL PRIZE', 'won the
Nobel Prize')` → `null`.

### Tested edge cases (must hold)
- Exact substring → its offset (e.g. 13).
- Match at very start → 0.
- Absent quote → null (the must-reject case).
- Empty / whitespace-only quote → null.
- Whitespace differences (newlines, runs of spaces) in source still locate; returned
  offset points at the match start in the ORIGINAL source (verified by slicing original
  at offset and checking it `startsWith` the first matched word).
- Curly quotes/apostrophes in source vs straight in quote → match.
- Leading/trailing whitespace on the quote ignored.
- Case differences → null.
- NFD source + NFC quote → match; reverse (NFC source + NFD quote) → also match.

### ReDoS
No regex with unbounded backtracking; the only regex are single-char class tests
(`\s`, `\p{M}`). Safe.

---

## 2. `classifyBodyUsability` — deterministic GIGO body-usability gate

**Signature.** `classifyBodyUsability(text: string | null | undefined) ->
{ usable: bool, reason: BodyUsabilityReason }`.

```ts
type BodyUsabilityReason =
  | 'ok' | 'json_ld_leak' | 'css_leak' | 'anti_bot_challenge'
  | 'wayback_redirect_notice' | 'wayback_chrome' | 'amazon_stub' | 'short_body';
```
`reason == 'ok'` iff `usable == true`. **Never throws.** `null`/`undefined` → `{ usable:
false, reason: 'short_body' }` (a failed fetch is treated as too-short). In Rust model
input as `Option<&str>` / `&str`; `None` → `short_body`.

### Constants (exact)
```
SIGNATURE_LEN   = 500   // signature window slice for several detectors
SHORT_BODY_FLOOR = 300  // bodies shorter than this (after trim) are unusable
CHROME_LENGTH_CAP = 600 // at/above this length, the wayback_chrome detector stands down
```
`.slice(0, N)` in JS = first N UTF-16 code units. For Rust use a char/byte-safe prefix;
exactness of N only affects whether a borderline pattern is inside the window — keep N
the same and slice on a code-unit/char boundary.

### Procedure
1. If `text == null` → `{usable:false, reason:'short_body'}`.
2. `trimmed = text.trim()`.
3. Run detectors **IN ORDER** (first match wins) over `trimmed`. Each returns true =
   unusable with that reason. Order is exactly:
   `json_ld_leak`, `css_leak`, `anti_bot_challenge`, `wayback_redirect_notice`,
   `wayback_chrome`, `amazon_stub`, `short_body`.
4. No detector matched → `{usable:true, reason:'ok'}`.

### Detector 1 — `json_ld_leak`
`head = trimmed.slice(0, 500)`. TWO anchored checks over the bounded head, both must
hold:
- `/^\s*[{[]/` — head starts (after optional whitespace) with `{` or `[`.
- `/"@(context|type|graph)":/` allowing whitespace before the colon: regex
  `"@(context|type|graph)"\s*:` — a schema.org keyword present in the head.
Two separate checks (NOT one regex spanning the prefix) so nested objects/arrays before
the keyword still match. Tests: plain blob; nested-before-`@context`; array-of-objects
`[{...}]` form — all → `json_ld_leak`.

### Detector 2 — `css_leak`
`head = trimmed.slice(0, 500)`.
- Require rule-like head: `/^[\s.#@\w-]+\{[^{}]{10,}/` — selector-ish chars, then `{`,
  then ≥10 non-brace chars. If not matched → not css_leak.
- Else count CSS glyphs: `cssGlyphs = (head.match(/[{};:]/g) ?? []).length` (count of
  `{`, `}`, `;`, `:`). Return `cssGlyphs / head.length > 0.05` (strictly greater than
  5% glyph density). Test: a multi-rule `<style>` block → css_leak.

### Detector 3 — `anti_bot_challenge`
Over `trimmed.slice(0, 1500)`, **case-insensitive** regex (one alternation):
```
(Making sure you('|&#39;)re not a bot|Anubis uses a Proof-of-Work|Just a moment\.\.\.|Verifying you are human|Please enable JavaScript and cookies|Checking your browser before accessing)
```
Note: the apostrophe alternative `('|&#39;)` covers a straight apostrophe OR the HTML
entity `&#39;`. Test: `'Just a moment... Checking your browser before accessing.'`.

### Detector 4 — `wayback_redirect_notice`
Over `slice(0,1500)`: `/Got an HTTP \d{3} response at crawl time/` (case-SENSITIVE).
Test: `'Got an HTTP 302 response at crawl time (redirecting)'`.

### Detector 5 — `wayback_chrome`
- If `trimmed.length >= 600` → NOT wayback_chrome (assume real article follows the
  chrome; favor false negatives). `head = trimmed.slice(0,500)`. Match if ANY of:
  - `/^The Wayback Machine - https?:\/\//`
  - `/\d{1,9} captures\s{1,5}\d{1,2} \w{1,30} \d{4}/` (the "N captures  DD Month YYYY"
    toolbar — bounded quantifiers, ReDoS-safe)
  - `/\bCOLLECTED BY\s+Collection:/`
Tests: short "The Wayback Machine - https://…"; "123 captures  7 January 2015 …";
"COLLECTED BY Collection: …" → wayback_chrome. A Wayback prefix on a LONG (≥600) body →
**usable** (real article follows).

### Detector 6 — `amazon_stub`
Whole `trimmed`, **case-insensitive**:
```
Conditions of Use(?: & Sale)?\s{0,20}Privacy Notice\s{0,20}©\s{0,20}\d{4}-\d{4},?\s{0,20}Amazon\.com
```
Bounded `\s{0,20}` gaps (ReDoS-safe). Test: `'Conditions of Use & Sale\nPrivacy
Notice\n© 2010-2024, Amazon.com, Inc.'`.

### Detector 7 — `short_body` (catch-all)
`trimmed.length < 300` → short_body. Test: `'A one-line snippet.'` → short_body.

### Usable cases (must NOT flag)
- Real long article prose → ok.
- Wayback prefix + long body (≥600 chars) → ok.

### ReDoS
All quantifiers bounded; head/window slices bound the input. Test feeds
`'{'.repeat(5000) + '"@context'.repeat(2000)` and asserts no throw.

---

## 3. `voting` — measured ensemble agreement (the confidence replacement)

Two functions, both **throw on an empty panel** ("a vote needs voters — fail loud").
In Rust return `Result` or `panic!`/`assert!`; tests just require a thrown/erroring path
for `[]`.

### Skeptical tiebreaker rank (EXACT — higher wins on a tie at max count)
```
PARTIAL           = 4
NOT_SUPPORTED     = 3
SOURCE_UNAVAILABLE = 2
SUPPORTED         = 1
```
Mirrors wikidata-SIFT's "tie-toward-reject" — NEVER tie *up* to SUPPORTED. PARTIAL >
NOT_SUPPORTED > SOURCE_UNAVAILABLE > SUPPORTED.

### `nClassVote(verdicts: &[Verdict]) -> NClassVote`
```ts
interface NClassVote { winner: Verdict; agreement: number; counts: Record<Verdict, number>; }
```
1. Empty → throw.
2. `counts` = zeroed over all four VERDICTS, then tally each vote.
3. `maxCount = max(counts.values())`.
4. `tied = [v for v in VERDICTS if counts[v] == maxCount]` — **iterate in the fixed
   VERDICTS order** `[SUPPORTED, PARTIAL, NOT_SUPPORTED, SOURCE_UNAVAILABLE]`.
5. `winner = tied[0]`; for each `v in tied`, if `TIEBREAKER_RANK[v] >
   TIEBREAKER_RANK[winner]` set `winner = v`. (So winner = the tied verdict with highest
   tiebreaker rank.)
6. `agreement = maxCount / verdicts.len()` (fraction of panel backing the winner —
   MEASURED, not model-emitted).
Return `{winner, agreement, counts}`.
Tests: unanimous → agreement 1, full counts; clear plurality → winner=most-voted,
agreement=fraction (2/3); tie SUPPORTED vs NOT_SUPPORTED → NOT_SUPPORTED, 0.5; tie
PARTIAL vs NOT_SUPPORTED → PARTIAL.

### `binaryVote(verdicts: &[Verdict]) -> BinaryVote`
```ts
interface BinaryVote { positive: boolean; agreement: number; }
```
- "support class" = `SUPPORTED || PARTIAL`.
- `supportCount = count(isSupportClass)`.
- `positive = supportCount > verdicts.len() / 2` (**STRICT majority**; a tie is NOT a
  majority → negative, the skeptical default).
- `backing = positive ? supportCount : (len - supportCount)`.
- `agreement = backing / len`.
Tests: majority support → positive, 2/3; no support → negative, agreement 1; tie
(SUPPORTED vs NOT_SUPPORTED) → negative (skeptical), agreement 0.5.

**`agreement` = the wikiharness equivalent of SP42's `PanelAgreement`** — the fraction
of the panel backing the chosen side, computed from votes, never reported by a model.
SP42 ADR-0006 makes `PanelAgreement` a first-class persisted signal; this is the
arithmetic behind it. SP42 ADR-0006 also adds a "skeptical tiebreaker" — the
`TIEBREAKER_RANK` above is that tiebreaker.

---

## 4. Between-markers claim extraction + citation-walk (`article.ts`)

Wikiharness parses **Parsoid REST HTML** with `node-html-parser`. SP42's source
structure may differ (wikitext AST vs HTML), but the **claim-span rule is the algorithm
to port faithfully** regardless of the parser. Below: both the structural walk and the
core span rule, plus the SP42/ADR-0007 deviations flagged.

### Output types
```ts
interface ParsedCitation {
  index: number;          // 0-based document-order of the inline marker == use_site ordinal
  refId: string;          // <sup> marker id, e.g. "cite_ref-philosophy_9-1"
  noteId: string | null;  // ref-list item id it links to, e.g. "cite_note-philosophy-9"
  name?: string;          // ref name from data-mw, if any
  inProse: boolean;       // marker inside a body <p> (prose), vs infobox/table
  claim: string | null;   // the between-markers claim (null for non-prose markers)
  sourceUrls: string[];   // external http(s) URLs from the ref-list entry
}
interface ParsedArticle { title; wiki; revision; citations: ParsedCitation[]; }
```
`index` IS the document-order ordinal — SP42's `use_site_ordinal`.

### Marker collection (HTML specifics — adapt to SP42's parse model)
- A citation marker = `<sup>` with class containing `mw-ref` (`isMwRef`).
- `collectMarkers`: `querySelectorAll('sup.mw-ref')` then **filter out refs nested
  inside another mw-ref** (`hasMwRefAncestor` walks parents; if any ancestor is an
  mw-ref, drop it). Test: a ref nested inside another ref counts as ONE citation
  (the outer). Result is document order; array index == `index`.
- `noteId`: from the marker's `<a href="#...">`, the fragment after `#`.
- `name`: parse the `data-mw` JSON attribute → `.attrs.name` (best-effort; malformed
  JSON → undefined).
- `sourceUrls`: look up the ref-list `<li>` by `noteId` via `getElementById`, collect
  every `<a href>` matching `/^https?:\/\//i`, **dedupe** (`new Set`). Empty if no
  noteId / no li.

### Prose detection — `nearestProseParagraph(el)`
Walk ancestors from the marker upward:
- If an ancestor is `TABLE`/`TD`/`TH` → return null (NOT prose — tabular data, even if
  inside a `<p>` in a cell).
- Record the FIRST `<p>` ancestor seen (closest enclosing paragraph).
- Return that paragraph (or null). `inProse = (paragraph != null)`. Non-prose markers
  get `claim = null`. Test: marker in a table cell `<p>` → `inProse=false`,
  `claim=null`.

### CORE: between-markers span — `claimWithinBlock(block, refId)` (quote verbatim)
```ts
// The text in `block` from the previous ref marker (or block start) up to `refId`.
function claimWithinBlock(block: HTMLElement, refId: string): string {
  const tokens: Token[] = [];
  tokenize(block, tokens);                 // flatten block into [{text} | {ref,refId}]
  const target = tokens.findIndex((t) => t.type === 'ref' && t.refId === refId);
  if (target === -1) return '';
  let start = 0;
  for (let i = target - 1; i >= 0; i--) {  // walk back to the previous ref marker
    if (tokens[i].type === 'ref') { start = i + 1; break; }
  }
  return tokens
    .slice(start, target)                  // text BETWEEN prev marker and this one
    .filter((t) => t.type === 'text')
    .map((t) => t.value)
    .join('')
    .replace(/\s+/g, ' ')                  // collapse ALL whitespace runs to one space
    .trim();
}
```
`tokenize`: depth-first over the block's children; a TEXT node → `{type:'text', value}`;
an mw-ref element → `{type:'ref', refId}` (do NOT descend into it); any other element →
recurse. So nested non-ref markup is flattened to its text; refs become boundary tokens.

**Rule in words:** the claim a citation backs is the contiguous run of rendered prose
**from the end of the previous ref marker in the same block up to this marker** (text
*between adjacent citations*). The FIRST marker in a block takes the run from block
start (`start` stays 0). Whitespace collapsed (`/\s+/g` → single space), trimmed.
Footnote numbers are not in the prose tokens (the marker text e.g. `[1]` lives inside
the `<sup>` which is emitted as a `ref` boundary token, not text) so they are naturally
excluded. No sentence segmentation, no NLP, no model. Pure function of the parsed
structure.

### Bundled markers — wikiharness behavior (the deviation, READ CAREFULLY)
For bundled markers `[1][2][3]` with NO prose between them: in wikiharness, the second
and third markers have `target - 1` immediately a `ref` token, so `start = target` and
the slice `[start, target)` is **EMPTY** → `claim = ''` (empty string after
join/trim). Wikiharness therefore **effectively DROPS** the empty-span bundled markers
(an empty claim is not verifiable; downstream filters out `claim == ''` / `null`). Only
the FIRST of a bundle gets the real preceding prose. **Wikiharness does NOT share the
span.**

### No-URL filter
Downstream of `parseArticle`, a citation with `sourceUrls.length == 0` has nothing to
fetch/ground and is filtered before fetch (ADR-0007 Decision 7 third bullet). `article.ts`
itself just records `sourceUrls` (possibly empty); the filter lives in the verify
pipeline.

### ETag → revision — `parseRevisionFromEtag(etag) -> number | null`
Regex `/"(\d{2,})[/"]/` — first run of ≥2 digits inside the quotes, terminated by `/`
or `"`. Avoids mistaking a date for a revid. Tests: `W/"1353541055/uuid/view/html"` →
1353541055; `"no-digits/here"` → null; `"2024-06-01/no-revid-here"` → null. (HTTP-shape
detail; SP42 may not need it if it doesn't go through Parsoid REST ETags.)

### Tested facts on the real fixture (Markdown article)
61 citations in document order; index[0]==0; every refId starts `cite_ref-`, noteId
(when present) starts `cite_note-`; >10 prose claims; non-prose (infobox/table) markers
all have `claim==null`; >20 citations have external http(s) URLs; daringfireball.net is
among them.

---

## DEVIATION FLAGS — ADR-0007 vs wikiharness (implement the ADR rule where they differ)

ADR-0007 Decision 7 (`SP42-adr-citation/docs/adr/0007-citation-verification-semantics.md`,
§7) adopts the wikiharness between-markers base rule but **deliberately changes two
behaviors**. Port the **ADR-0007 rule**, not the wikiharness behavior, for these:

1. **Bundled markers — SHARE, do not DROP.** ADR-0007 §7 (and the Definition-of-Done
   bullet): "Bundled citations (markers with no prose between them, e.g. `[1][2][3]`)
   all back the **same** claim: the extractor walks back **past the zero-text markers**
   to the real preceding prose, so each bundled marker is its own use-site verified
   against that shared claim — **not dropped**." Wikiharness instead yields an EMPTY span
   for the 2nd/3rd bundled markers and drops them. **SP42 implements SHARE:** when the
   between-markers slice is empty, keep walking back past consecutive ref tokens until a
   non-empty prose run is found, and assign THAT run to this marker too. ADR-0007
   explicitly notes this "gets its own SP42 test, since wikiharness does not validate
   it." (Concretely: replace the `for` loop's early `break` so that an empty resulting
   span continues walking back past the previous marker(s) to the nearest real prose
   run, rather than returning `''`.)

2. **Maintenance-tag stripping.** ADR-0007 §7: "footnote numbers and maintenance tags
   (e.g. *[citation needed]*) are stripped." Wikiharness's `claimWithinBlock` strips
   footnote numbers only incidentally (they're inside the `<sup>` ref boundary, not
   text) and does **NOT** explicitly strip maintenance tags like `[citation needed]`,
   `[dubious]`, `[clarification needed]` — those are separate `<sup>`/`<span>` markup
   that may surface as text depending on the parse. **SP42 must explicitly strip
   maintenance tags** from the extracted span. (Follows alex-citation-checker, per
   ADR-0007's "Two refinements above follow alex-citation-checker specifically.")

3. **Non-prose markers / no-fetchable-source — same as wikiharness** (skip non-prose;
   filter out use-sites with no fetchable URL before fetch). No deviation; confirm
   parity.

Everything else (the base between-markers span, whitespace collapse, document-order
ordinal == use_site_ordinal, language-agnostic / no sentence segmentation) is the SAME —
port faithfully. ADR-0007 also retracted a sentence-bounding refinement (deferred
post-port — needs a language-specific sentence-boundary detector), so do NOT add
sentence segmentation.

---

## 5. `buildVerifyPrompt` — the gold two-step verification prompt

**Signature.**
```ts
interface VerifyPromptInput { claim: string; sourceText: string; sourceUrl: string; metadata?: CitoidMetadata; }
buildVerifyPrompt(input) -> [{role:'system', content:SYSTEM}, {role:'user', content:USER}]
```
Always returns exactly two messages in order: system, then user.

### SYSTEM message — FULL TEXT (verbatim, port character-for-character)
```
You verify whether a cited SOURCE supports a CLAIM from a Wikipedia article.

Judge using ONLY the text of the provided source. Do NOT use outside knowledge, and do
NOT assume facts that are not present in the source.

Use this two-step process for every claim.

STEP 1 — Source check:
Determine whether the source text contains usable article body content: real paragraphs,
quotes, narrative passages, or factual statements. This holds true even when that content is
surrounded by navigation, headers, footers, web.archive.org captures, or other page chrome.

The source is NOT usable if it contains only: a library/database catalog page (Google Books,
WorldCat, a JSTOR preview), a paywall, a login wall, a 404, a cookie/consent notice, an
anti-bot challenge, or bibliographic metadata with no article body.

Long sources may arrive as an excerpt — gaps between paragraphs, blank lines, text ending
mid-sentence, or passages separated by "..." are NORMAL and mean "not shown here", not "failed
to load". Brevity alone is not a SOURCE_UNAVAILABLE signal: if any article prose is present,
evaluate it. If STEP 1 fails, return SOURCE_UNAVAILABLE and do NOT attempt STEP 2.

STEP 2 — Claim verification:
Identify what the claim asserts (specific dates, numbers, names, events, attributions), then
look in the source for support, contradiction, or partial coverage.
- DATES: the source must contain the date in some form. Equivalent expressions count —
  "Wednesday" supports "January 7, 2026" if the article is dated that day; "7 Jan 2026" counts
  for "7 January 2026".
- NUMBERS, NAMES, QUOTED statements: the source must contain that specific number/name/quote,
  or a directly equivalent paraphrase.
- Accept paraphrasing and direct implications, but NOT speculative inferences or logical leaps.
- Distinguish definitive statements from hedged language ("it is believed", "some sources
  suggest"). A claim stated as fact requires source text that is also definitive.
- Names from non-Latin scripts have multiple valid romanizations; treat transliteration
  variant spellings of the same name ("Chekhov"/"Tchekhov") as equal, not as factual errors.

Return exactly one verdict from this graded scale:
- SUPPORTED — the source contains all of the claim's specific assertions (paraphrase OK if substance matches).
- PARTIAL — the source addresses the claim but contains only some of its assertions, OR asserts it only with hedged/uncertain language.
- NOT_SUPPORTED — the source addresses the topic but contradicts the claim, or has no evidence for its specific assertions.
- SOURCE_UNAVAILABLE — STEP 1 failed: no usable article body.

For SUPPORTED or PARTIAL you MUST quote a short, VERBATIM span copied exactly (character for
character) from the source that backs the claim. Never paraphrase, reword, or invent the quote.
If you cannot find such a verbatim span, the verdict is NOT_SUPPORTED.

Do NOT output any confidence score, probability, or percentage — only the categorical verdict
and the verbatim quote.

Respond with a single JSON object: {"verdict": "<one of the four>", "quote": "<verbatim span or empty>"}.

Examples:

Claim: "The company was founded in 1985 by John Smith."
Source: "Acme Corp was established in 1985. Its founder, John Smith, served as CEO until 2001."
{"verdict": "SUPPORTED", "quote": "Acme Corp was established in 1985. Its founder, John Smith"}

Claim: "The committee published its findings in 1932."
Source: "History of Modern Economics - Google Books Sign in ... My library Help Advanced Book Search"
{"verdict": "SOURCE_UNAVAILABLE", "quote": ""}

Claim: "The bridge was completed in 1998."
Source: "The Morrison Bridge broke ground in 1994. The bridge was finally opened to traffic in August 2002, four years behind schedule."
{"verdict": "NOT_SUPPORTED", "quote": "finally opened to traffic in August 2002"}

Claim: "The treaty was signed in Paris."
Source: "It is believed the treaty was signed in Paris, though some historians dispute this."
{"verdict": "PARTIAL", "quote": "It is believed the treaty was signed in Paris"}
```

### USER message template (exact construction)
```
CLAIM:
{claim}

{metadataSection}SOURCE ({sourceUrl}):
"""
{sourceText}
"""

Respond with the JSON object described in the instructions.
```
- `{metadataSection}` is `''` when `metadata` is `undefined` (then the prompt is
  byte-identical to the no-metadata form). When present, it is the rendered block from
  `metadataSection()` BELOW, which itself ends with a blank line so it slots cleanly
  before `SOURCE (...)`.
- The source body is fenced in triple-double-quotes (`"""` … `"""`).

### Two-step framing
STEP 1 = source-usability check (short-circuits to `SOURCE_UNAVAILABLE` and forbids
STEP 2 if it fails). STEP 2 = claim verification with the date/number/name/hedged/
transliteration rules. This is the SIFT "no fetch, no verdict" discipline at prompt
level — it ports across cheap open models.

### Metadata "context only — DO NOT quote" section — `metadataSection(meta)`
Renders Citoid bibliographic metadata as a **clearly-labeled, context-only** block so
the model sees publication/author/date/title but a quote drawn from it CANNOT pass the
grounding gate (because grounding hashes/locates only `sourceText`, never the metadata).
Fields, in this fixed order, each rendered ONLY if defined:
```
- publication: {meta.publication}
- published: {meta.published}
- author: {meta.author}
- title: {meta.title}
```
(Build `lines` by filtering out undefined values and joining with `\n`.) If NO field is
present (`lines == ''`) → return `''` (no section at all). Otherwise return:
```
SOURCE METADATA (bibliographic context only — DO NOT quote from here; your supporting quote MUST come verbatim from the SOURCE text below):
{lines}

```
(Note the TRAILING blank line — the section is followed by one empty line before the
`SOURCE (...)` line in the user template.) `CitoidMetadata` here is consumed for the
fields `publication`, `published`, `author`, `title` (all `string | undefined`); a `url`
field exists on the type but is not rendered.

**Anti-contamination invariant (load-bearing for SP42):** metadata is NEVER part of the
content-addressed source bytes the grounding gate (`locateQuoteInSource` / object store)
sees — only `sourceText` is. So even a model that ignores the "do not quote" instruction
and quotes a metadata field is caught by the gate (the field is not in the grounded
body). The unsafe `prependMetadataHeader` (which concatenated metadata INTO the grounded
bytes) was DELETED. Implement metadata strictly as prompt context + display, never as
groundable bytes.

### Tested assertions
- system+user roles in order; user contains claim, source text, source URL.
- system contains "only", a "do not/never/must not", "verbatim".
- system names all four verdicts; FORBIDS confidence numbers (matches
  /do not output any confidence|no confidence|never (output|emit).*confidence/); must
  NOT request a 0-100 / "on a scale" rating.
- has STEP 1 / STEP 2; step-1 short-circuit phrasing; ≥1 worked example with
  `"verdict":`; keeps transliteration / hedged / date-equivalence / paraphrase /
  speculation rules.
- metadata present → user contains the field values, the word "metadata", and a
  /do not quote|not quote|context only/ phrase. metadata absent → user does NOT contain
  "metadata".

---

## 6. Verdict parser — model text → graded verdict (`parsing.ts`)

Two exported functions.

### `canonicalizeVerdict(raw: string) -> Verdict | null`
1. Normalize: `t = raw.toLowerCase().replaceAll('_',' ').replace(/\s+/g,' ').trim()`
   (lowercase, underscores→spaces, collapse whitespace, trim). If `t` empty → null.
2. Test in THIS ORDER (first match wins):
   - **SOURCE_UNAVAILABLE** if: `/\b(unavailable|inaccessible)\b/` OR
     `/(could ?n.?t|cannot|can.?t|unable to|failed to|no) (access|retrieve|reach|load|fetch)/`.
   - **NOT_SUPPORTED** if: `/\bunsupported\b/` OR `/\bnot supported\b/` OR
     `/\bcontradict/` OR `/\brefut/`.
   - **PARTIAL** if: `/\bpartial/` OR `/\bpartly\b/`.
   - **SUPPORTED** if: `/\bsupported\b/` OR `/\bsupports\b/` OR `/\bconfirm/`.
   - else → null.
Order matters: SOURCE_UNAVAILABLE and NOT_SUPPORTED are checked before SUPPORTED so
"not supported" / "could not access" don't fall through to the `supported` branch.
Tests: canonical labels pass through; case/whitespace insensitive; "fully supported"→
SUPPORTED, "partially supported"/"partly supported"→PARTIAL, "unsupported"/
"contradicted"→NOT_SUPPORTED, "source unavailable"/"could not access the source"→
SOURCE_UNAVAILABLE; "banana"/""→null.

### `parseVerdictResponse(text: string) -> { verdict; quote? } | null`
```ts
interface ParsedVerdict { verdict: Verdict; quote?: string; }
```
1. **JSON candidates first** (`jsonCandidates(text)`, in this order):
   - Fenced block: `/```(?:json)?\s*([\s\S]{0,50000}?)```/i` (bounded lazy body —
     ReDoS-safe) → push capture group 1 if present.
   - Brace span: `first = text.indexOf('{')`, `last = text.lastIndexOf('}')`; if
     `first != -1 && last > first` push `text.slice(first, last+1)`.
2. For each candidate: `JSON.parse` (skip on parse error). If result is an object with a
   `verdict` key: `canonicalizeVerdict(asString(record.verdict))` (non-string verdict →
   treated as empty string → null). If it canonicalizes:
   - quote = first defined of `record.quote ?? record.supporting_quote ??
     record.evidence`, but only if it's a non-empty string after trim → `quote = raw.trim()`;
     else `undefined`.
   - return `{verdict, quote?}` (quote field omitted when undefined).
   If the verdict field does NOT canonicalize, do NOT return — fall through to prose
   scan.
3. **Prose fallback:** `verdict = canonicalizeVerdict(text)` (scan the whole text). If
   null → return null. Else `quote = firstQuotedSpan(text)`.
4. `firstQuotedSpan(text)`: regex `/["“]([^"”]{1,2000})["”]/` (opening straight or
   left-curly double quote, 1–2000 non-quote chars, closing straight or right-curly) →
   `m[1].trim()` if non-empty, else undefined. Bounded length (≤2000) avoids
   backtracking risk.

**Default-to-not_supported note:** the PARSER itself returns `null` when no verdict can
be recovered — it does NOT default to NOT_SUPPORTED. The "if you cannot find a verbatim
span, the verdict is NOT_SUPPORTED" rule lives in the PROMPT (model side) and the
downstream grounding gate (an unlocatable quote → suppressed), not in this parser. The
caller decides what a `null` parse means (in wikiharness the pipeline treats an
unparseable/ungrounded response as a failed/suppressed verdict). Port the parser to
return `Option<ParsedVerdict>` and handle null at the call site exactly as wikiharness
does.

Tests: plain JSON; ```json fence ignoring surrounding prose; `supporting_quote` field
name; markdown emphasis `**NOT_SUPPORTED**` + a quoted span recovered as the quote; loose
prose "partially supported" → `{verdict:'PARTIAL'}` (no quote); pure garbage → null;
invalid JSON verdict value ("banana") but prose says "supported" → falls back to prose →
SUPPORTED.

---

## 7. `mapWithConcurrency` — bounded worker pool

**Signature.**
```ts
mapWithConcurrency<T,R>(items: readonly T[], limit: number, fn: (item:T, index:number)=>Promise<R>): Promise<R[]>
```
- Results returned in **INPUT order** (results[i] corresponds to items[i]).
- `effectiveLimit = Number.isFinite(limit) ? max(1, floor(limit)) : 1` — a non-finite
  limit falls back to 1, so a bad value never spawns zero workers. (Rust: clamp to
  `>=1`; there's no NaN/Inf for `usize` but keep the `max(1, …)` floor.)
- Spawn `min(effectiveLimit, n)` workers; each worker pulls the next index off a shared
  `cursor` (post-increment) and `results[i] = await fn(items[i], i)` until `cursor >= n`.
- Empty input → empty output, `fn` never called.
Rust equivalent: a bounded-concurrency map preserving input order — e.g. `futures`
`buffered`/`buffer_unordered` with index reassembly, or `tokio` + a semaphore +
`join_all` writing into a pre-sized `Vec<Option<R>>` by index, or a shared atomic cursor
+ `min(limit,n)` tasks. The contracts to preserve: (1) output order == input order,
(2) at most `limit` concurrent `fn` calls, (3) limit floored to ≥1, (4) empty → empty,
fn never called.
Used by `citation-verify` (≤3 per citation) and the benchmark runner — it is the ONE
worker-pool primitive in the repo.

Tests: out-of-order completion still input-ordered; peak concurrency ≤ limit; empty →
empty + 0 calls; `Number.POSITIVE_INFINITY` limit → still works (falls back to 1).

---

## Port checklist (what to start with)
1. `locate_quote` (anti-fabrication gate) — the non-negotiable invariant; port
   normalization (NFC per base+marks unit, quote-substitution table, whitespace
   collapse, trim, case-sensitive) + the found/not-found decision. Decide offset
   convention (byte/char) and document.
2. `voting` (`n_class_vote` + `binary_vote`) — exact tiebreaker ranks, strict-majority,
   empty-panel error; this is `PanelAgreement`'s arithmetic.
3. `body_classifier` — seven detectors IN ORDER, constants 500/300/600, verbatim
   regexes, None→short_body, never panics.
4. `parsing` (`canonicalize_verdict` + `parse_verdict_response`) — branch ORDER,
   JSON-then-prose recovery, Option (no default-to-NOT_SUPPORTED in the parser).
5. `prompts` (`build_verify_prompt`) — verbatim SYSTEM text, USER template, metadata
   context-only section (never groundable).
6. `claim extraction` — between-markers rule, BUT implement the **ADR-0007 deviations**:
   bundled markers SHARE the preceding span (not drop), and explicitly strip maintenance
   tags. Adapt the structural walk to SP42's parse model; keep `use_site_ordinal` =
   document-order index.
7. `map_with_concurrency` — input-order, bounded, limit≥1, empty→empty.
