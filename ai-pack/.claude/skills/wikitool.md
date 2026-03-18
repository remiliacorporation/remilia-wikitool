# /wikitool - Thin wrapper

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or cached behavior; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Use `wikitool knowledge article-start "Topic" --format json` as the authoring front door.
Use `wikitool knowledge pack "Topic" --format json` only when you need the raw authoring substrate behind article-start.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
