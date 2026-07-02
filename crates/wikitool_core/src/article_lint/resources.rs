use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use rusqlite::Connection;

use crate::content_store::parsing::open_indexed_connection;
use crate::filesystem::{ScanOptions, scan_files};
use crate::profile::{
    ProfileOverlay, TemplateCatalog, WikiCapabilityManifest, build_template_catalog_with_overlay,
    load_latest_wiki_capabilities, load_or_build_remilia_profile_overlay, scan_local_asset_titles,
    scan_local_module_functions, scan_local_module_titles,
};
use crate::runtime::ResolvedPaths;

#[derive(Debug)]
pub(super) struct LoadedResources {
    pub(super) overlay: ProfileOverlay,
    pub(super) capabilities: Option<WikiCapabilityManifest>,
    pub(super) template_catalog: Option<TemplateCatalog>,
    pub(super) local_module_titles: BTreeSet<String>,
    pub(super) local_module_functions: BTreeMap<String, BTreeSet<String>>,
    pub(super) local_asset_titles: BTreeSet<String>,
    /// Lowercased single words drawn from local page/template titles and the profile's
    /// configured proper nouns. The sentence-case heading rule treats these as proper
    /// nouns that may stay capitalized mid-heading.
    pub(super) proper_noun_words: BTreeSet<String>,
    pub(super) index_connection: Option<Connection>,
}

pub(super) fn load_resources(paths: &ResolvedPaths) -> Result<LoadedResources> {
    let overlay = load_or_build_remilia_profile_overlay(paths)?;

    let capabilities = if paths.db_path.exists() {
        load_latest_wiki_capabilities(paths)?
    } else {
        None
    };
    let template_catalog = {
        let built = build_template_catalog_with_overlay(paths, &overlay)?;
        if built.entries.is_empty() {
            None
        } else {
            Some(built)
        }
    };
    let local_module_titles = scan_local_module_titles(paths)?;
    let local_module_functions = scan_local_module_functions(paths)?;
    let local_asset_titles = scan_local_asset_titles(paths)?;
    let proper_noun_words = build_proper_noun_words(paths, &overlay)?;
    let index_connection = open_indexed_connection(paths)?;

    Ok(LoadedResources {
        overlay,
        capabilities,
        template_catalog,
        local_module_titles,
        local_module_functions,
        local_asset_titles,
        proper_noun_words,
        index_connection,
    })
}

/// Build the proper-noun vocabulary the sentence-case rule consults. Sources, in order:
/// the profile's configured `proper_nouns`, then local main/template titles. Title-derived
/// words are intentionally narrower than profile terms: a MediaWiki title's first word is
/// capitalized by convention, so it is not enough by itself to prove proper-noun casing.
fn build_proper_noun_words(
    paths: &ResolvedPaths,
    overlay: &ProfileOverlay,
) -> Result<BTreeSet<String>> {
    let mut words = BTreeSet::new();
    for term in &overlay.lint.proper_nouns {
        insert_profile_proper_noun_words(&mut words, term);
    }
    let files = scan_files(
        paths,
        &ScanOptions {
            include_content: true,
            include_templates: true,
            custom_content_folders: Vec::new(),
        },
    )?;
    for file in files {
        if file.is_redirect {
            continue;
        }
        if !matches!(file.namespace.as_str(), "Main" | "Template") {
            continue;
        }
        insert_title_proper_noun_words(&mut words, &file.title);
    }
    Ok(words)
}

fn insert_profile_proper_noun_words(words: &mut BTreeSet<String>, phrase: &str) {
    for raw in phrase.split_whitespace() {
        insert_cleaned_proper_noun_word(words, raw);
    }
}

fn insert_title_proper_noun_words(words: &mut BTreeSet<String>, title: &str) {
    let bare_title = title_without_namespace(title);
    let tokens = bare_title
        .split_whitespace()
        .filter_map(title_word)
        .collect::<Vec<_>>();
    let non_stopword_count = tokens.iter().filter(|token| !token.is_stopword).count();
    let proper_candidate_count = tokens
        .iter()
        .filter(|token| !token.is_stopword && token.looks_proper)
        .count();
    let has_lowercase_title_word = tokens
        .iter()
        .any(|token| !token.is_stopword && !token.looks_proper);

    for (index, token) in tokens.iter().enumerate() {
        if token.is_stopword || !token.looks_proper {
            continue;
        }
        let first_meaningful = tokens[..index].iter().all(|previous| previous.is_stopword);
        if first_meaningful
            && non_stopword_count > 1
            && (proper_candidate_count < 2 || has_lowercase_title_word)
        {
            continue;
        }
        words.insert(token.cleaned.clone());
    }
}

struct TitleWord {
    cleaned: String,
    is_stopword: bool,
    looks_proper: bool,
}

fn title_word(raw: &str) -> Option<TitleWord> {
    let cleaned = cleaned_proper_noun_word(raw)?;
    Some(TitleWord {
        is_stopword: is_heading_stopword(&cleaned),
        looks_proper: looks_like_title_proper_noun(raw),
        cleaned,
    })
}

fn insert_cleaned_proper_noun_word(words: &mut BTreeSet<String>, raw: &str) {
    let Some(cleaned) = cleaned_proper_noun_word(raw) else {
        return;
    };
    if !is_heading_stopword(&cleaned) {
        words.insert(cleaned);
    }
}

fn cleaned_proper_noun_word(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if cleaned.chars().count() < 3 {
        return None;
    }
    if !cleaned.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(cleaned)
}

fn looks_like_title_proper_noun(raw: &str) -> bool {
    let letters = raw
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .collect::<Vec<_>>();
    let Some(first) = letters.first() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    letters.iter().skip(1).all(|ch| ch.is_ascii_lowercase())
        || letters.iter().all(|ch| ch.is_ascii_uppercase())
        || letters.iter().skip(1).any(|ch| ch.is_ascii_uppercase())
}

fn title_without_namespace(title: &str) -> &str {
    title.split_once(':').map(|(_, rest)| rest).unwrap_or(title)
}

fn is_heading_stopword(word: &str) -> bool {
    matches!(
        word,
        "and" | "the" | "for" | "with" | "from" | "into" | "onto" | "over"
    )
}
