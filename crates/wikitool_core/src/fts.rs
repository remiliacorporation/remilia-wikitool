//! Shared FTS5 MATCH-expression construction.
//!
//! Query text reaching the local index comes from topics, titles, and free-form
//! agent queries. Interpolating it into a MATCH expression unescaped is a syntax
//! error as soon as it carries a double quote, and wrapping the whole query in
//! one phrase makes multi-word queries match only the exact contiguous phrase.
//! Every MATCH built from arbitrary text must go through
//! [`fts_prefix_match_expression`].

/// Build a safe FTS5 MATCH expression from arbitrary query text.
///
/// Tokenizes on non-alphanumeric boundaries and requires every token as a
/// quoted prefix: `"network"* AND "spirituality"*`. Multi-token queries also
/// keep the exact phrase as an OR branch so contiguous-phrase hits still match
/// (and rank ahead under bm25): `("network spirituality") OR ("network"* AND
/// "spirituality"*)`. Returns `None` when no token survives tokenization; the
/// caller should skip the FTS query entirely.
pub(crate) fn fts_prefix_match_expression(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect();
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 {
        return Some(format!("\"{}\"*", tokens[0]));
    }
    let phrase = format!("\"{}\"", tokens.join(" "));
    let conjunction = tokens
        .iter()
        .map(|token| format!("\"{token}\"*"))
        .collect::<Vec<_>>()
        .join(" AND ");
    Some(format!("({phrase}) OR ({conjunction})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_token_becomes_quoted_prefix() {
        assert_eq!(
            fts_prefix_match_expression("network").as_deref(),
            Some("\"network\"*")
        );
    }

    #[test]
    fn multi_token_keeps_phrase_and_requires_all_prefixes() {
        assert_eq!(
            fts_prefix_match_expression("network spirituality").as_deref(),
            Some("(\"network spirituality\") OR (\"network\"* AND \"spirituality\"*)")
        );
    }

    #[test]
    fn embedded_quotes_and_punctuation_cannot_malform_the_expression() {
        assert_eq!(
            fts_prefix_match_expression("Mc\"Donald's (draft)").as_deref(),
            Some("(\"Mc Donald s draft\") OR (\"Mc\"* AND \"Donald\"* AND \"s\"* AND \"draft\"*)")
        );
    }

    #[test]
    fn hyphens_and_underscores_split_into_tokens() {
        assert_eq!(
            fts_prefix_match_expression("post-authorship_theory").as_deref(),
            Some("(\"post authorship theory\") OR (\"post\"* AND \"authorship\"* AND \"theory\"*)")
        );
    }

    #[test]
    fn unicode_letters_survive_tokenization() {
        assert_eq!(
            fts_prefix_match_expression("ミラディ 東京").as_deref(),
            Some("(\"ミラディ 東京\") OR (\"ミラディ\"* AND \"東京\"*)")
        );
    }

    #[test]
    fn operator_only_input_yields_none() {
        assert_eq!(fts_prefix_match_expression("\"*()^-"), None);
        assert_eq!(fts_prefix_match_expression("   "), None);
        assert_eq!(fts_prefix_match_expression(""), None);
    }
}
