# Citation Verification — Model & Panel Choices

Last verified: 2026-06-24

Status page for which models to run in the citation-verification panel, and why. The
numbers below are **local dev-loop measurements** on the alex-cite-checker 189-row
GT-corrected corpus (cached sources served over localhost; jointly-judged = accuracy
excluding `source_unavailable`; single runs, ±1–2pp noise). They are a guide for picking
models, **not a reproducible in-repo benchmark**.

## Default panel (recommended)

**Open-weight, non-reasoning, three models:**

- `google/gemma-4-26b-a4b-it`
- `ibm-granite/granite-4.1-8b`
- `mistralai/mistral-small-3.2-24b-instruct`

Measured ~**68% jointly-judged** — which **ties a frontier-led panel** (sonnet+gemma+granite,
68.8%). A frontier model in the panel buys essentially nothing here, so the default stays
all-open-weight. `deepseek/deepseek-chat-v3-0324` is an equally-good non-reasoning swap-in.

Frontier models (Claude Sonnet, GPT-4o) are for **ceiling / oracle measurement only** — keep
them out of the default panel.

## Per-model notes (this corpus)

| model | jointly-judged (treat) | notes |
|---|---|---|
| gemma-4-26b-a4b | 65.1% | strongest single open-weight; engages well |
| granite-4.1-8b | 64.7% | ≈ gemma; clean JSON; weak on the `partial` class |
| qwen3-32b | 64.0% | **reasoning — unreliable, see below** |
| deepseek-v3 (0324) | 63.5% | non-reasoning; clean; good swap-in |
| mistral-small-3.2-24b | 62.3% | non-reasoning; alex's reference model |
| gpt-oss-20b | 58.6% | **reasoning — worst + breaks, see below** |
| gpt-4o-mini | ~55% | closed; ~4pp below gemma |
| **gemini-2.0-flash** | **— (dead)** | **`source_unavailable` on 176/185 via OpenRouter as of 2026-06-23 — does not engage. Do not use.** |

## No reasoning models (and why)

The verifier requests **`max_tokens: 256`** (`crates/sp42-types/src/model.rs`). Reasoning
models spend that budget *thinking* and, on the long real sources (~12K chars), never reach
the answer. Reproduced on a 12K-char source with `max_tokens` ≈ the verifier's:

| model | `finish_reason` | content | reasoning tokens |
|---|---|---|---|
| **gpt-oss-20b** | **`length`** | **empty** | **197 / 200** |
| qwen3-32b | stop | present | 0 *(reasons inconsistently)* |
| deepseek-v3 | stop | present | 0 *(non-reasoning)* |

When the response is empty/truncated, the verifier's parser fails and the validate-gate
**defaults the vote to `not_supported`** — so "the model ran out of room" silently becomes
"the source doesn't support the claim." In the panel run this skewed gpt-oss to `not_supported`
(79/185) and made it the worst model; qwen3-32b reasons inconsistently and is `not_supported`-
skewed too. Reasoning models also cost ~2× latency and dropped ~9 calls to timeouts — for **no
accuracy gain** over the non-reasoning models.

**Rule:** no reasoning models in the default panel (gpt-oss-*, qwen3-*-thinking, olmo-*-think).
If one is ever wanted, first raise `max_tokens` substantially and/or pass OpenRouter's
`reasoning: { effort: "none" }`.

This truncation→false-`not_supported` is a verification-gate bug, not just a model-choice issue
— tracked on issue #25 (mechanical failure masquerading as a `not_supported` assessment).

## Open weak spot (not a model-choice problem)

Across every model and panel, the **`partial` class is the hard one** (~41–57% vs ~65–75% for
supported / not_supported), matching #25's finding (~42–44%). The next accuracy lever is the
`source_unavailable`↔`partial`↔`not_supported` prompt boundary, not model selection.
