"""Opus first-pass citation verdicts: one INDEPENDENT API call per case (clean
context, no cross-case contamination), reusing SP42's verbatim two-step verifier
prompt. Output is a REVIEW QUEUE (opus-firstpass.json) with agreement-vs-prior —
not gold; a human promotes a proposal to a corrections.json entry (PRD-0007).

Usage:
    python3 opus_verify.py --candidates <gold-candidates.json> --out <opus-firstpass.json> \
        [--env <.env with ANTHROPIC_API_KEY>] [--model claude-opus-4-8] [--workers 6] [--limit N]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.request
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed

# Verbatim SP42 verifier system prompt (crates/sp42-citation/src/citation/prompts.rs SYSTEM).
SYSTEM = """You verify whether a cited SOURCE supports a CLAIM from a Wikipedia article.

Judge using ONLY the text of the provided source. Do NOT use outside knowledge, and do NOT assume facts that are not present in the source.

Use this two-step process for every claim.

STEP 1 — Source check:
Determine whether the source text contains usable article body content: real paragraphs, quotes, narrative passages, or factual statements. This holds true even when that content is surrounded by navigation, headers, footers, web.archive.org captures, or other page chrome.

The source is NOT usable if it contains only: a library/database catalog page (Google Books, WorldCat, a JSTOR preview), a paywall, a login wall, a 404, a cookie/consent notice, an anti-bot challenge, or bibliographic metadata with no article body.

Long sources may arrive as an excerpt — gaps between paragraphs, blank lines, text ending mid-sentence, or passages separated by "..." are NORMAL and mean "not shown here", not "failed to load". Brevity alone is not a SOURCE_UNAVAILABLE signal: if any article prose is present, evaluate it. If STEP 1 fails, return SOURCE_UNAVAILABLE and do NOT attempt STEP 2.

STEP 2 — Claim verification:
Identify what the claim asserts (specific dates, numbers, names, events, attributions), then look in the source for support, contradiction, or partial coverage.
- DATES: the source must contain the date in some form. Equivalent expressions count — "Wednesday" supports "January 7, 2026" if the article is dated that day; "7 Jan 2026" counts for "7 January 2026".
- NUMBERS, NAMES, QUOTED statements: the source must contain that specific number/name/quote, or a directly equivalent paraphrase.
- Accept paraphrasing and direct implications, but NOT speculative inferences or logical leaps.
- Distinguish definitive statements from hedged language ("it is believed", "some sources suggest"). A claim stated as fact requires source text that is also definitive.
- Names from non-Latin scripts have multiple valid romanizations; treat transliteration variant spellings of the same name ("Chekhov"/"Tchekhov") as equal, not as factual errors.

Return exactly one verdict from this graded scale:
- SUPPORTED — the source contains all of the claim's specific assertions (paraphrase OK if substance matches).
- PARTIAL — the source addresses the claim but contains only some of its assertions, OR asserts it only with hedged/uncertain language.
- NOT_SUPPORTED — the source addresses the topic but contradicts the claim, or has no evidence for its specific assertions.
- SOURCE_UNAVAILABLE — STEP 1 failed: no usable article body.

For SUPPORTED or PARTIAL you MUST quote a short, VERBATIM span copied exactly (character for character) from the source that backs the claim. Never paraphrase, reword, or invent the quote. If you cannot find such a verbatim span, the verdict is NOT_SUPPORTED.

