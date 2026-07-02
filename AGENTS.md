# Wikitool Development Guidance

This file is for implementation work in the wikitool source checkout. It is not the packaged
operator guidance shipped to wiki users. User-facing AI companion guidance lives in
`ai-pack/CLAUDE.md`, `ai-pack/AGENTS.md`, `ai-pack/.claude/skills/`, and
`ai-pack/codex_skills/`.

When changing shipped behavior, update the relevant operator guidance and regenerate
`docs/wikitool/reference.md`. When changing only internal implementation practice, keep the change
scoped here.

## Implementation Rules

- Closely corroborate all implementation work against the authoritative sources for the project:
  specifications, existing code, documentation, tests, and observed runtime behavior.
- Prefer directly evidenced behavior over inferred design.
- When work reveals a canonical or directly evidenced name that supersedes a current label, stage
  that rename across all relevant locations in the same changeset unless a documented blocker
  prevents immediate closeout.
- Implement for correctness first.
- Treat established naming, structure, and subsystem boundaries as evidence, not obligations.
- Preserve them where they aid correctness or comprehension, but not mechanically.
- Where behavior is not directly established, state the uncertainty explicitly, document the gap at
  the relevant site, and do not present hypotheses as facts.
- Do not silently infer missing behavior.
- Do not add defensive code, fallback paths, or error-mitigating logic that obscures divergence from
  the specification or expected behavior.
- Surface errors, mismatches, and unhandled states immediately and locally.
- Prefer explicit assertions, narrow failure points, and observable diagnostics over hidden recovery.
- If the correct behavior at a site is unknown, that unknowing should be visible in the code.
- Write lean, maintainable code with high local comprehensibility.
- Minimize implicit state, cross-file indirection, and abstractions not yet justified by repeated
  evidence.
- Avoid premature generalization.
- Only extract shared machinery when multiple cases demonstrably share the same behavior and
  constraints.
- Use full-cutover judgment where appropriate, but confine changes to what is directly motivated by
  the current work.
- Do not perform speculative rewrites of adjacent code just because it appears improvable.
- If adjacent code is suspect, note it and continue.

## Source Contracts

- Avoid regex-based parsing for wikitext, HTML extraction, and command-contract logic. Use
  deterministic state machines, structured parsers, or character-by-character parsing.
- Keep CLI output contracts explicit. Agent-facing commands should prefer `--format json` when the
  output is consumed programmatically.
- Hidden maintainer commands belong behind the explicit `maintainer` feature; default
  builds are end-user builds.
- The runtime project root is the caller's wiki project, not this source checkout, unless the
  command explicitly accepts a repository root.
- The local SQLite database is disposable state. Do not build correctness around preserving it.

## Verification

- Run targeted unit tests for touched modules.
- Run `cargo test --workspace` before considering source changes complete.
- Run `cargo clippy --workspace --all-targets -- -D warnings` for maintainer-facing cleanup or
  release-adjacent changes.
- For CLI contract changes, run the relevant command help and regenerate
  `docs/wikitool/reference.md` with `cargo run --features maintainer -- docs generate-reference`.

## Bounded Output

Bootstrap once per checkout with `bash scripts/fetch_contextmink.sh --install` (fetches the pinned
contextmink release, installs `tools/contextmink/bin/` and the `scripts/contextmink` launcher; both
are gitignored). Then:

Use `scripts/contextmink` when a file/text/JSON/SQLite/command-output read may
produce more output than the transcript should carry.

- Start with `dirs` to orient in an unfamiliar tree, then `files` or `grep`
  for candidate discovery. Prefer `files --ext json` / `--extension jsonl`
  across Windows-to-Bash boundaries because wildcard globs can expand before
  contextmink receives them.
- Read source files through `outline` then `slice`, not dump windows. A named
  file is still reconnaissance while the answer's location inside it is
  unknown: `outline <file>` maps declaration lines with line numbers
  (`--contains TEXT` filters rows; `--lang`, `--prefix <text>`, or
  `--pattern <regex>` cover unrecognized extensions), then
  `slice --range START:END` prints the region. `slice` replaces `sed -n` /
  `cat` / `head` file windows. Keep its default caps (120-line window,
  220-line ceiling); narrow an oversized read with `outline` or
  `grep --context` instead of raising `--max-lines`.
- Use `grep --pattern-file <file>` for shell-fragile regex; use `grep-terms`
  for literal tokens or phrases (`--or` / `--any`, `--term-file`, `--limit`,
  `--max-matches`). Narrow either with `--glob` / `--ext`, add `-i` for
  case-insensitive matching, and `--context N` when the surrounding lines
  would otherwise need a follow-up `slice`.
- Use `slice --tail N` for the end of logs, `json-find`, `json-select` (with
  `--where FIELD=VALUE` / `--where-contains FIELD=TEXT` row filters),
  `sqlite-schema`, and `sqlite --sql-file` for bounded reads instead of
  opening whole large files, reports, or databases.
- Prefer a domain command's native compact/projection/limit flags first. Use
  `capture -- <command> ...` or `run` only when output size is uncertain and no
  native bound exists; read the child `exit_code`/`success` fields in the
  receipt. Truncated captures keep both the head and the tail of the output.
- Treat a `CONTEXTMINK_RECEIPT` with `"truncated": true` or `"complete": false`
  as capped output and narrow the query. When `cap_reason` is `"scan"` or
  lower-bound fields are true, totals and no-match results cover only the
  scanned subset. A no-match grep with `no_match_scope: "scanned_subset"` or a
  `json-select` with `all_null_fields` entries needs a narrower or corrected
  query, not a conclusion.
- Direct commands are fine when output is already known to be small or
  structurally bounded: `git status --short`, `git diff --stat`, a focused
  test command, a domain tool that emits compact records, or one exact file
  region already known to fit a slice window (about 120 lines). Above that,
  the read is reconnaissance — go through `outline`/`grep`/`slice`.
