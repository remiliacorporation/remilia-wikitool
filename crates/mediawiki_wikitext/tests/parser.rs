use std::collections::BTreeSet;

use mediawiki_wikitext::{
    InternalLink, LinkKind, Node, NodeKind, ParseErrorKind, ProfiledParseInput, ProtectedKind,
    RewriteError, RewritePlan, SOURCE_DOCUMENT_SCHEMA, SourceDocumentInput, SourceDocumentReceipt,
    SourceProfile, parse_profiled, parse_source_document,
};
use sha2::{Digest, Sha256};

fn profile() -> SourceProfile {
    let mut profile = SourceProfile::mediawiki_defaults("test-v1", "example", 1_000_000);
    profile.file_namespace_aliases.insert("Datei".to_string());
    profile
        .category_namespace_aliases
        .insert("Kategorie".to_string());
    profile.redirect_magic_words =
        BTreeSet::from(["REDIRECT".to_string(), "WEITERLEITUNG".to_string()]);
    profile
}

fn links<'document, 'source>(
    document: &'document mediawiki_wikitext::Document<'source>,
) -> Vec<(&'document Node, &'document InternalLink)> {
    document
        .nodes()
        .iter()
        .filter_map(|node| match node.kind() {
            NodeKind::InternalLink(link) => Some((node, link)),
            _ => None,
        })
        .collect()
}

#[test]
fn parses_nested_templates_parameters_and_arguments_without_losing_spans() {
    let source = "{{ Infobox\n | name = Ada\n | nested = {{Value|1={{{fallback|[[Target|label]]}}}}}\n | positional\n}}";
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: source,
    })
    .unwrap();

    assert_eq!(document.nodes().len(), 4);
    let NodeKind::Template(outer) = document.nodes()[0].kind() else {
        panic!("expected outer template");
    };
    assert_eq!(document.text(outer.name), "Infobox");
    assert_eq!(outer.arguments.len(), 3);
    assert_eq!(document.text(outer.arguments[0].name.unwrap()), "name");
    assert_eq!(document.text(outer.arguments[0].value), "Ada");
    assert_eq!(document.text(outer.arguments[2].value), "positional");
    assert!(outer.arguments[2].name.is_none());

    let nested = &document.nodes()[1];
    assert_eq!(nested.parent(), Some(document.nodes()[0].id()));
    let NodeKind::Template(nested_template) = nested.kind() else {
        panic!("expected nested template");
    };
    assert_eq!(document.text(nested_template.name), "Value");

    let NodeKind::Parameter(parameter) = document.nodes()[2].kind() else {
        panic!("expected parameter");
    };
    assert_eq!(document.text(parameter.name), "fallback");
    assert_eq!(
        document.text(parameter.default.expect("parameter default")),
        "[[Target|label]]"
    );
    assert_eq!(document.nodes()[2].parent(), Some(nested.id()));
    assert_eq!(document.nodes()[3].parent(), Some(document.nodes()[2].id()));
    assert_eq!(document.rewrite(&RewritePlan::new()).unwrap(), source);
}

#[test]
fn protects_comments_and_literal_extension_regions_at_every_depth() {
    let source = "<!-- {{hidden}} -->{{Box|a=<nowiki>[[No]]|{{No}}</nowiki>|b=<PRE class='x'>{{No}}</pre>|c=<source lang=cpp>[[No]]</SOURCE>|d=<syntaxhighlight lang=\"rust\">{{No}}</syntaxhighlight>}}";
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: source,
    })
    .unwrap();

    let kinds = document
        .nodes()
        .iter()
        .filter_map(|node| match node.kind() {
            NodeKind::Protected(region) => Some(region.kind),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            ProtectedKind::Comment,
            ProtectedKind::Nowiki,
            ProtectedKind::Pre,
            ProtectedKind::Source,
            ProtectedKind::SyntaxHighlight,
        ]
    );
    assert_eq!(
        document
            .nodes()
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::Template(_)))
            .count(),
        1
    );
}

#[test]
fn classifies_source_configured_link_namespaces_and_retains_file_components() {
    let source = "[[Main page]] [[#Part]] [[File:A.png|thumb|alt=Text|Caption]] [[:Category:C]] [[Kategorie:K|S]] [[Datei:B.jpg]] [[Help:X]]";
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: source,
    })
    .unwrap();
    let links = links(&document);
    assert_eq!(
        links.iter().map(|(_, link)| link.kind).collect::<Vec<_>>(),
        vec![
            LinkKind::Main,
            LinkKind::Fragment,
            LinkKind::File,
            LinkKind::Category,
            LinkKind::Category,
            LinkKind::File,
            LinkKind::Other,
        ]
    );
    assert_eq!(
        links[2]
            .1
            .components
            .iter()
            .map(|span| document.text(*span))
            .collect::<Vec<_>>(),
        vec!["thumb", "alt=Text", "Caption"]
    );
    assert!(links[3].1.leading_colon);
    assert!(!links[4].1.leading_colon);
    assert_eq!(document.text(links[1].1.title), "");
    assert_eq!(
        document.text(links[1].1.fragment.expect("fragment-only link")),
        "Part"
    );
}

