# /review - Pre-Push Quality Gate

Thin wrapper for the final gate before write push.
Validate flags via `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

## Gate sequence

```bash
wikitool article lint wiki_content/Main/<Title>.wiki --format json
wikitool validate
wikitool diff
```

Manual checks:

1. Structure (`SHORTDESC`, article quality banner, refs, categories)
2. Style compliance (`llm_instructions/style_rules.md`)
3. Citation quality and source reliability
4. Template/profile fit when the page depends on infobox or citation conventions

## Push gate

```bash
wikitool push --dry-run --summary "Review pass: <scope>"
```

Do not approve `--force` without explicit instruction.
