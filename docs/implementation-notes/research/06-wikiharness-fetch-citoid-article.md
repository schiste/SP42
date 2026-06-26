# 06 — wikiharness fetch / Citoid / article-fetch — exact HTTP shapes for the Rust edge port

Research notes extracted verbatim from the wikiharness TypeScript codebase
(`/var/home/louie/Projects/Volunteering-Consulting/wikiharness`) for a faithful Rust port of
the source-fetch / Wayback-recovery / Citoid-sidecar / article-fetch+parse edges into SP42.

Scope reminder (ADR-0009 first cut): **HTML pages + existing Wayback snapshots only.**
Anything PDF or Save-Page-Now (SPN) is OUT — flagged inline below.

All file paths below are absolute under that wikiharness root unless noted.

---

## 0. Edge interfaces (the build/parse split to mirror in Rust)

Source: `packages/core/src/edges.ts`.

```ts
type HttpMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'HEAD';

interface HttpRequest {
  method: HttpMethod;
  url: string;
  headers?: Readonly<Record<string, string>>;
  body?: string;                       // serialized form/JSON string
}
interface HttpResponse {
  status: number;
  headers: Readonly<Record<string, string>>;   // response header names are LOWERCASED by the real client (see §6)
  body: string;                                  // already decoded to UTF-8 text (await res.text())
}
interface HttpClient { fetch(req: HttpRequest): Promise<HttpResponse>; }
```

Discipline to copy in Rust: each external call is split into a **pure `build_*Request`** (no I/O,
returns the request struct) and a **pure `parse_*Response`** (no I/O, takes the body string). The
actual `fetch` is the only impure edge. This is exactly what makes citoid/source-fetch/article-fetch
unit-testable with zero network.

`ObjectStore` (content-addressed cache; the anti-hallucination grounding store):
```ts
interface ObjectStore {
  put(content: string): Promise<string>;  // returns the content hash (sha256 hex, 64 chars)
  get(hash: string): Promise<string | undefined>;
  has(hash: string): Promise<boolean>;
}
```
Grounding identity = sha256 hex of the extracted text. Test asserts `contentHash` matches
`/^[0-9a-f]{64}$/` and equals `sha256Hex(extractedText)`.

`HtmlExtractor` edge (main-content extraction; optional in the fetch path):
```ts
interface HtmlExtractor { extract(html: string, url: string): Promise<string>; }
```

---

## 1. Article fetch — EXACT endpoint, params, headers, and parse output

### 1a. The URL (Parsoid REST v1 HTML — NOT action=parse, NOT raw Parsoid host)

Source: `packages/core/src/citation/urls.ts` `buildArticleHtmlUrl`.

```ts
function buildArticleHtmlUrl(wiki, title, revision?) {
  assertWikiCode(wiki);
  const base = `https://${wiki}.wikipedia.org/api/rest_v1/page/html/${encodeURIComponent(title)}`;
  return revision === undefined ? base : `${base}/${revision}`;
}
```

EXACT patterns (verified by `urls.test.ts`):
- No revision:  `https://en.wikipedia.org/api/rest_v1/page/html/Markdown`
- With title spaces + revision: `buildArticleHtmlUrl('en','Foo Bar',123)` →
  `https://en.wikipedia.org/api/rest_v1/page/html/Foo%20Bar/123`

Notes for the Rust port:
- Host is `https://{wiki}.wikipedia.org`. `{wiki}` is interpolated into the HOST → **SSRF risk**;
  it MUST be validated first (see §1b).
- Title is **URL-encoded with `encodeURIComponent`** semantics. JS `encodeURIComponent` encodes
  space → `%20` (NOT `+`), and leaves unescaped: `A-Za-z0-9 - _ . ! ~ * ' ( )`. Use a Rust
  percent-encode set that matches (`AsciiSet` excluding those chars), NOT `application/x-www-form-urlencoded`.
- Revision, when pinned, is appended as a trailing path segment `/{revision}`.
- This is the **Parsoid HTML** REST endpoint (`/api/rest_v1/page/html/...`). It returns Parsoid-
  annotated HTML (with `data-mw`, `mw-ref` classes, `cite_ref-*` / `cite_note-*` ids) that the parser
  in §1d depends on. Do NOT substitute `action=parse` output — its DOM shape differs.

### 1b. Wiki-code validation (SSRF guard) — port verbatim

Source: `packages/core/src/citation/urls.ts`.

