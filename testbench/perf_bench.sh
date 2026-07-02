#!/usr/bin/env bash
# wikitool performance benchmark harness.
#
# Times end-to-end CLI scenarios against a real wikitool project root and prints a
# markdown baseline table. Every optimization claim must show its before/after on
# this table (see AGENTS.md).
#
# The project root MUST be a disposable copy of a real project: scenarios rewrite
# pulled content, temporarily modify articles for push conflict checks, and (in the
# live tier) delete and rebuild the runtime database. Never point this at a working
# checkout you care about.
#
# Usage:
#   testbench/perf_bench.sh --project-root <disposable-project-copy> [options]
#
# Options:
#   --project-root <path>   Required. Disposable project copy to benchmark against.
#   --tier <local|live>     local (default) runs only offline scenarios; live adds
#                           network scenarios (session-refresh, pull, push dry-run).
#   --runs <n>              Timed runs per local scenario (default 3; live always 1).
#   --scenarios <a,b,c>     Comma-separated scenario filter (default: all in tier).
#   --topic <title>         Article-start topic (default "Milady Maker").
#   --output <file>         Also append the markdown table to <file>.
#
# Env:
#   WIKITOOL                Override the wikitool command (default: release binary
#                           next to this script's repo, else cargo run --quiet --).
#
# Scenarios (local): status-modified, diff, lint-corpus, article-start-brief,
#   fts-search, knowledge-build, knowledge-warm
# Scenarios (live adds): session-refresh-warm, session-refresh-cold, pull-full-all,
#   push-conflict-dryrun

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

PROJECT_ROOT=""
TIER="local"
RUNS=3
SCENARIO_FILTER=""
TOPIC="Milady Maker"
OUTPUT_FILE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --project-root) PROJECT_ROOT="$2"; shift 2 ;;
        --tier) TIER="$2"; shift 2 ;;
        --runs) RUNS="$2"; shift 2 ;;
        --scenarios) SCENARIO_FILTER="$2"; shift 2 ;;
        --topic) TOPIC="$2"; shift 2 ;;
        --output) OUTPUT_FILE="$2"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [ -z "$PROJECT_ROOT" ]; then
    echo "error: --project-root is required (a DISPOSABLE project copy)" >&2
    exit 2
fi
if [ ! -d "$PROJECT_ROOT/.wikitool" ]; then
    echo "error: $PROJECT_ROOT has no .wikitool/ directory; not a wikitool project" >&2
    exit 2
fi
case "$TIER" in local|live) ;; *) echo "error: --tier must be local or live" >&2; exit 2 ;; esac

resolve_wikitool() {
    if [ -n "${WIKITOOL:-}" ]; then
        echo "$WIKITOOL"
        return
    fi
    for candidate in "$REPO_ROOT/target/release/wikitool.exe" "$REPO_ROOT/target/release/wikitool"; do
        if [ -x "$candidate" ]; then
            echo "$candidate"
            return
        fi
    done
    echo "cargo run --quiet --"
}
WT_CMD="$(resolve_wikitool)"

# Convert a Git Bash path to the form the binary expects on Windows.
to_native_path() {
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) cygpath -m "$1" ;;
        *) echo "$1" ;;
    esac
}
NATIVE_ROOT="$(to_native_path "$PROJECT_ROOT")"

wt() {
    # shellcheck disable=SC2086
    (cd "$PROJECT_ROOT" && $WT_CMD --project-root "$NATIVE_ROOT" "$@")
}

now_ns() { date +%s%N; }

declare -a RESULT_ROWS=()

