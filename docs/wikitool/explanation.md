# Wikitool Explanation

This section explains what wikitool is, why it exists, and how it fits into the repo.

## What it is

Wikitool is a Bun-powered TypeScript CLI that synchronizes MediaWiki content with local files and provides tooling for search, validation, imports, and inspections.

## Why it exists

- **Consistency:** Local edits track exact wiki state and reduce manual mistakes.
- **Speed:** Local full-text search and structured context are faster than remote queries.
- **Safety:** Diff, dry-run, and validation steps help prevent accidental pushes.
- **Automation:** Imports and validation reduce repetitive editing work.

## How it works (high level)

- Uses the MediaWiki API to pull/push content.
- Stores sync state, links, and search indexes in a local SQLite database.
- Generates derived data (sections, template usage, infobox metadata) for context.

## Where to learn more

- `<wikitool-dir>/README.md` - architecture, database schema, and dev workflows
- `docs/wikitool/reference.md` - canonical command/flag reference
- `SETUP.md` - setup guide for editors and agents
