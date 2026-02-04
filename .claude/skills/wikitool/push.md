---
name: push
description: Upload local changes to the live wiki (always dry-run first!)
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: -s "Edit summary" [options]
---

# /wikitool push - Upload Wiki Content

Push local changes to wiki.remilia.org.

## Safety Rules

1. **ALWAYS run `--dry-run` first** to preview changes
2. **NEVER use `--force`** without explicit user confirmation
3. **Check diff** before pushing: `/wikitool diff`
4. **Confirm deletions** before using `--delete`

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Standard Workflow

```bash
# 1. Check what changed
/wikitool diff

# 2. Preview the push
/wikitool push --dry-run -s "Fix typos"

# 3. If dry-run looks good, push for real
/wikitool push -s "Fix typos"
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool push $ARGUMENTS
```


