use super::*;
use clap::Parser;

#[test]
fn merged_paths_defaults_to_workspace_root() {
    assert_eq!(merged_paths(&[], &[]), vec![PathBuf::from(".")]);
    assert_eq!(
        merged_paths(&[PathBuf::from("src")], &[PathBuf::from("tests")]),
        vec![PathBuf::from("src"), PathBuf::from("tests")]
    );
}

#[test]
fn grep_accepts_named_pattern_and_positional_paths() {
    let cli = Cli::try_parse_from([
        "contextmink",
        "grep",
        "--pattern",
        "implementation-query",
        "ghidramink/tools/ghidramink-core/src",
    ])
    .expect("parse grep --pattern");

    match cli.command {
        Command::Grep { args, pattern, .. } => {
            assert_eq!(pattern.as_deref(), Some("implementation-query"));
            assert_eq!(args, vec!["ghidramink/tools/ghidramink-core/src"]);
        }
        _ => panic!("expected grep command"),
    }
}
