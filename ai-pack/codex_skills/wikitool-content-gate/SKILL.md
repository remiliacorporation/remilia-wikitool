---
name: wikitool-content-gate
description: Run deterministic content quality gates for wiki edits before push, including validate, diff, index/category checks, and explicit dry-run gate output.
---

# Skill: wikitool-content-gate

Keep this skill thin and policy-focused.
Canonical command truth is CLI help and runbooks.

## Validation pass

```bash
wikitool validate
wikitool diff
```

## Link, category, and index signals

```bash
wikitool index orphans
wikitool index backlinks "Title"
wikitool index prune-categories
wikitool search "Category:"
```

## Docs-assisted checks

```bash
wikitool docs search "extension feature"
wikitool context "Template:Infobox person"
```

## Push-gate report contract

Before any write push, report:

1. Validation result (`pass` or explicit failures)
2. Diff scope summary
3. Risk notes (delete, force, template/category scope)
4. Next command (`wikitool push --dry-run --summary ...`)
