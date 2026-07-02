use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};

use crate::config::WikiConfig;
use crate::knowledge::status::record_docs_profile_artifact;
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::{compute_hash, unix_timestamp};

mod catalog;
mod fetch;
mod import;
mod model;
mod parse;
mod persist;
mod profiles;
mod query;
mod support;

pub use fetch::{
    DocsApi, DocsClientConfig, MediaWikiDocsClient, RemoteDocsPage,
    discover_installed_extensions_from_wiki, discover_installed_extensions_from_wiki_with_config,
    is_transient_docs_error,
};
#[cfg(test)]
use model::DOCS_NAMESPACE_MANUAL;
use model::{
    CorpusDescriptor, DOCS_BUNDLE_SCHEMA_VERSION, DOCS_CACHE_TTL_SECONDS, DOCS_NAMESPACE_EXTENSION,
    DOCS_PROFILES, DocsProfileDefinition, ExtensionRefreshSpec, FetchedDocsPage, PersistStats,
    ProfileRefreshSpec, StaticRefreshSpec, TechnicalRefreshSpec,
};
pub use model::{
    DocsBundle, DocsBundleExtension, DocsBundleImportReport, DocsBundlePage, DocsBundleTechnical,
    DocsContextExample, DocsContextOptions, DocsContextReport, DocsContextSection,
    DocsCorpusSummary, DocsImportOptions, DocsImportProfileOptions, DocsImportProfileReport,
    DocsImportReport, DocsImportTechnicalOptions, DocsImportTechnicalReport, DocsListOptions,
    DocsListReport, DocsOutdatedCorpus, DocsOutdatedReport, DocsRemoveKind, DocsRemoveReport,
    DocsSearchHit, DocsSearchOptions, DocsStats, DocsSymbolHit, DocsSymbolLookupOptions,
    DocsUpdateReport, TechnicalDocType, TechnicalImportTask,
};
pub use query::{build_docs_context, lookup_docs_symbols, search_docs};
use support::{
    OutdatedRefreshRow, cleanup_empty_corpora, count_query, dedupe_titles_in_order,
    deserialize_string_list, extension_corpus_id, extension_local_path, filter_archive_titles,
    filter_translation_titles, infer_doc_type_from_title, normalize_corpus_kind_filter,
    normalize_extension_list, normalize_extension_name, normalize_extensions, open_docs_connection,
    profile_corpus_id, rebuild_docs_fts_indexes, serialize_string_list, technical_corpus_id,
    technical_local_path,
};

