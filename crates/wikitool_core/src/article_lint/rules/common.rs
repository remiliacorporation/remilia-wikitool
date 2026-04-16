use crate::article_lint::document::{ArticleSection, ParsedArticleDocument, TemplateOccurrence};
use crate::article_lint::fix::TextEdit;
use crate::article_lint::model::{SuggestedFix, SuggestedFixKind};
use crate::content_store::parsing::{make_content_preview, normalize_spaces};
use crate::profile::ProfileOverlay;

const COMMON_SENTENCE_CASE_HEADINGS: &[(&str, &str)] = &[
    ("see also", "See also"),
    ("external links", "External links"),
    ("further reading", "Further reading"),
    ("early life", "Early life"),
    ("early life and education", "Early life and education"),
    ("personal life", "Personal life"),
    ("notable works", "Notable works"),
    ("notable work", "Notable work"),
];

pub(super) fn parse_markdown_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim();
    let count = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(2..=6).contains(&count) {
        return None;
    }
    let text = trimmed[count..].trim();
    if text.is_empty() {
        return None;
    }
    Some((u8::try_from(count).unwrap_or(6), text.to_string()))
}

pub(super) fn line_has_short_description(line: &str) -> bool {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();
    lowered.starts_with("{{shortdesc:")
        || lowered.starts_with("{{short description|")
        || lowered.starts_with("{{short description |")
}

pub(super) fn preferred_short_description_snippet(overlay: &ProfileOverlay) -> String {
    if overlay
        .authoring
        .short_description_forms
        .iter()
        .any(|form| form.eq_ignore_ascii_case("magic_word:SHORTDESC"))
    {
        return "{{SHORTDESC:Brief one-line description}}".to_string();
    }
    "{{Short description|Brief one-line description}}".to_string()
}

pub(super) fn section_body_contains_template(
    section: &ArticleSection,
    templates: &[TemplateOccurrence],
    template_title: &str,
) -> bool {
    templates.iter().any(|template| {
        template.start >= section.body_start
            && template.end <= section.body_end
            && template.template_title.eq_ignore_ascii_case(template_title)
    })
}

pub(super) fn canonical_sentence_case_heading(heading: &str) -> Option<String> {
    let normalized = normalize_spaces(heading);
    if normalized.is_empty() {
        return None;
    }
    for (wrong, right) in COMMON_SENTENCE_CASE_HEADINGS {
        if normalized.eq_ignore_ascii_case(wrong) {
            return Some((*right).to_string());
        }
    }

    let words = normalized
        .split_whitespace()
        .filter(|word| word.chars().any(|ch| ch.is_ascii_alphabetic()))
        .collect::<Vec<_>>();
    if words.len() < 3 {
        return None;
    }
    if words
        .iter()
        .skip(1)
        .any(|word| is_stopword(word) && is_title_case_word(word))
    {
        return Some(lowercase_heading_tail(&normalized));
    }
    if words
        .iter()
        .skip(1)
        .filter(|word| is_title_case_word(word))
        .count()
        >= 2
    {
        return Some(lowercase_heading_tail(&normalized));
    }
    None
}

pub(super) fn safe_heading_rewrite_available(original: &str, canonical: &str) -> bool {
    COMMON_SENTENCE_CASE_HEADINGS.iter().any(|(wrong, _)| {
        original.eq_ignore_ascii_case(wrong) || canonical.eq_ignore_ascii_case(wrong)
    })
}

fn lowercase_heading_tail(value: &str) -> String {
    let mut out = Vec::new();
    for (index, word) in value.split_whitespace().enumerate() {
        if index == 0
            || word
                .chars()
                .all(|ch| !ch.is_ascii_alphabetic() || ch.is_ascii_uppercase())
        {
            out.push(word.to_string());
        } else {
            out.push(word.to_ascii_lowercase());
        }
    }
    out.join(" ")
}

fn is_title_case_word(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && chars.clone().any(|ch| ch.is_ascii_lowercase())
        && chars.all(|ch| !ch.is_ascii_alphabetic() || ch.is_ascii_lowercase())
}

fn is_stopword(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "and" | "of" | "the" | "for" | "in" | "to" | "on" | "with"
    )
}

pub(super) fn straight_quote_for(ch: char) -> char {
    match ch {
        '“' | '”' => '"',
        '‘' | '’' => '\'',
        _ => ch,
    }
}

pub(super) fn safe_fix_for_edit(
    document: &ParsedArticleDocument,
    edit: &TextEdit,
    label: &str,
) -> SuggestedFix {
    let patch = patch_preview(document, edit);
    SuggestedFix {
        label: label.to_string(),
        kind: SuggestedFixKind::SafeAutofix,
        replacement_preview: Some(make_content_preview(&edit.replacement, 96)),
        patch: Some(patch),
    }
}

fn patch_preview(document: &ParsedArticleDocument, edit: &TextEdit) -> String {
    let line = document
        .line_for_offset(edit.start)
        .map(|line| line.number)
        .unwrap_or(1);
    let before = if edit.start == edit.end {
        "<insert>".to_string()
    } else {
        make_content_preview(&document.content[edit.start..edit.end], 96)
    };
    let after = if edit.replacement.is_empty() {
        "<delete>".to_string()
    } else {
        make_content_preview(&edit.replacement, 96)
    };
    format!("@@ line {line} @@\n- {before}\n+ {after}")
}
