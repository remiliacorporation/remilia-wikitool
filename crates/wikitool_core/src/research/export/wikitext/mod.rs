use super::super::template_render::{
    ParsedTemplate, TemplateContext, TemplateRendering, render_template,
};
use super::normalize_markdown;

pub fn wikitext_to_markdown(content: &str, _code_language: Option<&str>) -> String {
    let mut renderer = WikitextMarkdownRenderer::default();
    renderer.render(content)
}

mod helpers;
mod inline;
pub(crate) mod lint;
mod segments;

use helpers::{
    append_agent_sections, format_reference_entry, push_blank_separator, push_fenced_wikitext,
};
use inline::{
    convert_heading, convert_inline_wikitext, convert_list_prefix, convert_redirect_line,
    extract_category_link, extract_media_link, index_of_ignore_case, line_starts_redirect,
    parse_ref_name, strip_html_comments,
};
use segments::{
    Segment, classify_template_context, segment_wikitext,
    split_prose_lines_preserving_opaque_blocks,
};

#[derive(Default)]
struct WikitextMarkdownRenderer {
    references: Vec<String>,
    categories: Vec<String>,
    media: Vec<String>,
    table_buffer: Vec<String>,
    in_table: bool,
}

impl WikitextMarkdownRenderer {
    fn render(&mut self, content: &str) -> String {
        let cleaned = strip_html_comments(content);
        let body = self.render_fragment(&cleaned);
        let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
        append_agent_sections(&mut lines, &self.media, &self.categories, &self.references);
        normalize_markdown(&lines.join("\n"))
    }

    /// Segment-aware render of an arbitrary wikitext fragment. Used both for the top
    /// level and recursively for template parameter values. State (references,
    /// categories, media, table buffer) is shared across recursive calls so that a
    /// `<ref>` nested inside an infobox parameter is still lifted into the document's
    /// reference section.
    ///
    /// Logical wikitext lines can span multiple segments (an inline template inside a
    /// paragraph is one example). The renderer accumulates the current in-progress
    /// line across segments and only finalizes it when a newline is encountered in a
    /// prose segment or when a block-level template/extension forces a flush.
    fn render_fragment(&mut self, content: &str) -> String {
        let segments = segment_wikitext(content);
        let mut output_lines: Vec<String> = Vec::new();
        let mut current_line = String::new();

        for index in 0..segments.len() {
            match segments[index] {
                Segment::Prose(text) => {
                    let parts = split_prose_lines_preserving_opaque_blocks(text);
                    let mut parts = parts.into_iter();
                    if let Some(first) = parts.next() {
                        current_line.push_str(first);
                    }
                    for part in parts {
                        let completed = std::mem::take(&mut current_line);
                        self.finalize_prose_line(&completed, &mut output_lines);
                        current_line.push_str(part);
                    }
                }
                Segment::Template { inner } => {
                    let context = classify_template_context(&segments, index);
                    let rendering = self.render_template_invocation(inner, context);
                    match rendering {
                        TemplateRendering::Drop => {}
                        TemplateRendering::Inline(text) => current_line.push_str(&text),
                        TemplateRendering::Block(body) => {
                            self.flush_pending_line(&mut current_line, &mut output_lines);
                            push_blank_separator(&mut output_lines);
                            for line in body.lines() {
                                output_lines.push(line.to_string());
                            }
                            output_lines.push(String::new());
                        }
                        TemplateRendering::Fenced => {
                            if line_starts_redirect(&current_line) {
                                continue;
                            }
                            self.flush_pending_line(&mut current_line, &mut output_lines);
                            push_blank_separator(&mut output_lines);
                            push_fenced_wikitext(&mut output_lines, &format!("{{{{{inner}}}}}"));
                        }
                    }
                }
                Segment::ExtensionBlock { raw } => {
                    self.flush_pending_line(&mut current_line, &mut output_lines);
                    push_blank_separator(&mut output_lines);
                    push_fenced_wikitext(&mut output_lines, raw);
                }
            }
        }
        self.flush_pending_line(&mut current_line, &mut output_lines);
        output_lines.join("\n")
    }

