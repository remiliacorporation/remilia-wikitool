---
name: docs
description: Import and search MediaWiki extension documentation
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*), Read
argument-hint: <subcommand> [args]
---

# /wikitool docs - Documentation Management

Import and manage MediaWiki extension documentation locally.

## Reference

See `docs/wikitool/reference.md` for full subcommands and flags.

## Examples

```bash
/wikitool docs import Extension:Cargo       # Import Cargo extension docs
/wikitool docs import Extension:CirrusSearch
/wikitool docs search "cargo table"         # Search imported docs
/wikitool docs list                         # List what's imported
/wikitool docs import-technical Manual:Hooks --subpages
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>
bun run wikitool docs $ARGUMENTS
```


