# Real article authoring eval

Research and draft or revise a real wiki article. This is an editorial-quality eval, not a prompt to
maximize length or satisfy a generic article shape.

## Topic

- **Name:** {{TOPIC_NAME}}
- **Intent:** {{NEW_EXPAND_AUDIT_REFRESH}}
- **Reader need:** {{READER_NEED}}
- **Known risk or framing trap:** {{RISK}}

## Method

1. Read the current article when one exists.
2. Run `knowledge article-start` with the selected intent and retain its evidence IDs, warnings,
   and local integration signals.
3. Inspect the actual sources behind load-bearing and sensitive claims. Build a claim-source map
   that records support, locators, contradictions, and omissions.
4. Define the subject on its own terms and select a factual spine. Do not copy a comparable outline
   or infer importance from profile/category frequency.
5. Draft the body, then the lead, as raw MediaWiki wikitext.
6. Run the canonical `prose-review` skill in a fresh context when possible and record what was
   removed or reframed in the adversarial reader edit.
7. Run lint and review, but do not treat mechanical success as a quality score.
8. Stop short of `article accept`, promotion, or push; a named human reviewer owns those decisions.

Use only claims supported by inspected sources or explicitly permitted target-wiki testimony.
Preserve good existing prose. If evidence supports only a stub, write a stub. If it does not support
a standalone page, recommend a redirect or merge instead of padding.

## Deliverables

- Draft: `wiki_content_testing/Main/{{TOPIC_NAME_ENCODED}}.wiki`
- Claim-source map: `wiki_content_testing/evidence/{{TOPIC_NAME_ENCODED}}.md`
- Review note: hard failures, rubric scores, and the answer to “Would someone read this?”

Outputs are evaluation artifacts and are never promoted without separate human review.
