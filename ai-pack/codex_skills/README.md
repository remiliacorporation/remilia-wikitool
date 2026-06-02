# Codex Skills Bundle

Three skills matching the Claude Code `.claude/skills/` surface:

1. `wikitool-operator` - when/how to use the CLI (authoring, retrieval, sync, diagnostics)
2. `wikitool-content-gate` - pre-push review contract (`wikitool review`, scoped lint/validate/diff)
3. `wikitool-knowledge-interview` - human knowledge intake for article creation, expansion, and non-mechanical review gaps

These are thin overlays. Canonical truth is `AGENTS.md`, `docs/wikitool/*`, `writing_context/*`,
and live CLI help. Use `wikitool` where it adds wiki-aware value; use normal reasoning and file
tools for everything else.
