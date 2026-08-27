use anyhow::Result;
use wikitool_core::profile::load_or_build_site_profile;

use crate::cli_support::{normalize_path, resolve_runtime_with_config};
use crate::{LOCAL_DB_POLICY_MESSAGE, RuntimeOptions};

use super::output::print_profile;
use super::*;
pub(super) fn run_wiki_rules(runtime: &RuntimeOptions, args: WikiRulesArgs) -> Result<()> {
    match args.command {
        WikiRulesSubcommand::Show(args) => run_wiki_rules_show(runtime, args.format),
    }
}

fn run_wiki_rules_show(runtime: &RuntimeOptions, format: OutputFormat) -> Result<()> {
    let (paths, _) = resolve_runtime_with_config(runtime)?;
    let profile = load_or_build_site_profile(&paths)?;

    if format.is_json() {
        println!("{}", serde_json::to_string_pretty(&profile)?);
        return Ok(());
    }

    println!("wiki rules show");
    println!("project_root: {}", normalize_path(&paths.project_root));
    print_profile(&profile);
    println!("policy: {LOCAL_DB_POLICY_MESSAGE}");
    if runtime.diagnostics {
        println!("\n[diagnostics]\n{}", paths.diagnostics());
    }
    Ok(())
}
