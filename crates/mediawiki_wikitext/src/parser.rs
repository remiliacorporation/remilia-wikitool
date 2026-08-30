use super::{
    Document, InternalLink, LinkKind, Node, NodeId, NodeKind, Parameter, ParseError,
    ParseErrorKind, ProtectedKind, ProtectedRegion, Redirect, SourceProfile, Span, Template,
    TemplateArgument, normalize_mediawiki_word, trim_span,
};

struct DraftNode {
    parent: Option<NodeId>,
    depth: usize,
    start: usize,
    completed: Option<(usize, NodeKind)>,
}

#[derive(Clone, Copy)]
enum BraceOpening {
    Template,
    Parameter,
}

pub(super) fn parse<'a>(
    source: &'a str,
    profile: &SourceProfile,
) -> Result<Document<'a>, ParseError> {
    let mut parser = Parser {
        source,
        bytes: source.as_bytes(),
        profile,
        nodes: Vec::new(),
    };
    parser.parse_root()?;
    let redirect = parser.detect_redirect();
    let nodes = parser
        .nodes
        .into_iter()
        .enumerate()
        .map(|(index, draft)| {
            let (end, kind) = draft
                .completed
                .expect("parser only returns after completing every reserved node");
            Node {
                id: NodeId(index),
                parent: draft.parent,
                depth: draft.depth,
                span: Span {
                    start: draft.start,
                    end,
                },
                kind,
            }
        })
        .collect();
    Ok(Document {
        source,
        nodes,
        redirect,
    })
}

struct Parser<'source, 'profile> {
    source: &'source str,
    bytes: &'source [u8],
    profile: &'profile SourceProfile,
    nodes: Vec<DraftNode>,
}

impl Parser<'_, '_> {
    fn parse_root(&mut self) -> Result<(), ParseError> {
        let mut cursor = 0;
        while cursor < self.bytes.len() {
            if let Some(end) = self.parse_protected(cursor, None, 0)? {
                cursor = end;
                continue;
            }
            if let Some(opening) = self.brace_opening(cursor) {
                cursor = match opening {
                    BraceOpening::Template => self.parse_template(cursor, None, 0)?,
                    BraceOpening::Parameter => self.parse_parameter(cursor, None, 0)?,
                };
                continue;
            }
            if self.starts_with(cursor, b"[[")
                && let Some(end) = self.parse_link(cursor, None, 0)?
            {
                cursor = end;
                continue;
            }
            cursor = self.next_char(cursor);
        }
        Ok(())
    }

    fn parse_template(
        &mut self,
        start: usize,
        parent: Option<NodeId>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        self.ensure_depth(depth, start)?;
        let id = self.reserve_node(start, parent, depth);
        let mut cursor = start + 2;
        let mut part_start = cursor;
        let mut parts = Vec::new();
        let mut equals = Vec::new();
        let mut current_equals = None;

        loop {
            if cursor >= self.bytes.len() {
                return Err(ParseError::new(ParseErrorKind::UnclosedTemplate, start));
            }
            if self.starts_with(cursor, b"}}") {
                parts.push(Span {
                    start: part_start,
                    end: cursor,
                });
                equals.push(current_equals);
                let end = cursor + 2;
                let name = trim_span(self.source, parts[0].start, parts[0].end);
                let arguments = parts
                    .into_iter()
                    .zip(equals)
                    .skip(1)
                    .map(|(span, equal)| match equal {
                        Some(equal) => TemplateArgument {
                            span,
                            name: Some(trim_span(self.source, span.start, equal)),
                            value: trim_span(self.source, equal + 1, span.end),
                        },
                        None => TemplateArgument {
                            span,
                            name: None,
                            value: trim_span(self.source, span.start, span.end),
                        },
                    })
                    .collect();
                self.complete_node(id, end, NodeKind::Template(Template { name, arguments }));
                return Ok(end);
            }
            if let Some(end) = self.parse_protected(cursor, Some(id), depth + 1)? {
                cursor = end;
                continue;
            }
            if let Some(opening) = self.brace_opening(cursor) {
                cursor = match opening {
                    BraceOpening::Template => self.parse_template(cursor, Some(id), depth + 1)?,
                    BraceOpening::Parameter => self.parse_parameter(cursor, Some(id), depth + 1)?,
                };
                continue;
            }
            if self.starts_with(cursor, b"[[")
                && let Some(end) = self.parse_link(cursor, Some(id), depth + 1)?
            {
                cursor = end;
                continue;
            }
            match self.bytes[cursor] {
                b'|' => {
                    parts.push(Span {
                        start: part_start,
                        end: cursor,
                    });
                    equals.push(current_equals);
                    part_start = cursor + 1;
                    current_equals = None;
                    cursor += 1;
                }
                b'=' if current_equals.is_none() && !parts.is_empty() => {
                    current_equals = Some(cursor);
                    cursor += 1;
                }
                _ => cursor = self.next_char(cursor),
            }
        }
    }