use catalog::{load_docs_corpora, load_docs_stats, load_outdated_docs, load_outdated_refresh_rows};
use import::{
    import_extension_docs_with_api_internal, import_extension_docs_with_api_internal_deferred,
    import_technical_docs_with_api_internal, import_technical_docs_with_api_internal_deferred,
};
use parse::{
    DocsPageParseInput, ParsedDocsExample, ParsedDocsSection, ParsedDocsSymbol, estimate_tokens,
    is_translation_variant, normalize_retrieval_key, normalize_title, parse_docs_page,
};
use persist::{accumulate_stats, persist_docs_corpus};
use profiles::{collect_profile_pages, resolve_docs_profile};
pub fn import_docs_bundle(
    paths: &ResolvedPaths,
    bundle_path: &Path,
) -> Result<DocsBundleImportReport> {
    let bundle_data = fs::read_to_string(bundle_path)
        .with_context(|| format!("failed to read docs bundle {}", bundle_path.display()))?;
    let bundle: DocsBundle =
        serde_json::from_str(&bundle_data).context("failed to parse docs bundle JSON")?;
    if bundle.schema_version != DOCS_BUNDLE_SCHEMA_VERSION {
        bail!(
            "unsupported docs bundle schema version {} (expected {})",
            bundle.schema_version,
            DOCS_BUNDLE_SCHEMA_VERSION
        );
    }

    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let source = bundle
        .source
        .clone()
        .unwrap_or_else(|| "precomposed_bundle".to_string());

    let mut imported_extensions = 0usize;
    let mut imported_technical_types = 0usize;
    let mut stats = PersistStats::default();
    let mut failures = Vec::new();

    for extension in &bundle.extensions {
        let extension_name = normalize_extension_name(&extension.extension_name);
        if extension_name.is_empty() {
            failures.push("bundle extension entry with empty extension_name".to_string());
            continue;
        }
        let mut pages = Vec::new();
        for page in &extension.pages {
            let page_title = normalize_title(&page.page_title);
            if page_title.is_empty() || page.content.trim().is_empty() {
                continue;
            }
            let local_path = page
                .local_path
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| extension_local_path(&extension_name, &page_title));
            pages.push(FetchedDocsPage {
                page_title,
                alias_titles: Vec::new(),
                local_path,
                content: page.content.clone(),
            });
        }
        if pages.is_empty() {
            failures.push(format!(
                "{extension_name}: bundle entry has no usable pages"
            ));
            continue;
        }
        let descriptor = CorpusDescriptor {
            corpus_id: extension_corpus_id(&extension_name, ""),
            corpus_kind: "extension".to_string(),
            label: format!("Extension:{extension_name}"),
            source_wiki: extension
                .source_wiki
                .clone()
                .unwrap_or_else(|| source.clone()),
            source_version: extension.version.clone().unwrap_or_default(),
            source_profile: String::new(),
            technical_type: String::new(),
            refresh_kind: "static".to_string(),
            refresh_spec: serde_json::to_string(&StaticRefreshSpec {
                source: source.clone(),
            })?,
            fetched_at_unix: now_unix,
            expires_at_unix,
        };
        let persisted = persist_docs_corpus(paths, &descriptor, &pages)?;
        imported_extensions += 1;
        accumulate_stats(&mut stats, &persisted);
    }

    for technical in &bundle.technical {
        let Some(doc_type) = TechnicalDocType::parse(&technical.doc_type) else {
            failures.push(format!(
                "bundle technical entry has unsupported doc_type `{}`",
                technical.doc_type
            ));
            continue;
        };
        let mut pages = Vec::new();
        for page in &technical.pages {
            let page_title = normalize_title(&page.page_title);
            if page_title.is_empty() || page.content.trim().is_empty() {
                continue;
            }
            let local_path = page
                .local_path
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| technical_local_path(doc_type, &page_title));
            pages.push(FetchedDocsPage {
                page_title,
                alias_titles: Vec::new(),
                local_path,
                content: page.content.clone(),
            });
        }
        if pages.is_empty() {
            continue;
        }
        let descriptor = CorpusDescriptor {
            corpus_id: technical_corpus_id(doc_type, None, ""),
            corpus_kind: "technical".to_string(),
            label: doc_type.main_page().to_string(),
            source_wiki: source.clone(),
            source_version: String::new(),
            source_profile: String::new(),
            technical_type: doc_type.as_str().to_string(),
            refresh_kind: "static".to_string(),
            refresh_spec: serde_json::to_string(&StaticRefreshSpec {
                source: source.clone(),
            })?,
            fetched_at_unix: now_unix,
            expires_at_unix,
        };
        let persisted = persist_docs_corpus(paths, &descriptor, &pages)?;
        imported_technical_types += 1;
        accumulate_stats(&mut stats, &persisted);
    }

    rebuild_docs_fts_indexes(paths)?;

    Ok(DocsBundleImportReport {
        schema_version: bundle.schema_version,
        source,
        imported_extensions,
        imported_technical_types,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        failures,
    })
}

pub fn import_extension_docs(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
) -> Result<DocsImportReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_extension_docs_with_api(paths, options, &mut api)
}

pub fn import_extension_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportOptions,
    api: &mut A,
) -> Result<DocsImportReport> {
    import_extension_docs_with_api_internal(paths, options, api, "", "")
}

pub fn import_technical_docs(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
) -> Result<DocsImportTechnicalReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_technical_docs_with_api(paths, options, &mut api)
}

pub fn import_technical_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportTechnicalOptions,
    api: &mut A,
) -> Result<DocsImportTechnicalReport> {
    import_technical_docs_with_api_internal(paths, options, api, "", "")
}

