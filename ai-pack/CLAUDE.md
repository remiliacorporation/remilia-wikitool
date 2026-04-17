# CLAUDE.md

Canonical guidance for AI-assisted Remilia Wiki editing.

This file is designed for two contexts:

1. Source context: `tools/wikitool/ai-pack/`
2. Packaged context: unzipped release bundle root

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
5. SEO/network page diagnostics

Out of scope by default:

1. Infrastructure/deployment/server operations
2. Release engineering and artifact publishing
3. Risky writes (`--force`, bulk destructive actions) without explicit approval

## Canonical Paths

### Source repository layout

1. `tools/wikitool/ai-pack/CLAUDE.md`
2. `tools/wikitool/ai-pack/AGENTS.md`
3. `tools/wikitool/ai-pack/llm_instructions/*.md`
4. `tools/wikitool/ai-pack/.claude/rules/*`
5. `tools/wikitool/ai-pack/.claude/skills/*`
6. `tools/wikitool/ai-pack/codex_skills/*`
7. `tools/wikitool/docs/wikitool/*.md`
8. optional `tools/wikitool/ai-pack/docs-bundle-v1.json`

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
7. `docs/wikitool/guide.md`
8. `docs/wikitool/reference.md`
9. `wikitool --help` and `wikitool <command> --help`

## Skill Surfaces

Claude and Codex share the same 2-skill structure:

| Claude (`.claude/skills/`) | Codex (`codex_skills/`) | Role |
|---|---|---|
| `/wikitool` | `wikitool-operator` | When/how to use the CLI — retrieval, authoring, sync, diagnostics |
| `/review` | `wikitool-content-gate` | Pre-push validation contract — lint, validate, diff, gate report |

Skill bodies are structurally identical across platforms. They are thin overlays that defer to runbooks and CLI help.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Reach for `wikitool` when it adds wiki-aware value: local knowledge retrieval, MediaWiki-aware fetch/export, template/profile lookup, article lint/fix/validate, and guarded sync/push.
Do not route every step through `wikitool`.

## Operational Rules

1. Do not perform write pushes by default.
2. For requested writes, require:
   - `wikitool review --format json --summary "..."`
   - `wikitool diff`
   - `wikitool push --dry-run --summary "..."`
3. Do not use `--force` without explicit user approval.
4. Keep output factual, neutral, and encyclopedic.
5. Return raw MediaWiki wikitext for content edits.

## Useful Wikitool Lanes

These are example lanes, not a required sequence.

```bash
wikitool pull --full --all
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool wiki profile sync
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
wikitool knowledge article-start "Topic" --intent new --format json
wikitool research search "Topic" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool research mediawiki-templates "https://en.wikipedia.org/wiki/Cheetah" --template "Template:Speciesbox" --format json
wikitool templates show "Template:Infobox person"
wikitool templates examples "Template:Infobox person" --limit 2
wikitool wiki profile show --format json
wikitool article lint wiki_content/Main/Title.wiki --format json
wikitool article lint --changed --format json
wikitool knowledge inspect references duplicates --title "Title" --format json
wikitool knowledge pack "Topic" --format json
wikitool search "Category:"
wikitool status --modified --format json
wikitool status --conflicts --title "Title"
wikitool knowledge inspect chunks "Title" --query "aspect" --limit 6 --token-budget 480
wikitool knowledge inspect chunks --across-pages --query "topic" --max-pages 8 --limit 10 --token-budget 1200 --format json --diversify
wikitool module lint --format json
wikitool docs import-profile remilia-mw-1.44
wikitool docs context "extension name" --profile remilia-mw-1.44 --format json
wikitool fetch "https://www.mediawiki.org/wiki/Manual:Hooks"
wikitool export "https://www.mediawiki.org/wiki/Manual:Hooks" --subpages --combined --limit 25
wikitool export --urls-file sources.txt --output-dir wikitool_exports/sources --format markdown
wikitool validate --summary
wikitool validate --category broken-links --title "Title" --limit 20 --verify-live --format json
wikitool review --format json --summary "Summary"
wikitool diff
```

Docs bootstrap paths:

1. Preferred: `wikitool knowledge warm --docs-profile remilia-mw-1.44` then `wikitool knowledge status --docs-profile remilia-mw-1.44 --format json`
2. Admin surface: `wikitool docs import-profile remilia-mw-1.44`
3. Source offline bundle: `wikitool docs import --bundle ./ai-pack/docs-bundle-v1.json`
4. Packaged offline bundle: `wikitool docs import --bundle ./ai/docs-bundle-v1.json`

Use `knowledge status` before depending on docs-bridged local retrieval; it surfaces `readiness`, `degradations`, the requested docs profile, and the current `knowledge_generation`.
Use `knowledge pack` only when `article-start` is too collapsed and you need the deeper retrieval substrate. Its default `--payload compact` output separates subject context from wiki contract context and omits heavy implementation chunks; add `--payload full` only when you need full template/module implementation bodies or docs section text. Use `--contract-query` when the article topic and the template/module lookup differ, such as `wikitool knowledge article-start "Cheetah" --contract-query "species infobox taxonomy" --format json`. Use `wikitool knowledge contracts search "contract terms" --format json` for a direct token-budgeted search of the template/module graph.
Use `wikitool research mediawiki-templates "URL"` when a source MediaWiki page's live template/module contract matters, especially on arbitrary source wikis such as Wikipedia. Treat it as source-wiki context, not target-wiki permission; target-wiki template use must still pass local `knowledge contracts`, `templates show`, and `article lint`.
Use `wikitool export "URL"` for agent-readable markdown snapshots. MediaWiki URLs are fetched as wikitext before markdown rendering; arbitrary web pages use the research extractor and frontmatter metadata. Use `--subpages --limit N` for bounded MediaWiki tree stress tests, and `--urls-file PATH --output-dir PATH --format markdown` for off-wiki source packs with a generated `_index.md`. Wikitext export is only for recognizable MediaWiki URLs, and blocked arbitrary sources should remain explicit source-access failures.

## API Verification Rule

When investigating redirects, missing pages, or conflicting claims, verify against live API:

```bash
curl -s "https://wiki.remilia.org/api.php?action=query&list=search&srsearch=QUERY&format=json"
curl -s "https://wiki.remilia.org/api.php?action=query&titles=PAGE&prop=revisions&rvprop=content&format=json"
curl -s "https://wiki.remilia.org/api.php?action=query&titles=PAGE&redirects&format=json"
```

## Host Overlay Behavior

Release packaging may inject host context via `--host-project-root <PATH>`.

When host overlay is used:

1. Host `CLAUDE.md` becomes the effective guidance source.
2. Release packaging writes that same guidance body to both packaged `CLAUDE.md` and packaged `AGENTS.md`.
3. Wikitool-local guidance is preserved as `WIKITOOL_CLAUDE.md`.
4. Host `.claude/{rules,skills}` overlays packaged `.claude/{rules,skills}`.

Without host overlay, release bundles stay generic and ship only wikitool-maintained guidance.

## Documentation Sync Contract

When behavior changes, update all of:

1. `ai-pack/CLAUDE.md`
2. `ai-pack/AGENTS.md` (must match `CLAUDE.md`)
3. `ai-pack/llm_instructions/*.md`
4. `ai-pack/.claude/rules/*`
5. `ai-pack/.claude/skills/*`
6. `ai-pack/codex_skills/*`
7. `docs/wikitool/guide.md`
8. `docs/wikitool/reference.md`

Regenerate command reference after CLI changes from a source checkout with the maintainer surface enabled:

```bash
wikitool docs generate-reference
```
