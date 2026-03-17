# /research - Topic Research

Thin wrapper for the evidence-gathering workflow.
Validate flags via `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

## Discovery

```bash
wikitool knowledge article-start "Topic" --format json
wikitool research search "Topic" --format json
wikitool research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json
wikitool search "topic"
wikitool context "Page Title"
wikitool docs import-profile remilia-mw-1.44
wikitool docs context "extension feature" --profile remilia-mw-1.44 --format json
wikitool search-external "topic"
```

## Local retrieval depth

```bash
wikitool knowledge inspect chunks "Title" --query "aspect" --limit 6 --token-budget 480
wikitool knowledge inspect chunks --across-pages --query "topic" --max-pages 8 --token-budget 1200 --format json --diversify
wikitool knowledge pack "Topic" --format json
```

Use `knowledge pack` only when the raw local retrieval substrate is needed beyond `article-start`.

## Live wiki verification

```bash
curl -s "https://wiki.remilia.org/api.php?action=query&list=search&srsearch=QUERY&format=json"
curl -s "https://wiki.remilia.org/api.php?action=query&titles=PAGE&prop=revisions&rvprop=content&format=json"
curl -s "https://wiki.remilia.org/api.php?action=query&titles=PAGE&redirects&format=json"
```

## Source policy

1. Prefer primary and official sources.
2. Follow `llm_instructions/style_rules.md` source restrictions.
3. Use named references for repeated citations.