pub fn import_docs_profile(
    paths: &ResolvedPaths,
    options: &DocsImportProfileOptions,
) -> Result<DocsImportProfileReport> {
    import_docs_profile_with_config(paths, options, &WikiConfig::default())
}

pub fn import_docs_profile_with_config(
    paths: &ResolvedPaths,
    options: &DocsImportProfileOptions,
    config: &WikiConfig,
) -> Result<DocsImportProfileReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    import_docs_profile_with_api(paths, options, config, &mut api)
}

pub fn import_docs_profile_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportProfileOptions,
    config: &WikiConfig,
    api: &mut A,
) -> Result<DocsImportProfileReport> {
    import_docs_profile_with_api_internal(paths, options, config, api, true)
}

fn import_docs_profile_with_api_internal<A: DocsApi>(
    paths: &ResolvedPaths,
    options: &DocsImportProfileOptions,
    config: &WikiConfig,
    api: &mut A,
    rebuild_fts: bool,
) -> Result<DocsImportProfileReport> {
    let definition = resolve_docs_profile(&options.profile)?;
    let now_unix = unix_timestamp()?;
    let expires_at_unix = now_unix.saturating_add(DOCS_CACHE_TTL_SECONDS);
    let include_installed_extensions =
        options.include_installed_extensions || definition.include_installed_extensions_by_default;

    let profile_pages = collect_profile_pages(api, definition, options.limit)?;
    let descriptor = CorpusDescriptor {
        corpus_id: profile_corpus_id(definition.id),
        corpus_kind: "profile".to_string(),
        label: definition.label.to_string(),
        source_wiki: "mediawiki.org".to_string(),
        source_version: definition.source_version.to_string(),
        source_profile: definition.id.to_string(),
        technical_type: "profile".to_string(),
        refresh_kind: "profile".to_string(),
        refresh_spec: serde_json::to_string(&ProfileRefreshSpec {
            profile: definition.id.to_string(),
            include_installed_extensions,
            include_extension_subpages: options.include_extension_subpages,
            extra_extensions: options.extra_extensions.clone(),
            limit: options.limit.max(1),
        })?,
        fetched_at_unix: now_unix,
        expires_at_unix,
    };
    let mut stats = persist_docs_corpus(paths, &descriptor, &profile_pages)?;
    let mut failures = Vec::new();
    let mut imported_extensions = 0usize;

    let mut extension_names = definition
        .extension_seeds
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    extension_names.extend(options.extra_extensions.clone());
    if include_installed_extensions {
        match discover_installed_extensions_from_wiki_with_config(config) {
            Ok(discovered) => extension_names.extend(discovered),
            Err(error) => failures.push(format!(
                "profile {}: installed extension discovery skipped: {error}",
                definition.id
            )),
        }
    }
    normalize_extension_list(&mut extension_names);

    if !extension_names.is_empty() {
        let extension_report = import_extension_docs_with_api_internal_deferred(
            paths,
            &DocsImportOptions {
                extensions: extension_names,
                include_subpages: options.include_extension_subpages,
            },
            api,
            definition.id,
            definition.source_version,
        )?;
        imported_extensions = extension_report.imported_extensions;
        stats.pages += extension_report.imported_pages;
        stats.sections += extension_report.imported_sections;
        stats.symbols += extension_report.imported_symbols;
        stats.examples += extension_report.imported_examples;
        failures.extend(extension_report.failures);
    }

    if rebuild_fts && stats.pages > 0 {
        rebuild_docs_fts_indexes(paths)?;
    }
    let connection = open_initialized_database_connection(&paths.db_path)?;
    record_docs_profile_artifact(
        &connection,
        definition.id,
        stats.pages,
        &serde_json::json!({
            "profile": definition.id,
            "imported_corpora": 1 + imported_extensions,
            "imported_extensions": imported_extensions,
            "imported_pages": stats.pages,
            "imported_sections": stats.sections,
            "imported_symbols": stats.symbols,
            "imported_examples": stats.examples,
            "failures": failures.clone(),
        })
        .to_string(),
    )?;

    Ok(DocsImportProfileReport {
        profile: definition.id.to_string(),
        imported_corpora: 1 + imported_extensions,
        imported_extensions,
        imported_pages: stats.pages,
        imported_sections: stats.sections,
        imported_symbols: stats.symbols,
        imported_examples: stats.examples,
        failures,
        request_count: api.request_count(),
    })
}

