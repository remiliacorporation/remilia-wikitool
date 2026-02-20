---
name: pull
description: Pull latest content from remote wiki.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [options]
---

# /wikitool pull

Thin wrapper for:

```bash
wikitool pull $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- pull $ARGUMENTS
```

Validate flags via:

1. `wikitool pull --help`
2. `docs/wikitool/reference.md`
