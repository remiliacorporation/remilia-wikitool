use super::*;

#[test]
fn parse_line_range_requires_bounded_one_based_range() {
    assert_eq!(parse_line_range("10:20").unwrap(), (10, Some(20)));
    assert!(parse_line_range("10").is_err());
    assert!(parse_line_range("0:1").is_err());
    assert!(parse_line_range("20:10").is_err());
}
