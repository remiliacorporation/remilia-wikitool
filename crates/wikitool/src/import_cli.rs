use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use mediawiki_html_to_wikitext::{
    Coverage, MediaReference, ProfiledCompileInput, SourceProfile, TargetProfile,
    UnmappedStructure, compile_profiled,
};
use serde::{Deserialize, Serialize};
use wikitool_core::import_cargo::{
    CargoImportOptions, ImportError, ImportPageResult, ImportSourceType, ImportUpdateMode,
    import_to_cargo,
};

use crate::cli_support::{
    OutputFormat, format_flag, normalize_option, normalize_path, resolve_runtime_paths,
};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

#[derive(Debug, Args)]
pub(crate) struct ImportArgs {
    #[command(subcommand)]
    command: ImportSubcommand,
}

#[derive(Debug, Subcommand)]
enum ImportSubcommand {
    Cargo {
        path: String,
        #[arg(long, value_name = "NAME", help = "Cargo table name")]
        table: String,
        #[arg(long, value_enum, value_name = "TYPE", help = "Input type: csv|json")]
        r#type: Option<ImportSourceTypeArg>,
        #[arg(long, value_name = "NAME", help = "Template wrapper name")]
        template: Option<String>,
        #[arg(long, value_name = "FIELD", help = "Field name to use as page title")]
        title_field: Option<String>,
        #[arg(long, value_name = "PREFIX", help = "Prefix for generated page titles")]
        title_prefix: Option<String>,
        #[arg(long, value_name = "NAME", help = "Category to add to generated pages")]
        category: Option<String>,
        #[arg(
            long,
            value_enum,
            default_value_t = ImportUpdateModeArg::Create,
            value_name = "MODE",
            help = "create|update|upsert"
        )]
        mode: ImportUpdateModeArg,
        #[arg(long, help = "Write files (default: dry-run)")]
        write: bool,
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
        #[arg(
            long,
            help = "Add SHORTDESC + Article quality header in main namespace"
        )]
        article_header: bool,
        #[arg(long, help = "Omit metadata from JSON output")]
        no_meta: bool,
    },
    #[command(
        name = "html-to-wikitext",
        about = "Compile captured HTML through explicit source and target profiles"
    )]
    HtmlToWikitext {
        #[arg(value_name = "PATH", help = "Captured HTML input path")]
        path: String,
        #[arg(long, value_name = "PATH", help = "Source interpretation profile")]
        source_profile: String,
        #[arg(long, value_name = "PATH", help = "Target authoring profile")]
        target_profile: String,
        #[arg(long, value_name = "TITLE", help = "Canonical source page title")]
        canonical_title: String,
        #[arg(long, value_name = "URL", help = "Canonical source page URL")]
        canonical_url: String,
        #[arg(long, value_name = "KEY", help = "Captured source evidence key")]
        source_key: String,
        #[arg(long, value_name = "SCOPE", help = "Target archive-media scope")]
        media_scope: String,
        #[arg(
            long,
            value_name = "PATH",
            help = "Optional captured media-reference inventory"
        )]
        media_inventory: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Project-scoped output path for exact compiled wikitext"
        )]
        output: String,
        #[arg(
            long,
            value_enum,
            default_value_t = OutputFormat::Text,
            value_name = "FORMAT",
            help = "Output format: text|json"
        )]
        format: OutputFormat,
    },
}

#[derive(Debug, Serialize)]
struct ImportJson<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pages_created: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages_updated: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages_skipped: Option<&'a [String]>,
    errors: &'a [ImportError],
    pages: &'a [ImportPageResult],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaReferenceInventory {
    schema: String,
    #[serde(default)]
    images: Vec<MediaReference>,
    #[serde(default)]
    media_occurrences: Vec<MediaReference>,
}

#[derive(Debug, Serialize)]
struct HtmlToWikitextJson<'a> {
    schema: &'static str,
    status: &'static str,
    source_path: String,
    source_html_sha256: String,
    source_profile_path: String,
    source_profile_id: &'a str,
    source_profile_sha256: String,
    target_profile_path: String,
    target_profile_id: &'a str,
    target_profile_sha256: String,
    media_inventory_path: Option<String>,
    media_inventory_sha256: Option<String>,
    output_path: String,
    canonical_title: &'a str,
    canonical_url: &'a str,
    source_key: &'a str,
    media_scope: &'a str,
    wikitext_sha256: String,
    wikitext_bytes: usize,
    coverage: &'a Coverage,
    used_media: &'a std::collections::BTreeSet<String>,
    media_occurrences_consumed: usize,
    unmapped_structures: &'a [UnmappedStructure],
}

