# /template - Template and Module Editing

Work on templates/modules/CSS with safe wiki sync flow.

## Lookup context

```bash
wikitool context "Template:Infobox person"
wikitool context "Template:Cite web"
wikitool docs search "Scribunto" --tier extension
wikitool docs search "TemplateStyles" --tier extension
```

## Template workflow

```bash
wikitool pull --templates
# edit files in custom/templates/
wikitool diff --templates
wikitool validate
wikitool push --templates --dry-run --summary "Template update"
```

Only run non-dry-run push when explicitly requested.
