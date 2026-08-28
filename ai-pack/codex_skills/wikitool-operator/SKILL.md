---
name: wikitool-operator
description: Operate Wikitool for MediaWiki target configuration, capability and template discovery, local indexing, source capture, deterministic article checks and fixes, validation, revision-bound sync, diagnostics, and guarded writes. Use for mechanical or mixed wiki work; route prose judgment to the dedicated skills.
---

# Wikitool operator

Use Wikitool as the mechanical and evidence substrate for a configured MediaWiki project. Do not let convenience commands absorb editorial judgment that belongs in `wiki-writing`, `prose-review`, or `wiki-interview`.

## Procedure

1. Resolve the project root. Run `wikitool config show --format json` and inspect the configured target, adapter path, and bot edit-marking policy.
2. Inspect `wikitool status --modified --format json` and a scoped diff before changing local content. Preserve unrelated work.
3. Run `wikitool adapter inspect --format json` for project-owned policy. Inspect live capabilities with `wikitool wiki capabilities show --format json` and the derived authoring catalog with `wikitool catalog surface show --format json --view brief` when the task needs them. Read the adapter's source documents for target-specific work.
4. Check `wikitool catalog status --format json`. Use the narrowest warm/build operation needed; do not rebuild healthy state by reflex.
5. Use `article scout` for typed local context and integration facts. Its readiness describes retrieval artifacts, not research completeness or publishability.
6. Use `source wiki-search` only for the configured target wiki. Use `source fetch`, sessions, cache, and export to create bounded provenance for external sources. Access challenges require user help, not circumvention.
7. Route real prose authoring to `wiki-writing`, independent editorial review to `prose-review`, and human intake to `wiki-interview`.
8. Use `article lint` and `article fix --apply safe` only for deterministic diagnostics and mechanical edits. Inspect every fix and lint again.
9. Use scoped `validate --verify-live` when local state may be stale. Treat live read evidence as authoritative for redirects, missing pages, and target capabilities.
10. Before a push, inspect status and diff, run review, then preview an exact scope with `push --title ... --summary ...` (or deliberately select `--all`). Inspect the returned plan and retain its `plan_id`; publication requires a second invocation with the same scope, summary, policy flags, and `--apply PLAN_ID`. Wikitool rejects drift rather than silently replanning, uses revision constraints, and does not retry writes blindly. Global planning requires an explicit successful `pull --full --all`; scoped or legacy-migrated rows do not prove coverage. Durable sync identity is bound to the configured API endpoint, so preserve or move an old store aside and establish a fresh full/all baseline when deliberately changing targets.
11. Standalone delete also previews by default. Inspect its target, title, observed revision, reason, local effect, and `plan_id`, then apply only that exact plan with `delete ... --apply PLAN_ID`. If any write becomes ambiguous, never replay it. Use `mutation list`, `mutation show <edit|delete> ID`, and `mutation reconcile <edit|delete> ID`. When remote truth is permanently unavailable, `mutation close <edit|delete> ID --actor ... --reason ... --confirm` records an unresolved operator closure rather than a fabricated outcome, preserves its provenance, invalidates the title's baseline, and requires `pull --full --all` before another write; a present page with local drift may also require explicit `--overwrite-local`.

## macOS release trust

First verify the archive against the release's external `SHA256SUMS.txt`, then inspect
`macos-release-trust.json`, which must identify an `unsigned_github_release`. Explain that Wikitool
cannot repair quarantine before its first execution and that the checksum does not provide an
Apple identity. After the user approves those exact verified bytes, use `xattr -d
com.apple.quarantine` only on each exact executable path they intend to run: `wikitool`,
`contextmink/contextmink`, `papertiger/papertiger`, and, only if requested,
`papertiger/papertiger-mise`. Never use recursive quarantine removal on a download directory or
disable Gatekeeper globally. See `docs/wikitool/macos-gatekeeper.md`.

## Acceptance boundary

`article accept` writes a transactional authorization bound to the exact bytes, acceptance timestamp, normalized MediaWiki API endpoint, and full SHA-256 identity of the active site-adapter policy plus declared guidance. For a coherent multi-file review, use `article changeset prepare` to freeze every title/path/hash/origin/lint/authority item, then `article changeset accept` to commit one named decision and every independently invalidating member row in a single SQLite transaction. Any prose, target, or publication-policy change invalidates the authorization, and legacy JSON ledgers cannot authorize publication. The named editor is a self-reported, unauthenticated claim; Wikitool cannot prove who typed it or judge prose quality. A project workflow may require a real human review, but the tool must describe its assurance honestly. Inspect push-report acceptance provenance, and never put the claimed editor name into a public edit summary.

## Exit conditions

Report the exact scope, adapter used, live versus cached evidence, deterministic findings, changes made, and remaining authority or editorial gates. Do not claim prose quality from lint, research truth from retrieval rank, human identity from the ledger, or publication success from a preview.
