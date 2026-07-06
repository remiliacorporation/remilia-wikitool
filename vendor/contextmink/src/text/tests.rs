use super::*;

#[test]
fn contains_ignore_case_folds_ascii_bytes() {
    assert!(contains_ignore_case("CacheChunk", "cachechunk"));
    assert!(contains_ignore_case("prefix MiXeD suffix", "mixed"));
    assert!(contains_ignore_case("shout", "shout"));
    assert!(!contains_ignore_case("CacheChunk", "cgraph"));
    assert!(!contains_ignore_case("ab", "abc"));
}

#[test]
fn contains_ignore_case_non_ascii_needle_lowercases_haystack() {
    assert!(contains_ignore_case("STRA\u{df}E \u{dc}BER", "\u{fc}ber"));
    assert!(!contains_ignore_case("plain ascii", "\u{fc}ber"));
}

#[test]
fn contains_ignore_case_never_matches_inside_a_multibyte_codepoint() {
    // Every byte of a multibyte UTF-8 sequence is >= 0x80 and never folds to
    // ASCII, so an ASCII needle cannot false-match mid-codepoint.
    assert!(!contains_ignore_case("na\u{ef}ve", "ive"));
    assert!(contains_ignore_case("na\u{ef}ve", "ve"));
    assert!(!contains_ignore_case("\u{65e5}\u{672c}\u{8a9e}", "e"));
    // ASCII fast path deliberately skips Unicode folds: KELVIN SIGN (U+212A)
    // does not match ASCII 'k'.
    assert!(!contains_ignore_case("\u{212a}elvin", "k"));
}

#[test]
fn contains_ignore_case_handles_empty_needle_and_haystack() {
    assert!(contains_ignore_case("", ""));
    assert!(contains_ignore_case("anything", ""));
    assert!(!contains_ignore_case("", "a"));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "pre-lowercased")]
fn contains_ignore_case_rejects_uppercase_ascii_needle() {
    contains_ignore_case("abc", "ABC");
}

#[test]
fn parse_line_range_requires_bounded_one_based_range() {
    assert_eq!(parse_line_range("10:20").unwrap(), (10, Some(20)));
    assert!(parse_line_range("10").is_err());
    assert!(parse_line_range("0:1").is_err());
    assert!(parse_line_range("20:10").is_err());
}
