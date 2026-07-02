use super::{decode_base64, sed_window_span};

#[test]
fn base64_decodes_standard_urlsafe_and_padded_forms() {
    assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
    assert_eq!(decode_base64("aGVs\nbG8=").unwrap(), b"hello");
    // URL-safe '-'/'_' map onto the standard '+'/'/' values.
    assert_eq!(
        decode_base64("-_-_").unwrap(),
        decode_base64("+/+/").unwrap()
    );
    assert_eq!(decode_base64("").unwrap(), b"");
    assert!(decode_base64("a!b").unwrap_err().contains("0x21"));

    let argv = "printf\0%s\0he said \"hi\"\0^// PART";
    let mut token = String::new();
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in argv.as_bytes().chunks(3) {
        let mut buffer = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            buffer |= u32::from(*byte) << (16 - 8 * index);
        }
        for position in 0..=chunk.len() {
            let shift = 18 - 6 * position;
            token.push(alphabet.as_bytes()[((buffer >> shift) & 0x3f) as usize] as char);
        }
    }
    assert_eq!(decode_base64(&token).unwrap(), argv.as_bytes());
}

#[test]
fn sed_window_spans_parse_print_ranges_only() {
    assert_eq!(sed_window_span("1,460p"), Some(460));
    assert_eq!(sed_window_span("-n930,1260p"), Some(331));
    assert_eq!(sed_window_span("5,5p"), Some(1));
    assert_eq!(sed_window_span("s/a/b/"), None);
    assert_eq!(sed_window_span("1,460d"), None);
    assert_eq!(sed_window_span("460p"), None);
}
