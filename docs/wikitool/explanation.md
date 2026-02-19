# Wikitool Explanation

This section explains what wikitool is and how it fits into the repo.

## What it is

Wikitool is a Rust CLI that synchronizes MediaWiki content with local files and provides local indexing/search, validation, docs ingestion, and inspection utilities.

## Why it exists

- Consistency: local edits are tied to explicit sync state
- Speed: local context/search avoids repeated network lookups
- Safety: diff, dry-run, and validation reduce risky pushes
- Automation: docs import, cargo import, lint/inspect workflows reduce repetitive work

## How it works

- Pull/push use MediaWiki API read/write flows
- Local sync/index/docs state is stored in SQLite under `.wikitool/data/wikitool.db`
- Runtime state/config is kept under `.wikitool/`
- Delete and push flows include explicit write safeguards and diagnostics

## Cutover policy

Rust CLI is the primary runtime. No migration path is provided during current cutover; operators are expected to delete local DB state and repull when needed.

## Related docs

- `README.md` overview and operator entrypoint
- `SETUP.md` installation and day-one workflow
- `docs/wikitool/reference.md` canonical command/flag reference
