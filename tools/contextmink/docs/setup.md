# Setting Up contextmink In A Repository

This guide is for adding `contextmink` to an existing repository.

`contextmink` is a transcript guard. Use it before broad file, text, line-slice,
JSON, read-only SQLite, or unknown-size command-output reads when the output
cardinality is unknown, when a known file must be navigated beyond one bounded
window, or when host-shell quoting would become the task. It is not a
replacement for project-native tools.

## Prerequisites

- For standalone use, download the release archive for your platform and put
  `contextmink` on `PATH`, or run it from the unpacked directory.
- Rust and Cargo are needed only for source builds or vendored integrations that
  build the local `tools/contextmink` copy. `contextmink` uses Rust edition
  2024.
- A POSIX-compatible shell is needed only for the optional `scripts/contextmink`
  launcher. On Windows, Git Bash works. Without Bash, call the release binary
  directly or use `cargo run --manifest-path tools/contextmink/Cargo.toml -- ...`.
  For `capture` of extensionless repository scripts on Windows, use the launcher;
  it supplies the Bash interpreter needed for script fallback.

## Release Archives

Release archives are published at
<https://github.com/remiliacorporation/contextmink/releases>. Download the
archive for the host platform:

- `contextmink-<version>-windows-x86_64.zip`
- `contextmink-<version>-macos-x86_64.tar.gz`
- `contextmink-<version>-macos-arm64.tar.gz`
- `contextmink-<version>-linux-x86_64.tar.gz`

Each archive includes:

```text
contextmink(.exe)
README.md
SETUP.md
docs/
templates/
manifest.json
LICENSE
LICENSE-SSL
LICENSE-VPL
```

Verify the adjacent `.sha256` checksum when the archive was downloaded through
automation or mirrored storage.

## Standalone Binary Install

This installs `contextmink` on `PATH` instead of vendoring it per repository:

1. Unpack the release archive.
2. Put `contextmink(.exe)` on `PATH`, or run it from the unpacked directory.
3. Verify:

   ```bash
   contextmink files --path . --max 20
   ```

The binary can use a repository-local `.contextmink.toml`; it searches upward
from the current directory.

On Windows, direct `contextmink.exe` can run built-in commands and native
executables. Use Project Binary Integration when `capture` needs to run
extensionless Bash scripts from the repository.

## Project Binary Integration

This gives a target repository a local `scripts/contextmink` entrypoint without
a source build.

Use this layout for Windows repositories that expect agents to run `capture`
around extensionless Bash scripts. The launcher supplies the Bash interpreter
for script fallback.

1. Unpack the release archive next to, or outside, the target repository.

2. In the target repository, create the local binary directory:

   ```bash
   mkdir -p tools/contextmink/bin scripts
   ```

3. Copy the release binary into the target repository:

   ```bash
   cp /path/to/unpacked/contextmink tools/contextmink/bin/contextmink
   # Windows binary name:
   # cp /path/to/unpacked/contextmink.exe tools/contextmink/bin/contextmink.exe
   ```

4. Copy the release launcher:

   ```bash
   cp /path/to/unpacked/templates/scripts/contextmink scripts/contextmink
   chmod +x scripts/contextmink
   ```

5. Copy and edit the config:

   ```bash
   cp /path/to/unpacked/templates/.contextmink.toml .contextmink.toml
   ```

   Keep only repo-local high-output paths. Good candidates include generated
   build directories, vendored dependencies, caches, exported reports, large
   binary asset trees, and tool output directories. These excludes keep broad
   scans quiet; callers can still pass an explicit file or subdirectory inside
   an excluded tree when that tree is the target.

6. Merge repository guidance:

   - Codex: merge `templates/AGENTS.contextmink.md` into the target
     repository's `AGENTS.md` or equivalent Codex guidance file.
   - Claude: merge `templates/CLAUDE.contextmink.md` into the target
     repository's `CLAUDE.md` or equivalent Claude guidance file.

7. Verify from the target repository root:

   ```bash
   scripts/contextmink files --path . --max 20
   scripts/contextmink grep contextmink --path . --limit 5
   ```

Delegated setup prompt:

```text
Set up contextmink in <target-repo> from the unpacked release at <path>. Use
the release binary, not a source build. Install
tools/contextmink/bin/contextmink(.exe), scripts/contextmink, and
.contextmink.toml with repo-appropriate high-output excludes. Merge the
AGENTS/CLAUDE contextmink snippet into the project guidance. Verify with
scripts/contextmink files --path . --max 20.
```

## Optional: PowerShell -> Git Bash Bridge (Windows + Codex-style hosts)

Nothing in contextmink requires a bridge, Git Bash, or the
`scripts/contextmink` launcher: the contextmink binary is native on every
platform, and PowerShell, cmd, and WSL invoke it directly. Skip this section
unless the repository deliberately keeps its scripts Bash-first for
cross-platform authoring and is driven by a PowerShell-hosted agent.

Such repositories have two bridge options. Both exist because Windows has two
distinct argv hazards; neither hazard exists on POSIX hosts, where agents
should run bash and commands directly.

