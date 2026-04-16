//! Template-aware rendering for the markdown exporter.
//!
//! Goal: produce an information-equivalent markdown representation of what the source
//! MediaWiki page would render, not a visual reproduction. Known templates get
//! structural flattening (infoboxes -> definition lists, cvt/lang/small -> inline text,
//! maintenance banners -> dropped). Unknown templates fall back to fenced wikitext so
//! the source is never silently discarded.
//!
//! The parser reuses crate-wide depth-aware segment helpers from
//! `content_store::parsing` (split_template_segments, split_once_top_level_equals).

use crate::content_store::parsing::{split_once_top_level_equals, split_template_segments};

/// Context signals whether a template was encountered inside flowing prose or as a
/// standalone block. The dispatcher uses this to decide between inline substitution
/// and block layout for the small number of templates whose rendering depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TemplateContext {
    Inline,
    Block,
}

/// Outcome of rendering a single template invocation.
pub(super) enum TemplateRendering {
    /// The template is a metadata/banner marker with no reader-facing content.
    Drop,
    /// Substitute inline at the template's position.
    Inline(String),
    /// Emit as a stand-alone block (lines bracketed by blank lines by the caller).
    Block(String),
    /// Template is not in the known set; caller should fall back to fenced wikitext.
    Fenced,
}

/// A parsed template invocation. `name` is trimmed; params are preserved in source order
/// with original positional indexes so `{{t|a||c}}` stays disambiguated.
pub(super) struct ParsedTemplate {
    pub(super) name: String,
    pub(super) params: Vec<TemplateParam>,
}

pub(super) struct TemplateParam {
    pub(super) key: ParamKey,
    pub(super) value: String,
}

pub(super) enum ParamKey {
    Positional(usize),
    Named(String),
}

impl ParsedTemplate {
    pub(super) fn parse(inner: &str) -> Option<Self> {
        let segments = split_template_segments(inner);
        let mut iter = segments.into_iter();
        let name = iter.next()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        let mut params = Vec::new();
        let mut positional_index = 1usize;
        for segment in iter {
            if let Some((key, value)) = split_once_top_level_equals(&segment) {
                let key = key.trim().to_string();
                if !key.is_empty() {
                    params.push(TemplateParam {
                        key: ParamKey::Named(key),
                        value: value.trim().to_string(),
                    });
                    continue;
                }
            }
            params.push(TemplateParam {
                key: ParamKey::Positional(positional_index),
                value: segment.trim().to_string(),
            });
            positional_index += 1;
        }
        Some(Self { name, params })
    }

    pub(super) fn name_lc(&self) -> String {
        self.name.trim().to_ascii_lowercase().replace('_', " ")
    }

    pub(super) fn positional(&self, index: usize) -> Option<&str> {
        self.params.iter().find_map(|param| match &param.key {
            ParamKey::Positional(i) if *i == index => Some(param.value.as_str()),
            _ => None,
        })
    }
}

/// Prefixes of templates that are pure source-side metadata or banner markup — the
/// markdown export drops them entirely. Matches on the lowercased, underscore-to-space
/// normalized template name; uses starts_with so templated variants like
/// `{{pp-semi-indef}}` and `{{short description|...}}` are both caught.
const METADATA_TEMPLATE_PREFIXES: &[&str] = &[
    "short description",
    "shortdesc",
    "defaultsort:",
    "displaytitle:",
    "#seo:",
    "use dmy dates",
    "use mdy dates",
    "use british english",
    "use american english",
    "use oxford spelling",
    "use canadian english",
    "use australian english",
    "use indian english",
    "use new zealand english",
    "use south african english",
    "engvarb",
    "italic title",
    "no italic title",
    "lowercase title",
    "lowercase",
    "good article",
    "featured article",
    "featured list",
    "featured topic",
    "good topic",
    "former featured article",
    "protection padlock",
    "pp",
    "pp-",
    "pp semi",
    "pp move",
    "pp-semi",
    "pp-move",
    "pp-vandalism",
    "pp-blp",
    "pp-dispute",
    "about",
    "for",
    "for multi",
    "for-multi",
    "hatnote",
    "redirect",
    "redirect-multi",
    "redirect-several",
    "distinguish",
    "other uses",
    "other people",
    "other places",
    "other ships",
    "main article",
    "main",
    "see also",
    "further",
    "further information",
    "disambiguation",
    "disambiguation needed",
    "dmbox",
    "authority control",
    "taxonbar",
    "commons category",
    "commons",
    "wikisource",
    "wiktionary",
    "wikispecies",
    "sister project",
    "sister project links",
    "sisterlinks",
    "portal bar",
    "portal",
    "coord missing",
    "coord",
    "geobox coor",
    "#if:",
    "#ifeq:",
    "#switch:",
    "#expr:",
    "#ifexpr:",
    "template:documentation",
    "documentation",
    "template shortcut",
    "clear",
    "clearleft",
    "clear-left",
    "clear right",
    "clr",
    "-",
    "·",
];

