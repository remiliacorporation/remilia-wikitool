use std::cmp::min;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::config::ContextConfig;
use crate::files::display_path;
use crate::output::{base_receipt, clamp_text, emit_json_checked, write_receipt_checked};

/// An outline maps declaration-shaped lines, not parsed syntax: each built-in
/// language rule is a hand-written token classifier over one line (no regex
/// engine), so false positives (string literals, unusual formatting) are
/// possible and acceptable for orientation. Leading indentation is preserved
/// in output so nesting stays visible.
type Classifier = fn(&str) -> bool;

/// Languages whose structure needs cross-line context (XML element nesting,
/// C banner fences) classify the whole document at once and return the
/// 0-based indices of declaration lines.
type DocumentClassifier = fn(&str) -> Vec<usize>;

#[derive(Clone, Copy)]
enum LanguageRule {
    Line(Classifier),
    Document(DocumentClassifier),
}

struct OutlineLanguage {
    name: &'static str,
    extensions: &'static [&'static str],
    classify: LanguageRule,
}

const OUTLINE_LANGUAGES: &[OutlineLanguage] = &[
    OutlineLanguage {
        name: "rust",
        extensions: &["rs"],
        classify: LanguageRule::Line(rust_declaration),
    },
    OutlineLanguage {
        name: "python",
        extensions: &["py", "pyi"],
        classify: LanguageRule::Line(python_declaration),
    },
    OutlineLanguage {
        name: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        classify: LanguageRule::Line(js_ts_declaration),
    },
    OutlineLanguage {
        name: "typescript",
        extensions: &["ts", "tsx", "mts", "cts"],
        classify: LanguageRule::Line(js_ts_declaration),
    },
    OutlineLanguage {
        name: "go",
        extensions: &["go"],
        classify: LanguageRule::Line(go_declaration),
    },
    OutlineLanguage {
        name: "c",
        extensions: &["c", "h"],
        classify: LanguageRule::Document(c_document_outline),
    },
    OutlineLanguage {
        name: "cpp",
        extensions: &["cpp", "hpp", "cc", "hh", "cxx", "hxx", "inl"],
        classify: LanguageRule::Document(c_document_outline),
    },
    OutlineLanguage {
        name: "java",
        extensions: &["java"],
        classify: LanguageRule::Line(java_declaration),
    },
    OutlineLanguage {
        name: "csharp",
        extensions: &["cs"],
        classify: LanguageRule::Line(csharp_declaration),
    },
    OutlineLanguage {
        name: "kotlin",
        extensions: &["kt", "kts"],
        classify: LanguageRule::Line(kotlin_declaration),
    },
    OutlineLanguage {
        name: "shell",
        extensions: &["sh", "bash", "zsh"],
        classify: LanguageRule::Line(shell_declaration),
    },
    OutlineLanguage {
        name: "lua",
        extensions: &["lua"],
        classify: LanguageRule::Line(lua_declaration),
    },
    OutlineLanguage {
        name: "ruby",
        extensions: &["rb"],
        classify: LanguageRule::Line(ruby_declaration),
    },
    OutlineLanguage {
        name: "markdown",
        extensions: &["md", "markdown"],
        classify: LanguageRule::Line(markdown_declaration),
    },
    OutlineLanguage {
        name: "toml",
        extensions: &["toml"],
        classify: LanguageRule::Line(toml_declaration),
    },
    OutlineLanguage {
        name: "ini",
        extensions: &["ini", "cfg", "conf"],
        classify: LanguageRule::Line(toml_declaration),
    },
    OutlineLanguage {
        name: "yaml",
        extensions: &["yml", "yaml"],
        classify: LanguageRule::Line(yaml_declaration),
    },
    OutlineLanguage {
        name: "sql",
        extensions: &["sql"],
        classify: LanguageRule::Line(sql_declaration),
    },
    OutlineLanguage {
        name: "wikitext",
        extensions: &["wiki", "wikitext", "mediawiki"],
        classify: LanguageRule::Line(wikitext_declaration),
    },
    OutlineLanguage {
        name: "json",
        extensions: &["json", "jsonc"],
        classify: LanguageRule::Line(json_declaration),
    },
    OutlineLanguage {
        name: "xml",
        extensions: &["xml", "xsd", "xaml"],
        classify: LanguageRule::Document(xml_document_outline),
    },
];

fn ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// If `rest` begins with `keyword` at a word boundary, return the remainder.
fn strip_keyword<'a>(rest: &'a str, keyword: &str) -> Option<&'a str> {
    let after = rest.strip_prefix(keyword)?;
    match after.chars().next() {
        None => Some(after),
        Some(ch) if !ident_char(ch) => Some(after),
        Some(_) => None,
    }
}

fn starts_keyword(rest: &str, keyword: &str) -> bool {
    strip_keyword(rest, keyword).is_some()
}

