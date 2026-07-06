//! Agent-harness PreToolUse hook adapter over the destructive-command scan.
//!
//! Reads one hook event JSON object from stdin (the shape Claude Code and
//! compatible harnesses emit: `{"tool_input": {"command": "..."}}`), extracts
//! the command string, and evaluates it with the same deny rules the bridge
//! and `capture`/`run` apply to child argv — one policy, three enforcement
//! points. Exit 0 allows the tool call; exit 2 blocks it with the deny
//! message on stderr (the blocking exit code hook protocols reserve).
//!
//! Payload tolerance is deliberate and asymmetric: a hook that fails closed
//! on payload-shape drift blocks every shell command for every agent in the
//! workspace (the 2026-07-05 outage, when a deleted hook script bricked all
//! shell tool calls). Malformed JSON or an absent command field therefore
//! allows with a stderr note; only a recognized destructive command blocks.

use std::io::Read;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::DestructiveGuardConfig;
use crate::destructive_guard::{DenyDecision, destructive_override_active, evaluate_argv};

/// Exit code that hook protocols treat as "block this tool call".
const EXIT_BLOCK: i32 = 2;

pub(crate) fn command_hook_guard(
    config: &DestructiveGuardConfig,
    command_field: &str,
) -> Result<()> {
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .context("hook-guard: reading hook payload from stdin")?;
    match evaluate_hook_payload(&raw, command_field, config, destructive_override_active()) {
        HookVerdict::Allow => Ok(()),
        HookVerdict::AllowUnparsed { note } => {
            eprintln!("[contextmink hook-guard] {note}; allowing (nothing to scan)");
            Ok(())
        }
        HookVerdict::AllowWithOverride { message } => {
            eprintln!(
                "[contextmink hook-guard] WARNING: destructive command allowed by \
                 {env}=1 break-glass override: {message}",
                env = crate::destructive_guard::ALLOW_DESTRUCTIVE_ENV
            );
            Ok(())
        }
        HookVerdict::Deny { message } => {
            eprintln!("BLOCKED by contextmink hook-guard: {message}");
            std::process::exit(EXIT_BLOCK);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HookVerdict {
    Allow,
    /// Payload could not be interpreted; allow, but say why on stderr.
    AllowUnparsed {
        note: String,
    },
    AllowWithOverride {
        message: String,
    },
    Deny {
        message: String,
    },
}

/// Pure evaluation: parse the payload, pull the command string at
/// `command_field` (a dot-separated object path), and scan it as a shell
/// payload so word-splitting matches what a shell would execute.
pub(crate) fn evaluate_hook_payload(
    raw: &str,
    command_field: &str,
    config: &DestructiveGuardConfig,
    override_active: bool,
) -> HookVerdict {
    // Windows shells prepend BOMs when files or pipelines re-encode; serde
    // rejects a BOM'd document, and an unparseable payload fails open here —
    // so tolerate the BOM rather than let it disable the guard.
    let raw = raw.trim_start_matches('\u{feff}');
    if raw.trim().is_empty() {
        return HookVerdict::AllowUnparsed {
            note: "empty hook payload".to_owned(),
        };
    }
    let payload: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(error) => {
            return HookVerdict::AllowUnparsed {
                note: format!("hook payload is not valid JSON ({error})"),
            };
        }
    };
    let mut cursor = &payload;
    for key in command_field.split('.') {
        match cursor.get(key) {
            Some(next) => cursor = next,
            None => {
                return HookVerdict::AllowUnparsed {
                    note: format!("hook payload has no `{command_field}` field"),
                };
            }
        }
    }
    let Some(command) = cursor.as_str() else {
        return HookVerdict::AllowUnparsed {
            note: format!("hook payload `{command_field}` is not a string"),
        };
    };
    if command.trim().is_empty() {
        return HookVerdict::Allow;
    }
    // Present the command exactly as a shell payload: argv[0] is a shell
    // stem, so the guard's nested word-scan applies the same rules it uses
    // for `bash -lc '<script>'` children.
    let argv = vec!["sh".to_owned(), "-c".to_owned(), command.to_owned()];
    match evaluate_argv(&argv, config, override_active) {
        DenyDecision::Allow => HookVerdict::Allow,
        DenyDecision::AllowWithOverride { message } => HookVerdict::AllowWithOverride { message },
        DenyDecision::Deny { message } => HookVerdict::Deny { message },
    }
}

#[cfg(test)]
#[path = "hook_guard/tests.rs"]
mod tests;
