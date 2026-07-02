use std::path::Path;

use super::{Classifier, builtin_classifier, resolve_matcher};

fn classifier(name: &str) -> Classifier {
    builtin_classifier(name).unwrap_or_else(|| panic!("missing outline language {name}"))
}

fn assert_matches(name: &str, lines: &[&str]) {
    let classify = classifier(name);
    for line in lines {
        assert!(classify(line), "{name} outline should match: {line}");
    }
}

fn assert_skips(name: &str, lines: &[&str]) {
    let classify = classifier(name);
    for line in lines {
        assert!(!classify(line), "{name} outline should skip: {line}");
    }
}

#[test]
fn rust_classifier_matches_declaration_shapes() {
    assert_matches(
        "rust",
        &[
            "fn main() {",
            "pub fn command_slice(",
            "pub(crate) async fn fetch() -> Result<()> {",
            "    pub(crate) unsafe fn poke(&mut self) {",
            "struct FileMatch {",
            "pub enum Command {",
            "impl TextMatcher {",
            "impl<T> Render for T {",
            "pub trait Render {",
            "mod tests;",
            "pub(crate) type Rows = Vec<Row>;",
            "const NESTED_REPOS_RECEIPT_LIMIT: usize = 12;",
            "pub static GLOBAL: OnceLock<u32> = OnceLock::new();",
            "macro_rules! receipt {",
            "pub const fn width() -> usize {",
            "pub extern \"C\" fn callback(data: *mut c_void) {",
        ],
    );
    assert_skips(
        "rust",
        &[
            "    let value = compute();",
            "// struct layout notes",
            "/// fn docs mention fn here",
            "        samples.push(json!({",
            "use std::path::PathBuf;",
            "pubx fn not_a_decl() {",
            "    functional(value);",
        ],
    );
}

#[test]
fn python_lua_shell_and_markdown_classifiers_match_declaration_shapes() {
    assert_matches(
        "python",
        &[
            "def run(args):",
            "    async def fetch(self):",
            "class Harness:",
        ],
    );
    assert_skips("python", &["    result = defaults()", "definitely = 1"]);

    assert_matches(
        "lua",
        &[
            "function Frame:OnLoad()",
            "local function clamp(value)",
            "Mixin.OnEvent = function(self, event)",
        ],
    );
    assert_skips("lua", &["    return functional(value)", "local x = 5"]);

    assert_matches(
        "shell",
        &[
            "resolve_workspace_path() {",
            "function warn_dump",
            "usage()",
        ],
    );
    assert_skips(
        "shell",
        &["  echo \"listing()\"", "run_tool() { echo hi; }"],
    );

    assert_matches("markdown", &["## Commands", "# Title"]);
    assert_skips("markdown", &["#comment-not-heading", "####### seven"]);

    assert_matches("toml", &["[workspace]", "  [profile.test]"]);
    assert_skips("toml", &["key = \"value\""]);
}

#[test]
fn config_and_markup_classifiers_match_structure_lines() {
    assert_matches("ini", &["[core]", "  [remote \"origin\"]"]);
    assert_skips("ini", &["key = value", "; comment"]);

    assert_matches(
        "yaml",
        &[
            "jobs:",
            "name: Release Artifacts",
            "on:",
            "\"quoted key\": value",
        ],
    );
    assert_skips(
        "yaml",
        &[
            "  steps:",
            "# comment: not a key",
            "- list: item",
            "plain scalar line",
        ],
    );

    assert_matches(
        "sql",
        &[
            "CREATE TABLE contract_claims (",
            "  create index idx_claims on contract_claims(spec_id);",
            "ALTER TABLE evidence_objects ADD COLUMN role TEXT;",
            "DROP VIEW stale_units;",
        ],
    );
    assert_skips(
        "sql",
        &[
            "SELECT * FROM contract_claims;",
            "INSERT INTO t VALUES (1);",
            "  created_at TEXT NOT NULL,",
        ],
    );

    assert_matches(
        "wikitext",
        &[
            "== Commands ==",
            "= Title =",
            "====== Deep ======",
            "==Tight==",
        ],
    );
    assert_skips(
        "wikitext",
        &[
            "= no closing run",
            "======= seven =======",
            "== ==",
            "body text with = equals",
        ],
    );
}

#[test]
fn c_family_classifier_matches_definitions_not_prototypes() {
    assert_matches(
        "c",
        &[
            "typedef struct FT_FaceRec_ FT_Face;",
            "struct CGxDevice {",
            "#define FT_LOAD_DEFAULT 0x0",
            "static int raster_render(RAS_ARG_ int flipped)",
            "void CWorldScene_CullGlobalMapObjDefGroups(float* bounds, int pass)",
            "template <typename T>",
            "CGxDevice::~CGxDevice()",
        ],
    );
    assert_skips(
        "c",
        &[
            "int prototype_only(int value);",
            "    call_site(value);",
            "// void commentary(int x)",
            "return frame(count)",
        ],
    );
}

