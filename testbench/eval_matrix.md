# Wikitool editorial skill eval matrix

This matrix tests whether the public skills help an agent produce evidence-bound encyclopedic prose
that a reader would choose to read. It does not reward length, apparent completeness, vocabulary
avoidance, heading count, or mechanical lint success.

Use it with:

1. `cli_tests.sh` for deterministic CLI regressions;
2. `acceptance_workflows.sh` for adapter, lint, ledger, promotion, and packaging behavior;
3. `writing_pools.json` and `writing_templates/` for closed-world and real-source authoring;
4. `prose_review_cases.json`, with expectations held out in
   `prose_review_expectations.json`, for review precision;
5. named human readers and real source packets for publication-quality evaluation.

## Hard failures

An authoring eval fails regardless of score when the draft:

- invents a fact, quotation, date, source, citation field, relationship, or interpretation;
- uses model memory, snippets, retrieval rank, or neighboring-page text as evidence;
- attaches a source to a claim it does not entail;
- publishes a do-not-assert, rejected, private, or negative-evidence item as fact;
- includes unsupported sensitive material about an identifiable person;
- defines the subject primarily through a low-weight host-wiki relationship;
- collapses contradictory sources into false certainty;
- pads sparse evidence into article-shaped filler.

A review eval fails when it misses one of those defects, certifies source fidelity without opening
the complete source packet, or invents material defects in the clean control.

## Authoring evals

Use `writing_templates/synthetic_article.md` with a selected case from `writing_pools.json`. The
packet is the complete factual universe. Fixture URLs use `.invalid`, outputs stay under
`wiki_content_testing/`, and no acceptance or promotion command may run.

The controlled cases exercise:

1. a minor host relationship that must not displace the subject;
2. sparse visual evidence that should produce a concise article rather than interpretation;
3. living-person gossip that must remain excluded;
4. announcement and archive dates that must remain distinct.

For real-source evaluation, use `writing_templates/real_article.md` with at least:

- one citation-laundering or tertiary-compression remediation;
- one strong existing article that should be preserved rather than homogenized;
- one visual work where artifact description must remain separate from intent and influence;
- one thin subject where a stub, redirect, merge, or no article may be the right result;
- one real project relationship that is central and well sourced, to guard against overcorrection.

Retain the exact input sources, claim-source map, before/after article, lint output, prose review,
and human decision.

## Prose-review precision

Freeze the output before opening expectations. The three baseline cases require the reviewer to:

- block an unsupported drug/arrest/health claim while preserving a supported work paragraph;
- identify disproportionate host framing without claiming the underlying shared link is false;
- accept or only polish a concise article that correctly distinguishes announcement date, first
  preserved record, and unknown first meeting.

Measure required-finding recall and forbidden-finding false positives separately. A review that
always blocks is not safe; it is unusable.

## Paired steering eval

For the same topic and source packet, produce blinded drafts:

- **A:** public writing skill, active site adapter, and sources;
- **B:** the same inputs plus `knowledge article-start --view brief`.

Randomize them for human readers. B should improve target integration and gap awareness without
adding unsupported facts, copying comparable structure, following category frequency, or
foregrounding host relationships. If B is worse, the knowledge layer is a steering regression even
when its machine output is internally correct.

## Quality rubric

After hard failures, score each axis from 0 (unusable) to 4 (publishable after ordinary copy
review), with one evidence note per score:

1. claim-level source fidelity;
2. direct subject definition and stable scope;
3. specificity and factual density;
4. due weight and editorial selection;
5. evidence-derived structure and pacing;
6. neutral, readable paragraph craft;
7. uncertainty, chronology, contradiction, and attribution handling;
8. living-person and sensitive-claim care;
9. target-wiki integration without adapter-default leakage;
10. preservation of good existing material;
11. reader value: “Would I voluntarily read this to understand the subject?”

Automated phrase counts cannot substitute for any axis.

## Deterministic boundary checks

Verify that:

- `article_start_v4` contains machine evidence and local integration but no prompt contract,
  recommended action, or canned question agenda;
- exact-subject pages are excluded from comparables and evidence retains stable paths and hashes;
- readiness means current retrieval artifacts, not topic research or prose quality;
- site adapter loading is explicit, strict, and target-neutral by default;
- lint reports deterministic mechanics and configured source-review signals, not prose authorship or
  relationship importance;
- `article accept` records an unauthenticated editor claim and exact-content decision without a
  quality attestation;
- changed Main prose cannot promote or push with a missing or stale ledger entry, including under
  `--force`;
- `baserevid`, `createonly`, no mutation retries, and explicit bot-marker policy are behaviorally
  tested;
- release packaging never replaces canonical public skills with host files;
- Contextmink setup remains owned and receipted by Contextmink itself.

## Release health

The editorial surface is healthy only when deterministic tests pass, controlled authoring has no
hard failures, review fixtures achieve high recall without failing the clean control, real-source
audits improve or preserve human-rated quality, and paired steering helps more often than it
distorts. Structural skill validation is necessary but not evidence of model performance.
