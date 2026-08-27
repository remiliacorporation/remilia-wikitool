use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use wikitool_core::profile::site_adapter_resource_paths;

use crate::cli_support::{
    copy_dir_recursive, copy_file, format_flag, is_markdown_file, normalize_path, reset_directory,
    resolve_repo_root,
};

use super::ReleaseBuildAiPackArgs;

#[derive(Debug)]
pub(super) struct AiPackBuildResult {
    pub(super) output_dir: PathBuf,
    claude_rules_included: bool,
    claude_skills_included: bool,
    integration_included: bool,
    generic_site_adapter_included: bool,
    host_site_adapter_included: bool,
    codex_skills_included: bool,
    docs_bundle_included: bool,
}

pub(super) fn run_release_build_ai_pack(args: ReleaseBuildAiPackArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo_root)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| repo_root.join("dist/ai-pack"));

    let result = build_ai_pack(&repo_root, &output_dir, args.host_project_root.as_deref())?;

    println!("release build-ai-pack");
    println!("repo_root: {}", normalize_path(&repo_root));
    println!("output_dir: {}", normalize_path(&result.output_dir));
    print_ai_pack_build_flags(&result);

    Ok(())
}

pub(super) fn print_ai_pack_build_flags(result: &AiPackBuildResult) {
    println!(
        "claude_rules_included: {}",
        format_flag(result.claude_rules_included)
    );
    println!(
        "claude_skills_included: {}",
        format_flag(result.claude_skills_included)
    );
    println!(
        "integration_included: {}",
        format_flag(result.integration_included)
    );
    println!(
        "generic_site_adapter_included: {}",
        format_flag(result.generic_site_adapter_included)
    );
    println!(
        "host_site_adapter_included: {}",
        format_flag(result.host_site_adapter_included)
    );
    println!(
        "codex_skills_included: {}",
        format_flag(result.codex_skills_included)
    );
    println!(
        "docs_bundle_included: {}",
        format_flag(result.docs_bundle_included)
    );
}

pub(super) fn build_ai_pack(
    repo_root: &Path,
    output_dir: &Path,
    host_project_root: Option<&Path>,
) -> Result<AiPackBuildResult> {
    let ai_pack_root = repo_root.join("ai-pack");
    reset_directory(output_dir)?;
    copy_required_ai_pack_top_level_files(repo_root, output_dir)?;

    let mut result = AiPackBuildResult {
        output_dir: output_dir.to_path_buf(),
        claude_rules_included: false,
        claude_skills_included: false,
        integration_included: false,
        generic_site_adapter_included: false,
        host_site_adapter_included: false,
        codex_skills_included: false,
        docs_bundle_included: false,
    };

    copy_public_agent_guidance(&ai_pack_root, output_dir, &mut result)?;
    let host_root = resolve_host_project_root(host_project_root)?;

    copy_integration_and_site_adapter(
        repo_root,
        &ai_pack_root,
        output_dir,
        host_root.as_deref(),
        &mut result,
    )?;

    let docs_source = repo_root.join("docs/wikitool");
    if docs_source.is_dir() {
        copy_markdown_files(&docs_source, &output_dir.join("docs/wikitool"))?;
    }

    result.codex_skills_included = copy_optional_directory(
        &ai_pack_root.join("codex_skills"),
        &output_dir.join("codex_skills"),
    )?;
    result.docs_bundle_included = copy_optional_file(
        &ai_pack_root.join("docs-bundle-v1.json"),
        &output_dir.join("ai/docs-bundle-v1.json"),
    )?;

    write_ai_pack_manifest(&result)?;
    Ok(result)
}

fn copy_required_ai_pack_top_level_files(repo_root: &Path, output_dir: &Path) -> Result<()> {
    for file in [
        ".env.template",
        "README.md",
        "LICENSE",
        "LICENSE-SSL",
        "LICENSE-VPL",
    ] {
        let source = repo_root.join(file);
        require_file(&source, "missing required AI pack file")?;
        copy_file(&source, &output_dir.join(file))?;
    }
    Ok(())
}

