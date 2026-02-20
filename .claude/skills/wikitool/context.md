---
name: context
description: Run local context bundle lookups for page titles.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [title] [options]
---

# /wikitool context

Thin wrapper for:

```bash
wikitool context $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- context $ARGUMENTS
```

Validate flags via:

1. `wikitool context --help`
2. `docs/wikitool/reference.md`
