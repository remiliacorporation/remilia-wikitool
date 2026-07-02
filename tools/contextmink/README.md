# contextmink

`contextmink` is a transcript guard for command-line code work. It provides
bounded ways to list files, search text, map file structure, read line
windows, inspect JSON, run read-only SQLite queries, and capture unknown-size
command output without dumping large outputs into the conversation.

It is deliberately generic. Project-specific parsing, validation, indexing,
diagnostics, and synchronization should stay in project-native tools.

## Commands

- `files`: list candidate files with hard caps and configured excludes. Include
  globs match either the displayed path or the basename, so `--glob '*.jsonl'`
  works inside an explicit queue directory. Configured excludes apply to broad
  scans, but an explicit path inside an excluded tree is treated as the target
  and searched without `--with-excluded`. Use `--with-git-ignored` only when
  intentionally inspecting files hidden by Git or `.ignore` rules. On Windows
  shell bridges, prefer `--ext jsonl` over wildcard globs for extension filters.
- `dirs`: bounded directory overview with recursive file counts per directory,
  `--depth` levels below each root. Use it to orient in an unfamiliar tree
  before running `files` or `grep`. Directories that contain no files do not
  appear.
- `grep`: print a bounded file/sample summary for a regex or literal pattern.
  `total_matches` counts matching lines. Use `--pattern-file <file>` when regex
  punctuation would be fragile through a host shell bridge; `--glob` / `--ext`
  to narrow the candidate set; `-i` / `--ignore-case` for case-insensitive
  matching; `--context N` to include surrounding lines with each sample
  (context lines print with a `-` separator and count against the sample
  budget). `--max-count-files` stops content scanning after enough matching
  files have been found; receipts mark match totals as lower bounds when that
  cap fires. Content scanning runs on multiple threads with walk-order
  deterministic output.
- `grep-terms`: match lines containing all `--term` values by default, or any
  term with `--mode any` / `--any` / `--or`. This is the preferred search form
  when the query is tokens rather than structure — no regex syntax to quote,
  and ASCII case-insensitive matching folds bytes without allocating. Use
  `--term-file <file>` for phrase lists when shell quoting or regex
  punctuation would make inline arguments fragile. Accepts the same `--glob` / `--ext` / `-i` / `--context`
  narrowing as `grep`. `--limit` caps printed files; `--max-matches` /
  `--max-lines` cap printed sample match lines.
- `slice`: the guarded file-read primitive. Print bounded line windows
  (`--range`, `--start`/`--end`, or `--tail N` for the end of a file), or
  character windows for very long single-line files and pasted attachments.
  Use it where `sed -n`, `cat`, or `head` windows would otherwise dump file
  content into the transcript. The defaults (120-line window, 220-line
  `--max-lines` ceiling) are deliberate: a read that wants more than one
  window is reconnaissance — locate the region with `outline` or
  `grep --context` first instead of raising the caps. Receipts report the
  detected `encoding` and the file's `total_lines`.
- `outline`: map declaration-shaped lines in one source file — functions,
  types, impls, classes, headings — as `line: text` rows, so a large file can
  be navigated for tens of lines of output instead of dumped in windows. Each
  built-in language rule (Rust, Python, JavaScript/TypeScript, Go, C/C++,
  Java, C#, Kotlin, shell, Lua, Ruby, Markdown, TOML, INI, YAML, SQL DDL,
  MediaWiki wikitext) is a hand-written token classifier, not a parser and
  not a regex: rows can include false positives
  and indentation conveys nesting. Extensionless scripts resolve through
  their `#!` shebang line (bash/sh/zsh, python, lua, ruby, node). `--lang`
  overrides detection, `--prefix <text>` outlines any other format by literal
  line prefix (after indentation), `--pattern <regex>` remains as the
  full-power escape hatch, and `--contains TEXT` (with `-i`) filters rows. Use it before `slice` when
  the file is known but the location of the answer inside it is not.
