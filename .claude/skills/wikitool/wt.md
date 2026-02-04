---
name: wt
description: Shorthand alias for /wikitool.
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*), Read, Write
user_invocable: true
---

<command-name>wt</command-name>

# /wt - Shorthand Alias

Shorthand alias for `/wikitool`. Pass arguments directly.

Wikitool auto-detects standalone vs embedded mode; run from the wikitool directory.

## Examples

- `/wt pull --full` is equivalent to `/wikitool pull --full`
- `/wt status` is equivalent to `/wikitool status`
- `/wt diff` is equivalent to `/wikitool diff`

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool $ARGUMENTS
```
