#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use percent_encoding::percent_decode_str;
use scraper::{ElementRef, Html, Node, Selector};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LinkPolicy {
    pub internal_route_prefix: String,
    pub preserve_fragments: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaPolicy {
    pub image_template: String,
    #[serde(default)]
    pub audio_template: Option<String>,
    pub max_audio_sources: usize,
    pub empty_alt_policy: EmptyAltPolicy,
    pub emit_dimensions: bool,
    pub non_image_media_policy: NonImageMediaPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmptyAltPolicy {
    Reject,
    Decorative,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NonImageMediaPolicy {
    Reject,
    ExternalLinks,
    TemplateAudio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InfoboxPolicy {
    pub source_table_class: String,
    pub template: String,
    pub default_type: String,
    pub unlabeled_field_label: String,
    pub max_custom_fields: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaReference {
    pub ordinal: Option<usize>,
    pub media_kind: Option<String>,
    pub owner_element: Option<String>,
    pub owner_ordinal: Option<usize>,
    pub element: Option<String>,
    pub attribute: Option<String>,
    pub candidate_index: Option<usize>,
    pub descriptor: Option<String>,
    pub source_url: String,
    pub alt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub content_type: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub headings: usize,
    pub paragraphs: usize,
    pub list_items: usize,
    pub tables: usize,
    pub table_rows: usize,
    pub table_cells: usize,
    pub preformatted_blocks: usize,
    pub internal_links: usize,
    pub external_links: usize,
    pub image_elements: usize,
    pub image_locators: usize,
    pub external_media_elements: usize,
    pub external_media_locators: usize,
    pub archived_audio_elements: usize,
    pub archived_audio_locators: usize,
    pub native_infoboxes: usize,
}

pub struct HtmlToWikitextInput<'a> {
    pub html: &'a str,
    pub canonical_title: &'a str,
    pub canonical_url: &'a str,
    pub media_scope: &'a str,
    pub link_policy: &'a LinkPolicy,
    pub media_policy: &'a MediaPolicy,
    pub infobox_policy: Option<&'a InfoboxPolicy>,
    pub images: &'a BTreeMap<String, MediaReference>,
    pub media_occurrences: Option<&'a [MediaReference]>,
}

pub struct HtmlToWikitextOutput {
    pub wikitext: String,
    pub coverage: Coverage,
    pub used_media: BTreeSet<String>,
    pub media_occurrences_consumed: usize,
}

pub fn convert(input: HtmlToWikitextInput<'_>) -> Result<HtmlToWikitextOutput> {
    let base_url = Url::parse(input.canonical_url).context("parse canonical article URL")?;
    let document = Html::parse_fragment(input.html);
    let mut renderer = Renderer {
        input,
        base_url,
        coverage: Coverage::default(),
        used_media: BTreeSet::new(),
        media_cursor: 0,
        image_owner_ordinal: 0,
        picture_owner_ordinal: 0,
        audio_owner_ordinal: 0,
    };
    let raw = renderer.render_children(document.root_element())?;
    let wikitext = normalize_document(&raw);
    ensure!(
        !wikitext.trim().is_empty(),
        "article HTML produced empty wikitext"
    );
    Ok(HtmlToWikitextOutput {
        wikitext,
        coverage: renderer.coverage,
        used_media: renderer.used_media,
        media_occurrences_consumed: renderer.media_cursor,
    })
}

struct Renderer<'a> {
    input: HtmlToWikitextInput<'a>,
    base_url: Url,
    coverage: Coverage,
    used_media: BTreeSet<String>,
    media_cursor: usize,
    image_owner_ordinal: usize,
    picture_owner_ordinal: usize,
    audio_owner_ordinal: usize,
}

impl Renderer<'_> {
    fn render_children(&mut self, element: ElementRef<'_>) -> Result<String> {
        let mut output = String::new();
        for child in element.children() {
            match child.value() {
                Node::Text(text) => output.push_str(&escape_text(text.text.as_ref())),
                Node::Element(_) => {
                    if let Some(child) = ElementRef::wrap(child) {
                        output.push_str(&self.render_element(child)?);
                    }
                }
                _ => {}
            }
        }
        Ok(output)
    }

    fn render_element(&mut self, element: ElementRef<'_>) -> Result<String> {
        if should_drop(element) {
            return Ok(String::new());
        }
        let name = element.value().name();
        match name {
            "html" | "body" | "main" | "article" | "section" | "div" | "span" | "figure"
            | "figcaption" | "details" | "summary" | "time" | "small" | "sub" | "sup" | "abbr"
            | "dfn" | "bdi" | "bdo" | "ruby" | "rt" | "rp" => self.render_children(element),
            "head" | "script" | "style" | "noscript" | "template" | "form" | "input" | "button"
            | "select" | "option" | "textarea" | "iframe" | "object" | "embed" => Ok(String::new()),
            "p" => {
                self.coverage.paragraphs += 1;
                Ok(block(&self.render_children(element)?))
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.coverage.headings += 1;
                let level = name[1..].parse::<usize>().unwrap_or(2).clamp(1, 6);
                let marker = "=".repeat(level);
                let body = one_line(&self.render_children(element)?);
                if body.is_empty() {
                    Ok(String::new())
                } else {
                    Ok(format!("\n{marker} {body} {marker}\n\n"))
                }
            }
            "strong" | "b" => wrap_inline("'''", &self.render_children(element)?),
            "em" | "i" | "cite" => wrap_inline("''", &self.render_children(element)?),
            "del" | "s" | "strike" => wrap_tag("s", &self.render_children(element)?),
            "u" | "ins" => wrap_tag("u", &self.render_children(element)?),
            "mark" => wrap_tag("mark", &self.render_children(element)?),
            "code" | "kbd" | "samp" | "var" => wrap_tag("code", &self.render_children(element)?),
            "blockquote" => {
                let body = normalize_document(&self.render_children(element)?);
                Ok(format!(
                    "\n<blockquote>\n{}\n</blockquote>\n\n",
                    body.trim_end()
                ))
            }
            "pre" => {
                self.coverage.preformatted_blocks += 1;
                let body = escape_preformatted(&element.text().collect::<String>());
                Ok(format!("\n<pre>{body}</pre>\n\n"))
            }
            "br" => Ok("\n".to_string()),
            "hr" => Ok("\n----\n\n".to_string()),
            "a" => self.render_link(element),
            "img" => {
                let ordinal = self.image_owner_ordinal;
                self.image_owner_ordinal += 1;
                self.render_image(element, "img", ordinal)
            }
            "ul" => self.render_list(element, "*", true),
            "ol" => self.render_list(element, "#", true),
            "dl" => self.render_definition_list(element),
            "li" | "dt" | "dd" => self.render_children(element),
            "table" => self.render_table(element),
            "thead" | "tbody" | "tfoot" | "tr" | "th" | "td" | "caption" | "colgroup" | "col" => {
                self.render_children(element)
            }
            "audio" => {
                let ordinal = self.audio_owner_ordinal;
                self.audio_owner_ordinal += 1;
                if self.input.media_occurrences.is_some()
                    && self.input.media_policy.non_image_media_policy
                        == NonImageMediaPolicy::TemplateAudio
                {
                    self.render_template_audio(element, ordinal)
                } else {
                    self.render_external_media(element)
                }
            }
            "video" => self.render_external_media(element),
            "picture" => {
                let ordinal = self.picture_owner_ordinal;
                self.picture_owner_ordinal += 1;
                self.render_picture(element, ordinal)
            }
            "source" | "track" => Ok(String::new()),
            "canvas" | "svg" | "math" => {
                bail!("unsupported retained structured element <{name}> in article HTML")
            }
            _ => self.render_children(element),
        }
    }

    fn render_link(&mut self, element: ElementRef<'_>) -> Result<String> {
        let image_selector = Selector::parse("img, picture").expect("static image selector");
        if element.select(&image_selector).next().is_some() {
            let mut images = String::new();
            for image in element.select(&image_selector) {
                if image.value().name() == "img"
                    && image
                        .ancestors()
                        .filter_map(ElementRef::wrap)
                        .any(|ancestor| ancestor.value().name() == "picture")
                {
                    continue;
                }
                images.push_str(&self.render_element(image)?);
            }
            return Ok(one_line(&images));
        }
        let body = one_line(&self.render_children(element)?);
        if body.is_empty() {
            return Ok(String::new());
        }
        let href = match element.value().attr("href") {
            Some(value) if !value.trim().is_empty() => value.trim(),
            _ => return Ok(body),
        };
        let resolved = if href.starts_with('#') {
            let mut current = self.base_url.clone();
            current.set_fragment(Some(href.trim_start_matches('#')));
            current
        } else {
            self.base_url
                .join(href)
                .with_context(|| format!("resolve article link {href}"))?
        };
        if same_origin(&self.base_url, &resolved) {
            let (title, fragment) = self.internal_title(&resolved)?;
            let mut target = format!(
                "{}/{}/{}",
                self.input.link_policy.internal_route_prefix, self.input.media_scope, title
            );
            if self.input.link_policy.preserve_fragments
                && let Some(fragment) = fragment
                && !fragment.is_empty()
            {
                target.push('#');
                target.push_str(&fragment);
            }
            validate_wikilink_target(&target)?;
            self.coverage.internal_links += 1;
            Ok(format!("[[{target}|{body}]]"))
        } else {
            ensure!(
                matches!(resolved.scheme(), "http" | "https"),
                "external article link uses unsupported scheme {}",
                resolved.scheme()
            );
            let locator = resolved.as_str();
            ensure!(
                !locator.contains([']', '\n', '\r', ' ']),
                "external article link cannot be represented safely"
            );
            self.coverage.external_links += 1;
            Ok(format!("[{locator} {body}]"))
        }
    }

    fn internal_title(&self, resolved: &Url) -> Result<(String, Option<String>)> {
        let query_title = resolved
            .query_pairs()
            .find(|(key, _)| key == "title")
            .map(|(_, value)| value.into_owned());
        let raw_title = if let Some(title) = query_title {
            title
        } else if resolved.path() == self.base_url.path() {
            self.input.canonical_title.to_string()
        } else {
            let path = resolved.path().trim_start_matches('/');
            let path = path.strip_prefix("wiki/").unwrap_or(path);
            percent_decode_str(path)
                .decode_utf8()
                .context("decode internal MediaWiki title")?
                .into_owned()
        };
        let title = raw_title.replace('_', " ").trim().to_string();
        ensure!(
            !title.is_empty(),
            "internal MediaWiki link omitted a page title"
        );
        validate_title_component(&title)?;
        let fragment = resolved
            .fragment()
            .map(|value| {
                percent_decode_str(value)
                    .decode_utf8()
                    .map(|value| value.into_owned())
                    .context("decode internal MediaWiki fragment")
            })
            .transpose()?;
        if let Some(fragment) = &fragment {
            validate_fragment(fragment)?;
        }
        Ok((title, fragment))
    }

    fn render_image(
        &mut self,
        element: ElementRef<'_>,
        owner_element: &str,
        owner_ordinal: usize,
    ) -> Result<String> {
        self.coverage.image_elements += 1;
        let mut locators = Vec::new();
        if let Some(src) = element.value().attr("src") {
            locators.push((normalized_http_url(src)?, None, None, 0_u64));
        }
        if let Some(srcset) = element.value().attr("srcset") {
            for (index, candidate) in srcset.split(',').enumerate() {
                let fields = candidate.split_whitespace().collect::<Vec<_>>();
                ensure!(
                    (1..=2).contains(&fields.len()),
                    "img srcset candidate has an unsupported shape"
                );
                let score = fields
                    .get(1)
                    .map(|descriptor| descriptor_score(descriptor, index))
                    .transpose()?
                    .unwrap_or((index + 1) as u64);
                locators.push((
                    normalized_http_url(fields[0])?,
                    Some(index),
                    fields.get(1).map(|value| (*value).to_string()),
                    score,
                ));
            }
        }
        ensure!(!locators.is_empty(), "img element omitted src and srcset");
        let mut selected: Option<(u64, MediaReference)> = None;
        for (locator, candidate_index, descriptor, score) in &locators {
            let media = if self.input.media_occurrences.is_some() {
                self.consume_v3_media(
                    "image",
                    owner_element,
                    owner_ordinal,
                    "img",
                    if candidate_index.is_some() {
                        "srcset"
                    } else {
                        "src"
                    },
                    *candidate_index,
                    descriptor.as_deref(),
                    locator,
                )?
            } else {
                self.input
                    .images
                    .get(locator)
                    .with_context(|| {
                        format!("article img locator is absent from images.json: {locator}")
                    })?
                    .clone()
            };
            self.used_media.insert(locator.clone());
            self.coverage.image_locators += 1;
            if selected
                .as_ref()
                .map(|(selected_score, _)| score >= selected_score)
                .unwrap_or(true)
            {
                selected = Some((*score, media));
            }
        }
        let (_, media) = selected.context("img element did not select one captured media row")?;
        let dom_alt = element
            .value()
            .attr("alt")
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let alt = if self.input.media_occurrences.is_some() {
            media.alt.clone().or(dom_alt)
        } else {
            dom_alt.or(media.alt.clone())
        }
        .unwrap_or_default();
        ensure!(
            !alt.trim().is_empty()
                || self.input.media_policy.empty_alt_policy == EmptyAltPolicy::Decorative,
            "captured image {} has no nonempty alt text and the contract rejects decorative images",
            media.source_url
        );
        let dom_width = positive_dimension(element.value().attr("width"));
        let dom_height = positive_dimension(element.value().attr("height"));
        let width = if self.input.media_occurrences.is_some() {
            media.width.or(dom_width)
        } else {
            dom_width.or(media.width)
        };
        let height = if self.input.media_occurrences.is_some() {
            media.height.or(dom_height)
        } else {
            dom_height.or(media.height)
        };
        self.image_invocation(&media, &alt, width, height)
    }

    fn render_picture(&mut self, element: ElementRef<'_>, owner_ordinal: usize) -> Result<String> {
        let selector = Selector::parse("source, img").expect("static picture media selector");
        let mut selected: Option<(u64, MediaReference)> = None;
        let mut fallback_image = None;
        let mut fallback_score = 0_u64;
        let mut saw_locator = false;
        for child in element.select(&selector) {
            let child_name = child.value().name();
            if child_name == "img" {
                self.coverage.image_elements += 1;
                fallback_image = Some(child);
            }
            if let Some(src) = child.value().attr("src") {
                let locator = normalized_http_url(src)?;
                let media = if self.input.media_occurrences.is_some() {
                    self.consume_v3_media(
                        "image",
                        "picture",
                        owner_ordinal,
                        child_name,
                        "src",
                        None,
                        None,
                        &locator,
                    )?
                } else {
                    self.input
                        .images
                        .get(&locator)
                        .with_context(|| {
                            format!("picture src is absent from images.json: {locator}")
                        })?
                        .clone()
                };
                saw_locator = true;
                self.coverage.image_locators += 1;
                self.used_media.insert(locator);
                fallback_score += 1;
                selected = Some((fallback_score, media));
            }
            if let Some(srcset) = child.value().attr("srcset") {
                for (index, candidate) in srcset.split(',').enumerate() {
                    let fields = candidate.split_whitespace().collect::<Vec<_>>();
                    ensure!(
                        (1..=2).contains(&fields.len()),
                        "picture srcset candidate has an unsupported shape"
                    );
                    let locator = normalized_http_url(fields[0])?;
                    let descriptor = fields.get(1).copied();
                    let score = descriptor
                        .map(|value| descriptor_score(value, index))
                        .transpose()?
                        .unwrap_or((index + 1) as u64);
                    let media = if self.input.media_occurrences.is_some() {
                        self.consume_v3_media(
                            "image",
                            "picture",
                            owner_ordinal,
                            child_name,
                            "srcset",
                            Some(index),
                            descriptor,
                            &locator,
                        )?
                    } else {
                        self.input
                            .images
                            .get(&locator)
                            .with_context(|| {
                                format!("picture srcset is absent from images.json: {locator}")
                            })?
                            .clone()
                    };
                    saw_locator = true;
                    self.coverage.image_locators += 1;
                    self.used_media.insert(locator);
                    if selected
                        .as_ref()
                        .map(|(selected_score, _)| score >= *selected_score)
                        .unwrap_or(true)
                    {
                        selected = Some((score, media));
                    }
                }
            }
        }
        ensure!(
            saw_locator,
            "picture element omitted captured image locators"
        );
        let image = fallback_image.context("picture element omitted fallback img")?;
        let (_, media) = selected.context("picture element did not select captured media")?;
        let alt = media
            .alt
            .clone()
            .or_else(|| image.value().attr("alt").map(ToOwned::to_owned))
            .unwrap_or_default();
        ensure!(
            !alt.trim().is_empty()
                || self.input.media_policy.empty_alt_policy == EmptyAltPolicy::Decorative,
            "captured picture has no nonempty alt text and the contract rejects decorative images"
        );
        self.image_invocation(
            &media,
            &alt,
            media
                .width
                .or_else(|| positive_dimension(image.value().attr("width"))),
            media
                .height
                .or_else(|| positive_dimension(image.value().attr("height"))),
        )
    }

    fn image_invocation(
        &self,
        media: &MediaReference,
        alt: &str,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<String> {
        let template = self
            .input
            .media_policy
            .image_template
            .strip_prefix("Template:")
            .unwrap_or(&self.input.media_policy.image_template);
        let mut invocation = format!(
            "{{{{{}|site={}|sha256={}|alt={}",
            template,
            escape_template_value(self.input.media_scope),
            media.sha256,
            escape_template_value(alt)
        );
        if alt.trim().is_empty()
            && self.input.media_policy.empty_alt_policy == EmptyAltPolicy::Decorative
        {
            invocation.push_str("|decorative=yes");
        }
        if self.input.media_policy.emit_dimensions {
            if let Some(width) = width {
                invocation.push_str(&format!("|width={width}"));
            }
            if let Some(height) = height {
                invocation.push_str(&format!("|height={height}"));
            }
        }
        invocation.push_str("}}");
        Ok(invocation)
    }

    fn render_template_audio(
        &mut self,
        element: ElementRef<'_>,
        owner_ordinal: usize,
    ) -> Result<String> {
        let mut sources = Vec::new();
        if let Some(src) = element.value().attr("src") {
            let locator = normalized_http_url(src)?;
            let descriptor = media_type_descriptor(element.value().attr("type"));
            let media = self.consume_v3_media(
                "audio",
                "audio",
                owner_ordinal,
                "audio",
                "src",
                None,
                descriptor.as_deref(),
                &locator,
            )?;
            sources.push((media, element.value().attr("type")));
        }
        let source_selector = Selector::parse("source").expect("static source selector");
        for (candidate_index, source) in element.select(&source_selector).enumerate() {
            let src = source
                .value()
                .attr("src")
                .context("retained audio source omitted src")?;
            let locator = normalized_http_url(src)?;
            let descriptor = media_type_descriptor(source.value().attr("type"));
            let media = self.consume_v3_media(
                "audio",
                "audio",
                owner_ordinal,
                "source",
                "src",
                Some(candidate_index),
                descriptor.as_deref(),
                &locator,
            )?;
            sources.push((media, source.value().attr("type")));
        }
        ensure!(
            !sources.is_empty(),
            "retained audio element omitted source locators"
        );
        ensure!(
            sources.len() <= self.input.media_policy.max_audio_sources,
            "retained audio has {} sources, exceeding the contract maximum {}",
            sources.len(),
            self.input.media_policy.max_audio_sources
        );
        let configured_template = self
            .input
            .media_policy
            .audio_template
            .as_deref()
            .context("preservation audio template is absent")?;
        let template = configured_template
            .strip_prefix("Template:")
            .unwrap_or(configured_template);
        let label = element
            .value()
            .attr("aria-label")
            .or_else(|| element.value().attr("title"))
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "Audio sample {} from {}",
                    owner_ordinal + 1,
                    self.input.canonical_title
                )
            });
        let mut invocation = format!(
            "{{{{{template}|site={}|label={}",
            escape_template_value(self.input.media_scope),
            escape_template_value(&label)
        );
        for (index, (media, declared_type)) in sources.iter().enumerate() {
            let source_type = audio_source_type(declared_type.as_deref(), &media.content_type)?;
            invocation.push_str(&format!(
                "|source{}_sha256={}|source{}_type={source_type}",
                index + 1,
                media.sha256,
                index + 1
            ));
            self.used_media.insert(media.source_url.clone());
            self.coverage.archived_audio_locators += 1;
        }
        invocation.push_str("}}");
        self.coverage.archived_audio_elements += 1;
        Ok(block(&invocation))
    }

    #[allow(clippy::too_many_arguments)]
    fn consume_v3_media(
        &mut self,
        media_kind: &str,
        owner_element: &str,
        owner_ordinal: usize,
        element: &str,
        attribute: &str,
        candidate_index: Option<usize>,
        descriptor: Option<&str>,
        source_url: &str,
    ) -> Result<MediaReference> {
        let occurrences = self
            .input
            .media_occurrences
            .context("v3 media occurrence inventory is absent")?;
        let row = occurrences.get(self.media_cursor).with_context(|| {
            format!(
                "article DOM has an unbound {media_kind} occurrence at ordinal {}",
                self.media_cursor
            )
        })?;
        ensure!(
            row.ordinal == Some(self.media_cursor),
            "v3 media ordinal drifted"
        );
        ensure!(
            row.media_kind.as_deref() == Some(media_kind)
                && row.owner_element.as_deref() == Some(owner_element)
                && row.owner_ordinal == Some(owner_ordinal)
                && row.element.as_deref() == Some(element)
                && row.attribute.as_deref() == Some(attribute)
                && row.candidate_index == candidate_index
                && row.descriptor.as_deref() == descriptor
                && row.source_url == source_url,
            "article DOM media occurrence {} differs from the ordered media inventory",
            self.media_cursor
        );
        self.media_cursor += 1;
        Ok(row.clone())
    }

    fn render_external_media(&mut self, element: ElementRef<'_>) -> Result<String> {
        ensure!(
            self.input.media_policy.non_image_media_policy == NonImageMediaPolicy::ExternalLinks,
            "retained <{}> media is not admitted by the projection contract",
            element.value().name()
        );
        self.coverage.external_media_elements += 1;
        let source_selector = Selector::parse("source").expect("static source selector");
        let mut locators = Vec::new();
        if let Some(src) = element.value().attr("src") {
            locators.push((src, element.value().attr("type")));
        }
        for source in element.select(&source_selector) {
            if let Some(src) = source.value().attr("src") {
                locators.push((src, source.value().attr("type")));
            }
        }
        ensure!(
            !locators.is_empty(),
            "retained <{}> element omitted source locators",
            element.value().name()
        );
        let mut unique = BTreeSet::new();
        let mut output = String::new();
        for (src, content_type) in locators {
            let locator = normalized_http_url(src)?;
            if !unique.insert(locator.clone()) {
                continue;
            }
            self.coverage.external_media_locators += 1;
            self.coverage.external_links += 1;
            let kind = element.value().name();
            let label = content_type
                .map(|value| format!("Source {kind} ({})", escape_text(value)))
                .unwrap_or_else(|| format!("Source {kind}"));
            output.push_str(&format!("* [{locator} {label}]\n"));
        }
        Ok(block(&output))
    }

    fn render_list(&mut self, element: ElementRef<'_>, prefix: &str, root: bool) -> Result<String> {
        let mut output = String::new();
        for child in element.children() {
            let Some(item) = ElementRef::wrap(child) else {
                continue;
            };
            if item.value().name() != "li" {
                continue;
            }
            self.coverage.list_items += 1;
            let mut body = String::new();
            let mut nested = Vec::new();
            for item_child in item.children() {
                match item_child.value() {
                    Node::Text(text) => body.push_str(&escape_text(text.text.as_ref())),
                    Node::Element(_) => {
                        let Some(item_element) = ElementRef::wrap(item_child) else {
                            continue;
                        };
                        match item_element.value().name() {
                            "ul" => nested.push(self.render_list(
                                item_element,
                                &format!("{prefix}*"),
                                false,
                            )?),
                            "ol" => nested.push(self.render_list(
                                item_element,
                                &format!("{prefix}#"),
                                false,
                            )?),
                            _ => body.push_str(&self.render_element(item_element)?),
                        }
                    }
                    _ => {}
                }
            }
            output.push_str(prefix);
            output.push(' ');
            output.push_str(&one_line(&body));
            output.push('\n');
            for value in nested {
                output.push_str(&value);
            }
        }
        if root { Ok(block(&output)) } else { Ok(output) }
    }

    fn render_definition_list(&mut self, element: ElementRef<'_>) -> Result<String> {
        let mut output = String::new();
        for child in element.children() {
            let Some(item) = ElementRef::wrap(child) else {
                continue;
            };
            let marker = match item.value().name() {
                "dt" => ';',
                "dd" => ':',
                _ => continue,
            };
            self.coverage.list_items += 1;
            output.push(marker);
            output.push(' ');
            output.push_str(&one_line(&self.render_children(item)?));
            output.push('\n');
        }
        Ok(block(&output))
    }

    fn render_table(&mut self, element: ElementRef<'_>) -> Result<String> {
        self.coverage.tables += 1;
        if let Some(policy) = self.input.infobox_policy
            && element
                .value()
                .attr("class")
                .map(|classes| {
                    classes
                        .split_ascii_whitespace()
                        .any(|class| class == policy.source_table_class)
                })
                .unwrap_or(false)
            && self.breakout_is_admissible(element, policy)
        {
            return self.render_breakout_infobox(element, policy);
        }
        let row_selector = Selector::parse("tr").expect("static tr selector");
        let cell_selector = Selector::parse("th, td").expect("static table-cell selector");
        let caption_selector = Selector::parse("caption").expect("static caption selector");
        let mut output = String::from("\n{| class=\"wikitable\"\n");
        if let Some(caption) = element.select(&caption_selector).next() {
            let body = one_line(&self.render_children(caption)?);
            if !body.is_empty() {
                output.push_str("|+ ");
                output.push_str(&body);
                output.push('\n');
            }
        }
        for row in element.select(&row_selector) {
            if row
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| ancestor.value().name() == "table")
                != Some(element)
            {
                continue;
            }
            self.coverage.table_rows += 1;
            output.push_str("|-\n");
            for cell in row.select(&cell_selector) {
                if cell
                    .ancestors()
                    .filter_map(ElementRef::wrap)
                    .find(|ancestor| matches!(ancestor.value().name(), "tr"))
                    != Some(row)
                {
                    continue;
                }
                self.coverage.table_cells += 1;
                let marker = if cell.value().name() == "th" {
                    '!'
                } else {
                    '|'
                };
                output.push(marker);
                let mut attributes = Vec::new();
                for name in ["colspan", "rowspan", "scope"] {
                    if let Some(value) = cell.value().attr(name)
                        && safe_table_attribute(value)
                    {
                        attributes.push(format!("{name}=\"{value}\""));
                    }
                }
                if !attributes.is_empty() {
                    output.push(' ');
                    output.push_str(&attributes.join(" "));
                    output.push_str(" |");
                }
                output.push(' ');
                output.push_str(&one_line(&self.render_children(cell)?));
                output.push('\n');
            }
        }
        output.push_str("|}\n\n");
        Ok(output)
    }

    fn breakout_is_admissible(&self, element: ElementRef<'_>, policy: &InfoboxPolicy) -> bool {
        let row_selector = Selector::parse("tr").expect("static tr selector");
        let cell_selector = Selector::parse("th, td").expect("static table-cell selector");
        let nested_table_selector = Selector::parse("table table").expect("static table selector");
        if element.select(&nested_table_selector).next().is_some() {
            return false;
        }
        let mut title_rows = 0_usize;
        let mut estimated_fields = 0_usize;
        let mut trailing_break_allowance = 0_usize;
        for row in element.select(&row_selector).filter(|row| {
            row.ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| ancestor.value().name() == "table")
                == Some(element)
        }) {
            let cells = row
                .select(&cell_selector)
                .filter(|cell| {
                    cell.ancestors()
                        .filter_map(ElementRef::wrap)
                        .find(|ancestor| ancestor.value().name() == "tr")
                        == Some(row)
                })
                .collect::<Vec<_>>();
            if cells.len() != 1 {
                return false;
            }
            let is_title = row
                .value()
                .attr("class")
                .map(|classes| {
                    classes
                        .split_ascii_whitespace()
                        .any(|class| class == "breakouttitle")
                })
                .unwrap_or(false);
            if is_title {
                title_rows += 1;
                if cells[0].value().name() != "th" {
                    return false;
                }
                continue;
            }
            let image_selector = Selector::parse("img").expect("static img selector");
            if cells[0].select(&image_selector).next().is_some()
                && cells[0].text().all(|text| text.trim().is_empty())
            {
                continue;
            }
            let br_selector = Selector::parse("br").expect("static br selector");
            let paragraph_selector = Selector::parse("p").expect("static paragraph selector");
            let breaks = cells[0].select(&br_selector).count();
            let paragraphs = cells[0].select(&paragraph_selector).count().max(1);
            estimated_fields += breaks + paragraphs;
            trailing_break_allowance += paragraphs;
        }
        title_rows == 1 && estimated_fields <= policy.max_custom_fields + trailing_break_allowance
    }

    fn render_breakout_infobox(
        &mut self,
        element: ElementRef<'_>,
        policy: &InfoboxPolicy,
    ) -> Result<String> {
        let row_selector = Selector::parse("tr").expect("static tr selector");
        let cell_selector = Selector::parse("th, td").expect("static table-cell selector");
        let image_selector = Selector::parse("img").expect("static img selector");
        let mut name = None;
        let mut image_content = None;
        let mut fields = Vec::new();
        let mut unlabeled_fields = Vec::new();
        for row in element.select(&row_selector).filter(|row| {
            row.ancestors()
                .filter_map(ElementRef::wrap)
                .find(|ancestor| ancestor.value().name() == "table")
                == Some(element)
        }) {
            let cell = row
                .select(&cell_selector)
                .find(|cell| {
                    cell.ancestors()
                        .filter_map(ElementRef::wrap)
                        .find(|ancestor| ancestor.value().name() == "tr")
                        == Some(row)
                })
                .context("admitted breakout row omitted its cell")?;
            self.coverage.table_rows += 1;
            self.coverage.table_cells += 1;
            let is_title = row
                .value()
                .attr("class")
                .map(|classes| {
                    classes
                        .split_ascii_whitespace()
                        .any(|class| class == "breakouttitle")
                })
                .unwrap_or(false);
            if is_title {
                name = Some(one_line(&self.render_children(cell)?));
                continue;
            }
            if cell.select(&image_selector).next().is_some()
                && cell.text().all(|text| text.trim().is_empty())
            {
                let rendered = one_line(&self.render_children(cell)?);
                ensure!(
                    rendered.starts_with("{{") && rendered.ends_with("}}"),
                    "admitted breakout image row produced non-template content"
                );
                image_content = Some(rendered);
                continue;
            }

            let rendered = self.render_children(cell)?;
            for line in rendered
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && *line != "----")
            {
                if let Some(rest) = line.strip_prefix("'''")
                    && let Some((label, data)) = rest.split_once("''':")
                    && !label.trim().is_empty()
                    && !data.trim().is_empty()
                {
                    fields.push((label.trim().to_string(), data.trim().to_string()));
                } else {
                    unlabeled_fields.push(line.to_string());
                }
            }
        }
        if !unlabeled_fields.is_empty() {
            fields.push((
                policy.unlabeled_field_label.clone(),
                unlabeled_fields.join("<br>"),
            ));
        }
        ensure!(
            fields.len() <= policy.max_custom_fields,
            "admitted breakout table produced too many custom fields"
        );
        let template = policy
            .template
            .strip_prefix("Template:")
            .unwrap_or(&policy.template);
        let mut output = format!(
            "\n{{{{{template}\n| name = {}\n| type = {}\n",
            name.context("admitted breakout table omitted its title")?,
            escape_template_value(&policy.default_type)
        );
        if let Some(image_content) = image_content {
            output.push_str("| image_content = ");
            output.push_str(&image_content);
            output.push('\n');
        }
        for (index, (label, data)) in fields.into_iter().enumerate() {
            output.push_str(&format!(
                "| label{} = {}\n| data{} = {}\n",
                index + 1,
                label,
                index + 1,
                data
            ));
        }
        output.push_str("}}\n\n");
        self.coverage.native_infoboxes += 1;
        Ok(output)
    }
}