```ts
const WIKI_CODE =
  /^[a-z]{2,3}(-[a-z]{2,8})?$|^(?:simple|test|test2|beta|commons|meta|species|incubator)$/;

function assertWikiCode(wiki) {
  if (!WIKI_CODE.test(wiki)) throw new Error(`invalid wiki code ${JSON.stringify(wiki)} ...`);
}
```
Accepts: `en`, `simple`, `pt`, `zh-yue`, `commons` (test confirms). Rejects (must throw):
`evil.com/`, `x.attacker.com#`, `en.wikipedia.org.evil`, `a/b`, `../../etc`.
Port this regex EXACTLY before any host interpolation.

Also: `TEST_WIKIS = ['test','test2']` and `isTestWiki(wiki)` exist for the WRITE allowlist
(`adr/0008`) — not relevant to read fetch, but note they are the code-level "production wikis
forbidden" enforcement.

### 1c. HTTP method + headers

Source: `packages/tools/src/citation/get-article.ts` `getArticleTool.execute`.

- Method: **GET** (`ctx.http.fetch({ method: 'GET', url })`).
- No request headers are added at the tool layer; the **User-Agent is injected by the HttpClient
  edge** (§6). That is the only required header.
- Response acceptance: throw unless `200 <= status < 400`.

### 1d. Determining the revision (no fabrication) — three-step fallback

After the GET, the revision is resolved (when not pinned in the arg) in this exact priority:

1. `revision` arg if provided.
2. Else parse from the **`ETag`** response header via `parseRevisionFromEtag`.
3. Else the **`content-revision-id`** response header, trimmed, IF it matches `/^\d+$/` → `Number`.
4. Else **throw** (`get_article: could not determine the revision ...`). Never fabricate `0`.

Header lookup is **case-insensitive** (`headerValue` lowercases names; test confirms a mixed-case
`ETag` is read).

`parseRevisionFromEtag` (source `packages/core/src/citation/article.ts`):
```ts
function parseRevisionFromEtag(etag) {
  const m = /"(\d{2,})[/"]/.exec(etag);   // first run of >=2 digits inside the quotes, terminated by / or "
  return m ? Number(m[1]) : null;
}
```
Verified cases:
- `W/"1353541055/f8b582cf-5e1a-11f1/view/html"` → `1353541055`
- `"no-digits/here"` → `null`
- `"2024-06-01/no-revid-here"` → `null` (the `[/"]` terminator stops a date from matching — `2024`
  is followed by `-`, not `/` or `"`).
Real REST ETag form is weak: `W/"<revid>/<uuid>/view/html"`.

### 1e. Provenance recorded for the article fetch

```ts
provenance = {
  sources: [{ url, fetchedAt: now, contentHash: await ctx.objects.put(res.body) }],
  createdAt: now,
  toolName: 'get_article',
};
```
`contentHash` is over the RAW HTML body (article fetch stores raw HTML, unlike fetch_source which
stores extracted text).

### 1f. How the parsed article exposes citation use-sites + their source URLs

Source: `packages/core/src/citation/article.ts` (`parseArticle`). Verified against a REAL recorded
Parsoid fixture `fixtures/en-Markdown.rest-html.cassette.json` (61 citations for enwiki Markdown).

Output shapes:
```ts
interface ParsedCitation {
  index: number;          // 0-based document order of the inline marker
  refId: string;          // the <sup> marker id, e.g. "cite_ref-philosophy_9-1"
  noteId: string | null;  // reference-list item id it links to, e.g. "cite_note-philosophy-9"
  name?: string;          // ref name from data-mw, if any
  inProse: boolean;       // true iff inside a body <p> and NOT inside any table/td/th
  claim: string | null;   // text "between adjacent citations" (null for non-prose markers)
  sourceUrls: string[];   // external http(s) URLs from the reference-list entry (de-duped)
}
interface ParsedArticle { title; wiki; revision; citations: ParsedCitation[]; }
```

Parsing algorithm (port deterministically; uses an HTML parser, `node-html-parser`):

1. **Find markers:** all `sup.mw-ref` elements (`tagName == SUP` AND class contains `mw-ref`), in
   document order, EXCLUDING any marker nested inside another `mw-ref` (a ref-in-ref is not its own
   citation). `ordinal/index == array position`.
2. **`refId`** = the marker's `id` attribute (`''` if absent).
3. **`noteId`** = fragment of the marker's child `<a href="#...">` (the part after `#`), else `null`.
   This links the in-prose marker to its entry in the reference list.
4. **`name`** = parse the `data-mw` attribute as JSON, read `.attrs.name` (best-effort; absent on
   JSON failure).
