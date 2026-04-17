---
name: wikitool-operator
description: Thin wrapper for operating the wikitool CLI with canonical help/reference alignment.
---

# Skill: wikitool-operator

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or workflow details; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

At the start of an editing session, refresh local wiki state before relying on indexed content:
`wikitool status --modified --format json`, `wikitool diff --format json`,
`wikitool pull --all --format json`, `wikitool knowledge warm --docs-profile remilia-mw-1.44 --format json`, and
`wikitool wiki profile sync --format json`. Use `pull --full --all` only for deliberate rebuilds
or missing sync state; do not use `--overwrite-local` without explicit approval.

Use `knowledge article-start --intent new|expand|audit|refresh` as the authoring front door.
Use `knowledge pack` only when the raw authoring substrate is needed behind article-start.
Use `research mediawiki-templates URL` when a source MediaWiki page's own template/module contract matters, especially for arbitrary wikis such as Wikipedia. The report is cached; add `--refresh` when live freshness matters. Treat that output as source-wiki context only; target-wiki template use still has to pass local `knowledge contracts`, `templates show`, and `article lint`.
Use `wiki profile remote URL` only for an explicitly scoped remote target capability probe when local import/profile data is unavailable. It reports extensions, parser tags, namespaces, and API capabilities, not portable template permission.
Use `knowledge inspect references` for indexed citation audits and duplicate cleanup prep.
Use scoped `validate --category ... --title ... --limit ...` when investigating a specific validation class. Use `--verify-live` for broken-link or redirect findings that need production API corroboration.
Use scoped `status`, `diff`, and `push --dry-run` selectors when working on a subset of pages.
Use `review --format json --summary "..."` for the full pre-push gate.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
