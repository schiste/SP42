"""Source-type classification and extractability for citation eval cases.

WAFER carries no source-type label, so we derive one from the cited URL's host
(see docs/CITATION_EVAL_CORPUS.md, "Cohorts"). The classifier only needs to be
good enough to *stratify* the sample, not to label authoritatively: unmatched
hosts fall to OTHER, and the domain map is meant to grow from what a sample
surfaces.

Extractability is a separate, capability-keyed axis: which source types SP42 can
extract *today*. Non-extractable types (e.g. books) are tagged `queued`, never
dropped, so a future extraction capability harvests them by a re-run.
"""

from __future__ import annotations

from urllib.parse import urlsplit

# --- Source types -----------------------------------------------------------

NEWS = "news"
REFERENCE = "reference"
JOURNAL = "journal"
BOOK = "book"
GOV = "gov"
BLOG = "blog"
OTHER = "other"

SOURCE_TYPES = (NEWS, REFERENCE, JOURNAL, BOOK, GOV, BLOG, OTHER)

# --- Domain map -------------------------------------------------------------
# Ordered, first match wins. Specific HOST rules run before generic TLD-suffix
# rules so that, e.g., pubmed.ncbi.nlm.nih.gov classifies as JOURNAL rather than
# GOV. A host matches a pattern when it equals the pattern or is a subdomain of
# it (host == p or host endswith "." + p).

_HOST_RULES: list[tuple[str, str]] = [
    # Academic / journals (must precede the .gov suffix rule for NCBI)
    (JOURNAL, "doi.org"),
    (JOURNAL, "ncbi.nlm.nih.gov"),
    (JOURNAL, "jstor.org"),
    (JOURNAL, "sciencedirect.com"),
    (JOURNAL, "springer.com"),
    (JOURNAL, "nature.com"),
    (JOURNAL, "arxiv.org"),
    (JOURNAL, "ieee.org"),
    (JOURNAL, "acm.org"),
    (JOURNAL, "wiley.com"),
    (JOURNAL, "tandfonline.com"),
    (JOURNAL, "sagepub.com"),
    (JOURNAL, "plos.org"),
    (JOURNAL, "cambridge.org"),
    (JOURNAL, "oup.com"),
    (JOURNAL, "biomedcentral.com"),
    (JOURNAL, "mdpi.com"),
    (JOURNAL, "frontiersin.org"),
    (JOURNAL, "ssrn.com"),
    (JOURNAL, "semanticscholar.org"),
    # Books (queued for extraction today)
    (BOOK, "books.google.com"),
    (BOOK, "openlibrary.org"),
    (BOOK, "worldcat.org"),
    (BOOK, "gutenberg.org"),
    (BOOK, "hathitrust.org"),
    (BOOK, "archive.org"),  # archive.org/details/* book scans; wayback is web.archive.org
    # Reference works
    (REFERENCE, "wikipedia.org"),
    (REFERENCE, "wikidata.org"),
    (REFERENCE, "wiktionary.org"),
    (REFERENCE, "britannica.com"),
    (REFERENCE, "merriam-webster.com"),
    (REFERENCE, "encyclopedia.com"),
    (REFERENCE, "dictionary.com"),
    # News
    (NEWS, "nytimes.com"),
    (NEWS, "washingtonpost.com"),
    (NEWS, "theguardian.com"),
    (NEWS, "bbc.com"),
    (NEWS, "bbc.co.uk"),
    (NEWS, "reuters.com"),
    (NEWS, "apnews.com"),
    (NEWS, "cnn.com"),
    (NEWS, "npr.org"),
    (NEWS, "wsj.com"),
    (NEWS, "ft.com"),
    (NEWS, "bloomberg.com"),
    (NEWS, "forbes.com"),
    (NEWS, "time.com"),
    (NEWS, "theatlantic.com"),
    (NEWS, "newyorker.com"),
    (NEWS, "latimes.com"),
    (NEWS, "usatoday.com"),
    (NEWS, "telegraph.co.uk"),
    (NEWS, "independent.co.uk"),
    (NEWS, "aljazeera.com"),
    (NEWS, "cbsnews.com"),
    (NEWS, "nbcnews.com"),
    (NEWS, "foxnews.com"),
    (NEWS, "politico.com"),
    (NEWS, "variety.com"),
    (NEWS, "espn.com"),
    # Blogs / self-published
    (BLOG, "medium.com"),
    (BLOG, "wordpress.com"),
    (BLOG, "blogspot.com"),
    (BLOG, "substack.com"),
    (BLOG, "tumblr.com"),
]

# TLD/suffix rules, applied only if no host rule matched.
_SUFFIX_RULES: list[tuple[str, str]] = [
    (GOV, ".gov"),
    (GOV, ".mil"),
    (GOV, ".gov.uk"),
    (GOV, ".gov.au"),
    (GOV, "europa.eu"),
    (GOV, "un.org"),
    (GOV, "who.int"),
]


def host_of(url: str) -> str:
    """Lower-cased registrable host, www. stripped. Empty string if unparseable."""
    host = (urlsplit(url).hostname or "").lower()
    if host.startswith("www."):
        host = host[4:]
    return host


def _host_matches(host: str, pattern: str) -> bool:
    return host == pattern or host.endswith("." + pattern)


def classify(url: str) -> str:
    """Return the source type for a cited URL. Unmatched hosts -> OTHER."""
    host = host_of(url)
    if not host:
        return OTHER
    for source_type, pattern in _HOST_RULES:
        if _host_matches(host, pattern):
            return source_type
    for source_type, suffix in _SUFFIX_RULES:
        if host == suffix.lstrip(".") or host.endswith(suffix):
            return source_type
    return OTHER


# --- Extractability ---------------------------------------------------------
# Which source types SP42 can extract today. Keyed to capability: add a type
# here when its extractor lands, then re-run the recipe to harvest its queued
# cases (docs/CITATION_EVAL_CORPUS.md, "The repeatable extraction recipe").

EXTRACTABLE_SOURCE_TYPES: frozenset[str] = frozenset(
    {NEWS, REFERENCE, JOURNAL, GOV, BLOG, OTHER}
)

EXTRACTABLE = "extractable"
QUEUED = "queued"


def extractability(source_type: str) -> str:
    """`extractable` if SP42 can extract this type now, else `queued`."""
    return EXTRACTABLE if source_type in EXTRACTABLE_SOURCE_TYPES else QUEUED