Do NOT output any confidence score, probability, or percentage — only the categorical verdict and the verbatim quote.

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
{"verdict": "PARTIAL", "quote": "It is believed the treaty was signed in Paris"}"""

WIRE = {
    "SUPPORTED": "supported", "PARTIAL": "partial",
    "NOT_SUPPORTED": "not_supported", "SOURCE_UNAVAILABLE": "source_unavailable",
}
_OBJ = re.compile(r"\{.*\}", re.S)


def load_key(env_path: str) -> str:
    for line in open(env_path, encoding="utf-8"):
        if line.startswith("ANTHROPIC_API_KEY="):
            return line.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit(f"ANTHROPIC_API_KEY not found in {env_path}")


def metadata_section(meta: dict | None) -> str:
    """Verbatim SP42 metadata_section: context-only block, only present fields."""
    if not meta:
        return ""
    lines = [f"- {k}: {meta[k]}" for k in ("publication", "published", "author", "title") if meta.get(k)]
    if not lines:
        return ""
    return ("SOURCE METADATA (bibliographic context only — DO NOT quote from here; "
            "your supporting quote MUST come verbatim from the SOURCE text below):\n"
            + "\n".join(lines) + "\n\n")


def call_opus(key: str, model: str, claim: str, source_url: str, source_text: str,
              meta: dict | None = None) -> dict:
    user = (
        f'CLAIM:\n{claim}\n\n{metadata_section(meta)}SOURCE ({source_url}):\n"""\n{source_text}\n"""\n\n'
        "Respond with the JSON object described in the instructions."
    )
    body = json.dumps({
        "model": model,
        "max_tokens": 400,
        "system": SYSTEM,
        "messages": [{"role": "user", "content": user}],
    }).encode("utf-8")
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages", data=body,
        headers={
            "x-api-key": key,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json",
        },
    )
    with urllib.request.urlopen(req, timeout=90) as r:
        resp = json.loads(r.read())
    text = "".join(b.get("text", "") for b in resp.get("content", []))
    m = _OBJ.search(text)
    return json.loads(m.group(0)) if m else {"verdict": None, "quote": ""}


def verify_one(key: str, model: str, c: dict, citoid: dict) -> dict:
    meta = citoid.get(c["id"]) if citoid else None
    try:
        parsed = call_opus(key, model, c["claim"], c["source_url"], c["source_text"], meta)
        opus = WIRE.get(str(parsed.get("verdict", "")).upper())
        full_quote = parsed.get("quote") or ""
        located = bool(full_quote) and full_quote[:60].lower() in (c["source_text"] or "").lower()
        quote, err = full_quote[:240], None
    except Exception as e:
        opus, quote, located, err = None, "", False, str(e)[:200]
    return {
        "id": c["id"], "corpus": c["corpus"], "prior": c["prior_outcome"],
        "opus": opus, "quote": quote, "agrees": opus == c["prior_outcome"],
        "quote_located": located, "has_metadata": bool(meta),
        "fetch_status": c.get("fetch_status"), "error": err,
    }


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--candidates", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--citoid", default=None, help="citoid.json (id -> metadata)")
    ap.add_argument("--env", default="/var/home/louie/Projects/Volunteering-Consulting/alex-cite-checker/.env")
    ap.add_argument("--model", default="claude-opus-4-8")
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--limit", type=int, default=0)
    args = ap.parse_args(argv)

    key = load_key(args.env)
    cands = json.load(open(args.candidates))
    citoid = json.load(open(args.citoid)) if args.citoid else {}
    if args.limit:
        cands = cands[: args.limit]

    results, done = [], 0
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = [ex.submit(verify_one, key, args.model, c, citoid) for c in cands]
        for fut in as_completed(futs):
            results.append(fut.result())
            done += 1
            if done % 20 == 0:
                print(f"  {done}/{len(cands)}", file=sys.stderr, flush=True)

    graded = [r for r in results if r["opus"]]
    agree = [r for r in graded if r["agrees"]]
    summary = {
        "count": len(results),
        "graded": len(graded),
        "errors": sum(1 for r in results if r["error"]),
        "agreement_overall": round(len(agree) / len(graded), 3) if graded else 0,
        "agreement_by_corpus": {},
        "disagreements": len(graded) - len(agree),
        "opus_verdict_dist": dict(sorted(Counter(r["opus"] for r in graded).items())),
    }
    for corpus in sorted({r["corpus"] for r in graded}):
        g = [r for r in graded if r["corpus"] == corpus]
        a = [r for r in g if r["agrees"]]
        summary["agreement_by_corpus"][corpus] = {
            "graded": len(g), "agreement": round(len(a) / len(g), 3) if g else 0,
        }
    json.dump({"summary": summary, "results": results}, open(args.out, "w"),
              indent=1, ensure_ascii=False)
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
