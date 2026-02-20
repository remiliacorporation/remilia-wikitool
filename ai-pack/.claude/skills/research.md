# /research - Topic Research

Research using local wiki index, live API checks, and external references.

## Discovery

```bash
wikitool search "topic"
wikitool context "Page Title"
wikitool docs search "extension feature"
wikitool search-external "topic"
```

## Live wiki verification

```bash
curl -s "https://wiki.remilia.org/w/api.php?action=query&list=search&srsearch=QUERY&format=json"
curl -s "https://wiki.remilia.org/w/api.php?action=query&titles=PAGE&prop=revisions&rvprop=content&format=json"
curl -s "https://wiki.remilia.org/w/api.php?action=query&titles=PAGE&redirects&format=json"
```

## Source policy

1. Prefer primary and official sources.
2. Follow `llm_instructions/style_rules.md` source restrictions.
3. Use named references for repeated citations.