pub fn list_docs(paths: &ResolvedPaths, options: &DocsListOptions) -> Result<DocsListReport> {
    let connection = open_docs_connection(paths)?;
    let now_unix = unix_timestamp()?;
    let stats = load_docs_stats(&connection)?;
    let corpora = load_docs_corpora(
        &connection,
        options.corpus_kind.as_deref(),
        options.technical_type.as_deref(),
        options.profile.as_deref(),
        now_unix,
    )?;
    let outdated = load_outdated_docs(&connection, now_unix)?;

    Ok(DocsListReport {
        now_unix,
        stats,
        corpora,
        outdated,
    })
}

pub fn update_outdated_docs(paths: &ResolvedPaths) -> Result<DocsUpdateReport> {
    update_outdated_docs_with_config(paths, &WikiConfig::default())
}

pub fn update_outdated_docs_with_config(
    paths: &ResolvedPaths,
    config: &WikiConfig,
) -> Result<DocsUpdateReport> {
    let mut api = MediaWikiDocsClient::from_env()?;
    update_outdated_docs_with_api(paths, config, &mut api)
}

pub fn update_outdated_docs_with_api<A: DocsApi>(
    paths: &ResolvedPaths,
    config: &WikiConfig,
    api: &mut A,
) -> Result<DocsUpdateReport> {
    let refresh_rows = load_outdated_refresh_rows(paths)?;
    let mut updated_corpora = 0usize;
    let mut updated_pages = 0usize;
    let mut updated_sections = 0usize;
    let mut updated_symbols = 0usize;
    let mut updated_examples = 0usize;
    let mut failures = Vec::new();
    let mut needs_fts_rebuild = false;

    for row in refresh_rows {
        match row.refresh_kind.as_str() {
            "extension" => {
                let spec: ExtensionRefreshSpec = serde_json::from_str(&row.refresh_spec)
                    .context("invalid extension refresh spec")?;
                match import_extension_docs_with_api_internal_deferred(
                    paths,
                    &DocsImportOptions {
                        extensions: vec![spec.extension_name.clone()],
                        include_subpages: spec.include_subpages,
                    },
                    api,
                    &spec.source_profile,
                    &spec.source_version,
                ) {
                    Ok(report) => {
                        if report.imported_extensions > 0 {
                            needs_fts_rebuild = true;
                            updated_corpora += report.imported_extensions;
                            updated_pages += report.imported_pages;
                            updated_sections += report.imported_sections;
                            updated_symbols += report.imported_symbols;
                            updated_examples += report.imported_examples;
                        }
                        failures.extend(report.failures);
                    }
                    Err(error) => failures.push(format!("{}: {error}", row.label)),
                }
            }
            "technical" => {
                let spec: TechnicalRefreshSpec = serde_json::from_str(&row.refresh_spec)
                    .context("invalid technical refresh spec")?;
                let Some(doc_type) = TechnicalDocType::parse(&spec.doc_type) else {
                    failures.push(format!(
                        "{}: unsupported technical doc type {}",
                        row.label, spec.doc_type
                    ));
                    continue;
                };
                match import_technical_docs_with_api_internal_deferred(
                    paths,
                    &DocsImportTechnicalOptions {
                        tasks: vec![TechnicalImportTask {
                            doc_type,
                            page_title: spec.page_title.clone(),
                            include_subpages: spec.include_subpages,
                        }],
                        limit: spec.limit.max(1),
                    },
                    api,
                    &spec.source_profile,
                    &spec.source_version,
                ) {
                    Ok(report) => {
                        if report.imported_corpora > 0 {
                            needs_fts_rebuild = true;
                            updated_corpora += report.imported_corpora;
                            updated_pages += report.imported_pages;
                            updated_sections += report.imported_sections;
                            updated_symbols += report.imported_symbols;
                            updated_examples += report.imported_examples;
                        }
                        failures.extend(report.failures);
                    }
                    Err(error) => failures.push(format!("{}: {error}", row.label)),
                }
            }
            "profile" => {
                let spec: ProfileRefreshSpec = serde_json::from_str(&row.refresh_spec)
                    .context("invalid profile refresh spec")?;
                match import_docs_profile_with_api_internal(
                    paths,
                    &DocsImportProfileOptions {
                        profile: spec.profile.clone(),
                        include_installed_extensions: spec.include_installed_extensions,
                        include_extension_subpages: spec.include_extension_subpages,
                        extra_extensions: spec.extra_extensions.clone(),
                        limit: spec.limit.max(1),
                    },
                    config,
                    api,
                    false,
                ) {
                    Ok(report) => {
                        if report.imported_corpora > 0 {
                            needs_fts_rebuild = true;
                        }
                        updated_corpora += report.imported_corpora;
                        updated_pages += report.imported_pages;
                        updated_sections += report.imported_sections;
                        updated_symbols += report.imported_symbols;
                        updated_examples += report.imported_examples;
                        failures.extend(report.failures);
                    }
                    Err(error) => failures.push(format!("{}: {error}", row.label)),
                }
            }
            _ => {}
        }
    }

    if needs_fts_rebuild {
        rebuild_docs_fts_indexes(paths)?;
    }

    Ok(DocsUpdateReport {
        updated_corpora,
        updated_pages,
        updated_sections,
        updated_symbols,
        updated_examples,
        failures,
        request_count: api.request_count(),
    })
}

