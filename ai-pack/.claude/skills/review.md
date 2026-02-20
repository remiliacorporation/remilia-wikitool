# /review - Pre-Push Quality Gate

Perform final review checks before write push.

## Gate sequence

```bash
wikitool validate
wikitool diff
```

Manual checks:

1. Structure (`SHORTDESC`, quality tag, refs, categories)
2. Style compliance (`llm_instructions/style_rules.md`)
3. Citation quality and source reliability

## Push gate

```bash
wikitool push --dry-run --summary "Review pass: <scope>"
```

Do not approve `--force` without explicit instruction.