const MAX_HTML_INPUT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PROFILE_INPUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MEDIA_INVENTORY_BYTES: u64 = 32 * 1024 * 1024;
const MEDIA_INVENTORY_SCHEMA: &str = "mediawiki.media-reference-inventory.v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ImportSourceTypeArg {
    Csv,
    Json,
}

impl From<ImportSourceTypeArg> for ImportSourceType {
    fn from(value: ImportSourceTypeArg) -> Self {
        match value {
            ImportSourceTypeArg::Csv => Self::Csv,
            ImportSourceTypeArg::Json => Self::Json,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ImportUpdateModeArg {
    Create,
    Update,
    Upsert,
}

impl ImportUpdateModeArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Upsert => "upsert",
        }
    }
}

impl From<ImportUpdateModeArg> for ImportUpdateMode {
    fn from(value: ImportUpdateModeArg) -> Self {
        match value {
            ImportUpdateModeArg::Create => Self::Create,
            ImportUpdateModeArg::Update => Self::Update,
            ImportUpdateModeArg::Upsert => Self::Upsert,
        }
    }
}

impl std::fmt::Display for ImportUpdateModeArg {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub(crate) fn run_import(runtime: &RuntimeOptions, args: ImportArgs) -> Result<()> {
    match args.command {
        ImportSubcommand::Cargo {
            path,
            table,
            r#type,
            template,
            title_field,
            title_prefix,
            category,
            mode,
            write,
            format,
            article_header,
            no_meta,
        } => run_import_cargo(
            runtime,
            &path,
            &table,
            r#type.map(ImportSourceType::from),
            template.as_deref(),
            title_field.as_deref(),
            title_prefix.as_deref(),
            category.as_deref(),
            ImportUpdateMode::from(mode),
            write,
            format,
            article_header,
            no_meta,
        ),
        ImportSubcommand::HtmlToWikitext {
            path,
            source_profile,
            target_profile,
            canonical_title,
            canonical_url,
            source_key,
            media_scope,
            media_inventory,
            output,
            format,
        } => run_import_html_to_wikitext(
            runtime,
            &path,
            &source_profile,
            &target_profile,
            &canonical_title,
            &canonical_url,
            &source_key,
            &media_scope,
            media_inventory.as_deref(),
            &output,
            format,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_import_html_to_wikitext(
    runtime: &RuntimeOptions,
    path: &str,
    source_profile_path: &str,
    target_profile_path: &str,
    canonical_title: &str,
    canonical_url: &str,
    source_key: &str,
    media_scope: &str,
    media_inventory_path: Option<&str>,
    output: &str,
    format: OutputFormat,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let source_path = resolve_import_source_path(path)?;
    let source_profile_path = resolve_import_source_path(source_profile_path)?;
    let target_profile_path = resolve_import_source_path(target_profile_path)?;
    let output_path = if Path::new(output).is_absolute() {
        PathBuf::from(output)
    } else {
        paths.project_root.join(output)
    };
    wikitool_core::filesystem::validate_scoped_path(&paths, &output_path)?;

    let html = read_bounded_text(&source_path, MAX_HTML_INPUT_BYTES, "HTML input")?;
    let source_profile_json = read_bounded_text(
        &source_profile_path,
        MAX_PROFILE_INPUT_BYTES,
        "source profile",
    )?;
    let target_profile_json = read_bounded_text(
        &target_profile_path,
        MAX_PROFILE_INPUT_BYTES,
        "target profile",
    )?;
    let source_profile: SourceProfile = serde_json::from_str(&source_profile_json)
        .with_context(|| format!("invalid source profile {}", source_profile_path.display()))?;
    let target_profile: TargetProfile = serde_json::from_str(&target_profile_json)
        .with_context(|| format!("invalid target profile {}", target_profile_path.display()))?;

    let resolved_inventory_path = media_inventory_path
        .map(resolve_import_source_path)
        .transpose()?;
    let (inventory, media_inventory_sha256) = if let Some(path) = &resolved_inventory_path {
        let encoded = read_bounded_text(path, MAX_MEDIA_INVENTORY_BYTES, "media inventory")?;
        let inventory = serde_json::from_str(&encoded)
            .with_context(|| format!("invalid media inventory {}", path.display()))?;
        (
            inventory,
            Some(wikitool_core::support::compute_sha256(&encoded)),
        )
    } else {
        (
            MediaReferenceInventory {
                schema: MEDIA_INVENTORY_SCHEMA.to_string(),
                images: Vec::new(),
                media_occurrences: Vec::new(),
            },
            None,
        )
    };
    if inventory.schema != MEDIA_INVENTORY_SCHEMA {
        bail!(
            "unsupported media-reference inventory schema {:?}; expected {:?}",
            inventory.schema,
            MEDIA_INVENTORY_SCHEMA
        );
    }
    let images = index_image_references(&inventory.images)?;
    let media_occurrences =
        (!inventory.media_occurrences.is_empty()).then_some(inventory.media_occurrences.as_slice());

    let result = compile_profiled(ProfiledCompileInput {
        html: &html,
        canonical_title,
        canonical_url,
        source_key,
        media_scope,
        source_profile: &source_profile,
        target_profile: &target_profile,
        images: &images,
        media_occurrences,
    })?;
    wikitool_core::support::atomic_write(&output_path, result.transformed.wikitext.as_bytes())?;

    let report = HtmlToWikitextJson {
        schema: "wikitool.import-html-to-wikitext.v1",
        status: "compiled",
        source_path: normalize_path(&source_path),
        source_html_sha256: wikitool_core::support::compute_sha256(&html),
        source_profile_path: normalize_path(&source_profile_path),
        source_profile_id: &source_profile.profile_id,
        source_profile_sha256: wikitool_core::support::compute_sha256(&source_profile_json),
        target_profile_path: normalize_path(&target_profile_path),
        target_profile_id: &target_profile.profile_id,
        target_profile_sha256: wikitool_core::support::compute_sha256(&target_profile_json),
        media_inventory_path: resolved_inventory_path.as_deref().map(normalize_path),
        media_inventory_sha256,
        output_path: normalize_path(&output_path),
        canonical_title,
        canonical_url,
        source_key,
        media_scope,
        wikitext_sha256: wikitool_core::support::compute_sha256(&result.transformed.wikitext),
        wikitext_bytes: result.transformed.wikitext.len(),
        coverage: &result.transformed.coverage,
        used_media: &result.transformed.used_media,
        media_occurrences_consumed: result.transformed.media_occurrences_consumed,
        unmapped_structures: &result.unmapped_structures,
    };

    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("import html-to-wikitext");
    println!("status: {}", report.status);
    println!("source_path: {}", report.source_path);
    println!(
        "source_profile: {} ({})",
        report.source_profile_id, report.source_profile_path
    );
    println!(
        "target_profile: {} ({})",
        report.target_profile_id, report.target_profile_path
    );
    println!("output_path: {}", report.output_path);
    println!("wikitext_sha256: {}", report.wikitext_sha256);
    println!("wikitext_bytes: {}", report.wikitext_bytes);
    println!("used_media: {}", report.used_media.len());
    println!(
        "media_occurrences_consumed: {}",
        report.media_occurrences_consumed
    );
    println!("unmapped_structures: {}", report.unmapped_structures.len());
    for structure in report.unmapped_structures {
        println!(
            "unmapped: element={} classes={} occurrences={}",
            structure.element,
            structure.classes.join(","),
            structure.occurrences
        );
    }
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}

fn read_bounded_text(path: &Path, max_bytes: u64, kind: &str) -> Result<String> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to inspect {kind} {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{kind} is not a file: {}", path.display());
    }
    if metadata.len() > max_bytes {
        bail!(
            "{kind} exceeds the {max_bytes}-byte input limit: {}",
            path.display()
        );
    }
    let mut bytes = Vec::with_capacity(metadata.len().min(max_bytes) as usize);
    fs::File::open(path)
        .with_context(|| format!("failed to open {kind} {}", path.display()))?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {kind} {}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        bail!(
            "{kind} exceeds the {max_bytes}-byte input limit while reading: {}",
            path.display()
        );
    }
    String::from_utf8(bytes).with_context(|| format!("{kind} is not UTF-8: {}", path.display()))
}

fn index_image_references(
    references: &[MediaReference],
) -> Result<BTreeMap<String, MediaReference>> {
    let mut indexed = BTreeMap::new();
    for reference in references {
        if indexed
            .insert(reference.source_url.clone(), reference.clone())
            .is_some()
        {
            bail!(
                "duplicate image media reference for source URL {:?}",
                reference.source_url
            );
        }
    }
    Ok(indexed)
}

#[allow(clippy::too_many_arguments)]
fn run_import_cargo(
    runtime: &RuntimeOptions,
    path: &str,
    table: &str,
    source_type: Option<ImportSourceType>,
    template: Option<&str>,
    title_field: Option<&str>,
    title_prefix: Option<&str>,
    category: Option<&str>,
    update_mode: ImportUpdateMode,
    write: bool,
    format: OutputFormat,
    article_header: bool,
    no_meta: bool,
) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let Some(source_type) = source_type.or_else(|| ImportSourceType::resolve(path, None)) else {
        bail!("unable to determine import type (use --type csv|json)");
    };

