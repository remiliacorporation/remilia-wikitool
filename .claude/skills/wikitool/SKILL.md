---
name: wikitool
description: Run wikitool commands for wiki sync, search, export, documentation, linting, imports, and inspections.
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*), Read, Write
argument-hint: [command] [options]
---

# /wikitool - Wiki Management Tool

Execute wikitool commands directly. All commands run from the wikitool directory (auto-detects standalone vs embedded layouts).

## Usage

```
/wikitool                    # Show all available commands
/wikitool help               # Same as above
/wikitool help <command>     # Show detailed help for a command
/wikitool <command> [args]   # Run a command
```

**Canonical command reference:** `docs/wikitool/reference.md` (generated from CLI help).
**Local help:** `bun run wikitool help` and `bun run wikitool help <command>`.

## Examples

```bash
# Pull latest articles
/wikitool pull

# Check what changed
/wikitool diff

# Push with edit summary
/wikitool push -s "Fix broken links"

# Export external wiki page
/wikitool export "https://wowdev.wiki/M2" --subpages -o exports/M2/

# Export direct markdown file
/wikitool export "https://example.com/docs/format.md" -o format.md

# Search for content
/wikitool search "Milady Maker"

# Context bundle for AI
/wikitool context "Milady Maker" --json
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool $ARGUMENTS
```

**If no arguments provided** (or `help`), run `bun run wikitool` to show available commands.
**If `help <command>` provided**, run `bun run wikitool help <command>` for detailed command help.

## Safety Rules

1. **Always dry-run before push**: `push --dry-run -s "Summary"` first
2. **Never use --force** without explicit user confirmation
3. **Check diff** before pushing changes
4. **Confirm deletions** before using `push --delete`


