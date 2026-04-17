# /wikitool - Thin wrapper

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or cached behavior; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Use `wikitool knowledge article-start "Topic" --intent new|expand|audit|refresh --format json` as the authoring front door.
Use `wikitool knowledge pack "Topic" --format json` only when you need the raw authoring substrate behind article-start.
Use `wikitool research mediawiki-templates "URL"` when a source MediaWiki page's template/module contract matters. The report is cached; add `--refresh` when live freshness matters. Treat it as source-wiki context only; target-wiki template use still has to pass local `knowledge contracts`, `templates show`, and `article lint`.
Use `wikitool wiki profile remote "URL"` only as an explicitly scoped remote target capability probe when local import/profile data is unavailable. It reports extensions, parser tags, namespaces, and API capabilities, not portable template permission.
Use `wikitool knowledge inspect references ...` for indexed citation audits and duplicate cleanup passes.
Use scoped `wikitool validate --category ... --title ... --limit ...` when investigating a specific validation class. Use `--verify-live` for broken-link or redirect findings that need production API corroboration.
Use scoped `wikitool status`, `wikitool diff`, and `wikitool push --dry-run` selectors when working on a subset of pages.
Use `wikitool review --format json --summary "..."` for the full pre-push gate.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