5. **`inProse` / nearest prose paragraph:** walk ancestors. If any ancestor is `TABLE`/`TD`/`TH` →
   NOT prose (`inProse:false`, `claim:null`) — an infobox/table marker's "claim" would be tabular
   data, not a verifiable sentence. Otherwise the nearest `<p>` ancestor is the prose block;
   `inProse = (block !== null)`.
6. **`claim` ("between adjacent citations"):** tokenize the prose block into a flat stream of
   `{text}` and `{ref:refId}` tokens (recursively, treating nested non-ref elements as transparent
   text; mw-ref markers become ref tokens). Find the target ref token; walk backwards to the
   previous ref token (or block start); the claim is the concatenation of the TEXT tokens strictly
   between that previous-ref boundary and the target, then `.replace(/\s+/g,' ').trim()`. (So a
   sentence with two refs attributes the text between them to the second ref.)
7. **`sourceUrls`:** look up the reference-list `<li>` by `noteId` (`getElementById(noteId)`); collect
   every descendant `<a href>` whose href matches `/^https?:\/\//i`; **de-dupe** (`new Set`). Empty
   when `noteId` is null or the entry has no external links.

Fixture-verified expectations (the Markdown article): 61 citations; >20 have `sourceUrls`; >10 prose
citations carry claims; some non-prose (infobox) citations have `claim===null`; the spec URL
`daringfireball.net` appears; all source URLs match `^https?://`.

**Citation use-site = (claim, sourceUrls) at a given `index`/`refId`.** This is the SP42 "use-site"
unit. For the resolve step, the relevant input is `ParsedCitation.sourceUrls`.

---

## 2. resolve_citation_url — choosing the source URL to fetch

Source: `packages/core/src/citation/urls.ts` (`resolveCitationUrl`, `isArchiveUrl`); tool wrapper
`packages/tools/src/citation/resolve-citation-url.ts`. Pure, read-only.

```ts
interface ResolvedUrl { url: string; isArchive: boolean; }

function resolveCitationUrl(sourceUrls: readonly string[]): ResolvedUrl | null {
  const live = sourceUrls.find((u) => !isArchiveUrl(u));
  if (live !== undefined) return { url: live, isArchive: false };
  const archive = sourceUrls[0];
  return archive === undefined ? null : { url: archive, isArchive: true };
}
```

Rule, exactly: **prefer the first LIVE (non-archive) URL; else fall back to the first URL (treated
as archive); else `null`.** This is a *preference for the live URL over the archive snapshot* — NOT
template-param parsing. There is **no `archive_url`/template-param extraction** here: the input is
already the flat `sourceUrls` array harvested from the reference-list `<a href>`s in §1f. (The doc
comments note sfn/Harvard short-cite following + page-number extraction are deferred refinements,
NOT implemented. There is no preference logic that picks `archive_url` from a `{{cite web}}` template
— that is post-MVP.)

`isArchiveUrl` (host-based, NOT substring — important to avoid misclassifying live URLs whose path
mentions an archive host):
```ts
const ARCHIVE_HOST_SUFFIX =
  /(?:^|\.)(?:web\.archive\.org|webcitation\.org|archive\.(?:today|ph|is|li))$/i;

function isArchiveUrl(url) {
  let parsed; try { parsed = new URL(url); } catch { return false; }   // unparseable → not archive
  const host = parsed.hostname.toLowerCase();
  if (ARCHIVE_HOST_SUFFIX.test(host)) return true;
  // archive.org itself is only a Wayback snapshot when the PATH starts with /web/
  return (host === 'archive.org' || host.endsWith('.archive.org'))
      && parsed.pathname.startsWith('/web/');
}
```
Verified: `web.archive.org/...`, `archive.today/...`, `archive.ph/...` are archives; `archive.org/web/20200101/...`
IS an archive (path `/web/`); `archive.org/details/somebook` is NOT (a usable IA item, not a snapshot);
`https://example.org/web.archive.org/notreally` is NOT; `https://archive.org.evil.example/x` is NOT;
non-URL string → NOT. Port the host-suffix regex AND the `archive.org` + `/web/`-path special case
exactly.

---

## 3. rewriteWaybackUrl — the EXACT id_ raw-snapshot rewrite rule

Source: `packages/core/src/citation/urls.ts`.

```ts
function rewriteWaybackUrl(url: string): string {
  return url.replace(/^(https?:\/\/web\.archive\.org\/web\/\d{14})\//, '$1id_/');
}
```

EXACT rule: at the **start of the string**, match `http://` or `https://`, then
`web.archive.org/web/`, then **exactly 14 digits** (the YYYYMMDDhhmmss timestamp), then a literal
`/`. Replace that trailing `/` with `id_/` (i.e. capture group 1 + `id_/`). Only the FIRST match,
anchored at start.

