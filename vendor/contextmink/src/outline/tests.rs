use std::path::Path;

use super::{DocumentClassifier, LanguageRule, builtin_rule, resolve_matcher};

fn line_matches(name: &str, line: &str) -> bool {
    match builtin_rule(name).unwrap_or_else(|| panic!("missing outline language {name}")) {
        LanguageRule::Line(classify) => classify(line),
        // Document rules still classify a one-line fragment for shapes that
        // need no cross-line context.
        LanguageRule::Document(classify) => classify(line).contains(&0),
    }
}

fn assert_matches(name: &str, lines: &[&str]) {
    for line in lines {
        assert!(
            line_matches(name, line),
            "{name} outline should match: {line}"
        );
    }
}

fn assert_skips(name: &str, lines: &[&str]) {
    for line in lines {
        assert!(
            !line_matches(name, line),
            "{name} outline should skip: {line}"
        );
    }
}

fn document_classifier(name: &str) -> DocumentClassifier {
    match builtin_rule(name).unwrap_or_else(|| panic!("missing outline language {name}")) {
        LanguageRule::Document(classify) => classify,
        LanguageRule::Line(_) => panic!("{name} is a line-rule language"),
    }
}

/// 1-based declaration line numbers for readable assertions.
fn document_hits(name: &str, text: &str) -> Vec<usize> {
    document_classifier(name)(text)
        .into_iter()
        .map(|index| index + 1)
        .collect()
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
            // Column-0 table roots: addon/module structure.
            "TraceRoot = {}",
            "local p = {}",
            "TraceStore = TraceStore or {}",
            "local Defaults = {",
        ],
    );
    assert_skips(
        "lua",
        &[
            "    return functional(value)",
            "local x = 5",
            // Indented table assignments are locals inside functions;
            // one-liner closed tables are data.
            "    local row = {}",
            "local colors = { 1, 2, 3 }",
            "VERSION = \"1.0\"",
        ],
    );

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
            "CREATE TABLE records (",
            "  create index idx_records on records(spec_id);",
            "ALTER TABLE artifacts ADD COLUMN role TEXT;",
            "DROP VIEW stale_units;",
        ],
    );
    assert_skips(
        "sql",
        &[
            "SELECT * FROM records;",
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
fn c_family_classifier_matches_definitions_prototypes_and_members() {
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
            // Prototypes carry a header's structure.
            "int prototype_only(int value);",
            "void Grid_GetFacets(Vector3 const* min, Vector3 const* max);",
            // Indented class members, nested aggregates, and access labels.
            "    virtual void Render(CGxBatch* batch) = 0;",
            "    void Resize(int width, int height) {",
            "    struct SubRange {",
            "public:",
            "    private:",
            // Operator overloads name themselves with punctuation.
            "bool operator==(const C3Vector& other) const;",
            "    T& operator[](size_t index) {",
        ],
    );
    assert_skips(
        "c",
        &[
            "    call_site(value);",
            "    ns::qualified_call(value);",
            "// void commentary(int x)",
            "return frame(count)",
            "    if (bounds->contains(point)) {",
            "    int x = compute(value);",
            "    result += accumulate(rows);",
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
            "const dispatch: () => void = () => {",
            "const build: Array<() => void> = () => {",
            // Class and object-literal method heads.
            "  render(frame, camera) {",
            "  async load(url) {",
            "  static create(options) {",
            "  get frameCount() {",
            "  #recompute() {",
            "  constructor(device) {",
            "  map<T>(fn: (row: Row) => T): T[] {",
            "  update() {}",
        ],
    );
    assert_skips(
        "typescript",
        &[
            "  const total = rows.length;",
            "  value === expected;",
            "return callback(value);",
            "const flag: boolean = value === expected;",
            // Calls end with `;`/`)`; object-argument and callback calls
            // leave the parentheses unbalanced before the trailing `{`.
            "  render(frame);",
            "  fetch(url, {",
            "  it('renders', () => {",
            "  if (ready) {",
            "  while (frames.pop()) {",
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
            // Package-private members and constructors.
            "    void run() {",
            "    Frame nextFrame(long deadline) throws TimeoutException {",
            "    public Renderer(Device device) {",
        ],
    );
    assert_skips(
        "java",
        &[
            "        state.frameCount(1);",
            "return frame(count);",
            "        throw new IllegalStateException(reason);",
            "        builder.append(row).append(sep);",
        ],
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

    assert_matches("ruby", &["def render", "class Renderer", "module Render"]);
    assert_skips("ruby", &["  render_all!", "defer_work"]);
}

#[test]
fn xml_outline_maps_named_and_shallow_containers() {
    // UI XML-shaped document: named containers emit at any depth; unnamed
    // wrapper elements under a named ancestor (Anchors, Layers) stay out;
    // leaves (self-closing, same-line-closed) stay out even when named.
    let ui_xml = concat!(
        "<?xml version=\"1.0\"?>
", // 1
        "<Ui xmlns=\"http://example.invalid/ui\">
", // 2
        "  <!-- named comment: name=\"x\" is not an element -->
", // 3
        "  <Frame name=\"ExplorerFrame\" parent=\"UIParent\">
", // 4
        "    <Size x=\"48\" y=\"28\" />
", // 5
        "    <Anchors>
", // 6
        "      <Anchor point=\"TOPLEFT\"/>
", // 7
        "    </Anchors>
", // 8
        "    <Frames>
", // 9
        "      <Button name=\"$parentClose\" inherits=\"UIPanelCloseButton\">
", // 10
        "        <Texture name=\"ExplorerThumb\" file=\"x\">
", // 11
        "          <TexCoords left=\"0\"/>
", // 12
        "        </Texture>
", // 13
        "      </Button>
", // 14
        "    </Frames>
", // 15
        "    <Property name=\"title\">Main</Property>
", // 16
        "  </Frame>
", // 17
        "</Ui>
", // 18
    );
    assert_eq!(document_hits("xml", ui_xml), vec![2, 4, 10, 11]);

    // Schema definition export: Table containers map, self-closing Field rows
    // (name-attributed leaves) never flood the outline.
    let definition = concat!(
        "<Definition xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">
", // 1
        "  <Table Name=\"Product\" Version=\"1\">
", // 2
        "    <Field Name=\"ID\" Type=\"int\" IsIndex=\"true\" />
", // 3
        "    <Field Name=\"Faction\" Type=\"int\"/>
", // 4
        "  </Table>
", // 5
        "  <Table Name=\"Customer\" Version=\"1\">
", // 6
        "    <Field Name=\"ID\" Type=\"int\" IsIndex=\"true\" />
", // 7
        "  </Table>
", // 8
        "</Definition>
", // 9
    );
    assert_eq!(document_hits("xml", definition), vec![1, 2, 6]);

    // MediaWiki-shaped export: unnamed shallow sections map (root, page,
    // revision); same-line leaf content (title) stays out.
    let mediawiki = concat!(
        "<mediawiki xml:lang=\"en\">
", // 1
        "  <page>
", // 2
        "    <title>Main Page</title>
", // 3
        "    <revision>
", // 4
        "      <text>body</text>
", // 5
        "    </revision>
", // 6
        "  </page>
", // 7
        "</mediawiki>
", // 8
    );
    assert_eq!(document_hits("xml", mediawiki), vec![1, 2, 4]);
}

#[test]
fn xml_outline_handles_multiline_tags_cdata_and_truncation() {
    // MSBuild-shaped: a tag whose attributes span lines anchors at its `<`
    // line; quoted `>` inside attributes does not end the tag; CDATA and
    // DOCTYPE are skipped; an unclosed container at EOF still outlines.
    let msbuild = concat!(
        "<!DOCTYPE project>
", // 1
        "<Project ToolsVersion=\"4.0\">
", // 2
        "  <PropertyGroup
", // 3
        "      Condition=\"'$(A)' > '1'\">
", // 4
        "    <Flag>true</Flag>
", // 5
        "  </PropertyGroup>
", // 6
        "  <Target Name=\"Build\">
", // 7
        "    <![CDATA[ <NotATag name=\"x\"> ]]>
", // 8
        "    <Exec Command=\"echo\"/>
", // 9
    );
    assert_eq!(document_hits("xml", msbuild), vec![2, 3, 7]);
}

#[test]
fn c_outline_maps_section_banners() {
    // Large annotated headers often use section banners as the only
    // navigable structure between declarations.
    let header = concat!(
        "struct CameraState {
", // 1: declaration
        "    // =========================
", // 2: fence
        "    // Projection Parameters
", // 3: fenced banner title
        "    // =========================
", // 4: fence
        "    float near_clip;
", // 5
        "    // ==== Mode / Type State ====
", // 6: one-liner banner
        "    uint32_t mode;
", // 7
        "    // plain commentary
", // 8
        "    // ========
", // 9: bare fence, no title
        "};
", // 10
    );
    let hits = document_hits("c", header);
    assert!(hits.contains(&1), "struct root: {hits:?}");
    assert!(hits.contains(&3), "fenced banner title: {hits:?}");
    assert!(hits.contains(&6), "one-liner banner: {hits:?}");
    for absent in [2, 4, 8, 9] {
        assert!(
            !hits.contains(&absent),
            "line {absent} must stay out: {hits:?}"
        );
    }
}

#[test]
fn json_classifier_maps_container_keys_only() {
    assert_matches(
        "json",
        &[
            "  \"textures\": [",
            "\"diagnostics\": {",
            "        \"nested\": {",
            "  \"key with \\\"quote\\\"\": [",
        ],
    );
    assert_skips(
        "json",
        &[
            "  \"mode\": \"demo\",",
            "  \"count\": 3,",
            "  \"flags\": [1, 2, 3],",
            "  \"empty\": {},",
            "{",
            "  ],",
            "  \"unterminated",
        ],
    );
}

#[test]
fn resolve_matcher_prefers_pattern_then_prefix_then_lang_then_extension() {
    let (name, matcher) =
        resolve_matcher(Path::new("notes.weird"), "", None, None, Some(r"^\d+:")).unwrap();
    assert_eq!(name, "pattern");
    assert!(matcher.is_match(0, "12: entry"));

    let (name, matcher) =
        resolve_matcher(Path::new("notes.weird"), "", None, Some("// PART"), None).unwrap();
    assert_eq!(name, "prefix");
    assert!(matcher.is_match(0, "  // PART 1: strides"));
    assert!(!matcher.is_match(0, "intro // PART"));

    let (name, _) =
        resolve_matcher(Path::new("notes.weird"), "", Some("Rust"), None, None).unwrap();
    assert_eq!(name, "rust");

    let (name, _) = resolve_matcher(Path::new("src/MAIN.RS"), "", None, None, None).unwrap();
    assert_eq!(name, "rust");
}

#[test]
fn resolve_matcher_detects_shebang_for_extensionless_scripts() {
    let (name, matcher) = resolve_matcher(
        Path::new("scripts/launcher"),
        "#!/usr/bin/env bash",
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(name, "shell");
    assert!(matcher.is_match(0, "main() {"));

    let (name, _) = resolve_matcher(
        Path::new("scripts/tool"),
        "#!/usr/bin/python3.11",
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(name, "python");

    let (name, _) =
        resolve_matcher(Path::new("hook"), "#!/usr/bin/env node", None, None, None).unwrap();
    assert_eq!(name, "javascript");

    // A known extension wins over the shebang; an unknown interpreter fails.
    let (name, _) = resolve_matcher(
        Path::new("gen.rs"),
        "#!/usr/bin/env python3",
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(name, "rust");
    let error =
        resolve_matcher(Path::new("run"), "#!/usr/bin/env pwsh", None, None, None).unwrap_err();
    assert!(error.to_string().contains("shebang"), "{error}");
}

#[test]
fn resolve_matcher_fails_fast_on_unknown_surface() {
    let error = resolve_matcher(Path::new("notes.weird"), "", None, None, None).unwrap_err();
    assert!(error.to_string().contains("--lang"), "{error}");

    let error = resolve_matcher(Path::new("main.rs"), "", Some("cobol"), None, None).unwrap_err();
    assert!(error.to_string().contains("not supported"), "{error}");

    let error =
        resolve_matcher(Path::new("main.rs"), "", None, None, Some("[unclosed")).unwrap_err();
    assert!(error.to_string().contains("invalid outline"), "{error}");

    let error = resolve_matcher(Path::new("main.rs"), "", None, Some(""), None).unwrap_err();
    assert!(error.to_string().contains("non-empty"), "{error}");
}