fn starts_any_keyword(rest: &str, keywords: &[&str]) -> bool {
    // First-byte prefilter: on non-matching lines (the overwhelming case)
    // this skips almost every candidate without a prefix comparison.
    let Some(&first) = rest.as_bytes().first() else {
        return false;
    };
    keywords
        .iter()
        .any(|keyword| keyword.as_bytes()[0] == first && starts_keyword(rest, keyword))
}

/// `keyword` followed by mandatory whitespace; returns the trimmed remainder.
fn strip_keyword_ws<'a>(rest: &'a str, keyword: &str) -> Option<&'a str> {
    let after = strip_keyword(rest, keyword)?;
    if after.starts_with(char::is_whitespace) {
        Some(after.trim_start())
    } else {
        None
    }
}

fn strip_any_keyword_ws<'a>(rest: &'a str, keywords: &[&str]) -> Option<&'a str> {
    let first = *rest.as_bytes().first()?;
    keywords
        .iter()
        .filter(|keyword| keyword.as_bytes()[0] == first)
        .find_map(|keyword| strip_keyword_ws(rest, keyword))
}

/// Byte length of the identifier at the start of `rest`, using the given
/// first-char and continuation predicates.
fn ident_span(rest: &str, first: fn(char) -> bool, continuation: fn(char) -> bool) -> usize {
    let mut end = 0;
    for (idx, ch) in rest.char_indices() {
        let allowed = if idx == 0 {
            first(ch)
        } else {
            continuation(ch)
        };
        if !allowed {
            break;
        }
        end = idx + ch.len_utf8();
    }
    end
}

const RUST_DECLARATION_KEYWORDS: &[&str] = &[
    "fn", "struct", "enum", "union", "trait", "impl", "mod", "type", "const", "static",
];
const RUST_MODIFIERS: &[&str] = &["default", "const", "async", "unsafe"];

fn rust_declaration(line: &str) -> bool {
    let mut rest = line.trim_start();
    if let Some(after) = strip_keyword(rest, "pub") {
        let after = after.trim_start();
        rest = if let Some(group) = after.strip_prefix('(') {
            match group.find(')') {
                Some(close) => group[close + 1..].trim_start(),
                None => return false,
            }
        } else {
            after
        };
    }
    loop {
        if starts_any_keyword(rest, RUST_DECLARATION_KEYWORDS) {
            return true;
        }
        if rest.starts_with("macro_rules!") {
            return true;
        }
        if let Some(after) = strip_keyword(rest, "extern") {
            let after = after.trim_start();
            let Some(quoted) = after.strip_prefix('"') else {
                return false;
            };
            let Some(close) = quoted.find('"') else {
                return false;
            };
            rest = quoted[close + 1..].trim_start();
            continue;
        }
        match strip_any_keyword_ws(rest, RUST_MODIFIERS) {
            Some(after) => rest = after,
            None => return false,
        }
    }
}

fn python_declaration(line: &str) -> bool {
    let mut rest = line.trim_start();
    if let Some(after) = strip_keyword_ws(rest, "async") {
        rest = after;
    }
    starts_keyword(rest, "def") || starts_keyword(rest, "class")
}

fn js_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_' || ch == '$'
}

fn js_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

const JS_DECLARATION_KEYWORDS: &[&str] = &[
    "function",
    "class",
    "interface",
    "enum",
    "namespace",
    "module",
];

fn js_ts_declaration(line: &str) -> bool {
    let mut rest = line.trim_start();
    if let Some(after) = strip_keyword_ws(rest, "export") {
        rest = after;
        if let Some(after) = strip_keyword_ws(rest, "default") {
            rest = after;
        }
    }
    if let Some(after) = strip_keyword_ws(rest, "declare") {
        rest = after;
    }
    if let Some(after) = strip_keyword_ws(rest, "abstract") {
        rest = after;
    }
    let after_async = strip_keyword_ws(rest, "async").unwrap_or(rest);
    if starts_any_keyword(after_async, JS_DECLARATION_KEYWORDS) {
        return true;
    }
    if let Some(after) = strip_keyword_ws(rest, "type")
        && after.starts_with(js_ident_start)
    {
        return true;
    }
    for keyword in ["const", "let", "var"] {
        if let Some(after) = strip_keyword_ws(rest, keyword) {
            return js_function_binding(after);
        }
    }
    js_method_shape(rest, line)
}

/// Statement heads that would otherwise satisfy the method shape
/// (`if (ready) {`, `while (frames.pop()) {`).
const JS_STATEMENT_KEYWORDS: &[&str] = &[
    "if", "for", "while", "switch", "catch", "return", "typeof", "await", "new", "else", "do",
    "case", "throw", "delete", "void", "yield", "in", "of", "try", "finally",
];

