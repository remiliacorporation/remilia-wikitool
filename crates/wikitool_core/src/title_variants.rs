#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationVariantInfo {
    pub base_title: String,
    pub language_code: String,
}

pub fn translation_variant_info(title: &str) -> Option<TranslationVariantInfo> {
    let (base_title, suffix) = title.rsplit_once('/')?;
    let base_title = base_title.trim();
    let suffix = suffix.trim();
    if base_title.is_empty() || suffix.is_empty() || suffix.contains(' ') {
        return None;
    }

    let lowered = suffix.to_ascii_lowercase();
    if is_non_translation_suffix(&lowered) {
        return None;
    }
    if lowered == "qqq" {
        return Some(TranslationVariantInfo {
            base_title: base_title.to_string(),
            language_code: lowered,
        });
    }

    let mut parts = lowered.split('-');
    let primary = parts.next()?;
    if !language_code_part_allowed(primary, true) {
        return None;
    }
    if !parts.all(|part| language_code_part_allowed(part, false)) {
        return None;
    }

    Some(TranslationVariantInfo {
        base_title: base_title.to_string(),
        language_code: lowered,
    })
}

pub fn is_translation_variant(title: &str) -> bool {
    translation_variant_info(title).is_some()
}

fn language_code_part_allowed(part: &str, primary: bool) -> bool {
    if part.is_empty() || part.len() > 8 {
        return false;
    }
    if !part
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
    {
        return false;
    }
    if primary {
        return part.len() == 2;
    }
    !part.chars().all(|ch| ch.is_ascii_digit())
}

fn is_non_translation_suffix(suffix: &str) -> bool {
    matches!(
        suffix,
        "doc"
            | "docs"
            | "sandbox"
            | "test"
            | "tests"
            | "testcases"
            | "meta"
            | "archive"
            | "usage"
            | "notes"
    )
}

#[cfg(test)]
mod tests {
    use super::{is_translation_variant, translation_variant_info};

    #[test]
    fn translation_variant_info_accepts_language_suffixes() {
        let info = translation_variant_info("Network spirituality/pt-br").expect("variant");
        assert_eq!(info.base_title, "Network spirituality");
        assert_eq!(info.language_code, "pt-br");
        assert!(is_translation_variant("Hyperstition/ko"));
        assert!(is_translation_variant("Manual:Hooks/PageSave/en"));
        assert!(is_translation_variant("API:Edit/qqq"));
    }

    #[test]
    fn translation_variant_info_rejects_non_language_suffixes() {
        assert!(!is_translation_variant("Template:Infobox person/doc"));
        assert!(!is_translation_variant("API:Edit/Sample code 1"));
        assert!(!is_translation_variant("Network spirituality/History"));
        assert!(translation_variant_info("Foo/bar").is_none());
    }
}
