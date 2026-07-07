# Changelog

All notable changes to contextmink are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

The release workflow extracts the section for the requested version and fails if it is missing, so land notes here (staged under Unreleased, then retitled) before dispatching a release. Write one line per paragraph or bullet: GitHub release bodies render every newline as a line break, so hard-wrapped prose comes out ragged.

## [Unreleased]

### Fixed

- The Bash launcher now finds Cargo in common Windows/WSL layouts (`$HOME/.cargo/bin` and a login-Bash lookup) instead of relying only on non-login `PATH`, and the installed launcher template is tested against the repo launcher so the two cannot drift.

## [0.6.0] - 2026-07-06

### Added

- `files --term TEXT` filters candidate paths by repeated literal path/name terms before extension and display caps, with `--name-contains` as a readable alias.
- `grep --pattern PATTERN` makes every positional value a path, matching `--pattern-file` ergonomics when the search pattern is shell-fragile or visually ambiguous.
- `contextmink-bridge` and `capture`/`run` refuse known destructive argv before spawn: built-in `git clean` blocking, nested shell payload scanning, and optional repository-configured protected deletion fragments. `CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE=1` is a human break-glass override and prints a warning.
- `capture` gains `--expect-exit CODE[,CODE...]`, recording `expected_exit_codes` and `exit_expected` without overwriting the child's actual `success`.
- `capture` gains `--receipt-out FILE`, writing the full JSON capture receipt with retained stdout/stderr text while keeping terminal output bounded.
- `hook-guard` evaluates an agent PreToolUse hook payload (JSON on stdin) against the same destructive-command deny scan the bridge and `capture`/`run` apply to child argv, so the harness hook layer, spawn paths, and shell payload scanning enforce one policy from one config. It extracts the command string at `--command-field DOT.PATH` (default `tool_input.command`, the Claude Code hook shape), exits 0 to allow, and exits 2 with the deny message on stderr to block. Unparseable payloads or a missing command field allow with a stderr note rather than blocking: a hook that fails closed on payload-shape drift blocks every shell command for every agent (the 2026-07-05 outage), while the guard's job is only to block recognized destructive commands. `CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE=1` downgrades a deny to a loud stderr warning, matching the bridge.
- `hook-snippet` prints a Claude `.claude/settings.json` fragment for `hook-guard`, using single `command` strings and Bash-safe Windows path spelling instead of raw backslash paths or an unverified `args` field.

### Changed

- `capture` now separates bounded display text from retained receipt text: terminal output still obeys line and byte caps, while `--json`, `--receipt-out`, and encoding-suspect scans use the retained head/tail text with explicit omitted-byte markers.
- `sqlite --sql-file` help now documents `-` for reading SQL from stdin.

## [0.5.0] - 2026-07-04

### Added

- `sqlite` accepts named JSON file bindings: `--json-param NAME=FILE` binds a JSON document and `--jsonl-param NAME=FILE` binds a JSONL file as a JSON array, for read-only `json_each(:NAME)` joins.
- `sqlite` registers a `hexint(x)` SQL function: it parses a `0x`-prefixed hex string (or a decimal digit string) to INTEGER, passes integers through, keeps NULL, and errors on anything else. SQLite's own CAST cannot parse hex, so a `0x...` string can join an integer column on its index — `JOIN targets t ON t.addr = hexint(q.value ->> '$.addr')` — without the table-side text formatting that would force a scan.
- `sqlite` param receipt rows carry `values`: the element count when the bound document is an array (its `json_each` row count), `null` otherwise, so a wrong-shape binding is visible in the receipt.
- `files` gains `--quiet`, matching `grep`/`grep-terms`: it suppresses the file list and emits only the receipt.
- `files`/`grep`/`grep-terms` `--ext` accepts comma-separated lists (`--ext xml,lua,toc`); a comma previously matched one literal extension and returned zero files.
- `json-select --array` accepts a bare top-level key (`--array entries`), sharing `--field`'s key-or-pointer semantics.
- `json-select --fields KEY,KEY` takes a comma-separated list (and `--field` now also does), so a multi-field projection is one flag.
- `json-select --keys` reports the union of top-level row keys with presence counts, non-null counts, and value types. Composes with `--where`/`--where-contains`; conflicts with `--field`/`--fields`.
- `outline` C/C++ emits section-banner comment titles: `// ==== Renderer ====` one-liners and the title line of a `// ====`-fenced banner.
- `grep`/`grep-terms` receipts split `skipped_large` and `skipped_binary` alongside the combined `skipped_large_or_binary`; only skipped large files leave text unexamined and mark match totals as lower bounds.
- `sqlite-schema` receipts gain `tables_detail_elided` and per-table `detail_elided`.
- `slice` and `outline` receipts gain an `encoding_suspects` object (and a one-line note) when the decoded file carries mojibake. `double_encoded` counts character runs whose CP1252 bytes re-decode as valid multi-byte UTF-8 (the recovered character is named in `sample`), `replacement_chars` counts U+FFFD, and `c1_controls` counts raw C1 characters. A 2-byte run counts only with a Latin-1 lead or when it clusters, so accented text does not trip it. `capture` reports the double-encoded count for child output, where UTF-8 written through a CP1252 boundary appears. The field is absent when nothing is found, and it never fails a command.