fn should_drop(element: ElementRef<'_>) -> bool {
    let Some(classes) = element.value().attr("class") else {
        return false;
    };
    classes.split_ascii_whitespace().any(|class| {
        matches!(
            class,
            "mw-editsection" | "toc" | "toctitle" | "noprint" | "printfooter" | "mw-empty-elt"
        )
    })
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn descriptor_score(value: &str, fallback: usize) -> Result<u64> {
    if let Some(width) = value.strip_suffix('w') {
        return width
            .parse::<u64>()
            .context("srcset width descriptor is invalid");
    }
    if let Some(scale) = value.strip_suffix('x') {
        let scale = scale
            .parse::<f64>()
            .context("srcset density descriptor is invalid")?;
        ensure!(
            scale.is_finite() && scale > 0.0,
            "srcset density descriptor is invalid"
        );
        return Ok((scale * 1_000_000.0) as u64);
    }
    Ok((fallback + 1) as u64)
}

fn positive_dimension(value: Option<&str>) -> Option<u32> {
    value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
}

fn validate_title_component(value: &str) -> Result<()> {
    ensure!(
        value.len() <= 1024 && !value.contains(['[', ']', '{', '}', '|', '\n', '\r', '#']),
        "internal MediaWiki title cannot be represented safely"
    );
    Ok(())
}

fn validate_fragment(value: &str) -> Result<()> {
    ensure!(
        value.len() <= 1024 && !value.contains(['[', ']', '{', '}', '|', '\n', '\r']),
        "internal MediaWiki fragment cannot be represented safely"
    );
    Ok(())
}

fn validate_wikilink_target(value: &str) -> Result<()> {
    ensure!(
        !value.contains(['[', ']', '{', '}', '|', '\n', '\r']),
        "generated archive link target is unsafe"
    );
    Ok(())
}

fn safe_table_attribute(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn wrap_inline(marker: &str, body: &str) -> Result<String> {
    let body = one_line(body);
    if body.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{marker}{body}{marker}"))
    }
}

fn wrap_tag(tag: &str, body: &str) -> Result<String> {
    let body = one_line(body);
    if body.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("<{tag}>{body}</{tag}>"))
    }
}

