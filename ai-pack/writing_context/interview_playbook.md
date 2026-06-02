# Knowledge Interview Playbook

Use this playbook when a user asks for article creation, substantial expansion, article refresh,
or a review where human context can materially improve the article. It is a general knowledge
elicitation workflow, not a script for one article type and not a checklist to rush through.

Interview briefs are working notes. They are not article prose, citation evidence, proof that a
claim is true, or proof that the interview is complete. Treat user assertions as leads to
corroborate with durable provenance before publishing them as facts: comparable wiki pages,
target-wiki records, hosted artifacts, first-party posts, archived primary records, or later
creator/editor-published statements. For Remilia Wiki, provenance does not have to be external or
secondary; the wiki often exists because no broader source has preserved the subcultural record.

## When To Interview

Default to an interview for:

- New articles where the user likely knows context, names, chronology, relationships, terminology,
  or subcultural importance that public search may miss.
- Substantial expansions or rewrites where the user's taste and domain model should shape the
  scope before drafting.
- Audits or refreshes where mechanical checks reveal unresolved conflicts, missing context, or
  editorial choices that cannot be decided from sources alone.

Skip or keep it minimal for:

- mechanical link checks, lint/fix passes, category cleanup, sync review, or source extraction.
- Requests with an explicit opt-out such as "no interview" or "just draft from sources".
- Tiny edits where a direct clarification question is cheaper than an intake round.

## Flow

1. Scout first. Run the normal authoring front door, usually
   `wikitool knowledge article-start "<Topic>" --intent new|expand|audit|refresh --format json --view brief`,
   and do a cursory source/wiki search before asking broad questions.
2. Read supplied materials before narrowing the interview. If the user provides documents, links,
   notes, screenshots, transcripts, or source excerpts, inspect them first when access permits.
   Start the interview by asking the user to explain what the subject is, why it matters, and what
   the supplied materials should make legible.
3. Start with a freeform dump. Invite the user to speak broadly, as if recording a monologue for a
   future editor: what the subject is, origin and chronology, names and terminology, relationships,
   aesthetics or mechanics, what makes it worth canonicalizing from the wiki's perspective, what
   outsiders miss, what is uncertain, and what the article must not overstate.
4. Reflect the shape back. Summarize the emerging scope in neutral wiki language, separating
   source-backed facts, user-provided leads, source leads from supplied materials, open questions,
   and editorial constraints. Do not collapse unprovenanced but important context into thin prose;
   turn it into follow-up questions, source requests, artifact-description decisions, or a durable
   primary-record publishing path.
5. Ask adaptive follow-ups. Focus questions on gaps that change article structure, not on trivia the
   agent can find mechanically. There is no fixed number of rounds and no hard cap. Continue as long
   as new answers materially improve the article's scope, chronology, terminology, source strategy,
   section plan, or risk profile.
6. Run an interviewer/critic loop before drafting. The same agent should briefly critique the
   emerging article plan: what would make this article thin, duplicative, unsourced, wrongly framed,
   or missing the human's actual knowledge? Use that critique to ask another round of questions when
   needed.
7. Stop only when the interview is editorially sufficient. Move to research and drafting once the
   article object, source strategy, likely sections, major claims, unresolved risks, and any
   intentionally deferred gaps are clear. A mechanically valid brief is not enough.

## Interview Stance

The agent is the interviewer, not a form-filler. It should use ordinary conversation, open-ended
prompts, reflection, and follow-up questions to elicit context broadly. Good interviews often begin
with one broad question and then branch into several rounds:

```text
Before I narrow this into article sections, tell me what this subject is in your own words. Include
why it matters, where it came from, what people misunderstand about it, what sources or artifacts I
should look at, and what you would be disappointed to see omitted.
```

When supplied materials exist, ask from them:

```text
I read the supplied source. It supports X and Y, but it does not explain Z. Is Z part of the subject,
an inferred reading, or something we should seek a publishable source for?
```

Avoid turning the interview into quality-marker compliance. The checklist exists to preserve a
handoff, not to decide that the article is good. The interviewer should prefer more context, clearer
source leads, and better article judgment over early closure.

For Remilia Wiki, do not force every adjacent subject into a "relationship to Remilia" frame. The
wiki is the online world viewed from Remilia's perspective; a visual artist, game, scene, or artifact
may belong because it is part of the field of view, adjacent canon, or subcultural record. Write the
subject as itself. Add explicit Remilia/Milady/community relationship framing only when that
relationship is real, sourceable or editor-attested, and central enough to improve the article.

## Interview Brief Ledger

Save the distilled brief when the interview produces reusable knowledge:

