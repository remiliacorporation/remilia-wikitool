---
name: wikitool-content-gate
description: Thin wrapper for deterministic wikitool content gates before push.
---

# Skill: wikitool-content-gate

Thin wrapper for content gating with `wikitool`.

Use normal reasoning and editorial judgment. Verify the live command surface against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Typical gate loop:
- Preferred full gate: `wikitool review --format json --summary "..."`
- Draft-first gate: `wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --format json --summary "Draft review"`
- `wikitool article lint <path> --format json`
- `wikitool article fix <path> --apply safe`
- `wikitool knowledge inspect references duplicates --title "<Title>" --format json`
- `wikitool validate --summary`
- Targeted integrity follow-up when requested: `wikitool validate --category broken-links --title "<Title>" --limit 20 --verify-live --format json`
- `wikitool diff`
- `wikitool push --dry-run --summary "..."`
