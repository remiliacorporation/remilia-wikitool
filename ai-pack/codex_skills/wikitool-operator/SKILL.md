---
name: wikitool-operator
description: Thin wrapper for operating the wikitool CLI with canonical help/reference alignment.
---

# Skill: wikitool-operator

Thin wrapper for the `wikitool` CLI.

Use normal reasoning, ordinary shell/file tools, and direct editing by default.
Do not invent flags or workflow details; verify against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Use `knowledge article-start` as the authoring front door.
Use `knowledge pack` only when the raw authoring substrate is needed behind article-start.
Use `knowledge inspect references` for indexed citation audits and duplicate cleanup prep.
Use scoped `status`, `diff`, and `push --dry-run` selectors when working on a subset of pages.

Reach for `wikitool` when you need wiki-grounded retrieval, template/profile lookup, lint/fix, sync, or guarded push flows.
