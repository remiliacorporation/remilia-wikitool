---
name: wikitool-knowledge-interview
description: Interview a human contributor to define, research, and shape an evidence-bound MediaWiki article before real agent or collaborative drafting. Use for new articles, substantial revisions, niche history, sensitive framing, and source gaps.
---

# Wikitool knowledge interview

Read `writing_context/interview_playbook.md`. Verify commands with `wikitool --help`,
`wikitool <command> --help`, and
`docs/wikitool/reference.md`.

Use `knowledge article-start --intent new|expand|audit|refresh --view brief` as the initial scout.
Read supplied materials first. Ask questions that can change the article object, scope, chronology,
terminology, claim-source map, emphasis, exclusions, or risk. Separate inspected support,
target-wiki testimony, source leads, interpretation, editor intent, privacy exclusions, and
do-not-assert material.

The human does not need to pre-write the article. The agent may draft genuine encyclopedic prose
from a validated brief and inspected sources. Do not convert editor intent into fact or use model memory
to fill gaps, and do not copy a neighboring page's framing. A brief is not independent evidence or
publication acceptance.

Do not force adjacent subjects into relationship-to-Remilia framing. Determine why the subject is
in scope, then define it on its own terms. Include a Remilia, Milady, community, or Charlotte Fang
relationship only when it is important, supported, and proportionate.

Use `wikitool knowledge interview init|validate|show|audit|open-item` for the deterministic ledger.
Save reusable notes under `.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. Record
inaccessible, rejected, missing, contradicted, negative, and do-not-assert material as open items.

Pass a validated brief to `knowledge article-start --brief-path` and `review --brief-path`. After
drafting, return to the human for concrete corrections to accuracy, emphasis, terminology, omitted
context, and sensitive material. Before promotion or push, a named human must read and accept the
exact prose with `article accept`; the agent must never self-attest.
