# Knowledge interview playbook

Use an interview to improve a new article, substantial revision, or editorial repair. The human can
contribute intent, firsthand knowledge, artifacts, source leads, terminology, exclusions, or prose.
The human does not need to pre-write the article. An agent may draft real encyclopedic prose after
the interview when the evidence is adequate.

An interview brief is a durable editorial ledger. It is not automatically independent evidence,
article prose, proof of truth, or human acceptance of a later draft.

## What the interview should resolve

Ask only questions whose answers can change one or more of these:

- the article object and title;
- why a standalone page helps a reader;
- scope, chronology, entities, and terminology;
- the claim-source map;
- emphasis and omissions;
- privacy, living-person, controversy, or attribution risk;
- whether a primary record needs to be located or created;
- the line between fact, creator interpretation, and editor perspective.

The interview should not be a fixed questionnaire or a ritual used to bless a predetermined
outline.

## Flow

1. Scout with `wikitool knowledge article-start "Topic" --intent
   new|expand|audit|refresh --format json --view brief`. Treat neighboring pages and headings as
   prompts and local-fit evidence, not facts or an outline.
2. Read every supplied document, link, image, transcript, and existing page before narrowing the
   questions.
3. Invite a freeform account: what the subject is, what happened, why the record matters, canonical
   names, chronology, artifacts, source leads, common misunderstandings, uncertainty, and what must
   not be overstated.
4. Reflect the proposed article in neutral language. Ask the human to correct the object, scope, and
   missing emphasis before pursuing details.
5. Classify each material statement as one of: inspected support, target-wiki testimony, source
   lead, interpretation, editor intent, open question, privacy exclusion, or do-not-assert.
6. Ask adaptive follow-ups where the source map is thin, entities are conflated, the chronology is
   unclear, or the proposed framing depends on a relationship rather than the subject itself.
7. Run a critic pass: would the page be duplicative, padded, unduly promotional, unfair to a living
   person, or impossible to substantiate? Ask another round only if an answer can improve the
   result.
8. Stop when the requested deliverable is possible. That may be a full draft, a short sourced
   article, a partial draft with explicit gaps, a redirect recommendation, or an evidence brief.

Do not force adjacent subjects into Remilia framing. Ask how the subject enters the wiki's field of
view, but keep that answer separate from the subject's definition. A Remilia, Milady, community, or
Charlotte Fang connection belongs in the article only when it is important, supported, and given
proportionate weight.

## From interview to draft

Before drafting:

1. validate the brief;
2. inspect the sources behind its factual claims;
3. identify which human statements the target wiki can preserve as attributed testimony and which
   need a durable public record;
4. resolve or explicitly carry forward `do_not_assert` and negative-evidence items;
5. derive the article's factual spine independently of the brief's heading suggestions.

The agent may then draft the article. It must not turn editor intent into fact, smooth contradictions
away, or fill missing evidence from model memory. After drafting, ask the human about concrete
editorial choices: inaccurate emphasis, omitted context, terminology, sensitive material, and
whether the result is genuinely useful to a reader.

Human review of the exact final prose is recorded separately with `article accept`.

## Ledger

Create and validate interview artifacts with:

```bash
wikitool knowledge interview init "Topic" --intent new --format json
wikitool knowledge interview validate .wikitool/interviews/Title/20260601T172430Z.brief.md --format json
wikitool knowledge interview show .wikitool/interviews/Title/20260601T172430Z.brief.md --view brief --format json
wikitool knowledge interview open-item add .wikitool/interviews/Title/20260601T172430Z.brief.md --kind rejected-source --text "Candidate source did not support the claimed date."
wikitool knowledge interview open-item update .wikitool/interviews/Title/20260601T172430Z.brief.md --item-id OI-20260601T172430Z --status resolved
wikitool knowledge interview audit --view brief --format json
```

Save reusable briefs under `.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. Preserve
the canonical sections created by `init`: Article Object, Scope, Initial Materials, User-Framed
Summary, Interview Transcript and Context, Chronology, Entities and Relationships, Editorial
Framing, Research Plan, Interviewer Critic Notes, and Draft Plan.

Store unresolved work in the `.open_items.jsonl` sidecar. Use specific kinds such as
`missing-source`, `do-not-assert`, `rejected-source`, `inaccessible-source`, `disproven-link`,
`source-wiki-only-template`, `rejected-category`, `scope-unresolved`, `privacy-exclusion`, and
`negative-evidence`.

Pass a validated brief to `knowledge article-start --brief-path` and `review --brief-path`. A named
human must read and accept the exact final prose before promotion or push; an agent must never
self-attest.