fn copy_public_agent_guidance(
    ai_pack_root: &Path,
    output_dir: &Path,
    result: &mut AiPackBuildResult,
) -> Result<()> {
    let ai_pack_agents = ai_pack_root.join("AGENTS.md");
    let ai_pack_claude = ai_pack_root.join("CLAUDE.md");
    require_file(&ai_pack_agents, "missing required AI pack source file")?;
    require_file(&ai_pack_claude, "missing required AI pack source file")?;

    let claude_rules_source = ai_pack_root.join(".claude/rules");
    require_dir(
        &claude_rules_source,
        "missing required AI pack Claude rules directory",
    )?;
    copy_dir_recursive(&claude_rules_source, &output_dir.join(".claude/rules"))?;
    result.claude_rules_included = true;

    let claude_skills_source = ai_pack_root.join(".claude/skills");
    require_dir(
        &claude_skills_source,
        "missing required AI pack Claude skills directory",
    )?;
    copy_dir_recursive(&claude_skills_source, &output_dir.join(".claude/skills"))?;
    result.claude_skills_included = true;

    copy_file(&ai_pack_claude, &output_dir.join("CLAUDE.md"))?;
    copy_file(&ai_pack_agents, &output_dir.join("AGENTS.md"))?;
    Ok(())
}

fn resolve_host_project_root(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    explicit
        .map(|path| {
            fs::canonicalize(path)
                .with_context(|| format!("failed to canonicalize {}", normalize_path(path)))
        })
        .transpose()
}

fn copy_integration_and_site_adapter(
    repo_root: &Path,
    ai_pack_root: &Path,
    output_dir: &Path,
    host_root: Option<&Path>,
    result: &mut AiPackBuildResult,
) -> Result<()> {
    let integration_source = ai_pack_root.join("integration");
    require_dir(&integration_source, "missing AI pack integration directory")?;
    let integration_count =
        copy_markdown_files(&integration_source, &output_dir.join("integration"))?;
    if integration_count == 0 {
        bail!("no ai-pack/integration/*.md files found");
    }
    result.integration_included = true;

    let generic_adapter = repo_root.join("config/generic-site-adapter.toml");
    require_file(&generic_adapter, "missing generic site-adapter example")?;
    let generic_resources = site_adapter_resource_paths(&generic_adapter)
        .context("generic site-adapter example is invalid")?;
    if generic_resources.len() != 1 {
        bail!("generic site-adapter example must be self-contained");
    }
    copy_file(
        &generic_adapter,
        &output_dir.join("site_adapter/generic.toml"),
    )?;
    result.generic_site_adapter_included = true;

    let Some(host_root) = host_root else {
        return Ok(());
    };
    let host_adapter = host_root.join("wikitool_adapter");
    require_dir(
        &host_adapter,
        "host project root is missing required wikitool_adapter directory",
    )?;
    let policy = host_adapter.join("profile.toml");
    if !policy.is_file() {
        bail!(
            "host site adapter is missing required profile.toml: {}",
            normalize_path(&policy)
        );
    }
    let resources =
        site_adapter_resource_paths(&policy).context("host project site adapter is invalid")?;
    let destination = output_dir.join("site_adapter/project");
    for source in resources {
        let relative = source.strip_prefix(&host_adapter).with_context(|| {
            format!(
                "declared site-adapter resource escaped {}",
                normalize_path(&host_adapter)
            )
        })?;
        copy_file(&source, &destination.join(relative))?;
    }
    result.host_site_adapter_included = true;
    Ok(())
}

fn copy_markdown_files(source: &Path, destination: &Path) -> Result<usize> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", normalize_path(destination)))?;

    let mut copied = 0usize;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read {}", normalize_path(source)))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_markdown_file(&path) {
            copy_file(&path, &destination.join(entry.file_name()))?;
            copied += 1;
        }
    }
    Ok(copied)
}