pub(super) fn is_metadata_template_name(name_normalized: &str) -> bool {
    METADATA_TEMPLATE_PREFIXES
        .iter()
        .any(|prefix| name_normalized.starts_with(prefix))
}

/// Render a template invocation to its markdown equivalent.
///
/// `recurse` renders an arbitrary wikitext fragment (used for parameter values that may
/// themselves contain templates and links).
pub(super) fn render_template(
    template: &ParsedTemplate,
    context: TemplateContext,
    recurse: &mut dyn FnMut(&str) -> String,
) -> TemplateRendering {
    let name = template.name_lc();

    if is_metadata_template_name(&name) {
        return TemplateRendering::Drop;
    }

    // Inline textual templates: take a positional argument and emit it verbatim.
    if matches!(
        name.as_str(),
        "small" | "nobr" | "nowrap" | "nbs" | "nobold" | "noitalic"
    ) {
        let text = template.positional(1).unwrap_or("");
        return TemplateRendering::Inline(recurse(text));
    }

    // `lang|code|text` / `lang-xx|text`: emit text only, drop the language tag — prose
    // usually names the language already, and an agent does not need the BCP-47 tag.
    if name == "lang" || name == "langx" || name == "wikt-lang" {
        let text = template.positional(2).unwrap_or("").trim();
        let text = if text.is_empty() {
            template.positional(1).unwrap_or("")
        } else {
            text
        };
        return TemplateRendering::Inline(recurse(text));
    }
    if name.starts_with("lang-") {
        let text = template.positional(1).unwrap_or("");
        return TemplateRendering::Inline(recurse(text));
    }

    // `ill|Local|lang|Foreign` and interwiki variants: agent sees the local display text.
    if name == "ill" || name == "interlanguage link" || name == "interlanguage link multi" {
        let text = template.positional(1).unwrap_or("");
        return TemplateRendering::Inline(recurse(text));
    }

    // Unit conversion. `{{cvt|93|km/h|mph}}` — emit the first value and the first unit
    // only; we do not have a units engine and inventing converted values would be wrong.
    if name == "cvt" || name == "convert" {
        return TemplateRendering::Inline(render_convert(template));
    }

    // Citation templates: preserve as inline wikitext so that downstream export of the
    // parent page's <ref> block keeps the citation contract intact. Rendering a Cite
    // template visually would drop structured provenance (access-date, archive-url, etc.)
    // that an agent may later surface.
    if name.starts_with("cite") || name == "citation" {
        return TemplateRendering::Inline(format_raw_template(template));
    }

    // Abbreviation / tooltip templates: emit the visible text.
    if matches!(name.as_str(), "abbr" | "tooltip") {
        let text = template.positional(1).unwrap_or("");
        return TemplateRendering::Inline(recurse(text));
    }

    // Non-breaking space helpers.
    if matches!(name.as_str(), "nbsp" | "spaces") {
        return TemplateRendering::Inline(" ".to_string());
    }
    if name == "spaced ndash" || name == "snd" {
        return TemplateRendering::Inline(" \u{2013} ".to_string());
    }

    // Collapsible list: when block-level, emit as a markdown bullet list; when inline,
    // join with commas.
    if name == "collapsible list" || name == "flatlist" || name == "plainlist" {
        return render_bullet_list(template, context, recurse);
    }

    // `hlist`: semicolon-separated inline list.
    if name == "hlist" || name == "hlist-comma" {
        return TemplateRendering::Inline(render_inline_list(template, " \u{00b7} ", recurse));
    }

    // Infobox family: render as definition list block.
    if is_infobox_like(&name) {
        return TemplateRendering::Block(render_infobox(template, recurse));
    }

    // Clade and phylogeny templates: preserve as fenced wikitext — the nested structure
    // is information-bearing and we do not want to flatten it to unstructured text.
    if name == "clade" || name == "cladogram" || name == "clade-link" {
        return TemplateRendering::Fenced;
    }

    // Plain `main` links (hatnotes covered in metadata list already).

    TemplateRendering::Fenced
}

fn is_infobox_like(name_normalized: &str) -> bool {
    name_normalized.starts_with("infobox")
        || matches!(
            name_normalized,
            "speciesbox"
                | "automatic taxobox"
                | "taxobox"
                | "subspeciesbox"
                | "paraphyletic group"
                | "fossilrange"
                | "persondata"
                | "chembox"
                | "drugbox"
        )
}

