use std::fs;
use std::path::PathBuf;

use super::{decode_base64, resolve_root_from_exe_dir, sed_window_span};

fn temp_tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("bridge-root-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root); // guardrail: allow-ignore-result cleanup is best-effort for reused test temp dirs
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn root_resolution_prefers_policy_root_over_nested_vendored_git() {
    // Workspace layout: <ws>/.contextmink.toml with a vendored contextmink
    // checkout (its own .git) at <ws>/tools/contextmink and the bridge binary
    // under its target/release. Relative paths must anchor to <ws>.
    let workspace = temp_tree("policy");
    fs::write(workspace.join(".contextmink.toml"), "profile = \"t\"\n").unwrap();
    let exe_dir = workspace.join("tools/contextmink/target/release");
    fs::create_dir_all(&exe_dir).unwrap();
    fs::create_dir_all(workspace.join("tools/contextmink/.git")).unwrap();
    assert_eq!(resolve_root_from_exe_dir(&exe_dir), Some(workspace.clone()));

    // Standalone clone: no policy file anywhere, nearest .git wins.
    let clone = temp_tree("standalone");
    fs::create_dir_all(clone.join(".git")).unwrap();
    let exe_dir = clone.join("target/release");
    fs::create_dir_all(&exe_dir).unwrap();
    assert_eq!(resolve_root_from_exe_dir(&exe_dir), Some(clone.clone()));

    // Neither marker inside our tree: resolution must not invent a root
    // within it (a host-level ancestor .git outside the temp tree may still
    // resolve, so only the absence of a false positive is asserted).
    let bare = temp_tree("bare");
    let exe_dir = bare.join("bin");
    fs::create_dir_all(&exe_dir).unwrap();
    let resolved = resolve_root_from_exe_dir(&exe_dir);
    assert!(
        resolved
            .as_deref()
            .is_none_or(|root| !root.starts_with(&bare)),
        "resolved: {resolved:?}"
    );
}

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