### Changed

- `outline` xml is now a depth-tracking element-stack parser over the whole document instead of per-line shape checks: multi-line tags anchor at their `<` line, quoted `>` inside attributes cannot end a tag, comments/CDATA/DOCTYPE are skipped exactly, and unnamed wrapper elements under a named ancestor stay out while unnamed shallow sections with no named ancestor still map. Unclosed containers at EOF still outline.
- `sqlite-schema` column/index budgets are now table-atomic: a table shows its complete column and index detail or elides it whole with a per-table `(detail elided: … rerun with --table <name>)` note. Previously the global column cap could show a mid-list table's columns partially while still printing its indexes, which read as a complete table.

### Fixed

- `outline` xml no longer floods on attribute-schema exports: name-attributed elements that self-close or close on the same line (`<Field Name="ID"/>` rows in schema definition XML) are scalar enumeration, not structure, and stay out; name-attributed containers still map.
- `sqlite --jsonl-param` now rejects a file holding a single top-level JSON array instead of silently wrapping it to `[[...]]`, where `json_each` would see one row instead of N; the error points at `--json-param`.
- `sqlite --json-param` fed a JSONL file now teaches the fix (`parses as N JSONL values; bind it with --jsonl-param instead`) instead of surfacing a bare serde `trailing characters` error.

## [0.4.0] - 2026-07-03

### Added

- `grep`/`grep-terms` `--quiet`: suppresses per-file match content and file lists and emits only the receipt (totals, caps, truncation, scan-scope fields), for existence/count checks that do not need the matching lines. The receipt is unchanged apart from a `quiet: true` disclosure.
- `contextmink-bridge --print-root`: prints the resolved bridge root (`CONTEXTMINK_BRIDGE_ROOT`, else the policy/`.git` anchor) and exits, so a silently wrong anchoring root is inspectable.
- `capture --fail-with-child`: exits with the child's exit code when the child fails, after the receipt is emitted, so shell chains (`capture --fail-with-child -- cmd && next`) can gate on the child. The default stays exit 0 with the child status only in the receipt's `exit_code`/`success` fields.
- `outline` gains a `json` language (`.json`/`.jsonc`): container-opening keys (`"key": {` / `"key": [`) map sidecar and recipe structure without enumerating scalars, composing with `slice` and `json-select` for the region found.
- `outline` gains an `xml` language (`.xml`/`.xsd`/`.xaml`): elements carrying a boundary-checked `name`/`id` attribute (FrameXML frames, MSBuild targets, Android views; `filename=` does not count) plus shallow block-opening sections (`<page>` in MediaWiki exports); closing tags, comments, processing instructions, and shallow leaf content that self-closes or closes on the same line stay out.
- `outline` Lua coverage now includes column-0 table roots (`MyAddon = {}`, `local p = {}` in Scribunto modules, `T = T or {}`, multi-line `Defaults = {`); indented table assignments (locals inside functions) and one-liner closed tables (`t = {1, 2}`) stay out.
- `outline` C-family coverage now includes prototypes (`int f(int);` — headers carry their structure as prototypes), indented class members and nested aggregates, access labels (`public:`), `operator` overloads, and out-of-line ctor/dtor definitions; calls stay filtered by their single-token heads and statement keywords.
- `outline` Java/C# coverage now includes package-private members (statement-keyword filter replaces the modifier requirement) and constructors (modifier-led single-token heads).
- `outline` JS/TS coverage now includes class and object-literal method heads (`render(a) {`, `static create() {`, `get value() {`, `#private() {`, `constructor(` and TS generic methods); call statements are excluded by trailing-`;`/`)` shape and object-argument or callback calls (`fetch(url, {`, `it('x', () => {`) by parenthesis balance.

### Changed

