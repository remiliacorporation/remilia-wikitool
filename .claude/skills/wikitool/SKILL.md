---
name: wikitool
description: Run the Rust wikitool CLI for sync, indexing, docs, import, inspection, workflow, and release operations. Use when executing command-line workflows with dry-run guardrails and canonical CLI-help validation.
allowed-tools: Bash(wikitool:*), Bash(cargo:*), Bash(cd:*), Read, Write
argument-hint: [command] [options]
---

# /wikitool - Rust CLI Gateway

Treat this skill as a thin overlay.
Canonical behavior is defined by runbooks and live CLI help.

## Usage

```text
/wikitool
/wikitool <command>
/wikitool <command> [args]
```

## Resolution

1. Prefer direct binary: `wikitool ...`.
2. If binary is unavailable, use `cargo run --quiet --package wikitool -- ...`.
3. Do not invent flags from memory; verify against help.

## Canonical lookup order

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

## Guardrails

1. Run `diff` before push/delete workflows.
2. Run `push --dry-run --summary "..."` before write push.
3. Do not use `--force` without explicit approval.
4. Treat the local DB as disposable; use `db reset` or delete `.wikitool/data/wikitool.db` instead of preserving old schema state.

## Binary-native workflow helpers

```bash
wikitool workflow bootstrap
wikitool workflow full-refresh --yes
wikitool knowledge warm --docs-profile remilia-mw-1.44
wikitool knowledge status --docs-profile remilia-mw-1.44
wikitool knowledge article-start "Topic" --format json
wikitool research search "Topic" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool wiki profile sync
wikitool templates show "Template:Infobox person"
wikitool article lint wiki_content/Main/Title.wiki --format json
wikitool knowledge inspect chunks --across-pages --query "topic terms" --max-pages 6 --limit 10 --token-budget 1200 --format json --diversify
wikitool knowledge pack "Topic" --format json
wikitool docs generate-reference
wikitool dev install-git-hooks
wikitool release build-ai-pack
wikitool release package
wikitool release build-matrix
```

## Retrieval model

1. Treat local files as the editor-facing source of truth.
2. Treat SQLite as an AI retrieval index: semantic page profiles, sections, templates, module invocation patterns, references, source authorities, identifiers, template implementation bundles, pinned docs corpora, links, and media.
3. Do not describe reference rows as quality scored; use the stored retrieval signals, authority/identifier data, and source metadata directly.
4. Prefer `knowledge article-start` as the default authoring retrieval front door; use `knowledge pack` only when the deeper raw substrate is needed, and use `knowledge status` to verify readiness before depending on local context.