pub fn remove_docs(paths: &ResolvedPaths, target: &str) -> Result<DocsRemoveReport> {
    let connection = open_docs_connection(paths)?;
    let normalized_target = normalize_title(target);
    if normalized_target.is_empty() {
        bail!("docs remove target is empty");
    }

    if let Some(doc_type) = TechnicalDocType::parse(&normalized_target) {
        let removed = connection.execute(
            "DELETE FROM docs_corpora WHERE corpus_kind = 'technical' AND technical_type = ?1",
            params![doc_type.as_str()],
        )?;
        if removed > 0 {
            rebuild_docs_fts_indexes(paths)?;
            return Ok(DocsRemoveReport {
                kind: DocsRemoveKind::TechnicalType,
                target: normalized_target,
                removed_rows: removed,
            });
        }
    }

    let removed = connection.execute(
        "DELETE FROM docs_corpora
         WHERE lower(corpus_id) = lower(?1)
            OR lower(label) = lower(?1)
            OR lower(replace(label, 'Extension:', '')) = lower(?1)",
        params![normalized_target],
    )?;
    if removed > 0 {
        rebuild_docs_fts_indexes(paths)?;
        return Ok(DocsRemoveReport {
            kind: DocsRemoveKind::Corpus,
            target: normalized_target,
            removed_rows: removed,
        });
    }

    let removed_pages = connection.execute(
        "DELETE FROM docs_pages WHERE lower(page_title) = lower(?1)",
        params![normalized_target],
    )?;
    if removed_pages > 0 {
        rebuild_docs_fts_indexes(paths)?;
        cleanup_empty_corpora(&connection)?;
        return Ok(DocsRemoveReport {
            kind: DocsRemoveKind::Page,
            target: normalized_target,
            removed_rows: removed_pages,
        });
    }

    Ok(DocsRemoveReport {
        kind: DocsRemoveKind::NotFound,
        target: normalized_target,
        removed_rows: 0,
    })
}

pub fn format_expiration(now_unix: u64, expires_at_unix: u64) -> String {
    if expires_at_unix <= now_unix {
        return "expired".to_string();
    }
    let delta = expires_at_unix - now_unix;
    let day = 24 * 60 * 60;
    let hour = 60 * 60;
    if delta >= day {
        let days = delta / day;
        return format!("expires in {days} day{}", if days == 1 { "" } else { "s" });
    }
    if delta >= hour {
        let hours = delta / hour;
        return format!(
            "expires in {hours} hour{}",
            if hours == 1 { "" } else { "s" }
        );
    }
    "expires soon".to_string()
}

#[cfg(test)]
mod tests;
