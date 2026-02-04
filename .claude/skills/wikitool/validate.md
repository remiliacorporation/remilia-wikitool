---
name: validate
description: Validate wiki content for broken links, missing refs, style issues
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: [options]
---

# /wikitool validate - Validate Wiki Content

Check wiki content for common issues like broken links, missing references, and style problems.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool validate                    # Check all articles
/wikitool validate --report wikitool_exports/validation-report.md --format md
/wikitool validate --include-remote --format json
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool validate $ARGUMENTS
```


