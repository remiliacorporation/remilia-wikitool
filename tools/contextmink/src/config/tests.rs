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