    fn parse_parameter(
        &mut self,
        start: usize,
        parent: Option<NodeId>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        self.ensure_depth(depth, start)?;
        let id = self.reserve_node(start, parent, depth);
        let mut cursor = start + 3;
        let name_start = cursor;
        let mut default_start = None;

        loop {
            if cursor >= self.bytes.len() {
                return Err(ParseError::new(ParseErrorKind::UnclosedParameter, start));
            }
            if self.starts_with(cursor, b"}}}") {
                let end = cursor + 3;
                let name_end = default_start.map_or(cursor, |start| start - 1);
                let parameter = Parameter {
                    name: trim_span(self.source, name_start, name_end),
                    default: default_start
                        .map(|default_start| trim_span(self.source, default_start, cursor)),
                };
                self.complete_node(id, end, NodeKind::Parameter(parameter));
                return Ok(end);
            }
            if let Some(end) = self.parse_protected(cursor, Some(id), depth + 1)? {
                cursor = end;
                continue;
            }
            if let Some(opening) = self.brace_opening(cursor) {
                cursor = match opening {
                    BraceOpening::Template => self.parse_template(cursor, Some(id), depth + 1)?,
                    BraceOpening::Parameter => self.parse_parameter(cursor, Some(id), depth + 1)?,
                };
                continue;
            }
            if self.starts_with(cursor, b"[[")
                && let Some(end) = self.parse_link(cursor, Some(id), depth + 1)?
            {
                cursor = end;
                continue;
            }
            if self.bytes[cursor] == b'|' && default_start.is_none() {
                default_start = Some(cursor + 1);
                cursor += 1;
                continue;
            }
            cursor = self.next_char(cursor);
        }
    }

    fn parse_link(
        &mut self,
        start: usize,
        parent: Option<NodeId>,
        depth: usize,
    ) -> Result<Option<usize>, ParseError> {
        self.ensure_depth(depth, start)?;
        let id = self.reserve_node(start, parent, depth);
        let mut cursor = start + 2;
        let mut part_start = cursor;
        let mut parts = Vec::new();

        loop {
            if cursor >= self.bytes.len() {
                return Err(ParseError::new(ParseErrorKind::UnclosedInternalLink, start));
            }
            if self.starts_with(cursor, b"]]") {
                parts.push(Span {
                    start: part_start,
                    end: cursor,
                });
                let end = cursor + 2;
                let target = trim_span(self.source, parts[0].start, parts[0].end);
                let target_text = &self.source[target.start..target.end];
                let fragment_offset = target_text
                    .char_indices()
                    .find_map(|(offset, character)| (character == '#').then_some(offset));
                let title = match fragment_offset {
                    Some(offset) => trim_span(self.source, target.start, target.start + offset),
                    None => target,
                };
                let fragment = fragment_offset.map(|offset| Span {
                    start: target.start + offset + 1,
                    end: target.end,
                });
                let title_text = &self.source[title.start..title.end];
                let (kind, leading_colon) =
                    classify_link(title_text, fragment.is_some(), self.profile);
                let components = parts
                    .into_iter()
                    .skip(1)
                    .map(|part| trim_span(self.source, part.start, part.end))
                    .collect();
                self.complete_node(
                    id,
                    end,
                    NodeKind::InternalLink(InternalLink {
                        target,
                        title,
                        fragment,
                        components,
                        kind,
                        leading_colon,
                    }),
                );
                return Ok(Some(end));
            }
            if let Some(end) = self.parse_protected(cursor, Some(id), depth + 1)? {
                cursor = end;
                continue;
            }
            if let Some(opening) = self.brace_opening(cursor) {
                cursor = match opening {
                    BraceOpening::Template => self.parse_template(cursor, Some(id), depth + 1)?,
                    BraceOpening::Parameter => self.parse_parameter(cursor, Some(id), depth + 1)?,
                };
                continue;
            }
            if parts.is_empty() && matches!(self.bytes[cursor], b'[' | b']') {
                self.nodes.truncate(id.0);
                return Ok(None);
            }
            if self.starts_with(cursor, b"[[")
                && let Some(end) = self.parse_link(cursor, Some(id), depth + 1)?
            {
                cursor = end;
                continue;
            }
            if self.bytes[cursor] == b'|' {
                parts.push(Span {
                    start: part_start,
                    end: cursor,
                });
                part_start = cursor + 1;
                cursor += 1;
                continue;
            }
            cursor = self.next_char(cursor);
        }
    }

