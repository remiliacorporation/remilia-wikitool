---
name: status
description: Show sync status between local files and wiki
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: [options]
---

# /wikitool status - Show Sync Status

Display synchronization status between local files and wiki.remilia.org.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool status              # Article sync status
/wikitool status --templates  # Template sync status
/wikitool status --modified   # Only modified
/wikitool status --conflicts  # Only conflicts
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool status $ARGUMENTS
```


