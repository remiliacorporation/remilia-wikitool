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

## 5) Optional: configure write credentials

Create `.env` in project root and set:

```bash
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

Bot password setup:

1. Open `https://wiki.remilia.org/Special:BotPasswords`
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
scripts/generate-wikitool-reference.ps1   # Windows
scripts/generate-wikitool-reference.sh    # macOS/Linux
```

Release AI pack includes setup/docs/instructions outside the binary. If `ai/docs-bundle-v1.json` is present, import it with:

```bash
wikitool docs import --bundle ./ai/docs-bundle-v1.json
```

## 8) Troubleshooting

`db migrate` is intentionally unsupported during cutover.

If runtime/schema changes break local state:

1. Delete `.wikitool/data/wikitool.db`
2. Run `wikitool pull --full --all`

If push fails with auth errors, verify `WIKI_BOT_USER` and `WIKI_BOT_PASS` in project root `.env`.