    fn flush_pending_line(&mut self, current_line: &mut String, output: &mut Vec<String>) {
        if current_line.is_empty() {
            return;
        }
        let completed = std::mem::take(current_line);
        self.finalize_prose_line(&completed, output);
    }

    fn finalize_prose_line(&mut self, line: &str, output: &mut Vec<String>) {
        if self.in_table {
            self.table_buffer.push(line.to_string());
            if line.trim_start().starts_with("|}") {
                let table = self.flush_table();
                output.push(table);
            }
            return;
        }
        if line.trim_start().starts_with("{|") {
            self.in_table = true;
            self.table_buffer.clear();
            self.table_buffer.push(line.to_string());
            return;
        }
        let trimmed = line.trim();
        if let Some(redirect) = convert_redirect_line(trimmed) {
            output.push(redirect);
            return;
        }
        if let Some(category) = extract_category_link(trimmed) {
            self.categories.push(category);
            return;
        }
        if let Some(media) = extract_media_link(trimmed) {
            self.media.push(media);
            return;
        }
        let converted = convert_heading(line).unwrap_or_else(|| {
            let line_with_list = convert_list_prefix(line);
            let line_with_refs = self.convert_refs(&line_with_list);
            convert_inline_wikitext(&line_with_refs)
        });
        output.push(converted);
    }

    fn render_template_invocation(
        &mut self,
        inner: &str,
        context: TemplateContext,
    ) -> TemplateRendering {
        let Some(template) = ParsedTemplate::parse(inner) else {
            return TemplateRendering::Fenced;
        };
        let mut recurse = |fragment: &str| self.render_fragment(fragment);
        render_template(&template, context, &mut recurse)
    }

    fn convert_refs(&mut self, line: &str) -> String {
        let mut output = String::new();
        let mut index = 0usize;
        while index < line.len() {
            let Some(start_offset) = index_of_ignore_case(line, "<ref", index) else {
                output.push_str(&line[index..]);
                break;
            };
            output.push_str(&line[index..start_offset]);
            let Some(open_end_offset) = line[start_offset..].find('>') else {
                output.push_str(&line[start_offset..]);
                break;
            };
            let open_end = start_offset + open_end_offset;
            let open_tag = &line[start_offset..=open_end];
            let self_closing = open_tag.trim_end().ends_with("/>");
            let name = parse_ref_name(open_tag);
            if self_closing {
                let marker = name.unwrap_or_else(|| format!("ref-{}", self.references.len() + 1));
                output.push_str(&format!("[^{marker}]"));
                index = open_end + 1;
                continue;
            }
            let Some(close_start) = index_of_ignore_case(line, "</ref>", open_end + 1) else {
                output.push_str(&line[start_offset..]);
                break;
            };
            let raw_ref = line[open_end + 1..close_start].trim();
            let marker = name.unwrap_or_else(|| format!("ref-{}", self.references.len() + 1));
            if !raw_ref.is_empty()
                && !self
                    .references
                    .iter()
                    .any(|entry| entry.starts_with(&format!("[^{marker}]:")))
            {
                let ref_text = convert_inline_wikitext(raw_ref);
                self.references
                    .push(format_reference_entry(&marker, &ref_text));
            }
            output.push_str(&format!("[^{marker}]"));
            index = close_start + "</ref>".len();
        }
        output
    }

    fn flush_table(&mut self) -> String {
        self.in_table = false;
        let table = self.table_buffer.join("\n");
        self.table_buffer.clear();
        format!("```wikitext\n{table}\n```")
    }
}

#[cfg(test)]
mod tests {
    use super::wikitext_to_markdown;