- `json-find`: query JSON by key, path, or summarized value without opening the
  whole document.
- `json-select`: project a JSON document or array to bounded row summaries using
  JSON Pointer and field selectors. `--where FIELD=VALUE` and
  `--where-contains FIELD=TEXT` keep only matching rows (repeatable; all must
  hold; string values compare without JSON quotes). Files named `*.jsonl` are
  streamed row-by-row without loading the whole file; other files fall back to
  JSONL parsing when they are not one complete JSON document. A requested or
  predicate field that is null/missing in every scanned row is reported in
  `all_null_fields` and as a warning, so selector typos cannot silently project
  `null`. The launcher preserves slash-leading JSON Pointer selector arguments
  on Git Bash/Windows so they are not rewritten as host paths before reaching
  the native binary; it gives the same protection to slash-bearing
  `--pattern`, `--prefix`, `--contains`, and `--term` values, which MSYS would
  otherwise rewrite or collapse (`^// PART` becomes `^/ PART` without it).
- `sqlite`: run a read-only query from `--sql` or `--sql-file <file>` with row
  caps and receipt metadata. A runaway query is interrupted after
  `--timeout-secs` (default 60; 0 disables) so it fails accountably instead
  of hanging until the calling shell kills it. Prefer `--path <file>` for the
  DB path; positional DB paths and `--db <file>` remain accepted.
- `sqlite-schema`: summarize SQLite tables, columns, indexes, and foreign keys
  from SQLite metadata without hand-written PRAGMA queries. Prefer
  `--path <file>` for the DB path; positional DB paths and `--db <file>` remain
  accepted.
- `capture` (`run` alias): execute argv directly and print capped stdout/stderr
  summaries with exit status. When output exceeds the line or byte budget, the
  head and the tail are both kept with an omission marker between them, because
  tool verdicts (test summaries, error totals) live at the end of output. Use
  capture only when a command's output cardinality is unknown and the command
  lacks a better native filter or projection.

Use `--json` when another script or tool should consume the result directly.
Use `--fail-if-truncated` (aliases: `--fail-on-truncate`,
`--strict-complete`) when a capped result should stop automation after the
receipt is emitted. Use `--require-complete-scan` when display caps may be fine
but scan-capped lower-bound totals should fail.

## Text Encodings

Content scanning and slicing decode by BOM: UTF-16LE/UTF-16BE files (PowerShell
`Out-File` default on Windows) are decoded and searched instead of being
skipped as binary, and a UTF-8 BOM is stripped before JSON parsing. Files with
NUL bytes and no UTF-16 BOM are skipped as binary; UTF-8 decoding is lossy so
mixed-encoding files still surface their ASCII content.

## Nested Git Repositories

Multi-repo workspaces routinely git-ignore sibling repos for repo separation
(`.gitignore` or `.git/info/exclude` entries like `myrepo/`). A standard
git-aware walk silently skips those trees, which makes a broad scan claim
completeness it does not have. `files`, `dirs`, `grep`, and `grep-terms`
therefore enter a git-ignored directory that is itself a git repository root
(it contains `.git`), applying that repository's own ignore rules, and disclose
every entry in `nested_repos_entered` (capped list) plus
`nested_repos_entered_total`. Pass `--skip-nested-repos` to restore strict
Git-scope behavior, or exclude unwanted repos in `.contextmink.toml` (policy
stays in configuration). Repos nested more than one level below an ignored
plain directory are not auto-detected; pass them as explicit roots.

## Install

