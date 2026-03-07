# /template - Template and Module Editing

Work on templates/modules/CSS with safe wiki sync flow.

## Lookup context

```bash
wikitool context "Template:Infobox person"
wikitool context "Template:Cite web"
wikitool docs import-profile remilia-mw-1.44
wikitool docs context "Scribunto" --profile remilia-mw-1.44 --format json
wikitool docs search "TemplateStyles" --profile remilia-mw-1.44 --tier extension
```

## Template workflow

```bash
wikitool pull --templates
# edit files in templates/
wikitool diff --templates
wikitool validate
wikitool push --templates --dry-run --summary "Template update"
```

Only run non-dry-run push when explicitly requested.
