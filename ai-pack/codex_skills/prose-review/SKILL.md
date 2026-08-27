---
name: prose-review
description: Adversarially review encyclopedic wiki prose for source fidelity, due weight, reader value, BLP risk, structure, and gratuitous framing. Use for review-only, pre-publication, audit, or remediation requests; keep it independent from authoring.
---

# Prose review

Review encyclopedic prose as critically as a code reviewer reviews a change: reconstruct what it claims, test those claims against the actual inputs, identify reader-facing failures, and report actionable findings before any summary.

## Required context

1. Read [source-fidelity.md](references/source-fidelity.md) for every review.
2. Read [reader-value.md](references/reader-value.md) for every review.
3. Read [blp-sensitive.md](references/blp-sensitive.md) whenever a living person, health, crime, drugs, sexuality, identity, harassment, finances, allegations, or reputational claim appears or may be implied.
4. Run `wikitool wiki profile show --format json` and read the active project's supplemental guidance. Treat source-review matches as review prompts, not automatic reliability verdicts.
5. Inspect the exact article bytes and the exact source documents. If the source packet is incomplete, state the review limit and do not certify source fidelity.

## Independence

Do not silently rewrite the article while reviewing it. Keep findings separate from repair. When the same agent authored the draft, explicitly adopt an adversarial pass: discard the authoring rationale, rebuild the claim-source map from the text, and look for reasons the prose should not ship.

## Procedure

### 1. Freeze the review target

Record the title, path, revision or content hash, namespace, stated article object, and available source packet. Note whether the review is complete, sampled, or blocked by inaccessible material. A later prose change invalidates the review result.

### 2. Read once as a reader

Read without opening sources first. Answer:

- What does this article say the subject is?
- What new understanding does each section give the reader?
- Does the lead reflect the supported center of gravity?
- Where does attention drift to an institution, scene, author, or local wiki relationship that is not the subject?
- Would a reader choose to continue, and where would they stop?

Record confusion, boredom, repetition, inflated significance, missing context, and abrupt compression. Do not reduce this pass to forbidden phrases.

### 3. Reconstruct every material claim

Map factual statements, quotations, implications, infobox fields, captions, section headings, and category memberships to sources. Open each citation. Confirm that the URL is the cited work and that the source entails the nearby claim with the same actors, dates, quantities, confidence, and causal direction.

Flag citation laundering, tertiary compression, citation bundles, source-title mismatches, failed links, overbroad reuse, and claims supported only by snippets, memory, retrieval, or neighboring pages.

### 4. Test weight and framing

Compare prose weight with source weight. Ask whether independent sources make the claimed relationship, reception, controversy, or significance central enough to include and prominent enough for its placement. A true fact can still be gratuitous, disproportionate, or misleading.

Check whether the subject leads on its own terms. A host-wiki connection belongs only when important, sourced, and proportionate. Local relevance is not an encyclopedic definition.

### 5. Apply the sensitive-claim pass

When the BLP/sensitive reference is triggered, inspect every such claim individually. Require direct, high-quality support and careful attribution. Block publication when a material sensitive claim lacks adequate evidence, even if the rest of the article is sound.

### 6. Inspect prose and structure

Review paragraph purpose, information density, transitions, chronology, terminology, headings, lead-body agreement, repetition, quotations, and source voice. Identify artificial prose by its failure mode: vague subject, generic consequence, unsupported synthesis, repetitive cadence, ornamental abstraction, or a paragraph that could fit many topics.

Do not report “sounds AI-generated” as a standalone finding. Point to the exact information or reader problem and show how to repair it.

### 7. Run mechanical checks last

Use Wikitool lint, duplicate-reference inspection, validation, and live link/redirect verification as appropriate. Mechanical findings supplement the editorial review; they cannot override it.

## Findings format

Report findings first, ordered by severity:

- **P0 — stop:** publication would create acute legal, privacy, safety, fabrication, or destructive sync risk.
- **P1 — block:** material claim-source failure, BLP/sensitive defect, plagiarism or close paraphrase, wrong article object, or fundamentally misleading framing.
- **P2 — revise:** significant due-weight, readability, structure, attribution, context, or evidence-coverage defect.
- **P3 — polish:** bounded clarity, style, wikitext, or consistency issue that does not distort the article.

Each finding must include the smallest useful location, the exact problem, evidence for the diagnosis, reader or publication impact, and a concrete repair direction. Distinguish verified defects from inferences and review limitations.

After findings, provide:

1. **Reader verdict:** directly answer “Would someone want to read this article?” and why.
2. **Source verdict:** complete, incomplete, or not assessable, with the principal evidence gap.
3. **Disposition:** `accept`, `revise`, or `block`.
4. **Residual risk:** what still requires a human judgment or inaccessible source.

If there are no findings, say so explicitly and still state the evidence scope. Do not inflate a review with cosmetic comments.

## Exit conditions

A review is complete only when the target is fixed, the cited source packet has been inspected or its absence disclosed, conditional sensitive review has run, mechanical checks are separated from editorial judgment, and the disposition follows from enumerated findings. Review completion is not human acceptance and must not create an acceptance ledger entry.
