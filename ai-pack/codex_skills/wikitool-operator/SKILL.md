---
name: wikitool-operator
description: Operate the Rust wikitool CLI for wiki editing sync, docs, import, and inspection workflows with dry-run guardrails and canonical CLI help alignment.
---

# Skill: wikitool-operator

Keep this skill as a thin overlay.
Canonical truth is `CLAUDE.md`, runbooks (`SETUP.md`, `docs/wikitool/*`), and live CLI help.

## Canonical lookup order

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

Do not introduce flags or command shapes that only exist in this skill.

## Safe write sequence

```bash
wikitool pull --full --all
wikitool diff
wikitool validate
wikitool push --dry-run --summary "Summary"
```

If dry-run is correct:

```bash
wikitool push --summary "Summary"
```

## Preflight and diagnostics

```bash
wikitool status
wikitool db stats
wikitool knowledge inspect stats
wikitool docs list --outdated
```

## Editing workflows

```bash
wikitool pull --full --all
wikitool context "Template:Infobox person"
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool knowledge status --docs-profile remilia-mw-1.44
wikitool knowledge pack "Topic" --format json
wikitool knowledge inspect chunks --across-pages --query "topic terms" --max-pages 6 --limit 10 --token-budget 1200 --format json --diversify
wikitool search "Category:"
wikitool docs import-profile remilia-mw-1.44
wikitool docs search "extension feature" --profile remilia-mw-1.44
wikitool docs context "parser function" --profile remilia-mw-1.44 --format json
```

## Retrieval guidance

1. Treat local files as the human editing surface and SQLite as the AI retrieval layer.
2. Prefer `knowledge pack` for authoring retrieval, and use `knowledge status` to confirm whether docs-bridged context is available before relying on it.
3. Describe references using their source metadata, authority/identifier matches, and retrieval signals; do not imply that wikitool assigns authoritative quality ratings.
4. Use `knowledge inspect templates TEMPLATE` when you need the implementation bundle for an active template, including `/doc` and `Module:` pages when present.
5. When `remilia-mw-1.44` docs are imported, authoring retrieval can bridge pinned MediaWiki docs with local template/module patterns; use that before falling back to generic web docs.

## Safety constraints

1. Never skip dry-run before write push.
2. Do not use `--force` without explicit user approval.
3. For delete flows, require `--reason` and prefer `--dry-run` first.
4. Treat infrastructure/release operations as out of scope unless explicitly requested.
5. If the local DB is stale or incompatible, run `db reset --yes`, then rerun `pull --full --all` if needed and rebuild it with `knowledge build` or `knowledge warm`.
