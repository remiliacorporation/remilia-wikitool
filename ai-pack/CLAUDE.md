# CLAUDE.md

Canonical guidance for AI-assisted Remilia Wiki editing.

This file is designed for two contexts:

1. Source context: `custom/wikitool/ai-pack/`
2. Packaged context: unzipped release bundle root

`AGENTS.md` must remain byte-identical to this file.

## Mission

Provide a comprehensive wiki editing suite that works out of the box in release artifacts:

1. Content authoring and revision
2. Template/module editing support
3. Research and citation workflows
4. Cleanup/review gates
5. Page-level diagnostics

Default behavior excludes wiki-ops/admin work.

## Scope

In scope:

1. Article writing/editing in raw MediaWiki wikitext
2. Template/module/category/linking workflows
3. Research and source verification
4. Pre-push quality gates
5. SEO/network/perf page diagnostics

Out of scope by default:

1. Infrastructure/deployment/server operations
2. Release engineering and artifact publishing
3. Risky writes (`--force`, bulk destructive actions) without explicit approval

## Canonical Paths

### Source repository layout

1. `custom/wikitool/ai-pack/CLAUDE.md`
2. `custom/wikitool/ai-pack/AGENTS.md`
3. `custom/wikitool/ai-pack/llm_instructions/*.md`
4. `custom/wikitool/ai-pack/.claude/rules/*`
5. `custom/wikitool/ai-pack/.claude/skills/*`
6. `custom/wikitool/ai-pack/codex_skills/*`
7. `custom/wikitool/docs/wikitool/*.md`
8. optional `custom/wikitool/ai-pack/docs-bundle-v1.json`

### Packaged release layout

1. `CLAUDE.md`
2. `AGENTS.md`
3. `llm_instructions/*.md`
4. `.claude/rules/*`
5. `.claude/skills/*`
6. `codex_skills/*`
7. `docs/wikitool/*.md`
8. optional `ai/docs-bundle-v1.json`
9. optional `WIKITOOL_CLAUDE.md` when host overlay is injected

If path references conflict, prefer packaged-root paths first.

## Instruction Priority

Use this order:

1. `llm_instructions/style_rules.md` (non-negotiable)
2. `llm_instructions/article_structure.md`
3. `llm_instructions/writing_guide.md`
4. `llm_instructions/extensions.md`
5. `.claude/rules/wiki-style.md`
6. `.claude/rules/safety.md`
7. `docs/wikitool/how-to.md`
8. `docs/wikitool/reference.md`
9. `wikitool --help` and `wikitool <command> --help`

## Skill Surfaces

### Claude skill files

The packaged `.claude/skills/` suite includes:

1. `/article`
2. `/template`
3. `/sync`
4. `/research`
5. `/cleanup`
6. `/review`
7. `/seo`
8. `/mw-fetch`
9. `/wikitool`

These are thin workflow wrappers, not command-surface authorities.

### Codex skill files

`codex_skills/` includes:

1. `wikitool-operator`
2. `wikitool-content-gate`

Codex skills must remain thin overlays that defer to runbooks and CLI help.

## Operational Rules

1. Do not perform write pushes by default.
2. For requested writes, require:
   - `wikitool diff`
   - `wikitool validate`
   - `wikitool push --dry-run --summary "..."`
3. Do not use `--force` without explicit user approval.
4. Keep output factual, neutral, and encyclopedic.
5. Return raw MediaWiki wikitext for content edits.

## Preferred Commands

```bash
wikitool pull --full --all
wikitool context "Template:Infobox person"
wikitool search "Category:"
wikitool docs search "extension name"
wikitool fetch "https://www.mediawiki.org/wiki/Manual:Hooks"
wikitool export "https://www.mediawiki.org/wiki/Manual:Hooks" --subpages --combined
wikitool validate
wikitool diff
```

Docs bundle import path mapping:

1. Source: `wikitool docs import --bundle ./ai-pack/docs-bundle-v1.json`
2. Packaged: `wikitool docs import --bundle ./ai/docs-bundle-v1.json`

## API Verification Rule

When investigating redirects, missing pages, or conflicting claims, verify against live API:

```bash
curl -s "https://wiki.remilia.org/w/api.php?action=query&list=search&srsearch=QUERY&format=json"
curl -s "https://wiki.remilia.org/w/api.php?action=query&titles=PAGE&prop=revisions&rvprop=content&format=json"
curl -s "https://wiki.remilia.org/w/api.php?action=query&titles=PAGE&redirects&format=json"
```

## Host Overlay Behavior

Release packaging may inject host context via `--host-project-root <PATH>`.

When host overlay is used:

1. Host `CLAUDE.md` becomes packaged `CLAUDE.md` and `AGENTS.md`.
2. Wikitool-local guidance is preserved as `WIKITOOL_CLAUDE.md`.
3. Host `.claude/{rules,skills}` overlays packaged `.claude/{rules,skills}`.

Without host overlay, release bundles stay generic and ship only wikitool-maintained guidance.

## Documentation Sync Contract

When behavior changes, update all of:

1. `ai-pack/CLAUDE.md`
2. `ai-pack/AGENTS.md` (must match `CLAUDE.md`)
3. `ai-pack/llm_instructions/*.md`
4. `ai-pack/.claude/rules/*`
5. `ai-pack/.claude/skills/*`
6. `ai-pack/codex_skills/*`
7. `docs/wikitool/how-to.md`
8. `docs/wikitool/reference.md`
9. `docs/wikitool/explanation.md`

Regenerate command reference after CLI changes:

```bash
wikitool docs generate-reference
```
