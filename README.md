# Remilia Wikitool

Unified MediaWiki tooling for Remilia Wiki.

Primary runtime is a Rust CLI binary (`wikitool`) on stable Rust (edition 2024).

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

Source files for release AI companion packaging live under `ai-pack/`.

CI publishes zipped release artifacts (`wikitool-release-<target>`) where each zip unpacks into `wikitool-<target>/` with the binary and AI companion files in one folder.

Maintainer command for multi-target bundles:

```bash
wikitool release build-matrix
```

By default this emits versioned bundle names, for example:

1. `wikitool-v0.1.0-x86_64-unknown-linux-gnu.zip`
2. `wikitool-v0.1.0-x86_64-pc-windows-msvc.zip`

For CI matrix jobs, package one target explicitly:

```bash
wikitool release build-matrix --targets x86_64-unknown-linux-gnu --unversioned-names
```

Manual multi-OS artifact builds are also available via GitHub Actions:

1. Run workflow: `.github/workflows/release-artifacts.yml`
2. Provide `artifact_version` (for example `v0.2.0`)
3. Download separate artifacts for:
   - `x86_64-pc-windows-msvc`
   - `x86_64-unknown-linux-gnu`
   - `x86_64-apple-darwin`

Release folder contents:

1. `AGENTS.md`, `CLAUDE.md`, `SETUP.md`, `README.md`
2. `.claude/rules/*`, `.claude/skills/*` (baseline ai-pack guidance)
3. `llm_instructions/*.md`
4. `docs/wikitool/*.md`
5. `codex_skills/*` installable Codex skill bundle
6. optional `ai/docs-bundle-v1.json`
7. optional host overlay extras when `--host-project-root` is provided:
   - host `CLAUDE.md` (mirrored to `AGENTS.md`)
   - `WIKITOOL_CLAUDE.md` preserving wikitool-local guidance
   - host `.claude/{rules,skills}` merged over baseline

This content is intentionally shipped outside the binary.

Use bundle import to preload docs offline:

```bash
wikitool docs import --bundle ./ai/docs-bundle-v1.json
```

By default, release bundles stay wikitool-generic while still including the ai-pack `.claude` baseline.
If `--host-project-root` is provided, host context is layered on top and wikitool-local guidance is preserved as `WIKITOOL_CLAUDE.md`.

## Documentation

- `SETUP.md` setup guide
- `docs/wikitool/how-to.md` task recipes
- `docs/wikitool/reference.md` command reference generated from Rust CLI help
- `docs/wikitool/explanation.md` architecture notes
- `VERSIONING.md` version bump policy and release checklist
- `RELEASE_LOG.md` release history

Regenerate reference docs:

```bash
wikitool docs generate-reference
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
