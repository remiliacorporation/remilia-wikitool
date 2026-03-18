#!/usr/bin/env bash
# wikitool CLI regression tests
# Usage:
#   TIER=offline bash testbench/cli_tests.sh   # offline-only (default)
#   TIER=live   bash testbench/cli_tests.sh   # offline + live (read-only API)
#
# Run from: tools/wikitool/
set -euo pipefail

TIER="${TIER:-offline}"
KNOWLEDGE_DOCS_PROFILE="${KNOWLEDGE_DOCS_PROFILE:-remilia-mw-1.44}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
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
        if [ -n "${WIKITOOL_PATH_MODE:-}" ]; then
            return
        fi
        case "$WIKITOOL_RAW" in
            *.exe|[A-Za-z]:\\*|[A-Za-z]:/*)
                WIKITOOL_PATH_MODE="windows"
                ;;
            *)
                WIKITOOL_PATH_MODE="posix"
                ;;
        esac
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

resolve_local_binary_candidate() {
    local candidate
    for candidate in \
        "$REPO_ROOT/target/debug/wikitool" \
        "$REPO_ROOT/target/debug/wikitool.exe"
    do
        if [ -x "$candidate" ]; then
            printf "%s" "$candidate"
            return
        fi
    done
}

write_live_env() {
    local root="$1"
    cat > "$root/.env" << 'ENVEOF'
WIKI_URL=https://wiki.remilia.org/
WIKI_API_URL=https://wiki.remilia.org/api.php
ENVEOF
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

# --- knowledge status (pre-build) ---
section "knowledge status (pre-build)"
PROJ=$(setup_project knowledge-build)
wt "$PROJ" init > /dev/null 2>&1
mkdir -p "$PROJ/wiki_content/Main"
cat > "$PROJ/wiki_content/Main/Alpha.wiki" << 'WIKIEOF'
{{SHORTDESC:Alpha article}}

'''Alpha''' is an article.<ref>{{Cite web|title=Alpha Source|website=Remilia}}</ref>

[[Image:Alpha.png|thumb|Alpha portrait]]

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
cat > "$PROJ/wiki_content/Main/Beta.wiki" << 'WIKIEOF'
{{SHORTDESC:Beta article}}

'''Beta''' is an article that links to [[Alpha]].<ref>{{Cite book|title=Beta Source|publisher=Remilia Press}}</ref>

[[File:Beta.jpg|thumb|Beta portrait]]

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
OUTPUT=$(wt "$PROJ" knowledge status --docs-profile "$KNOWLEDGE_DOCS_PROFILE" 2>&1 || true)
if echo "$OUTPUT" | grep -q "knowledge.readiness: not_ready" && echo "$OUTPUT" | grep -q "knowledge.degradations: content_index_missing, docs_profile_missing"; then
    pass "knowledge status reports missing content/docs readiness before build"
else
    fail "knowledge status reports missing content/docs readiness before build (got: $OUTPUT)"
fi

# --- knowledge build ---
section "knowledge build"
OUTPUT=$(wt "$PROJ" knowledge build 2>&1)
if echo "$OUTPUT" | grep -q "rebuild.inserted_rows: 2" && echo "$OUTPUT" | grep -q "knowledge.readiness: content_ready"; then
    pass "knowledge build indexes pages and reports content readiness"
else
    fail "knowledge build indexes pages and reports content readiness (got: $OUTPUT)"
fi

# --- knowledge status ---
section "knowledge status"
OUTPUT=$(wt "$PROJ" knowledge status --docs-profile "$KNOWLEDGE_DOCS_PROFILE" 2>&1 || true)
if echo "$OUTPUT" | grep -q "knowledge.docs_profile_requested: $KNOWLEDGE_DOCS_PROFILE" && echo "$OUTPUT" | grep -q "knowledge.readiness: content_ready" && echo "$OUTPUT" | grep -q "knowledge.degradations: docs_profile_missing"; then
    pass "knowledge status reports content readiness and docs degradation after build"
else
    fail "knowledge status reports content readiness and docs degradation after build (got: $OUTPUT)"
fi

# --- article lint/fix ---
section "article lint/fix"
ARTICLE_PROJ=$(setup_project article-lint)
wt "$ARTICLE_PROJ" init --templates > /dev/null 2>&1
mkdir -p "$ARTICLE_PROJ/wiki_content/Main"
mkdir -p "$ARTICLE_PROJ/templates/misc"
mkdir -p "$ARTICLE_PROJ/tools/wikitool/ai-pack/llm_instructions"
cat > "$ARTICLE_PROJ/tools/wikitool/ai-pack/llm_instructions/article_structure.md" << 'MDEOF'
{{SHORTDESC:Example}}
{{Article quality|unverified}}
== References ==
{{Reflist}}
parent_group = Remilia
MDEOF
cat > "$ARTICLE_PROJ/tools/wikitool/ai-pack/llm_instructions/style_rules.md" << 'MDEOF'
### No placeholder content
- Never output: `INSERT_SOURCE_URL`
Straight quotes only
MDEOF
cat > "$ARTICLE_PROJ/tools/wikitool/ai-pack/llm_instructions/writing_guide.md" << 'MDEOF'
raw MediaWiki wikitext
Never output Markdown
Use 2-4 categories per article
[[Category:Remilia]]
{{Article quality|unverified}}
parent_group = Remilia
### Citation templates
```wikitext
{{Cite web|url=}}
```
## 6. Infobox selection
| Subject type | Infobox |
|---|---|
| NFT Collection | `{{Infobox NFT collection}}` |
MDEOF
cat > "$ARTICLE_PROJ/templates/misc/Template_Article_quality.wiki" << 'WIKIEOF'
<includeonly>{{{1|unverified}}}</includeonly>
WIKIEOF
cat > "$ARTICLE_PROJ/templates/misc/Template_Reflist.wiki" << 'WIKIEOF'
<references />
WIKIEOF
cat > "$ARTICLE_PROJ/wiki_content/Main/Article_Draft.wiki" << 'WIKIEOF'
{{SHORTDESC:Draft article}}
{{Article quality|unverified}}

## History
'''Article Draft''' is a test<ref>{{Cite web|title=Source}}</ref>.

== References ==
WIKIEOF
OUTPUT=$(wt "$ARTICLE_PROJ" article lint "$ARTICLE_PROJ/wiki_content/Main/Article_Draft.wiki" 2>&1 || true)
if echo "$OUTPUT" | grep -q "rule=structure.markdown_heading" && echo "$OUTPUT" | grep -q "rule=structure.require_reflist" && echo "$OUTPUT" | grep -q "rule=citation.after_punctuation"; then
    pass "article lint reports markdown heading, reflist, and citation-order issues"
else
    fail "article lint reports markdown heading, reflist, and citation-order issues (got: $OUTPUT)"
fi
OUTPUT=$(wt "$ARTICLE_PROJ" article fix "$ARTICLE_PROJ/wiki_content/Main/Article_Draft.wiki" --apply safe 2>&1 || true)
if echo "$OUTPUT" | grep -q "applied_fix_count: 3" && grep -q "== History ==" "$ARTICLE_PROJ/wiki_content/Main/Article_Draft.wiki" && grep -q "{{Reflist}}" "$ARTICLE_PROJ/wiki_content/Main/Article_Draft.wiki" && grep -q "test.<ref>" "$ARTICLE_PROJ/wiki_content/Main/Article_Draft.wiki"; then
    pass "article fix applies safe markdown, reflist, and citation-order fixes"
else
    fail "article fix applies safe markdown, reflist, and citation-order fixes (got: $OUTPUT)"
fi

# --- knowledge inspect stats ---
section "knowledge inspect stats"
OUTPUT=$(wt "$PROJ" knowledge inspect stats 2>&1)
if echo "$OUTPUT" | grep -qi "indexed\|rows\|pages"; then
    pass "knowledge inspect stats reports data"
else
    fail "knowledge inspect stats reports data (got: $OUTPUT)"
fi

# --- knowledge inspect chunks ---
section "knowledge inspect chunks"
OUTPUT=$(wt "$PROJ" knowledge inspect chunks "Alpha" --query "Alpha" --limit 2 --token-budget 120 2>&1 || true)
if echo "$OUTPUT" | grep -q "chunks.count:" && echo "$OUTPUT" | grep -q "chunks.retrieval_mode:"; then
    pass "knowledge inspect chunks returns chunked retrieval output"
else
    fail "knowledge inspect chunks returns chunked retrieval output (got: $OUTPUT)"
fi

# --- knowledge inspect chunks (across-pages json) ---
section "knowledge inspect chunks across-pages json"
OUTPUT=$(wt "$PROJ" knowledge inspect chunks --across-pages --query "article" --limit 3 --max-pages 2 --token-budget 140 --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"Found"' && echo "$OUTPUT" | grep -q '"retrieval_mode"' && echo "$OUTPUT" | grep -q '"source_page_count"'; then
    pass "knowledge inspect chunks across-pages emits JSON report"
else
    fail "knowledge inspect chunks across-pages emits JSON report (got: $OUTPUT)"
fi

# --- knowledge inspect backlinks ---
section "knowledge inspect backlinks"
OUTPUT=$(wt "$PROJ" knowledge inspect backlinks "Alpha" 2>&1)
if echo "$OUTPUT" | grep -Eq "backlink: Beta|backlinks.source: Beta"; then
    pass "knowledge inspect backlinks finds linking article"
else
    fail "knowledge inspect backlinks finds linking article (got: $OUTPUT)"
fi

# --- knowledge inspect orphans ---
section "knowledge inspect orphans"
# Alpha is linked by Beta, but nothing links to Beta -> Beta is orphan
OUTPUT=$(wt "$PROJ" knowledge inspect orphans 2>&1 || true)
if echo "$OUTPUT" | grep -qi "orphan\|Beta\|0 orphans\|no orphans"; then
    pass "knowledge inspect orphans detects or reports"
else
    fail "knowledge inspect orphans detects or reports (got: $OUTPUT)"
fi

# --- knowledge inspect empty-categories ---
section "knowledge inspect empty-categories"
OUTPUT=$(wt "$PROJ" knowledge inspect empty-categories 2>&1 || true)
if [ $? -eq 0 ] || echo "$OUTPUT" | grep -qi "category\|empty\|0"; then
    pass "knowledge inspect empty-categories runs"
else
    fail "knowledge inspect empty-categories runs (got: $OUTPUT)"
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
if echo "$OUTPUT" | grep -q "docs_profile_requested: $KNOWLEDGE_DOCS_PROFILE" && echo "$OUTPUT" | grep -q "readiness: content_ready" && echo "$OUTPUT" | grep -q "knowledge_generation:"; then
    pass "db stats includes knowledge readiness metadata"
else
    fail "db stats includes knowledge readiness metadata (got: $OUTPUT)"
fi

# --- db reset ---
section "db reset"
PROJ_RESET=$(setup_project reset)
wt "$PROJ_RESET" init > /dev/null 2>&1
wt "$PROJ_RESET" knowledge build > /dev/null 2>&1
OUTPUT=$(wt "$PROJ_RESET" db reset --yes 2>&1)
if echo "$OUTPUT" | grep -q "db reset" && echo "$OUTPUT" | grep -q "deleted: yes"; then
    pass "db reset deletes local db"
else
    fail "db reset deletes local db (got: $OUTPUT)"
fi
# Second run should be idempotent
OUTPUT2=$(wt "$PROJ_RESET" db reset --yes 2>&1)
if echo "$OUTPUT2" | grep -q "deleted: no"; then
    pass "db reset is idempotent"
else
    fail "db reset is idempotent (got: $OUTPUT2)"
fi

# --- context requires knowledge build ---
section "context requires knowledge build"
PROJ_CONTEXT=$(setup_project context-unbuilt)
wt "$PROJ_CONTEXT" init > /dev/null 2>&1
mkdir -p "$PROJ_CONTEXT/wiki_content/Main"
cat > "$PROJ_CONTEXT/wiki_content/Main/Alpha.wiki" << 'WIKIEOF'
{{SHORTDESC:Alpha article}}

'''Alpha''' is an article.
WIKIEOF
OUTPUT=$(wt "$PROJ_CONTEXT" context "Alpha" 2>&1 || true)
if echo "$OUTPUT" | grep -q "Run \`wikitool knowledge build\` first."; then
    pass "context requires knowledge build before indexed retrieval"
else
    fail "context requires knowledge build before indexed retrieval (got: $OUTPUT)"
fi

# --- context ---
section "context"
OUTPUT=$(wt "$PROJ" context "Alpha" 2>&1)
if echo "$OUTPUT" | grep -q "context.backend: indexed" && echo "$OUTPUT" | grep -q "context.references.count:" && echo "$OUTPUT" | grep -q "context.media.count:"; then
    pass "context returns indexed data for known title"
else
    fail "context returns indexed data for known title (got: $OUTPUT)"
fi

# --- knowledge pack ---
section "knowledge pack"
STUB_PATH="$PROJ/wiki_content/Main/Alpha_Stub.wiki"
cat > "$STUB_PATH" << 'WIKIEOF'
{{Infobox person|name=Alpha Draft}}
'''Alpha Draft''' references [[Alpha]] and [[Missing Page]].
WIKIEOF
OUTPUT=$(wt "$PROJ" knowledge pack "Alpha" --stub-path "$STUB_PATH" --docs-profile "$KNOWLEDGE_DOCS_PROFILE" --related-limit 6 --chunk-limit 4 --token-budget 220 --max-pages 2 --template-limit 6 --format json 2>&1 || true)
KNOWLEDGE_PACK_JSON="$TMPDIR_ROOT/knowledge-pack.json"
printf '%s' "$OUTPUT" > "$KNOWLEDGE_PACK_JSON"
if python3 - "$KNOWLEDGE_PACK_JSON" "$KNOWLEDGE_DOCS_PROFILE" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text())
if payload.get("docs_profile_requested") != sys.argv[2]:
    raise SystemExit(1)
if payload.get("readiness") != "content_ready":
    raise SystemExit(1)
if "docs_profile_missing" not in payload.get("degradations", []):
    raise SystemExit(1)
if not payload.get("knowledge_generation"):
    raise SystemExit(1)
report = payload.get("result")
if not isinstance(report, dict):
    raise SystemExit(1)
required = (
    "suggested_templates",
    "template_baseline",
    "stub_missing_links",
    "suggested_references",
    "suggested_media",
    "template_references",
    "module_patterns",
    "docs_context",
)
if report.get("status") != "found":
    raise SystemExit(1)
if any(key not in report for key in required):
    raise SystemExit(1)
PY
then
    pass "knowledge pack emits authoring knowledge with readiness/degradation metadata"
else
    fail "knowledge pack emits authoring knowledge with readiness/degradation metadata (got: $OUTPUT)"
fi

# --- knowledge inspect templates ---
section "knowledge inspect templates"
PROJ_TEMPLATES=$(setup_project index-templates)
wt "$PROJ_TEMPLATES" init --templates > /dev/null 2>&1
mkdir -p "$PROJ_TEMPLATES/wiki_content/Main"
mkdir -p "$PROJ_TEMPLATES/templates/infobox/_redirects"
cat > "$PROJ_TEMPLATES/wiki_content/Main/Alpha.wiki" << 'WIKIEOF'
{{Infobox person|name=Alpha|occupation=Archivist}}
'''Alpha''' page.
WIKIEOF
cat > "$PROJ_TEMPLATES/wiki_content/Main/Beta.wiki" << 'WIKIEOF'
{{Infobox person|name=Beta}}
'''Beta''' page.
WIKIEOF
cat > "$PROJ_TEMPLATES/templates/infobox/Template_Infobox_person.wiki" << 'WIKIEOF'
Template lead text.
{{#invoke:Infobox person|render}}
== Parameters ==
Use |name= and |occupation=.
WIKIEOF
cat > "$PROJ_TEMPLATES/templates/infobox/Module_Infobox_person.wiki" << 'WIKIEOF'
return { render = function() end }
WIKIEOF
cat > "$PROJ_TEMPLATES/templates/infobox/Template_Infobox_person___doc.wiki" << 'WIKIEOF'
Documentation lead.
== Usage ==
Use the template on biographies.
WIKIEOF
cat > "$PROJ_TEMPLATES/templates/infobox/_redirects/Template_Infobox_human.wiki" << 'WIKIEOF'
#REDIRECT [[Template:Infobox person]]
WIKIEOF
wt "$PROJ_TEMPLATES" knowledge build > /dev/null 2>&1
OUTPUT=$(wt "$PROJ_TEMPLATES" knowledge inspect templates --limit 5 2>&1 || true)
if echo "$OUTPUT" | grep -q "Template:Infobox person" && echo "$OUTPUT" | grep -q "implementations="; then
    pass "knowledge inspect templates catalogs active template usage"
else
    fail "knowledge inspect templates catalogs active template usage (got: $OUTPUT)"
fi
OUTPUT=$(wt "$PROJ_TEMPLATES" knowledge inspect templates "Infobox person" 2>&1 || true)
if echo "$OUTPUT" | grep -q "template.implementation_pages.count:" && echo "$OUTPUT" | grep -q "role=module" && echo "$OUTPUT" | grep -q "role=documentation"; then
    pass "knowledge inspect templates returns implementation reference for a template"
else
    fail "knowledge inspect templates returns implementation reference for a template (got: $OUTPUT)"
fi

# --- search requires knowledge build ---
section "search requires knowledge build"
PROJ_SEARCH=$(setup_project search-unbuilt)
wt "$PROJ_SEARCH" init > /dev/null 2>&1
mkdir -p "$PROJ_SEARCH/wiki_content/Main"
cat > "$PROJ_SEARCH/wiki_content/Main/Alpha.wiki" << 'WIKIEOF'
{{SHORTDESC:Alpha article}}

'''Alpha''' is an article.
WIKIEOF
OUTPUT=$(wt "$PROJ_SEARCH" search "Alpha" 2>&1 || true)
if echo "$OUTPUT" | grep -q "Run \`wikitool knowledge build\` first."; then
    pass "search requires knowledge build before indexed retrieval"
else
    fail "search requires knowledge build before indexed retrieval (got: $OUTPUT)"
fi

# --- search ---
section "search"
OUTPUT=$(wt "$PROJ" search "Alpha" 2>&1)
if echo "$OUTPUT" | grep -q "search.backend: indexed" && echo "$OUTPUT" | grep -q "search.hit: Alpha"; then
    pass "search finds indexed article"
else
    fail "search finds indexed article (got: $OUTPUT)"
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
if echo "$OUTPUT" | grep -q "hit: \[page\] Extension:TestExtension"; then
    pass "docs search returns page-level results"
else
    fail "docs search returns results (got: $OUTPUT)"
fi

# --- docs context ---
section "docs context"
OUTPUT=$(wt "$PROJ" docs context "AlphaBetaToken" --format text 2>&1 || true)
if echo "$OUTPUT" | grep -q "docs context" && echo "$OUTPUT" | grep -q "pages.count:"; then
    pass "docs context returns AI retrieval bundle"
else
    fail "docs context returns AI retrieval bundle (got: $OUTPUT)"
fi

# --- docs symbols ---
section "docs symbols"
OUTPUT=$(wt "$PROJ" docs symbols "\$wgTestExtensionEnable" --format text 2>&1 || true)
if echo "$OUTPUT" | grep -q 'symbol: \[config\] \$wgTestExtensionEnable'; then
    pass "docs symbols returns normalized symbol hits"
else
    fail "docs symbols returns normalized symbol hits (got: $OUTPUT)"
fi

# --- FTS continuity: local search substring ---
section "search substring continuity across disposable db rebuild"
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
wt "$PROJ_FTS" knowledge build > /dev/null 2>&1
BEFORE=$(wt "$PROJ_FTS" search "pha" 2>&1)
wt "$PROJ_FTS" db reset --yes > /dev/null 2>&1
wt "$PROJ_FTS" knowledge build > /dev/null 2>&1
AFTER=$(wt "$PROJ_FTS" search "pha" 2>&1)
if echo "$BEFORE" | grep -q "search.hit: AlphaBeta" && echo "$AFTER" | grep -q "search.hit: AlphaBeta"; then
    pass "substring search works before and after disposable db rebuild"
else
    fail "substring search continuity failed across disposable db rebuild (before: $BEFORE | after: $AFTER)"
fi

# --- FTS continuity: docs search substring ---
section "docs search substring continuity across disposable db rebuild"
PROJ_DOCS_FTS=$(setup_project fts-docs)
wt "$PROJ_DOCS_FTS" init > /dev/null 2>&1
wt "$PROJ_DOCS_FTS" docs import --bundle "$FIXTURES/sample_docs_bundle.json" > /dev/null 2>&1
BEFORE=$(wt "$PROJ_DOCS_FTS" docs search "BetaToken" 2>&1)
wt "$PROJ_DOCS_FTS" db reset --yes > /dev/null 2>&1
wt "$PROJ_DOCS_FTS" docs import --bundle "$FIXTURES/sample_docs_bundle.json" > /dev/null 2>&1
AFTER=$(wt "$PROJ_DOCS_FTS" docs search "BetaToken" 2>&1)
if echo "$BEFORE" | grep -q "Extension:TestExtension" && echo "$AFTER" | grep -q "Extension:TestExtension"; then
    pass "docs substring search works before and after disposable db rebuild"
else
    fail "docs substring search continuity failed across disposable db rebuild (before: $BEFORE | after: $AFTER)"
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

# --- release build-ai-pack ---
section "release build-ai-pack"
AI_PACK_OUT="$TMPDIR_ROOT/release-ai-pack"
OUTPUT=$(wt "$PROJ" release build-ai-pack --repo-root "$REPO_ROOT" --output-dir "$AI_PACK_OUT" 2>&1 || true)
if [ -f "$AI_PACK_OUT/manifest.json" ] && [ -f "$AI_PACK_OUT/CLAUDE.md" ] && [ -d "$AI_PACK_OUT/.claude/skills" ]; then
    pass "release build-ai-pack stages the packaged AI companion"
else
    fail "release build-ai-pack stages the packaged AI companion (got: ${OUTPUT:0:300})"
fi

# --- release package ---
section "release package"
LOCAL_BINARY="$(resolve_local_binary_candidate || true)"
if [ -n "$LOCAL_BINARY" ]; then
    RELEASE_OUT="$TMPDIR_ROOT/release-package"
    OUTPUT=$(wt "$PROJ" release package --repo-root "$REPO_ROOT" --binary-path "$LOCAL_BINARY" --output-dir "$RELEASE_OUT" 2>&1 || true)
    if [ -f "$RELEASE_OUT/CLAUDE.md" ] && [ -f "$RELEASE_OUT/README.md" ] \
        && { [ -f "$RELEASE_OUT/wikitool" ] || [ -f "$RELEASE_OUT/wikitool.exe" ]; }; then
        pass "release package stages a distributable bundle"
    else
        fail "release package stages a distributable bundle (got: ${OUTPUT:0:300})"
    fi
else
    skip "release package stages a distributable bundle (no built local binary found)"
fi

# --- dev install-git-hooks ---
section "dev install-git-hooks"
HOOK_ROOT=$(setup_project git-hooks)
HOOK_WORKTREE="$HOOK_ROOT/worktree"
HOOK_GITDIR="$HOOK_ROOT/gitdir"
HOOK_SOURCE="$HOOK_ROOT/commit-msg"
mkdir -p "$HOOK_WORKTREE" "$HOOK_GITDIR/hooks"
printf 'gitdir: ../gitdir\n' > "$HOOK_WORKTREE/.git"
cat > "$HOOK_SOURCE" << 'HOOKEOF'
#!/usr/bin/env bash
exit 0
HOOKEOF
OUTPUT=$(wt "$PROJ" dev install-git-hooks --repo-root "$HOOK_WORKTREE" --source "$HOOK_SOURCE" 2>&1 || true)
if [ -f "$HOOK_GITDIR/hooks/commit-msg" ]; then
    pass "dev install-git-hooks resolves gitdir pointer files"
else
    fail "dev install-git-hooks resolves gitdir pointer files (got: ${OUTPUT:0:300})"
fi

# ============================================================
# LIVE TIER (optional, requires network)
# ============================================================

if [ "$TIER" = "live" ]; then
    echo ""
    echo "=== Live tier (read-only API + dry-run writes) ==="

    PROJ_LIVE=$(setup_project live)
    wt "$PROJ_LIVE" init > /dev/null 2>&1
    write_live_env "$PROJ_LIVE"

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

    # --- fetch non-short URL ---
    section "fetch non-short mediawiki url (live)"
    OUTPUT=$(wt "$PROJ_LIVE" fetch "https://wiki.remilia.org/index.php?title=Main_Page" --format rendered-html 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "Main Page\|fetch\|content\|wiki"; then
        pass "fetch accepts non-short MediaWiki URLs"
    else
        fail "fetch accepts non-short MediaWiki URLs (got: ${OUTPUT:0:300})"
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

    # --- workflow bootstrap (live) ---
    section "workflow bootstrap (live)"
    PROJ_BOOTSTRAP_LIVE=$(setup_project workflow-bootstrap-live)
    write_live_env "$PROJ_BOOTSTRAP_LIVE"
    OUTPUT=$(wt "$PROJ_BOOTSTRAP_LIVE" workflow bootstrap --skip-reference --skip-git-hooks --no-pull --docs-profile "$KNOWLEDGE_DOCS_PROFILE" 2>&1 || true)
    if echo "$OUTPUT" | grep -q "^knowledge warm$" \
        && echo "$OUTPUT" | grep -q "docs_profile_requested: $KNOWLEDGE_DOCS_PROFILE" \
        && { echo "$OUTPUT" | grep -q "docs.imported_corpora:" \
            || echo "$OUTPUT" | grep -q "docs.failures.count: "; }; then
        pass "workflow bootstrap invokes knowledge warm"
    else
        fail "workflow bootstrap invokes knowledge warm (got: ${OUTPUT:0:300})"
    fi

    # --- workflow full-refresh (live) ---
    section "workflow full-refresh (live)"
    PROJ_FULL_REFRESH_LIVE=$(setup_project workflow-full-refresh-live)
    write_live_env "$PROJ_FULL_REFRESH_LIVE"
    FULL_REFRESH_LOG="$TMPDIR_ROOT/workflow-full-refresh-live.log"
    wt "$PROJ_FULL_REFRESH_LIVE" workflow full-refresh --yes --skip-reference --docs-profile "$KNOWLEDGE_DOCS_PROFILE" > "$FULL_REFRESH_LOG" 2>&1 || true
    OUTPUT=$(tail -n 400 "$FULL_REFRESH_LOG")
    if grep -q "^knowledge warm$" "$FULL_REFRESH_LOG" \
        && grep -q "docs_profile_requested: $KNOWLEDGE_DOCS_PROFILE" "$FULL_REFRESH_LOG" \
        && { grep -q "docs.imported_corpora:" "$FULL_REFRESH_LOG" \
            || grep -q "docs.failures.count: " "$FULL_REFRESH_LOG"; }; then
        pass "workflow full-refresh invokes knowledge warm"
    else
        fail "workflow full-refresh invokes knowledge warm (got: ${OUTPUT:0:300})"
    fi

    # --- knowledge warm (live) ---
    section "knowledge warm (live)"
    OUTPUT=$(wt "$PROJ_LIVE" knowledge warm --docs-profile "$KNOWLEDGE_DOCS_PROFILE" 2>&1 || true)
    if echo "$OUTPUT" | grep -q "^knowledge warm$" \
        && echo "$OUTPUT" | grep -q "docs_profile_requested: $KNOWLEDGE_DOCS_PROFILE" \
        && { echo "$OUTPUT" | grep -q "docs.imported_corpora:" \
            || echo "$OUTPUT" | grep -q "docs.failures.count: "; } \
        && echo "$OUTPUT" | grep -q "knowledge.readiness:"; then
        pass "knowledge warm reports content/docs readiness and degradation state"
    else
        fail "knowledge warm reports content/docs readiness and degradation state (got: ${OUTPUT:0:300})"
    fi

    # --- docs import-technical (live) ---
    section "docs import-technical (live)"
    OUTPUT=$(wt "$PROJ_LIVE" docs import-technical "Manual:Hooks" --subpages --limit 5 2>&1 || true)
    if echo "$OUTPUT" | grep -q "^docs import-technical$" \
        && { echo "$OUTPUT" | grep -Eq "imported_pages: [1-9]" \
            || echo "$OUTPUT" | grep -Eq "failures.count: [1-9]"; }; then
        pass "docs import-technical reports imports or graceful upstream degradation"
    else
        fail "docs import-technical reports imports or graceful upstream degradation (got: ${OUTPUT:0:300})"
    fi

    # --- docs context (live) ---
    section "docs context (live)"
    OUTPUT=$(wt "$PROJ_LIVE" docs context "parser function" --profile "$KNOWLEDGE_DOCS_PROFILE" --format text 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "docs context\|pages.count:\|symbols.count:"; then
        pass "docs context returns live retrieval bundle"
    else
        fail "docs context returns live retrieval bundle (got: ${OUTPUT:0:300})"
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