Download the release archive for your platform from
[GitHub Releases](https://github.com/remiliacorporation/contextmink/releases)
and unpack it. Put `contextmink` on your `PATH`, or run it from the unpacked
directory:

```bash
contextmink files --path . --max 20
```

Release archives are built for Windows x64, macOS Intel, macOS ARM, and Linux
x64. Each archive includes the binary, setup docs, repository instruction
templates, a manifest, and a SHA-256 checksum.

The binary is self-contained on every platform: PowerShell, cmd, WSL, and
POSIX shells all invoke it directly. Nothing in contextmink requires Git Bash,
the `scripts/contextmink` launcher, or the optional Windows bridge — those
exist only for repositories that deliberately keep their scripts Bash-first.

Release builds include bundled SQLite support for portability.

On Windows repositories that use extensionless Bash scripts, use the
project-local `scripts/contextmink` launcher below for `capture`; it supplies the
Bash interpreter needed for script fallback. The raw `contextmink.exe` is fine
for built-in commands and native executables.

## Add To A Project

Rust and Cargo are not required for release installs. For a project-local
install, unpack the release archive and copy the binary plus templates into the
target repository:

```text
target-repo/
  scripts/contextmink
  tools/contextmink/bin/contextmink(.exe)
  .contextmink.toml
```

Use the files from the release archive:

1. Copy `contextmink(.exe)` to `tools/contextmink/bin/contextmink(.exe)`.
2. Copy `templates/scripts/contextmink` to `scripts/contextmink`.
3. Copy `templates/.contextmink.toml` to `.contextmink.toml` and edit excludes.
4. Merge `templates/AGENTS.contextmink.md` into `AGENTS.md` for Codex, and/or
   `templates/CLAUDE.contextmink.md` into `CLAUDE.md` for Claude.
5. Optional, only for Bash-first repositories driven by a PowerShell-hosted
   agent on Windows: copy `contextmink-bridge.exe` (Windows archive only) next
   to the contextmink binary — a native PowerShell -> Git Bash bridge that
   discovers Git Bash itself, spawns direct commands with zero MSYS argument
   rewriting, and accepts argv through `--argv-b64` (a single base64 token
   PowerShell cannot mangle) or `--argfile`. The `templates/scripts/codex-bash.sh`
   script launcher covers the same ground for setups that prefer a shell
   entrypoint. Repositories on pure PowerShell or WSL skip this entirely and
   invoke the contextmink binary directly. See [docs/setup.md](docs/setup.md).
6. Verify from the target repository root:

   ```bash
   scripts/contextmink files --path . --max 20
   ```

For delegated setup, give the agent the unpacked release directory and target
repository path. The full checklist is in [SETUP.md](SETUP.md).

## Build From Source

```bash
cargo test
cargo build --release
target/release/contextmink files --path . --max 20
```

`contextmink` uses Rust edition 2024 and requires a recent stable Rust
toolchain only when building from source.

## Examples

```bash
scripts/contextmink files --path . --max 20
scripts/contextmink files --path . --max 20 --max-scan-files 5000
scripts/contextmink files --path vendor --with-git-ignored --max 20
scripts/contextmink files --path specs/_assets --with-git-ignored --ext json --max 20
scripts/contextmink dirs crates --depth 2 --max 40
scripts/contextmink grep --pattern-file pattern.txt src tests --limit 8
scripts/contextmink grep CMapChunk src --ext rs --context 2 --limit 8
scripts/contextmink grep-terms --term "--flag-like" --term "panic" --or src --max-matches 12
scripts/contextmink outline src/renderer.rs
scripts/contextmink outline src/renderer.rs --contains cull -i
scripts/contextmink outline vendor/header.h --lang c --limit 60
scripts/contextmink outline notes/pseudocode.h --prefix '// PART'
scripts/contextmink slice src/main.rs --range 120:180
scripts/contextmink slice build.log --tail 40
scripts/contextmink json-find report.json --key-contains error --max 10
scripts/contextmink json-select report.json --array /rows --field id --field /status
scripts/contextmink json-select queue.jsonl --field addr --where-contains name=CMap --limit 10
scripts/contextmink sqlite --path state.sqlite --sql-file query.sql --max-rows 20
scripts/contextmink sqlite-schema --path state.sqlite --name-contains user --max-tables 8
scripts/contextmink capture --max-lines 40 -- some-tool --compact-target query
scripts/contextmink --fail-if-truncated run --max-lines 40 -- some-tool --compact-target query
```

## Receipts

Every human-readable command ends with `CONTEXTMINK_RECEIPT ` followed by JSON.
If a receipt has `"truncated": true` or `"complete": false`, the output is
capped. Narrow the path, glob, pattern, or slice and run again.
With strict completion flags, contextmink still emits the receipt and then exits
nonzero when the requested completeness condition fails.

Stable receipt fields:

| field | meaning |
| --- | --- |
| `tool` | always `"contextmink"` |
| `command` | subcommand that ran |
| `profile` | active `.contextmink.toml` profile, or `null` |
| `unit` | what `shown` and `total` count |
| `shown` | items printed, in `unit` |
| `total` | items available, in `unit` |
| `truncated` | whether output was capped |
| `complete` | `!truncated` |
| `cap_reason` | why output stopped, or `null` |
| `duration_ms` | wall-clock cost of the command |

For `grep` and `grep-terms`, `shown` and `total` are file counts and
`total_matches` counts matching lines. Match, sample, scan, and skip counts are
reported in dedicated fields; `skipped_files_sample` names the first files that
were skipped as too large or binary, and a no-match verdict reports
`no_match_scope: "scanned_subset"` whenever large files went unexamined. If
`matched_files_total_is_lower_bound` or `total_matches_is_lower_bound` is true,
the content scan stopped at `--max-count-files`; narrow the query or raise that
cap before treating match totals as exact.
When `cap_reason` is `"scan"`, `candidate_files_total_is_lower_bound` is true,
or grep match-total lower-bound fields are true, totals and no-match results
only describe the scanned subset. Narrow the path/glob/query before treating the
result as complete.
Grep receipts also include `no_match_scope` (`"complete_scope"` or
`"scanned_subset"`) when no files match.

For `capture`, `shown` and `total` are stdout plus stderr line counts; each
stream reports `head_lines`, `tail_lines`, and `omitted_lines` when output was
split around an omission marker. The receipt records the child command's
`exit_code` and `success`; `contextmink` itself exits successfully when capture
succeeds, even if the child command failed. `capture` is not a shell, sandbox, retry layer, or read-only guard. On
Windows through the Bash launcher, extensionless shell scripts that fail direct
spawn with "not a Win32 application" are retried through the current Bash
interpreter as argv, not as a shell string; receipts include `spawn_fallback`
and `effective_argv` when that happens.

## Configuration

`contextmink` searches upward from the current directory for
`.contextmink.toml`:

```toml
profile = "repo-name"

exclude_globs = [
  "target/**",
  "**/target/**",
  "node_modules/**",
  "**/node_modules/**",
]
```

The configuration surface is exactly these two keys; unknown keys, duplicate
keys, and malformed values are hard errors so a config typo cannot silently
change scan scope. Exclude globs are matched against paths relative to the
config file's directory, so anchored rules like `Data/**` hold even when a
scan root is passed as an absolute path or the command runs from a
subdirectory.

Keep repository policy in `.contextmink.toml` and repository instructions, not in the
binary. Exclude generated or high-output trees from broad scans, then pass an
explicit subdirectory or file when that tree is the target.
`--with-excluded` includes files matched by contextmink's built-in and
configured exclude globs for the whole command. It does not disable Git ignore
rules; pass an explicit path when an ignored artifact tree is the target.

## Scope

Add to this tool only when the failure mode is generic transcript overflow or
host-shell friction from file enumeration, text search, line slicing, JSON
inspection, read-only SQLite inspection/schema summarization, or bounded capture
of otherwise unknown command output. If behavior needs domain knowledge, a
schema beyond the data being selected, a compiler, an indexer, a runtime, or a
specialized parser, extend that domain tool instead.

## License

MIT. See [LICENSE](LICENSE). The distribution also carries
[LICENSE-SSL](LICENSE-SSL) and [LICENSE-VPL](LICENSE-VPL); both accompany every
release archive and mirror sync.
