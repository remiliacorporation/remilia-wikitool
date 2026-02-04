---
name: delete
description: Delete a page from the live wiki (requires authentication)
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: "<title>" --reason "Reason" [--dry-run]
---

# /wikitool delete - Delete Wiki Pages

Delete a page from wiki.remilia.org. Requires bot credentials.

## Safety

- Use `--dry-run` first.
- Provide a clear `--reason`.
- Avoid `--no-backup` unless explicitly requested.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool delete "Page Title" --reason "Duplicate page" --dry-run
/wikitool delete "Page Title" --reason "Duplicate page"
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool delete $ARGUMENTS
```
