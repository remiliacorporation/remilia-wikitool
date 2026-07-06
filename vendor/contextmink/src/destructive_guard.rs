//! Blocking deny-list for destructive child argv.
//!
//! Shared by every subprocess-spawn path in this crate: the
//! `contextmink-bridge` binary (all four command forms, including `--script`
//! mode) and `contextmink capture`/`run`.
//!
//! The built-in rule blocks `git clean` because its flags are easy to
//! misunderstand and it deletes ignored files that the tool cannot enumerate
//! safely first. Repositories can optionally add protected path fragments in
//! `.contextmink.toml`; those fragments remain project-owned config, not
//! release-binary policy.
//!
//! Break-glass: `CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE=1` skips the deny with a
//! loud stderr warning at the call site. It exists for human operators doing
//! deliberate, understood maintenance only; agents must never set it.

use crate::config::DestructiveGuardConfig;

/// Break-glass override — human operators only. `=1` runs a denied command
/// anyway; callers must print a loud stderr warning when it fires.
pub(crate) const ALLOW_DESTRUCTIVE_ENV: &str = "CONTEXTMINK_BRIDGE_ALLOW_DESTRUCTIVE";

/// Program stems whose remaining arguments are opaque script payloads that
/// must be re-scanned word by word (`bash -lc '<script>'` and friends).
const SHELL_STEMS: &[&str] = &[
    "bash",
    "sh",
    "dash",
    "zsh",
    "ksh",
    "powershell",
    "pwsh",
    "cmd",
];

const GIT_CLEAN_MESSAGE: &str = "git clean is blocked by contextmink's built-in \
     destructive-command guard. Its -e flag adds ignore patterns instead of protecting files, \
     and -x/-X delete git-ignored artifacts with no recovery path. Delete explicit paths with \
     `rm -f <path>` instead.";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DenyDecision {
    Allow,
    /// Denied argv riding through on the break-glass env var; the caller must
    /// print the message as a loud stderr warning before spawning.
    AllowWithOverride {
        message: String,
    },
    /// Denied argv; the caller must print the message and must not spawn.
    Deny {
        message: String,
    },
}

/// Combine the pure deny scan with the (caller-sampled) break-glass state.
pub(crate) fn evaluate_argv(
    argv: &[String],
    config: &DestructiveGuardConfig,
    override_active: bool,
) -> DenyDecision {
    match deny_destructive_argv(argv, config) {
        None => DenyDecision::Allow,
        Some(message) if override_active => DenyDecision::AllowWithOverride { message },
        Some(message) => DenyDecision::Deny { message },
    }
}

pub(crate) fn destructive_override_active() -> bool {
    std::env::var_os(ALLOW_DESTRUCTIVE_ENV).is_some_and(|value| value == "1")
}

/// Pure deny scan: `Some(message)` when the argv matches a deny rule.
///
/// Tokens are matched at any position (an `env`/`nice`/`xargs` prefix must
/// not hide `rm`), and when argv[0] is a shell the remaining arguments are
/// additionally split into words and re-scanned, so `-lc '<script>'`
/// payloads face the same rules.
fn deny_destructive_argv(argv: &[String], config: &DestructiveGuardConfig) -> Option<String> {
    let first_stem = stem_lower(argv.first()?);
    if let Some(message) = deny_tokens(argv, config) {
        return Some(message);
    }
    if SHELL_STEMS.contains(&first_stem.as_str()) {
        let nested: Vec<String> = argv[1..].iter().flat_map(|arg| shell_words(arg)).collect();
        if let Some(message) = deny_tokens(&nested, config) {
            return Some(message);
        }
    }
    None
}

