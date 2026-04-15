#[cfg(any(test, feature = "maintainer-surface"))]
use super::*;

#[cfg(feature = "maintainer-surface")]
#[derive(Debug, Args)]
pub(crate) struct DocsGenerateReferenceArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Output markdown path (default: <repo>/docs/wikitool/reference.md)"
    )]
    pub(crate) output: Option<PathBuf>,
}

#[cfg(feature = "maintainer-surface")]
pub(crate) fn run_docs_generate_reference(args: DocsGenerateReferenceArgs) -> Result<()> {
    let output = match args.output {
        Some(output) if output.is_absolute() => output,
        Some(output) => std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(output),
        None => source_repo_root()?.join("docs/wikitool/reference.md"),
    };

    let markdown = generate_docs_reference_markdown()?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", normalize_path(parent)))?;
    }
    fs::write(&output, markdown)
        .with_context(|| format!("failed to write {}", normalize_path(&output)))?;

    println!("Wrote {}", normalize_path(&output));
    Ok(())
}

#[cfg(feature = "maintainer-surface")]
fn generate_docs_reference_markdown() -> Result<String> {
    let command = Cli::command();
    let mut command_paths = Vec::new();
    collect_command_paths(&command, &[], &mut command_paths);

    let mut lines = vec![
        "# Wikitool Command Reference".to_string(),
        "".to_string(),
        "This file is generated from Rust CLI help output. Do not edit manually.".to_string(),
        "".to_string(),
        "Maintainer-only commands hidden from default help are intentionally omitted.".to_string(),
        "".to_string(),
        "Regenerate from a source checkout with the maintainer surface enabled:".to_string(),
        "".to_string(),
        "```bash".to_string(),
        "wikitool docs generate-reference".to_string(),
        "```".to_string(),
        "".to_string(),
    ];

    for path in command_paths {
        let title = if path.is_empty() {
            "Global".to_string()
        } else {
            path.join(" ")
        };
        let help_text = help_text_for_command_path(&path)?;
        lines.push(format!("## {title}"));
        lines.push(String::new());
        lines.push("```text".to_string());
        lines.push(help_text);
        lines.push("```".to_string());
        lines.push(String::new());
    }

    Ok(lines.join("\n"))
}

#[cfg(feature = "maintainer-surface")]
fn source_repo_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .context("failed to resolve wikitool source repository root")
}

#[cfg(any(test, feature = "maintainer-surface"))]
fn collect_command_paths(command: &Command, prefix: &[String], out: &mut Vec<Vec<String>>) {
    out.push(prefix.to_vec());

    for subcommand in command.get_subcommands() {
        if subcommand.is_hide_set() {
            continue;
        }
        let mut next = prefix.to_vec();
        next.push(subcommand.get_name().to_string());
        collect_command_paths(subcommand, &next, out);
    }
}

#[cfg(any(test, feature = "maintainer-surface"))]
fn help_text_for_command_path(path: &[String]) -> Result<String> {
    let mut command = Cli::command();
    command = command.bin_name("wikitool");

    let mut args = Vec::with_capacity(path.len() + 2);
    args.push("wikitool".to_string());
    args.extend(path.iter().cloned());
    args.push("--help".to_string());

    match command.try_get_matches_from(args) {
        Ok(_) => bail!(
            "failed to render help for path {}",
            if path.is_empty() {
                "<global>".to_string()
            } else {
                path.join(" ")
            }
        ),
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                Ok(normalize_help_text(&error.to_string()))
            }
            _ => Err(error).with_context(|| {
                format!(
                    "failed to resolve command path {}",
                    if path.is_empty() {
                        "<global>".to_string()
                    } else {
                        path.join(" ")
                    }
                )
            }),
        },
    }
}

#[cfg(any(test, feature = "maintainer-surface"))]
fn normalize_help_text(help_text: &str) -> String {
    help_text
        .trim_end()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_command_paths_skips_hidden_commands() {
        let command = Cli::command();
        let mut paths = Vec::new();
        collect_command_paths(&command, &[], &mut paths);

        assert!(!paths.iter().any(|path| path == &["workflow".to_string()]));
        assert!(!paths.iter().any(|path| path == &["release".to_string()]));
        assert!(!paths.iter().any(|path| path == &["dev".to_string()]));
        assert!(
            !paths
                .iter()
                .any(|path| { path == &["docs".to_string(), "generate-reference".to_string()] })
        );
        assert!(paths.iter().any(|path| path == &["research".to_string()]));
        assert!(paths.iter().any(|path| path == &["lsp".to_string()]));
        assert!(
            paths
                .iter()
                .any(|path| path == &["lsp".to_string(), "generate-config".to_string()])
        );
    }

    #[test]
    #[cfg(feature = "maintainer-surface")]
    fn hidden_commands_remain_invocable_directly() {
        let release_help =
            help_text_for_command_path(&["release".to_string()]).expect("render release help");
        let workflow_help =
            help_text_for_command_path(&["workflow".to_string()]).expect("render workflow help");

        assert!(release_help.contains("Usage: wikitool release"));
        assert!(workflow_help.contains("Usage: wikitool workflow"));
    }

    #[test]
    fn removed_top_level_aliases_are_not_invocable() {
        assert!(help_text_for_command_path(&["search-external".to_string()]).is_err());
        assert!(help_text_for_command_path(&["lsp:generate-config".to_string()]).is_err());
    }

    #[test]
    fn normalize_help_text_removes_clap_alignment_padding() {
        assert_eq!(
            normalize_help_text("Options:\n      --flag  \n  -h, --help  \n\n"),
            "Options:\n      --flag\n  -h, --help"
        );
    }
}
