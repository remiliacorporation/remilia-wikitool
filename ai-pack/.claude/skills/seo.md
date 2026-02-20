# /seo - Page Diagnostics

Run page-level SEO/network/perf inspections.

## Core commands

```bash
wikitool seo inspect "Main Page"
wikitool net inspect "Main Page" --limit 25
wikitool perf lighthouse "Main Page" --output html
```

Useful variants:

```bash
wikitool seo inspect "Main Page" --json
wikitool net inspect "Main Page" --no-probe --json
wikitool perf lighthouse --show-version
```

Use findings to improve page metadata and internal linking quality.
