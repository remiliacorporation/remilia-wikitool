# Wikitool Setup Guide

This guide gets a fresh clone ready for `wikitool`.

Read-only workflows do not require credentials. Push/delete writes require bot credentials.

## 1) Get the binary

Option A: build from source:

```bash
cargo build --package wikitool --release
```

Option B: download a release artifact for your OS.

## 2) Initialize runtime

From the project root (or pass `--project-root`):

```bash
wikitool init --templates
```

This materializes `.wikitool/` runtime state.

## 3) Pull content

```bash
wikitool pull --full --all
```

Incremental pull examples:

```bash
wikitool pull
wikitool pull --templates
wikitool pull --categories
```

## 4) Verify install

```bash
wikitool status
wikitool index stats
```

## 5) Optional: configure credentials and API target

Create `.env` in project root (next to `wiki_content/`) and set:

```bash
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

If your API URL is not set in `.wikitool/config.toml`:

```bash
WIKI_API_URL=https://your-wiki.example.org/api.php
```

Bot password setup:

1. Open `https://<your-wiki>/Special:BotPasswords`
2. Create a bot password with edit grants
3. Copy generated username/password into `.env`

## 6) Common workflow

```bash
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Summary"
wikitool push --summary "Summary"
```

## 7) Docs and AI pack

Canonical command docs:

- `docs/wikitool/reference.md`
- regenerate with:

```bash
wikitool docs generate-reference
```

AI companion source assets are maintained under `ai-pack/` in this repository.

Release AI pack includes setup/docs/instructions outside the binary. If `ai/docs-bundle-v1.json` is present, import it with:

```bash
wikitool docs import --bundle ./ai/docs-bundle-v1.json
```

Release assembly commands:

```bash
wikitool release package
wikitool release build-matrix
```

`release build-matrix` uses versioned artifact names by default (`wikitool-vX.Y.Z-<target>.zip`).
For ephemeral CI-style output names, add `--unversioned-names`.

By default, release output is wikitool-generic and includes ai-pack `.claude/rules` and `.claude/skills`.
To layer host `.claude/rules`, host `.claude/skills`, and host `CLAUDE.md` on top, pass `--host-project-root <PATH>`.
When host overlay is used, wikitool-local guidance is preserved as `WIKITOOL_CLAUDE.md`.

Codex skill templates are also included under `codex_skills/` and can be copied into `$CODEX_HOME/skills`.

## 8) Troubleshooting

Schema migrations run automatically on startup. To run manually: `wikitool db migrate`.

If runtime/schema changes break local state:

1. Delete `.wikitool/data/wikitool.db`
2. Run `wikitool pull --full --all`

If push fails with auth errors, verify `WIKI_BOT_USER` and `WIKI_BOT_PASS` in project root `.env`.
