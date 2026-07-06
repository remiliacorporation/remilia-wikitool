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

#[test]
fn encoding_suspects_prove_double_encoding_by_round_trip() {
    // Mojibake is generated here, never written as literal bytes, so the
    // fixture cannot itself be "repaired" and the em-dash/arrow/é each
    // round-trips back to its true character.
    let text = cp1252_double_encode("dash — arrow → eacute é");
    let suspects = scan_encoding_suspects(&text, false);
    assert_eq!(suspects.double_encoded, 3);
    assert_eq!(suspects.replacement_chars, 0);
    assert_eq!(suspects.c1_controls, 0);
    let sample = suspects.sample.expect("sample");
    assert!(sample.contains('—'), "repair shown: {sample}");
    assert!(sample.contains("line 1"), "{sample}");

    // Second line sample numbering.
    let text = format!("clean line\nbad {} here", cp1252_double_encode("é"));
    let suspects = scan_encoding_suspects(&text, false);
    assert!(suspects.sample.expect("sample").contains("line 2"));
}

#[test]
fn encoding_suspects_ignore_legitimate_latin_text() {
    // Accented letters map to CP1252 bytes but never form valid multi-byte
    // UTF-8 on their own; none of these may flag.
    for text in [
        "À la carte — déjà vu, âge, côté, für, señor",
        "Ão is not a lead+continuation pair",
        "Â followed by space",
        "L'élève étudie l'histoire à l'école",
    ] {
        let suspects = scan_encoding_suspects(text, false);
        assert!(
            suspects.is_empty(),
            "false positive on {text:?}: {suspects:?}"
        );
    }
}

#[test]
fn encoding_suspects_count_replacement_and_c1_separately() {
    let suspects = scan_encoding_suspects("lossy \u{FFFD} and raw C1 \u{92} control", false);
    assert_eq!(suspects.double_encoded, 0);
    assert_eq!(suspects.replacement_chars, 1);
    assert_eq!(suspects.c1_controls, 1);
    assert!(suspects.sample.is_none());

    // double_encode_only mode (capture streams) skips both.
    let text = format!(
        "lossy \u{FFFD} raw \u{92} but {} real",
        cp1252_double_encode("é")
    );
    let suspects = scan_encoding_suspects(&text, true);
    assert_eq!(suspects.replacement_chars, 0);
    assert_eq!(suspects.c1_controls, 0);
    assert_eq!(suspects.double_encoded, 1);
}

#[test]
fn encoding_suspects_reject_invalid_round_trips() {
    // Lead-shaped char followed by a continuation-range char whose bytes do
    // NOT form valid UTF-8 (overlong / out of range) stays unflagged.
    // 0xC0/0xC1 are not in the accepted lead range at all.
    let suspects = scan_encoding_suspects("ÀÁ á é ú", false);
    assert!(suspects.is_empty(), "{suspects:?}");
}

/// Encode `text` as UTF-8, then read those bytes back as WHATWG windows-1252
/// — the exact double-encode a UTF-8 stream suffers through a CP1252
/// boundary. Undefined CP1252 bytes pass through as their C1 code point.
fn cp1252_double_encode(text: &str) -> String {
    const SPECIALS: &[(u8, char)] = &[
        (0x80, '\u{20AC}'),
        (0x82, '\u{201A}'),
        (0x83, '\u{0192}'),
        (0x84, '\u{201E}'),
        (0x85, '\u{2026}'),
        (0x86, '\u{2020}'),
        (0x87, '\u{2021}'),
        (0x88, '\u{02C6}'),
        (0x89, '\u{2030}'),
        (0x8A, '\u{0160}'),
        (0x8B, '\u{2039}'),
        (0x8C, '\u{0152}'),
        (0x8E, '\u{017D}'),
        (0x91, '\u{2018}'),
        (0x92, '\u{2019}'),
        (0x93, '\u{201C}'),
        (0x94, '\u{201D}'),
        (0x95, '\u{2022}'),
        (0x96, '\u{2013}'),
        (0x97, '\u{2014}'),
        (0x98, '\u{02DC}'),
        (0x99, '\u{2122}'),
        (0x9A, '\u{0161}'),
        (0x9B, '\u{203A}'),
        (0x9C, '\u{0153}'),
        (0x9E, '\u{017E}'),
        (0x9F, '\u{0178}'),
    ];
    text.bytes()
        .map(|b| {
            if let Some((_, ch)) = SPECIALS.iter().find(|(byte, _)| *byte == b) {
                *ch
            } else {
                b as char
            }
        })
        .collect()
}

#[test]
fn encoding_suspects_do_not_flag_accented_capital_before_punctuation() {
    // The stress-test false positive: `CAFÉ»` round-trips (É»->ɻ) but is
    // plain typography. An isolated 2-byte run with a non-Latin-1 lead must
    // stay clean.
    for text in [
        "«LE CAFÉ», l'élève — déjà vu",
        "RÉSUMÉ» and CAFÉ° here",
        "PROVENÇAL «mot»",
    ] {
        let suspects = scan_encoding_suspects(text, false);
        assert!(
            suspects.is_empty(),
            "false positive on {text:?}: {suspects:?}"
        );
    }
}

#[test]
fn encoding_suspects_recover_dense_non_latin_mojibake_by_clustering() {
    // Double-encoded Cyrillic is all 2-byte runs with non-Latin-1 leads
    // (0xD0/0xD1); isolated they'd be dropped, but dense clustering rescues
    // them.
    let cyrillic = cp1252_double_encode("Привет мир, как дела?");
    let suspects = scan_encoding_suspects(&cyrillic, false);
    assert!(
        suspects.double_encoded > 5,
        "cyrillic mojibake missed: {suspects:?}"
    );

    let greek = cp1252_double_encode("Ελληνικά κείμενα");
    assert!(scan_encoding_suspects(&greek, false).double_encoded > 5);
}

#[test]
fn encoding_suspects_neighbor_check_is_linear_on_dense_weak_runs() {
    // A large dense weak-run file must not trigger O(n^2) behavior on the
    // passive read path. This is ~50k runs; a quadratic scan would hang.
    let dense = cp1252_double_encode(&"Привет ".repeat(8000));
    let suspects = scan_encoding_suspects(&dense, false);
    assert!(suspects.double_encoded > 10000);
}

#[test]
fn encoding_suspects_keep_latin1_and_multibyte_regardless_of_clustering() {
    // Latin-1 (0xC2/0xC3) and 3-4 byte runs are strong on their own even
    // when isolated among ASCII.
    let e = cp1252_double_encode("é"); // 2-byte, lead 0xC3
    assert_eq!(
        scan_encoding_suspects(&format!("word {e} word"), false).double_encoded,
        1
    );
    let c = cp1252_double_encode("©"); // 2-byte, lead 0xC2
    assert_eq!(
        scan_encoding_suspects(&format!("a {c} b"), false).double_encoded,
        1
    );
    let d = cp1252_double_encode("—"); // 3-byte
    assert_eq!(
        scan_encoding_suspects(&format!("dash {d} end"), false).double_encoded,
        1
    );
    let emoji = cp1252_double_encode("😀"); // 4-byte
    assert_eq!(
        scan_encoding_suspects(&format!("emoji {emoji} !"), false).double_encoded,
        1
    );
}
