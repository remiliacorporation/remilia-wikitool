use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TextEdit {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) replacement: String,
}

pub(crate) fn apply_text_edits(content: &str, edits: &[TextEdit]) -> Result<String> {
    if edits.is_empty() {
        return Ok(content.to_string());
    }

    let mut ordered = edits.to_vec();
    ordered.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then(left.end.cmp(&right.end))
            .then(left.replacement.cmp(&right.replacement))
    });

    let mut cursor = 0usize;
    for edit in &ordered {
        if edit.start > edit.end || edit.end > content.len() {
            bail!("invalid text edit range: {}..{}", edit.start, edit.end);
        }
        if edit.start < cursor {
            bail!("overlapping text edits are not safe to apply");
        }
        cursor = edit.end;
    }

    let mut output = String::with_capacity(content.len());
    let mut previous_end = 0usize;
    for edit in &ordered {
        output.push_str(&content[previous_end..edit.start]);
        output.push_str(&edit.replacement);
        previous_end = edit.end;
    }
    output.push_str(&content[previous_end..]);
    Ok(output)
}
