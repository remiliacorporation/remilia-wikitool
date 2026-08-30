#![forbid(unsafe_code)]

//! Deterministic structural parsing and explicit rewriting for MediaWiki source wikitext.
//!
//! The crate deliberately does not fetch pages, read corpus layouts, expand templates, or choose
//! target-wiki semantics. A caller supplies revision-bound UTF-8 bytes and a small parsing profile;
//! this crate verifies that neutral envelope, identifies the syntax needed by an archival
//! projector, and applies only caller-authorized, non-overlapping replacements.

mod parser;

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SOURCE_PROFILE_SCHEMA: &str = "mediawiki.wikitext-source-profile.v1";
pub const SOURCE_DOCUMENT_SCHEMA: &str = "mediawiki.wikitext-source-document.v1";

/// Site syntax needed to classify links and recognize redirects without embedding a source site.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceProfile {
    pub schema: String,
    pub profile_id: String,
    pub source_key: String,
    pub max_wikitext_bytes: u64,
    pub max_nesting_depth: u32,
    pub file_namespace_aliases: BTreeSet<String>,
    pub category_namespace_aliases: BTreeSet<String>,
    pub redirect_magic_words: BTreeSet<String>,
}

impl SourceProfile {
    /// Constructs an English MediaWiki baseline. Callers should replace the aliases with the
    /// namespace and magic-word data reported by the source wiki when those data are available.
    pub fn mediawiki_defaults(
        profile_id: impl Into<String>,
        source_key: impl Into<String>,
        max_wikitext_bytes: u64,
    ) -> Self {
        Self {
            schema: SOURCE_PROFILE_SCHEMA.to_string(),
            profile_id: profile_id.into(),
            source_key: source_key.into(),
            max_wikitext_bytes,
            max_nesting_depth: 256,
            file_namespace_aliases: ["File", "Image"].map(str::to_string).into(),
            category_namespace_aliases: ["Category"].map(str::to_string).into(),
            redirect_magic_words: ["REDIRECT"].map(str::to_string).into(),
        }
    }
}

/// Revision and byte identity passed across an external corpus boundary.
///
/// This is intentionally a document envelope, not a corpus manifest. Private or site-specific
/// producers can translate their page records into this stable public contract without becoming a
/// Wikitool runtime dependency.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceDocumentReceipt {
    pub schema: String,
    pub source_key: String,
    pub page_id: u64,
    pub namespace_id: i32,
    pub title: String,
    pub revision_id: u64,
    pub revision_timestamp: String,
    pub content_model: String,
    pub source_sha256: String,
    pub source_bytes: u64,
}

pub struct SourceDocumentInput<'wikitext, 'receipt, 'profile> {
    pub receipt: &'receipt SourceDocumentReceipt,
    pub source: &'profile SourceProfile,
    pub wikitext: &'wikitext str,
}

pub struct ProfiledParseInput<'wikitext, 'profile> {
    pub source: &'profile SourceProfile,
    pub wikitext: &'wikitext str,
}

/// Verifies a revision-bound source envelope before parsing its wikitext.
pub fn parse_source_document<'wikitext>(
    input: SourceDocumentInput<'wikitext, '_, '_>,
) -> Result<Document<'wikitext>, ParseError> {
    validate_source_profile(input.source)?;
    validate_source_document(input.receipt, input.source, input.wikitext)?;
    parser::parse(input.wikitext, input.source)
}

/// Parses wikitext under a validated source profile when document identity is managed separately.
pub fn parse_profiled<'wikitext>(
    input: ProfiledParseInput<'wikitext, '_>,
) -> Result<Document<'wikitext>, ParseError> {
    validate_source_profile(input.source)?;
    validate_input_size(input.wikitext, input.source.max_wikitext_bytes)?;
    parser::parse(input.wikitext, input.source)
}

