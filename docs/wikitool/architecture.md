# Wikitool Architecture

Wikitool is shaped around agentic wiki work, not generic scraping. The long-term split is:

- `crates/wikitool` owns CLI argument parsing, command-family dispatch, text output, JSON view selection, and release packaging surfaces.
- `crates/wikitool_core` owns MediaWiki IO, sync state, indexing, retrieval, profile/catalog construction, parsing, linting, and reusable output models.
- `ai-pack/` owns shipped agent instructions and writing context. It should describe workflows and decision rules, not duplicate generated command help.
- `docs/wikitool/reference.md` is generated from CLI help and is the canonical flag reference.

## CLI Shape

CLI modules should stay thin at the top level. Large command families should use a facade plus
focused submodules:

- `knowledge_cli/`: build/warm/status, article-start, interview ledger, contract traversal, shared output helpers.
- `knowledge_inspect_cli/`: chunks, backlinks, templates, reference audits, index/page summaries.
- `review_cli/`: pre-push workflow orchestration, lint/validation/push dry-run checks, draft gates,
  next-step shaping, and report output.
- `sync_cli/`: init, pull, push, diff, status, delete, and sync presentation/shared selection helpers.
- `wiki_cli/`: capabilities, profile probes, rules, authoring surface, text printers, JSON summaries.

Adding a command should put the clap arguments near the facade and the implementation in the
owning command-family module. Shared behavior belongs in `wikitool_core` when it is reusable across
CLI lanes, or in a local `shared.rs` only when it is presentation glue.

## Agentic Token Contract

Default outputs must be useful in a constrained model context:

- Prefer interpreted entry points such as `knowledge article-start` and wikitool brief JSON views.
- Keep expanded output explicit: `--view full` is opt-in on brief-first surfaces.
- Keep retrieval bounded by `--limit`, `--token-budget`, and `--max-pages`; broad commands should
  return counts, summaries, and follow-up commands before full bodies.
- Preserve scoped drill-down lanes: `knowledge inspect chunks`, `knowledge inspect references`,
  `templates show/examples`, and `wiki surface show` should let agents ask for the next slice rather
  than loading the whole project.
- JSON envelopes should expose readiness, degradation, selection, and schema/version fields where
  agents need to reason about whether an answer is safe to use.

When changing output shape, update the owning docs/skill surface and add or adjust tests that lock
the compact/default behavior. Generated help changes require regenerating `docs/wikitool/reference.md`.

## Guidance Surfaces

Agent guidance should stay aligned with the command boundaries:

- Route authoring through `knowledge article-start`; use `knowledge contracts` and `knowledge inspect` for targeted drill-downs.
- Route new articles and substantial expansions through the knowledge-interview skill when human
  context can improve scope, source leads, or editorial framing, unless the user opts out.
- Use `wiki profile show` and `wiki surface show` for target-wiki contracts, not assumptions from
  source wikis.
- Use `knowledge inspect` subcommands for targeted retrieval and audit slices.
- Keep Claude and Codex wrappers thin and help-backed; the wrappers should name front doors and
  safety boundaries, not restate flags.

## Knowledge Interview Artifacts

The first human-in-loop authoring boundary is an agent skill plus a Rust-validated ledger artifact:
`.wikitool/interviews/<Title-safe>/<YYYYMMDDTHHMMSSZ>.brief.md`. These briefs capture distilled
user knowledge, supplied materials, candidate structure, source leads, open questions, and
high-risk interview statements.
They are not article prose, citation evidence, or a replacement for editorial quality-gating. On
Remilia Wiki, quality-gated human statements can become article prose as reasonable truth; cite when
research surfaces a source, when a claim is external or contested, or when a primary record exists.
Source paths may include target-wiki records, hosted artifacts, first-party sources, archived
primary records, creator-published statements, or target-wiki source notes; the architecture should
not require outside secondary coverage for niche subjects the wiki is preserving first.
It should also avoid forcing adjacent subjects into direct Remilia-relation framing. The interview
surface should elicit editorial vantage, adjacency, and canon purpose, then let articles cover
subjects as themselves unless a direct Remilia/Milady/community relationship is real and
article-shaping.

`knowledge interview init|validate|show|audit|open-item` owns deterministic path creation, starter
sidecars, frontmatter/section validation, typed open-items JSONL records with status transitions,
negative-evidence counts, freshness classification, compact summaries, and ledger audits.
The conversational interview loop stays in the agent skill. It is open-ended and critic-driven:
agents should ask broad initial questions, inspect supplied materials, reflect the emerging article
shape, and continue follow-up rounds while answers materially improve the article. `knowledge
article-start` remains the authoring scout front door and accepts `--brief-path` to surface a
validated interview summary. `review --brief-path` carries the same summary into the gate and fails
on invalid brief metadata. Mechanical validation proves the ledger is parseable, not that the
interview is complete or the article is good.
Neither command treats user assertions as evidence.

## Agentic Maturity Backlog

The ghidramink stack has several mature agent-facing patterns that map cleanly to wikitool without
importing reverse-engineering-specific machinery. Stage these as future implementation lanes:

- Add a strict maintainer `doc-audit` lane that verifies CLI help, generated reference, ai-pack
  Claude/Codex wrappers, writing context, and root redirect stubs against the live command surface.
- Keep compact wikitool brief views for high-value retrieval surfaces: `knowledge article-start`,
  `knowledge inspect chunks`, `templates show`, `wiki surface show`, and `review`. The default
  should be compact and evidence-rich; full bodies remain explicit opt-in.
- Make promotion gates first-class: draft-to-article promotion, template/catalog adoption, and
  push dry-runs should carry machine-readable evidence, blocking reasons, and next commands in one
  bounded receipt.
- Expand review integration only after real use: future checks can compare explicit claim/source
  metadata against drafts, but should avoid overfitting prose matching.
- Prefer capability-first shaping over broad inspection. Commands that expose optional detail
  levels should report accepted modes and defaults in JSON receipts so agents do not guess which
  payload shape is token-safe.
- Keep closeout receipts cheap and replayable: review, validate, docs/reference generation, and
  release packaging should produce bounded JSON summaries suitable for CI and agent handoff.