    fn parse_protected(
        &mut self,
        start: usize,
        parent: Option<NodeId>,
        depth: usize,
    ) -> Result<Option<usize>, ParseError> {
        if self.starts_with(start, b"<!--") {
            self.ensure_depth(depth, start)?;
            let mut end_start = start + 4;
            while end_start < self.bytes.len() && !self.starts_with(end_start, b"-->") {
                end_start = self.next_char(end_start);
            }
            if end_start == self.bytes.len() {
                return Err(ParseError::new(
                    ParseErrorKind::UnclosedProtected(ProtectedKind::Comment),
                    start,
                ));
            }
            let end = end_start + 3;
            let id = self.reserve_node(start, parent, depth);
            self.complete_node(
                id,
                end,
                NodeKind::Protected(ProtectedRegion {
                    kind: ProtectedKind::Comment,
                }),
            );
            return Ok(Some(end));
        }
        if self.bytes.get(start) != Some(&b'<') || self.bytes.get(start + 1) == Some(&b'/') {
            return Ok(None);
        }

        let name_start = start + 1;
        let mut name_end = name_start;
        while self
            .bytes
            .get(name_end)
            .is_some_and(u8::is_ascii_alphanumeric)
        {
            name_end += 1;
        }
        if name_end == name_start {
            return Ok(None);
        }
        if !self
            .bytes
            .get(name_end)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'/' | b'>'))
        {
            return Ok(None);
        }
        let Some(kind) = protected_kind(&self.source[name_start..name_end]) else {
            return Ok(None);
        };
        self.ensure_depth(depth, start)?;
        let Some(open_end) = self.find_tag_end(name_end) else {
            return Err(ParseError::new(
                ParseErrorKind::MalformedProtectedTag(kind),
                start,
            ));
        };
        let id = self.reserve_node(start, parent, depth);
        if self.source[start..open_end].trim_end().ends_with("/>") {
            self.complete_node(id, open_end, NodeKind::Protected(ProtectedRegion { kind }));
            return Ok(Some(open_end));
        }

        let tag_name = &self.source[name_start..name_end];
        let mut cursor = open_end;
        while cursor < self.bytes.len() {
            if self.bytes[cursor] == b'<'
                && self.bytes.get(cursor + 1) == Some(&b'/')
                && self.ascii_case_matches(cursor + 2, tag_name)
            {
                let boundary = cursor + 2 + tag_name.len();
                if self
                    .bytes
                    .get(boundary)
                    .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
                {
                    let Some(close_end) = self.find_tag_end(boundary) else {
                        return Err(ParseError::new(
                            ParseErrorKind::MalformedProtectedTag(kind),
                            cursor,
                        ));
                    };
                    self.complete_node(
                        id,
                        close_end,
                        NodeKind::Protected(ProtectedRegion { kind }),
                    );
                    return Ok(Some(close_end));
                }
            }
            cursor = self.next_char(cursor);
        }

        Err(ParseError::new(
            ParseErrorKind::UnclosedProtected(kind),
            start,
        ))
    }

    fn find_tag_end(&self, start: usize) -> Option<usize> {
        let mut cursor = start;
        let mut quote = None;
        while cursor < self.bytes.len() {
            let byte = self.bytes[cursor];
            match (quote, byte) {
                (None, b'\'' | b'"') => quote = Some(byte),
                (Some(open), close) if open == close => quote = None,
                (None, b'>') => return Some(cursor + 1),
                _ => {}
            }
            cursor += 1;
        }
        None
    }

    fn detect_redirect(&self) -> Option<Redirect> {
        let mut cursor = usize::from(self.source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();
        loop {
            cursor = self.skip_whitespace(cursor);
            let comment = self.nodes.iter().enumerate().find(|(_, node)| {
                node.start == cursor
                    && matches!(
                        node.completed,
                        Some((
                            _,
                            NodeKind::Protected(ProtectedRegion {
                                kind: ProtectedKind::Comment
                            })
                        ))
                    )
            });
            let Some((_, comment)) = comment else {
                break;
            };
            cursor = comment.completed.as_ref()?.0;
        }
        if self.bytes.get(cursor) != Some(&b'#') {
            return None;
        }
        cursor += 1;
        let word_start = cursor;
        while cursor < self.bytes.len() {
            let character = self.source[cursor..].chars().next()?;
            if character.is_whitespace() || matches!(character, ':' | '[') {
                break;
            }
            cursor += character.len_utf8();
        }
        if word_start == cursor {
            return None;
        }
        let keyword = &self.source[word_start..cursor];
        if !self
            .profile
            .redirect_magic_words
            .iter()
            .any(|word| normalized_eq(word, keyword))
        {
            return None;
        }
        cursor = self.skip_whitespace(cursor);
        if self.bytes.get(cursor) == Some(&b':') {
            cursor += 1;
            cursor = self.skip_whitespace(cursor);
        }
        let (index, _) = self.nodes.iter().enumerate().find(|(_, node)| {
            node.start == cursor
                && node.parent.is_none()
                && matches!(node.completed, Some((_, NodeKind::InternalLink(_))))
        })?;
        Some(Redirect {
            keyword: Span {
                start: word_start,
                end: word_start + keyword.len(),
            },
            link: NodeId(index),
        })
    }

    fn reserve_node(&mut self, start: usize, parent: Option<NodeId>, depth: usize) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(DraftNode {
            parent,
            depth,
            start,
            completed: None,
        });
        id
    }

    fn ensure_depth(&self, depth: usize, offset: usize) -> Result<(), ParseError> {
        if depth > self.profile.max_nesting_depth as usize {
            return Err(ParseError::new(
                ParseErrorKind::NestingTooDeep {
                    depth,
                    max_depth: self.profile.max_nesting_depth,
                },
                offset,
            ));
        }
        Ok(())
    }

    fn complete_node(&mut self, id: NodeId, end: usize, kind: NodeKind) {
        let node = &mut self.nodes[id.0];
        debug_assert!(node.completed.is_none());
        node.completed = Some((end, kind));
    }

    fn starts_with(&self, start: usize, sequence: &[u8]) -> bool {
        self.bytes.get(start..start.saturating_add(sequence.len())) == Some(sequence)
    }

    /// Decomposes a run using MediaWiki's rightmost three-brace argument preference. A remainder
    /// of two braces forms an outer template. A remainder of one is literal, so four braces are a
    /// literal brace around an argument while five are a template around an argument.
    fn brace_opening(&self, start: usize) -> Option<BraceOpening> {
        let mut count = 0;
        while self.bytes.get(start + count) == Some(&b'{') {
            count += 1;
        }
        match count % 3 {
            0 if count >= 3 => Some(BraceOpening::Parameter),
            2 => Some(BraceOpening::Template),
            _ => None,
        }
    }

    fn ascii_case_matches(&self, start: usize, value: &str) -> bool {
        self.bytes
            .get(start..start.saturating_add(value.len()))
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(value.as_bytes()))
    }

    fn next_char(&self, cursor: usize) -> usize {
        cursor
            + self.source[cursor..]
                .chars()
                .next()
                .expect("cursor is before the UTF-8 source end")
                .len_utf8()
    }

    fn skip_whitespace(&self, mut cursor: usize) -> usize {
        while cursor < self.bytes.len() {
            let character = self.source[cursor..]
                .chars()
                .next()
                .expect("cursor is before the UTF-8 source end");
            if !character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        cursor
    }
}

