use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum BriefView {
    Brief,
    Full,
}

impl BriefView {
    pub(crate) fn is_full(self) -> bool {
        self == Self::Full
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Full => "full",
        }
    }
}

impl std::fmt::Display for BriefView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BriefCommand {
    pub(crate) argv: Vec<String>,
    pub(crate) display: String,
}

pub(crate) fn brief_command(args: &[&str]) -> BriefCommand {
    let argv = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    BriefCommand {
        display: display_command(&argv),
        argv,
    }
}

pub(crate) fn brief_command_owned(args: Vec<String>) -> BriefCommand {
    BriefCommand {
        display: display_command(&args),
        argv: args,
    }
}

fn display_command(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| display_command_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_command_arg(arg: &str) -> String {
    if arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '\\' | ':'))
    {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
}

pub(crate) fn capped_strings(values: &[String], limit: usize) -> Vec<String> {
    values.iter().take(limit).cloned().collect()
}

pub(crate) fn text_preview(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let mut preview = normalized.chars().take(max_chars).collect::<String>();
    preview.push_str("...");
    preview
}
