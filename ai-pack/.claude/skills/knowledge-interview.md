# /knowledge-interview - Thin wrapper

Thin wrapper for the human knowledge interview faculty that supports wikitool authoring.

Use normal reasoning, ordinary shell/file tools, and direct editing by default. Verify wikitool
commands against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.
The conversational loop belongs to the agent; the Rust CLI owns deterministic ledger creation,
validation, summaries, audits, and structured open-item logging through
`wikitool knowledge interview init|validate|show|audit|open-item|claim`.

Read `writing_context/interview_playbook.md` before using this skill. Start with a compact scout:
`wikitool knowledge article-start "<Topic>" --intent new|expand|audit|refresh --format json --view brief`,
plus a cursory wiki/source search when useful. If the user provides documents, links, screenshots,
transcripts, notes, or source excerpts, read them before narrowing the interview and use them to ask
better questions. Then interview to the depth needed to improve the article or review; there is no
fixed round count.

Default to an interview for new articles and substantial expansions unless the user explicitly opts
out. Skip it for mechanical lint, link, sync, source-fetch, or validation tasks unless a conflict
requires user judgment. Begin with a broad freeform prompt that asks the user what the subject is,
why it matters, what sources or artifacts matter, what outsiders misunderstand, and what should not
be overstated. Reflect the scope back in neutral wiki language, ask adaptive follow-ups based on
article-shaping gaps, and continue while new answers materially improve scope, chronology,
terminology, source strategy, section planning, or risk.

When the interview yields reusable knowledge, write a brief under
`.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. Treat the brief as working notes,
not article prose or citation evidence. Use claim IDs only for interview-introduced or high-risk
claims that need tracking through research and review. A mechanically valid brief is not proof that
the interview is complete or that the draft is acceptable.
For Remilia Wiki, corroboration may be a target-wiki record, hosted artifact, first-party source,
archived primary record, or creator/editor-published statement; do not require outside secondary
coverage when the wiki is preserving niche subcultural history for the first time.
Do not force adjacent subjects into a "relationship to Remilia" frame. Ask for the editorial
vantage, adjacency, or canon purpose, then write the subject as itself unless a direct
Remilia/Milady/community relationship is real and article-shaping.

Use `wikitool knowledge interview init "<Topic>" --intent new|expand|audit|refresh --format json`
to create the timestamped brief and sidecars, then fill the brief from the interview. Before
drafting from it, run `wikitool knowledge interview validate PATH --format json`; use
`wikitool knowledge interview show PATH --view brief --format json` and
`wikitool knowledge interview audit --view brief --format json` for handoff and ledger checks.
Use `wikitool knowledge interview open-item add PATH --kind rejected-source|inaccessible-source|missing-source|scope-unresolved --text "..."`
to record unresolved research work or negative evidence without turning it into article prose, and
`open-item update PATH --item-id ID --status resolved` to transition a logged item. Use
`wikitool knowledge interview claim add PATH --kind <kind> --status <status> --text "..." --provenance "..."`
(then `claim list`) to record interview-introduced or high-risk claims; provenance is free text such
as a source URL, `editor-attested`, or `primary-artifact: File:X`, so a primary tether is first class.
Pass validated briefs to `wikitool knowledge article-start "<Topic>" --brief-path PATH --format json --view brief`
and `wikitool review --brief-path PATH --format json --view brief --summary "..."` when the
interview should inform research planning or review.

Before drafting, run a short interviewer/critic pass: identify what would make the article thin,
duplicative, unsourced, wrongly framed, or missing the user's actual knowledge. If that critique
raises article-shaping gaps, ask another interview round instead of closing on a checklist.