fn copy_optional_directory(source: &Path, destination: &Path) -> Result<bool> {
    if !source.is_dir() {
        return Ok(false);
    }
    copy_dir_recursive(source, destination)?;
    Ok(true)
}

fn copy_optional_file(source: &Path, destination: &Path) -> Result<bool> {
    if !source.is_file() {
        return Ok(false);
    }
    copy_file(source, destination)?;
    Ok(true)
}

fn write_ai_pack_manifest(result: &AiPackBuildResult) -> Result<()> {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let manifest = serde_json::json!({
        "schema_version": 2,
        "generated_at_unix": now_unix,
        "claude_rules_included": result.claude_rules_included,
        "claude_skills_included": result.claude_skills_included,
        "integration_included": result.integration_included,
        "generic_site_adapter_included": result.generic_site_adapter_included,
        "host_site_adapter_included": result.host_site_adapter_included,
        "codex_skills_included": result.codex_skills_included,
        "docs_bundle_included": result.docs_bundle_included,
        "notes": "AI companion pack for wikitool; content is intentionally shipped outside the binary."
    });

    let manifest_path = result.output_dir.join("manifest.json");
    wikitool_core::support::atomic_write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    Ok(())
}

fn require_file(path: &Path, message: &str) -> Result<()> {
    if !path.is_file() {
        bail!("{message}: {}", normalize_path(path));
    }
    Ok(())
}

