# Closed-evidence article eval

Write an encyclopedic article using one controlled case from `testbench/writing_pools.json`.

The case packet is the entire factual universe for this test. Do not use web search, model memory,
or plausible invention. Do not infer a date, motive, relationship, reception, influence, or
significance that the packet does not state. Omit every `do_not_assert` item. Preserve contradictions
and source scope explicitly.

## Case

- **ID:** {{CASE_ID}}
- **Topic:** {{TOPIC_NAME}}
- **Reader need:** {{READER_NEED}}
- **Evidence packet:** {{EVIDENCE_PACKET}}
- **Do not assert:** {{DO_NOT_ASSERT}}
- **Expected risk:** {{EXPECTED_RISK}}

## Method

1. Build a claim-source map from the packet.
2. Decide whether the evidence supports a standalone article and how long it should be.
3. Draft the factual body, then the lead, in raw MediaWiki wikitext.
4. Cite fixture source IDs using the supplied `.invalid` URLs exactly; never invent metadata.
5. Add headings, an infobox, appendices, links, and categories only when the packet and target
   fixture justify them. There is no required count.
6. Apply `writing_context/style_rules.md` and record the reader-value judgment.

If a required fact or source is absent, omit it or state the gap in the review note—not in fluent
replacement prose.

## Output

Save only under `wiki_content_testing/controlled/{{CASE_ID}}.wiki`. Fixture URLs and fictional facts
must never be promoted to the real wiki.
