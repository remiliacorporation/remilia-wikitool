# Wikitool Agent Card

This is the compact packaged guidance for AI-assisted MediaWiki editing with wikitool. The same
guidance body is shipped as both `AGENTS.md` and `CLAUDE.md` so Claude, Codex, and other agent
front doors land on the same operating contract.

Use normal reasoning, ordinary shell/file tools, and direct editing by default. Reach for wikitool
when it adds wiki-aware value: local retrieval, MediaWiki-aware fetch/export, template/profile
lookup, article lint/fix/validate, or guarded sync/push. Do not route every step through wikitool.

## Contexts

Source checkout paths:

1. `ai-pack/AGENTS.md` and `ai-pack/CLAUDE.md`
2. `ai-pack/.claude/skills/`
3. `ai-pack/codex_skills/`
4. `ai-pack/writing_context/`
5. `docs/wikitool/`

Packaged release paths:

1. `AGENTS.md` and `CLAUDE.md`
2. `.claude/skills/`
3. `codex_skills/`
4. `writing_context/`
5. `docs/wikitool/`

Prefer packaged-root paths when working from an extracted release bundle.

## Surface Ownership

| Surface | Role |
|---|---|
| `README.md` | Human first-run and release-layout overview |
| `AGENTS.md` / `CLAUDE.md` | This compact agent routing card |
| `.claude/skills/wikitool.md` | Claude operator wrapper |
| `.claude/skills/review.md` | Claude pre-push gate wrapper |
| `codex_skills/` | Codex equivalents of the two wrappers |
| `writing_context/` | Writing rules and target-wiki editorial profile |
| `docs/wikitool/guide.md` | Detailed operator manual |
| `docs/wikitool/reference.md` | Generated CLI help reference |

Do not duplicate command reference material here. Check `wikitool --help`,
`wikitool <command> --help`, and `docs/wikitool/reference.md` for exact flags.

## Instruction Order

For article work, read in this order:

1. `writing_context/style_rules.md`
2. `writing_context/article_structure.md`
3. `writing_context/writing_guide.md`
4. `writing_context/extensions.md`
5. `.claude/rules/wiki-style.md`
6. `.claude/rules/safety.md`
7. `docs/wikitool/guide.md`
8. `docs/wikitool/reference.md`
9. CLI help

## Session Start

At the beginning of an editing session, inspect local changes and refresh the local wiki surface
before relying on indexed content:

```bash
wikitool status --modified --format json
wikitool diff --format json
wikitool pull --all --format json
wikitool knowledge warm --docs-profile remilia-mw-1.44 --format json
wikitool wiki profile sync --format json
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
```

Use `wikitool pull --full --all` only when the local database or sync ledger is missing, stale, or
being deliberately rebuilt. Do not use `--overwrite-local` unless the user explicitly approves
discarding local edits.

## Authoring Entry Points

Use these as front doors, then continue with normal research and editing judgment:

```bash
wikitool knowledge article-start "Topic" --intent new --format json
wikitool knowledge article-start "Topic" --intent expand --format json
wikitool knowledge article-start "Topic" --intent audit --format json
wikitool knowledge article-start "Topic" --intent refresh --format json
wikitool knowledge article-start "Cheetah" --contract-query "species infobox taxonomy" --format json
wikitool knowledge contracts search "contract terms" --format json
```

Use `knowledge pack` only when `article-start` is too collapsed and you need the deeper retrieval
substrate. Its default compact payload separates subject context from wiki contract context; use
`--payload full` only when implementation bodies or expanded docs text are needed.

## Research And Source Boundaries

Use `wikitool research search`, `research fetch`, and `research discover` for external evidence.
If fetch returns `status: "error"`, treat it as a source-access failure, not article evidence.

Use `wikitool research mediawiki-templates "URL"` when a source MediaWiki page's own
template/module contract matters, especially on arbitrary source wikis such as Wikipedia. Treat the
report as source-wiki context only. Target-wiki template use must still pass local
`knowledge contracts`, `templates show`, and `article lint`.

Use `wikitool wiki profile remote "URL"` only for an explicitly scoped remote target capability
probe when local import/profile data is unavailable. It reports extensions, parser tags,
namespaces, and API capabilities; it does not make source-wiki templates portable.

For local/custom content features, use the deployed target-wiki contract. Remilia's current
D3Charts surface is `Module:D3Chart` plus ResourceLoader; a future bespoke extension may supersede
that module form.

## Review Gate

Before any live write push:

```bash
wikitool article lint --changed --format json
wikitool validate --summary
wikitool validate --category broken-links --title "Title" --limit 20 --verify-live --format json
wikitool review --format json --summary "Summary"
wikitool diff
wikitool push --dry-run --summary "Summary"
```

Only push after the dry run is reviewed. Never use `--force` without explicit user approval.
For content investigations involving redirects, missing pages, or broken links, verify against the
live API at `https://wiki.remilia.org/api.php`.

## Host Overlay

Release packaging may inject host context with `--host-project-root <PATH>`.

When host overlay is used:

1. Host `CLAUDE.md` becomes the effective guidance source.
2. Release packaging writes that same guidance body to both packaged `CLAUDE.md` and packaged
   `AGENTS.md`.
3. Host `.claude/{rules,skills}` overlays packaged `.claude/{rules,skills}`.
4. If the host has `writing_context/`, those files become the packaged writing context.

Without host overlay, release bundles stay generic and ship only wikitool-maintained guidance.

## Development Hygiene

Do not put local experiments, mock drafts, probe outputs, or one-off research notes under
`ai-pack/` unless they are intended to ship in the next release. Use project scratch space such as
`.wikitool/drafts/`, `plans/`, or test fixtures.

When behavior changes, update the owning surface:

1. CLI behavior: command help and `docs/wikitool/reference.md`
2. Operator workflow: `docs/wikitool/guide.md` and the two skill wrappers
3. Agent routing: this file and its mirrored counterpart
4. Writing behavior: `writing_context/`
5. Release packaging: `README.md`, `ai-pack/README.md`, and release tests