fn deny_tokens(tokens: &[String], config: &DestructiveGuardConfig) -> Option<String> {
    let stems: Vec<String> = tokens.iter().map(|token| stem_lower(token)).collect();

    // Rule 1: any `git clean` invocation. Flags and `-C <dir>` may sit
    // between `git` and `clean`; a bare later token equal to `clean` is
    // enough (false positives like `git commit -m clean` are acceptable).
    for (index, stem) in stems.iter().enumerate() {
        if stem == "git"
            && tokens[index + 1..]
                .iter()
                .any(|token| token.eq_ignore_ascii_case("clean"))
        {
            return Some(GIT_CLEAN_MESSAGE.to_owned());
        }
    }

    // Rule 2: recursive/forced deletion whose argv also references a fragment
    // the repository explicitly configured as protected.
    if let Some(fragment) = any_fragment(tokens, &config.recursive_delete_fragments) {
        for (index, stem) in stems.iter().enumerate() {
            let after = &tokens[index + 1..];
            let recursive_forced = match stem.as_str() {
                "rm" => {
                    after
                        .iter()
                        .any(|token| rm_flag(token, &['r', 'R'], "--recursive"))
                        && after
                            .iter()
                            .any(|token| rm_flag(token, &['f', 'F'], "--force"))
                }
                "remove-item" | "ri" => after.iter().any(|token| powershell_recurse_flag(token)),
                // `del`/`erase` are both a cmd builtin (`/s` recurses) and
                // PowerShell aliases of Remove-Item (`-Recurse`).
                "del" | "erase" => after.iter().any(|token| {
                    powershell_recurse_flag(token) || token.eq_ignore_ascii_case("/s")
                }),
                "rmdir" | "rd" => after.iter().any(|token| token.eq_ignore_ascii_case("/s")),
                _ => false,
            };
            if recursive_forced {
                return Some(format!(
                    "recursive forced deletion references configured protected path fragment \
                     {fragment:?}; remove or change the fragment in .contextmink.toml only for \
                     deliberate human maintenance"
                ));
            }
        }
    }

    // Rule 3: any deletion verb at all next to a repository-configured
    // protected fragment. Copy/backup tools (robocopy, cp, sqlite3 .backup)
    // are deliberately not listed; backups must not be blocked.
    if let Some(fragment) = any_fragment(tokens, &config.delete_fragments)
        && stems.iter().any(|stem| {
            matches!(
                stem.as_str(),
                "rm" | "del" | "erase" | "unlink" | "remove-item" | "ri" | "rmdir" | "rd"
            )
        })
    {
        return Some(format!(
            "deletion references configured protected path fragment {fragment:?}; remove or \
             change the fragment in .contextmink.toml only for deliberate human maintenance"
        ));
    }

    None
}

/// Lowercased program stem: `git`, `git.exe`, `/usr/bin/git`, and
/// `C:\...\git.EXE` all reduce to `git` on every host OS.
fn stem_lower(token: &str) -> String {
    let leaf = token
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(token);
    let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
    stem.to_ascii_lowercase()
}

fn any_fragment<'a>(tokens: &[String], fragments: &'a [String]) -> Option<&'a str> {
    tokens.iter().find_map(|token| {
        let lower = token.to_ascii_lowercase();
        fragments.iter().find_map(|fragment| {
            let fragment = fragment.trim();
            (!fragment.is_empty() && lower.contains(&fragment.to_ascii_lowercase()))
                .then_some(fragment)
        })
    })
}

/// `-rf`, `-fr`, `-r`, `-Rf`, or the long spelling: short-flag clusters carry
/// the letter anywhere in the cluster.
fn rm_flag(token: &str, letters: &[char], long: &str) -> bool {
    if token == long {
        return true;
    }
    token.len() > 1
        && token.starts_with('-')
        && !token.starts_with("--")
        && token[1..].chars().any(|ch| letters.contains(&ch))
}

/// PowerShell accepts any unambiguous parameter prefix, so `-r`, `-rec`, and
/// `-Recurse` all mean `-Recurse` on Remove-Item.
fn powershell_recurse_flag(token: &str) -> bool {
    let Some(rest) = token.strip_prefix('-') else {
        return false;
    };
    !rest.is_empty() && "recurse".starts_with(&rest.to_ascii_lowercase())
}

/// Conservative word split for nested shell payloads: whitespace plus common
/// shell separators/quotes. `cd x && git clean -fdX` yields `git` and
/// `clean` as separate words regardless of quoting style.
fn shell_words(text: &str) -> Vec<String> {
    text.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ';' | '&' | '|' | '(' | ')' | '{' | '}' | '<' | '>' | '"' | '\'' | '`'
            )
    })
    .filter(|word| !word.is_empty())
    .map(str::to_owned)
    .collect()
}

// Explicit path: this module is #[path]-included by both the contextmink
// and contextmink-bridge targets, which makes it mod-rs for child
// resolution — a bare `mod tests;` would look for src/tests.rs.
#[cfg(test)]
#[path = "destructive_guard/tests.rs"]
mod tests;