**Native binary (preferred on Windows).** The Windows release archive carries
`contextmink-bridge.exe`. It locates Git Bash itself (no hardcoded path on the
agent side), spawns direct commands natively with zero MSYS argument
rewriting, and accepts argv through channels PowerShell cannot mangle:

```powershell
# Direct command; slash-bearing args arrive verbatim:
& tools\contextmink\bin\contextmink-bridge.exe -- <program> <args...>
# Repository bash script, Git Bash discovered automatically:
& tools\contextmink\bin\contextmink-bridge.exe --script scripts/some_tool.sh <args...>
# Lossless single-token argv channel (immune to PowerShell 5.1 quote loss):
$argv = @('grep', '-n', 'he said "hi"', 'notes.md')
$b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($argv -join [char]0)))
& tools\contextmink\bin\contextmink-bridge.exe --argv-b64 $b64
```

`--print-argv` shows exactly what survived the PowerShell boundary;
`--argfile <file>` (one argument per line) is the file-based alternative;
`--cwd` and `--login` work as in the script bridge. Relative paths resolve
from `CONTEXTMINK_BRIDGE_ROOT`, else the first ancestor of the binary with
`.git` or `.contextmink.toml`.

**Script launcher (bash-first setups).** Install the template when the
repository prefers a shell entrypoint or must not carry a second binary:

```bash
cp /path/to/unpacked/templates/scripts/codex-bash.sh scripts/codex-bash.sh
chmod +x scripts/codex-bash.sh
```

The agent then invokes Git Bash by absolute path with argv-safe forms instead
of `bash -lc "<string>"`:

```powershell
& "C:\Program Files\Git\bin\bash.exe" scripts/codex-bash.sh -- <program> <args...>
& "C:\Program Files\Git\bin\bash.exe" scripts/codex-bash.sh --script scripts/some_tool.sh <args...>
```

The bridge handles the two distinct Windows argv hazards separately:

- PowerShell 5.1 drops embedded quotes and merges arguments at the
  PowerShell -> bash boundary. Diagnose with `--print-argv`; pass fragile
  argv through `--argfile <file>` (one argument per line, no quoting).
- MSYS rewrites or collapses slash-bearing arguments (regex patterns,
  POSIX-looking switches) at the bash -> native-exe boundary. Pass
  `--no-msys-conversion` when a native child must receive argv verbatim.

`--cwd <dir>` selects the working directory, `--login` supplies a login-shell
environment argv-safely, and a built-in warn-only trip-wire flags raw content
dumps (`sed -n` windows, `cat` on large files) toward `contextmink
outline`/`slice`. Without a command form the launcher opens an interactive
shell only on a real terminal; a headless agent gets a usage error instead of
a hang.

## Source Vendored Integration

Use this pattern only when the target repository should carry and build its own
copy of the Rust crate:

1. Copy this repository's Rust crate into the target repository at
   `tools/contextmink/`.

2. Copy `templates/scripts/contextmink` to `scripts/contextmink`.

   Preserve the executable bit on Unix-like systems:

   ```bash
   chmod +x scripts/contextmink
   ```

   The launcher uses `tools/contextmink/target/release/contextmink(.exe)` when
   it builds from source. For release binary installs, use Project Binary
   Integration instead.

3. Copy `templates/.contextmink.toml` to `.contextmink.toml`, then edit it.

   Keep only repo-local high-output paths. Good candidates include generated build
   directories, vendored dependencies, caches, exported reports, large binary
   asset trees, and tool output directories. These excludes keep broad scans
   quiet; callers can still pass an explicit file or subdirectory inside an
   excluded tree when that tree is the target.

4. Add the instruction snippet for the tool surface the target repository uses:

   - Codex: copy `templates/AGENTS.contextmink.md` into the repository's
     `AGENTS.md` or equivalent Codex guidance file.
   - Claude: copy `templates/CLAUDE.contextmink.md` into the repository's
     `CLAUDE.md` or equivalent Claude guidance file.

   The two snippets are intentionally equivalent in policy. Keep any
   repository-specific shell or path guidance in the target repository, not in
   these templates.

5. Verify the integration from the target repository root:

   ```bash
   scripts/contextmink files --path . --max 20
   scripts/contextmink grep contextmink --path . --limit 5
   ```

   The first source-backed run may build the release binary. Build output is
   sent to stderr so stdout remains parseable. Release builds include bundled
   SQLite support so read-only DB inspection works without a system SQLite
   install.

## Source Install

Use this for local development or when a release archive is not available for
the host:

```bash
cargo install --path .
contextmink files --path . --max 20
```

## Config Template

Start from:

```toml
profile = "repo-name"

exclude_globs = [
  "target/**",
  "**/target/**",
  "node_modules/**",
  "**/node_modules/**",
  ".venv/**",
  "**/.venv/**",
]
```

The binary already excludes common high-output paths such as `.git`, `target`,
`node_modules`, and `.venv`. Include them in repo configs only if doing so makes
the local policy clearer for future maintainers.

