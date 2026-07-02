# Changelog

All notable changes to contextmink are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

The release workflow extracts the section for the requested version and fails
if it is missing, so land notes here (staged under Unreleased, then retitled)
before dispatching a release.

## [Unreleased]

## [0.3.0] - 2026-07-02

### Added

- `outline`: declaration-line map of one source file (functions, types,
  headings) so the right region can be found without dumping whole windows
  into the transcript; `slice` then prints that region. Covers 19 languages
  through hand-written token classifiers, resolves extensionless scripts by
  shebang, filters rows with `--contains`, and takes `--prefix` for literal
  line starts or `--pattern` as the regex escape hatch.
- `contextmink-bridge` (Windows archive only): native PowerShell to Git Bash
  bridge. Finds Git Bash on its own, runs direct commands without MSYS
  argument rewriting, and accepts argv as `--argv-b64` or `--argfile` so
  PowerShell 5.1 quoting cannot corrupt arguments. Optional: contextmink
  itself runs from any shell.
- `codex-bash.sh` template: script bridge for repositories that want a shell
  entrypoint instead of a second binary.

### Changed

- Case-insensitive literal and `grep-terms` matching folds ASCII bytes
  without allocating.
- The launcher shields slash-bearing `--pattern`, `--prefix`, `--contains`,
  and `--term` values from MSYS rewriting, and rebuilds both binaries when
  either is missing or stale.
- Bridge-relative paths resolve from the `.contextmink.toml` policy root, so
  a vendored checkout anchors to the workspace it serves rather than to its
  own repository.
- `.gitattributes` ships with the sync surface, keeping the bash launchers
  LF on Windows checkouts.

## [0.2.0] - 2026-07-01

### Added

- `dirs`: bounded directory overview with recursive file counts.
- Broad scans enter git-ignored nested repository roots and disclose them in
  receipts, so multi-repo workspaces no longer report false completeness;
  `--skip-nested-repos` restores strict Git scope.
- `grep`/`grep-terms`: `--glob`/`--ext` filters, `-i`, `--context N`, named
  skipped-file samples, honest no-match scope.
- `json-select` `--where`/`--where-contains` row filters, `all_null_fields`
  audit, streamed `*.jsonl` input; `slice --tail N`; `duration_ms` in every
  receipt.
- `sqlite` `--timeout-secs` watchdog (default 60) interrupts runaway queries.

### Changed

- UTF-16LE/BE files are decoded and searched instead of skipped as binary; a
  UTF-8 BOM no longer breaks JSON parsing.
- Content scanning runs on multiple threads with walk-order-deterministic
  output.
- Bespoke fail-fast config parser: unknown or duplicate `.contextmink.toml`
  keys are hard errors.
- `capture` keeps both head and tail of truncated output.
