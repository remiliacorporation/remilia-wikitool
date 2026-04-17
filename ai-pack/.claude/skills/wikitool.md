# /wikitool - Thin wrapper

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or cached behavior; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Use `wikitool knowledge article-start "Topic" --format json` as the authoring front door.
Use `wikitool knowledge pack "Topic" --format json` only when you need the raw authoring substrate behind article-start.
Use `wikitool research mediawiki-templates "URL"` when a source MediaWiki page's template/module contract matters. Treat it as source-wiki context only; target-wiki template use still has to pass local template lookup and article lint.
Use `wikitool knowledge inspect references ...` for indexed citation audits and duplicate cleanup passes.
Use `wikitool status --modified|--conflicts` and scoped `wikitool push --dry-run --title ...` for targeted sync review.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
