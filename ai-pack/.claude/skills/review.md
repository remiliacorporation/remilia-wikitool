# /review - Thin wrapper

Thin wrapper for deterministic wiki content gating with `wikitool`.

Use normal reasoning and editorial judgment. Verify current commands against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Typical gate loop:
- `wikitool article lint <path> --format json`
- `wikitool article fix <path> --apply safe`
- `wikitool validate`
- `wikitool diff`
- `wikitool push --dry-run --summary "..."`
