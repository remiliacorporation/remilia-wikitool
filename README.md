# Wikitool

A single-binary CLI for working on a MediaWiki wiki: pull and push content, search and
retrieve context, research sources, lint and validate articles, and sync changes back to the
live site. It keeps a local SQLite index so an AI agent — Claude, Codex, or another — can read
the wiki, draft an article, and check it before pushing.

Local article files stay plain wikitext you can edit by hand. The database is derived and
disposable. Each release ships the agent guidance files alongside the binary, so an agent run
from the unpacked folder already knows how to drive the tool.

## Install

Download the release zip for your platform and unpack it. Everything sits at the top level:

```text
wikitool(.exe)        the binary
README.md             this file
CLAUDE.md AGENTS.md   agent routing brief (identical content)
.claude/              Claude skills and rules
codex_skills/         Codex skill equivalents
writing_context/      article-writing rules and style profile
docs/wikitool/        operator manual and generated command reference
contextmink/          bundled contextmink transcript guard (separate binary,
                      templates, and setup docs; Windows bundles also carry
                      contextmink-bridge.exe, a PowerShell -> Git Bash bridge)
manifest.json LICENSE*
```

Put `wikitool` on your `PATH`, or run it from the unpacked folder.
For GitHub releases, verify the downloaded zip against the published `SHA256SUMS.txt`
before unpacking when you need a reproducible provenance check.

## First run

Run one command. By default, `wikitool init` materializes Remilia Wiki as the target:

```bash
wikitool workflow session-refresh
```

For another MediaWiki target, set project config with `wikitool init --wiki-url ... --api-url ...`
or use temporary `WIKITOOL_*` environment overrides:

```bash
WIKITOOL_WIKI_URL=https://your-wiki.example.org/
WIKITOOL_WIKI_API_URL=https://your-wiki.example.org/api.php
# only needed to push or delete:
WIKITOOL_BOT_USER=Username@BotName
WIKITOOL_BOT_PASS=your-bot-password
```

This creates the runtime layout, pulls content, builds the knowledge index, and syncs the
wiki's capability profile. Run it again at the start of any session to refresh state; use
`wikitool workflow full-refresh` to rebuild from scratch. Read-only work needs no credentials.

## Using it with an agent

Run `claude` or `codex` from the unpacked folder. The bundled `CLAUDE.md` / `AGENTS.md` and the
`.claude/` and `codex_skills/` directories tell the agent which commands to use and in what
order. The `/wikitool` skill drives retrieval, authoring, and sync; `/review` gates content
before a push.

## What it does

- **Author** — `knowledge article-start "Topic" --view brief` returns an interpreted brief:
  section skeleton from comparable pages, applicable templates and categories, and where the
  evidence is thin.
- **Research** — `research wiki-search` queries the wiki API; `research fetch` pulls a URL with
  structured metadata; `research archive` captures a site to disk; `research session` imports
  cookies for session-gated sources.
- **Inspect** — `templates show`, `knowledge inspect chunks/references`, and `wiki surface`
  expose the target wiki's templates, content, and capabilities.
- **Check** — `article lint`/`fix`, `validate`, `module lint`, and `review` catch structural,
  citation, and link problems before a push.
- **Sync** — `pull`, `status`, `diff`, and `push --dry-run` keep local files aligned with the
  live wiki. Push detects conflicts against the remote and previews every change first.

Every command has `--help`, and `docs/wikitool/reference.md` is the full generated reference.

Release bundles also include a `contextmink/` pack. From the project or agent
working directory, run `wikitool contextmink install`; the installer writes
relative to the current directory unless `--project-root <dir>` is explicit.
When `wikitool` is source-built instead of run from an unpacked release bundle,
the installer falls back to the vendored `vendor/contextmink` source checkout
and builds its binaries only for a real install. Use `--from <dir>` to point at
an explicit release pack or contextmink source checkout.

## Documentation

| File | Role |
|---|---|
| `docs/wikitool/guide.md` | Operator manual |
| `docs/wikitool/reference.md` | Generated command reference |
| `docs/wikitool/architecture.md` | Internals and the agent token contract |
| `writing_context/` | Article structure, style rules, and writing guide |
| `CLAUDE.md` / `AGENTS.md` | Agent routing brief |
| `VERSIONING.md` / `CHANGELOG.md` | Release process and history |

## Runtime state

```text
project-root/
  .env
  .wikitool/config.toml
  .wikitool/data/wikitool.db    derived index — safe to delete
  wiki_content/                 article wikitext
  templates/                    template and module sources
```

Copy the bundled `.env.template` to `.env` and set `WIKITOOL_BOT_USER` / `WIKITOOL_BOT_PASS`
(from `Special:BotPasswords`) before `push` or `delete`. The project-root `.env` is loaded
automatically.

If the local database is stale or incompatible, reset and rebuild:

```bash
wikitool db reset --yes
wikitool workflow full-refresh
```

## Build from source

```bash
cargo build --package wikitool --release
```

Normal builds produce the end-user binary. Maintainer commands (release packaging, reference
generation, docs audit) live behind a feature flag:

```bash
cargo build --package wikitool --release --features maintainer
```

## License

AGPL-3.0-only, with supplementary terms in `LICENSE-SSL` and `LICENSE-VPL`.
