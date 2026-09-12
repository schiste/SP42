"""Shared eval-case schema, conformed to PRD-0007 (commit 35a5aec).

Both the WAFER recipe and the alex importer emit `EmittedCase`. The schema
follows PRD-0007's facet taxonomy:

- *inherent* facets (input provenance) are STORED — claim, context, cohorts
  like language/featured/categories;
- *computed* facets (source type, body usability, compoundness) are NOT stored
  as case truth — they are derived at analysis/build time (here: in the build
  manifest, for sampling), never baked into the case;
- *assessed* facets (the GT label) are stored WITH provenance — `label_method`,
  `label_as_of`; model-labeled methods may never feed a gate;
- every text payload carries **licensing**: claim/context → CC BY-SA (Wikipedia,
  attributed to article + revision); bounded source extracts → fair use.

Banned keys (rejected by PRD-0007's loader) are never emitted: no confidence, no
version/tranche token, no row-number ids (ids are content-hash derived).
"""

from __future__ import annotations

import hashlib
from dataclasses import asdict, dataclass, field
from typing import Optional
from urllib.parse import parse_qs, urlsplit

# Categorical outcome set (the only legal expected_outcome values).
SUPPORTED = "supported"
PARTIAL = "partial"
NOT_SUPPORTED = "not_supported"
SOURCE_UNAVAILABLE = "source_unavailable"
OUTCOMES = (SUPPORTED, PARTIAL, NOT_SUPPORTED, SOURCE_UNAVAILABLE)

# Licensing constants.
CC_BY_SA = "CC-BY-SA-4.0"
CC0 = "CC0-1.0"
FAIR_USE = "fair-use"


@dataclass(frozen=True)
class EmittedCase:
    id: str
    claim: str
    claim_context: dict  # {context_sentences, section, article_title, ...}
    source_url: str
    expected_outcome: Optional[str]  # None => awaiting a gold label
    label_method: str  # provenance of the label (assessed facet)
    label_as_of: str  # ISO date the label reflects
    cohorts: dict  # INHERENT only: language, featured, categories
    licensing: dict  # per-payload: {"claim": {...}, "source_text": {...}?}
    provenance: dict = field(default_factory=dict)
    source_text: Optional[str] = None  # bounded fair-use extract, when available


def case_id(claim: str, url: str) -> str:
    """Content-hash id, stable under reordering/insertion (never a row number)."""
    h = hashlib.sha1(f"{claim}\t{url}".encode("utf-8")).hexdigest()
    return f"cit-{h[:16]}"


def revision_from_url(url: str) -> Optional[str]:
    """Extract a MediaWiki `oldid` (the revision) from an article URL, if present."""
    if not url:
        return None
    qs = parse_qs(urlsplit(url).query)
    vals = qs.get("oldid")
    return vals[0] if vals else None


def wikipedia_licensing(
    article_title: Optional[str],
    article_url: Optional[str] = None,
    wikipedia_id: Optional[str] = None,
) -> dict:
    """CC BY-SA attribution for a Wikipedia-derived text payload (claim/context)."""
    return {
        "license": CC_BY_SA,
        "source": "Wikipedia",
        "article_title": article_title,
        "article_url": article_url,
        "wikipedia_id": wikipedia_id,
        "revision": revision_from_url(article_url),
    }


def fair_use_licensing(origin_url: str) -> dict:
    """Fair-use label for a bounded extract from a cited third-party website."""
    return {
        "license": FAIR_USE,
        "basis": "bounded extraction, limited to the excerpt needed for verification",
        "origin_url": origin_url,
    }


def case_to_dict(c: EmittedCase) -> dict:
    """Serialize, dropping a null source_text so WAFER cases stay lean."""
    d = asdict(c)
    if d.get("source_text") is None:
        d.pop("source_text", None)
    return d
