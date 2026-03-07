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
- Authoring retrieval uses semantic page profiles plus explicit template/link/reference/media signals, normalized source authorities, identifier rows, and template implementation bundles to narrow context for agents
- Authoring retrieval can also bridge a pinned MediaWiki docs corpus with local template/module usage so agents can compare upstream behavior with live wiki implementation patterns
- The DB does not assign opaque reference quality scores; it stores inspectable source metadata, authority/identifier matches, and retrieval signals so ranking stays transparent

## Cutover policy

Rust CLI is the primary runtime. Local DB state is disposable; operators are expected to delete/reset it and repull or resync when needed.

## Related docs

- `README.md` overview and operator entrypoint
- `SETUP.md` installation and day-one workflow
- `docs/wikitool/reference.md` canonical command/flag reference
