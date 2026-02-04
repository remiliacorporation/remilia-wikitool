---
name: import
description: Import data to Cargo tables from CSV/JSON files
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: cargo <file> --table <Name> [options]
---

# /wikitool import - Cargo Data Import

Import structured data from CSV or JSON files into Cargo tables.

## Subcommands

| Subcommand | Description |
|------------|-------------|
| `cargo <file>` | Import from CSV/JSON to a Cargo table |

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
# Preview import (dry run)
/wikitool import cargo data.csv --table=Projects

# Write pages locally
/wikitool import cargo data.csv --table=Projects --write

# Import with template wrapper
/wikitool import cargo data.csv --table=Projects --write --template="Project infobox"

# Import from JSON
/wikitool import cargo data.json --table=Events --write
```

## File Format

**CSV format:**
```csv
name,date,location
Event 1,2024-01-15,New York
Event 2,2024-02-20,Los Angeles
```

**JSON format:**
```json
[
  {"name": "Event 1", "date": "2024-01-15", "location": "New York"},
  {"name": "Event 2", "date": "2024-02-20", "location": "Los Angeles"}
]
```

## Workflow

1. **Preview** - Run without `--write` to see what pages will be created
2. **Write locally** - Add `--write` to generate `.wiki` files
3. **Review** - Check generated files in `wiki_content/`
4. **Push** - Use `/wikitool push -s "Import summary"` to upload

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool import $ARGUMENTS
```