pub fn validate_source_profile(source: &SourceProfile) -> Result<(), ParseError> {
    if source.schema != SOURCE_PROFILE_SCHEMA {
        return Err(ParseError::invalid_profile(format!(
            "source profile schema must be {SOURCE_PROFILE_SCHEMA}, got {}",
            source.schema
        )));
    }
    validate_identifier(&source.profile_id, "profile_id")?;
    validate_identifier(&source.source_key, "source_key")?;
    if source.max_wikitext_bytes == 0 {
        return Err(ParseError::invalid_profile(
            "max_wikitext_bytes must be greater than zero",
        ));
    }
    if source.max_nesting_depth == 0 {
        return Err(ParseError::invalid_profile(
            "max_nesting_depth must be greater than zero",
        ));
    }
    validate_aliases(&source.file_namespace_aliases, "file_namespace_aliases")?;
    validate_aliases(
        &source.category_namespace_aliases,
        "category_namespace_aliases",
    )?;
    for file_alias in &source.file_namespace_aliases {
        if source
            .category_namespace_aliases
            .iter()
            .any(|category_alias| normalized_words_equal(file_alias, category_alias))
        {
            return Err(ParseError::invalid_profile(format!(
                "namespace alias {file_alias:?} cannot classify both file and category links"
            )));
        }
    }
    if source.redirect_magic_words.is_empty() {
        return Err(ParseError::invalid_profile(
            "redirect_magic_words must not be empty",
        ));
    }
    for word in &source.redirect_magic_words {
        let normalized = word.trim();
        if normalized.is_empty()
            || normalized.starts_with('#')
            || normalized.contains('[')
            || normalized.contains(']')
        {
            return Err(ParseError::invalid_profile(format!(
                "redirect magic word {word:?} is not a bare magic word"
            )));
        }
    }
    Ok(())
}

fn validate_source_document(
    receipt: &SourceDocumentReceipt,
    source: &SourceProfile,
    wikitext: &str,
) -> Result<(), ParseError> {
    if receipt.schema != SOURCE_DOCUMENT_SCHEMA {
        return Err(ParseError::invalid_receipt(format!(
            "source document schema must be {SOURCE_DOCUMENT_SCHEMA}, got {}",
            receipt.schema
        )));
    }
    if receipt.source_key != source.source_key {
        return Err(ParseError::invalid_receipt(format!(
            "receipt source_key {:?} does not match profile source_key {:?}",
            receipt.source_key, source.source_key
        )));
    }
    if receipt.page_id == 0 {
        return Err(ParseError::invalid_receipt(
            "page_id must be greater than zero",
        ));
    }
    if receipt.revision_id == 0 {
        return Err(ParseError::invalid_receipt(
            "revision_id must be greater than zero",
        ));
    }
    if receipt.title.trim().is_empty() {
        return Err(ParseError::invalid_receipt("title must not be empty"));
    }
    if receipt.revision_timestamp.trim().is_empty() {
        return Err(ParseError::invalid_receipt(
            "revision_timestamp must not be empty",
        ));
    }
    if receipt.content_model != "wikitext" {
        return Err(ParseError::invalid_receipt(format!(
            "content_model must be wikitext, got {:?}",
            receipt.content_model
        )));
    }
    validate_input_size(wikitext, source.max_wikitext_bytes)?;
    let actual_bytes = u64::try_from(wikitext.len()).map_err(|_| {
        ParseError::invalid_receipt("wikitext byte length cannot be represented as u64")
    })?;
    if receipt.source_bytes != actual_bytes {
        return Err(ParseError::invalid_receipt(format!(
            "source_bytes is {}, but the supplied wikitext has {actual_bytes} bytes",
            receipt.source_bytes
        )));
    }
    if !is_lower_hex_digest(&receipt.source_sha256) {
        return Err(ParseError::invalid_receipt(
            "source_sha256 must be a lowercase 64-character hexadecimal digest",
        ));
    }
    let actual_sha256 = format!("{:x}", Sha256::digest(wikitext.as_bytes()));
    if receipt.source_sha256 != actual_sha256 {
        return Err(ParseError::invalid_receipt(format!(
            "source_sha256 does not match the supplied wikitext (expected {actual_sha256})"
        )));
    }
    Ok(())
}

fn validate_input_size(wikitext: &str, max_bytes: u64) -> Result<(), ParseError> {
    let actual_bytes = wikitext.len() as u64;
    if actual_bytes > max_bytes {
        return Err(ParseError::new(
            ParseErrorKind::InputTooLarge {
                actual_bytes,
                max_bytes,
            },
            0,
        ));
    }
    Ok(())
}

fn normalized_words_equal(left: &str, right: &str) -> bool {
    normalize_mediawiki_word(left) == normalize_mediawiki_word(right)
}