/// Class/object method heads: optional `static`/`async`/`get`/`set`/`*`
/// modifiers, a (possibly `#`-private) name, optional TS generics, `(`, and
/// a line whose parentheses balance before a trailing `{`. The brace
/// separates definitions from call statements (which end with `;`/`)`) and
/// the balance requirement drops object-argument calls (`fetch(url, {`) and
/// callback registrations (`it('x', () => {`), whose `{` opens inside the
/// parameter list.
fn js_method_shape(rest: &str, line: &str) -> bool {
    let mut rest = rest;
    for keyword in ["static", "async", "get", "set"] {
        if let Some(after) = strip_keyword_ws(rest, keyword) {
            rest = after;
        }
    }
    if let Some(after) = rest.strip_prefix('*') {
        rest = after.trim_start();
    }
    if starts_any_keyword(rest, JS_STATEMENT_KEYWORDS) {
        return false;
    }
    let rest = rest.strip_prefix('#').unwrap_or(rest);
    let name = ident_span(rest, js_ident_start, js_ident_char);
    if name == 0 {
        return false;
    }
    let mut after = rest[name..].trim_start();
    if after.starts_with('<') {
        match after.find('>') {
            Some(close) => after = after[close + 1..].trim_start(),
            None => return false,
        }
    }
    if !after.starts_with('(') {
        return false;
    }
    let trailer = line.trim_end().trim_end_matches('}').trim_end();
    if !trailer.ends_with('{') {
        return false;
    }
    let opens = line.matches('(').count();
    let closes = line.matches(')').count();
    opens == closes
}

/// `name [: annotation] = [async] (` / `= function` / `= param =>`.
fn js_function_binding(rest: &str) -> bool {
    let name = ident_span(rest, js_ident_start, js_ident_char);
    if name == 0 {
        return false;
    }
    let mut rest = rest[name..].trim_start();
    if let Some(annotation) = rest.strip_prefix(':') {
        match annotation_assignment(annotation) {
            Some(eq) => rest = &annotation[eq..],
            None => return false,
        }
    }
    let Some(after_eq) = rest.strip_prefix('=') else {
        return false;
    };
    if after_eq.starts_with('=') || after_eq.starts_with('>') {
        return false;
    }
    let value = after_eq.trim_start();
    let value = strip_keyword_ws(value, "async").unwrap_or(value);
    if value.starts_with('(') || starts_keyword(value, "function") {
        return true;
    }
    let param = ident_span(value, js_ident_start, js_ident_char);
    param > 0 && value[param..].trim_start().starts_with("=>")
}

/// Byte offset of the assignment `=` inside a type annotation. The annotation
/// itself may contain `=` bytes that are not the binding's assignment —
/// arrows (`() => void`, also nested as in `Array<() => void>`), equality
/// runs (`==`/`===`), and comparisons (`<=`/`>=`/`!=`) — so those are skipped.
fn annotation_assignment(annotation: &str) -> Option<usize> {
    let bytes = annotation.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'=' {
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            // Arrow: `=` immediately followed by `>` is a function type.
            Some(b'>') => index += 2,
            // Equality run: swallow `==` and `===` whole.
            Some(b'=') => {
                index += 1;
                while bytes.get(index) == Some(&b'=') {
                    index += 1;
                }
            }
            // Comparison tail: the `=` of `<=`, `>=`, or `!=`.
            _ if index > 0 && matches!(bytes[index - 1], b'<' | b'>' | b'!') => index += 1,
            _ => return Some(index),
        }
    }
    None
}

fn go_declaration(line: &str) -> bool {
    if line.starts_with(char::is_whitespace) {
        return false;
    }
    starts_any_keyword(line, &["func", "type", "package"])
        || strip_keyword_ws(line, "var").is_some()
        || strip_keyword_ws(line, "const").is_some()
}

const C_KEYWORDS: &[&str] = &[
    "typedef",
    "class",
    "struct",
    "enum",
    "union",
    "namespace",
    "extern",
    "using",
];
const C_HEAD_PUNCTUATION: &[char] = &[':', '*', '&', '<', '>', ',', '~', '[', ']'];
/// A line led by a statement keyword is control flow, not a definition,
/// even when it opens a parenthesized expression.
const C_STATEMENT_KEYWORDS: &[&str] = &[
    "return", "if", "while", "for", "switch", "goto", "else", "do", "case", "break", "continue",
    "throw", "delete", "sizeof",
];

/// C-family document outline: every per-line declaration
/// ([`c_family_declaration`]) plus section-banner comment titles. Large
/// annotated headers often put their only navigable structure between
/// declarations in `// === Section ===` banner comments, which need
/// neighbor-line context to recognize.
fn c_document_outline(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut hits = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if c_family_declaration(line) || c_banner_title(&lines, index) {
            hits.push(index);
        }
    }
    hits
}

const C_BANNER_FENCE_CHARS: &[char] = &['=', '-', '*', '#'];

