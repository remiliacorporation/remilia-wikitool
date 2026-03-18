---
name: wikitool-content-gate
description: Thin wrapper for deterministic wikitool content gates before push.
---

# Skill: wikitool-content-gate

Thin wrapper for content gating with `wikitool`.

Use normal reasoning and editorial judgment. Verify the live command surface against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Typical gate loop:
- `wikitool article lint <path> --format json`
- `wikitool article fix <path> --apply safe`
- `wikitool validate`
- `wikitool diff`
- `wikitool push --dry-run --summary "..."`