#[test]
fn js_go_and_java_family_classifiers_match_declaration_shapes() {
    assert_matches(
        "typescript",
        &[
            "export default async function boot() {",
            "export class Renderer {",
            "  interface FrameState {",
            "type RowFilter = (row: Row) => boolean;",
            "export const clamp = (value: number): number =>",
            "const onEvent = async (event) => {",
            "let handler = function (event) {",
            "const forward = event => dispatch(event);",
        ],
    );
    assert_skips(
        "typescript",
        &[
            "  const total = rows.length;",
            "  value === expected;",
            "return callback(value);",
        ],
    );

    assert_matches(
        "go",
        &[
            "func (s *Server) Run() error {",
            "type Config struct {",
            "var ErrClosed = errors.New(\"closed\")",
        ],
    );
    assert_skips("go", &["\tfunc := notADecl()", "  var indented = 1"]);

    assert_matches(
        "java",
        &[
            "public final class Renderer {",
            "    private static int frameCount(FrameState state) {",
            "public @interface Inject {",
        ],
    );
    assert_skips(
        "java",
        &["        state.frameCount(1);", "return frame(count);"],
    );

    assert_matches(
        "csharp",
        &[
            "public sealed partial class Renderer {",
            "    public int FrameCount { get; }",
            "internal static void Main(string[] args) {",
        ],
    );
    assert_skips("csharp", &["        state.Render();"]);

    assert_matches(
        "kotlin",
        &[
            "class Renderer(private val device: Device) {",
            "    suspend fun fetch(): Frame {",
            "enum class Mode { A, B }",
            "companion object {",
            "data class Point(val x: Int)",
        ],
    );
    assert_skips("kotlin", &["    val total = rows.size", "enum { }"]);

    assert_matches("ruby", &["def render", "class Renderer", "module Mink"]);
    assert_skips("ruby", &["  render_all!", "defer_work"]);
}

#[test]
fn resolve_matcher_prefers_pattern_then_prefix_then_lang_then_extension() {
    let (name, matcher) =
        resolve_matcher(Path::new("notes.weird"), None, None, None, Some(r"^\d+:")).unwrap();
    assert_eq!(name, "pattern");
    assert!(matcher.is_match("12: entry"));

    let (name, matcher) =
        resolve_matcher(Path::new("notes.weird"), None, None, Some("// PART"), None).unwrap();
    assert_eq!(name, "prefix");
    assert!(matcher.is_match("  // PART 1: strides"));
    assert!(!matcher.is_match("intro // PART"));

    let (name, _) =
        resolve_matcher(Path::new("notes.weird"), None, Some("Rust"), None, None).unwrap();
    assert_eq!(name, "rust");

    let (name, _) = resolve_matcher(Path::new("src/MAIN.RS"), None, None, None, None).unwrap();
    assert_eq!(name, "rust");
}

#[test]
fn resolve_matcher_detects_shebang_for_extensionless_scripts() {
    let (name, matcher) = resolve_matcher(
        Path::new("scripts/launcher"),
        Some("#!/usr/bin/env bash"),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(name, "shell");
    assert!(matcher.is_match("main() {"));

    let (name, _) = resolve_matcher(
        Path::new("scripts/tool"),
        Some("#!/usr/bin/python3.11"),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(name, "python");

    let (name, _) = resolve_matcher(
        Path::new("hook"),
        Some("#!/usr/bin/env node"),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(name, "javascript");

    // A known extension wins over the shebang; an unknown interpreter fails.
    let (name, _) = resolve_matcher(
        Path::new("gen.rs"),
        Some("#!/usr/bin/env python3"),
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(name, "rust");
    let error = resolve_matcher(
        Path::new("run"),
        Some("#!/usr/bin/env pwsh"),
        None,
        None,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("shebang"), "{error}");
}

#[test]
fn resolve_matcher_fails_fast_on_unknown_surface() {
    let error = resolve_matcher(Path::new("notes.weird"), None, None, None, None).unwrap_err();
    assert!(error.to_string().contains("--lang"), "{error}");

    let error = resolve_matcher(Path::new("main.rs"), None, Some("cobol"), None, None).unwrap_err();
    assert!(error.to_string().contains("not supported"), "{error}");

    let error =
        resolve_matcher(Path::new("main.rs"), None, None, None, Some("[unclosed")).unwrap_err();
    assert!(error.to_string().contains("invalid outline"), "{error}");

    let error = resolve_matcher(Path::new("main.rs"), None, None, Some(""), None).unwrap_err();
    assert!(error.to_string().contains("non-empty"), "{error}");
}
