# Wikitool safety

Treat local files as the editing workspace. Read-only MediaWiki API calls are safe when they stay within the configured target. Perform live writes only through Wikitool and only when the user requested them.

Before a push, inspect scoped status and diff, run review, preview the exact scope, and inspect the returned plan ID. Apply only that same scope and policy with `push --apply PLAN_ID`. Never use force, delete, or `--all` without clear authority and exact scope.

Standalone delete also previews by default and applies only an exact plan ID. Never replay an ambiguous write: inspect and reconcile its durable mutation receipt. An operator closure may record unresolved remote truth without claiming an outcome, but it invalidates that title's baseline and requires `pull --full --all` before another write.

The acceptance ledger binds a self-reported editor claim and decision to exact content. It does not authenticate identity and is not a prose-quality oracle.
