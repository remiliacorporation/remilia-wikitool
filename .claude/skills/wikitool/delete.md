---
name: delete
description: Run local delete workflow (prefer dry-run + reason).
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [title] --reason "..." [options]
---

# /wikitool delete

Thin wrapper for:

```bash
wikitool delete $ARGUMENTS
```

Fallback when `wikitool` is not on PATH:

```bash
cargo run --quiet --package wikitool -- delete $ARGUMENTS
```

Validate flags via:

1. `wikitool delete --help`
2. `docs/wikitool/reference.md`
