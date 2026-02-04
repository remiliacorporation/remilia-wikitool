---
name: index
description: Link graph operations - rebuild indexes, find backlinks, orphans
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: <subcommand> [args]
---

# /wikitool index - Link Graph Operations

Manage the link graph index for analyzing page relationships.

## Reference

See `docs/wikitool/reference.md` for full subcommands and flags.

## Examples

```bash
/wikitool index rebuild              # Rebuild indexes
/wikitool index stats                # Show statistics
/wikitool index backlinks "Milady"   # Find links to Milady page
/wikitool index orphans              # Find unlinked pages
/wikitool index prune-categories     # List empty categories
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool index $ARGUMENTS
```