fn render_convert(template: &ParsedTemplate) -> String {
    // Forms handled:
    //   {{cvt|93|km/h}}                   -> "93 km/h"
    //   {{cvt|93|km/h|mph|}}              -> "93 km/h" (output-unit conversion dropped)
    //   {{cvt|67|-|94|cm}}                -> "67–94 cm"
    //   {{cvt|1.1|and|1.5|m}}             -> "1.1 and 1.5 m"
    //   {{cvt|30|-|200|m|ft}}             -> "30–200 m"
    //   {{cvt|X}}                         -> "X"
    // We never invent converted target-unit values because we have no units engine;
    // dropping the target-unit argument is more honest than guessing.
    let p1 = template.positional(1).unwrap_or("").trim();
    let p2 = template.positional(2).unwrap_or("").trim();
    let p3 = template.positional(3).unwrap_or("").trim();
    let p4 = template.positional(4).unwrap_or("").trim();

    if matches!(p2, "-" | "\u{2013}" | "to") && !p3.is_empty() && !p4.is_empty() {
        return format!("{p1}\u{2013}{p3} {p4}");
    }
    if matches!(p2, "and" | "or" | "by" | "x" | "\u{00d7}") && !p3.is_empty() && !p4.is_empty() {
        return format!("{p1} {p2} {p3} {p4}");
    }
    if p2.is_empty() {
        return p1.to_string();
    }
    format!("{p1} {p2}")
}

fn render_bullet_list(
    template: &ParsedTemplate,
    context: TemplateContext,
    recurse: &mut dyn FnMut(&str) -> String,
) -> TemplateRendering {
    let items: Vec<String> = template
        .params
        .iter()
        .filter_map(|param| match &param.key {
            ParamKey::Positional(_) => Some(param.value.trim().to_string()),
            ParamKey::Named(_) => None,
        })
        .filter(|item| !item.is_empty())
        .collect();
    if items.is_empty() {
        return TemplateRendering::Drop;
    }
    match context {
        TemplateContext::Block => {
            let mut lines = Vec::with_capacity(items.len());
            for item in &items {
                let rendered = recurse(item);
                let first_line = rendered.lines().next().unwrap_or("").trim();
                let remainder: Vec<&str> = rendered.lines().skip(1).collect();
                let mut entry = format!("- {first_line}");
                for extra in remainder {
                    entry.push_str("\n  ");
                    entry.push_str(extra.trim_end());
                }
                lines.push(entry);
            }
            TemplateRendering::Block(lines.join("\n"))
        }
        TemplateContext::Inline => TemplateRendering::Inline(render_inline_list_values(
            &items, ", ", recurse,
        )),
    }
}

fn render_inline_list(
    template: &ParsedTemplate,
    separator: &str,
    recurse: &mut dyn FnMut(&str) -> String,
) -> String {
    let items: Vec<String> = template
        .params
        .iter()
        .filter_map(|param| match &param.key {
            ParamKey::Positional(_) => Some(param.value.trim().to_string()),
            ParamKey::Named(_) => None,
        })
        .filter(|item| !item.is_empty())
        .collect();
    render_inline_list_values(&items, separator, recurse)
}

fn render_inline_list_values(
    items: &[String],
    separator: &str,
    recurse: &mut dyn FnMut(&str) -> String,
) -> String {
    items
        .iter()
        .map(|item| recurse(item))
        .collect::<Vec<_>>()
        .join(separator)
}

fn render_infobox(template: &ParsedTemplate, recurse: &mut dyn FnMut(&str) -> String) -> String {
    // Render as a labeled definition list. Named parameters become entries; unnamed
    // positional parameters are preserved as numbered entries because their semantics
    // are template-specific and we do not want to silently discard them.
    let mut lines = Vec::new();
    let display_name = template.name.trim();
    lines.push(format!("**{display_name}**"));
    for param in &template.params {
        let rendered_value = recurse(param.value.trim());
        let trimmed = rendered_value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let label = match &param.key {
            ParamKey::Named(name) => name.clone(),
            ParamKey::Positional(index) => format!("${index}"),
        };
        let (first, rest) = split_first_line(trimmed);
        let mut entry = format!("- **{label}:** {first}");
        for extra in rest {
            entry.push_str("\n  ");
            entry.push_str(extra);
        }
        lines.push(entry);
    }
    lines.join("\n")
}

fn split_first_line(value: &str) -> (&str, Vec<&str>) {
    let mut iter = value.split('\n');
    let first = iter.next().unwrap_or("").trim_end();
    let rest: Vec<&str> = iter.map(str::trim_end).collect();
    (first, rest)
}

