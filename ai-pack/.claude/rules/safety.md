# Wikitool safety

Treat local files as the editing workspace. Read-only MediaWiki API calls are safe when they stay within the configured target. Perform live writes only through Wikitool and only when the user requested them.

Before a push, inspect scoped status and diff, run review, and verify `push --dry-run`. Never use overwrite, force, or delete flags without clear authority and exact scope.

The acceptance ledger binds a self-reported editor claim and decision to exact content. It does not authenticate identity and is not a prose-quality oracle.