fn normalize_mediawiki_word(value: &str) -> String {
    let mut output = String::new();
    let mut pending_space = false;
    for character in value.trim().chars() {
        if character == '_' || character.is_whitespace() {
            pending_space = !output.is_empty();
            continue;
        }
        if pending_space {
            output.push(' ');
            pending_space = false;
        }
        output.extend(character.to_lowercase());
    }
    output
}

fn validate_identifier(value: &str, label: &str) -> Result<(), ParseError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ParseError::invalid_profile(format!(
            "{label} must contain only ASCII letters, digits, '.', '-' or '_'"
        )));
    }
    Ok(())
}

fn validate_aliases(aliases: &BTreeSet<String>, label: &str) -> Result<(), ParseError> {
    if aliases.is_empty() {
        return Err(ParseError::invalid_profile(format!(
            "{label} must not be empty"
        )));
    }
    for alias in aliases {
        let normalized = alias.trim();
        if normalized.is_empty() || normalized.contains(':') || normalized.starts_with('#') {
            return Err(ParseError::invalid_profile(format!(
                "namespace alias {alias:?} in {label} is invalid"
            )));
        }
    }
    Ok(())
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(usize);

impl NodeId {
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    start: usize,
    end: usize,
}

impl Span {
    pub fn start(self) -> usize {
        self.start
    }

    pub fn end(self) -> usize {
        self.end
    }

    pub fn len(self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document<'a> {
    source: &'a str,
    nodes: Vec<Node>,
    redirect: Option<Redirect>,
}

impl<'a> Document<'a> {
    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0)
    }

    pub fn text(&self, span: Span) -> &'a str {
        &self.source[span.start..span.end]
    }

    pub fn redirect(&self) -> Option<&Redirect> {
        self.redirect.as_ref()
    }

    /// Applies a set of exact node replacements. Ancestor/descendant replacements are rejected so
    /// the caller must make the intended rewrite authority explicit.
    pub fn rewrite(&self, plan: &RewritePlan) -> Result<String, RewriteError> {
        let mut replacements = Vec::with_capacity(plan.replacements.len());
        for (id, replacement) in &plan.replacements {
            let Some(node) = self.node(*id) else {
                return Err(RewriteError::UnknownNode(*id));
            };
            replacements.push((*id, node.span, replacement.as_str()));
        }
        replacements.sort_by_key(|(_, span, _)| (span.start, span.end));

        for pair in replacements.windows(2) {
            let (left_id, left, _) = pair[0];
            let (right_id, right, _) = pair[1];
            if right.start < left.end {
                return Err(RewriteError::OverlappingNodes {
                    first: left_id,
                    second: right_id,
                });
            }
        }

        let replaced_bytes = replacements
            .iter()
            .map(|(_, span, _)| span.len())
            .sum::<usize>();
        let replacement_bytes = replacements
            .iter()
            .map(|(_, _, replacement)| replacement.len())
            .sum::<usize>();
        let mut output = String::with_capacity(
            self.source
                .len()
                .saturating_sub(replaced_bytes)
                .saturating_add(replacement_bytes),
        );
        let mut cursor = 0;
        for (_, span, replacement) in replacements {
            output.push_str(&self.source[cursor..span.start]);
            output.push_str(replacement);
            cursor = span.end;
        }
        output.push_str(&self.source[cursor..]);
        Ok(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    id: NodeId,
    parent: Option<NodeId>,
    depth: usize,
    span: Span,
    kind: NodeKind,
}

impl Node {
    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn kind(&self) -> &NodeKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    Template(Template),
    Parameter(Parameter),
    InternalLink(InternalLink),
    Protected(ProtectedRegion),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub name: Span,
    pub arguments: Vec<TemplateArgument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateArgument {
    /// The argument bytes excluding the leading `|`, including source whitespace.
    pub span: Span,
    /// The trimmed name before the first top-level `=`, or `None` for a positional argument.
    pub name: Option<Span>,
    /// The trimmed positional value or named value after the first top-level `=`.
    pub value: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: Span,
    /// Everything after the first top-level `|`; further pipes remain part of the default.
    pub default: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalLink {
    /// The complete trimmed target, including any `#fragment` suffix.
    pub target: Span,
    /// The target title before `#`, or an empty span for a fragment-only link.
    pub title: Span,
    /// The fragment text after the first `#`, excluding the marker.
    pub fragment: Option<Span>,
    /// Link components after the target. For file links these retain each option/caption segment.
    pub components: Vec<Span>,
    pub kind: LinkKind,
    pub leading_colon: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Main,
    File,
    Category,
    Other,
    Fragment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedRegion {
    pub kind: ProtectedKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectedKind {
    Comment,
    Nowiki,
    Pre,
    Source,
    SyntaxHighlight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redirect {
    pub keyword: Span,
    pub link: NodeId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RewritePlan {
    replacements: BTreeMap<NodeId, String>,
}

impl RewritePlan {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace(
        &mut self,
        node: NodeId,
        replacement: impl Into<String>,
    ) -> Result<(), RewritePlanError> {
        if self.replacements.contains_key(&node) {
            return Err(RewritePlanError::DuplicateNode(node));
        }
        self.replacements.insert(node, replacement.into());
        Ok(())
    }

    pub fn remove(&mut self, node: NodeId) -> Result<(), RewritePlanError> {
        self.replace(node, String::new())
    }

    pub fn is_empty(&self) -> bool {
        self.replacements.is_empty()
    }

    pub fn len(&self) -> usize {
        self.replacements.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewritePlanError {
    DuplicateNode(NodeId),
}

impl Display for RewritePlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode(node) => {
                write!(formatter, "node {} already has a replacement", node.index())
            }
        }
    }
}

impl Error for RewritePlanError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteError {
    UnknownNode(NodeId),
    OverlappingNodes { first: NodeId, second: NodeId },
}

impl Display for RewriteError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode(node) => write!(formatter, "unknown node {}", node.index()),
            Self::OverlappingNodes { first, second } => write!(
                formatter,
                "replacement nodes {} and {} overlap",
                first.index(),
                second.index()
            ),
        }
    }
}

impl Error for RewriteError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    kind: ParseErrorKind,
    byte_offset: usize,
}

impl ParseError {
    fn new(kind: ParseErrorKind, byte_offset: usize) -> Self {
        Self { kind, byte_offset }
    }

    fn invalid_profile(message: impl Into<String>) -> Self {
        Self::new(ParseErrorKind::InvalidProfile(message.into()), 0)
    }

    fn invalid_receipt(message: impl Into<String>) -> Self {
        Self::new(ParseErrorKind::InvalidReceipt(message.into()), 0)
    }

    pub fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }

    pub fn byte_offset(&self) -> usize {
        self.byte_offset
    }
}

impl Display for ParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte {}", self.kind, self.byte_offset)
    }
}