fn block(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        String::new()
    } else {
        format!("\n{value}\n\n")
    }
}

fn one_line(value: &str) -> String {
    collapse_whitespace(value).trim().to_string()
}

fn collapse_whitespace(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut pending_space = false;
    let mut began_with_space = false;
    for character in value.chars() {
        if character.is_whitespace() {
            if output.is_empty() {
                began_with_space = true;
            }
            pending_space = true;
        } else {
            if pending_space && (!output.is_empty() || began_with_space) {
                output.push(' ');
            }
            pending_space = false;
            output.push(character);
        }
    }
    if pending_space && !output.ends_with(' ') {
        output.push(' ');
    }
    output
}

fn escape_text(value: &str) -> String {
    let collapsed = collapse_whitespace(value);
    let mut output = String::with_capacity(collapsed.len());
    let characters = collapsed.chars().collect::<Vec<_>>();
    let mut index = 0_usize;
    while index < characters.len() {
        let character = characters[index];
        if character == '\'' {
            let start = index;
            while index < characters.len() && characters[index] == '\'' {
                index += 1;
            }
            let length = index - start;
            if length == 1 {
                output.push('\'');
            } else {
                for _ in 0..length {
                    output.push_str("&#39;");
                }
            }
            continue;
        }
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '[' => output.push_str("&#91;"),
            ']' => output.push_str("&#93;"),
            '{' => output.push_str("&#123;"),
            '|' => output.push_str("&#124;"),
            '}' => output.push_str("&#125;"),
            _ => output.push(character),
        }
        index += 1;
    }
    output
}

