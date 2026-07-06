use super::*;
use serde_json::json;

#[test]
fn json_identifier_filter_matches_plain_keys_only() {
    assert!(is_json_identifier("alpha_beta1"));
    assert!(!is_json_identifier("1alpha"));
    assert!(!is_json_identifier("alpha-beta"));
}

#[test]
fn value_summary_keeps_large_json_structural() {
    let large = json!({
        "items": (0..120).map(|index| json!({"index": index})).collect::<Vec<_>>(),
        "kind": "large",
    });
    let summary = value_summary(&large, 80);
    assert!(summary.starts_with("<object:2 keys sample="));
    assert!(!summary.contains("\"index\":119"));

    let small = json!({"address": "0x7FF954", "function_count": 12});
    assert_eq!(
        value_summary(&small, 200),
        "{\"address\":\"0x7FF954\",\"function_count\":12}"
    );
}

#[test]
fn normalizes_msys_converted_json_selector() {
    assert_eq!(
        normalize_json_selector_arg("C:/Program Files/Git/textures"),
        "/textures"
    );
    assert_eq!(
        normalize_msys_drive_git_selector("C:/Program Files/Git/textures"),
        Some("/textures".to_owned())
    );
    assert_eq!(
        normalize_msys_converted_json_selector("D:/Tools/Git/textures/0/path", "D:/Tools/Git"),
        Some("/textures/0/path".to_owned())
    );
    assert_eq!(normalize_msys_drive_git_selector("/textures"), None);
}
