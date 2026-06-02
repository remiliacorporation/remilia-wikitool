---
name: wikitool-knowledge-interview
description: Interview users for wiki authoring and review knowledge before research and drafting when human context matters.
---

# Skill: wikitool-knowledge-interview

Thin wrapper for the human knowledge interview faculty that supports wikitool authoring.

Use normal reasoning, ordinary shell/file tools, and direct editing by default. Verify wikitool
commands against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.
The conversational loop belongs to the agent; the Rust CLI owns deterministic ledger creation,
validation, summaries, audits, and structured open-item logging through
`wikitool knowledge interview init|validate|show|audit|open-item`.

Read `writing_context/interview_playbook.md` before using this skill. Start with a compact scout:
`wikitool knowledge article-start "<Topic>" --intent new|expand|audit|refresh --format json --view brief`,
plus a cursory wiki/source search when useful. Then interview only to the depth that improves the
article or review.

Default to an interview for new articles and substantial expansions unless the user explicitly opts
out. Skip it for mechanical lint, link, sync, source-fetch, or validation tasks unless a conflict
requires user judgment. Begin with a freeform dump, reflect the scope back in neutral wiki language,
and ask adaptive follow-ups based on evidence gaps.

When the interview yields reusable knowledge, write a brief under
`.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. Treat the brief as working notes,
not article prose or citation evidence. Use claim IDs only for interview-introduced or high-risk
claims that need tracking through research and review.

Use `wikitool knowledge interview init "<Topic>" --intent new|expand|audit|refresh --format json`
to create the timestamped brief and sidecars, then fill the brief from the interview. Before
drafting from it, run `wikitool knowledge interview validate PATH --format json`; use
`wikitool knowledge interview show PATH --view brief --format json` and
`wikitool knowledge interview audit --view brief --format json` for handoff and ledger checks.
Use `wikitool knowledge interview open-item add PATH --kind rejected-source|inaccessible-source|missing-source|scope-unresolved --text "..."`
to record unresolved research work or negative evidence without turning it into article prose.
Pass validated briefs to `wikitool knowledge article-start "<Topic>" --brief-path PATH --format json --view brief`
and `wikitool review --brief-path PATH --format json --view brief --summary "..."` when the
interview should inform research planning or review.
