use super::*;

#[test]
fn byte_truncated_single_line_keeps_head_and_tail_fragments_visible() {
    let raw = RawCapturedStream {
        head: br#"{"rows":["#.to_vec(),
        tail: br#""tail"]}"#.to_vec(),
        tail_start: 128,
        total_bytes: 136,
        total_lines: 1,
    };

    let rendered = render_captured_stream(raw, 8, 120);

    assert!(rendered.byte_truncated);
    assert!(!rendered.display_text.is_empty());
    assert!(rendered.display_text.contains(r#"{"rows":["#));
    assert!(rendered.display_text.contains("[contextmink] ... omitted"));
    assert!(rendered.display_text.contains(r#""tail"]}"#));
}
