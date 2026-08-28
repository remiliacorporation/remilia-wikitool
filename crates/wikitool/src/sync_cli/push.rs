use anyhow::{Result, bail};
use serde::Serialize;
use wikitool_core::publication::encyclopedic_preflight;
use wikitool_core::runtime::{ensure_runtime_ready_for_sync, inspect_runtime};
use wikitool_core::sync::{PushOptions, PushReport, SyncSelection, push_to_remote_with_config};

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::PushArgs;
use super::shared::load_sync_selection;

#[derive(Debug, Serialize)]
struct PushJsonReport<'a> {
    project_root: String,
    summary: &'a str,
    mode: &'static str,
    apply_plan_id: Option<&'a str>,
    force: bool,
    delete: bool,
    templates: bool,
    categories: bool,
    selection: &'a SyncSelection,
    report: &'a PushReport,
}

pub(crate) fn run_push(runtime: &RuntimeOptions, args: PushArgs) -> Result<()> {
    let (paths, config) = resolve_runtime_with_config(runtime)?;
    let status = inspect_runtime(&paths)?;
    ensure_runtime_ready_for_sync(&paths, &status)?;
    let selection = load_sync_selection(&args.titles, &args.paths, args.titles_file.as_ref())?;

    let summary = args.summary.trim().to_string();
    if summary.is_empty() {
        bail!("push requires a non-empty --summary");
    }
    let dry_run = args.apply.is_none();

    let preflight = encyclopedic_preflight(&paths)?;
    let report = push_to_remote_with_config(
        &paths,
        &PushOptions {
            summary: summary.clone(),
            dry_run,
            force: args.force,
            delete: args.delete,
            include_templates: args.templates,
            categories_only: args.categories,
            all: args.all,
            selection: selection.clone(),
            apply_plan_id: args.apply.clone(),
        },
        &config,
        &preflight,
    )?;

    if args.format.is_json() {
        println!(
            "{}",
            serde_json::to_string_pretty(&PushJsonReport {
                project_root: normalize_path(&paths.project_root),
                summary: &summary,
                mode: if dry_run { "preview" } else { "apply" },
                apply_plan_id: args.apply.as_deref(),
                force: args.force,
                delete: args.delete,
                templates: args.templates,
                categories: args.categories,
                selection: &selection,
                report: &report,
            })?
        );
        if report.success {
            return Ok(());
        }
        if !report.conflicts.is_empty() && !args.force {
            bail!(
                "push blocked by {} conflict(s); rerun with --force after review",
                report.conflicts.len()
            );
        }
        bail!("push completed with {} error(s)", report.errors.len());
    }

    println!("push");
    println!("project_root: {}", normalize_path(&paths.project_root));
    println!("summary: {summary}");
    println!("mode: {}", if dry_run { "preview" } else { "apply" });
    println!(
        "apply_plan_id: {}",
        args.apply.as_deref().unwrap_or("<none>")
    );
    println!("force: {}", args.force);
    println!("delete: {}", args.delete);
    println!("templates: {}", args.templates);
    println!("categories: {}", args.categories);
    if !selection.titles.is_empty() {
        println!("selection.titles: {}", selection.titles.join(" | "));
    }
    if !selection.paths.is_empty() {
        println!("selection.paths: {}", selection.paths.join(" | "));
    }
    println!("push.request_count: {}", report.request_count);
    println!(
        "push.plan_id: {}",
        report.plan_id.as_deref().unwrap_or("<none>")
    );
    println!("push.pushed: {}", report.pushed);
    println!("push.created: {}", report.created);
    println!("push.updated: {}", report.updated);
    println!("push.deleted: {}", report.deleted);
    println!("push.unchanged: {}", report.unchanged);
    println!("push.conflicts.count: {}", report.conflicts.len());
    println!("push.errors.count: {}", report.errors.len());
    if report.pages.is_empty() {
        println!("push.pages: <none>");
    } else {
        for page in &report.pages {
            println!(
                "push.page: title={} action={} detail={}",
                page.title,
                page.action,
                page.detail.as_deref().unwrap_or("<none>")
            );
            if let Some(acceptance) = &page.acceptance {
                let target_api_url = acceptance
                    .publication_authority
                    .as_ref()
                    .map(|authority| authority.target_api_url.as_str())
                    .unwrap_or("<unbound>");
                let site_adapter_id = acceptance
                    .publication_authority
                    .as_ref()
                    .map(|authority| authority.site_adapter_id.as_str())
                    .unwrap_or("<unbound>");
                let publication_policy_sha256 = acceptance
                    .publication_authority
                    .as_ref()
                    .map(|authority| authority.publication_policy_sha256.as_str())
                    .unwrap_or("<unbound>");
                println!(
                    "push.page.acceptance: title={} content_sha256={} accepted_at_unix={} prose_origin={} identity_assurance={} warning_decision={} target_api_url={} site_adapter_id={} publication_policy_sha256={} decision_id={} changeset_sha256={}",
                    page.title,
                    acceptance.content_sha256,
                    acceptance.accepted_at_unix,
                    acceptance.prose_origin.as_str(),
                    acceptance.editor_identity_assurance,
                    acceptance.warning_decision.as_str(),
                    target_api_url,
                    site_adapter_id,
                    publication_policy_sha256,
                    acceptance.decision_id,
                    acceptance
                        .changeset_sha256
                        .as_deref()
                        .unwrap_or("<single-item>")
                );
            }
        }
    }
    for title in &report.conflicts {
        println!("push.conflict: {title}");
    }
    for error in &report.errors {
        println!("push.error: {error}");
    }
    for effect in &report.mutation_effects {
        println!(
            "push.mutation: mutation_id={} title={} kind={:?} old_revision_id={} new_revision_id={} new_timestamp={} detail={}",
            effect.mutation_id,
            effect.title,
            effect.kind,
            effect
                .old_revision_id
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or("<none>"),
            effect
                .new_revision_id
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or("<none>"),
            effect.new_timestamp.as_deref().unwrap_or("<none>"),
            effect.detail.as_deref().unwrap_or("<none>")
        );
    }
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }

    if report.success {
        Ok(())
    } else if !report.conflicts.is_empty() && !args.force {
        bail!(
            "push blocked by {} conflict(s); rerun with --force after review",
            report.conflicts.len()
        )
    } else {
        bail!("push completed with {} error(s)", report.errors.len())
    }
}