# time_scenario <name> <runs> <notes> -- <command...>
# Records median/min/max wall time in ms. A nonzero exit fails the whole bench:
# a scenario that errors is not a measurement.
time_scenario() {
    local name="$1" runs="$2" notes="$3"
    shift 3
    [ "$1" = "--" ] && shift
    local -a samples=()
    local i start end elapsed_ms
    for i in $(seq 1 "$runs"); do
        start=$(now_ns)
        "$@" >/dev/null
        end=$(now_ns)
        elapsed_ms=$(( (end - start) / 1000000 ))
        samples+=("$elapsed_ms")
        echo "  run $i/$runs: ${elapsed_ms} ms" >&2
    done
    local sorted median min max
    sorted=$(printf '%s\n' "${samples[@]}" | sort -n)
    median=$(printf '%s\n' "$sorted" | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}')
    min=$(printf '%s\n' "$sorted" | head -n1)
    max=$(printf '%s\n' "$sorted" | tail -n1)
    RESULT_ROWS+=("| $name | $runs | $median | $min | $max | $notes |")
}

wants() {
    local name="$1"
    if [ -z "$SCENARIO_FILTER" ]; then
        return 0
    fi
    case ",$SCENARIO_FILTER," in
        *",$name,"*) return 0 ;;
        *) return 1 ;;
    esac
}

section() { echo "" >&2; echo "== $1 ==" >&2; }

# --- Corpus titles file for lint-corpus (Main namespace, non-redirect) ---
TITLES_FILE=""
build_titles_file() {
    local db="$PROJECT_ROOT/.wikitool/data/wikitool.db"
    if ! command -v sqlite3 >/dev/null 2>&1; then
        echo "sqlite3 not found; skipping lint-corpus" >&2
        return 1
    fi
    if [ ! -f "$db" ]; then
        echo "no runtime db at $db; skipping lint-corpus" >&2
        return 1
    fi
    TITLES_FILE="$(mktemp)"
    # Ledger entries can outlive their local files (deleted_local); lint only what exists.
    while IFS=$'\t' read -r title rel; do
        rel="${rel%$'\r'}"
        [ -f "$PROJECT_ROOT/$rel" ] && printf '%s\n' "$title"
    done < <(sqlite3 -separator $'\t' "$db" \
        "SELECT title, relative_path FROM sync_ledger_pages WHERE namespace = 0 AND is_redirect = 0 ORDER BY title") \
        > "$TITLES_FILE"
    local count
    count=$(wc -l < "$TITLES_FILE")
    if [ "$count" -eq 0 ]; then
        echo "sync ledger has no main-namespace pages; skipping lint-corpus" >&2
        return 1
    fi
    echo "lint-corpus: $count titles" >&2
}

cleanup() {
    [ -n "$TITLES_FILE" ] && rm -f "$TITLES_FILE"
    restore_push_modifications || true
}
trap cleanup EXIT

# --- push-conflict-dryrun setup: modify N articles, restore afterwards ---
PUSH_BACKUP_DIR=""
modify_articles_for_push() {
    local n="$1"
    PUSH_BACKUP_DIR="$(mktemp -d)"
    local modified=0
    local f rel
    while IFS= read -r f; do
        [ "$modified" -ge "$n" ] && break
        rel="${f#"$PROJECT_ROOT"/}"
        mkdir -p "$PUSH_BACKUP_DIR/$(dirname "$rel")"
        cp "$f" "$PUSH_BACKUP_DIR/$rel"
        printf '\n<!-- perf-bench conflict-check marker -->\n' >> "$f"
        modified=$((modified + 1))
    done < <(find "$PROJECT_ROOT/wiki_content/Main" -maxdepth 1 -name '*.wiki' | sort)
    echo "push-conflict-dryrun: modified $modified articles" >&2
}

restore_push_modifications() {
    if [ -z "$PUSH_BACKUP_DIR" ] || [ ! -d "$PUSH_BACKUP_DIR" ]; then
        return 0
    fi
    (cd "$PUSH_BACKUP_DIR" && find . -type f -name '*.wiki' -print0) | \
    while IFS= read -r -d '' rel; do
        cp "$PUSH_BACKUP_DIR/$rel" "$PROJECT_ROOT/$rel"
    done
    rm -rf "$PUSH_BACKUP_DIR"
    PUSH_BACKUP_DIR=""
}

