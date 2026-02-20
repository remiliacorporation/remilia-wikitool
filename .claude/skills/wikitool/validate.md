---
name: validate
description: Run validation checks for links/redirects/categories/orphans.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [options]
---

# /wikitool validate

Thin wrapper for:

```bash
wikitool validate $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- validate $ARGUMENTS
```

Validate flags via:

1. `wikitool validate --help`
2. `docs/wikitool/reference.md`
