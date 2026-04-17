# AI Pack Source

Canonical source for the agent companion files shipped in release artifacts.

The packaged release root is intentionally flat and ready to use: `AGENTS.md`, `CLAUDE.md`,
`.claude/`, `codex_skills/`, `writing_context/`, and `docs/wikitool/` sit next to the binary.

## Ownership

| Source | Packaged path | Role |
|---|---|---|
| `AGENTS.md` | `AGENTS.md` | Agent routing card |
| `CLAUDE.md` | `CLAUDE.md` | Same body as `AGENTS.md` |
| `.claude/rules/*` | `.claude/rules/*` | Claude always-on editing rules |
| `.claude/skills/*` | `.claude/skills/*` | Claude operator/review wrappers |
| `codex_skills/*` | `codex_skills/*` | Codex equivalents |
| `writing_context/*.md` | `writing_context/*.md` | Article-writing profile |
| `docs-bundle-v1.json` | `ai/docs-bundle-v1.json` | Optional offline docs preload |

`writing_context/` is deliberately not named after a model family. It is the target-wiki writing
profile: style, article structure, sourcing rules, and content-extension notes. Global agent
behavior belongs in `AGENTS.md` / `CLAUDE.md` and the skill wrappers.

## Host Overlay

`wikitool release package --host-project-root <PATH>` and `release build-matrix --host-project-root
<PATH>` may overlay host context:

1. Host `CLAUDE.md` becomes the active guidance body and is written to both packaged `CLAUDE.md`
   and packaged `AGENTS.md`.
2. Host `.claude/{rules,skills}` overlays packaged `.claude/{rules,skills}`.
3. Host `writing_context/` replaces the packaged writing profile at the same release-root path.

Without a host overlay, release bundles ship the generic wikitool-maintained context.

## Development Contract

1. Do not put local experiments, mock drafts, probe outputs, or one-off research notes under
   `ai-pack/` unless they are intended to ship in the next release.
2. Use `.wikitool/drafts/`, `plans/`, or test fixtures for scratch work.
3. Keep target-specific writing rules explicit. If a rule only applies to one wiki, label it as
   target-specific or ship it through a host overlay.
4. After CLI or workflow changes, update the owning agent/docs surface, regenerate
   `docs/wikitool/reference.md` when help changes, and run the guidance contract tests.

## Packaging Contract

1. `wikitool release build-ai-pack` stages these files.
2. `wikitool release package` stages one local binary with the AI companion files.
3. `wikitool release build-matrix` builds target binaries and emits zip artifacts that unpack into
   ready-to-run agent bundles.
