# AI Pack Source

This directory is the canonical source for AI companion content shipped with release artifacts.

Contents:

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.claude/rules/*`
4. `.claude/skills/*`
5. `llm_instructions/*.md`
6. `codex_skills/*`
7. optional `docs-bundle-v1.json` (schema version `1` for `wikitool docs import --bundle`)

Instruction contract:

1. `AGENTS.md` and `CLAUDE.md` are intentionally mirrored and must stay in lockstep.
2. Paths in these files must work in packaged artifacts (bundle-root relative), not only in source-repo layout.
3. Baseline `.claude/` content in this folder is packaged by default.
4. Host overlay may replace/extend `.claude/` when `--host-project-root` is used.

Packaging contract:

1. `wikitool release build-ai-pack` stages AI content from `ai-pack/`.
2. `wikitool release package` produces one host-target release folder where `wikitool` and AI companion files sit side by side.
3. `wikitool release build-matrix` builds target binaries and emits versioned zip artifacts (`wikitool-vX.Y.Z-<target>.zip`) that unpack into ready-to-run agent bundles.
4. Generic bundles include ai-pack `.claude/rules` and `.claude/skills` by default.
5. Host project context is overlaid only when `--host-project-root <PATH>` is provided.
6. Wikitool-local AI guidance is preserved as `WIKITOOL_CLAUDE.md` when host context is injected.
