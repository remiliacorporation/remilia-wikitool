use super::*;

fn utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[test]
fn decodes_utf16le_bom_files_as_text() {
    let bytes = utf16le_with_bom("needle in utf16\nsecond line\n");
    match decode_bytes(&bytes) {
        FileText::Text { text, encoding } => {
            assert_eq!(encoding, "utf16le");
            assert!(text.contains("needle in utf16"));
        }
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn decodes_utf16be_bom_files_as_text() {
    let mut bytes = vec![0xFE, 0xFF];
    for unit in "beacon".encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    match decode_bytes(&bytes) {
        FileText::Text { text, encoding } => {
            assert_eq!(encoding, "utf16be");
            assert_eq!(text, "beacon");
        }
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn strips_utf8_bom_before_decoding() {
    let bytes = b"\xEF\xBB\xBF{\"a\":1}".to_vec();
    match decode_bytes(&bytes) {
        FileText::Text { text, encoding } => {
            assert_eq!(encoding, "utf8");
            assert_eq!(text, "{\"a\":1}");
        }
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn nul_bytes_without_utf16_bom_stay_binary() {
    assert!(matches!(
        decode_bytes(b"MZ\x00\x00binary"),
        FileText::SkippedBinary
    ));
}
