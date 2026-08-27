---
name: wikitool-operator
description: Operate Wikitool for MediaWiki target configuration, capability and template discovery, local indexing, research capture, deterministic article checks and fixes, validation, revision-bound sync, diagnostics, and guarded writes. Use for mechanical or mixed wiki work; route prose judgment to the dedicated skills.
---

# Wikitool operator

Use Wikitool as the mechanical and evidence substrate for a configured MediaWiki project. Do not let convenience commands absorb editorial judgment that belongs in `wiki-writing`, `prose-review`, or `wiki-interview`.

## Procedure

1. Resolve the project root. Run `wikitool config show --format json` and inspect the configured target, adapter path, and bot edit-marking policy.
2. Inspect `wikitool status --modified --format json` and a scoped diff before changing local content. Preserve unrelated work.
3. Run `wikitool wiki profile show --format json` to inspect the generic base, explicit site adapter, cached live capabilities, and template catalog. Read the adapter's source documents for target-specific work.
4. Check `wikitool knowledge status --format json`. Use the narrowest warm/build operation needed; do not rebuild healthy state by reflex.
5. Use `knowledge article-start` for typed local evidence and integration facts. Its readiness describes retrieval artifacts, not research completeness or publishability.
6. Use `research wiki-search` only for the configured target wiki. Use `research fetch`, sessions, cache, and export to create bounded provenance for external sources. Access challenges require user help, not circumvention.
7. Route real prose authoring to `wiki-writing`, independent editorial review to `prose-review`, and human intake to `wiki-interview`.
8. Use `article lint` and `article fix --apply safe` only for deterministic diagnostics and mechanical edits. Inspect every fix and lint again.
9. Use scoped `validate --verify-live` when local state may be stale. Treat live read evidence as authoritative for redirects, missing pages, and target capabilities.
10. Before a push, inspect status and diff, run review, and verify `push --dry-run`. Wikitool uses revision constraints and does not retry writes blindly.

## Acceptance boundary

`article accept` writes a hash-bound local ledger entry. Any prose edit invalidates it. The named editor is a self-reported, unauthenticated claim; Wikitool cannot prove who typed it or judge prose quality. A project workflow may require a real human review, but the tool must describe its assurance honestly.

## Exit conditions

Report the exact scope, adapter/profile used, live versus cached evidence, deterministic findings, changes made, and remaining authority or editorial gates. Do not claim prose quality from lint, research truth from retrieval rank, human identity from the ledger, or publication success from a dry run.
