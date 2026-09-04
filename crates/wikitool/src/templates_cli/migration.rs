use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use wikitool_core::site::template_migration::{TemplateMigrationSpec, plan_template_migration};

use crate::RuntimeOptions;
use crate::cli_support::{OutputFormat, resolve_runtime_paths};

#[derive(Debug, Args)]
pub(super) struct TemplatesMigrationPlanArgs {
    #[arg(
        value_name = "SPEC",
        help = "Strict template_migration_spec_v1 JSON file"
    )]
    spec: PathBuf,
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

pub(super) fn run(runtime: &RuntimeOptions, args: TemplatesMigrationPlanArgs) -> Result<()> {
    let paths = resolve_runtime_paths(runtime)?;
    let path = if args.spec.is_absolute() {
        args.spec
    } else {
        paths.project_root.join(args.spec)
    };
    wikitool_core::filesystem::validate_project_path(&paths, &path)?;
    let mut text = String::new();
    File::open(&path)
        .context("open migration specification")?
        .take(64 * 1024 + 1)
        .read_to_string(&mut text)
        .context("read migration specification")?;
    if text.len() > 64 * 1024 {
        bail!("migration specification exceeds 64 KiB");
    }
    let spec: TemplateMigrationSpec =
        serde_json::from_str(&text).context("parse migration specification")?;
    let plan = plan_template_migration(&paths, spec)?;
    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        println!("plan_id: {}", plan.plan_id);
        println!("scope: {}", plan.scope);
        println!("scanned_files: {}", plan.scanned_files);
        println!("affected_files: {}", plan.affected_files);
        println!("invocations: {}", plan.invocation_count);
        println!("mechanical_patches: {}", plan.mechanical_patch_count);
        println!("review_required_files: {}", plan.review_required_files);
        println!("retirement_ready: false (live usage and render verification required)");
    }
    Ok(())
}
