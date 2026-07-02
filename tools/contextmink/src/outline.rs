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

struct OutlineLanguage {
    name: &'static str,
    extensions: &'static [&'static str],
    classify: Classifier,
}

const OUTLINE_LANGUAGES: &[OutlineLanguage] = &[
    OutlineLanguage {
        name: "rust",
        extensions: &["rs"],
        classify: rust_declaration,
    },
    OutlineLanguage {
        name: "python",
        extensions: &["py", "pyi"],
        classify: python_declaration,
    },
    OutlineLanguage {
        name: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        classify: js_ts_declaration,
    },
    OutlineLanguage {
        name: "typescript",
        extensions: &["ts", "tsx", "mts", "cts"],
        classify: js_ts_declaration,
    },
    OutlineLanguage {
        name: "go",
        extensions: &["go"],
        classify: go_declaration,
    },
    OutlineLanguage {
        name: "c",
        extensions: &["c", "h"],
        classify: c_family_declaration,
    },
    OutlineLanguage {
        name: "cpp",
        extensions: &["cpp", "hpp", "cc", "hh", "cxx", "hxx", "inl"],
        classify: c_family_declaration,
    },
    OutlineLanguage {
        name: "java",
        extensions: &["java"],
        classify: java_declaration,
    },
    OutlineLanguage {
        name: "csharp",
        extensions: &["cs"],
        classify: csharp_declaration,
    },
    OutlineLanguage {
        name: "kotlin",
        extensions: &["kt", "kts"],
        classify: kotlin_declaration,
    },
    OutlineLanguage {
        name: "shell",
        extensions: &["sh", "bash", "zsh"],
        classify: shell_declaration,
    },
    OutlineLanguage {
        name: "lua",
        extensions: &["lua"],
        classify: lua_declaration,
    },
    OutlineLanguage {
        name: "ruby",
        extensions: &["rb"],
        classify: ruby_declaration,
    },
    OutlineLanguage {
        name: "markdown",
        extensions: &["md", "markdown"],
        classify: markdown_declaration,
    },
    OutlineLanguage {
        name: "toml",
        extensions: &["toml"],
        classify: toml_declaration,
    },
    OutlineLanguage {
        name: "ini",
        extensions: &["ini", "cfg", "conf"],
        classify: toml_declaration,
    },
    OutlineLanguage {
        name: "yaml",
        extensions: &["yml", "yaml"],
        classify: yaml_declaration,
    },
    OutlineLanguage {
        name: "sql",
        extensions: &["sql"],
        classify: sql_declaration,
    },
    OutlineLanguage {
        name: "wikitext",
        extensions: &["wiki", "wikitext", "mediawiki"],
        classify: wikitext_declaration,
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
    false
}

/// `name [: annotation] = [async] (` / `= function` / `= param =>`.
fn js_function_binding(rest: &str) -> bool {
    let name = ident_span(rest, js_ident_start, js_ident_char);
    if name == 0 {
        return false;
    }
    let mut rest = rest[name..].trim_start();
    if let Some(annotation) = rest.strip_prefix(':') {
        match annotation.find('=') {
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
/// A column-0 line led by a statement keyword is control flow, not a
/// definition, even when it opens a parenthesized expression.
const C_STATEMENT_KEYWORDS: &[&str] = &[
    "return", "if", "while", "for", "switch", "goto", "else", "do", "case", "break", "continue",
    "throw", "delete", "sizeof",
];

/// Column-0 heuristic for C-family definitions: type/aggregate keywords,
/// object-like/function-like macro definitions, and identifier-led lines that
/// open a parameter list without a terminating semicolon (definitions, not
/// prototypes or statements).
fn c_family_declaration(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if first.is_whitespace() {
        return false;
    }
    if let Some(after) = line.strip_prefix('#') {
        return starts_keyword(after.trim_start(), "define");
    }
    if let Some(after) = strip_keyword(line, "template")
        && after.trim_start().starts_with('<')
    {
        return true;
    }
    if starts_any_keyword(line, C_KEYWORDS) {
        return true;
    }
    if starts_any_keyword(line, C_STATEMENT_KEYWORDS) {
        return false;
    }
    if !ident_start(first) {
        return false;
    }
    let mut last_significant = first;
    let mut open_paren = None;
    for (idx, ch) in line.char_indices().skip(first.len_utf8()) {
        if ch == '(' {
            open_paren = Some(idx);
            break;
        }
        if ident_char(ch) || ch.is_whitespace() || C_HEAD_PUNCTUATION.contains(&ch) {
            if !ch.is_whitespace() {
                last_significant = ch;
            }
        } else {
            return false;
        }
    }
    let Some(open_paren) = open_paren else {
        return false;
    };
    if !(ident_char(last_significant) || last_significant == ':' || last_significant == '~') {
        return false;
    }
    !line[open_paren + 1..].contains(';')
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
    modifiers > 0 && signature_shape(rest, &['('])
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
    modifiers > 0 && signature_shape(rest, &['(', '{'])
}

/// `Type name(` shape after modifiers: at least two whitespace-separated
/// tokens of identifier/generic/array characters before the first terminator.
fn signature_shape(rest: &str, terminators: &[char]) -> bool {
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
    tokens >= 2
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
    starts_keyword(after.trim_start(), "function")
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
    Prefix(String),
    Pattern(Regex),
}

impl OutlineMatcher {
    fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Builtin(classify) => classify(line),
            Self::Prefix(prefix) => line.trim_start().starts_with(prefix.as_str()),
            Self::Pattern(regex) => regex.is_match(line),
        }
    }
}

#[cfg(test)]
fn builtin_classifier(name: &str) -> Option<Classifier> {
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
    first_line: Option<&str>,
    lang: Option<&str>,
    prefix: Option<&str>,
    pattern: Option<&str>,
) -> Result<(String, OutlineMatcher)> {
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
    Ok((
        language.name.to_owned(),
        OutlineMatcher::Builtin(language.classify),
    ))
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
    let (language, matcher) = resolve_matcher(file, text.lines().next(), lang, prefix, pattern)?;
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
        if !matcher.is_match(line) {
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
        write_receipt_checked(cli, map)
    }
}

#[cfg(test)]
mod tests;
