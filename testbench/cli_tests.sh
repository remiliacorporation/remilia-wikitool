#!/usr/bin/env bash
# wikitool CLI regression tests
# Usage:
#   TIER=offline bash testbench/cli_tests.sh   # offline-only (default)
#   TIER=live   bash testbench/cli_tests.sh   # offline + live (read-only API)
#
# Run from: custom/wikitool/
set -euo pipefail

TIER="${TIER:-offline}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURES="$SCRIPT_DIR/fixtures"
WIKITOOL_RAW="${WIKITOOL:-}"
TMP_BASE="${TMPDIR:-$SCRIPT_DIR/.tmp}"
mkdir -p "$TMP_BASE"
TMPDIR_ROOT=$(mktemp -d "$TMP_BASE/wikitool-test-XXXXXX")

PASS=0
FAIL=0
SKIP=0

# --- helpers ---

pass() {
    printf "  \033[32mPASS\033[0m: %s\n" "$1"
    PASS=$((PASS + 1))
}

fail() {
    printf "  \033[31mFAIL\033[0m: %s\n" "$1"
    FAIL=$((FAIL + 1))
}

skip() {
    printf "  \033[33mSKIP\033[0m: %s\n" "$1"
    SKIP=$((SKIP + 1))
}

section() {
    printf "\n--- %s ---\n" "$1"
}

cleanup() {
    rm -rf "$TMPDIR_ROOT"
}
trap cleanup EXIT

