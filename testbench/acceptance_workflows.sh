#!/usr/bin/env bash
# Focused acceptance checks for the post-cutover authoring workflow.
# Usage:
#   TIER=offline bash testbench/acceptance_workflows.sh
#   TIER=live    bash testbench/acceptance_workflows.sh
set -euo pipefail

TIER="${TIER:-offline}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
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

    # On WSL/Git Bash, prefer the host Rust toolchain when it is available.
    # A distro Cargo may be older than the workspace's declared Rust edition,
    # while the surrounding Windows checkout is already built with cargo.exe.
    if command -v cargo.exe > /dev/null 2>&1; then
        WIKITOOL_CMD=(cargo.exe run --quiet --)
        WIKITOOL_PATH_MODE="windows"
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

write_site_adapter() {
    local root="$1"
    mkdir -p "$root/site-adapter"
    cp "$REPO_ROOT/crates/wikitool_core/testdata/site-adapter.toml" "$root/site-adapter/profile.toml"
    sed -i.bak 's|# path = "site-adapter/profile.toml"|path = "site-adapter/profile.toml"|' "$root/.wikitool/config.toml"
    rm -f "$root/.wikitool/config.toml.bak"
}

write_minimal_templates() {
    local root="$1"
    mkdir -p "$root/templates/misc" "$root/templates/infobox" "$root/templates/cite"
    cat > "$root/templates/misc/Template_Article_quality.wiki" << 'WIKIEOF'
<includeonly>{{{1|unverified}}}</includeonly>
WIKIEOF
    cat > "$root/templates/misc/Template_Reflist.wiki" << 'WIKIEOF'
<references />
WIKIEOF
    cat > "$root/templates/infobox/Template_Infobox_NFT_collection.wiki" << 'WIKIEOF'
<includeonly>{{{name|}}} {{{parent_group|}}}</includeonly>
WIKIEOF
    cat > "$root/templates/cite/Template_Cite_web.wiki" << 'WIKIEOF'
<includeonly>{{{title|}}} {{{url|}}}</includeonly>
WIKIEOF
}