impl Error for ParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    InvalidProfile(String),
    InvalidReceipt(String),
    InputTooLarge { actual_bytes: u64, max_bytes: u64 },
    NestingTooDeep { depth: usize, max_depth: u32 },
    UnclosedTemplate,
    UnclosedParameter,
    UnclosedInternalLink,
    UnclosedProtected(ProtectedKind),
    MalformedProtectedTag(ProtectedKind),
}

impl Display for ParseErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProfile(message) => write!(formatter, "invalid source profile: {message}"),
            Self::InvalidReceipt(message) => {
                write!(formatter, "invalid source document receipt: {message}")
            }
            Self::InputTooLarge {
                actual_bytes,
                max_bytes,
            } => write!(
                formatter,
                "wikitext is {actual_bytes} bytes, exceeding the {max_bytes}-byte profile limit"
            ),
            Self::NestingTooDeep { depth, max_depth } => write!(
                formatter,
                "wikitext nesting depth {depth} exceeds the profile limit {max_depth}"
            ),
            Self::UnclosedTemplate => formatter.write_str("unclosed template invocation"),
            Self::UnclosedParameter => formatter.write_str("unclosed template parameter"),
            Self::UnclosedInternalLink => formatter.write_str("unclosed internal link"),
            Self::UnclosedProtected(kind) => write!(formatter, "unclosed {kind:?} region"),
            Self::MalformedProtectedTag(kind) => {
                write!(formatter, "malformed opening tag for {kind:?} region")
            }
        }
    }
}

fn trim_span(source: &str, mut start: usize, mut end: usize) -> Span {
    while start < end {
        let Some(character) = source[start..end].chars().next() else {
            break;
        };
        if !character.is_whitespace() {
            break;
        }
        start += character.len_utf8();
    }
    while start < end {
        let Some(character) = source[start..end].chars().next_back() else {
            break;
        };
        if !character.is_whitespace() {
            break;
        }
        end -= character.len_utf8();
    }
    Span { start, end }
}
