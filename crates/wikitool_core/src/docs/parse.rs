pub(crate) use self::parse_common::{
    classify_docs_page_kind, collapse_whitespace, estimate_token_count, estimate_tokens,
    is_translation_variant, make_summary_text, namespace_label, normalize_retrieval_key,
    normalize_title,
};
use self::parse_examples::extract_examples_for_section;
use self::parse_markup::{dedupe_strings, extract_link_titles, extract_template_titles};
use self::parse_sections::{
    build_page_aliases, build_page_links, build_section_semantic_text, build_semantic_text,
    split_into_sections,
};
use self::parse_symbols::{dedupe_symbols, extract_content_symbols, extract_title_symbols};

#[path = "parse_common.rs"]
mod parse_common;
#[path = "parse_examples.rs"]
mod parse_examples;
#[path = "parse_markup.rs"]
mod parse_markup;
#[path = "parse_sections.rs"]
mod parse_sections;
#[path = "parse_symbols.rs"]
mod parse_symbols;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsPage {
    pub page_title: String,
    pub page_namespace: String,
    pub page_kind: String,
    pub local_path: String,
    pub source_revision_id: Option<i64>,
    pub source_parent_revision_id: Option<i64>,
    pub source_timestamp: Option<String>,
    pub summary_text: String,
    pub lead_text: String,
    pub headings_text: String,
    pub alias_titles: Vec<String>,
    pub link_titles: Vec<String>,
    pub template_titles: Vec<String>,
    pub symbol_names: Vec<String>,
    pub normalized_content: String,
    pub semantic_text: String,
    pub content: String,
    pub token_estimate: usize,
    pub sections: Vec<ParsedDocsSection>,
    pub symbols: Vec<ParsedDocsSymbol>,
    pub examples: Vec<ParsedDocsExample>,
    pub links: Vec<ParsedDocsLink>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsSection {
    pub section_index: usize,
    pub heading: String,
    pub section_heading: Option<String>,
    pub heading_path: String,
    pub section_level: u8,
    pub section_kind: String,
    pub summary_text: String,
    pub section_text: String,
    pub semantic_text: String,
    pub symbol_names: Vec<String>,
    pub link_titles: Vec<String>,
    pub token_estimate: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsSymbol {
    pub symbol_name: String,
    pub canonical_name: String,
    pub symbol_kind: String,
    pub page_title: String,
    pub section_heading: Option<String>,
    pub signature_text: String,
    pub summary_text: String,
    pub aliases: Vec<String>,
    pub origin: String,
    pub normalized_symbol_key: String,
    pub detail_text: String,
    pub retrieval_text: String,
    pub token_estimate: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsExample {
    pub example_index: usize,
    pub page_title: String,
    pub section_heading: Option<String>,
    pub example_kind: String,
    pub language: Option<String>,
    pub language_hint: String,
    pub summary_text: String,
    pub example_text: String,
    pub retrieval_text: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedDocsLink {
    pub target_title: String,
    pub relation_kind: String,
    pub display_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DocsPageParseInput {
    pub page_title: String,
    pub local_path: String,
    pub content: String,
    pub source_revision_id: Option<i64>,
    pub source_parent_revision_id: Option<i64>,
    pub source_timestamp: Option<String>,
}

pub(crate) fn parse_docs_page(input: DocsPageParseInput) -> ParsedDocsPage {
    let page_kind = classify_docs_page_kind(&input.page_title);
    let page_namespace = namespace_label(&input.page_title);
    let raw_sections = split_into_sections(&input.content);
    let link_titles = extract_link_titles(&input.content);
    let template_titles = extract_template_titles(&input.content);
    let mut symbols = extract_title_symbols(&input.page_title, &page_kind);
    symbols.extend(extract_content_symbols(
        &input.page_title,
        &page_kind,
        &input.content,
        &raw_sections,
    ));
    dedupe_symbols(&mut symbols);

    let mut sections = Vec::with_capacity(raw_sections.len());
    for (index, raw_section) in raw_sections.iter().enumerate() {
        let section_link_titles = extract_link_titles(&raw_section.text);
        let section_symbol_names = symbols
            .iter()
            .filter(|symbol| {
                symbol
                    .section_heading
                    .as_deref()
                    .is_some_and(|heading| heading == raw_section.heading)
            })
            .map(|symbol| symbol.symbol_name.clone())
            .collect::<Vec<_>>();
        let section_heading = if raw_section.kind == "lead" {
            None
        } else {
            Some(raw_section.heading.clone())
        };
        let summary_text = make_summary_text(&raw_section.text, 260);
        let semantic_text = build_section_semantic_text(
            &input.page_title,
            raw_section,
            &summary_text,
            &section_symbol_names,
            &section_link_titles,
        );
        sections.push(ParsedDocsSection {
            section_index: index,
            heading: raw_section.heading.clone(),
            section_heading,
            heading_path: raw_section.heading_path.clone(),
            section_level: raw_section.level,
            section_kind: raw_section.kind.clone(),
            summary_text,
            section_text: raw_section.text.clone(),
            semantic_text,
            symbol_names: section_symbol_names,
            link_titles: section_link_titles,
            token_estimate: estimate_token_count(&raw_section.text),
        });
    }

    let mut examples = Vec::new();
    for raw_section in &raw_sections {
        examples.extend(extract_examples_for_section(&input.page_title, raw_section));
    }
    for (index, example) in examples.iter_mut().enumerate() {
        example.example_index = index;
    }

    let mut alias_titles = build_page_aliases(&input.page_title);
    for symbol in &symbols {
        alias_titles.extend(symbol.aliases.clone());
    }
    dedupe_strings(&mut alias_titles);

    let symbol_names = symbols
        .iter()
        .map(|symbol| symbol.symbol_name.clone())
        .collect::<Vec<_>>();
    let lead_text = raw_sections
        .first()
        .map(|section| section.text.clone())
        .unwrap_or_default();
    let headings_text = raw_sections
        .iter()
        .skip(1)
        .map(|section| section.heading_path.clone())
        .collect::<Vec<_>>()
        .join(" | ");
    let normalized_content = collapse_whitespace(&input.content);
    let summary_text = sections
        .iter()
        .find(|section| !section.summary_text.is_empty())
        .map(|section| section.summary_text.clone())
        .unwrap_or_else(|| make_summary_text(&input.content, 260));
    let semantic_text = build_semantic_text(
        &input.page_title,
        &page_kind,
        &summary_text,
        &headings_text,
        &alias_titles,
        &symbol_names,
        &link_titles,
        &sections,
        &examples,
    );
    let links = build_page_links(&link_titles, &template_titles);
    let token_estimate = estimate_token_count(&lead_text);

    ParsedDocsPage {
        page_title: input.page_title,
        page_namespace,
        page_kind,
        local_path: input.local_path,
        source_revision_id: input.source_revision_id,
        source_parent_revision_id: input.source_parent_revision_id,
        source_timestamp: input.source_timestamp,
        summary_text,
        lead_text,
        headings_text,
        alias_titles,
        link_titles,
        template_titles,
        symbol_names,
        normalized_content,
        semantic_text,
        content: input.content,
        token_estimate,
        sections,
        symbols,
        examples,
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DocsPageParseInput, classify_docs_page_kind, is_translation_variant, parse_docs_page,
    };

    #[test]
    fn parse_docs_page_extracts_symbols_sections_and_examples() {
        let parsed = parse_docs_page(DocsPageParseInput {
            page_title: "Manual:Hooks/PageContentSave".to_string(),
            local_path: "docs/mediawiki/mw-1.44/hooks/Manual_Hooks_PageContentSave.wiki"
                .to_string(),
            content: "Lead intro.\n== Parameters ==\n<syntaxhighlight lang=\"php\">$hookContainer->run( 'PageContentSave' );</syntaxhighlight>\n== Related ==\nSee [[API:Edit]] and {{#if:foo|bar}}.".to_string(),
            source_revision_id: Some(1),
            source_parent_revision_id: Some(0),
            source_timestamp: Some("2026-01-01T00:00:00Z".to_string()),
        });

        assert_eq!(parsed.page_kind, "hook_page");
        assert!(
            parsed
                .sections
                .iter()
                .any(|section| section.heading == "Parameters")
        );
        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "PageContentSave")
        );
        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "#if")
        );
        assert!(
            parsed
                .examples
                .iter()
                .any(|example| example.language.as_deref() == Some("php"))
        );
        assert!(parsed.link_titles.iter().any(|title| title == "API:Edit"));
    }

    #[test]
    fn page_kind_classification_covers_core_mediawiki_surfaces() {
        assert_eq!(
            classify_docs_page_kind("Manual:$wgParserEnableLegacyMediaDOM"),
            "config_page"
        );
        assert_eq!(classify_docs_page_kind("Help:Tags"), "tag_reference");
        assert_eq!(
            classify_docs_page_kind("Extension:Scribunto/Lua reference manual"),
            "lua_reference"
        );
    }

    #[test]
    fn translation_variant_detection_skips_language_subpages_only() {
        assert!(is_translation_variant("Manual:Hooks/PageSave/en"));
        assert!(is_translation_variant("API:Edit/pt-br"));
        assert!(!is_translation_variant("API:Edit/Sample code 1"));
        assert!(!is_translation_variant(
            "Extension:Scribunto/Lua reference manual"
        ));
    }

    #[test]
    fn parse_docs_page_extracts_inline_config_symbols_without_promoting_local_php_vars() {
        let parsed = parse_docs_page(DocsPageParseInput {
            page_title: "Extension:TestExtension".to_string(),
            local_path: "docs/extensions/TestExtension/Extension_TestExtension.wiki".to_string(),
            content: "Configuration: $wgTestExtensionEnable = true.\n== Hooks ==\nHook parameters include $parser and $text.".to_string(),
            source_revision_id: None,
            source_parent_revision_id: None,
            source_timestamp: None,
        });

        assert!(
            parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "$wgTestExtensionEnable")
        );
        assert!(
            !parsed
                .symbols
                .iter()
                .any(|symbol| symbol.symbol_name == "$parser")
        );
    }
}
