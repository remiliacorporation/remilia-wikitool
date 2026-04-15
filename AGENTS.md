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
- Hidden maintainer commands belong behind the `maintainer-surface` feature.
- The runtime project root is the caller's wiki project, not this source checkout, unless the
  command explicitly accepts a repository root.
- The local SQLite database is disposable state. Do not build correctness around preserving it.

## Verification

- Run targeted unit tests for touched modules.
- Run `cargo test --workspace` before considering source changes complete.
- Run `cargo clippy --workspace --all-targets -- -D warnings` for maintainer-facing cleanup or
  release-adjacent changes.
- For CLI contract changes, run the relevant command help and regenerate
  `docs/wikitool/reference.md` with `cargo run -- docs generate-reference`.