#[test]
fn recognizes_only_leading_profiled_redirects() {
    let source = "\u{feff} <!-- provenance -->\n#weiterleitung: [[Target#Part]]";
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: source,
    })
    .unwrap();
    let redirect = document.redirect().expect("redirect");
    assert_eq!(document.text(redirect.keyword), "weiterleitung");
    let NodeKind::InternalLink(link) = document.node(redirect.link).unwrap().kind() else {
        panic!("redirect must reference a link node");
    };
    assert_eq!(document.text(link.target), "Target#Part");
    assert_eq!(document.text(link.title), "Target");
    assert_eq!(
        document.text(link.fragment.expect("redirect target fragment")),
        "Part"
    );

    let not_redirect = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: "Lead\n#REDIRECT [[Target]]",
    })
    .unwrap();
    assert!(not_redirect.redirect().is_none());
}

#[test]
fn verifies_revision_bound_document_identity_before_parsing() {
    let source = "{{A|[[B]]}}";
    let digest = format!("{:x}", Sha256::digest(source.as_bytes()));
    let receipt = SourceDocumentReceipt {
        schema: SOURCE_DOCUMENT_SCHEMA.to_string(),
        source_key: "example".to_string(),
        page_id: 42,
        namespace_id: 0,
        title: "Example".to_string(),
        revision_id: 84,
        revision_timestamp: "2026-08-29T00:00:00Z".to_string(),
        content_model: "wikitext".to_string(),
        source_sha256: digest,
        source_bytes: source.len() as u64,
    };
    let document = parse_source_document(SourceDocumentInput {
        receipt: &receipt,
        source: &profile(),
        wikitext: source,
    })
    .unwrap();
    assert_eq!(document.nodes().len(), 2);

    let mut wrong = receipt;
    wrong.revision_id += 1;
    wrong.source_sha256 = "0".repeat(64);
    let error = parse_source_document(SourceDocumentInput {
        receipt: &wrong,
        source: &profile(),
        wikitext: source,
    })
    .unwrap_err();
    assert!(matches!(error.kind(), ParseErrorKind::InvalidReceipt(_)));
}

#[test]
fn applies_only_explicit_non_overlapping_node_rewrites() {
    let source = "Before {{Outer|[[A]]}} and [[B|label]].";
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: source,
    })
    .unwrap();
    let outer = document.nodes()[0].id();
    let nested_link = document.nodes()[1].id();
    let sibling_link = document.nodes()[2].id();

    let mut valid = RewritePlan::new();
    valid.replace(outer, "mapped").unwrap();
    valid.replace(sibling_link, "[[Archive/B|label]]").unwrap();
    assert_eq!(
        document.rewrite(&valid).unwrap(),
        "Before mapped and [[Archive/B|label]]."
    );

    let mut overlapping = RewritePlan::new();
    overlapping.replace(outer, "mapped").unwrap();
    overlapping.replace(nested_link, "nested").unwrap();
    assert!(matches!(
        document.rewrite(&overlapping),
        Err(RewriteError::OverlappingNodes { .. })
    ));
}

#[test]
fn rejects_unbalanced_constructs_at_the_exact_opening_offset() {
    let error = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: "é {{open",
    })
    .unwrap_err();
    assert_eq!(error.byte_offset(), 3);
    assert_eq!(error.kind(), &ParseErrorKind::UnclosedTemplate);

    let protected = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: "<nowiki>[[literal]]",
    })
    .unwrap_err();
    assert_eq!(
        protected.kind(),
        &ParseErrorKind::UnclosedProtected(ProtectedKind::Nowiki)
    );

    let crossed = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: "{{outer|[[crossed}}]]",
    })
    .unwrap_err();
    assert_eq!(crossed.byte_offset(), 0);
    assert_eq!(crossed.kind(), &ParseErrorKind::UnclosedTemplate);
}

