use super::*;

#[test]
fn parses_profile_and_multiline_exclude_globs() {
    let config = parse_config(
        "# repo policy\nprofile = \"demo\"\n\nexclude_globs = [\n  \"target/**\", # build output\n  \"**/node_modules/**\",\n]\n",
    )
    .unwrap();
    assert_eq!(config.profile.as_deref(), Some("demo"));
    assert_eq!(
        config.exclude_globs.unwrap(),
        vec!["target/**".to_owned(), "**/node_modules/**".to_owned()]
    );
}

#[test]
fn parses_destructive_guard_fragment_lists() {
    let config = parse_config(
        "destructive_guard_recursive_delete_fragments = [\n  \"protected_cache\",\n]\ndestructive_guard_delete_fragments = [\"critical.sqlite\", 'project.gpr']\n",
    )
    .unwrap();
    assert_eq!(
        config.destructive_guard_recursive_delete_fragments.unwrap(),
        vec!["protected_cache".to_owned()]
    );
    assert_eq!(
        config.destructive_guard_delete_fragments.unwrap(),
        vec!["critical.sqlite".to_owned(), "project.gpr".to_owned()]
    );
}

#[test]
fn parses_single_line_array() {
    let config = parse_config("exclude_globs = [\"a/**\", 'b/**']\n").unwrap();
    assert_eq!(
        config.exclude_globs.unwrap(),
        vec!["a/**".to_owned(), "b/**".to_owned()]
    );
}

#[test]
fn unknown_keys_fail_fast() {
    let error = parse_config("exclude_glob = [\"typo/**\"]\n").unwrap_err();
    assert!(error.to_string().contains("unknown key `exclude_glob`"));
}

#[test]
fn duplicate_keys_fail_fast() {
    let error = parse_config("profile = \"a\"\nprofile = \"b\"\n").unwrap_err();
    assert!(error.to_string().contains("duplicate key `profile`"));

    let error = parse_config(
        "destructive_guard_delete_fragments = [\"a\"]\ndestructive_guard_delete_fragments = [\"b\"]\n",
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("duplicate key `destructive_guard_delete_fragments`")
    );
}

#[test]
fn unterminated_array_fails_fast() {
    let error = parse_config("exclude_globs = [\n  \"a/**\",\nprofile = \"x\"\n").unwrap_err();
    assert!(error.to_string().contains("never closed"));
}

#[test]
fn comments_inside_strings_are_preserved() {
    let config = parse_config("profile = \"has#hash\"\n").unwrap();
    assert_eq!(config.profile.as_deref(), Some("has#hash"));
}
