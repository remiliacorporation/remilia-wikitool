---
name: docs
description: Run docs import/list/search/update/remove/reference workflows.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: <subcommand> [options]
---

# /wikitool docs

Thin wrapper for:

```bash
wikitool docs $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- docs $ARGUMENTS
```

Validate flags via:

1. `wikitool docs --help`
2. `docs/wikitool/reference.md`
