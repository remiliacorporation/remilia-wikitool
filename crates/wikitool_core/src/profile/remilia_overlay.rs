use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use crate::config::WikiConfig;
use crate::knowledge::status::{DEFAULT_DOCS_PROFILE, KNOWLEDGE_GENERATION};
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, normalize_path, unix_timestamp};

use super::rules::{
    AuthoringRules, CategoryRules, CitationRules, CitationTemplateRule, GoldenSetRules,
    InfoboxPreference, LintRules, ProfileOverlay, ProfileSourceDocument, RemiliaRules,
    UnreliableSourceRule, WikiProfileSnapshot,
};
use super::template_catalog::load_template_catalog;
use super::wiki_capabilities::{
    load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};

const PROFILE_OVERLAY_ARTIFACT_KIND: &str = "profile_overlay";
const PROFILE_OVERLAY_SCHEMA_VERSION: &str = "profile_overlay_v1";
const REMILIA_PROFILE_ID: &str = "remilia";
const MEDIAWIKI_GENERIC_PROFILE_ID: &str = "mediawiki-generic";

const ARTICLE_STRUCTURE_PATH: &str = "tools/wikitool/ai-pack/llm_instructions/article_structure.md";
const STYLE_RULES_PATH: &str = "tools/wikitool/ai-pack/llm_instructions/style_rules.md";
const WRITING_GUIDE_PATH: &str = "tools/wikitool/ai-pack/llm_instructions/writing_guide.md";

pub fn build_remilia_profile_overlay(paths: &ResolvedPaths) -> Result<ProfileOverlay> {
    let (article_structure, article_structure_source) =
        load_source_document(paths, ARTICLE_STRUCTURE_PATH)?;
    let (style_rules, style_rules_source) = load_source_document(paths, STYLE_RULES_PATH)?;
    let (writing_guide, writing_guide_source) = load_source_document(paths, WRITING_GUIDE_PATH)?;

    let preferred_templates = extract_citation_templates(&writing_guide);
    let infobox_preferences = extract_infobox_preferences(&writing_guide);
    let unreliable_sources = merge_unreliable_sources(
        &extract_unreliable_sources(&writing_guide),
        &extract_unreliable_sources(&style_rules),
    );
    let banned_phrases = extract_banned_phrases(&style_rules);
    let watchlist_terms = extract_watchlist_terms(&style_rules);
    let placeholder_fragments = extract_placeholder_fragments(&style_rules);
    let preferred_categories = if writing_guide.contains("[[Category:Remilia]]") {
        vec!["Category:Remilia".to_string()]
    } else {
        Vec::new()
    };

    Ok(ProfileOverlay {
        schema_version: PROFILE_OVERLAY_SCHEMA_VERSION.to_string(),
        profile_id: REMILIA_PROFILE_ID.to_string(),
        base_profile_id: MEDIAWIKI_GENERIC_PROFILE_ID.to_string(),
        docs_profile: DEFAULT_DOCS_PROFILE.to_string(),
        source_documents: vec![
            article_structure_source,
            style_rules_source,
            writing_guide_source,
        ],
        authoring: AuthoringRules {
            require_short_description: article_structure.contains("{{SHORTDESC:"),
            short_description_forms: if article_structure.contains("{{SHORTDESC:") {
                vec!["magic_word:SHORTDESC".to_string()]
            } else {
                Vec::new()
            },
            require_article_quality_banner: article_structure
                .contains("{{Article quality|unverified}}")
                || writing_guide.contains("{{Article quality|unverified}}"),
            article_quality_template: if article_structure
                .contains("{{Article quality|unverified}}")
                || writing_guide.contains("{{Article quality|unverified}}")
            {
                Some("Template:Article quality".to_string())
            } else {
                None
            },
            article_quality_default_state: if article_structure
                .contains("{{Article quality|unverified}}")
                || writing_guide.contains("{{Article quality|unverified}}")
            {
                Some("unverified".to_string())
            } else {
                None
            },
            required_appendix_sections: if article_structure.contains("== References ==") {
                vec!["References".to_string()]
            } else {
                Vec::new()
            },
            references_template: if article_structure.contains("{{Reflist}}")
                || writing_guide.contains("{{Reflist}}")
            {
                Some("Template:Reflist".to_string())
            } else {
                None
            },
            prefer_sentence_case_headings: article_structure.contains("Sentence case")
                || style_rules.contains("Sentence case for headings"),
            prefer_wikitext_only: writing_guide.contains("raw MediaWiki wikitext"),
            forbid_markdown: writing_guide.contains("Never output Markdown"),
            require_straight_quotes: style_rules.contains("Straight quotes only"),
        },
        citations: CitationRules {
            preferred_templates,
            use_named_references: writing_guide.contains("First use:")
                && writing_guide.contains("Later:")
                || style_rules.contains("First use:") && style_rules.contains("Subsequent:"),
            leave_archive_fields_blank: writing_guide.contains("archive fields blank")
                || style_rules.contains("Leave ALL archive fields blank"),
            unreliable_sources,
        },
        remilia: RemiliaRules {
            default_parent_group: if article_structure.contains("parent_group = Remilia")
                || writing_guide.contains("parent_group = Remilia")
            {
                Some("Remilia".to_string())
            } else {
                None
            },
            preferred_group_field: if article_structure.contains("parent_group = Remilia")
                || writing_guide.contains("parent_group = Remilia")
            {
                Some("parent_group".to_string())
            } else {
                None
            },
            avoid_group_fields: if article_structure.contains("creator")
                || writing_guide.contains("creator")
            {
                vec!["creator".to_string(), "artist".to_string()]
            } else {
                Vec::new()
            },
            infobox_preferences,
        },
        categories: CategoryRules {
            preferred_categories,
            min_per_article: if writing_guide.contains("Use 2-4 categories per article") {
                2
            } else {
                0
            },
            max_per_article: if writing_guide.contains("Use 2-4 categories per article") {
                4
            } else {
                0
            },
        },
        lint: LintRules {
            banned_phrases,
            watchlist_terms,
            forbid_curly_quotes: style_rules.contains("any curly quotes"),
            forbid_placeholder_fragments: placeholder_fragments,
        },
        golden_set: GoldenSetRules {
            article_corpus_available: false,
            source_documents: vec![
                ARTICLE_STRUCTURE_PATH.to_string(),
                STYLE_RULES_PATH.to_string(),
                WRITING_GUIDE_PATH.to_string(),
            ],
        },
        refreshed_at: unix_timestamp()?.to_string(),
    })
}

