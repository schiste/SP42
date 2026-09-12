"""Parse WAFER records into a normalized intermediate for corpus building.

Schema verified against a real `wafer-fail-dev` record (2026-06-23). A record:

    id     : str (uuid)
    input  : "{title} [SEP] Section::::{section}. [SEP] {body ... [CIT] ...}"
             — title + section + the surrounding text, with a [CIT] marker at the
             citation position. Sentences are newline-separated.
    output : [ { provenance: [ {chunk_id, url}, ... ],   # cited source, chunked
                 answer: "{Failed verification}" | "<url>" } ]   # url for positives
    meta   : { wikipedia_id, wikipedia_title, wikipedia_section,
               cit_paragraph_id, cit_offset, sentences:[...] }   # context window

Notes that shaped this parser:
- Sources are at `output[].provenance[].url`, NOT `output[].url`. A provenance
  is the cited document split into chunks (often many, same url).
- There is **no inline source text** in fail-dev — only chunk_id + url. The text
  lives in the separate Sphere chunk corpus. So `snapshot_text` is None here; the
  both-sources (live + snapshot) design needs either the positive splits (TBD) or
  the chunk corpus to supply oracle text.
- `featured` is absent from fail-dev (present only in the positive test split).
- Precise claim-sentence selection is best-effort; the WAFER context window
  (`meta.sentences`) is carried verbatim for the downstream extraction work.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from typing import Iterator, Optional

# WAFER joins categories with "," while category names contain ", " internally
# (e.g. "Musicians from Nashville, Tennessee"). Split on a comma NOT followed by
# whitespace to separate categories without breaking those names.
_CATEGORY_SPLIT = re.compile(r",(?!\s)")

CIT_MARKER = "[CIT]"
SEP = " [SEP] "
_SECTION_PREFIX = "Section::::"
_FAILED_VERIFICATION = "{Failed verification}"

# Splits whose existing citations are deliberately-identified failures.
FAIL_SPLITS = frozenset({"fail-dev", "fail-test"})


@dataclass(frozen=True)
class WaferSource:
    url: str
    answer: Optional[str]  # "{Failed verification}" or, for positives, a url
    chunk_ids: tuple[str, ...] = ()
    snapshot_text: Optional[str] = None  # not inline in fail-dev (Sphere corpus)

    @property
    def failed_verification(self) -> bool:
        return self.answer == _FAILED_VERIFICATION


@dataclass(frozen=True)
class WaferCase:
    id: str
    title: str
    section: Optional[str]
    claim_text: str  # best-effort: sentence preceding [CIT]
    context_sentences: list[str]  # meta.sentences, the WAFER context window
    body: str  # raw text segment of `input` (for precise downstream extraction)
    cit_offset: Optional[int]
    featured: bool
    categories: list[str]  # Wikipedia categories (dev only; topic/bias signal)
    wikipedia_url: Optional[str]
    sources: list[WaferSource]
    split: Optional[str] = None
    raw_meta: dict = field(default_factory=dict, repr=False)

    @property
    def is_fail_split(self) -> bool:
        return self.split in FAIL_SPLITS


def clean_section(value: Optional[str]) -> Optional[str]:
    """'Section::::Biography.' -> 'Biography'."""
    if not value:
        return None
    s = value.strip()
    if s.startswith(_SECTION_PREFIX):
        s = s[len(_SECTION_PREFIX):]
    return s.rstrip(".") or None


def split_input(input_str: str) -> tuple[str, str]:
    """Return (title, body). Body is the text segment carrying [CIT]."""
    parts = input_str.split(SEP)
    if len(parts) >= 3:
        return parts[0].strip(), SEP.join(parts[2:])
    if len(parts) == 2:
        return parts[0].strip(), parts[1]
    return "", input_str


_SENTENCE_MARKERS = ("Section::::", "BULLET::::")


def select_claim_and_context(sentences: list[str]) -> tuple[str, list[str]]:
    """Claim = last content sentence of meta.sentences (the one [CIT] follows);
    context = the preceding content sentences. Structural markers are dropped.

    Verified against real dev/fail-dev: the citation is placed immediately after
    its claim sentence, and that sentence is the last entry of meta.sentences.
    """
    content = [
        s.strip()
        for s in sentences
        if s and s.strip() and not s.strip().startswith(_SENTENCE_MARKERS)
    ]
    if not content:
        return "", []
    return content[-1], content[:-1]


def claim_before_cit(body: str, fallback: str = "") -> str:
    """Fallback claim extractor: last line of the text preceding [CIT]."""
    pre = body.split(CIT_MARKER)[0] if CIT_MARKER in body else body
    pieces = [p.strip() for p in pre.replace("\r", "").split("\n") if p.strip()]
    if pieces:
        return pieces[-1]
    return fallback.strip()


def parse_categories(value) -> list[str]:
    """WAFER `categories` is a comma-joined string; split into individual names."""
    if not value:
        return []
    if isinstance(value, list):
        return [str(x).strip() for x in value if str(x).strip()]
    return [c.strip() for c in _CATEGORY_SPLIT.split(str(value)) if c.strip()]


def _coerce_sentences(value) -> list[str]:
    if not value:
        return []
    if isinstance(value, str):
        return [value.strip()]
    return [str(s).strip() for s in value]


def normalize_url(url: str) -> str:
    """WAFER `answer` urls are sometimes scheme-less (e.g. 'bbc.co.uk/x')."""
    if url and "://" not in url:
        return "https://" + url
    return url


def parse_source(obj: dict) -> Optional[WaferSource]:
    """Resolve the cited source URL.

    Positives: `answer` is the *gold* cited url (provenance is a candidate pool,
    ~14 urls/record). Failures: `answer` is "{Failed verification}" and the single
    provenance url *is* the failed source. So answer wins when it is a url.
    """
    answer = obj.get("answer")
    prov = obj.get("provenance") or []
    prov_urls = [p.get("url") for p in prov if isinstance(p, dict) and p.get("url")]
    if answer and answer != _FAILED_VERIFICATION:
        url = normalize_url(answer)
    elif prov_urls:
        url = prov_urls[0]
    else:
        return None
    # chunk ids for *this* url (for later Sphere snapshot lookup).
    chunk_ids = tuple(
        p.get("chunk_id")
        for p in prov
        if isinstance(p, dict)
        and p.get("chunk_id")
        and normalize_url(p.get("url", "")) == url
    )
    return WaferSource(url=url, answer=answer, chunk_ids=chunk_ids)


def parse_record(obj: dict, split: Optional[str] = None) -> WaferCase:
    meta = obj.get("meta") or {}
    title, body = split_input(obj.get("input", ""))
    sentences = _coerce_sentences(meta.get("sentences"))
    claim, context = select_claim_and_context(sentences)
    if not claim:  # records without a sentence window: fall back to body parse
        claim, context = claim_before_cit(body), []
    sources = [s for s in (parse_source(o) for o in (obj.get("output") or [])) if s]
    return WaferCase(
        id=str(obj.get("id", "")),
        title=title or str(meta.get("wikipedia_title", "")),
        section=clean_section(meta.get("wikipedia_section")),
        claim_text=claim,
        context_sentences=context,
        body=body,
        cit_offset=meta.get("cit_offset"),
        featured=bool(meta.get("featured", False)),
        categories=parse_categories(meta.get("categories")),
        wikipedia_url=meta.get("wikipedia_url"),
        sources=sources,
        split=split,
        raw_meta=meta,
    )


def iter_jsonl(path: str, split: Optional[str] = None) -> Iterator[WaferCase]:
    """Yield WaferCase per line of a WAFER JSONL file. Blank lines skipped."""
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            yield parse_record(json.loads(line), split=split)
