use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use regex::Regex;

#[derive(Clone)]
pub(crate) enum TextMatcher {
    Literal {
        pattern: String,
        ignore_case: bool,
    },
    Regex {
        regex: Regex,
        ignore_case: bool,
    },
    Terms {
        terms: Vec<String>,
        mode: TermMode,
        ignore_case: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum TermMode {
    All,
    Any,
}

impl TextMatcher {
    pub(crate) fn new(pattern: &str, literal: bool, ignore_case: bool) -> Result<Self> {
        if literal {
            Ok(Self::Literal {
                pattern: if ignore_case {
                    pattern.to_lowercase()
                } else {
                    pattern.to_owned()
                },
                ignore_case,
            })
        } else {
            Ok(Self::Regex {
                regex: regex::RegexBuilder::new(pattern)
                    .case_insensitive(ignore_case)
                    .build()
                    .with_context(|| format!("invalid regex pattern: {pattern}"))?,
                ignore_case,
            })
        }
    }

    pub(crate) fn terms(terms: Vec<String>, mode: TermMode, ignore_case: bool) -> Self {
        let terms = if ignore_case {
            terms.into_iter().map(|term| term.to_lowercase()).collect()
        } else {
            terms
        };
        Self::Terms {
            terms,
            mode,
            ignore_case,
        }
    }

    pub(crate) fn is_match(&self, text: &str) -> bool {
        match self {
            Self::Literal {
                pattern,
                ignore_case,
            } => {
                if *ignore_case {
                    contains_ignore_case(text, pattern)
                } else {
                    text.contains(pattern)
                }
            }
            Self::Regex { regex, .. } => regex.is_match(text),
            Self::Terms {
                terms,
                mode,
                ignore_case,
            } => {
                if *ignore_case {
                    match mode {
                        TermMode::All => terms.iter().all(|term| contains_ignore_case(text, term)),
                        TermMode::Any => terms.iter().any(|term| contains_ignore_case(text, term)),
                    }
                } else {
                    match mode {
                        TermMode::All => terms.iter().all(|term| text.contains(term.as_str())),
                        TermMode::Any => terms.iter().any(|term| text.contains(term.as_str())),
                    }
                }
            }
        }
    }

    pub(crate) fn label(&self) -> String {
        let suffix = if self.is_ignore_case() {
            " ignore_case"
        } else {
            ""
        };
        match self {
            Self::Literal { pattern, .. } => format!("{pattern:?}{suffix}"),
            Self::Regex { regex, .. } => format!("{:?}{suffix}", regex.as_str()),
            Self::Terms { terms, mode, .. } => match mode {
                TermMode::All => format!("all_terms({}){suffix}", terms.join(",")),
                TermMode::Any => format!("any_terms({}){suffix}", terms.join(",")),
            },
        }
    }

    fn is_ignore_case(&self) -> bool {
        match self {
            Self::Literal { ignore_case, .. }
            | Self::Regex { ignore_case, .. }
            | Self::Terms { ignore_case, .. } => *ignore_case,
        }
    }
}

/// Case-insensitive substring test against a needle that is already
/// lowercased. ASCII needles fold haystack bytes in place without allocating
/// (a UTF-8 continuation byte is >= 0x80 and never folds to ASCII, so byte
/// comparison cannot false-match mid-codepoint); non-ASCII needles fall back
/// to a Unicode lowercase of the haystack.
pub(crate) fn contains_ignore_case(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if !needle_lower.is_ascii() {
        return haystack.to_lowercase().contains(needle_lower);
    }
    let haystack = haystack.as_bytes();
    let needle = needle_lower.as_bytes();
    if needle.len() > haystack.len() {
        return false;
    }
    let first = needle[0];
    'candidates: for start in 0..=haystack.len() - needle.len() {
        if haystack[start].to_ascii_lowercase() != first {
            continue;
        }
        for (offset, expected) in needle.iter().enumerate().skip(1) {
            if haystack[start + offset].to_ascii_lowercase() != *expected {
                continue 'candidates;
            }
        }
        return true;
    }
    false
}

pub(crate) fn collect_terms(terms: &[String], term_files: &[PathBuf]) -> Result<Vec<String>> {
    let mut collected = terms.to_vec();
    for file in term_files {
        let text = fs::read_to_string(file)
            .with_context(|| format!("failed to read term file {}", file.display()))?;
        let text = strip_utf8_bom(&text);
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if !line.is_empty() {
                collected.push(line.to_owned());
            }
        }
    }
    if collected.is_empty() {
        return Err(anyhow!(
            "grep-terms requires at least one --term or --term-file entry"
        ));
    }
    Ok(collected)
}

pub(crate) fn resolve_term_mode(mode: TermMode, any: bool, all: bool) -> Result<TermMode> {
    match (any, all) {
        (true, true) => Err(anyhow!(
            "grep-terms accepts only one of --any/--or or --all/--and"
        )),
        (true, false) => Ok(TermMode::Any),
        (false, true) => Ok(TermMode::All),
        (false, false) => Ok(mode),
    }
}

pub(crate) fn collect_single_text_source(
    label: &str,
    inline: Option<&str>,
    file: Option<&Path>,
    trim_terminal_newlines: bool,
) -> Result<String> {
    match (inline, file) {
        (Some(_), Some(_)) => Err(anyhow!(
            "{label} accepts either an inline value or a file, not both"
        )),
        (Some(value), None) => Ok(value.to_owned()),
        (None, Some(path)) => {
            let mut text = if path == Path::new("-") {
                let mut text = String::new();
                io::stdin()
                    .read_to_string(&mut text)
                    .with_context(|| format!("failed to read {label} from stdin"))?;
                text
            } else {
                fs::read_to_string(path)
                    .with_context(|| format!("failed to read {label} file {}", path.display()))?
            };
            if trim_terminal_newlines {
                trim_trailing_line_endings(&mut text);
            }
            Ok(strip_utf8_bom(&text).to_owned())
        }
        (None, None) => Err(anyhow!("{label} requires an inline value or file")),
    }
}

fn strip_utf8_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

fn trim_trailing_line_endings(value: &mut String) {
    while value.ends_with('\n') || value.ends_with('\r') {
        value.pop();
    }
}

pub(crate) fn parse_line_range(range: &str) -> Result<(usize, Option<usize>)> {
    let (start, end) = range
        .split_once(':')
        .ok_or_else(|| anyhow!("slice --range must use START:END line numbers"))?;
    if start.is_empty() || end.is_empty() {
        return Err(anyhow!("slice --range requires both START and END"));
    }
    let start = start
        .parse::<usize>()
        .with_context(|| format!("invalid slice --range start: {start}"))?;
    let end = end
        .parse::<usize>()
        .with_context(|| format!("invalid slice --range end: {end}"))?;
    if start == 0 || end == 0 {
        return Err(anyhow!("slice --range is 1-based; line 0 is invalid"));
    }
    if end < start {
        return Err(anyhow!(
            "slice --range end must be greater than or equal to start"
        ));
    }
    Ok((start, Some(end)))
}

#[cfg(test)]
mod tests;
