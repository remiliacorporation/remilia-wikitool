### Bounded Output

Use `scripts/contextmink` when a file/text/JSON/SQLite/command-output read may
produce more output than the transcript should carry.

- Start with `dirs` to orient in an unfamiliar tree, then `files` or `grep`
  for candidate discovery. Prefer `files --ext json` / `--extension jsonl`
  (comma-separated lists work: `--ext rs,toml`) across Windows-to-Bash
  boundaries because wildcard globs can expand before contextmink receives
  them.
- Read source files through `outline` then `slice`, not dump windows. A named
  file is still reconnaissance while the answer's location inside it is
  unknown: `outline <file>` maps declaration lines with line numbers
  (`--contains TEXT` filters rows; `--lang`, `--prefix <text>`, or
  `--pattern <regex>` cover unrecognized extensions), then
  `slice --range START:END` prints the region. `slice` replaces `sed -n` /
  `cat` / `head` file windows. Keep its default caps (120-line window,
  220-line ceiling); narrow an oversized read with `outline` or
  `grep --context` instead of raising `--max-lines`.
- Use `grep --pattern-file <file>` for shell-fragile regex; use `grep-terms`
  for literal tokens or phrases (`--or` / `--any`, `--term-file`, `--limit`,
  `--max-matches`). Narrow either with `--glob` / `--ext`, add `-i` for
  case-insensitive matching, and `--context N` when the surrounding lines
  would otherwise need a follow-up `slice`.
- Use `slice --tail N` for the end of logs, `json-find`, `json-select` (with
  `--where FIELD=VALUE` / `--where-contains FIELD=TEXT` row filters;
  `--keys` first when the row shape is unknown), `sqlite-schema`, and
  `sqlite --sql-file` for bounded reads instead of opening whole large
  files, reports, or databases. `sqlite` binds JSON/JSONL worklists as
  named parameters (`--jsonl-param w=file.jsonl` with `json_each(:w)`) and
  registers `hexint(x)` for joining `0x...` hex strings against integer
  columns.
- Prefer a domain command's native compact/projection/limit flags first. Use
  `capture -- <command> ...` or `run` only when output size is uncertain and no
  native bound exists; read the child `exit_code`/`success` fields in the
  receipt. Truncated captures keep both the head and the tail of the output.
- Configured excludes keep broad scans quiet. Pass an explicit file or
  subdirectory when an excluded tree is the target. Use `--with-excluded` to
  include files matched by contextmink exclude globs, and `--with-git-ignored`
  only for files hidden by Git or `.ignore` rules. Broad scans enter
  git-ignored nested repository roots and disclose them in
  `nested_repos_entered`; pass `--skip-nested-repos` for strict Git scope.
- Treat a `CONTEXTMINK_RECEIPT` with `"truncated": true` or `"complete": false`
  as capped output and narrow the query. Use `--fail-if-truncated` for
  automation that requires full displayed output, or `--require-complete-scan`
  when scan-capped totals should fail. When
  `cap_reason` is `"scan"` or match-side lower-bound fields are true, match
  totals and no-match results cover only the scanned subset (candidate file
  totals stay exact). A no-match grep with
  `no_match_scope: "scanned_subset"` or a `json-select` with `all_null_fields`
  entries needs a narrower or corrected query, not a conclusion.
- Direct commands are fine when output is already known to be small or
  structurally bounded: `git status --short`, `git diff --stat`, a focused
  test command, a domain tool that emits compact records, or one exact file
  region already known to fit a slice window (about 120 lines). Above that,
  the read is reconnaissance — go through `outline`/`grep`/`slice`. Knowing
  the range you chose does not make the output small; choosing a large range
  is the failure the caps exist to catch.
- Keep domain-specific parsing, validation, indexing, diagnostics, and
  synchronization in project-native tools.
