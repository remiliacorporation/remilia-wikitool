use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::artifact::portable;

pub(crate) fn canonicalize_exact_paths(
    bytes: &[u8],
    paths: &[(PathBuf, &'static str)],
) -> Result<Vec<u8>> {
    let mut replacements = Vec::new();
    for (path, token) in paths {
        for encoding in exact_path_encodings(path)? {
            replacements.push((encoding, token.as_bytes().to_vec()));
        }
    }
    replacements.sort_by_key(|right| std::cmp::Reverse(right.0.len()));
    replacements.dedup_by(|left, right| left.0 == right.0);
    let mut canonical = bytes.to_vec();
    for (value, token) in replacements {
        canonical = replace_exact_bytes(&canonical, &value, &token);
    }
    Ok(canonical)
}

fn exact_path_encodings(path: &Path) -> Result<BTreeSet<Vec<u8>>> {
    let raw = path.to_string_lossy().into_owned();
    let mut values = BTreeSet::from([raw.clone(), portable(path)]);
    if let Some(value) = raw.strip_prefix(r"\\?\UNC\") {
        values.insert(format!(r"\\{value}"));
    } else if let Some(value) = raw.strip_prefix(r"\\?\") {
        values.insert(value.to_owned());
    }
    for value in values.clone() {
        values.insert(value.replace('\\', "/"));
    }
    let mut encodings = BTreeSet::new();
    for value in values {
        encodings.insert(value.as_bytes().to_vec());
        let json = serde_json::to_string(&value)?;
        encodings.insert(json.as_bytes()[1..json.len() - 1].to_vec());
    }
    Ok(encodings)
}

fn replace_exact_bytes(bytes: &[u8], value: &[u8], replacement: &[u8]) -> Vec<u8> {
    if value.is_empty() {
        return bytes.to_vec();
    }
    let mut result = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while let Some(offset) = bytes[cursor..]
        .windows(value.len())
        .position(|window| window == value)
    {
        let start = cursor + offset;
        result.extend_from_slice(&bytes[cursor..start]);
        result.extend_from_slice(replacement);
        cursor = start + value.len();
    }
    result.extend_from_slice(&bytes[cursor..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_windows_verbatim_and_json_paths_are_canonicalized() {
        let root = PathBuf::from(r"\\?\C:\Users\Onno\AppData\Local\Temp\wikitest-run");
        let input = br#"plain=C:/Users/Onno/AppData/Local/Temp/wikitest-run json=\\\\?\\C:\\Users\\Onno\\AppData\\Local\\Temp\\wikitest-run slash=//?/C:/Users/Onno/AppData/Local/Temp/wikitest-run"#;
        let canonical = canonicalize_exact_paths(input, &[(root, "<ROOT>")]).expect("canonical");
        let text = String::from_utf8(canonical).expect("utf8");
        assert_eq!(text, "plain=<ROOT> json=<ROOT> slash=<ROOT>");
    }

    #[test]
    fn unrelated_host_content_is_not_rewritten() {
        let root = PathBuf::from(r"C:\Users\Onno\AppData\Local\Temp\wikitest-run");
        let input = br#"source says C:\Users\Onno\Documents\notes.txt"#;
        assert_eq!(
            canonicalize_exact_paths(input, &[(root, "<ROOT>")]).expect("canonical"),
            input
        );
    }
}
