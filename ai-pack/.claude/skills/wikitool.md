# /wikitool - Thin wrapper

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or cached behavior; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

At the start of an editing session, inspect local edits and refresh local wiki state before relying on indexed content:
`wikitool status --modified --format json`, `wikitool diff --format json`,
`wikitool workflow session-refresh`, and
`wikitool knowledge status --docs-profile remilia-wiki --format json`. Use `wikitool workflow full-refresh`
only for deliberate rebuilds or missing sync state; do not use `pull --overwrite-local` without explicit approval.

Use `wikitool knowledge article-start "Topic" --intent new|expand|audit|refresh --format json --view brief` as the authoring front door.
For new articles and substantial expansions, route to `/knowledge-interview` when human context can
improve scope, terminology, chronology, relationships, or source leads, unless the user explicitly
opts out. Skip interview rounds for mechanical lint, link, sync, source-fetch, or validation work
unless a conflict requires user judgment.
Keep agent context compact: prefer wikitool briefs (`article-start --view brief`, `knowledge inspect chunks --view brief`, `templates show --view brief`, `wiki surface show --view brief`, `review --view brief`) before using `--view full`, broad reference selections, or high token budgets.
Use normal agent web search to choose arbitrary external sources, then use `wikitool research fetch`, `research discover`, and `export` for extraction and provenance. Use `research wiki-search` only for the configured target wiki API.
When `research fetch --output json` returns `error.challenge_handoffs`, relay the exact handoff to the user and ask them to solve the source challenge in a browser, then import source-issued cookies with `research session import ... --cookies -` and retry with `--refresh`. Do not use stealth clients, TLS impersonation, paid crawlers, or third-party reader services. Use `research session list|show|clear|prune` to manage local sessions; cookie values are stored locally and not printed by CLI output.
Use `wikitool research mediawiki-templates "URL"` when a source MediaWiki page's template/module contract matters. The report is cached; add `--refresh` when live freshness matters. Treat it as source-wiki context only; target-wiki template use still has to pass local `knowledge contracts`, `templates show`, and `article lint`.
Use `wikitool wiki profile remote "URL"` only as an explicitly scoped remote target capability probe when local import/profile data is unavailable. It reports extensions, parser tags, namespaces, and API capabilities, not portable template permission.
Use `wikitool knowledge inspect references ...` for indexed citation audits and duplicate cleanup passes.
Use scoped `wikitool validate --category ... --title ... --limit ...` when investigating a specific validation class. Use `--verify-live` for broken-link or redirect findings that need production API corroboration.
Use scoped `wikitool status`, `wikitool diff`, and `wikitool push --dry-run` selectors when working on a subset of pages.
Use `wikitool review --format json --view brief --summary "..."` for the pre-push gate; request `--view full` only when the brief points to a needed detail.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