## Instruction Rule

Use the maintained snippets rather than copying setup prose into project
guidance:

- `templates/AGENTS.contextmink.md` for Codex-facing guidance.
- `templates/CLAUDE.contextmink.md` for Claude-facing guidance.

Tests keep the two snippets equivalent so Codex and Claude guidance do not
drift.

The snippets invoke the repo-local `scripts/contextmink` launcher form.
Repositories that skip the Bash launcher (pure PowerShell or WSL setups)
should replace those references with direct `contextmink` binary invocation
when merging; the policy content is shell-agnostic.

Do not create a separate contextmink skill or slash command by default.
Put the bounded-output rule in always-loaded project guidance so it applies
before broad reads start. Use host-specific integration only when the host
requires it.

## Operational Notes

- Start with `dirs --depth 2` to orient in an unfamiliar tree before running
  `files` or `grep`.
- For extension filtering, prefer `files --ext json` / `--extension jsonl`
  (also available on `grep` and `grep-terms`) over wildcard globs when
  commands cross Windows-to-Bash boundaries. Wildcards can be expanded before
  contextmink receives them.
- Prefer `grep --pattern-file <file>` when regex punctuation is shell-fragile.
  Add `-i` for case-insensitive matching and `--context N` when the
  surrounding lines would otherwise need a follow-up `slice`.
- Broad scans enter git-ignored nested repository roots and disclose them in
  `nested_repos_entered`; pass `--skip-nested-repos` for strict Git scope, or
  exclude unwanted repos in `.contextmink.toml`.
- For `grep` and `grep-terms`, use `--limit` to cap printed matching files,
  `--max-matches` / `--max-lines` to cap printed sample match lines, and
  `--max-count-files` only when it is acceptable for match totals to become
  lower bounds after enough matching files are found.
- Pass an explicit file or subdirectory for artifact lookups inside configured
  high-output trees. Use `--with-excluded` only when the whole command should
  include files matched by contextmink's built-in and configured exclude globs.
  It does not disable Git ignore rules.
- Use `--with-git-ignored` only when intentionally inspecting a Git-ignored
  vendor, cache, or artifact tree. Contextmink exclude globs still apply unless
  `--with-excluded` is also set.
- Prefer `grep-terms --term-file <file>` when phrases contain shell-fragile
  punctuation or spaces.
- Read known files through `outline` then `slice` instead of dump windows:
  `outline <file>` maps declaration lines with line numbers (`--contains TEXT`
  filters rows; `--lang`, `--prefix <text>`, or `--pattern <regex>` cover
  unrecognized extensions), then `slice --range START:END` prints the located
  region. Use `slice --tail N` for the end of logs. Keep slice's default caps;
  narrow an oversized read with `outline` or `grep --context` instead of
  raising `--max-lines`.
- Prefer `json-find` over opening whole JSON reports, and `json-select` for
  bounded row/field projection from JSON arrays or JSONL row files
  (`--where FIELD=VALUE` / `--where-contains FIELD=TEXT` filter rows; a field
  reported in `all_null_fields` is a selector typo or shape mismatch, not
  evidence).
- On Git Bash/Windows, use the `scripts/contextmink` launcher for
  `json-select` and for slash-bearing `--pattern` / `--prefix` / `--contains`
  / `--term` values; it preserves slash-leading JSON Pointer selectors and
  shields those values from MSYS path rewriting while still leaving normal
  file path handling to the shell/runtime boundary.
- Prefer `sqlite-schema --path <db>` before ad hoc SQLite queries against
  unfamiliar databases.
- Prefer `sqlite --path <db> --sql-file <file>` for read-only SQL containing
  shell-fragile operators or quotes.
- Prefer a domain command's native compact/projection/limit flags first. Use
  `capture -- <command> ...` only when output size is uncertain and no better
  native bound exists; read the child `exit_code`/`success` fields in the
  receipt. Truncated captures keep both the head and the tail of the output.
  On Windows, the Bash launcher lets `capture` retry extensionless shell
  scripts through the current Bash interpreter without shell-string parsing.
- Treat capped receipts as `complete: false`. When `cap_reason` is `scan` or
  `candidate_files_total_is_lower_bound` is true, totals and no-match results
  only describe the scanned subset; narrow and rerun.
- Keep repository-specific policy in `.contextmink.toml` and repository
  instructions, not in `contextmink` source code.

## Maintenance

For a vendored copy, compare or sync only the generic surface:

```text
tools/contextmink/src/
tools/contextmink/tests/
tools/contextmink/Cargo.toml
tools/contextmink/Cargo.lock
tools/contextmink/README.md
tools/contextmink/SETUP.md
tools/contextmink/docs/
tools/contextmink/scripts/
tools/contextmink/templates/
tools/contextmink/.github/
tools/contextmink/.gitignore
tools/contextmink/LICENSE
tools/contextmink/LICENSE-SSL
tools/contextmink/LICENSE-VPL
```

Do not sync a target repository's `.contextmink.toml`; that file is local
policy.
