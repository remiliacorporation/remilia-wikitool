# /review - Thin wrapper

Thin wrapper for deterministic wiki content gating with `wikitool`.

Use normal reasoning and editorial judgment. Verify current commands against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Typical gate loop:
- Preferred full gate: `wikitool review --format json --summary "..."`
- `wikitool article lint <path> --format json`
- `wikitool article fix <path> --apply safe`
- `wikitool knowledge inspect references duplicates --title "<Title>" --format json`
- `wikitool validate --summary`
- Targeted integrity follow-up when requested: `wikitool validate --category broken-links --title "<Title>" --limit 20 --verify-live --format json`
- `wikitool diff`
- `wikitool push --dry-run --summary "..."`