#[test]
fn preserves_literal_closing_markers_from_wowdev_wmo_revision_37042() {
    const SOURCE: &str = concat!(
        "     {{Template:Type|C3Vector}}* points[3] = \n",
        "       { &this->m_vertices[this->movi[3*mopy_index + 0]]\n",
        "       , &this->m_vertices[this->movi[3*mopy_index + 1]]\n",
        "       , &this->m_vertices[this->movi[3*mopy_index + 2]]\n",
        "       };\n",
    );

    assert_eq!(SOURCE.len(), 227);
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE.as_bytes())),
        "60f5716c6da2835d51a5c1948d9e6a210b7b60b77bb4d14466d79082f80898f2"
    );
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: SOURCE,
    })
    .unwrap();
    assert_eq!(
        document
            .nodes()
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::Template(_)))
            .count(),
        1
    );
    assert_eq!(document.rewrite(&RewritePlan::new()).unwrap(), SOURCE);
}

#[test]
fn preserves_unmatched_closing_markers_inside_and_outside_open_constructs() {
    let source = concat!(
        "literal ]] }} }}}\n",
        "{{T|value=array[index]] and brace }}}}\n",
        "[[Target|array[index]] and brace }}]]\n",
        "{{{name|array[index]] and brace }}}}\n",
    );
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: source,
    })
    .unwrap();
    assert_eq!(document.rewrite(&RewritePlan::new()).unwrap(), source);
}

#[test]
fn preserves_wowdev_player_revision_5447_pointer_bracket_run() {
    const SOURCE: &str = " [[[[[00DEE930]+0x2478]+0xC]+0x1028]+0x120]\n";

    assert_eq!(SOURCE.len(), 44);
    assert_eq!(
        format!("{:x}", Sha256::digest(SOURCE.as_bytes())),
        "5a29be1c5ddddd0a867d180861af8301a7eeeb300fa4112d079acdfc31b7613f"
    );
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: SOURCE,
    })
    .unwrap();
    assert!(document.nodes().is_empty());
    assert_eq!(document.rewrite(&RewritePlan::new()).unwrap(), SOURCE);
}

#[test]
fn distinguishes_impossible_link_targets_from_genuinely_unclosed_links() {
    for source in ["[[Title", "[[Title|label"] {
        let error = parse_profiled(ProfiledParseInput {
            source: &profile(),
            wikitext: source,
        })
        .unwrap_err();
        assert_eq!(error.byte_offset(), 0);
        assert_eq!(error.kind(), &ParseErrorKind::UnclosedInternalLink);
    }

    for source in ["[[A[B]]", "[[A]B]]"] {
        let document = parse_profiled(ProfiledParseInput {
            source: &profile(),
            wikitext: source,
        })
        .unwrap();
        assert!(document.nodes().is_empty());
        assert_eq!(document.rewrite(&RewritePlan::new()).unwrap(), source);
    }

    let wrapped = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: "[[[Title]]]",
    })
    .unwrap();
    assert_eq!(links(&wrapped).len(), 1);
    assert_eq!(wrapped.rewrite(&RewritePlan::new()).unwrap(), "[[[Title]]]");
}

#[test]
fn enforces_profiled_nesting_bound_before_recursing_further() {
    let mut bounded = profile();
    bounded.max_nesting_depth = 1;
    let error = parse_profiled(ProfiledParseInput {
        source: &bounded,
        wikitext: "{{one|{{two|{{three}}}}}}",
    })
    .unwrap_err();
    assert_eq!(error.byte_offset(), 12);
    assert_eq!(
        error.kind(),
        &ParseErrorKind::NestingTooDeep {
            depth: 2,
            max_depth: 1,
        }
    );
}

#[test]
fn decomposes_dynamic_template_and_parameter_names_by_brace_run() {
    let source = "{{{{{selector}}}|x=1}} {{{{{{p}}}}}} {{{{NAMESPACE}}}}";
    let document = parse_profiled(ProfiledParseInput {
        source: &profile(),
        wikitext: source,
    })
    .unwrap();

    let node_kinds = document
        .nodes()
        .iter()
        .map(|node| match node.kind() {
            NodeKind::Template(_) => "template",
            NodeKind::Parameter(_) => "parameter",
            NodeKind::InternalLink(_) => "link",
            NodeKind::Protected(_) => "protected",
        })
        .collect::<Vec<_>>();
    assert_eq!(
        node_kinds,
        vec![
            "template",
            "parameter",
            "parameter",
            "parameter",
            "parameter",
        ]
    );
    assert_eq!(document.nodes()[1].parent(), Some(document.nodes()[0].id()));
    assert_eq!(document.nodes()[3].parent(), Some(document.nodes()[2].id()));
    assert_eq!(document.nodes()[4].parent(), None);
    assert_eq!(document.text(document.nodes()[4].span()), "{{{NAMESPACE}}}");
}

#[test]
fn source_profile_json_is_strict_and_versioned() {
    let mut value = serde_json::to_value(profile()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SourceProfile>(value).is_err());
}