pub fn sync_remilia_profile_overlay(paths: &ResolvedPaths) -> Result<ProfileOverlay> {
    let overlay = build_remilia_profile_overlay(paths)?;
    store_profile_overlay(paths, &overlay)?;
    Ok(overlay)
}

pub fn load_profile_overlay(
    paths: &ResolvedPaths,
    profile_id: &str,
) -> Result<Option<ProfileOverlay>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let overlay_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM knowledge_artifacts
             WHERE artifact_key = ?1",
            params![profile_overlay_artifact_key(profile_id)],
            |row| row.get(0),
        )
        .optional()
        .with_context(|| format!("failed to load profile overlay for {profile_id}"))?;

    overlay_json
        .map(|value| serde_json::from_str(&value).context("failed to decode profile overlay"))
        .transpose()
}

pub fn load_latest_profile_overlay(paths: &ResolvedPaths) -> Result<Option<ProfileOverlay>> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let overlay_json: Option<String> = connection
        .query_row(
            "SELECT metadata_json
             FROM knowledge_artifacts
             WHERE artifact_kind = ?1
             ORDER BY built_at_unix DESC
             LIMIT 1",
            params![PROFILE_OVERLAY_ARTIFACT_KIND],
            |row| row.get(0),
        )
        .optional()
        .context("failed to load latest profile overlay")?;

    overlay_json
        .map(|value| serde_json::from_str(&value).context("failed to decode profile overlay"))
        .transpose()
}

pub fn load_or_build_remilia_profile_overlay(paths: &ResolvedPaths) -> Result<ProfileOverlay> {
    if let Some(overlay) = load_profile_overlay(paths, REMILIA_PROFILE_ID)? {
        return Ok(overlay);
    }
    build_remilia_profile_overlay(paths)
}

pub fn load_wiki_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<WikiProfileSnapshot> {
    let overlay = load_or_build_remilia_profile_overlay(paths)?;
    build_wiki_profile_snapshot(paths, config, overlay, false)
}

pub fn sync_wiki_profile_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<WikiProfileSnapshot> {
    let overlay = sync_remilia_profile_overlay(paths)?;
    build_wiki_profile_snapshot(paths, config, overlay, true)
}