fn protected_kind(name: &str) -> Option<ProtectedKind> {
    if name.eq_ignore_ascii_case("nowiki") {
        Some(ProtectedKind::Nowiki)
    } else if name.eq_ignore_ascii_case("pre") {
        Some(ProtectedKind::Pre)
    } else if name.eq_ignore_ascii_case("source") {
        Some(ProtectedKind::Source)
    } else if name.eq_ignore_ascii_case("syntaxhighlight") {
        Some(ProtectedKind::SyntaxHighlight)
    } else {
        None
    }
}

fn classify_link(title: &str, has_fragment: bool, profile: &SourceProfile) -> (LinkKind, bool) {
    let mut effective = title.trim();
    let leading_colon = effective.starts_with(':');
    if leading_colon {
        effective = effective[1..].trim_start();
    }
    if effective.is_empty() && has_fragment {
        return (LinkKind::Fragment, leading_colon);
    }
    let Some((namespace, _)) = effective.split_once(':') else {
        return (LinkKind::Main, leading_colon);
    };
    if profile
        .file_namespace_aliases
        .iter()
        .any(|alias| normalized_eq(alias, namespace))
    {
        (LinkKind::File, leading_colon)
    } else if profile
        .category_namespace_aliases
        .iter()
        .any(|alias| normalized_eq(alias, namespace))
    {
        (LinkKind::Category, leading_colon)
    } else {
        (LinkKind::Other, leading_colon)
    }
}

fn normalized_eq(left: &str, right: &str) -> bool {
    normalize_mediawiki_word(left) == normalize_mediawiki_word(right)
}