/// Section-banner comment titles: a one-liner (`// ==== Renderer ====`) or
/// the middle line of a fenced banner (`// ====` / `// Title` / `// ====`).
/// Bare fences without a title never emit.
fn c_banner_title(lines: &[&str], index: usize) -> bool {
    let Some(content) = c_comment_text(lines[index]) else {
        return false;
    };
    if content.is_empty() || c_banner_fence_text(content) {
        return false;
    }
    // One-liner: fence runs on both sides of a non-empty title.
    let leading = content
        .chars()
        .take_while(|ch| C_BANNER_FENCE_CHARS.contains(ch))
        .count();
    let trailing = content
        .chars()
        .rev()
        .take_while(|ch| C_BANNER_FENCE_CHARS.contains(ch))
        .count();
    let total = content.chars().count();
    if leading >= 4 && trailing >= 4 && leading + trailing < total {
        let title: String = content
            .chars()
            .skip(leading)
            .take(total - leading - trailing)
            .collect();
        if !title.trim().is_empty() {
            return true;
        }
    }
    // Fenced: both neighbor lines are pure fence comments.
    let neighbor_is_fence = |neighbor: Option<&&str>| {
        neighbor
            .and_then(|line| c_comment_text(line))
            .is_some_and(c_banner_fence_text)
    };
    neighbor_is_fence(index.checked_sub(1).and_then(|prev| lines.get(prev)))
        && neighbor_is_fence(lines.get(index + 1))
}

/// Comment payload of a `//` line comment or a one-line `/* ... */` block,
/// trimmed; None for non-comment lines.
fn c_comment_text(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    if let Some(text) = rest.strip_prefix("//") {
        return Some(text.trim_matches(|ch: char| ch.is_whitespace() || ch == '/'));
    }
    if let Some(text) = rest.strip_prefix("/*") {
        let text = text.strip_suffix("*/").unwrap_or(text);
        return Some(text.trim());
    }
    None
}

/// A pure fence: at least 8 fence characters and nothing else.
fn c_banner_fence_text(content: &str) -> bool {
    content.chars().count() >= 8 && content.chars().all(|ch| C_BANNER_FENCE_CHARS.contains(&ch))
}

/// Heuristic for C-family structure at any indentation: type/aggregate
/// keywords, macro definitions (column 0 only), access labels, `operator`
/// overloads, and identifier-led lines that open a parameter list with at
/// least two head tokens (`void Render(` / `int prototype(int);`).
/// Definitions and prototypes both count — headers carry their structure as
/// prototypes. Calls have single-token heads and assignments put `=` before
/// the parameter list, so statements fall out of the shape check.
fn c_family_declaration(line: &str) -> bool {
    let rest = line.trim_start();
    let Some(first) = rest.chars().next() else {
        return false;
    };
    if let Some(after) = rest.strip_prefix('#') {
        return !line.starts_with(char::is_whitespace)
            && starts_keyword(after.trim_start(), "define");
    }
    if matches!(rest.trim_end(), "public:" | "private:" | "protected:") {
        return true;
    }
    if let Some(after) = strip_keyword(rest, "template")
        && after.trim_start().starts_with('<')
    {
        return true;
    }
    if starts_any_keyword(rest, C_KEYWORDS) {
        return true;
    }
    if starts_any_keyword(rest, C_STATEMENT_KEYWORDS) {
        return false;
    }
    if !ident_start(first) {
        return false;
    }
    let Some(open_paren) = rest.find('(') else {
        return false;
    };
    let head = rest[..open_paren].trim_end();
    // Operator overloads name themselves with punctuation the head scan
    // would otherwise reject (`bool operator==(`, `T& operator[](`).
    if head.split_whitespace().last().is_some_and(|token| {
        token
            .strip_prefix("operator")
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|ch| !ident_char(ch)))
    }) {
        return true;
    }
    if !head
        .chars()
        .all(|ch| ident_char(ch) || ch.is_whitespace() || C_HEAD_PUNCTUATION.contains(&ch))
    {
        return false;
    }
    let last_significant = head.chars().next_back().unwrap_or(first);
    if !(ident_char(last_significant) || last_significant == ':' || last_significant == '~') {
        return false;
    }
    if head.split_whitespace().count() >= 2 {
        return true;
    }
    // Qualified single-token heads are out-of-line ctor/dtor definitions
    // (`CGxDevice::~CGxDevice()`); with a trailing `;` after the parameter
    // list they are call statements instead.
    head.contains("::") && !rest[open_paren + 1..].contains(';')
}

const JAVA_MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "static",
    "final",
    "abstract",
    "synchronized",
    "native",
    "default",
    "sealed",
    "strictfp",
];

/// Statement heads that would otherwise satisfy the `Type name(` shape
/// (`return frame(count);`, `throw new FooException(...)`).
const JAVA_STATEMENT_KEYWORDS: &[&str] = &[
    "return", "throw", "new", "if", "while", "for", "switch", "else", "do", "case", "break",
    "continue", "assert", "yield", "try", "catch", "finally", "super", "this",
];