fn build_wiki_profile_snapshot(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    overlay: ProfileOverlay,
    sync_capabilities: bool,
) -> Result<WikiProfileSnapshot> {
    let capabilities = if sync_capabilities {
        if config.api_url_owned().is_some() {
            Some(sync_wiki_capabilities_with_config(paths, config)?)
        } else {
            load_wiki_capabilities_with_config(paths, config)?
        }
    } else {
        load_wiki_capabilities_with_config(paths, config)?
    };
    let template_catalog =
        load_template_catalog(paths, &overlay.profile_id)?.map(|catalog| catalog.summary());

    Ok(WikiProfileSnapshot {
        base_profile_id: overlay.base_profile_id.clone(),
        overlay,
        capabilities,
        template_catalog,
    })
}

fn store_profile_overlay(paths: &ResolvedPaths, overlay: &ProfileOverlay) -> Result<()> {
    let connection = open_initialized_database_connection(&paths.db_path)?;
    let metadata_json =
        serde_json::to_string_pretty(overlay).context("failed to serialize profile overlay")?;
    let row_count = overlay
        .profile_template_titles()
        .len()
        .saturating_add(overlay.lint.banned_phrases.len())
        .saturating_add(overlay.citations.unreliable_sources.len());
    let built_at_unix = unix_timestamp()?;

    connection
        .execute(
            "INSERT INTO knowledge_artifacts (
                artifact_key,
                artifact_kind,
                profile,
                schema_generation,
                built_at_unix,
                row_count,
                metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(artifact_key) DO UPDATE SET
                artifact_kind = excluded.artifact_kind,
                profile = excluded.profile,
                schema_generation = excluded.schema_generation,
                built_at_unix = excluded.built_at_unix,
                row_count = excluded.row_count,
                metadata_json = excluded.metadata_json",
            params![
                profile_overlay_artifact_key(&overlay.profile_id),
                PROFILE_OVERLAY_ARTIFACT_KIND,
                Some(overlay.profile_id.as_str()),
                KNOWLEDGE_GENERATION,
                i64::try_from(built_at_unix).context("artifact timestamp does not fit into i64")?,
                i64::try_from(row_count).context("artifact row count does not fit into i64")?,
                metadata_json,
            ],
        )
        .with_context(|| format!("failed to store profile overlay for {}", overlay.profile_id))?;

    Ok(())
}

fn profile_overlay_artifact_key(profile_id: &str) -> String {
    format!("profile_overlay:{}", profile_id.trim().to_ascii_lowercase())
}

fn load_source_document(
    paths: &ResolvedPaths,
    relative_path: &str,
) -> Result<(String, ProfileSourceDocument)> {
    let path = resolve_source_document_path(paths, relative_path)?;
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok((
        content.clone(),
        ProfileSourceDocument {
            relative_path: normalize_path(path),
            content_hash: compute_hash(&content),
        },
    ))
}

fn resolve_source_document_path(
    paths: &ResolvedPaths,
    relative_path: &str,
) -> Result<std::path::PathBuf> {
    let relative = Path::new(relative_path);
    let file_name = relative
        .file_name()
        .context("profile source document path is missing a file name")?;
    let mut candidates = Vec::new();
    candidates.push(paths.project_root.join(relative));
    candidates.push(paths.project_root.join("llm_instructions").join(file_name));

    if let Ok(executable) = env::current_exe() {
        for ancestor in executable.ancestors().take(8) {
            candidates.push(ancestor.join(relative));
            candidates.push(ancestor.join("ai-pack/llm_instructions").join(file_name));
            candidates.push(ancestor.join("llm_instructions").join(file_name));
        }
    }

    candidates.sort();
    candidates.dedup();
    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let fallback = paths.project_root.join(relative);
    Err(anyhow::anyhow!("failed to read {}", fallback.display()))
}

fn extract_citation_templates(content: &str) -> Vec<CitationTemplateRule> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("{{Cite ") {
            continue;
        }
        let Some(end) = trimmed.find('|') else {
            continue;
        };
        let template_name = collapse_whitespace(&trimmed[2..end].replace('_', " "));
        let family = template_name
            .strip_prefix("Cite ")
            .unwrap_or(&template_name)
            .to_ascii_lowercase();
        if seen.insert(family.clone()) {
            out.push(CitationTemplateRule {
                family,
                template_title: format!("Template:{template_name}"),
            });
        }
    }
    out
}

