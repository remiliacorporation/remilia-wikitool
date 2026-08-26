---
name: wikitool-operator
description: Research, author, revise, review, and safely sync evidence-bound MediaWiki articles with wikitool. Use for real encyclopedic prose drafting, wiki-grounded retrieval, source handling, interviews, wikitext, lint/fix, validation, exact human acceptance, and guarded publication.
---

# Wikitool operator

Use normal reasoning and direct editing. Verify current flags with `wikitool --help`, `wikitool
<command> --help`, and `docs/wikitool/reference.md`.

For article prose, read `writing_context/writing_guide.md`, `style_rules.md`, and
`article_structure.md`; read `visual_subjects.md` when applicable. Agent-authored prose is allowed
and should meet a real encyclopedic standard. Model memory, retrieval rank, search snippets, nearby
pages, and fluent synthesis are not evidence.

At session start, inspect `wikitool status --modified --format json` and `wikitool diff --format json`,
then run `wikitool workflow session-refresh` and `wikitool knowledge status`. Use
`workflow full-refresh` only for deliberate rebuilds or missing state. Never use
`pull --overwrite-local` without explicit approval.

Use `knowledge article-start --intent new|expand|audit|refresh --view brief` for the typed
coauthoring contract, evidence coverage, local fit signals, and open questions. `drafting_ready`
means the current retrieval artifacts are usable, not that the topic is researched or the draft is
publishable. Comparable outlines and categories are observations, not defaults. Route new articles,
substantial rewrites, niche history, and unclear intent through `wikitool-knowledge-interview`.
Use `knowledge contracts` for target template and module decisions.

Build a claim-source map before drafting. Use normal web search to choose arbitrary external
sources, then `research fetch`, `research discover`, and `export` for bounded extraction and
provenance. Use `research wiki-search` only for the configured target wiki. Relay access challenges
to the user rather than bypassing them. Inspect the exact source behind each important claim.

Draft from the factual spine, write the lead after the body, and perform the adversarial reader edit
in `style_rules.md`. Define the subject on its own terms. Do not force Remilia, Charlotte Fang,
Milady, or community framing from local adjacency. Keep uncertainty, source attribution, and
living-person risk visible.

Use `article lint` and `article fix --apply safe` for deterministic diagnostics and mechanical
repairs. These checks cannot establish source fidelity or prose quality. A named human must read the
exact final prose and run or explicitly direct `article accept`, using `agent-draft`,
`collaborative-draft`, or the other truthful origin. Never self-attest. Acceptance is hash-bound;
the stated identity is an audit assertion, not cryptographic authentication. Promotion and changed
Main-namespace pushes require the receipt, and `--force` does not bypass it.

Use scoped `validate --verify-live`, `status`, `diff`, `review --view brief`, and `push --dry-run`
before a live push. After dynamic rendering changes, run `wiki render-check` for every relevant
consumer shape.
