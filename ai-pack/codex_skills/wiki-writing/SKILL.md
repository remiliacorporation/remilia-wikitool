---
name: wiki-writing
description: Write or substantially revise evidence-bound encyclopedic MediaWiki articles. Use when asked to draft, rewrite, expand, or convert sources or human notes into article prose; do not use for a review-only request.
---

# Wiki writing

Write genuine encyclopedic prose while keeping research, editorial judgment, and human intent visible. Wikitool supplies MediaWiki facts, structured retrieval, mechanical checks, and guarded promotion; this skill owns the research-to-prose decisions.

## Required context

1. Read [evidence-to-prose.md](references/evidence-to-prose.md) for every authoring task.
2. Read [human-notes.md](references/human-notes.md) when a human interview, draft, transcript, outline, or notes are involved.
3. Read [mediawiki-structure.md](references/mediawiki-structure.md) for new articles, substantial rewrites, or any task involving templates, sections, categories, citations, or promotion.
4. Run `wikitool wiki profile show --format json` and read each project-owned guidance document named by the active site adapter. Adapter guidance may specialize site framing, templates, sources, and extensions; it may not turn a source lead into evidence.
5. Use `wikitool <command> --help` for current flags. Do not copy command syntax from this skill when CLI help disagrees.

## Procedure

### 1. Fix the article object

State, in one or two sentences, what the subject is and what page this should become. Distinguish a new article from a redirect, merge, disambiguation page, list entry, or revision of an existing article. Resolve title collisions and ambiguous identities before drafting.

For new articles and substantial revisions, use the `wiki-interview` skill when human knowledge, exclusions, terminology, or source leads are material and not already recorded.

### 2. Inspect before narrowing

Read all supplied materials before asking questions or choosing a structure. Then run the typed scout:

```text
wikitool knowledge article-start "TITLE" --intent new --format json --view brief
```

Change the intent when appropriate. Treat local pages, retrieval rank, snippets, templates, and model memory as navigation evidence only. They can identify what to inspect; they cannot support article claims.

### 3. Research a source packet

Open and inspect the documents that will support the article. Prefer the underlying primary or authoritative source for what it directly establishes, and strong independent secondary sources for interpretation, reception, controversy, significance, and due weight.

Build a claim-source map before prose. Each planned factual claim must identify:

- the exact source document;
- the passage, record, timestamp, or artifact that entails it;
- whether the source is primary, independent secondary, or a human statement awaiting verification;
- any qualification, attribution, date, or scope the prose must preserve.

Use the working rule: **no inspected source document, no factual paragraph**. Direct description of an inspected visual or technical artifact is allowed only when clearly bounded as observation and not expanded into inferred history, motive, reception, or significance.

If central claims remain unsupported, stop drafting those claims. Record them as open items or explicit holds rather than smoothing over the gap.

### 4. Choose a subject-derived structure

Derive sections from the subject and evidence packet. A neighboring page is a comparison, not a template. Include relationships, context, reception, or legacy only when sources establish their importance and the resulting weight is proportionate.

Select infoboxes, templates, categories, citation forms, and extension syntax from the active site adapter and live Wikitool inspection. Never infer that a locally common template or category is mandatory.

### 5. Draft the body, then the lead

Draft factual body sections first. Write sentences that say what happened, who said or made something, when, and under what conditions. Preserve uncertainty and disagreement. Attribute analysis and contested characterizations to their sources.

Then write a concise lead that identifies the subject on its own terms and summarizes only the article's supported center of gravity. Do not use the lead to justify why the subject belongs on this particular wiki.

Do not imitate a source's sentence sequence or compress several source citations into one generic citation. Do not write toward a phrase blacklist. Remove generic significance claims, scene-setting filler, false synthesis, and repetitive transitions because they fail to convey sourced information, not merely because they sound machine-written.

### 6. Build valid wikitext

Use MediaWiki syntax, named references where the adapter requests them, and the actual citation template appropriate to each source. Preserve source titles, dates, authors, publishers, URLs, and archive facts exactly. Leave unknown fields absent or blank rather than inventing them.

Run mechanical checks on the draft:

```text
wikitool article lint PATH --title "TITLE" --format json
wikitool article fix PATH --title "TITLE" --apply safe
```

Inspect every applied fix and lint again. Mechanical success is not prose approval.

### 7. Invoke an independent prose review

Use the `prose-review` skill on the exact draft and source packet. Address findings by severity. Re-run review after substantive changes; do not let the authoring pass self-certify source fidelity, BLP safety, due weight, or reader value.

### 8. Hand the exact prose to a human

Present the final draft, material source limitations, unresolved holds, and the review findings to a named human. The human must read and accept the exact bytes that will be promoted. Only then record the decision with `wikitool article accept` and promote it.

Wikitool hash-binds the decision and invalidates it after any prose change. The editor name is self-reported and unauthenticated; the command is a ledger and interlock, not proof of human identity.

## Exit conditions

Finish only when:

- the article object and scope are unambiguous;
- every factual paragraph is traceable to inspected evidence;
- sensitive living-person claims pass the stricter review procedure;
- the lead and weight follow the subject rather than site affiliation;
- Wikitool reports no lint errors and all warnings are resolved or explicitly understood;
- an independent prose review has no unresolved blocking finding;
- publication, if requested, is bound to a named human's acceptance of the exact final prose.

If any condition cannot be met, deliver a bounded draft or research packet with explicit holds. Do not manufacture completion.