    let source_path = resolve_import_source_path(path)?;
    let result = import_to_cargo(
        &paths,
        &source_path,
        source_type,
        &CargoImportOptions {
            table_name: table.to_string(),
            template_name: normalize_option(template),
            title_field: normalize_option(title_field),
            title_prefix: normalize_option(title_prefix),
            update_mode,
            category_name: normalize_option(category),
            article_header,
            write,
        },
    )?;

    if format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&import_json_output(&result, no_meta))?
        );
    } else {
        println!("import cargo");
        println!("source_path: {}", normalize_path(&source_path));
        println!("source_type: {}", source_type.as_str());
        println!("table: {table}");
        println!("update_mode: {}", update_mode.as_str());
        println!("write: {}", format_flag(write));
        println!("created: {}", result.pages_created.len());
        println!("updated: {}", result.pages_updated.len());
        println!("skipped: {}", result.pages_skipped.len());
        println!("errors: {}", result.errors.len());
        for error in result.errors.iter().take(10) {
            println!(
                "error: row={} message={} title={}",
                error.row,
                error.message,
                error.title.as_deref().unwrap_or("<none>")
            );
        }
        for page in result.pages.iter().take(10) {
            println!(
                "page: action={:?} title={} path={}",
                page.action, page.title, page.relative_path
            );
        }
        if !write {
            println!("warning: dry-run only. Use --write to apply changes.");
        }
        println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
        if runtime.diagnostics {
            println!("\n[diagnostics]\n{}", paths.diagnostics());
        }
    }
    Ok(())
}