echo "wikitool perf bench" >&2
echo "  command: $WT_CMD" >&2
echo "  project root: $NATIVE_ROOT" >&2
echo "  tier: $TIER, runs: $RUNS" >&2

# --- Local scenarios ---

if wants status-modified; then
    section "status-modified"
    time_scenario "status-modified" "$RUNS" "local; full corpus scan" -- \
        wt status --modified --format json
fi

if wants diff; then
    section "diff"
    time_scenario "diff" "$RUNS" "local; sync plan only" -- \
        wt diff
fi

if wants lint-corpus; then
    section "lint-corpus"
    if build_titles_file; then
        NATIVE_TITLES="$(to_native_path "$TITLES_FILE")"
        # Lint exits nonzero when the corpus has findings; that is still a valid
        # timed run. Require the batch report so hard errors keep failing the bench.
        lint_corpus_run() {
            local out
            out="$(wt article lint --titles-file "$NATIVE_TITLES" --format json 2>&1 || true)"
            # Substring test, not `printf | grep -q`: under pipefail, grep -q's early
            # exit SIGPIPEs printf and misreports a matching report as a failure.
            if [[ "$out" != *'"target_count"'* ]]; then
                printf '%s\n' "$out" | tail -n 40 >&2
                return 1
            fi
        }
        time_scenario "lint-corpus" "$RUNS" "local; all main-ns non-redirect titles" -- \
            lint_corpus_run
    fi
fi

if wants article-start-brief; then
    section "article-start-brief"
    time_scenario "article-start-brief" "$RUNS" "local; topic: $TOPIC" -- \
        wt knowledge article-start "$TOPIC" --intent expand --format json --view brief
fi

if wants fts-search; then
    section "fts-search"
    time_scenario "fts-search" "$RUNS" "local; cross-page chunk retrieval" -- \
        wt knowledge inspect chunks --across-pages --query "network spirituality remilia" \
            --max-pages 6 --limit 10 --token-budget 1200 --format json --view brief
fi

if wants knowledge-build; then
    section "knowledge-build"
    time_scenario "knowledge-build" "$RUNS" "local; full index rebuild" -- \
        wt knowledge build
fi

if wants knowledge-warm; then
    section "knowledge-warm"
    time_scenario "knowledge-warm" "$RUNS" "local-ish; docs-mode missing" -- \
        wt knowledge warm --docs-mode missing
fi

# --- Live (network) scenarios ---

if [ "$TIER" = "live" ]; then
    if wants pull-full-all; then
        section "pull-full-all"
        time_scenario "pull-full-all" 1 "network; full enumerate + fetch" -- \
            wt pull --full --all
    fi

    if wants push-conflict-dryrun; then
        section "push-conflict-dryrun"
        modify_articles_for_push 120
        time_scenario "push-conflict-dryrun" 1 "network; 120 modified, dry run" -- \
            wt push --dry-run --summary "perf bench dry run"
        restore_push_modifications
    fi

    if wants session-refresh-warm; then
        section "session-refresh-warm"
        time_scenario "session-refresh-warm" 1 "network; existing runtime db" -- \
            wt workflow session-refresh
    fi

    if wants session-refresh-cold; then
        section "session-refresh-cold"
        wt db reset --yes >/dev/null
        time_scenario "session-refresh-cold" 1 "network; after db reset" -- \
            wt workflow session-refresh
    fi
fi

echo ""
TABLE="| scenario | runs | median_ms | min_ms | max_ms | notes |
|---|---|---|---|---|---|"
for row in "${RESULT_ROWS[@]}"; do
    TABLE="$TABLE
$row"
done
echo "$TABLE"

if [ -n "$OUTPUT_FILE" ]; then
    {
        echo ""
        echo "## perf bench $(date -u +%Y-%m-%dT%H:%M:%SZ) — tier=$TIER, cmd=$WT_CMD"
        echo ""
        echo "$TABLE"
    } >> "$OUTPUT_FILE"
fi