write_live_env() {
    local root="$1"
    cat > "$root/.env" << 'ENVEOF'
WIKITOOL_WIKI_URL=https://wiki.remilia.org
WIKITOOL_WIKI_API_URL=https://wiki.remilia.org/api.php
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
OUTPUT=$(wt "$PROJ" knowledge article-start "Gamma" --format json --view brief 2>&1 || true)
if echo "$OUTPUT" | grep -q '"schema_version": "wikitool_brief_v1"' \
    && echo "$OUTPUT" | grep -q '"command": "knowledge article-start"' \
    && ! echo "$OUTPUT" | grep -q '"article_type"' \
    && ! echo "$OUTPUT" | grep -q '"confidence"' \
    && echo "$OUTPUT" | grep -q '"required_templates"' \
    && echo "$OUTPUT" | grep -Eq '"local_state": "(linked_but_missing|likely_missing)"'; then
    pass "knowledge article-start produces a missing-page authoring brief"
else
    fail "knowledge article-start produces a missing-page authoring brief (got: $OUTPUT)"
fi

section "article-lint"
LINT_PROJ=$(setup_project article-lint)
wt "$LINT_PROJ" init --templates > /dev/null 2>&1
mkdir -p "$LINT_PROJ/wiki_content/Main"
write_site_adapter "$LINT_PROJ"
write_minimal_templates "$LINT_PROJ"
cat > "$LINT_PROJ/wiki_content/Main/Example_Draft.wiki" << 'WIKIEOF'
{{SHORTDESC:Draft article}}
{{Article quality|unverified}}
{{Infobox NFT collection
| name = Example Draft
| creator = Example Studio
}}

## History
'''Example Draft''' is a test<ref>{{Cite web|title=Source|url=https://en.wikipedia.org/wiki/Example}}</ref>.

== References ==
WIKIEOF
OUTPUT=$(wt "$LINT_PROJ" article lint "$LINT_PROJ/wiki_content/Main/Example_Draft.wiki" --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"rule_id": "structure.markdown_heading"' \
    && echo "$OUTPUT" | grep -q '"rule_id": "structure.require_reflist"' \
    && echo "$OUTPUT" | grep -q '"rule_id": "citation.source_review"' \
    && echo "$OUTPUT" | grep -q '"rule_id": "citation.after_punctuation"' \
    && echo "$OUTPUT" | grep -Eq '"content_sha256": "[0-9a-f]{64}"'; then
    pass "article lint reports the expected profile-aware draft issues"
else
    fail "article lint reports the expected profile-aware draft issues (got: $OUTPUT)"
fi

section "exact human acceptance and promotion"
ACCEPT_PROJ=$(setup_project article-accept)
wt "$ACCEPT_PROJ" init --templates > /dev/null 2>&1
write_site_adapter "$ACCEPT_PROJ"
write_minimal_templates "$ACCEPT_PROJ"
mkdir -p "$ACCEPT_PROJ/.wikitool/drafts"
ACCEPT_DRAFT="$ACCEPT_PROJ/.wikitool/drafts/Accepted_Draft.wiki"
cat > "$ACCEPT_DRAFT" << 'WIKIEOF'
{{SHORTDESC:Fictional subject used for an isolated workflow test}}
{{Article quality|unverified}}

'''Accepted Draft''' is a fictional subject used to verify the local publication workflow.

== History ==
The subject was documented for this test.<ref>{{Cite web|title=Workflow fixture|url=https://example.org/workflow}}</ref>

== References ==
{{Reflist}}
WIKIEOF
OUTPUT=$(wt "$ACCEPT_PROJ" article accept "$ACCEPT_DRAFT" --title "Accepted Draft" --human-editor "fixture-editor" --prose-origin agent-draft --format json 2>&1)
if echo "$OUTPUT" | grep -q '"schema_version": "article_accept_v2"' \
    && echo "$OUTPUT" | grep -q '"human_editor_claim": "fixture-editor"' \
    && echo "$OUTPUT" | grep -q '"editor_identity_assurance": "self_reported_unverified"' \
    && echo "$OUTPUT" | grep -q '"prose_origin": "agent_draft"' \
    && echo "$OUTPUT" | grep -Eq '"content_sha256": "[0-9a-f]{64}"' \
    && echo "$OUTPUT" | grep -q '"lint_errors": 0' \
    && echo "$OUTPUT" | grep -q '"lint_warnings": 0'; then
    pass "article accept records a full-hash exact-content ledger decision"
else
    fail "article accept records a full-hash exact-content ledger decision (got: $OUTPUT)"
fi

cp "$ACCEPT_DRAFT" "$ACCEPT_DRAFT.accepted"
printf '\nPost-acceptance mutation.\n' >> "$ACCEPT_DRAFT"
OUTPUT=$(wt "$ACCEPT_PROJ" article promote "$ACCEPT_DRAFT" --title "Accepted Draft" --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q 'changed after the recorded acceptance decision'; then
    pass "article promote rejects prose changed after its ledger decision"
else
    fail "article promote rejects prose changed after its ledger decision (got: $OUTPUT)"
fi
mv "$ACCEPT_DRAFT.accepted" "$ACCEPT_DRAFT"

OUTPUT=$(wt "$ACCEPT_PROJ" article promote "$ACCEPT_DRAFT" --title "Accepted Draft" --format json 2>&1)
PROMOTED="$ACCEPT_PROJ/wiki_content/Main/Accepted_Draft.wiki"
if echo "$OUTPUT" | grep -q '"schema_version": "article_promote_v3"' \
    && echo "$OUTPUT" | grep -q '"prose_origin": "agent_draft"' \
    && cmp -s "$ACCEPT_DRAFT" "$PROMOTED"; then
    pass "article promote consumes the exact accepted snapshot"
else
    fail "article promote consumes the exact accepted snapshot (got: $OUTPUT)"
fi

WARNING_DRAFT="$ACCEPT_PROJ/.wikitool/drafts/Warning_Draft.wiki"
cat > "$WARNING_DRAFT" << 'WIKIEOF'
{{SHORTDESC:Fictional subject with a source-review warning}}
{{Article quality|unverified}}

'''Warning Draft''' is a fictional subject used to verify explicit warning acceptance.

== History ==
The subject was documented for this test.<ref>{{Cite web|title=Warning fixture|url=https://en.wikipedia.org/wiki/Example}}</ref>

== References ==
{{Reflist}}
WIKIEOF
OUTPUT=$(wt "$ACCEPT_PROJ" article accept "$WARNING_DRAFT" --title "Warning Draft" --human-editor "fixture-editor" --prose-origin collaborative-draft --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q 'resolve them or explicitly record their acceptance'; then
    pass "article accept rejects warnings without explicit acknowledgement"
else
    fail "article accept rejects warnings without explicit acknowledgement (got: $OUTPUT)"
fi
OUTPUT=$(wt "$ACCEPT_PROJ" article accept "$WARNING_DRAFT" --title "Warning Draft" --human-editor "fixture-editor" --prose-origin collaborative-draft --allow-warnings --format json 2>&1)
if echo "$OUTPUT" | grep -q '"warnings_explicitly_accepted": true' \
    && echo "$OUTPUT" | grep -Eq '"lint_warnings": [1-9][0-9]*'; then
    pass "article accept records an explicit caller warning decision"
else
    fail "article accept records an explicit caller warning decision (got: $OUTPUT)"
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
OUTPUT=$(wt "$LIVE_PROJ" research wiki-search "Remilia" --format json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"schema_version": "research_search_v1"' && echo "$OUTPUT" | grep -q '"query": "Remilia"' && echo "$OUTPUT" | grep -q '"count":'; then
    pass "research wiki-search returns structured live search output"
else
    fail "research wiki-search returns structured live search output (got: $OUTPUT)"
fi

section "research-fetch"
OUTPUT=$(wt "$LIVE_PROJ" research fetch "https://wiki.remilia.org/wiki/Main_Page" --format rendered-html --output json 2>&1 || true)
if echo "$OUTPUT" | grep -q '"schema_version": "research_document_v2"' && echo "$OUTPUT" | grep -q '"rendered_fetch_mode": "parse_api"' && echo "$OUTPUT" | grep -q '"revision_id":'; then
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
