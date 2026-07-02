use super::*;

#[test]
fn merged_paths_defaults_to_workspace_root() {
    assert_eq!(merged_paths(&[], &[]), vec![PathBuf::from(".")]);
    assert_eq!(
        merged_paths(&[PathBuf::from("src")], &[PathBuf::from("tests")]),
        vec![PathBuf::from("src"), PathBuf::from("tests")]
    );
}
