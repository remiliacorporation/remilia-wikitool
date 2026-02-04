---
name: diff
description: Show local changes compared to the live wiki
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: [options]
---

# /wikitool diff - Show Local Changes

Display differences between local files and wiki.remilia.org.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool diff                    # Show all article changes
/wikitool diff --templates        # Show template changes
/wikitool diff --verbose          # Include hash/timestamp details
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool diff $ARGUMENTS
```