`.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`

Use a title-led directory so all sessions for the same article sit together, and use a UTC
timestamp so multiple sessions form a ledger. The `<Title-safe>` component should remain readable
and close to the intended article title while avoiding filesystem-hostile characters.

Use the Rust ledger commands for deterministic files and validation:

```bash
wikitool knowledge interview init "Topic" --intent new --format json
wikitool knowledge interview validate .wikitool/interviews/Title/20260601T172430Z.brief.md --format json
wikitool knowledge interview show .wikitool/interviews/Title/20260601T172430Z.brief.md --view brief --format json
wikitool knowledge interview open-item add .wikitool/interviews/Title/20260601T172430Z.brief.md --kind rejected-source --text "Candidate source did not support the claimed date."
wikitool knowledge interview open-item list .wikitool/interviews/Title/20260601T172430Z.brief.md --format json
wikitool knowledge interview audit --view brief --format json
```

Brief sections. The `init` command renders this canonical skeleton, and `validate` warns on any
that go missing, so keep these headings:

- Article Object: a neutral definition of what the subject is and what kind of page it may become.
- Scope: what is included or excluded, plus possible redirects and merge/split targets.
- Initial Materials: documents, links, transcripts, screenshots, source excerpts, or other context
  supplied at the start, with a note on how each should steer interview questions or research.
- User-Framed Summary: the user's high-level framing in neutral wiki language.
- Interview Transcript and Context: distilled freeform knowledge from the user's monologue and
  follow-up rounds, including important nuance that may not yet be publishable.
- Claim Map Summary: a short read of the interview claims; the claims themselves live in the
  `.claims.json` sidecar with provenance and status, not in the brief prose.
- Chronology: known dates, approximate periods, and open timeline gaps.
- Entities and Relationships: people, projects, groups, terms, and related wiki pages.
- Editorial Framing: recommended angle, tone risks, likely misconceptions, and terminology notes.
- Research Plan: primary-source leads, search queries, archive targets, pages to inspect, and
  blocking evidence gaps.
- Interviewer Critic Notes: the agent's critique of the emerging article plan, including thinness,
  duplication, source, framing, and missing-context risks that should trigger more questions.
- Draft Plan: likely sections, infobox/template candidates, categories to verify, claims that
  require citations, and open questions before drafting.

For machine-readable Draft Plan extraction, write `Likely sections` and `Open questions before
drafting` one item per line, or use semicolons on the label line. Do not comma-separate section
names; commas often belong inside natural headings.

Do not put planning labels such as "Editorial vantage", "Remilia Wiki context", or "Why this
belongs here" into `Likely sections` unless they name an actual sourceable article section. Use
those as open questions or framing notes. The article sections should describe the subject.

Record source leads and unresolved follow-up as structured open items in the `.open_items.jsonl`
sidecar rather than only as prose, so future sessions do not rediscover them.

Use claim IDs only for interview-introduced or high-risk claims that must be tracked through
research, drafting, and review. Do not assign IDs to ordinary prose or every sentence. A practical
format is `IK-001`, `IK-002`, and so on inside the brief.

Use structured open items for unresolved research work and negative evidence that future agents
should not rediscover from scratch. Prefer specific kinds such as `missing-source`,
`pending-corroboration`, `rejected-source`, `inaccessible-source`, `disproven-link`,
`source-wiki-only-template`, `rejected-category`, `scope-unresolved`, and `privacy-exclusion`.
Open items are not article content; they are ledger entries for follow-up, source rejection, access
failure, or editorial uncertainty.

## Drafting Boundary

Before drafting, convert the brief into a research plan and an editorial sufficiency check:

- Corroborate factual claims with durable provenance: target-wiki pages, hosted artifacts,
  first-party sources, archived primary records, or creator/editor-published statements.
- Preserve the user's thematic framing only when it survives neutral article language.
- Attribute disputed or source-specific claims rather than presenting them as settled facts.
- Omit or defer claims that remain uncited, unverifiable, or too thin for the target wiki, but do not
  let omission silently produce a poor article. If the omitted material is central, ask more
  questions, request durable provenance, propose a creator/editor-statement lane, or recommend
  redirect/merge until the article has enough sourceable shape.
- Run `wikitool knowledge interview validate PATH --format json` and resolve invalid metadata,
  duplicate claim IDs, missing sidecars, unsupported claim statuses, and invalid open-item records
  before relying on the brief.
- Confirm that open questions before drafting are either answered, intentionally deferred, or
  escalated into a clear editorial decision. Mechanical validation does not imply editorial
  acceptance.