fn extract_infobox_preferences(content: &str) -> Vec<InfoboxPreference> {
    let Some(section) = extract_markdown_section(content, "## 6. Infobox selection") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for row in section.lines() {
        let trimmed = row.trim();
        if !trimmed.starts_with('|') || trimmed.contains("Subject type") {
            continue;
        }
        let cells = split_markdown_row(trimmed);
        if cells.len() < 2 || cells[0].starts_with("---") {
            continue;
        }
        let Some(template_title) = extract_template_title(&cells[1]) else {
            continue;
        };
        out.push(InfoboxPreference {
            subject_type: collapse_whitespace(&cells[0]),
            template_title,
        });
    }
    out
}

fn extract_unreliable_sources(content: &str) -> Vec<UnreliableSourceRule> {
    let mut out = Vec::new();
    for heading in ["### Never cite:", "## VI. Unreliable sources"] {
        let Some(section) = extract_markdown_section(content, heading) else {
            continue;
        };
        for line in section.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("- ") {
                continue;
            }
            let item = trimmed.trim_start_matches("- ").trim();
            let (label, matcher) = parse_source_rule(item);
            if !label.is_empty() {
                out.push(UnreliableSourceRule { label, matcher });
            }
        }
    }
    out
}

fn merge_unreliable_sources(
    left: &[UnreliableSourceRule],
    right: &[UnreliableSourceRule],
) -> Vec<UnreliableSourceRule> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for rule in left.iter().chain(right.iter()) {
        let key = rule.matcher.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(rule.clone());
        }
    }
    out
}

fn extract_banned_phrases(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut capture = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "**Never use:**" {
            capture = true;
            continue;
        }
        if capture && trimmed.starts_with("### ") {
            capture = false;
        }
        if !capture || !trimmed.starts_with("- ") {
            continue;
        }
        for phrase in extract_quoted_fragments(trimmed) {
            let key = phrase.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(phrase);
            }
        }
    }
    out
}

fn extract_watchlist_terms(content: &str) -> Vec<String> {
    let Some(section) =
        extract_markdown_section(content, "**Watch list — use sparingly or not at all:**")
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- ") {
            continue;
        }
        for term in split_csv_terms(trimmed.trim_start_matches("- ")) {
            let key = term.to_ascii_lowercase();
            if !term.is_empty() && seen.insert(key) {
                out.push(term);
            }
        }
    }
    out
}

fn extract_placeholder_fragments(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for heading in ["### No placeholder content", "### No system artifacts"] {
        let Some(section) = extract_markdown_section(content, heading) else {
            continue;
        };
        for fragment in extract_backticked_fragments(section) {
            let key = fragment.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(fragment);
            }
        }
    }
    out
}

fn parse_source_rule(value: &str) -> (String, String) {
    let label = value
        .split('—')
        .next()
        .map(collapse_whitespace)
        .unwrap_or_default();
    let matcher = if let Some(start) = value.find('(') {
        if let Some(end_rel) = value[start + 1..].find(')') {
            collapse_whitespace(&value[start + 1..start + 1 + end_rel])
        } else {
            label.to_ascii_lowercase()
        }
    } else {
        label.to_ascii_lowercase()
    };
    (label, matcher)
}

fn extract_template_title(value: &str) -> Option<String> {
    let start = value.find("{{")?;
    let end_rel = value[start + 2..].find("}}")?;
    let name = collapse_whitespace(&value[start + 2..start + 2 + end_rel].replace('_', " "));
    if name.is_empty() {
        None
    } else if name.starts_with("Template:") {
        Some(name)
    } else {
        Some(format!("Template:{name}"))
    }
}

fn extract_quoted_fragments(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = None;
    for (index, ch) in value.char_indices() {
        if ch == '"' {
            if let Some(open) = start.take() {
                let fragment = collapse_whitespace(&value[open..index]);
                if !fragment.is_empty() {
                    out.push(fragment);
                }
            } else {
                start = Some(index + ch.len_utf8());
            }
        }
    }
    out
}

fn extract_backticked_fragments(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = None;
    for (index, ch) in value.char_indices() {
        if ch == '`' {
            if let Some(open) = start.take() {
                let fragment = collapse_whitespace(&value[open..index]);
                if !fragment.is_empty() {
                    out.push(fragment);
                }
            } else {
                start = Some(index + ch.len_utf8());
            }
        }
    }
    out
}

fn split_csv_terms(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(collapse_whitespace)
        .filter(|item| !item.is_empty())
        .collect()
}

fn split_markdown_row(value: &str) -> Vec<String> {
    value
        .trim_matches('|')
        .split('|')
        .map(|cell| collapse_whitespace(cell.trim_matches('`')))
        .collect()
}

