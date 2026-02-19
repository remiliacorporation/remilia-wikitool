# Remilia Wikitool

Unified MediaWiki tooling for Remilia Wiki.

Primary runtime is now a Rust CLI binary (`wikitool`) on stable Rust (edition 2024). Bun/TypeScript sources remain only for legacy parity tooling and are not the default operator path.

## Quick Start

From this repository:

```bash
cargo build --package wikitool --release
./target/release/wikitool init --templates
./target/release/wikitool pull --full --all
```

From a release package:

```bash
wikitool init --templates
wikitool pull --full --all
```

## Runtime Layout

Wikitool resolves paths from project root and uses `.wikitool/` for local runtime state.

- `wiki_content/` local page files
- `templates/` local template/module files
- `.wikitool/config.toml` runtime config
- `.wikitool/data/wikitool.db` local index/sync database

No migration path is provided during this cutover. For incompatible binary/schema changes, delete the local DB and repull.

## Core Workflow

```bash
wikitool init --templates
wikitool pull --full --all
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Edit summary"
wikitool push --summary "Edit summary"
```

## Namespaces

By default pull/push operate on Main namespace. Use flags for others:

- `--categories` for `Category:`
- `--templates` for Template/Module/MediaWiki namespaces
- `--all` (pull) for all supported namespaces

## AI Companion Pack

CI publishes unzip-ready release artifacts (`wikitool-release-<OS>`) where the binary and AI companion files are in one folder.

Release folder contents:

1. `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`
2. `llm_instructions/*.md`
3. `docs/wikitool/*.md`
4. optional `ai/docs-bundle-v1.json`

This content is intentionally shipped outside the binary.

Use bundle import to preload docs offline:

```bash
wikitool docs import --bundle ./ai/docs-bundle-v1.json
```

## Documentation

- `SETUP.md` setup guide
- `docs/wikitool/how-to.md` task recipes
- `docs/wikitool/reference.md` command reference generated from Rust CLI help
- `docs/wikitool/explanation.md` architecture notes

Regenerate reference docs:

```bash
scripts/generate-wikitool-reference.ps1   # Windows
scripts/generate-wikitool-reference.sh    # macOS/Linux
```

## Environment

Push/delete writes need bot credentials:

```bash
WIKI_BOT_USER=Username@BotName
WIKI_BOT_PASS=your-bot-password
```

Useful overrides:

```bash
WIKI_API_URL=https://wiki.remilia.org/api.php
WIKI_HTTP_TIMEOUT_MS=30000
WIKI_HTTP_RETRIES=2
WIKI_HTTP_WRITE_RETRIES=1
WIKI_HTTP_RETRY_DELAY_MS=500
```

## License

This project is licensed under the Viral Public License + Source Seppuku License.
