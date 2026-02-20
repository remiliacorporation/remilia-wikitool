# /article - Wiki Article Writing

Create or edit wiki articles with current Remilia Wiki rules.

## Mandatory reads

1. `llm_instructions/style_rules.md`
2. `llm_instructions/article_structure.md`
3. `llm_instructions/writing_guide.md`

## Preparation

```bash
wikitool pull
wikitool workflow authoring-pack "Topic" --format json
wikitool search "topic"
wikitool context "Template:Infobox person"
wikitool search "Category:"
wikitool index chunks "Title" --query "aspect" --limit 6 --token-budget 480
```

## Write and gate

1. Edit `wiki_content/Main/<Title>.wiki`.
2. Ensure required structure, citations, and categories.
3. Run:

```bash
wikitool validate
wikitool diff
wikitool push --dry-run --summary "Create or update: <Title>"
```

Only run non-dry-run push when explicitly requested.
