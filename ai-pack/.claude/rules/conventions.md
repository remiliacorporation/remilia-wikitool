# Wikitool conventions

Run commands from the active project root or pass `--project-root`. Use CLI help for flags. Read the canonical substantive skill under `codex_skills/` rather than relying on a wrapper.

The generic binary has no implicit target-wiki policy. Inspect `wikitool config show --format json` and `wikitool adapter inspect --format json`; then read project-owned adapter guidance before site-specific work. Query `wiki capabilities show` and `catalog surface show` only when live or derived site mechanics matter.

Keep mechanical operations, editorial judgment, and human acceptance distinct in reports and commits.
