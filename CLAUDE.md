# CLAUDE.md

Guidance for Claude Code when working with remilia-wikitool.

## Project Overview

`remilia-wikitool` is a Rust CLI for managing Remilia Wiki content locally:

1. pull wiki content to files
2. index/search/validate locally
3. push edits back with explicit dry-run and summary controls

Target wiki: `https://wiki.remilia.org`

## Runtime

- Rust stable toolchain
- Edition 2024
- Primary binary: `wikitool`

No migration path is provided during cutover. If local DB/runtime state is incompatible, delete `.wikitool/data/wikitool.db` and repull.

## Quick Start

```bash
cargo build --package wikitool --release
wikitool init --templates
wikitool pull --full --all
```

## Core Workflow

```bash
wikitool pull
# edit files in wiki_content/
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Edit summary"
wikitool push --summary "Edit summary"
```

## Repository Structure

- `crates/wikitool/` CLI entrypoint
- `crates/wikitool_core/` core runtime/sync/index/docs logic
- `docs/wikitool/` operator docs
- `llm_instructions/` writing guidance shipped in AI pack
- `ai/docs-bundle-v1.json` optional precomposed docs bundle for `docs import --bundle`

## Safety Rules

1. Run `push --dry-run` before write pushes.
2. Do not use `--force` unless explicitly requested.
3. Keep command docs aligned with actual CLI help.
4. Preserve no-migration policy unless explicitly changed.

## Help and Reference

- `wikitool --help`
- `wikitool <command> --help`
- `docs/wikitool/reference.md` (generated from current Rust CLI help)