fn java_declaration(line: &str) -> bool {
    let mut rest = line.trim_start();
    let mut modifiers = 0usize;
    while let Some(after) = strip_any_keyword_ws(rest, JAVA_MODIFIERS) {
        rest = after;
        modifiers += 1;
    }
    if starts_any_keyword(rest, &["class", "interface", "enum", "record"])
        || rest.starts_with("@interface")
    {
        return true;
    }
    if starts_any_keyword(rest, JAVA_STATEMENT_KEYWORDS) {
        return false;
    }
    if modifiers > 0 {
        // A modifier-led single-token head is a constructor (`public Renderer(`).
        return signature_shape(rest, &['('], 1);
    }
    // Package-private members need the full `Type name(` shape; calls have
    // single-token heads and assignments fail the token character check.
    signature_shape(rest, &['('], 2)
}

const CSHARP_MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "sealed",
    "abstract",
    "partial",
    "readonly",
    "virtual",
    "override",
    "async",
    "unsafe",
    "extern",
    "new",
];

fn csharp_declaration(line: &str) -> bool {
    let mut rest = line.trim_start();
    let mut modifiers = 0usize;
    while let Some(after) = strip_any_keyword_ws(rest, CSHARP_MODIFIERS) {
        rest = after;
        modifiers += 1;
    }
    if starts_any_keyword(
        rest,
        &[
            "class",
            "interface",
            "enum",
            "record",
            "struct",
            "namespace",
            "delegate",
            "event",
        ],
    ) {
        return true;
    }
    // A modifier-led single-token head is a constructor (`internal Renderer(`).
    modifiers > 0 && signature_shape(rest, &['(', '{'], 1)
}

/// `Type name(` shape: at least `min_tokens` whitespace-separated tokens of
/// identifier/generic/array characters before the first terminator.
fn signature_shape(rest: &str, terminators: &[char], min_tokens: usize) -> bool {
    let Some(cut) = rest.find(|ch| terminators.contains(&ch)) else {
        return false;
    };
    let head = rest[..cut].trim_end();
    let mut tokens = 0usize;
    for token in head.split_whitespace() {
        tokens += 1;
        if !token
            .chars()
            .all(|ch| ident_char(ch) || matches!(ch, '<' | '>' | '[' | ']' | ',' | '.' | '?'))
        {
            return false;
        }
    }
    tokens >= min_tokens
}

const KOTLIN_MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "internal",
    "open",
    "final",
    "abstract",
    "sealed",
    "data",
    "inline",
    "suspend",
    "operator",
    "override",
    "external",
    "expect",
    "actual",
    "annotation",
];

fn kotlin_declaration(line: &str) -> bool {
    let mut rest = line.trim_start();
    while let Some(after) = strip_any_keyword_ws(rest, KOTLIN_MODIFIERS) {
        rest = after;
    }
    if let Some(after) = strip_keyword_ws(rest, "enum") {
        return starts_keyword(after, "class");
    }
    if let Some(after) = strip_keyword_ws(rest, "companion") {
        return starts_keyword(after, "object");
    }
    starts_any_keyword(rest, &["class", "interface", "object", "fun", "typealias"])
}

fn shell_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn shell_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

fn shell_declaration(line: &str) -> bool {
    let rest = line.trim_start();
    if let Some(after) = strip_keyword_ws(rest, "function") {
        return after.starts_with(shell_ident_start);
    }
    let name = ident_span(rest, shell_ident_start, shell_ident_char);
    if name == 0 {
        return false;
    }
    let after = rest[name..].trim_start();
    let Some(after) = after.strip_prefix("()") else {
        return false;
    };
    let after = after.trim();
    after.is_empty() || after == "{"
}

fn lua_ident_char(ch: char) -> bool {
    ident_char(ch) || ch == '.' || ch == ':'
}

fn lua_declaration(line: &str) -> bool {
    let unindented = !line.starts_with(char::is_whitespace);
    let mut rest = line.trim_start();
    if let Some(after) = strip_keyword_ws(rest, "local") {
        rest = after;
    }
    if starts_keyword(rest, "function") {
        return true;
    }
    let name = ident_span(rest, ident_start, lua_ident_char);
    if name == 0 {
        return false;
    }
    let after = rest[name..].trim_start();
    let Some(after) = after.strip_prefix('=') else {
        return false;
    };
    let value = after.trim_start();
    if starts_keyword(value, "function") {
        return true;
    }
    // Column-0 table roots are module/addon structure (`MyAddon = {}`,
    // `local p = {}` in Scribunto modules, `T = T or {}`, multi-line
    // `Defaults = {`); indented table assignments are locals inside
    // functions and one-liner closed tables (`t = {1, 2}`) are data.
    let value = value.trim_end();
    unindented && (value.ends_with('{') || value.ends_with("{}"))
}

fn ruby_declaration(line: &str) -> bool {
    starts_any_keyword(line.trim_start(), &["def", "class", "module"])
}

fn markdown_declaration(line: &str) -> bool {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace)
}