Why: the `id_` flag serves the archived page WITHOUT Wayback wrapper chrome (toolbar/banner/footer),
so the extractor captures the inner article, not the wrapper.

Idempotency / safety (verified by `urls.test.ts`):
- `https://web.archive.org/web/20200101120000/http://example.com/a`
  → `https://web.archive.org/web/20200101120000id_/http://example.com/a`
- `http://...` works too (the `https?` branch).
- Already-flagged `...20200101120000id_/...` → UNCHANGED (the digit-run is followed by `i`, not `/`).
- Other flag `...20200101120000im_/...` → UNCHANGED.
- A non-Wayback URL whose path merely mimics the shape (`https://example.com/web/20200101120000/x`)
  → UNCHANGED (anchored at start + literal `web.archive.org` host).

Rust port: regex `^(https?://web\.archive\.org/web/\d{14})/` → replace first match's group 1 + `id_/`.
Use a single replace-first (not replace-all).

---

## 4. recoverWaybackBody — salvage the inner article from surviving Wayback chrome

Source: `packages/core/src/citation/source-fetch.ts`.

```ts
const WAYBACK_PREAMBLE = /The Wayback Machine - https:\/\/web\.archive\.org\/[^\s]+/;
const MIN_PREFIX = 200;     // chars up to & incl. preamble before we trust it's a real banner
const MIN_REMAINDER = 500;  // chars of remaining content for the slice to be worth taking

function recoverWaybackBody(text: string): string {
  const m = WAYBACK_PREAMBLE.exec(text);
  if (m?.index === undefined) return text;          // no banner → unchanged
  const cutAt = m.index + m[0].length;
  if (cutAt < MIN_PREFIX) return text;              // banner too early → false positive → unchanged
  const remainder = text.slice(cutAt).trim();
  if (remainder.length < MIN_REMAINDER) return text;// too little after → unchanged
  return remainder;                                  // the inner article, trimmed
}
```

When/how it runs: this is a TEXT-stage recovery applied AFTER fetch + AFTER html-extraction (it
operates on the extracted text, not raw bytes). It is a complement to (3): (3) avoids the chrome by
rewriting the URL; (4) recovers if a banner survived anyway. It has NO request shape of its own — it
takes a string, returns a string.

EXACT preamble regex: literal `The Wayback Machine - https://web.archive.org/` followed by one or
more non-whitespace chars (`[^\s]+`). Note: the regex is `https:` only (not `https?`); it scans the
EXTRACTED text banner, which is always the https form.

Gates (verified by `source-fetch.test.ts`):
- Marker present + prefix length (cut point) >= 200 + remainder (after trim) >= 500 → return remainder.
- No marker → unchanged.
- Remainder < 500 → unchanged (`short tail`).
- Marker at index 0 (cut point < 200) → unchanged (likely false positive).

Rust port: `find` the preamble pattern, compute `cut = match.start + match.len`, apply the two length
gates against the ORIGINAL string's `cut` (>=200) and the trimmed remainder (>=500). `slice` is by
**UTF-16 code units in JS**; for ASCII banners this equals byte/char offsets, but be careful with
multibyte — slice on a char boundary in Rust and compare `.chars().count()` lengths to stay faithful
to the 200/500 thresholds (JS `.length` is UTF-16 units). For the banner+article these are ASCII so
it rarely matters, but document the difference.

---

## 5. Citoid — endpoint, query, parsed fields, do-not-quote header, NEVER grounded

Source: `packages/core/src/citation/citoid.ts`; prompt rendering `packages/core/src/citation/prompts.ts`;
wiring `packages/tools/src/citation/fetch-source.ts`.

### 5a. Endpoint + request