    #[test]
    fn wikitext_to_markdown_extracts_agent_sections() {
        let markdown = wikitext_to_markdown(
            r#"
{{Short description|Example}}
'''Milady''' is linked to [[Remilia Corporation|Remilia]].<ref name="site">{{cite web|url=https://example.com|title=Example}}</ref>

== Gallery ==
[[File:Milady.png|thumb|Milady portrait]]

{| class="wikitable"
|-
! A !! B
|-
| 1 || 2
|}

[[Category:Remilia]]
"#,
            None,
        );

        assert!(
            markdown
                .contains("**Milady** is linked to [Remilia](wiki://Remilia Corporation).[^site]")
        );
        assert!(markdown.contains("```wikitext"));
        assert!(markdown.contains("## Media"));
        assert!(markdown.contains("- Milady.png - Milady portrait"));
        assert!(markdown.contains("## Categories"));
        assert!(markdown.contains("- Remilia"));
        assert!(markdown.contains("## References"));
        assert!(markdown.contains("[^site]: {{cite web|url=https://example.com|title=Example}}"));
        assert!(!markdown.contains("Short description"));
    }

    #[test]
    fn wikitext_to_markdown_converts_lists_and_external_links() {
        let markdown = wikitext_to_markdown(
            "* [https://example.com Example]\n** [[Target|Label]]\n# Step",
            None,
        );

        assert!(markdown.contains("- [Example](https://example.com)"));
        assert!(markdown.contains("  - [Label](wiki://Target)"));
        assert!(markdown.contains("1. Step"));
    }

    #[test]
    fn wikitext_to_markdown_skips_metadata_blocks_and_converts_definition_lists() {
        let markdown = wikitext_to_markdown(
            r#"{{#seo:
|title=Hidden metadata
}}
; [[:Category:Things|Things]]
: Useful description with [[Target]].
; Term : Inline definition
"#,
            None,
        );

        assert!(!markdown.contains("#seo"));
        assert!(!markdown.contains("Hidden metadata"));
        assert!(markdown.contains("- **[Things](wiki://:Category:Things)**"));
        assert!(markdown.contains("  Useful description with [Target](wiki://Target)."));
        assert!(markdown.contains("- **Term:** Inline definition"));
    }

    #[test]
    fn wikitext_to_markdown_flattens_infobox_and_inline_templates() {
        let markdown = wikitext_to_markdown(
            r#"{{Short description|Fastest land mammal}}
{{Use British English|date=May 2020}}
{{Good article}}
{{Speciesbox
| name = Cheetah
| status = VU
| authority = ([[Johann Christian Daniel von Schreber|Schreber]], 1775)
}}

The '''cheetah''' reaches {{cvt|93|km/h|mph}} and is native to {{lang|en|Africa}} and central [[Iran]]. In {{small|(older texts)}} the species was called a "hunting leopard".
"#,
            None,
        );

        assert!(!markdown.contains("Short description"));
        assert!(!markdown.contains("Use British English"));
        assert!(!markdown.contains("Good article"));
        assert!(markdown.contains("**Speciesbox**"));
        assert!(markdown.contains("- **name:** Cheetah"));
        assert!(markdown.contains("- **status:** VU"));
        assert!(markdown.contains(
            "- **authority:** ([Schreber](wiki://Johann Christian Daniel von Schreber), 1775)"
        ));
        assert!(markdown.contains("reaches 93 km/h"));
        assert!(markdown.contains("native to Africa"));
        assert!(markdown.contains("In (older texts) the species"));
        assert!(!markdown.contains("{{cvt"));
        assert!(!markdown.contains("{{lang"));
        assert!(!markdown.contains("{{small"));
        assert!(markdown.contains("[Iran](wiki://Iran)"));
    }

    #[test]
    fn wikitext_to_markdown_preserves_unknown_templates_as_fenced_wikitext() {
        let markdown = wikitext_to_markdown(
            "Head.\n\n{{UnknownTemplate\n|kind = test\n|value = 42\n}}\n\nTail.\n",
            None,
        );
        assert!(markdown.contains("Head."));
        assert!(markdown.contains("```wikitext"));
        assert!(markdown.contains("{{UnknownTemplate"));
        assert!(markdown.contains("Tail."));
    }

    #[test]
    fn wikitext_to_markdown_preserves_parser_functions_as_fenced_wikitext() {
        let markdown = wikitext_to_markdown(
            "Lead.\n\n{{#if: condition | visible | hidden}}\n\nTail.",
            None,
        );

        assert!(markdown.contains("Lead."));
        assert!(markdown.contains("```wikitext"));
        assert!(markdown.contains("{{#if: condition | visible | hidden}}"));
        assert!(markdown.contains("Tail."));
    }

