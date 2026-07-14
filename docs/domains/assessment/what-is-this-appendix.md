# What is this appendix?

## What the appendix is

The SP42 evidence appendix is a tool-generated report about whether an article's inline citations support its text. It is produced by [SP42](https://github.com/schiste/SP42), an open-source citation-verification tool. The tool checks each citation on an article revision and produces this appendix, which reviewers can paste onto the article's talk page. **The tool never edits the wiki itself** — it only provides evidence for reviewers to act on.

## What it is NOT

This appendix **is not** a verdict on the article's overall quality or readiness. It **does not determine** whether the article passes Good Article review. The appendix checks **evidence for criterion 2b only** (inline citations support the text); all other criteria and sub-criteria remain unassessed. A few missing or unconfirmed citations does not make an article ineligible — the decision rests with the reviewer.

## The vocabulary

Each finding in the appendix is labeled with one of these categories:

- **Claim–source disagreements** — the panel found the claim and source to be in conflict or unsupported
- **Supported via archive copy (citation update suggested)** — the citation was supported but the live URL is dead; an archive copy is available for the reviewer to verify and potentially link
- **Dead links (no archive copy found)** — the source URL could not be fetched and no archive copy exists
- **Sources the tool could not read (tool limitation — the citations may be fine)** — the tool fetched the page but could not extract readable text (a PDF, paywall, or interactive viewer); the citation may be correct despite the tool's inability to verify it
- **Unconfirmed supports (judged supported, quote not re-located)** — the panel judged the claim supported but could not pinpoint the exact quote in the source
- **Supported spot-checks** — the claim and source agree, and the supporting quote was located
- **Not machine-verified (book and offline sources)** — cites a book or offline source the tool does not verify
- **cites a book whose identifier matched no catalog record the tool could use** — the tool found an ISBN or similar identifier and looked it up, but no usable catalog record came back; the citation itself may be perfectly fine.
- **cites a book whose catalog lookup did not complete — the tool could not reach the catalog; nothing is implied about the book** — the lookup errored before an answer; nothing is implied about the book or the citation.
- **Books consulted** — every book the tool resolved against its catalog, with the scan state that determined whether it could be searched: *scanned (exact edition)*, *scanned (similar edition only)* — a different edition exists but the tool never verifies against one, *not scanned in the tool's catalog*, or *scan availability unknown (the lookup did not complete)*.
- **Refs the tool could not process** — parsing or extraction errors prevented assessment of these refs

**Panel split (low-confidence)** — if you see this note on a finding, the review panel split on the reading; treat this verdict as less certain than majority findings.

## How to read a line

Each line in the appendix contains:

1. A **ref label** (e.g., `ref "author_name"` or `ref #5`)
2. A **verdict** (disagreement, supported, dead link, etc.)
3. The **article claim** (truncated if long)
4. A **supporting quote or absence thereof** — if the panel found a quote, it is shown; if not found, that is noted
5. The **source link** and any archive copy reference

You can click the source link to verify the panel's reading yourself.

## Where to report problems

If you spot an error in the appendix — a mistaken verdict, a parsing error, or an unreachable source that's actually live — please [open an issue](https://github.com/schiste/SP42/issues) on the SP42 repository. Include the article revision, the ref label, and what you found.
