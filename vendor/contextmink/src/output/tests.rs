use super::*;
use serde_json::{Value, json};

#[test]
fn clamp_text_is_character_safe() {
    assert_eq!(clamp_text("abcdef", 3), "abc...");
    assert_eq!(clamp_text("abc", 3), "abc");
    assert_eq!(clamp_text("a->b", 20), "a->b");
}

#[test]
fn base_receipt_has_stable_envelope() {
    let map = base_receipt("grep", Some("demo"), "files", 3, 12, true, Some("files"));
    assert_eq!(map["tool"], json!("contextmink"));
    assert_eq!(map["unit"], json!("files"));
    assert_eq!(map["shown"], json!(3));
    assert_eq!(map["total"], json!(12));
    assert_eq!(map["truncated"], json!(true));
    assert_eq!(map["complete"], json!(false));
    assert_eq!(map["cap_reason"], json!("files"));

    let complete = base_receipt("files", None, "files", 5, 5, false, None);
    assert_eq!(complete["truncated"], json!(false));
    assert_eq!(complete["complete"], json!(true));
    assert_eq!(complete["cap_reason"], Value::Null);
    assert_eq!(complete["profile"], Value::Null);
}