fn import_json_output<'a>(
    result: &'a wikitool_core::import_cargo::ImportResult,
    no_meta: bool,
) -> ImportJson<'a> {
    ImportJson {
        pages_created: if no_meta {
            None
        } else {
            Some(&result.pages_created)
        },
        pages_updated: if no_meta {
            None
        } else {
            Some(&result.pages_updated)
        },
        pages_skipped: if no_meta {
            None
        } else {
            Some(&result.pages_skipped)
        },
        errors: &result.errors,
        pages: &result.pages,
    }
}

fn resolve_import_source_path(path: &str) -> Result<PathBuf> {
    if Path::new(path).is_absolute() {
        return Ok(PathBuf::from(path));
    }

    Ok(env::current_dir()
        .context("failed to resolve current directory")?
        .join(path))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use wikitool_core::import_cargo::{ImportPageAction, ImportResult};

    #[test]
    fn import_no_meta_json_omits_summary_indexes() {
        let result = ImportResult {
            pages_created: vec!["Alpha".to_string()],
            pages_updated: vec!["Beta".to_string()],
            pages_skipped: vec!["Gamma".to_string()],
            errors: vec![ImportError {
                row: 3,
                message: "Missing title".to_string(),
                title: None,
            }],
            pages: vec![ImportPageResult {
                title: "Alpha".to_string(),
                relative_path: "wiki_content/Main/Alpha.wiki".to_string(),
                action: ImportPageAction::Create,
                content: Some("Alpha content".to_string()),
            }],
        };

        let value =
            serde_json::to_value(import_json_output(&result, true)).expect("serialize import");

        assert!(value.get("pages_created").is_none());
        assert!(value.get("pages_updated").is_none());
        assert!(value.get("pages_skipped").is_none());
        assert_eq!(value["errors"][0]["row"], json!(3));
        assert_eq!(value["pages"][0]["title"], json!("Alpha"));
    }

    #[test]
    fn bounded_text_read_checks_the_bytes_actually_read() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("input.txt");
        fs::write(&path, b"abcd").expect("write fixture");

        assert_eq!(
            read_bounded_text(&path, 4, "fixture").expect("read exact limit"),
            "abcd"
        );
        let error = read_bounded_text(&path, 3, "fixture").expect_err("reject oversized input");
        assert!(error.to_string().contains("3-byte input limit"));
    }
}