fn escape_preformatted(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '[' => output.push_str("&#91;"),
            ']' => output.push_str("&#93;"),
            '{' => output.push_str("&#123;"),
            '|' => output.push_str("&#124;"),
            '}' => output.push_str("&#125;"),
            _ => output.push(character),
        }
    }
    output
}

fn escape_template_value(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '[' => output.push_str("&#91;"),
            ']' => output.push_str("&#93;"),
            '{' => output.push_str("&#123;"),
            '|' => output.push_str("&#124;"),
            '}' => output.push_str("&#125;"),
            '=' => output.push_str("&#61;"),
            '\n' | '\r' => output.push(' '),
            _ => output.push(character),
        }
    }
    output
}

fn normalize_document(value: &str) -> String {
    let mut output = String::new();
    let mut blank = false;
    let mut in_pre = false;
    for line in value.lines() {
        if in_pre || line.contains("<pre>") {
            output.push_str(line);
            output.push('\n');
            in_pre = !line.contains("</pre>");
            blank = false;
            continue;
        }
        let line = line.trim_end();
        if line.trim().is_empty() {
            if !output.is_empty() && !blank {
                output.push('\n');
                blank = true;
            }
            continue;
        }
        output.push_str(line.trim_start_matches(' '));
        output.push('\n');
        blank = false;
    }
    while output.starts_with('\n') {
        output.remove(0);
    }
    while output.ends_with("\n\n") {
        output.pop();
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn normalized_http_url(value: &str) -> Result<String> {
    let mut parsed = Url::parse(value).context("media source URL is invalid")?;
    ensure!(
        matches!(parsed.scheme(), "http" | "https") && parsed.host().is_some(),
        "media source URL must be absolute HTTP(S)"
    );
    ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "media source URL cannot contain credentials"
    );
    parsed.set_fragment(None);
    Ok(parsed.to_string())
}

fn audio_source_type(declared: Option<&str>, captured: &str) -> Result<&'static str> {
    let declared = declared.map(|value| value.trim().to_ascii_lowercase());
    let declared_type = match declared.as_deref() {
        Some("audio/mpeg") => Some("mp3"),
        Some(value) if value.starts_with("audio/ogg") => Some("ogg"),
        Some(value) => bail!("retained audio declares unsupported type {value}"),
        None => None,
    };
    let captured_type = match captured {
        "audio/mpeg" => "mp3",
        "audio/ogg" | "application/ogg" => "ogg",
        value => bail!("captured audio has unsupported content type {value}"),
    };
    if let Some(declared_type) = declared_type {
        ensure!(
            declared_type == captured_type,
            "retained audio declared type differs from captured content type"
        );
    }
    Ok(captured_type)
}