- Candidate enumeration (`files`, `dirs`, `grep`, `grep-terms`) now walks directories in parallel and always runs to completion before the `--max-scan-files` cap applies: scans are ~2.5x faster on multi-repo workspaces, candidate totals are exact even when the scan cap fires (`candidate_files_total_is_lower_bound` is now always `false`; match totals under a capped content scan remain lower bounds), and a capped candidate list is the sorted prefix — deterministic and independent of walk order or root spelling, where it was previously the walk-order prefix.
- `contextmink-bridge` no longer prints a stderr notice when an extensionless bash script retries through Git Bash: the retry is the designed path for repo scripts run in direct mode, the notice read as a warning on successful runs, and PowerShell 5.1 wraps native stderr in `NativeCommandError` records that can mark a zero-exit pipeline as failed. `CONTEXTMINK_BRIDGE_DEBUG=1` restores the interpreter disclosure.
- `contextmink-bridge` no longer falls back to Cygwin (`C:\cygwin64`) or MSYS2 (`C:\msys64`) bash when Git Bash is missing: those shells have different path and file-locking semantics and must not silently substitute for Git Bash. `CONTEXTMINK_BASH` remains the explicit override for exotic hosts.

### Fixed

- `grep`/`grep-terms` now marks `matched_files_total_is_lower_bound` and `total_matches_is_lower_bound` when `--max-scan-files` caps candidate scanning, so a partial scanned prefix cannot look like a complete match count.
- `contextmink-bridge` direct mode (`--`) now resolves a program spelled as a path (`./gradlew`, `bin/tool`) against `--cwd`, matching POSIX exec semantics. Rust's `Command` resolves relative programs against the parent's working directory, so `--cwd <dir> -- ./script` failed `command not found` before the extensionless-script Git Bash fallback could fire; bare names (`git`) keep PATH lookup, and absolute or rooted spellings are never re-anchored.
- `contextmink-bridge` `command not found` errors now teach the fix at the point of failure: the message names the resolved path, discloses the `--cwd` resolution when one happened, and points path-like programs at `--script <path>` (which resolves from the bridge root).
- `contextmink-bridge --argv-b64` no longer drops a trailing empty argument: the documented PowerShell encoder (`$argv -join [char]0`) never emits a trailing NUL, so the old drop-one-empty-tail compensation only ever destroyed a genuine trailing empty argument.
- `outline` no longer drops TS/JS annotated arrow bindings such as `const f: () => void = () => {}`: the assignment scan now skips `=>` arrows (including inside generics like `Array<() => void>`) and `==`/`===`/`<=`/`>=` sequences when locating the binding's `=`.
- Windows release archives now write the `.sha256` checksum file with LF line endings, so `sha256sum -c` on POSIX shells verifies it without stripping carriage returns. The 0.3.0 Windows checksum file carries CRLF; strip `\r` before verifying it.

## [0.3.0] - 2026-07-02

### Added

- `outline`: declaration-line map of one source file (functions, types, headings) so the right region can be found without dumping whole windows into the transcript; `slice` then prints that region. Covers 19 languages through hand-written token classifiers, resolves extensionless scripts by shebang, filters rows with `--contains`, and takes `--prefix` for literal line starts or `--pattern` as the regex escape hatch.
- `contextmink-bridge` (Windows archive only): native PowerShell to Git Bash bridge. Finds Git Bash on its own, runs direct commands without MSYS argument rewriting, and accepts argv as `--argv-b64` or `--argfile` so PowerShell 5.1 quoting cannot corrupt arguments. Optional: contextmink itself runs from any shell.
- `codex-bash.sh` template: script bridge for repositories that want a shell entrypoint instead of a second binary.

### Changed

- Case-insensitive literal and `grep-terms` matching folds ASCII bytes without allocating.
- The launcher shields slash-bearing `--pattern`, `--prefix`, `--contains`, and `--term` values from MSYS rewriting, and rebuilds both binaries when either is missing or stale.
- Bridge-relative paths resolve from the `.contextmink.toml` policy root, so a vendored checkout anchors to the workspace it serves rather than to its own repository.
- `.gitattributes` ships with the sync surface, keeping the bash launchers LF on Windows checkouts.

## [0.2.0] - 2026-07-01

### Added

- `dirs`: bounded directory overview with recursive file counts.
- Broad scans enter git-ignored nested repository roots and disclose them in receipts, so multi-repo workspaces no longer report false completeness; `--skip-nested-repos` restores strict Git scope.
- `grep`/`grep-terms`: `--glob`/`--ext` filters, `-i`, `--context N`, named skipped-file samples, honest no-match scope.
- `json-select` `--where`/`--where-contains` row filters, `all_null_fields` audit, streamed `*.jsonl` input; `slice --tail N`; `duration_ms` in every receipt.
- `sqlite` `--timeout-secs` watchdog (default 60) interrupts runaway queries.

### Changed

- UTF-16LE/BE files are decoded and searched instead of skipped as binary; a UTF-8 BOM no longer breaks JSON parsing.
- Content scanning runs on multiple threads with walk-order-deterministic output.
- Bespoke fail-fast config parser: unknown or duplicate `.contextmink.toml` keys are hard errors.
- `capture` keeps both head and tail of truncated output.
