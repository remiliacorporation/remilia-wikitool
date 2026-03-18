# /template - Template and Module Editing

Thin wrapper for template/module/CSS work.
Edit template, module, CSS, and JS files normally. Use `wikitool` when you need live template catalog context, Remilia profile preferences, docs lookup, and guarded sync.
Validate flags via `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

## Lookup context

```bash
wikitool templates show "Template:Infobox person"
wikitool templates examples "Template:Cite web" --limit 2
wikitool wiki profile show --format json
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