fn require_dir(path: &Path, message: &str) -> Result<()> {
    if !path.is_dir() {
        bail!("{message}: {}", normalize_path(path));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::build_ai_pack;

    const VALID_ADAPTER: &str = include_str!("../../../../config/generic-site-adapter.toml");

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "wikitool-ai-pack-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(path, contents).expect("write file");
    }

    fn create_repo(root: &Path) {
        for file in ["README.md", "LICENSE", "LICENSE-SSL", "LICENSE-VPL"] {
            write_file(&root.join(file), file);
        }
        write_file(
            &root.join(".env.template"),
            "WIKITOOL_BOT_USER=example@bot\nWIKITOOL_BOT_PASS=secret\n",
        );
        write_file(&root.join("ai-pack/CLAUDE.md"), "# Packaged CLAUDE\n");
        write_file(&root.join("ai-pack/AGENTS.md"), "# Packaged AGENTS\n");
        write_file(
            &root.join("ai-pack/integration/agent.md"),
            "# Integration\n",
        );
        write_file(
            &root.join("config/generic-site-adapter.toml"),
            VALID_ADAPTER,
        );
        write_file(
            &root.join("ai-pack/.claude/rules/wiki-style.md"),
            "# Rule\n",
        );
        write_file(
            &root.join("ai-pack/.claude/skills/wikitool.md"),
            "# Skill\n",
        );
    }

    fn create_host(root: &Path, claude: &str, agents: Option<&str>) {
        write_file(&root.join("CLAUDE.md"), claude);
        if let Some(agents) = agents {
            write_file(&root.join("AGENTS.md"), agents);
        }
        write_file(&root.join(".claude/rules/dev.md"), "# Host Rule\n");
        write_file(&root.join(".claude/skills/wt.md"), "# Host Skill\n");
    }

    #[test]
    fn build_ai_pack_keeps_public_guidance_when_host_adapter_is_included() {
        let temp = TestDir::new("public-guidance");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", None);
        write_file(
            &host_root.join("wikitool_adapter/profile.toml"),
            VALID_ADAPTER,
        );

        build_ai_pack(&repo_root, &output_dir, Some(&host_root)).expect("build ai pack");

        assert_eq!(
            fs::read_to_string(output_dir.join("CLAUDE.md")).expect("read packaged CLAUDE"),
            "# Packaged CLAUDE\n"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("AGENTS.md")).expect("read packaged AGENTS"),
            "# Packaged AGENTS\n"
        );
        assert!(!output_dir.join(".claude/rules/dev.md").exists());
        assert!(!output_dir.join(".claude/skills/wt.md").exists());
        assert!(!output_dir.join("SETUP.md").exists());
        assert!(output_dir.join("integration").is_dir());
        assert!(output_dir.join("site_adapter/generic.toml").is_file());
        assert!(
            output_dir
                .join("site_adapter/project/profile.toml")
                .is_file()
        );
        assert!(!output_dir.join("WIKITOOL_CLAUDE.md").exists());
    }

    #[test]
    fn build_ai_pack_rejects_explicit_host_without_site_adapter() {
        let temp = TestDir::new("missing-site-adapter");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", Some("# Host AGENTS\n"));

        let error = build_ai_pack(&repo_root, &output_dir, Some(&host_root))
            .expect_err("explicit host without adapter must fail");
        assert!(error.to_string().contains("wikitool_adapter directory"));
    }

    #[test]
    fn build_ai_pack_adds_project_site_adapter_as_a_supplement() {
        let temp = TestDir::new("host-site-adapter");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", None);
        write_file(
            &host_root.join("wikitool_adapter/profile.toml"),
            &VALID_ADAPTER.replace(
                "guidance_documents = []",
                "guidance_documents = [\"editorial.md\"]",
            ),
        );
        write_file(
            &host_root.join("wikitool_adapter/editorial.md"),
            "# Host supplement\n",
        );
        write_file(
            &host_root.join("wikitool_adapter/private-notes.md"),
            "must not ship\n",
        );

        build_ai_pack(&repo_root, &output_dir, Some(&host_root)).expect("build ai pack");

        assert_eq!(
            fs::read_to_string(output_dir.join("site_adapter/project/editorial.md"))
                .expect("read host supplement"),
            "# Host supplement\n"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("site_adapter/project/profile.toml"))
                .expect("read host policy"),
            VALID_ADAPTER.replace(
                "guidance_documents = []",
                "guidance_documents = [\"editorial.md\"]",
            )
        );
        assert!(
            !output_dir
                .join("site_adapter/project/private-notes.md")
                .exists()
        );
        assert!(output_dir.join("site_adapter/generic.toml").is_file());
        let manifest = fs::read_to_string(output_dir.join("manifest.json")).expect("read manifest");
        assert!(
            manifest.contains("\"host_site_adapter_included\": true"),
            "manifest must record the host site adapter supplement"
        );
    }

    #[test]
    fn build_ai_pack_rejects_host_site_adapter_without_typed_policy() {
        let temp = TestDir::new("host-site-adapter-without-policy");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", None);
        write_file(
            &host_root.join("wikitool_adapter/editorial.md"),
            "# Host supplement\n",
        );

        let error = build_ai_pack(&repo_root, &output_dir, Some(&host_root))
            .expect_err("host site adapter without profile.toml must fail closed");

        assert!(error.to_string().contains("missing required profile.toml"));
    }

    #[test]
    fn build_ai_pack_rejects_invalid_typed_host_policy() {
        let temp = TestDir::new("invalid-host-policy");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", None);
        write_file(
            &host_root.join("wikitool_adapter/profile.toml"),
            &VALID_ADAPTER.replace(
                "schema_version = \"site_adapter_v1\"",
                "schema_version = \"site_adapter_v1\"\nunknown_policy = true",
            ),
        );

        let error = build_ai_pack(&repo_root, &output_dir, Some(&host_root))
            .expect_err("invalid typed policy must fail closed");

        assert!(
            error
                .to_string()
                .contains("host project site adapter is invalid")
        );
    }

    #[test]
    fn build_ai_pack_ships_corrected_env_template() {
        let temp = TestDir::new("env-template");
        let repo_root = temp.path.join("repo");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);

        build_ai_pack(&repo_root, &output_dir, None).expect("build ai pack");

        let staged = output_dir.join(".env.template");
        assert!(staged.is_file(), "bundle must include .env.template");
        let contents = fs::read_to_string(&staged).expect("read staged env template");
        assert!(
            contents.contains("WIKITOOL_BOT_USER"),
            "env template must use WIKITOOL_* variable names"
        );
    }
}
