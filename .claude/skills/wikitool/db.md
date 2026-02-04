---
name: db
description: Database operations - stats, sync, migrations
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: <subcommand> [options]
---

# /wikitool db - Database Operations

Manage the wikitool SQLite database for sync state and full-text search.

## Subcommands

| Subcommand | Description |
|------------|-------------|
| `stats` | Show database statistics (pages, size, indexes) |
| `sync` | Repair database/file sync state |
| `migrate` | Run database migrations |

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool db stats              # Show statistics
/wikitool db sync               # Repair sync state
/wikitool db migrate --validate # Validate schema
/wikitool db migrate            # Run pending migrations
/wikitool db migrate --status   # Check migration status
```

## Database Location

The SQLite database is stored at:
```
data/wikitool.db
```

This directory is gitignored. Override with `WIKITOOL_DB` if needed.

## Troubleshooting

If the database becomes corrupted or out of sync:

```bash
# Option 1: Reinitialize
bun run wikitool init
bun run wikitool pull --full

# Option 2: Full refresh script (from repo root)
# Windows
scripts/wikitool-full-refresh.ps1

# macOS / Linux
scripts/wikitool-full-refresh.sh
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool db $ARGUMENTS
```
