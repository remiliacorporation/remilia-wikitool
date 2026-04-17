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

1. `AGENTS.md` and `CLAUDE.md` are intentionally mirrored in shipped bundles so both agent front doors see the same guidance body.
2. Paths in these files must work in packaged artifacts (bundle-root relative), not only in source-repo layout.
3. Baseline `.claude/` content in this folder is packaged by default.
4. Baseline `llm_instructions/` content is the wikitool-maintained default writing context. It must be release-ready, not an experimental scratchpad.
5. Host overlay may replace/extend `.claude/` when `--host-project-root` is used.
6. Host overlay may replace `llm_instructions/` at the same packaged path when the host project provides that directory.

Development contract:

1. Do not place local experiments, mock drafts, probe outputs, or one-off research notes under `ai-pack/` unless they are intended to ship in the next release.
2. Use repo-local scratch space such as `.wikitool/drafts/`, `plans/`, or test fixtures for experimental work.
3. Keep target-specific writing rules explicit. If a rule only applies to one wiki, label it as target-specific or ship it through a host overlay instead of presenting it as universal MediaWiki behavior.
4. After CLI or workflow changes, update the relevant ai-pack guidance, regenerate `docs/wikitool/reference.md`, and run the guidance contract tests.

Packaging contract:

1. `wikitool release build-ai-pack` stages AI content from `ai-pack/`.
2. `wikitool release package` produces one host-target release folder where `wikitool` and AI companion files sit side by side.
3. `wikitool release build-matrix` builds target binaries and emits versioned zip artifacts (`wikitool-vX.Y.Z-<target>.zip`) that unpack into ready-to-run agent bundles.
4. Generic bundles include ai-pack `.claude/rules` and `.claude/skills` by default.
5. Host project context is overlaid only when `--host-project-root <PATH>` is provided.
