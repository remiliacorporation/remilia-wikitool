# Codex Skills Bundle

Three skills matching the Claude Code `.claude/skills/` surface:

1. `wikitool-operator` - evidence-bound encyclopedic authoring, retrieval, sync, and diagnostics
2. `wikitool-content-gate` - source-fidelity review, exact human acceptance, and pre-push gating
3. `wikitool-knowledge-interview` - human knowledge intake for agent or collaborative drafting

These are thin overlays. Canonical truth is `AGENTS.md`, `docs/wikitool/*`, `writing_context/*`,
and live CLI help. Use `wikitool` where it adds wiki-aware value; use normal reasoning and file
tools for everything else. Agent-authored prose is supported; model output is not evidence, and a
named human must accept the exact article before publication.
