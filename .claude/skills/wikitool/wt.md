---
name: wt
description: Pass-through runner for arbitrary wikitool commands with guardrails.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: <command> [options]
---

# /wikitool wt

Pass-through entrypoint.

Preferred:

```bash
wikitool $ARGUMENTS
```

Fallback:

```bash
cargo run --quiet --package wikitool -- $ARGUMENTS
```

Guardrail:

1. For write flows, run `wikitool push --dry-run --summary "..."` before write push.
