---
name: wikitool-content-gate
description: Run deterministic content quality gates for wiki edits before push, including validate, diff, index/category checks, and explicit dry-run gate output.
---

# Skill: wikitool-content-gate

Keep this skill thin and policy-focused.
Canonical command truth is CLI help and runbooks.
Use `wikitool` here for deterministic wiki-aware gates, not as a replacement for editorial judgment or normal file editing.

## Canonical lookup order

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

## Validation pass

```bash
wikitool article lint wiki_content/Main/Title.wiki --format json
wikitool validate
wikitool diff
```

## Link, category, and index signals

```bash
wikitool knowledge inspect orphans
wikitool knowledge inspect backlinks "Title"
wikitool knowledge inspect empty-categories
wikitool search "Category:"
```

## Docs-assisted checks

```bash
wikitool docs context "extension feature" --profile remilia-mw-1.44 --format json
wikitool wiki profile show --format json
wikitool templates show "Template:Infobox person"
```

## Push-gate report contract

Before any write push, report:

1. Article lint result (`pass` or explicit rule hits)
2. Validation result (`pass` or explicit failures)
3. Diff scope summary
4. Risk notes (delete, force, template/category scope)
5. Next command (`wikitool push --dry-run --summary ...`)