fn extract_markdown_section<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    let mut offset = 0usize;
    let mut start = None;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(section_start) = start {
            if trimmed.starts_with('#') {
                return Some(&content[section_start..offset]);
            }
        } else if trimmed == heading {
            start = Some(offset + line.len());
        }
        offset += line.len();
    }
    start.map(|section_start| &content[section_start..])
}

fn collapse_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut last_was_space = false;
    for ch in value.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::runtime::ResolvedPaths;

    use super::{
        ARTICLE_STRUCTURE_PATH, STYLE_RULES_PATH, WRITING_GUIDE_PATH,
        build_remilia_profile_overlay, extract_banned_phrases, extract_citation_templates,
        extract_infobox_preferences, extract_placeholder_fragments,
    };

    #[test]
    fn citation_templates_and_infobox_preferences_are_extracted() {
        let guide = r#"
### Citation templates
```wikitext
{{Cite web|url=}}
{{Cite tweet|user=}}
```

## 6. Infobox selection
| Subject type | Infobox |
|---|---|
| Person | `{{Infobox person}}` |
| Website/Platform | `{{Infobox website}}` |
"#;

        let templates = extract_citation_templates(guide);
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].template_title, "Template:Cite web");

        let infoboxes = extract_infobox_preferences(guide);
        assert_eq!(infoboxes.len(), 2);
        assert_eq!(infoboxes[0].template_title, "Template:Infobox person");
    }

    #[test]
    fn phrase_and_placeholder_extractors_capture_expected_rules() {
        let style = r#"
**Never use:**
- "stands as", "rich tapestry"

### No placeholder content
- Never output: `[Author Name]`, `INSERT_SOURCE_URL`
"#;

        assert_eq!(
            extract_banned_phrases(style),
            vec!["stands as".to_string(), "rich tapestry".to_string()]
        );
        assert_eq!(
            extract_placeholder_fragments(style),
            vec!["[Author Name]".to_string(), "INSERT_SOURCE_URL".to_string()]
        );
    }

    #[test]
    fn remilia_overlay_builds_from_local_instruction_files() {
        let temp = tempdir().expect("tempdir");
        let project_root = temp.path().join("project");
        fs::create_dir_all(project_root.join("wiki_content/Main")).expect("wiki content");
        fs::create_dir_all(project_root.join("templates")).expect("templates");
        fs::create_dir_all(project_root.join(".wikitool/data")).expect("data");
        fs::create_dir_all(project_root.join("tools/wikitool/ai-pack/llm_instructions"))
            .expect("instructions");

        fs::write(
            project_root.join(ARTICLE_STRUCTURE_PATH),
            "{{SHORTDESC:Example}}\n{{Article quality|unverified}}\n== References ==\n{{Reflist}}\nparent_group = Remilia",
        )
        .expect("article structure");
        fs::write(
            project_root.join(STYLE_RULES_PATH),
            "**Never use:**\n- \"stands as\", \"rich tapestry\"\n### No system artifacts\n- Never output: `contentReference[oaicite:0]`",
        )
        .expect("style rules");
        fs::write(
            project_root.join(WRITING_GUIDE_PATH),
            "raw MediaWiki wikitext\nNever output Markdown\nUse 2-4 categories per article\n[[Category:Remilia]]\n{{Article quality|unverified}}\n### Citation templates\n```wikitext\n{{Cite web|url=}}\n```\n## 6. Infobox selection\n| Subject type | Infobox |\n|---|---|\n| Person | `{{Infobox person}}` |\n",
        )
        .expect("writing guide");

        let paths = ResolvedPaths {
            project_root: project_root.clone(),
            wiki_content_dir: project_root.join("wiki_content"),
            templates_dir: project_root.join("templates"),
            state_dir: project_root.join(".wikitool"),
            data_dir: project_root.join(".wikitool/data"),
            db_path: project_root.join(".wikitool/data/wikitool.db"),
            config_path: project_root.join(".wikitool/config.toml"),
            parser_config_path: project_root.join(".wikitool/parser-config.json"),
            root_source: crate::runtime::ValueSource::Default,
            data_source: crate::runtime::ValueSource::Default,
            config_source: crate::runtime::ValueSource::Default,
        };

        let overlay = build_remilia_profile_overlay(&paths).expect("overlay");
        assert_eq!(overlay.profile_id, "remilia");
        assert!(overlay.authoring.require_short_description);
        assert_eq!(
            overlay.citations.preferred_templates[0].template_title,
            "Template:Cite web"
        );
        assert_eq!(
            overlay.remilia.infobox_preferences[0].template_title,
            "Template:Infobox person"
        );
    }
}