fn media_type_descriptor(value: Option<&str>) -> Option<String> {
    value.map(|value| value.trim().to_ascii_lowercase().replace('"', ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policies() -> (LinkPolicy, MediaPolicy) {
        (
            LinkPolicy {
                internal_route_prefix: "Special:Archive".to_string(),
                preserve_fragments: true,
            },
            MediaPolicy {
                image_template: "Preservation image".to_string(),
                audio_template: Some("Preservation audio".to_string()),
                max_audio_sources: 4,
                empty_alt_policy: EmptyAltPolicy::Decorative,
                emit_dimensions: true,
                non_image_media_policy: NonImageMediaPolicy::TemplateAudio,
            },
        )
    }

    #[test]
    fn converts_mediawiki_primitives_and_routes_links() {
        let (link_policy, media_policy) = policies();
        let images = BTreeMap::new();
        let output = convert(HtmlToWikitextInput {
            html: "<h2>History</h2><p>Hello <strong>world</strong>. <a href=\"/wiki/Other_Page#Part\">Other</a> and <a href=\"https://outside.example/x\">outside</a>.</p><ul><li>One</li><li>Two</li></ul>",
            canonical_title: "Example",
            canonical_url: "https://source.example/wiki/Example",
            media_scope: "source",
            link_policy: &link_policy,
            media_policy: &media_policy,
            infobox_policy: None,
            images: &images,
            media_occurrences: None,
        })
        .expect("convert standard HTML");

        assert!(output.wikitext.contains("== History =="));
        assert!(output.wikitext.contains("Hello '''world'''."));
        assert!(
            output
                .wikitext
                .contains("[[Special:Archive/source/Other Page#Part|Other]]")
        );
        assert!(
            output
                .wikitext
                .contains("[https://outside.example/x outside]")
        );
        assert!(output.wikitext.contains("* One\n* Two"));
        assert_eq!(output.coverage.internal_links, 1);
        assert_eq!(output.coverage.external_links, 1);
    }

    #[test]
    fn binds_captured_images_without_knowing_the_producer_schema() {
        let (link_policy, media_policy) = policies();
        let source_url = "https://source.example/media/example.png".to_string();
        let mut images = BTreeMap::new();
        images.insert(
            source_url.clone(),
            MediaReference {
                ordinal: None,
                media_kind: None,
                owner_element: None,
                owner_ordinal: None,
                element: None,
                attribute: None,
                candidate_index: None,
                descriptor: None,
                source_url: source_url.clone(),
                alt: Some("Captured example".to_string()),
                width: Some(320),
                height: Some(200),
                content_type: "image/png".to_string(),
                sha256: "a".repeat(64),
            },
        );
        let output = convert(HtmlToWikitextInput {
            html: "<p><img src=\"https://source.example/media/example.png\" alt=\"Captured example\" width=\"320\" height=\"200\"></p>",
            canonical_title: "Example",
            canonical_url: "https://source.example/wiki/Example",
            media_scope: "source",
            link_policy: &link_policy,
            media_policy: &media_policy,
            infobox_policy: None,
            images: &images,
            media_occurrences: None,
        })
        .expect("convert captured image");

        assert!(
            output
                .wikitext
                .contains("{{Preservation image|site=source|sha256=")
        );
        assert!(
            output
                .wikitext
                .contains("|alt=Captured example|width=320|height=200}}")
        );
        assert_eq!(output.used_media, BTreeSet::from([source_url]));
    }
}
