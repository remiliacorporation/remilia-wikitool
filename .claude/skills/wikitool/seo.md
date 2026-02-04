---
name: seo
description: SEO, network, and performance inspection commands
allowed-tools: Bash(bun run wikitool:*), Bash(cd:*)
argument-hint: seo inspect "<title>" | net inspect "<title>" | perf lighthouse [target]
---

# /wikitool seo - SEO & Performance Inspection

Inspect SEO metadata, network resources, and run performance audits.

## Reference

See `docs/wikitool/reference.md` for full flags and defaults.

## Commands

### SEO Inspection

Inspect SEO tags including title, Open Graph, Twitter Cards, and canonical URLs.

```bash
/wikitool seo inspect "Main Page"
/wikitool seo inspect "Milady Maker"
```

**Output includes:**
- Page title and meta description
- Open Graph tags (og:title, og:description, og:image)
- Twitter Card tags
- Canonical URL
- Schema.org markup (if present)

### Network Inspection

Inspect page resources, response headers, and cache status.

```bash
/wikitool net inspect "Main Page"
/wikitool net inspect "Main Page" --limit 25
```

**Output includes:**
- HTTP response headers
- Cache headers (Varnish, CDN)
- Resource loading (CSS, JS, images)
- Response sizes and timing

### Performance Audit

Run a Lighthouse audit and save the report.

```bash
/wikitool perf lighthouse "Main Page"
/wikitool perf lighthouse "Milady Maker"
```

**Prerequisites:** Lighthouse must be installed. Run:
```bash
# Windows
scripts/setup-tools.ps1

# macOS / Linux
scripts/setup-tools.sh
```

**Output:**
- Performance score
- Accessibility score
- Best practices score
- SEO score
- HTML report saved to `wikitool_exports/`

## Examples

```bash
# Full inspection workflow
/wikitool seo inspect "Main Page"           # Check SEO tags
/wikitool net inspect "Main Page" --limit 25 # Check resources
/wikitool perf lighthouse "Main Page"        # Run audit
```

## Execution

Run from the wikitool directory (auto-detects standalone vs embedded mode):

```bash
cd <wikitool-dir>

# SEO inspection
bun run wikitool seo inspect $ARGUMENTS

# Network inspection
bun run wikitool net inspect $ARGUMENTS

# Performance audit
bun run wikitool perf lighthouse $ARGUMENTS
```