fn toml_declaration(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

/// Top-level mapping keys only: a col-0 line whose first `:` ends the key.
/// Indented keys, comments, and list items are structure below the outline.
fn yaml_declaration(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if first.is_whitespace() || first == '#' || first == '-' {
        return false;
    }
    match line.find(':') {
        Some(idx) => line[idx + 1..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace),
        None => false,
    }
}

/// DDL statement heads; DML and query lines are content, not structure.
fn sql_declaration(line: &str) -> bool {
    let rest = line.trim_start();
    ["create", "alter", "drop"].iter().any(|keyword| {
        rest.get(..keyword.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(keyword))
            && rest[keyword.len()..]
                .chars()
                .next()
                .is_none_or(|ch| !ident_char(ch))
    })
}

/// Container-opening keys (`"key": {` / `"key": [`) map document structure
/// without enumerating every scalar; a container that closes on its own line
/// is leaf content and stays out of the outline.
fn json_declaration(line: &str) -> bool {
    let rest = line.trim_start();
    let Some(after_quote) = rest.strip_prefix('"') else {
        return false;
    };
    let mut escaped = false;
    let mut key_end = None;
    for (idx, ch) in after_quote.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                key_end = Some(idx);
                break;
            }
            _ => {}
        }
    }
    let Some(key_end) = key_end else {
        return false;
    };
    let after_key = after_quote[key_end + 1..].trim_start();
    let Some(value) = after_key.strip_prefix(':') else {
        return false;
    };
    matches!(value.trim(), "{" | "[")
}

/// Element-nesting budget for "shallow" unnamed XML containers: the root and
/// its first two levels map document sections (`<page>`/`<revision>` in
/// MediaWiki exports, `<PropertyGroup>` in MSBuild) regardless of indent
/// style or minification.
const XML_SHALLOW_ELEMENT_DEPTH: usize = 2;

/// One open element on the parser stack.
struct XmlOpenElement {
    open_line: usize,
    named: bool,
    named_ancestor: bool,
    depth: usize,
    has_child_element: bool,
}

/// Depth-tracking XML outline: a real element-stack scan of the whole
/// document, not per-line shape checks. Emits the opening line of every
/// *container* element (one that holds child elements or closes on a later
/// line) that either carries a boundary-checked `name`/`id` attribute
/// (UI containers, build targets, Android views) or is an unnamed section
/// within [`XML_SHALLOW_ELEMENT_DEPTH`] with no named ancestor. Leaves —
/// self-closing or same-line-closed elements like `<Field Name="ID"/>` rows
/// in schema definition exports — plus comments, processing instructions,
/// CDATA, and DOCTYPE never emit.
fn xml_document_outline(text: &str) -> Vec<usize> {
    let mut line_starts = vec![0usize];
    for (pos, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(pos + 1);
        }
    }
    let line_of = |offset: usize| match line_starts.binary_search(&offset) {
        Ok(line) => line,
        Err(line) => line - 1,
    };
    let last_line = line_starts.len() - 1;

    let mut hits = std::collections::BTreeSet::new();
    let mut stack: Vec<XmlOpenElement> = Vec::new();
    let mut emit = |element: &XmlOpenElement, close_line: usize| {
        let container = element.has_child_element || close_line > element.open_line;
        let wanted = element.named
            || (element.depth <= XML_SHALLOW_ELEMENT_DEPTH && !element.named_ancestor);
        if container && wanted {
            hits.insert(element.open_line);
        }
    };

    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find('<') {
        let start = cursor + rel;
        let rest = &text[start..];
        if let Some(skip) = xml_skip_non_element(rest) {
            cursor = start + skip;
            continue;
        }
        let (tag_len, self_closing) = xml_scan_tag(rest);
        let tag_text = &rest[..tag_len];
        cursor = start + tag_len;
        if let Some(_close_name) = tag_text.strip_prefix("</") {
            if let Some(element) = stack.pop() {
                emit(&element, line_of(start));
            }
            continue;
        }
        if !tag_text[1..].starts_with(ident_start) {
            continue;
        }
        if let Some(parent) = stack.last_mut() {
            parent.has_child_element = true;
        }
        if self_closing {
            continue;
        }
        let named = xml_has_name_or_id_attribute(tag_text);
        let named_ancestor = stack
            .last()
            .is_some_and(|parent| parent.named || parent.named_ancestor);
        stack.push(XmlOpenElement {
            open_line: line_of(start),
            named,
            named_ancestor,
            depth: stack.len(),
            has_child_element: false,
        });
    }
    // Unclosed elements at EOF still outline: a truncated or fragmentary
    // document keeps its container structure visible.
    for element in stack.iter().rev() {
        emit(element, last_line);
    }
    hits.into_iter().collect()
}