```ts
const CITOID_ENDPOINT = 'https://en.wikipedia.org/api/rest_v1/data/citation/mediawiki-basefields/';

function buildCitoidRequest(sourceUrl: string): HttpRequest {
  return { method: 'GET', url: CITOID_ENDPOINT + encodeURIComponent(sourceUrl) };
}
```
EXACT URL: `https://en.wikipedia.org/api/rest_v1/data/citation/mediawiki-basefields/` + the
`encodeURIComponent`-encoded source URL appended directly (no `?` query — it's a path segment).
Format `mediawiki-basefields` is the Zotero base-fields shape (more consistent than raw `mediawiki`).
Method GET. Verified by `citoid.test.ts`:
`buildCitoidRequest('https://example.com/a?b=c').url ===
 'https://en.wikipedia.org/api/rest_v1/data/citation/mediawiki-basefields/' +
   encodeURIComponent('https://example.com/a?b=c')`.

Important wiring detail (`fetch-source.ts`): Citoid is queried on the **ORIGINAL citation URL
(`p.url`)**, NOT the Wayback-rewritten `id_` URL — deliberate (Citoid resolves the real publication
better than a raw archive form).

### 5b. Response parse

```ts
type CitoidRaw = Readonly<Record<string, unknown>>;

function parseCitoidResponse(body: string): CitoidRaw | null {
  let parsed; try { parsed = JSON.parse(body); } catch { return null; } // invalid JSON → null
  if (!Array.isArray(parsed) || parsed.length === 0) return null;       // non-array / empty → null
  const first = parsed[0];
  return (typeof first === 'object' && first !== null) ? first : null;  // first citation object
}
```
Citoid returns a JSON ARRAY of citation objects; take the FIRST. Any failure (bad JSON, not an
array, empty, first not an object) → `null`. Citoid NEVER blocks verification. Verified cases:
`[{title:'Headline'},{title:'Other'}]` → `{title:'Headline'}`; `[]` → null; `{"title":"x"}` → null
(not an array); `<html>error</html>` → null.

### 5c. Field extraction (buildCitoidHeader)

```ts
interface CitoidMetadata { publication?; published?; author?; title?; url: string; }

function buildCitoidHeader(raw, sourceUrl) {
  const publication = asString(raw.publicationTitle) ?? asString(raw.websiteTitle);
  const published   = asString(raw.date);
  const author      = formatAuthors(raw.author) ?? formatAuthors(raw.creators);
  const title       = asString(raw.title);
  if (publication===undefined && published===undefined && author===undefined && title===undefined)
    return null;   // bare url alone is not worth a header
  return { ...(publication&&{publication}), ...(published&&{published}),
           ...(author&&{author}), ...(title&&{title}), url: sourceUrl };
}
```
Field mapping (EXACT, from Zotero base-fields):
- `publication` ← `raw.publicationTitle`, falling back to `raw.websiteTitle`.
- `published`   ← `raw.date`.
- `author`      ← `raw.author`, falling back to `raw.creators` — both formatted by `formatAuthors`.
- `title`       ← `raw.title`.
- `url`         ← always the passed `sourceUrl` (echoed).

`asString(v)`: returns `v` iff it's a string with non-empty trim, else `undefined`.

`formatAuthors(value)`: value must be an array; each entry is either an array of name parts
(`['Jane','Doe']` → join with a SPACE → `"Jane Doe"`, filtering empty parts) or a plain string;
drop empties; join all names with `", "`. Returns `undefined` if not an array or no names.
EXAMPLES (verified): `author:[['Jane','Doe'],['John','Smith']]` → `"Jane Doe, John Smith"`;
`creators:[['Ada','Lovelace']]` → `"Ada Lovelace"`.

Returns `null` if NO meaningful field present (e.g. `{}` → null; `{accessDate:'2026-06-02'}` → null,
since accessDate is not one of the four). Verified full case:
`{publicationTitle:'The Guardian', date:'2020-01-01', author:[['Jane','Doe'],['John','Smith']],
 title:'Headline'}` → `{publication:'The Guardian', published:'2020-01-01',
 author:'Jane Doe, John Smith', title:'Headline', url}`.

### 5d. The do-not-quote prompt block (buildVerifyPrompt / metadataSection)

Source: `packages/core/src/citation/prompts.ts`. The metadata is rendered as a CONTEXT-ONLY section.
EXACT header text (port verbatim if SP42 reuses the prompt):

```
SOURCE METADATA (bibliographic context only — DO NOT quote from here; your supporting quote MUST come verbatim from the SOURCE text below):
- publication: <publication>
- published: <published>
- author: <author>
- title: <title>

```
Only present fields get a `- key: value` line, in the fixed order publication, published, author,
title. If NO field present, `metadataSection` returns `''` so the prompt is BYTE-IDENTICAL to the
no-metadata form.

The full user message template (for reference):
```
CLAIM:
<claim>

<metadataSection-or-empty>SOURCE (<sourceUrl>):
"""
<sourceText>
"""

Respond with the JSON object described in the instructions.
```
System prompt = the two-step "use ONLY the provided source" verifier (full text in `prompts.ts`
`SYSTEM`; verdict scale SUPPORTED / PARTIAL / NOT_SUPPORTED / SOURCE_UNAVAILABLE; verbatim quote
required for SUPPORTED/PARTIAL; NO numeric confidence). This is the SP42 ADR-0007 verdict semantics.

### 5e. CONFIRMED: metadata is NEVER content-hashed / NEVER grounded

Hard invariant, verified at three layers:
1. `fetch-source.ts`: `contentHash = await ctx.objects.put(text)` where `text` is the extracted
   BODY ONLY. `metadata` is attached to the returned `data` object AFTER hashing and is NEVER passed
   to `objects.put`. The comment is explicit: "A SIDECAR: NEVER part of `text`/`contentHash` ... so it
   cannot contaminate the anti-hallucination gate."
2. `prompts.ts`: metadata is rendered into the PROMPT but `objects.put` / `locateQuoteInSource` see
   only `sourceText`. The unsafe `prependMetadataHeader` (which concatenated metadata INTO the
   grounded bytes) was DELETED (#12).
3. Test `fetch-source.test.ts` (#12 sidecar test): asserts `result.data.metadata` is returned AND
   `result.data.text === bodyText` (no metadata mixed in) AND
   `result.data.contentHash === sha256Hex(bodyText)`. Also: without `includeMetadata`, NO Citoid
   request is made (an unmocked stub would throw if it were); a Citoid failure is swallowed
   (best-effort), the body still returns.

Rust port rule: the snapshot/grounding store hashes the extracted source body ONLY. Citoid metadata
rides as a separate field on the verdict/finding record, never entering the content-addressed bytes.

### 5f. fetch_source orchestration (the order of operations to replicate)

`packages/tools/src/citation/fetch-source.ts` `fetchSourceTool.execute`:
1. `url = rewriteWaybackUrl(p.url)` (id_ rewrite; idempotent for non-Wayback).
2. GET `url` over the HttpClient. A thrown network error → return `{ok:false, status:0, text:'',
   contentHash: hash('')}` (NOT a crash).
3. `ok = 200 <= status < 400`.
4. If ok: decide HTML-ness via `looksLikeHtml(contentType, body)` (§5g). If HTML →
   `htmlExtractor.extract(body, url)` when injected, else `htmlToText(body)`. Else (non-HTML) →
   use the raw body as text.
5. `text = recoverWaybackBody(text)` (§4) — strip any surviving banner.
6. `contentHash = objects.put(text)` (body text only).
7. If `p.includeMetadata`: best-effort Citoid on `p.url` (original, not rewritten) → metadata sidecar
   (swallow ALL errors). Else no metadata.
8. Return `{url:p.url, text, contentHash, status, ok, metadata?}` + provenance
   `{sources:[{url:p.url, fetchedAt:now, contentHash}], createdAt:now, toolName:'fetch_source'}`.

### 5g. looksLikeHtml gate (when to run extraction)

```ts
function looksLikeHtml(ct, body) {
  if (ct.includes('html') || ct.includes('xml')) return true;     // trust html/xml content-type
  if (ct !== '') return false;                                    // any other declared type → not HTML
  // no content-type: require an actual markup signature
  return /^\s*<(?:!doctype|html|head|body|\?xml)/i.test(body) ||
         /<\/(?:p|div|span|a|body|html|table|tr|td|li|ul|ol|h[1-6]|article|section)>/i.test(body);
}
```
`contentType` is read case-insensitively and lowercased before the test. Verified: `text/plain`
body `'if a < b and c > d then ok'` survives untouched; no-content-type `'use the <ref> tag ...'`
survives (no closing tag, no doctype); no-content-type `'<!doctype html><p>Hello <b>world</b></p>'`
IS extracted → `'Hello world'`.

---

## 6. Wikimedia User-Agent + read-etiquette (the HttpClient edge)

Source: `packages/edges/src/http-fetch.ts` (`FetchHttpClient`, `buildUserAgent`); composition root
`packages/server/src/main.ts`.

### 6a. User-Agent string (REQUIRED — Wikimedia policy)

```ts
function buildUserAgent(p) { return `${p.tool}/${p.version} (${p.url}; ${p.contact})`; }
```
Composition root value (the actual UA wikiharness sends):
```
WikiHarness/0.0.0 (https://github.com/tieguy/wikiharness; luis@lu.is)
```
For the SP42 port, substitute SP42's tool name/version/repo/contact but KEEP the exact
`tool/version (url; contact)` shape — Wikimedia policy requires a descriptive UA with a contact.
Overridable via `WIKIHARNESS_UA` env in wikiharness; SP42 should have an equivalent.

UA enforcement detail: caller-supplied headers are copied EXCEPT any `user-agent` (any case) is
dropped, then the policy UA is set as `User-Agent`. So the policy UA can NEVER be overridden by a
caller header. Port this: strip any incoming user-agent, force-set the policy one.

### 6b. Read-only by construction

`READ_METHODS = {GET, HEAD}`. Any other method THROWS before fetching
(`FetchHttpClient is read-only (GET/HEAD); refusing <METHOD> — no wiki writes via raw HTTP`). The
read HTTP edge can never write a wiki. Port: the source-fetch/citoid/article-fetch client should
refuse non-GET/HEAD.

### 6c. Backoff / retry etiquette

- `isRetryableStatus(s)`: `s === 429 || (500 <= s <= 599)`.
- `maxRetries` default **3**.
- Exponential backoff: `backoffDelayMs(attempt) = min(maxDelayMs, baseDelayMs * 2**attempt)`,
  defaults `baseDelayMs=500`, `maxDelayMs=30000`.
- On a retryable RESPONSE: prefer the server's `Retry-After` header
  (`parseRetryAfterMs`: integer seconds → `*1000`; else HTTP-date → `Date.parse - now`, clamped >=0;
  unparseable → null → fall back to exponential).
- On a thrown fetch error (network): retry with exponential backoff (no Retry-After available).
- After `maxRetries` exhausted: return the last response (or rethrow the last error).

### 6d. Concurrency + timeout

- `maxConcurrency` default **3** (a semaphore: acquire before fetch, release after; queued waiters).
- `timeoutMs` default **30000** via an AbortController that aborts the fetch.

### 6e. maxlag — NOT on the read path (note)

`maxlag` is a MediaWiki Action-API write/query param and is INTENTIONALLY NOT applied to these REST
reads (`/page/html`, Citoid). 429/503 + `Retry-After` backoff covers read-side etiquette. `maxlag`
belongs to the Action-API + OAuth2 WRITE path only (out of scope here). The composition root sets
`defaultParams: { maxlag: 5 }` ONLY on the mwn write bot, not the read client.

### 6f. Response header normalization

The real client lowercases response header names (`res.headers.forEach((v,k)=>{ out[k]=v })` — the
Fetch API yields lowercased names). Tool code still does case-insensitive lookups defensively. In
Rust, normalize response header keys to lowercase for stable lookups (esp. `etag`,
`content-revision-id`, `retry-after`, `content-type`).

---

## 7. HTML extraction (htmlToText fallback + Defuddle) — for the article body

The grounded bytes are the EXTRACTED TEXT of the source body. Two extractors:

### 7a. htmlToText (pure fallback) — `packages/core/src/html.ts`

1. Strip comments + declarations FIRST via regex (safe — `<!--`/`<!` don't nest):
   - `/<!--[\s\S]*?-->/g` → ` ` (terminated comments)
   - `/<!--[\s\S]*$/g` → ` ` (unterminated comment to end)
   - `/<![^>]*>/g` → ` ` (`<!DOCTYPE …>` declarations) — comments removed first so a `>` inside a
     comment can't truncate a declaration.
2. Parse with a real HTML parser; `script`/`style`/`noscript` content is DROPPED (not leaked as
   text); `pre` is kept as block text.
3. Take `structuredText` (block-aware, decodes the FULL HTML entity set — important: a partial
   entity table left `&eacute;`/`&deg;` undecoded, a false "quote not found" risk in the grounding
   gate). Then `.replace(/\s+/g,' ').trim()` (collapse all whitespace to single spaces).

Rust port: use a real HTML parser (e.g. `scraper`/`html5ever`), drop script/style/noscript text,
emit block-separated text, decode all entities, collapse whitespace. Do NOT hand-roll with regex
(the comment strip is the only safe regex step).

### 7b. DefuddleHtmlExtractor (the real edge) — `packages/edges/src/html-extractor.ts`

Main-content extraction via Defuddle (JS lib) over a linkedom DOM. Behavior to replicate with
whatever Rust readability/boilerplate-stripper is chosen:
- Run the extractor; take its `content` HTML; pass through `htmlToText`.
- **Thinness floor:** if extracted text length < `minUsefulChars` (default **200**), FALL BACK to
  `htmlToText(fullPage)` (a thin extract is worse than the whole page).
- **Max-char ceiling:** if result length > `maxChars` (default **200_000** ≈ 200 KB), head-truncate
  (`slice(0, maxChars)`) — guardrail against pathological mega-pages blowing the model-input limit
  (#10). Relevance-aware chunking is a noted follow-up, NOT implemented (head-truncate only).
- Defuddle throwing / returning nothing → treat as failed extract → fall back. Best-effort.

### 7c. Body-usability gate (STEP-1 GIGO classifier) — `packages/core/src/citation/body-classifier.ts`

Deterministic pre-model gate: classify the extracted body; if unusable, short-circuit to
`SOURCE_UNAVAILABLE` WITHOUT calling the model (so a scrape failure is never scored as a model
error). Reasons + EXACT detectors (all ReDoS-safe, bounded slices):
- `SIGNATURE_LEN=500`, `SHORT_BODY_FLOOR=300`, `CHROME_LENGTH_CAP=600`.
- `json_ld_leak`: head(500) matches `/^\s*[{[]/` AND `/"@(context|type|graph)"\s*:/`.
- `css_leak`: head(500) matches `/^[\s.#@\w-]+\{[^{}]{10,}/` AND css-glyph density
  `(count of [{};:]) / head.length > 0.05`.
- `anti_bot_challenge`: slice(1500) matches any of (case-insensitive):
  `Making sure you('|&#39;)re not a bot`, `Anubis uses a Proof-of-Work`, `Just a moment...`,
  `Verifying you are human`, `Please enable JavaScript and cookies`,
  `Checking your browser before accessing`.
- `wayback_redirect_notice`: slice(1500) matches `/Got an HTTP \d{3} response at crawl time/`.
- `wayback_chrome`: ONLY if `text.length < 600`, head(500) matches
  `/^The Wayback Machine - https?:\/\//` OR `/\d{1,9} captures\s{1,5}\d{1,2} \w{1,30} \d{4}/` OR
  `/\bCOLLECTED BY\s+Collection:/`.
- `amazon_stub`: matches `Conditions of Use(?: & Sale)? ... Privacy Notice ... © YYYY-YYYY, Amazon.com`.
- `short_body` (catch-all): `text.length < 300`.
Run over `text.trim()`; `null`/`undefined` → `short_body`. First matching pattern wins; else
`{usable:true, reason:'ok'}`.

---

## 8. PDF / Save-Page-Now — explicitly SKIP (ADR-0009 first cut)

- **PDF**: there is NO PDF extraction path in wikiharness. `fetch_source` treats non-HTML bodies as
  raw text (a PDF would fail the body-usability gate as `short_body`/garbage). "PDF page extraction
  is a later refinement" (noted in `fetch-source.ts` + CLAUDE.md). DO NOT implement PDF in the first
  Rust cut.
- **Save-Page-Now (SPN)**: NOT present. MVP-1 is **read-only Wayback** — it FINDS existing snapshots
  (via `isArchiveUrl` / `rewriteWaybackUrl`) but NEVER triggers archive creation. `archive_url`
  creation is post-MVP-1. The ONLY external write in MVP-1 is the human-confirmed wiki edit (out of
  scope for these fetch edges). DO NOT call any SPN endpoint.
- Also NOT implemented (note as future): sfn/Harvard short-cite following, page-number extraction,
  template-param `archive_url` preference, relevance-aware chunking.

---

## 9. Port checklist (the minimal set to START)

Pure functions (port + unit-test first, no I/O):
- `build_article_html_url(wiki, title, revision?) -> String` (§1a) + `assert_wiki_code` regex (§1b).
- `parse_revision_from_etag(etag) -> Option<u64>` (§1d): `"(\d{2,})[/"]`.
- `rewrite_wayback_url(url) -> String` (§3): `^(https?://web\.archive\.org/web/\d{14})/` → `$1id_/`.
- `recover_wayback_body(text) -> String` (§4): preamble regex + 200/500 gates.
- `is_archive_url(url) -> bool` + `resolve_citation_url(urls) -> Option<ResolvedUrl>` (§2).
- `build_citoid_request(url) -> HttpRequest`, `parse_citoid_response(body) -> Option<CitoidRaw>`,
  `build_citoid_header(raw, url) -> Option<CitoidMetadata>` (§5a–c) incl. `format_authors`.
- `html_to_text(html) -> String` (§7a), `looks_like_html(ct, body) -> bool` (§5g),
  `classify_body_usability(text) -> BodyUsability` (§7c).
- `parse_article(html, meta) -> ParsedArticle` (§1f) — needs an HTML parser; the trickiest port.

Impure edges:
- `HttpClient` with the Wikimedia UA `tool/version (url; contact)`, GET/HEAD-only, 429/5xx +
  Retry-After backoff (max 3, base 500ms, cap 30s), concurrency 3, timeout 30s, lowercased response
  headers (§6).
- `ObjectStore` = sha256-hex content addressing of EXTRACTED TEXT ONLY (never metadata) (§0/§5e).
- `HtmlExtractor` (readability) with 200-char thinness floor + 200KB head-truncate fallback (§7b).

Invariant to enforce in the snapshot/grounding store: hash the source BODY text only; Citoid
metadata is a sidecar field on the record, never hashed, never quotable.
