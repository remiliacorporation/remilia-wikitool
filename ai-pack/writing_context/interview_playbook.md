# Knowledge Interview Playbook

Use this playbook when a user asks for article creation, substantial expansion, article refresh,
or a review where human context can materially improve the article. It is a general knowledge
distillation workflow, not a script for one article type.

Interview briefs are working notes. They are not article prose, citation evidence, or proof that a
claim is true. Treat user assertions as leads to corroborate with sources, comparable wiki pages,
or target-wiki records before publishing them as facts.

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
2. Start with a freeform dump. Invite the user to say what they know, what matters, what common
   misunderstandings exist, and what the article should not overstate.
3. Reflect the shape back. Summarize the emerging scope in neutral wiki language, separating
   source-backed facts, user-provided leads, open questions, and editorial constraints.
4. Ask evidence-gated follow-ups. Focus questions on gaps that change article structure, not on
   trivia the agent can find mechanically. Keep rounds adaptive; do not force a fixed checklist.
5. Stop when marginal value drops. Move to research and drafting once the article shape, likely
   sections, major claims, and unresolved risks are clear.

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
- User-Framed Summary: the user's high-level framing in neutral wiki language.
- Claim Map Summary: a short read of the interview claims; the claims themselves live in the
  `.claims.json` sidecar with provenance and status, not in the brief prose.
- Chronology: known dates, approximate periods, and open timeline gaps.
- Entities and Relationships: people, projects, groups, terms, and related wiki pages.
- Editorial Framing: recommended angle, tone risks, likely misconceptions, and terminology notes.
- Research Plan: primary-source leads, search queries, archive targets, pages to inspect, and
  blocking evidence gaps.
- Draft Plan: likely sections, infobox/template candidates, categories to verify, claims that
  require citations, and open questions before drafting.

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

Before drafting, convert the brief into a research plan:

- Corroborate factual claims with external sources, target-wiki pages, or known primary records.
- Preserve the user's thematic framing only when it survives neutral article language.
- Attribute disputed or source-specific claims rather than presenting them as settled facts.
- Omit claims that remain uncited, unverifiable, or too thin for the target wiki.
- Run `wikitool knowledge interview validate PATH --format json` and resolve invalid metadata,
  duplicate claim IDs, missing sidecars, unsupported claim statuses, and invalid open-item records
  before relying on the brief.
