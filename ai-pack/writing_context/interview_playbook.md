# Knowledge Interview Playbook

Use this playbook when a user asks for article creation, substantial expansion, article refresh, or a
review. The interview is how the agent finds out what the person actually wants written, not only what
they happen to know that the public record lacks. It is a general elicitation workflow, not a script
for one article type and not a checklist to rush through.

Interview briefs are working notes. They are not article prose, citation evidence, proof that a
claim is true, or proof that the interview is complete. Treat user assertions as leads, then write
them as reasonable encyclopedic truth once they survive editorial quality-gating. Cite when research
surfaces a real source, when a claim is external, contested, or surprising, or when a primary record
exists, and anchor to comparable wiki pages, target-wiki records, hosted artifacts, first-party
posts, archived primary records, creator-published statements, or target-wiki source notes when you can. For Remilia
Wiki, a useful source path does not have to be external or secondary; the wiki often exists because
no broader source has preserved the subcultural record. Never route a primary or first-party fact
through a weaker third party just to manufacture an external citation.

## When To Interview

Interviewing is an optional, conversational lane, not a rigid gate before drafting - but for real
article work it is the normal move, not an exception. Its job is direction: before drafting, draw out
the subject as the person sees it - what the article should be about and where its emphasis sits, what
they already know, details that may never have been put online, and any sources, artifacts, or context
the agent should use. On Remilia Wiki every subject is written from the wiki's own perspective, so this
matters even when the topic is exhaustively documented elsewhere: an interview turns a generic entry on,
say, the manul into the manul as Remilia Wiki would frame it - what a person writing here wants it to
foreground and how it sits in the wiki's world. A contributor writing for this wiki already has that
lens; the interview surfaces it. This is framing, not a forced "relationship to Remilia" section.

Reach for it by default on:

- New articles, where the intent, scope, and angle are worth setting with the person before drafting,
  including well-documented subjects where the value is the wiki's framing rather than missing facts.
- Substantial expansions or rewrites, where the user's taste and domain model should shape the scope.
- Audits or refreshes, where mechanical checks reveal unresolved conflicts, missing context, or
  editorial choices that cannot be decided from sources alone.

Keep it light or skip it for:

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
   future editor: what the subject is, origin, date/order details where they disambiguate versions or records, names and terminology, relationships,
   aesthetics or mechanics, what makes it worth canonicalizing from the wiki's perspective, what
   outsiders miss, what is uncertain, and what the article must not overstate.
4. Reflect the shape back. Summarize the emerging scope in neutral wiki language, separating
   source-backed facts, user-provided leads, source leads from supplied materials, open questions,
   and editorial constraints. Do not collapse important context without a clear source path into thin prose;
   turn it into follow-up questions, source requests, artifact-description decisions, or a durable
   primary-record publishing path.
5. Ask adaptive follow-ups. Focus questions on gaps that change article structure, not on trivia the
   agent can find mechanically. There is no fixed number of rounds and no hard cap. Continue as long
   as new answers materially improve the article's scope, date/order disambiguation, terminology, source strategy,
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
relationship is real, sourceable or quality-gated, and central enough to improve the article.

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
wikitool knowledge interview open-item update .wikitool/interviews/Title/20260601T172430Z.brief.md --item-id OI-20260601T172430Z --status resolved
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
- Chronology: dates or order details only when they disambiguate versions, source records, release order, or handoff state; do not force a timeline when it does not improve the article.
- Entities and Relationships: people, projects, groups, terms, and related wiki pages.
- Editorial Framing: recommended angle, tone risks, likely misconceptions, and terminology notes.
- Research Plan: primary-source leads, search queries, archive targets, pages to inspect, and
  blocking evidence gaps.
- Interviewer Critic Notes: the agent's critique of the emerging article plan, including thinness,
  duplication, source, framing, and missing-context risks that should trigger more questions.
- Draft Plan: likely sections, infobox/template candidates, categories to verify, statements that
  require citations, and open questions before drafting.

For machine-readable Draft Plan extraction, write `Likely sections` and `Open questions before
drafting` one item per line, or use semicolons on the label line. Do not comma-separate section
names; commas often belong inside natural headings.

Do not put planning labels such as "Editorial vantage", "Remilia Wiki context", or "Why this
belongs here" into `Likely sections` unless they name an actual sourceable article section. Use
those as open questions or framing notes. The article sections should describe the subject.

Record source leads and unresolved follow-up as structured open items in the `.open_items.jsonl`
sidecar rather than only as prose, so future sessions do not rediscover them.

Use structured open items for unresolved research work and negative evidence that future agents
should not rediscover from scratch. Prefer specific kinds such as `missing-source`,
`do-not-assert`, `rejected-source`, `inaccessible-source`, `disproven-link`,
`source-wiki-only-template`, `rejected-category`, `scope-unresolved`, and `privacy-exclusion`.
Use `do-not-assert` for a creator's reading or an unsourced claim that should not enter article prose
until a source exists. Open items are not article content; they are ledger entries for follow-up,
source rejection, access failure, do-not-assert holds, or editorial uncertainty.

## Drafting Boundary

Before drafting, convert the brief into a research plan and an editorial sufficiency check:

- Treat quality-gated human statements as reasonable truth; cite when research surfaces a real
  source, when a claim is external, contested, or surprising, or when a primary record exists, and
  anchor to target-wiki pages, hosted artifacts, first-party sources, archived primary records, or
  creator-published statements or target-wiki source notes when you can. Do not launder a primary fact through a weaker
  third party for the sake of an external citation.
- Preserve the user's thematic framing only when it survives neutral article language.
- Attribute disputed or source-specific claims rather than presenting them as settled facts.
- Omit or defer material that is contested but unverifiable, or too thin for the target wiki, but do
  not let omission silently produce a poor article. If the omitted material is central, ask more
  questions, request a primary record, propose a source-note or creator-statement lane, or recommend
  redirect/merge until the article has enough shape. Log a `do-not-assert` open item for anything
  held back so a later session can revisit it.
- Run `wikitool knowledge interview validate PATH --format json` and resolve invalid metadata,
  missing sidecars, and invalid open-item records before relying on the brief.
- Confirm that open questions before drafting are either answered, intentionally deferred, or
  escalated into a clear editorial decision. Mechanical validation does not imply editorial
  acceptance.
