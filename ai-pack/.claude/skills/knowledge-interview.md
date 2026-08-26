# /knowledge-interview - Thin wrapper

Read `writing_context/interview_playbook.md`. Verify commands with `wikitool --help`,
`wikitool <command> --help`, and
`docs/wikitool/reference.md`.

Scout with `knowledge article-start --intent new|expand|audit|refresh --view brief`, read supplied
materials, then ask questions that improve the article object, scope, chronology, terminology,
claim-source map, emphasis, exclusions, or risk. Classify inspected support, target-wiki testimony,
source leads, interpretation, editor intent, privacy exclusions, and do-not-assert material.

The human does not need to pre-write the article. You may draft real encyclopedic prose from a
validated brief and inspected sources. The brief is not independent evidence or publication
acceptance; do not fill gaps from model memory or inherit framing from neighboring pages.

Use `wikitool knowledge interview init|validate|show|audit|open-item` and save reusable notes under
`.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. Do not force Remilia, Milady,
community, or Charlotte Fang framing unless it is important, supported, and proportionate.

Pass the validated brief to `knowledge article-start --brief-path` and `review --brief-path`.
After drafting, return to the human for accuracy and emphasis corrections. A named human must read
and accept the exact final prose with `article accept`; the agent must never self-attest.