    #[test]
    fn wikitext_to_markdown_keeps_prose_after_inline_template_on_same_logical_line() {
        let markdown = wikitext_to_markdown(
            "The cheetah runs at {{cvt|93|km/h|mph|}}; it has powerful hindlimbs.",
            None,
        );
        assert!(
            markdown.contains("runs at 93 km/h; it has powerful hindlimbs."),
            "unexpected render: {markdown}"
        );
        assert!(!markdown.contains("- **it has"));
    }

    #[test]
    fn wikitext_to_markdown_rejects_cite_through_refs_verbatim() {
        let markdown = wikitext_to_markdown(
            "Claim A.<ref name=a>{{cite web|url=https://example.com/a|title=A}}</ref> Claim B.<ref name=b>{{cite news|title=B|url=https://example.com/b}}</ref>\n",
            None,
        );
        assert!(markdown.contains("Claim A.[^a]"));
        assert!(markdown.contains("Claim B.[^b]"));
        assert!(markdown.contains("[^a]: {{cite web|url=https://example.com/a|title=A}}"));
        assert!(markdown.contains("[^b]: {{cite news|title=B|url=https://example.com/b}}"));
    }

    #[test]
    fn wikitext_to_markdown_extracts_multiline_refs_as_single_footnotes() {
        let markdown = wikitext_to_markdown(
            "The cheetah evolved.<ref>{{cite web\n|url=https://example.com\n|title=Example\n}}</ref> It runs fast.",
            None,
        );

        assert!(markdown.contains("The cheetah evolved.[^ref-1] It runs fast."));
        assert!(markdown.contains("[^ref-1]: {{cite web"));
        assert!(markdown.contains("    |url=https://example.com"));
        assert!(markdown.contains("    |title=Example"));
        assert!(!markdown.contains("<ref>"));
    }

    #[test]
    fn wikitext_to_markdown_preserves_nowiki_literal_markup() {
        let markdown = wikitext_to_markdown(
            "Literal <nowiki>[[Not a link]] and {{not a template}}</nowiki> text.",
            None,
        );

        assert!(markdown.contains("Literal [[Not a link]] and {{not a template}} text."));
        assert!(!markdown.contains("wiki://Not a link"));
        assert!(!markdown.contains("```wikitext"));
    }

    #[test]
    fn wikitext_to_markdown_renders_redirects_explicitly() {
        let markdown = wikitext_to_markdown("#REDIRECT [[Target Page]] {{R from move}}", None);

        assert_eq!(markdown, "Redirect to [Target Page](wiki://Target Page)");
        assert!(!markdown.contains("1. REDIRECT"));
    }

    #[test]
    fn wikitext_to_markdown_fences_complex_extension_blocks() {
        let markdown = wikitext_to_markdown(
            "Lead.\n\n<gallery>\nFile:Example.jpg|Caption\n</gallery>\n\nTail.",
            None,
        );

        assert!(markdown.contains("Lead."));
        assert!(
            markdown.contains("```wikitext\n<gallery>\nFile:Example.jpg|Caption\n</gallery>\n```")
        );
        assert!(markdown.contains("Tail."));
    }

    #[test]
    fn wikitext_to_markdown_fences_syntax_and_math_blocks() {
        let markdown = wikitext_to_markdown(
            "<syntaxhighlight lang=\"rust\">\nfn main() {}\n</syntaxhighlight>\n\n<math>E=mc^2</math>",
            None,
        );

        assert!(markdown.contains(
            "```wikitext\n<syntaxhighlight lang=\"rust\">\nfn main() {}\n</syntaxhighlight>\n```"
        ));
        assert!(markdown.contains("```wikitext\n<math>E=mc^2</math>\n```"));
    }

    #[test]
    fn wikitext_to_markdown_does_not_split_templates_inside_wikilinks() {
        let markdown = wikitext_to_markdown(
            "[[File:Icon.svg|alt=Icon|{{dir|en|left|right}}|125x125px]]",
            None,
        );

        assert!(markdown.contains("## Media"));
        assert!(markdown.contains("- Icon.svg - {{dir|en|left|right}}"));
        assert!(!markdown.contains("```wikitext"));
    }
}
