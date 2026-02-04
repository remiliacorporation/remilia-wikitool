---
name: lint
description: Lint Lua modules with Selene for code quality
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: [title] [options]
---

# /wikitool lint - Lua Module Linting

Lint Lua modules using Selene for code quality and style checks.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Examples

```bash
/wikitool lint                           # Lint all modules
/wikitool lint "Module:Infobox"          # Lint specific module
/wikitool lint --format json             # JSON output
/wikitool lint --strict                  # Strict mode
```

## Prerequisites

Selene must be installed. Run from repo root:

```bash
# Windows
scripts/setup-selene.ps1

# macOS / Linux
scripts/setup-selene.sh
```

Or run the full setup:

```bash
# Windows
scripts/setup-tools.ps1

# macOS / Linux
scripts/setup-tools.sh
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool lint $ARGUMENTS
```