fn format_raw_template(template: &ParsedTemplate) -> String {
    let mut out = String::from("{{");
    out.push_str(&template.name);
    for param in &template.params {
        out.push('|');
        match &param.key {
            ParamKey::Positional(_) => {}
            ParamKey::Named(name) => {
                out.push_str(name);
                out.push('=');
            }
        }
        out.push_str(&param.value);
    }
    out.push_str("}}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recurse_identity(input: &str) -> String {
        input.to_string()
    }

    fn render(inner: &str, context: TemplateContext) -> TemplateRendering {
        let template = ParsedTemplate::parse(inner).expect("template parses");
        render_template(&template, context, &mut recurse_identity)
    }

    #[test]
    fn drops_metadata_templates() {
        for case in [
            "Short description|Example",
            "shortdesc|Example",
            "Use British English|date=May 2020",
            "Good article",
            "Protection padlock|small=yes",
            "About|the animal||Cheetah (disambiguation)",
            "Hatnote|other",
            "Authority control",
            "Taxonbar|from=Q123",
            "DEFAULTSORT:Cheetah",
            "italic title",
        ] {
            assert!(
                matches!(render(case, TemplateContext::Block), TemplateRendering::Drop),
                "metadata template `{case}` should drop"
            );
        }
    }

    #[test]
    fn small_and_lang_strip_to_text() {
        assert!(matches!(
            render("small|(Schreber, 1775)", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "(Schreber, 1775)"
        ));
        assert!(matches!(
            render("lang|hi-Latn|ćītā", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "ćītā"
        ));
        assert!(matches!(
            render("langx|ur|چیتا", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "چیتا"
        ));
        assert!(matches!(
            render("lang-en|example", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "example"
        ));
    }

    #[test]
    fn convert_renders_first_value_and_unit() {
        assert!(matches!(
            render("cvt|93|km/h|mph", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "93 km/h"
        ));
        assert!(matches!(
            render("convert|67|-|94|cm|ft", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "67\u{2013}94 cm"
        ));
        assert!(matches!(
            render("cvt|100|km/h", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "100 km/h"
        ));
        assert!(matches!(
            render("cvt|1.1|and|1.5|m", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "1.1 and 1.5 m"
        ));
        assert!(matches!(
            render("cvt|21|and|65|kg", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "21 and 65 kg"
        ));
        assert!(matches!(
            render("cvt|10|x|20|m", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "10 x 20 m"
        ));
    }

    #[test]
    fn ill_emits_local_display_text() {
        assert!(matches!(
            render("ill|Hilzheimer|de|Max Hilzheimer", TemplateContext::Inline),
            TemplateRendering::Inline(ref s) if s == "Hilzheimer"
        ));
    }

    #[test]
    fn infobox_renders_definition_list() {
        let rendered = render(
            "Speciesbox\n| name = Cheetah\n| status = VU\n| authority = Schreber, 1775",
            TemplateContext::Block,
        );
        match rendered {
            TemplateRendering::Block(body) => {
                assert!(body.starts_with("**Speciesbox**"));
                assert!(body.contains("- **name:** Cheetah"));
                assert!(body.contains("- **status:** VU"));
                assert!(body.contains("- **authority:** Schreber, 1775"));
            }
            _ => panic!("infobox should render as block"),
        }
    }

    #[test]
    fn collapsible_list_block_renders_bullets_and_inline_joins() {
        let block = render(
            "collapsible list|Alpha|Beta|Gamma",
            TemplateContext::Block,
        );
        match block {
            TemplateRendering::Block(body) => {
                assert_eq!(body, "- Alpha\n- Beta\n- Gamma");
            }
            _ => panic!("block collapsible list should be a Block"),
        }
        let inline = render("collapsible list|Alpha|Beta", TemplateContext::Inline);
        match inline {
            TemplateRendering::Inline(body) => assert_eq!(body, "Alpha, Beta"),
            _ => panic!("inline collapsible list should be Inline"),
        }
    }

    #[test]
    fn cite_template_preserved_verbatim() {
        let rendered = render(
            "cite web|url=https://example.com|title=Example",
            TemplateContext::Inline,
        );
        match rendered {
            TemplateRendering::Inline(body) => {
                assert_eq!(body, "{{cite web|url=https://example.com|title=Example}}");
            }
            _ => panic!("cite template should stay inline verbatim"),
        }
    }

    #[test]
    fn unknown_template_requests_fenced_fallback() {
        assert!(matches!(
            render("WeirdUnknownTemplate|1=alpha", TemplateContext::Block),
            TemplateRendering::Fenced
        ));
    }

    #[test]
    fn parses_empty_positional_slots() {
        let template =
            ParsedTemplate::parse("About|the animal||Cheetah (disambiguation)").expect("parses");
        assert_eq!(template.name, "About");
        assert_eq!(template.positional(1), Some("the animal"));
        assert_eq!(template.positional(2), Some(""));
        assert_eq!(template.positional(3), Some("Cheetah (disambiguation)"));
    }
}