/// If `rest` (starting at `<`) opens non-element markup, return how many
/// bytes to skip past it (to end of input when unterminated).
fn xml_skip_non_element(rest: &str) -> Option<usize> {
    for (open, close) in [
        ("<!--", "-->"),
        ("<![CDATA[", "]]>"),
        ("<?", "?>"),
        ("<!", ">"),
    ] {
        if let Some(after) = rest.strip_prefix(open) {
            return Some(
                after
                    .find(close)
                    .map(|pos| open.len() + pos + close.len())
                    .unwrap_or(rest.len()),
            );
        }
    }
    None
}

/// Scan a tag starting at `<`, honoring quoted attribute values (which may
/// contain `>` and span lines). Returns the tag's byte length including the
/// closing `>` (to end of input when unterminated) and whether it
/// self-closes.
fn xml_scan_tag(rest: &str) -> (usize, bool) {
    let mut quote: Option<char> = None;
    let mut previous_significant = ' ';
    for (idx, ch) in rest.char_indices() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => return (idx + 1, previous_significant == '/'),
                _ => {
                    if !ch.is_whitespace() {
                        previous_significant = ch;
                    }
                }
            },
        }
    }
    (rest.len(), false)
}

/// `name="..."`/`id="..."` (or single-quoted) at an attribute boundary —
/// preceded by whitespace or a namespace `:` — so `filename="..."` does not
/// count.
fn xml_has_name_or_id_attribute(rest: &str) -> bool {
    let lower = rest.to_ascii_lowercase();
    ["name=\"", "id=\"", "name='", "id='"].iter().any(|needle| {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(needle) {
            let at = start + pos;
            if lower[..at]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_whitespace() || ch == ':')
            {
                return true;
            }
            start = at + 1;
        }
        false
    })
}

/// MediaWiki section headings: `= Title =` through `====== Title ======`,
/// requiring the closing equals run and non-empty title text.
fn wikitext_declaration(line: &str) -> bool {
    let trimmed = line.trim_end();
    let opening = trimmed.chars().take_while(|ch| *ch == '=').count();
    if !(1..=6).contains(&opening) {
        return false;
    }
    let rest = &trimmed[opening..];
    let title = rest.trim_end_matches('=');
    rest.len() > title.len() && !title.trim().is_empty()
}

#[derive(Debug)]
enum OutlineMatcher {
    Builtin(Classifier),
    /// Pre-computed declaration line indices from a document classifier.
    Document(std::collections::BTreeSet<usize>),
    Prefix(String),
    Pattern(Regex),
}

impl OutlineMatcher {
    fn is_match(&self, index: usize, line: &str) -> bool {
        match self {
            Self::Builtin(classify) => classify(line),
            Self::Document(hits) => hits.contains(&index),
            Self::Prefix(prefix) => line.trim_start().starts_with(prefix.as_str()),
            Self::Pattern(regex) => regex.is_match(line),
        }
    }
}

#[cfg(test)]
fn builtin_rule(name: &str) -> Option<LanguageRule> {
    OUTLINE_LANGUAGES
        .iter()
        .find(|language| language.name == name)
        .map(|language| language.classify)
}

/// Language name for a `#!` interpreter line, so extensionless scripts
/// resolve without `--lang`. Version suffixes (`python3.11`) are ignored.
fn shebang_language(first_line: &str) -> Option<&'static str> {
    let rest = first_line.strip_prefix("#!")?;
    let mut tokens = rest.split_whitespace();
    let mut interpreter = tokens.next()?;
    if interpreter.rsplit('/').next() == Some("env") {
        interpreter = tokens.next()?;
    }
    let base = interpreter.rsplit('/').next().unwrap_or(interpreter);
    let stem: String = base
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect();
    match stem.as_str() {
        "bash" | "sh" | "zsh" | "dash" | "ksh" => Some("shell"),
        "python" => Some("python"),
        "lua" | "luajit" => Some("lua"),
        "ruby" => Some("ruby"),
        "node" | "nodejs" | "deno" | "bun" => Some("javascript"),
        _ => None,
    }
}

