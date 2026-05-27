---
name: wikitool-operator
description: Thin wrapper for operating the wikitool CLI with canonical help/reference alignment.
---

# Skill: wikitool-operator

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or workflow details; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

At the start of an editing session, inspect local edits and refresh local wiki state before relying on indexed content:
`wikitool status --modified --format json`, `wikitool diff --format json`,
`wikitool workflow session-refresh`, and
`wikitool knowledge status --docs-profile remilia-mw-1.44 --format json`. Use `wikitool workflow full-refresh`
only for deliberate rebuilds or missing sync state; do not use `pull --overwrite-local` without explicit approval.

Use `knowledge article-start --intent new|expand|audit|refresh --view brief` as the authoring front door.
Use `knowledge pack` only when the raw authoring substrate is needed behind article-start.
Keep agent context compact: prefer wikitool briefs (`article-start --view brief`, `knowledge inspect chunks --view brief`, `templates show --view brief`, `wiki surface show --view brief`, `review --view brief`) before using `knowledge pack --payload full`, `--view full`, broad reference selections, or high token budgets.
Use normal agent web search to choose arbitrary external sources, then use `research fetch`, `research discover`, and `export` for extraction and provenance. Use `research wiki-search` only for the configured target wiki API.
When `research fetch --output json` returns `error.challenge_handoffs`, relay the exact handoff to the user and ask them to solve the source challenge in a browser, then import source-issued cookies with `research session import ... --cookies -` and retry with `--refresh`. Do not use stealth clients, TLS impersonation, paid crawlers, or third-party reader services. Use `research session list|show|clear|prune` to manage local sessions; cookie values are stored locally and not printed by CLI output.
Use `research mediawiki-templates URL` when a source MediaWiki page's own template/module contract matters, especially for arbitrary wikis such as Wikipedia. The report is cached; add `--refresh` when live freshness matters. Treat that output as source-wiki context only; target-wiki template use still has to pass local `knowledge contracts`, `templates show`, and `article lint`.
Use `wiki profile remote URL` only for an explicitly scoped remote target capability probe when local import/profile data is unavailable. It reports extensions, parser tags, namespaces, and API capabilities, not portable template permission.
Use `knowledge inspect references` for indexed citation audits and duplicate cleanup prep.
Use scoped `validate --category ... --title ... --limit ...` when investigating a specific validation class. Use `--verify-live` for broken-link or redirect findings that need production API corroboration.
Use scoped `status`, `diff`, and `push --dry-run` selectors when working on a subset of pages.
Use `article lint .wikitool/drafts/Title.wiki --title "Title"`, `article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe`, and `article promote .wikitool/drafts/Title.wiki --title "Title"` for direct draft iteration before push review.
Use `review --format json --view brief --summary "..."` for the pre-push gate; request `--view full` only when the brief points to a needed detail.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