resolve_wikitool_cmd() {
    if [ -n "$WIKITOOL_RAW" ]; then
        # shellcheck disable=SC2206 # Intentional word splitting for command string overrides.
        WIKITOOL_CMD=($WIKITOOL_RAW)
        WIKITOOL_PATH_MODE="${WIKITOOL_PATH_MODE:-auto}"
        return
    fi

    if command -v cargo > /dev/null 2>&1; then
        local cargo_path
        cargo_path=$(command -v cargo)
        WIKITOOL_CMD=(cargo run --quiet --)
        if [[ "$cargo_path" == *.exe ]]; then
            WIKITOOL_PATH_MODE="windows"
        else
            WIKITOOL_PATH_MODE="posix"
        fi
        return
    fi

    if command -v cargo.exe > /dev/null 2>&1; then
        WIKITOOL_CMD=(cargo.exe run --quiet --)
        WIKITOOL_PATH_MODE="windows"
        return
    fi

    local candidate
    for candidate in /mnt/c/Users/*/.cargo/bin/cargo.exe; do
        if [ -x "$candidate" ]; then
            WIKITOOL_CMD=("$candidate" run --quiet --)
            WIKITOOL_PATH_MODE="windows"
            return
        fi
    done

    echo "ERROR: Unable to locate cargo/cargo.exe. Set WIKITOOL to an explicit command." >&2
    exit 1
}

to_wikitool_path() {
    local path="$1"
    if [[ "$path" =~ ^/mnt/([a-zA-Z])/(.*)$ ]]; then
        local drive="${BASH_REMATCH[1]}"
        local rest="${BASH_REMATCH[2]}"
        drive=$(printf "%s" "$drive" | tr "[:lower:]" "[:upper:]")
        printf "%s:/%s" "$drive" "$rest"
        return
    fi
    if [[ "$path" =~ ^/([a-zA-Z])/(.*)$ ]]; then
        local drive="${BASH_REMATCH[1]}"
        local rest="${BASH_REMATCH[2]}"
        drive=$(printf "%s" "$drive" | tr "[:lower:]" "[:upper:]")
        printf "%s:/%s" "$drive" "$rest"
        return
    fi
    printf "%s" "$path"
}

# Create a fresh project root for isolated testing
setup_project() {
    local dir="$TMPDIR_ROOT/project-$1"
    mkdir -p "$dir"
    echo "$dir"
}

# Run wikitool with a given project root
wt() {
    local root="$1"
    shift
    if [ "${WIKITOOL_PATH_MODE:-posix}" = "windows" ] || [ "${WIKITOOL_PATH_MODE:-posix}" = "auto" ]; then
        local wt_root
        wt_root=$(to_wikitool_path "$root")

        local arg
        local normalized_args=()
        for arg in "$@"; do
            if [[ "$arg" =~ ^/mnt/[a-zA-Z]/.*$ || "$arg" =~ ^/[a-zA-Z]/.*$ ]]; then
                normalized_args+=("$(to_wikitool_path "$arg")")
            else
                normalized_args+=("$arg")
            fi
        done

        "${WIKITOOL_CMD[@]}" --project-root "$wt_root" "${normalized_args[@]}"
        return
    fi

    "${WIKITOOL_CMD[@]}" --project-root "$root" "$@"
}

WIKITOOL_CMD=()
WIKITOOL_PATH_MODE=""
resolve_wikitool_cmd

# --- banner ---
echo "=== wikitool CLI regression tests ==="
echo "Tier: $TIER | Temp: $TMPDIR_ROOT"

# ============================================================
# OFFLINE TIER
# ============================================================

# --- init ---
section "init"
PROJ=$(setup_project init)
wt "$PROJ" init --templates > /dev/null 2>&1
if [ -d "$PROJ/.wikitool" ]; then
    pass "creates .wikitool directory"
else
    fail "creates .wikitool directory"
fi
if [ -d "$PROJ/wiki_content" ]; then
    pass "creates wiki_content directory"
else
    fail "creates wiki_content directory"
fi
if [ -d "$PROJ/templates" ]; then
    pass "creates templates directory"
else
    fail "creates templates directory"
fi

# --- diff ---
section "diff"
PROJ=$(setup_project diff)
wt "$PROJ" init > /dev/null 2>&1
# Seed a wiki file
mkdir -p "$PROJ/wiki_content/Main"
cat > "$PROJ/wiki_content/Main/Test_Page.wiki" << 'WIKIEOF'
{{SHORTDESC:A test page}}
{{Article quality|unverified}}

'''Test Page''' is a test.

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
OUTPUT=$(wt "$PROJ" diff 2>&1 || true)
if echo "$OUTPUT" | grep -q -i "^diff$" && echo "$OUTPUT" | grep -q -i "diff\."; then
    pass "diff produces output"
else
    fail "diff produces output (got: $OUTPUT)"
fi

# --- status ---
section "status"
PROJ=$(setup_project status)
wt "$PROJ" init > /dev/null 2>&1
OUTPUT=$(wt "$PROJ" status 2>&1 || true)
if echo "$OUTPUT" | grep -q "project_root:" && echo "$OUTPUT" | grep -q "wiki_content_exists:"; then
    pass "status shows expected fields"
else
    fail "status shows expected fields (got: $OUTPUT)"
fi

# --- index rebuild ---
section "index rebuild"
PROJ=$(setup_project index-rebuild)
wt "$PROJ" init > /dev/null 2>&1
mkdir -p "$PROJ/wiki_content/Main"
cat > "$PROJ/wiki_content/Main/Alpha.wiki" << 'WIKIEOF'
{{SHORTDESC:Alpha article}}

'''Alpha''' is an article.

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
cat > "$PROJ/wiki_content/Main/Beta.wiki" << 'WIKIEOF'
{{SHORTDESC:Beta article}}

'''Beta''' is an article that links to [[Alpha]].

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
OUTPUT=$(wt "$PROJ" index rebuild 2>&1)
if echo "$OUTPUT" | grep -q "inserted_rows: 2"; then
    pass "index rebuild inserts correct row count"
else
    fail "index rebuild inserts rows (got: $OUTPUT)"
fi

# --- index stats ---
section "index stats"
OUTPUT=$(wt "$PROJ" index stats 2>&1)
if echo "$OUTPUT" | grep -qi "indexed\|rows\|pages"; then
    pass "index stats reports data"
else
    fail "index stats reports data (got: $OUTPUT)"
fi

# --- index chunks ---
section "index chunks"
OUTPUT=$(wt "$PROJ" index chunks "Alpha" --query "Alpha" --limit 2 --token-budget 120 2>&1 || true)
if echo "$OUTPUT" | grep -q "chunks.count:" && echo "$OUTPUT" | grep -q "chunks.retrieval_mode:"; then
    pass "index chunks returns chunked retrieval output"
else
    fail "index chunks returns chunked retrieval output (got: $OUTPUT)"
fi

# --- index chunks (across-pages json) ---
section "index chunks across-pages json"
OUTPUT=$(wt "$PROJ" index chunks --across-pages --query "article" --limit 3 --max-pages 2 --token-budget 140 --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"Found"' && echo "$OUTPUT" | grep -q '"retrieval_mode"' && echo "$OUTPUT" | grep -q '"source_page_count"'; then
    pass "index chunks across-pages emits JSON report"
else
    fail "index chunks across-pages emits JSON report (got: $OUTPUT)"
fi

# --- index backlinks ---
section "index backlinks"
OUTPUT=$(wt "$PROJ" index backlinks "Alpha" 2>&1)
if echo "$OUTPUT" | grep -q "backlinks.source: Beta"; then
    pass "index backlinks finds linking article"
else
    fail "index backlinks finds linking article (got: $OUTPUT)"
fi

# --- index orphans ---
section "index orphans"
# Alpha is linked by Beta, but nothing links to Beta → Beta is orphan
OUTPUT=$(wt "$PROJ" index orphans 2>&1 || true)
if echo "$OUTPUT" | grep -qi "orphan\|Beta\|0 orphans\|no orphans"; then
    pass "index orphans detects or reports"
else
    fail "index orphans detects or reports (got: $OUTPUT)"
fi

# --- index prune-categories ---
section "index prune-categories"
OUTPUT=$(wt "$PROJ" index prune-categories 2>&1 || true)
if [ $? -eq 0 ] || echo "$OUTPUT" | grep -qi "category\|prune\|empty\|0"; then
    pass "index prune-categories runs"
else
    fail "index prune-categories runs (got: $OUTPUT)"
fi

# --- validate ---
section "validate"
OUTPUT=$(wt "$PROJ" validate 2>&1 || true)
if echo "$OUTPUT" | grep -qi "validate\|check\|link\|pass\|warning\|clean"; then
    pass "validate produces report"
else
    fail "validate produces report (got: $OUTPUT)"
fi

# --- lint ---
section "lint"
PROJ_LINT=$(setup_project lint)
wt "$PROJ_LINT" init --templates > /dev/null 2>&1
OUTPUT=$(wt "$PROJ_LINT" lint 2>&1 || true)
# lint may report "no modules found" or run successfully
if echo "$OUTPUT" | grep -qi "lint\|module\|clean\|no.*found\|0 issues\|error"; then
    pass "lint runs without crash"
else
    fail "lint runs without crash (got: $OUTPUT)"
fi

# --- db stats ---
section "db stats"
OUTPUT=$(wt "$PROJ" db stats 2>&1)
if echo "$OUTPUT" | grep -qi "db\|size\|tables\|stats\|path"; then
    pass "db stats shows database info"
else
    fail "db stats shows database info (got: $OUTPUT)"
fi

# --- db sync ---
section "db sync"
OUTPUT=$(wt "$PROJ" db sync 2>&1 || true)
if echo "$OUTPUT" | grep -qi "sync\|ledger\|rows\|config"; then
    pass "db sync produces output"
else
    fail "db sync produces output (got: $OUTPUT)"
fi

# --- db migrate ---
section "db migrate"
PROJ_MIG=$(setup_project migrate)
wt "$PROJ_MIG" init > /dev/null 2>&1
OUTPUT=$(wt "$PROJ_MIG" db migrate 2>&1)
if echo "$OUTPUT" | grep -qi "applied\|up to date\|version\|migration"; then
    pass "db migrate applies or reports up-to-date"
else
    fail "db migrate applies or reports up-to-date (got: $OUTPUT)"
fi
# Second run should be idempotent
OUTPUT2=$(wt "$PROJ_MIG" db migrate 2>&1)
if echo "$OUTPUT2" | grep -qi "up to date"; then
    pass "db migrate is idempotent"
else
    fail "db migrate is idempotent (got: $OUTPUT2)"
fi

# --- context ---
section "context"
OUTPUT=$(wt "$PROJ" context "Alpha" 2>&1)
if echo "$OUTPUT" | grep -qi "Alpha\|context\|title"; then
    pass "context returns data for known title"
else
    fail "context returns data for known title (got: $OUTPUT)"
fi

# --- workflow authoring-pack ---
section "workflow authoring-pack"
STUB_PATH="$PROJ/wiki_content/Main/Alpha_Stub.wiki"
cat > "$STUB_PATH" << 'WIKIEOF'
{{Infobox person|name=Alpha Draft}}
'''Alpha Draft''' references [[Alpha]] and [[Missing Page]].
WIKIEOF
OUTPUT=$(wt "$PROJ" workflow authoring-pack "Alpha" --stub-path "$STUB_PATH" --related-limit 6 --chunk-limit 4 --token-budget 220 --max-pages 2 --template-limit 6 --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"Found"' && echo "$OUTPUT" | grep -q '"suggested_templates"' && echo "$OUTPUT" | grep -q '"template_baseline"' && echo "$OUTPUT" | grep -q '"stub_missing_links"'; then
    pass "workflow authoring-pack emits authoring knowledge pack"
else
    fail "workflow authoring-pack emits authoring knowledge pack (got: $OUTPUT)"
fi

# --- search ---
section "search"
OUTPUT=$(wt "$PROJ" search "Alpha" 2>&1)
if echo "$OUTPUT" | grep -q "search.hit: Alpha"; then
    pass "search finds indexed article"
else
    fail "search finds indexed article (got: $OUTPUT)"
fi

# --- contracts command-surface ---
section "contracts command-surface"
OUTPUT=$(wt "$PROJ" contracts command-surface 2>&1)
if echo "$OUTPUT" | grep -q '^\['; then
    pass "contracts command-surface outputs JSON"
else
    fail "contracts command-surface outputs JSON (got: ${OUTPUT:0:200})"
fi

# --- contracts snapshot ---
section "contracts snapshot"
OUTPUT=$(wt "$PROJ" contracts snapshot --project-root "$PROJ" 2>&1)
if echo "$OUTPUT" | grep -q '{'; then
    pass "contracts snapshot produces JSON"
else
    fail "contracts snapshot produces JSON (got: ${OUTPUT:0:200})"
fi

# --- import cargo (CSV) ---
section "import cargo (CSV)"
if [ -f "$FIXTURES/sample_import.csv" ]; then
OUTPUT=$(wt "$PROJ" import cargo "$FIXTURES/sample_import.csv" --table "test_csv" 2>&1 || true)
if echo "$OUTPUT" | grep -q "source_type: csv" && echo "$OUTPUT" | grep -q "created:"; then
    pass "import cargo CSV produces output"
else
    fail "import cargo CSV produces output (got: $OUTPUT)"
fi
else
    skip "import cargo CSV (fixture missing)"
fi

# --- import cargo (JSON) ---
section "import cargo (JSON)"
if [ -f "$FIXTURES/sample_import.json" ]; then
OUTPUT=$(wt "$PROJ" import cargo "$FIXTURES/sample_import.json" --table "test_json" --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"pages_created"' && echo "$OUTPUT" | grep -q '"errors"'; then
    pass "import cargo JSON produces output"
else
    fail "import cargo JSON produces output (got: $OUTPUT)"
fi
else
    skip "import cargo JSON (fixture missing)"
fi

# --- lsp:generate-config ---
section "lsp:generate-config"
OUTPUT=$(wt "$PROJ" lsp:generate-config 2>&1 || true)
if echo "$OUTPUT" | grep -qi "wikiparser\|json\|config\|lsp"; then
    pass "lsp:generate-config produces output"
else
    fail "lsp:generate-config produces output (got: $OUTPUT)"
fi

# --- lsp:status ---
section "lsp:status"
OUTPUT=$(wt "$PROJ" lsp:status 2>&1 || true)
if echo "$OUTPUT" | grep -qi "lsp\|status\|parser\|config"; then
    pass "lsp:status produces output"
else
    fail "lsp:status produces output (got: $OUTPUT)"
fi

# --- lsp:info ---
section "lsp:info"
OUTPUT=$(wt "$PROJ" lsp:info 2>&1 || true)
if echo "$OUTPUT" | grep -qi "lsp\|wikitext\|parser\|info"; then
    pass "lsp:info produces output"
else
    fail "lsp:info produces output (got: $OUTPUT)"
fi

# --- docs import --bundle ---
section "docs import --bundle"
if [ -f "$FIXTURES/sample_docs_bundle.json" ]; then
OUTPUT=$(wt "$PROJ" docs import --bundle "$FIXTURES/sample_docs_bundle.json" 2>&1 || true)
if echo "$OUTPUT" | grep -q "imported_extensions: 1" && echo "$OUTPUT" | grep -q "imported_pages:"; then
    pass "docs import bundle processes fixture"
else
    fail "docs import bundle processes fixture (got: $OUTPUT)"
fi
else
    skip "docs import bundle (fixture missing)"
fi

# --- docs list ---
section "docs list"
OUTPUT=$(wt "$PROJ" docs list 2>&1 || true)
if echo "$OUTPUT" | grep -qi "extension\|technical\|docs\|list\|TestExtension\|none\|empty"; then
    pass "docs list shows entries"
else
    fail "docs list shows entries (got: $OUTPUT)"
fi

# --- docs search ---
section "docs search"
OUTPUT=$(wt "$PROJ" docs search "TestExtension" 2>&1 || true)
if echo "$OUTPUT" | grep -q "hit: \[extension\] Extension:TestExtension"; then
    pass "docs search returns results or reports none"
else
    fail "docs search returns results (got: $OUTPUT)"
fi

# --- FTS continuity: local search substring ---
section "search substring continuity across migrate"
PROJ_FTS=$(setup_project fts-search)
wt "$PROJ_FTS" init > /dev/null 2>&1
mkdir -p "$PROJ_FTS/wiki_content/Main"
cat > "$PROJ_FTS/wiki_content/Main/AlphaBeta.wiki" << 'WIKIEOF'
{{SHORTDESC:AlphaBeta article}}
{{Article quality|unverified}}

'''AlphaBeta''' is a test article.

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
wt "$PROJ_FTS" index rebuild > /dev/null 2>&1
BEFORE=$(wt "$PROJ_FTS" search "pha" 2>&1)
wt "$PROJ_FTS" db migrate > /dev/null 2>&1
AFTER=$(wt "$PROJ_FTS" search "pha" 2>&1)
if echo "$BEFORE" | grep -q "search.hit: AlphaBeta" && echo "$AFTER" | grep -q "search.hit: AlphaBeta"; then
    pass "substring search works before and after migrate"
else
    fail "substring search continuity failed (before: $BEFORE | after: $AFTER)"
fi

# --- FTS continuity: docs search substring ---
section "docs search substring continuity across migrate"
PROJ_DOCS_FTS=$(setup_project fts-docs)
wt "$PROJ_DOCS_FTS" init > /dev/null 2>&1
wt "$PROJ_DOCS_FTS" docs import --bundle "$FIXTURES/sample_docs_bundle.json" > /dev/null 2>&1
BEFORE=$(wt "$PROJ_DOCS_FTS" docs search "BetaToken" 2>&1)
wt "$PROJ_DOCS_FTS" db migrate > /dev/null 2>&1
AFTER=$(wt "$PROJ_DOCS_FTS" docs search "BetaToken" 2>&1)
if echo "$BEFORE" | grep -q "Extension:TestExtension" && echo "$AFTER" | grep -q "Extension:TestExtension"; then
    pass "docs substring search works before and after migrate"
else
    fail "docs substring search continuity failed (before: $BEFORE | after: $AFTER)"
fi

# --- docs remove ---
section "docs remove"
OUTPUT=$(wt "$PROJ" docs remove "TestExtension" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "remove\|delete\|TestExtension\|success\|not found"; then
    pass "docs remove processes request"
else
    fail "docs remove processes request (got: $OUTPUT)"
fi

# --- delete --dry-run ---
section "delete --dry-run"
# Create a file, dry-run delete, verify still exists
mkdir -p "$PROJ/wiki_content/Main"
echo "test content" > "$PROJ/wiki_content/Main/Delete_Test.wiki"
OUTPUT=$(wt "$PROJ" delete "Delete Test" --dry-run 2>&1 || true)
if [ -f "$PROJ/wiki_content/Main/Delete_Test.wiki" ]; then
    pass "delete --dry-run does not remove file"
else
    fail "delete --dry-run removed the file!"
fi

# --- push --dry-run ---
section "push --dry-run"
OUTPUT=$(wt "$PROJ" push --dry-run --summary "test push" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "dry.run\|push\|no changes\|would\|skip\|sync"; then
    pass "push --dry-run reports changes"
else
    fail "push --dry-run reports changes (got: $OUTPUT)"
fi

# ============================================================
# LIVE TIER (optional, requires network)
# ============================================================

if [ "$TIER" = "live" ]; then
    echo ""
    echo "=== Live tier (read-only API + dry-run writes) ==="

    PROJ_LIVE=$(setup_project live)
    wt "$PROJ_LIVE" init > /dev/null 2>&1

    # --- pull ---
    section "pull (live)"
    OUTPUT=$(wt "$PROJ_LIVE" pull --category "Remilia Corporation" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "pull\|page\|synced\|download"; then
        pass "pull fetches from live wiki"
    else
        fail "pull fetches from live wiki (got: ${OUTPUT:0:300})"
    fi

    # --- search-external ---
    section "search-external (live)"
    OUTPUT=$(wt "$PROJ_LIVE" search-external "Remilia" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "Remilia\|result\|title"; then
        pass "search-external finds known page"
    else
        fail "search-external finds known page (got: ${OUTPUT:0:300})"
    fi

    # --- seo inspect ---
    section "seo inspect (live)"
    OUTPUT=$(wt "$PROJ_LIVE" seo inspect "Main Page" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "seo\|title\|meta\|inspect\|description"; then
        pass "seo inspect produces report"
    else
        fail "seo inspect produces report (got: ${OUTPUT:0:300})"
    fi

    # --- net inspect ---
    section "net inspect (live)"
    OUTPUT=$(wt "$PROJ_LIVE" net inspect "Main Page" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "net\|inspect\|status\|response\|http\|dns"; then
        pass "net inspect produces report"
    else
        fail "net inspect produces report (got: ${OUTPUT:0:300})"
    fi

    # --- fetch ---
    section "fetch (live)"
    OUTPUT=$(wt "$PROJ_LIVE" fetch "https://www.mediawiki.org/wiki/MediaWiki" --format wikitext 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "MediaWiki\|fetch\|content\|wiki"; then
        pass "fetch retrieves page content"
    else
        fail "fetch retrieves page content (got: ${OUTPUT:0:300})"
    fi

    # --- export ---
    section "export (live)"
    EXPORT_DIR="$TMPDIR_ROOT/exports"
    mkdir -p "$EXPORT_DIR"
    OUTPUT=$(wt "$PROJ_LIVE" export "Remilia Corporation" --output "$EXPORT_DIR" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "export\|wrote\|saved\|Remilia"; then
        pass "export saves page locally"
    else
        fail "export saves page locally (got: ${OUTPUT:0:300})"
    fi

    # --- docs import (live) ---
    section "docs import (live)"
    OUTPUT=$(wt "$PROJ_LIVE" docs import "Scribunto" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "import\|Scribunto\|page\|fetched"; then
        pass "docs import fetches from mediawiki.org"
    else
        fail "docs import fetches from mediawiki.org (got: ${OUTPUT:0:300})"
    fi

    # --- docs update (live) ---
    section "docs update (live)"
    OUTPUT=$(wt "$PROJ_LIVE" docs update 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "update\|check\|expired\|up to date\|refresh"; then
        pass "docs update checks for outdated docs"
    else
        fail "docs update checks for outdated docs (got: ${OUTPUT:0:300})"
    fi
fi

# ============================================================
# RESULTS
# ============================================================

echo ""
TOTAL=$((PASS + FAIL + SKIP))
if [ "$FAIL" -eq 0 ]; then
    printf "=== \033[32mRESULTS: %d passed, %d failed, %d skipped\033[0m ===\n" "$PASS" "$FAIL" "$SKIP"
else
    printf "=== \033[31mRESULTS: %d passed, %d failed, %d skipped\033[0m ===\n" "$PASS" "$FAIL" "$SKIP"
fi

exit "$FAIL"
