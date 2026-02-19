# Skill: wikitool-content-gate

Run a deterministic content quality gate for AI-authored wiki changes.

## Validation pass

```bash
wikitool validate
wikitool diff
```

## Link/category/index signals

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

## Push gate output standard

Before any write push, report:

1. Validation result (`pass` or explicit failures)
2. Diff scope summary
3. Risk notes (deletes/force/templates/categories)
4. Next command (`push --dry-run --summary ...`)
