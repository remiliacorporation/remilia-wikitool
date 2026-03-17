#!/usr/bin/env bash
# Focused acceptance checks for the post-cutover authoring workflow.
# Usage:
#   TIER=offline bash testbench/acceptance_workflows.sh
#   TIER=live    bash testbench/acceptance_workflows.sh
set -euo pipefail

TIER="${TIER:-offline}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WIKITOOL_RAW="${WIKITOOL:-}"
TMP_BASE="${TMPDIR:-$SCRIPT_DIR/.tmp}"
mkdir -p "$TMP_BASE"
TMPDIR_ROOT=$(mktemp -d "$TMP_BASE/wikitool-acceptance-XXXXXX")

PASS=0
FAIL=0
SKIP=0

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
        # shellcheck disable=SC2206
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

setup_project() {
    local dir="$TMPDIR_ROOT/project-$1"
    mkdir -p "$dir"
    echo "$dir"
}

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

write_authoring_guidance() {
    local root="$1"
    mkdir -p "$root/tools/wikitool/ai-pack/llm_instructions"
    cat > "$root/tools/wikitool/ai-pack/llm_instructions/article_structure.md" << 'MDEOF'
{{SHORTDESC:Example}}
{{Article quality|unverified}}
== References ==
{{Reflist}}
parent_group = Remilia
MDEOF
    cat > "$root/tools/wikitool/ai-pack/llm_instructions/style_rules.md" << 'MDEOF'
### No placeholder content
- Never output: `INSERT_SOURCE_URL`
Straight quotes only
MDEOF
    cat > "$root/tools/wikitool/ai-pack/llm_instructions/writing_guide.md" << 'MDEOF'
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
}

write_minimal_templates() {
    local root="$1"
    mkdir -p "$root/templates/misc" "$root/templates/infobox"
    cat > "$root/templates/misc/Template_Article_quality.wiki" << 'WIKIEOF'
<includeonly>{{{1|unverified}}}</includeonly>
WIKIEOF
    cat > "$root/templates/misc/Template_Reflist.wiki" << 'WIKIEOF'
<references />
WIKIEOF
    cat > "$root/templates/infobox/Template_Infobox_NFT_collection.wiki" << 'WIKIEOF'
<includeonly>{{{name|}}} {{{parent_group|}}}</includeonly>
WIKIEOF
}

write_live_env() {
    local root="$1"
    cat > "$root/.env" << 'ENVEOF'
WIKI_URL=https://wiki.remilia.org/
WIKI_API_URL=https://wiki.remilia.org/api.php
ENVEOF
}

WIKITOOL_CMD=()
WIKITOOL_PATH_MODE=""
resolve_wikitool_cmd

echo "=== wikitool targeted acceptance checks ==="
echo "Tier: $TIER | Temp: $TMPDIR_ROOT"

section "article-start"
PROJ=$(setup_project article-start)
wt "$PROJ" init --templates > /dev/null 2>&1
mkdir -p "$PROJ/wiki_content/Main"
cat > "$PROJ/wiki_content/Main/Alpha.wiki" << 'WIKIEOF'
{{SHORTDESC:Alpha article}}

'''Alpha''' is a local page that mentions [[Gamma]].<ref>{{Cite web|title=Alpha Source|url=https://example.org/alpha}}</ref>

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
cat > "$PROJ/wiki_content/Main/Beta.wiki" << 'WIKIEOF'
{{SHORTDESC:Beta article}}

'''Beta''' is a related page linked from [[Alpha]].

== References ==
{{Reflist}}

[[Category:Test]]
WIKIEOF
OUTPUT=$(wt "$PROJ" knowledge build 2>&1)
if echo "$OUTPUT" | grep -q "knowledge.readiness: content_ready"; then
    pass "knowledge build prepares the local index for authoring acceptance"
else
    fail "knowledge build prepares the local index for authoring acceptance (got: $OUTPUT)"
fi
OUTPUT=$(wt "$PROJ" knowledge article-start "Gamma" --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"schema_version": "article_start_v1"' \
    && echo "$OUTPUT" | grep -Eq '"local_state": "(linked_but_missing|likely_missing)"'; then
    pass "knowledge article-start produces a missing-page authoring brief"
else
    fail "knowledge article-start produces a missing-page authoring brief (got: $OUTPUT)"
fi

section "article-lint"
LINT_PROJ=$(setup_project article-lint)
wt "$LINT_PROJ" init --templates > /dev/null 2>&1
mkdir -p "$LINT_PROJ/wiki_content/Main"
write_authoring_guidance "$LINT_PROJ"
write_minimal_templates "$LINT_PROJ"
cat > "$LINT_PROJ/wiki_content/Main/Milady_Draft.wiki" << 'WIKIEOF'
{{SHORTDESC:Draft article}}
{{Article quality|unverified}}
{{Infobox NFT collection
| name = Milady Draft
| creator = Remilia
}}

## History
'''Milady Draft''' is a test<ref>{{Cite web|title=Source|url=https://example.org/source}}</ref>.

== References ==
WIKIEOF
OUTPUT=$(wt "$LINT_PROJ" article lint "$LINT_PROJ/wiki_content/Main/Milady_Draft.wiki" --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"rule_id": "structure.markdown_heading"' && echo "$OUTPUT" | grep -q '"rule_id": "profile.remilia_parent_group"' && echo "$OUTPUT" | grep -q '"rule_id": "structure.require_reflist"'; then
    pass "article lint reports the expected profile-aware draft issues"
else
    fail "article lint reports the expected profile-aware draft issues (got: $OUTPUT)"
fi

if [ "$TIER" != "live" ]; then
    echo
    printf "=== \033[32mRESULTS: %d passed, %d failed, %d skipped\033[0m ===\n" "$PASS" "$FAIL" "$SKIP"
    if [ "$FAIL" -ne 0 ]; then
        exit 1
    fi
    exit 0
fi

section "research-search"
LIVE_PROJ=$(setup_project live)
wt "$LIVE_PROJ" init --templates > /dev/null 2>&1
write_live_env "$LIVE_PROJ"
OUTPUT=$(wt "$LIVE_PROJ" research search "Remilia" --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"schema_version": "research_search_v1"' && echo "$OUTPUT" | grep -q '"query": "Remilia"' && echo "$OUTPUT" | grep -q '"count":'; then
    pass "research search returns structured live search output"
else
    fail "research search returns structured live search output (got: $OUTPUT)"
fi

section "research-fetch"
OUTPUT=$(wt "$LIVE_PROJ" research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"schema_version": "research_document_v1"' && echo "$OUTPUT" | grep -q '"rendered_fetch_mode": "parse_api"' && echo "$OUTPUT" | grep -q '"revision_id":'; then
    pass "research fetch returns rendered live wiki content with metadata"
else
    fail "research fetch returns rendered live wiki content with metadata (got: $OUTPUT)"
fi

section "wiki-capabilities"
OUTPUT=$(wt "$LIVE_PROJ" wiki capabilities sync --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"schema_version": "wiki_capabilities_v1"' && echo "$OUTPUT" | grep -q '"wiki_id": "wiki.remilia.org"' && echo "$OUTPUT" | grep -q '"mediawiki_version":'; then
    pass "wiki capabilities sync returns the live capability manifest"
else
    fail "wiki capabilities sync returns the live capability manifest (got: $OUTPUT)"
fi

echo
printf "=== \033[32mRESULTS: %d passed, %d failed, %d skipped\033[0m ===\n" "$PASS" "$FAIL" "$SKIP"
if [ "$FAIL" -ne 0 ]; then
    exit 1
fi
