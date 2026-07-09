---
name: wikitool-content-gate
description: Thin wrapper for deterministic wikitool content gates before push.
---

# Skill: wikitool-content-gate

Thin wrapper for content gating with `wikitool`.

Use normal reasoning and editorial judgment. Verify the live command surface against `wikitool --help`, `wikitool <command> --help`, and `docs/wikitool/reference.md`.

Typical gate loop:
- Preferred gate brief: `wikitool review --format json --view brief --summary "..."`
- Draft-first gate: `wikitool review --draft-path .wikitool/drafts/Title.wiki --title "Title" --format json --view brief --summary "Draft review"`; follow its `next_steps` for `article promote` and scoped push dry-run.
- Direct draft iteration: `wikitool article lint .wikitool/drafts/Title.wiki --title "Title" --format json`; `wikitool article fix .wikitool/drafts/Title.wiki --title "Title" --apply safe`; `wikitool article promote .wikitool/drafts/Title.wiki --title "Title" --format json`
- `wikitool article lint <path> --format json`
- `wikitool article fix <path> --apply safe`
- `wikitool knowledge inspect references duplicates --title "<Title>" --format json`
- `wikitool validate --summary`
- Targeted integrity follow-up when requested: `wikitool validate --category broken-links --title "<Title>" --limit 20 --verify-live --format json`
- `wikitool diff`
- `wikitool push --dry-run --summary "..."`
- After a live push that changes templates, Cargo output, or other dynamic HTML: `wikitool wiki render-check "<Consumer title>" --scope-class CLASS --expect-scopes N --require-interactive-link --require-href-contains TEXT --format json`; add `--require-link-class mw-file-description` for native MediaViewer links and `--require-page-image FILE` for PageImages/Popups selection.
