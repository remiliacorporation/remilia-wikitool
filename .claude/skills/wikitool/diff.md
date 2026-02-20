---
name: diff
description: Inspect local vs indexed changes before push.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [options]
---

# /wikitool diff

Thin wrapper for:

```bash
wikitool diff $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- diff $ARGUMENTS
```

Validate flags via:

1. `wikitool diff --help`
2. `docs/wikitool/reference.md`
