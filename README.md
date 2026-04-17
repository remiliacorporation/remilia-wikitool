# Wikitool

MediaWiki editing, retrieval, research, linting, and sync in one self-contained CLI.

Wikitool is built for agentic wiki work: local files stay human-editable, SQLite holds the
retrieval/index layer, and release bundles ship the guidance files that Claude, Codex, and other
agents need to use the tool without a separate unpack step.

## Release Layout

A release zip unpacks into a ready-to-run folder:

```text
wikitool(.exe)
README.md
AGENTS.md
CLAUDE.md
.claude/
codex_skills/
writing_context/
docs/wikitool/
manifest.json
LICENSE*
```

There is no separate setup document. This README is the top-level entry point; the detailed
operator manual is `docs/wikitool/guide.md`.

## First Run

From the extracted release folder, or from any wiki project root with `wikitool` on `PATH`:

```bash
wikitool init --templates
wikitool pull --full --all
wikitool knowledge warm --docs-profile remilia-mw-1.44 --docs-mode missing
wikitool wiki profile sync
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
```

Read-only workflows do not need credentials. Push/delete writes need bot credentials in `.env`:

```bash
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
WIKI_URL=https://your-wiki.example.org/
WIKI_API_URL=https://your-wiki.example.org/api.php
```

`WIKI_URL` and `WIKI_API_URL` are optional when the materialized config already points at the
target wiki.

## Session Refresh

At the start of an agentic editing session, inspect local changes and refresh wiki state:

```bash
wikitool status --modified --format json
wikitool diff --format json
wikitool pull --all --format json
wikitool knowledge warm --docs-profile remilia-mw-1.44 --docs-mode missing --format json
wikitool wiki profile sync --format json
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
```

Use `pull --full --all` for first syncs, missing sync state, or deliberate rebuilds. Do not use
`--overwrite-local` unless local edits should be discarded.

## Authoring Loop

```bash
wikitool knowledge article-start "Topic" --intent new --format json
wikitool research wiki-search "Topic" --format json
wikitool research fetch "https://example.org/source" --format rendered-html --output json
wikitool templates show "Template:Infobox person"
# edit wiki_content/Main/Topic.wiki
wikitool article lint wiki_content/Main/Topic.wiki --format json
wikitool knowledge inspect references duplicates --title "Topic" --format json
wikitool review --format json --summary "Add article on Topic"
wikitool push --dry-run --title "Topic" --summary "Add article on Topic"
```

## What It Does

- `knowledge article-start` builds an interpreted authoring brief.
- `knowledge contracts` and `templates` expose target-wiki template/module contracts.
- `research wiki-search` queries the configured wiki API; `research fetch/discover/mediawiki-templates` gathers source URLs and source-wiki evidence.
- `export` writes agent-readable markdown source packs.
- `article lint/fix`, `validate`, `module lint`, and `review` gate content before push.
- `pull`, `status`, `diff`, and `push --dry-run` keep local files synchronized with the live wiki.

## Documentation

| Surface | Role |
|---|---|
| `README.md` | Top-level first-run, session, and release-layout overview |
| `AGENTS.md` / `CLAUDE.md` | Compact packaged agent routing card |
| `.claude/skills/` | Claude `/wikitool` and `/review` wrappers |
| `codex_skills/` | Codex skill equivalents |
| `writing_context/` | Article-writing rules and Remilia default writing profile |
| `docs/wikitool/guide.md` | Detailed operator manual |
| `docs/wikitool/reference.md` | Generated command reference |
| `VERSIONING.md` | Maintainer version and release checklist |
| `RELEASE_LOG.md` | Release history |

Every command has `--help`. In a source checkout with the maintainer surface enabled, regenerate the
reference with:

```bash
wikitool docs generate-reference
```

## Source Builds

```bash
cargo build --package wikitool --release
```

Source builds keep maintainer-only commands enabled. To build the release-equivalent end-user
binary:

```bash
cargo build --package wikitool --release --no-default-features
```

## Runtime State

```text
project-root/
  .env
  .wikitool/config.toml
  .wikitool/data/wikitool.db
  wiki_content/
  templates/
```

The SQLite database is derived and disposable. If local state is incompatible or stale, reset it and
refresh from the live wiki:

```bash
wikitool db reset --yes
wikitool pull --full --all
wikitool knowledge warm --docs-profile remilia-mw-1.44 --docs-mode missing
```

## License

AGPL-3.0-only with supplementary terms in `LICENSE-SSL` and `LICENSE-VPL`.
