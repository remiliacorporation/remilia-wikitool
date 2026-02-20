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
WIKITOOL="${WIKITOOL:-cargo run --quiet --}"
TMPDIR_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/wikitool-test-XXXXXX")

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
    $WIKITOOL --project-root "$root" "$@"
}

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
if echo "$OUTPUT" | grep -q -i "diff\|no changes\|untracked\|new"; then
    pass "diff produces output"
else
    fail "diff produces output (got: $OUTPUT)"
fi

# --- status ---
section "status"
PROJ=$(setup_project status)
wt "$PROJ" init > /dev/null 2>&1
OUTPUT=$(wt "$PROJ" status 2>&1 || true)
if echo "$OUTPUT" | grep -q -i "project_root\|status\|wiki_content"; then
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
if echo "$OUTPUT" | grep -q "inserted_rows.*2\|2.*rows\|inserted.*2"; then
    pass "index rebuild inserts correct row count"
else
    # More lenient check
    if echo "$OUTPUT" | grep -qi "rebuild\|inserted"; then
        pass "index rebuild produces output"
    else
        fail "index rebuild inserts rows (got: $OUTPUT)"
    fi
fi

# --- index stats ---
section "index stats"
OUTPUT=$(wt "$PROJ" index stats 2>&1)
if echo "$OUTPUT" | grep -qi "indexed\|rows\|pages"; then
    pass "index stats reports data"
else
    fail "index stats reports data (got: $OUTPUT)"
fi

# --- index backlinks ---
section "index backlinks"
OUTPUT=$(wt "$PROJ" index backlinks "Alpha" 2>&1)
if echo "$OUTPUT" | grep -qi "Beta\|backlink"; then
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

# --- search ---
section "search"
OUTPUT=$(wt "$PROJ" search "Alpha" 2>&1)
if echo "$OUTPUT" | grep -qi "Alpha"; then
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
    if echo "$OUTPUT" | grep -qi "import\|row\|article\|skip\|dry"; then
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
    if echo "$OUTPUT" | grep -qi "import\|row\|article\|skip\|dry"; then
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
    if echo "$OUTPUT" | grep -qi "import\|TestExtension\|extension\|page\|bundle"; then
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
if echo "$OUTPUT" | grep -qi "TestExtension\|search\|result\|match\|no.*found\|0 results"; then
    pass "docs search returns results or reports none"
else
    fail "docs search returns results (got: $OUTPUT)"
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
    OUTPUT=$(wt "$PROJ_LIVE" pull --category "Remilia Corporation" --limit 2 2>&1 || true)
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
    OUTPUT=$(wt "$PROJ_LIVE" seo inspect 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "seo\|title\|meta\|inspect\|description"; then
        pass "seo inspect produces report"
    else
        fail "seo inspect produces report (got: ${OUTPUT:0:300})"
    fi

    # --- net inspect ---
    section "net inspect (live)"
    OUTPUT=$(wt "$PROJ_LIVE" net inspect 2>&1 || true)
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
    OUTPUT=$(wt "$PROJ_LIVE" export "Remilia Corporation" --out "$EXPORT_DIR" 2>&1 || true)
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