fn resolve_matcher(
    file: &Path,
    text: &str,
    lang: Option<&str>,
    prefix: Option<&str>,
    pattern: Option<&str>,
) -> Result<(String, OutlineMatcher)> {
    let first_line = text.lines().next();
    if let Some(pattern) = pattern {
        let regex = Regex::new(pattern)
            .with_context(|| format!("invalid outline --pattern regex: {pattern}"))?;
        return Ok(("pattern".to_owned(), OutlineMatcher::Pattern(regex)));
    }
    if let Some(prefix) = prefix {
        if prefix.is_empty() {
            return Err(anyhow!("outline --prefix requires non-empty text"));
        }
        return Ok((
            "prefix".to_owned(),
            OutlineMatcher::Prefix(prefix.to_owned()),
        ));
    }
    let language = if let Some(lang) = lang {
        let lang = lang.to_lowercase();
        OUTLINE_LANGUAGES
            .iter()
            .find(|candidate| candidate.name == lang)
            .ok_or_else(|| {
                anyhow!(
                    "outline --lang {lang} is not supported; known languages: {}",
                    known_language_names()
                )
            })?
    } else {
        let extension = file
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_lowercase)
            .unwrap_or_default();
        OUTLINE_LANGUAGES
            .iter()
            .find(|candidate| candidate.extensions.contains(&extension.as_str()))
            .or_else(|| {
                let name = shebang_language(first_line?)?;
                OUTLINE_LANGUAGES
                    .iter()
                    .find(|candidate| candidate.name == name)
            })
            .ok_or_else(|| {
                anyhow!(
                    "outline has no declaration heuristic for {} (no known extension or shebang); pass --lang <{}>, --prefix <text>, or --pattern <regex>",
                    file.display(),
                    known_language_names()
                )
            })?
    };
    let matcher = match language.classify {
        LanguageRule::Line(classify) => OutlineMatcher::Builtin(classify),
        LanguageRule::Document(classify) => {
            OutlineMatcher::Document(classify(text).into_iter().collect())
        }
    };
    Ok((language.name.to_owned(), matcher))
}

fn known_language_names() -> String {
    OUTLINE_LANGUAGES
        .iter()
        .map(|language| language.name)
        .collect::<Vec<_>>()
        .join("|")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn command_outline(
    cli: &Cli,
    config: &ContextConfig,
    file: &Path,
    lang: Option<&str>,
    prefix: Option<&str>,
    pattern: Option<&str>,
    contains: &[String],
    ignore_case: bool,
    max_items: usize,
    max_line_chars: usize,
) -> Result<()> {
    if max_items == 0 {
        return Err(anyhow!("outline --max-items must be greater than zero"));
    }
    // Read before resolving the language so a missing/unreadable file reports
    // as such instead of as a heuristic gap.
    let (text, encoding) = crate::encoding::read_required_text(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (language, matcher) = resolve_matcher(file, &text, lang, prefix, pattern)?;
    let lowered_contains = contains
        .iter()
        .map(|needle| {
            if ignore_case {
                needle.to_lowercase()
            } else {
                needle.clone()
            }
        })
        .collect::<Vec<_>>();
    let mut total_lines = 0usize;
    let mut declaration_lines_total = 0usize;
    let mut rows: Vec<(usize, &str)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        total_lines += 1;
        if !matcher.is_match(index, line) {
            continue;
        }
        declaration_lines_total += 1;
        if !lowered_contains.is_empty() {
            let matches_all = lowered_contains.iter().all(|needle| {
                if ignore_case {
                    crate::text::contains_ignore_case(line, needle)
                } else {
                    line.contains(needle.as_str())
                }
            });
            if !matches_all {
                continue;
            }
        }
        rows.push((index + 1, line.trim_end()));
    }
    let total = rows.len();
    let shown = min(total, max_items);
    let truncated = shown < total;
    let cap_reason = if truncated { Some("max_items") } else { None };
    let mut map = base_receipt(
        "outline",
        config.profile.as_deref(),
        "items",
        shown,
        total,
        truncated,
        cap_reason,
    );
    map.insert("path".to_string(), json!(display_path(file)));
    map.insert("language".to_string(), json!(language));
    map.insert("encoding".to_string(), json!(encoding));
    map.insert("total_lines".to_string(), json!(total_lines));
    map.insert(
        "declaration_lines_total".to_string(),
        json!(declaration_lines_total),
    );
    if !contains.is_empty() {
        map.insert("contains".to_string(), json!(contains));
    }
    // Whole-file scan (the read already happened); the field only exists
    // when something was found, so clean files cost nothing.
    let suspects = crate::encoding::scan_encoding_suspects(&text, false);
    if !suspects.is_empty() {
        map.insert("encoding_suspects".to_string(), suspects.receipt_value());
    }
    if cli.json {
        map.insert(
            "items".to_string(),
            json!(
                rows.iter()
                    .take(shown)
                    .map(|(line, text)| json!({
                        "line": line,
                        "text": clamp_text(text, max_line_chars),
                    }))
                    .collect::<Vec<_>>()
            ),
        );
        emit_json_checked(cli, Value::Object(map))
    } else {
        let mut stdout = io::stdout();
        writeln!(
            stdout,
            "[contextmink] outline path={} language={language} total_lines={total_lines}",
            display_path(file)
        )?;
        if rows.is_empty() {
            writeln!(stdout, "no_outline_rows")?;
        }
        for (line, text) in rows.iter().take(shown) {
            writeln!(stdout, "{line}: {}", clamp_text(text, max_line_chars))?;
        }
        if truncated {
            writeln!(
                stdout,
                "[contextmink] capped outline at {max_items} items; filter with --contains or raise --max-items."
            )?;
        }
        if !suspects.is_empty() {
            writeln!(stdout, "{}", suspects.human_note())?;
        }
        write_receipt_checked(cli, map)
    }
}

#[cfg(test)]
mod tests;
