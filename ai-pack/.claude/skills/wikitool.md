# /wikitool - Thin wrapper

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or cached behavior; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

At the start of an editing session, refresh local wiki state before relying on indexed content:
`wikitool status --modified --format json`, `wikitool diff --format json`,
`wikitool pull --all --format json`, `wikitool knowledge warm --docs-profile remilia-mw-1.44 --format json`, and
`wikitool wiki profile sync --format json`. Use `pull --full --all` only for deliberate rebuilds
or missing sync state; do not use `--overwrite-local` without explicit approval.

Use `wikitool knowledge article-start "Topic" --intent new|expand|audit|refresh --format json` as the authoring front door.
Use `wikitool knowledge pack "Topic" --format json` only when you need the raw authoring substrate behind article-start.
Use `wikitool research mediawiki-templates "URL"` when a source MediaWiki page's template/module contract matters. The report is cached; add `--refresh` when live freshness matters. Treat it as source-wiki context only; target-wiki template use still has to pass local `knowledge contracts`, `templates show`, and `article lint`.
Use `wikitool wiki profile remote "URL"` only as an explicitly scoped remote target capability probe when local import/profile data is unavailable. It reports extensions, parser tags, namespaces, and API capabilities, not portable template permission.
Use `wikitool knowledge inspect references ...` for indexed citation audits and duplicate cleanup passes.
Use scoped `wikitool validate --category ... --title ... --limit ...` when investigating a specific validation class. Use `--verify-live` for broken-link or redirect findings that need production API corroboration.
Use scoped `wikitool status`, `wikitool diff`, and `wikitool push --dry-run` selectors when working on a subset of pages.
Use `wikitool review --format json --summary "..."` for the full pre-push gate.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
