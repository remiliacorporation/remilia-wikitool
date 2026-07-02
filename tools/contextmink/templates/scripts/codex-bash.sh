#!/usr/bin/env bash
set -euo pipefail

# Generic PowerShell -> Git Bash bridge for repositories whose scripts are
# Bash-first. A PowerShell-hosted agent invokes this launcher through the Git
# Bash executable so project commands run with POSIX semantics and argv-safe
# argument passing (never `bash -lc "<string>"` re-parsing).
#
# Boundary model — two distinct hazards, two distinct fixes:
#   1. PowerShell -> bash argv (lossy embedded quotes in PS 5.1): pass fragile
#      arguments through --argfile instead of the command line.
#   2. bash -> native .exe argv (MSYS rewrites or collapses slash-bearing
#      arguments such as regex `^// PART` or POSIX-looking paths): pass
#      --no-msys-conversion when the child must receive argv verbatim.
# Use --print-argv to see exactly what survived boundary 1 before blaming
# boundary 2.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_CWD="$ROOT_DIR"

usage() {
  cat >&2 <<'EOF'
Usage:
  codex-bash.sh [flags] -- <program> [args...]
  codex-bash.sh [flags] --script <script> [args...]
  codex-bash.sh [flags] --argfile <file>
  codex-bash.sh                              (interactive shell; tty only)

Flags (must precede the command form):
  --cwd <dir>            Working directory; relative paths resolve from the
                         workspace root (the launcher's parent directory).
  --login                Run the command with a login-shell environment
                         (profile PATH etc.) while staying argv-safe.
  --no-msys-conversion   Export MSYS2_ARG_CONV_EXCL='*' so a native child
                         receives slash-bearing arguments verbatim instead of
                         MSYS-rewritten (e.g. regex patterns, POSIX-looking
                         switches).
  --print-argv           Print the assembled argv one entry per line and exit;
                         use to debug the PowerShell -> bash quoting boundary.
  --argfile <file>       Read the program argv from a UTF-8 file, one argument
                         per line, no quoting or escaping (a UTF-8 BOM on the
                         first line and trailing CRs are stripped). Escape
                         hatch for arguments PowerShell quoting would mangle.

Runs project commands from Git Bash without routing command text through
bash -lc. Exit codes: 64 usage, 66 missing path, 127 no bash; otherwise the
child's exit code.
EOF
}

resolve_workspace_path() {
  local raw="$1"
  case "$raw" in
    /* | [A-Za-z]:/* | [A-Za-z]:\\*) printf '%s\n' "$raw" ;;
    *) printf '%s/%s\n' "$ROOT_DIR" "$raw" ;;
  esac
}

# Transcript trip-wire: warn (never block) when argv is a raw content dump a
# bounded read would serve better. Threshold tracks the contextmink slice
# window guidance in the repository's bounded-output instructions; set
# CODEX_BASH_SUPPRESS_DUMP_WARNING=1 to silence deliberate wide reads.
DUMP_WARN_LINES=150

warn_content_dump() {
  if [[ "${CODEX_BASH_SUPPRESS_DUMP_WARNING:-0}" == 1 || $# -eq 0 ]]; then
    return 0
  fi
  local prog arg lines
  prog="$(basename "$1")"
  shift
  case "$prog" in
    sed | sed.exe)
      for arg in "$@"; do
        if [[ "$arg" =~ ^(-n)?([0-9]+),([0-9]+)p$ ]]; then
          local span=$((BASH_REMATCH[3] - BASH_REMATCH[2] + 1))
          if ((span > DUMP_WARN_LINES)); then
            echo "codex-bash: sed window of ${span} lines is a transcript dump; prefer scripts/contextmink outline <file> then slice --range START:END (CODEX_BASH_SUPPRESS_DUMP_WARNING=1 silences)" >&2
          fi
        fi
      done
      ;;
    cat | cat.exe | nl | nl.exe)
      for arg in "$@"; do
        if [[ "$arg" == -* || ! -f "$arg" ]]; then
          continue
        fi
        lines="$(wc -l <"$arg" 2>/dev/null || echo 0)"
        if ((lines > DUMP_WARN_LINES)); then
          echo "codex-bash: $prog on $arg (${lines} lines) is a transcript dump; prefer scripts/contextmink outline/slice (CODEX_BASH_SUPPRESS_DUMP_WARNING=1 silences)" >&2
        fi
      done
      ;;
    head | head.exe | tail | tail.exe)
      local expect_count=0
      for arg in "$@"; do
        lines=""
        if ((expect_count)); then
          lines="$arg"
          expect_count=0
        elif [[ "$arg" == "-n" || "$arg" == "--lines" ]]; then
          expect_count=1
          continue
        elif [[ "$arg" =~ ^(-n|--lines=|-)([0-9]+)$ ]]; then
          lines="${BASH_REMATCH[2]}"
        fi
        if [[ "$lines" =~ ^[0-9]+$ ]] && ((lines > DUMP_WARN_LINES)); then
          echo "codex-bash: $prog -n ${lines} is a transcript dump; prefer scripts/contextmink outline/slice (CODEX_BASH_SUPPRESS_DUMP_WARNING=1 silences)" >&2
        fi
      done
      ;;
  esac
  return 0
}

login_shell=0
no_msys_conversion=0
print_argv=0
argfile=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --cwd)
      shift
      if [[ $# -lt 1 ]]; then
        echo "codex-bash: --cwd requires a directory" >&2
        exit 64
      fi
      TARGET_CWD="$(resolve_workspace_path "$1")"
      shift
      ;;
    --login)
      login_shell=1
      shift
      ;;
    --no-msys-conversion)
      no_msys_conversion=1
      shift
      ;;
    --print-argv)
      print_argv=1
      shift
      ;;
    --argfile)
      shift
      if [[ $# -lt 1 ]]; then
        echo "codex-bash: --argfile requires a file" >&2
        exit 64
      fi
      argfile="$(resolve_workspace_path "$1")"
      shift
      ;;
    --help | -h)
      usage
      exit 0
      ;;
    --script | --)
      break
      ;;
    *)
      echo "codex-bash: unknown argument: $1 (use -- to separate the command, or --help)" >&2
      exit 64
      ;;
  esac
done

if [[ ! -d "$TARGET_CWD" ]]; then
  echo "codex-bash: working directory not found: $TARGET_CWD" >&2
  exit 66
fi

cd "$TARGET_CWD"

# Assemble the child argv from exactly one command form.
mode=""
cmd=()

if [[ -n "$argfile" ]]; then
  if [[ "${1:-}" == "--script" || "${1:-}" == "--" ]]; then
    echo "codex-bash: --argfile cannot be combined with --script or --" >&2
    exit 64
  fi
  if [[ ! -f "$argfile" ]]; then
    echo "codex-bash: argfile not found: $argfile" >&2
    exit 66
  fi
  mode="argv"
  first_line=1
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line%$'\r'}"
    if [[ "$first_line" -eq 1 ]]; then
      line="${line#$'\xef\xbb\xbf'}"
      first_line=0
    fi
    cmd+=("$line")
  done <"$argfile"
  if [[ "${#cmd[@]}" -eq 0 ]]; then
    echo "codex-bash: argfile is empty: $argfile" >&2
    exit 64
  fi
elif [[ "${1:-}" == "--script" ]]; then
  shift
  if [[ $# -lt 1 ]]; then
    echo "codex-bash: --script requires a script path" >&2
    exit 64
  fi
  script_path="$(resolve_workspace_path "$1")"
  shift
  if [[ ! -f "$script_path" ]]; then
    echo "codex-bash: script not found: $script_path" >&2
    exit 66
  fi
  mode="script"
  cmd=("$script_path" "$@")
elif [[ "${1:-}" == "--" ]]; then
  shift
  if [[ $# -eq 0 ]]; then
    echo "codex-bash: -- requires a command" >&2
    exit 64
  fi
  mode="argv"
  cmd=("$@")
fi

if [[ -n "$mode" ]]; then
  if [[ "$print_argv" -eq 1 ]]; then
    index=0
    for arg in "${cmd[@]}"; do
      printf 'argv[%d]=%s\n' "$index" "$arg"
      index=$((index + 1))
    done
    exit 0
  fi
  if [[ "$no_msys_conversion" -eq 1 ]]; then
    export MSYS2_ARG_CONV_EXCL='*'
  fi
  if [[ "$mode" == "script" ]]; then
    if [[ "$login_shell" -eq 1 ]]; then
      exec bash --login "${cmd[@]}"
    fi
    exec bash "${cmd[@]}"
  fi
  warn_content_dump "${cmd[@]}"
  if [[ "$login_shell" -eq 1 ]]; then
    # Constant -c text; the user command rides in as positional parameters,
    # so no command text is shell-reparsed.
    exec bash --login -c 'exec "$@"' bash "${cmd[@]}"
  fi
  exec "${cmd[@]}"
fi

if [[ "$print_argv" -eq 1 || "$login_shell" -eq 1 || "$no_msys_conversion" -eq 1 ]]; then
  echo "codex-bash: flags require a command form (--, --script, or --argfile)" >&2
  exit 64
fi

# No command form: open an interactive shell, but only on a real terminal —
# a headless agent reaching this point would hang forever on a hidden prompt.
if [[ ! -t 0 || ! -t 1 ]]; then
  echo "codex-bash: no command given and stdin/stdout is not a terminal; refusing to start an interactive shell" >&2
  usage
  exit 64
fi

if [[ "${MSYSTEM:-}" == MSYS* || "${MSYSTEM:-}" == MINGW* || "${MSYSTEM:-}" == CYGWIN* || "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == mingw* || "${OSTYPE:-}" == cygwin* ]]; then
  exec "${SHELL:-bash}" --login -i
fi

for candidate in \
  "${PROGRAMFILES:-}/Git/bin/bash.exe" \
  "C:/Program Files/Git/bin/bash.exe" \
  "C:/cygwin64/bin/bash.exe" \
  "C:/msys64/usr/bin/bash.exe"
do
  if [[ -x "$candidate" ]]; then
    exec "$candidate" --login -i
  fi
done

echo "codex-bash: unable to find a direct Git Bash executable" >&2
echo "codex-bash: expected one of:" >&2
echo "  C:/Program Files/Git/bin/bash.exe" >&2
echo "  C:/cygwin64/bin/bash.exe" >&2
echo "  C:/msys64/usr/bin/bash.exe" >&2
exit 127
