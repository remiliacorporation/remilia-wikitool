# /article - Wiki Article Writing

Thin wrapper for the default authoring workflow.
Write normally. Use `wikitool` to ground the draft in local/wiki context, check template/profile fit, lint/fix the page, and gate sync safely.
Canonical flags live in:

1. `wikitool --help`
2. `wikitool <command> --help`
3. `docs/wikitool/reference.md`

## Mandatory reads

1. `llm_instructions/style_rules.md`
2. `llm_instructions/article_structure.md`
3. `llm_instructions/writing_guide.md`

## Default lane

```bash
wikitool knowledge status --docs-profile remilia-mw-1.44 --format json
wikitool knowledge article-start "Topic" --format json
wikitool research search "Topic" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool templates show "Template:Infobox person"
wikitool wiki profile show --format json
```

Use `wikitool knowledge pack "Topic" --format json` only when the deeper raw retrieval substrate is needed behind `article-start`.
Use `wikitool templates examples "Template:Infobox person" --limit 2` when the template call shape is still unclear after `templates show`.

## Draft gate

```bash
wikitool article lint wiki_content/Main/<Title>.wiki --format json
wikitool article fix wiki_content/Main/<Title>.wiki --apply safe
wikitool validate
wikitool diff
wikitool push --dry-run --summary "Create or update: <Title>"
```

Only run non-dry-run push when explicitly requested.
