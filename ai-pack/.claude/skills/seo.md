# /seo - Page Diagnostics

Thin wrapper for page metadata and network inspection.
Use browser/devtools judgment for broader rendering or UX questions. Use `wikitool` here for SEO metadata and resource inspection.

Validate flags via `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

## Core commands

```bash
wikitool seo inspect "Main Page"
wikitool net inspect "Main Page" --limit 25
```

Useful variants:

```bash
wikitool seo inspect "Main Page" --json
wikitool net inspect "Main Page" --no-probe --json
```

Use findings to improve page metadata and internal linking quality.
