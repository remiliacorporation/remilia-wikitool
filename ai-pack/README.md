# Wikitool AI pack

Public agent companion for general MediaWiki work with Wikitool.

## Boundary

| Layer | Owns |
|---|---|
| Wikitool binary | MediaWiki transport, capability discovery, parsing, local knowledge artifacts, deterministic lint/fix, revision CAS, atomic writes, sync receipts, acceptance ledger |
| Agent skills | research-to-claim reasoning, article prose, source fidelity, due weight, BLP/sensitive review, reader value, adaptive interviews |
| Project site adapter | target templates, citation forms, source-review signals, categories, extensions, mechanical style, supplemental editorial guidance |
| Human editor | publication intent, private context, disputed judgments, and acceptance of the exact final prose |

`codex_skills/` contains the canonical substantive procedures. `.claude/skills/` contains harness entrypoints that route to those same files. `integration/` documents the architecture. Generic Wikitool ships no target-specific editorial behavior.

When a host project is packaged, its explicit adapter directory is copied as a supplement; it never replaces the public skills.

See `integration/agent_integration.md` and `integration/site_adapters.md`.
