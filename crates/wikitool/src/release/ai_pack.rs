use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::cli_support::{
    copy_dir_recursive, copy_file, detect_host_context_root, format_flag, is_markdown_file,
    normalize_path, paths_equivalent, reset_directory, resolve_repo_root,
};

use super::ReleaseBuildAiPackArgs;

#[derive(Debug)]
pub(super) struct AiPackBuildResult {
    pub(super) output_dir: PathBuf,
    host_context_included: bool,
    claude_rules_included: bool,
    claude_skills_included: bool,
    llm_instructions_included: bool,
    host_llm_instructions_included: bool,
    codex_skills_included: bool,
    docs_bundle_included: bool,
}

#[derive(Debug)]
struct AiPackGuidanceContext {
    effective_claude_source: PathBuf,
    host_root: Option<PathBuf>,
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
        "host_context_included: {}",
        format_flag(result.host_context_included)
    );
    println!(
        "claude_rules_included: {}",
        format_flag(result.claude_rules_included)
    );
    println!(
        "claude_skills_included: {}",
        format_flag(result.claude_skills_included)
    );
    println!(
        "llm_instructions_included: {}",
        format_flag(result.llm_instructions_included)
    );
    println!(
        "host_llm_instructions_included: {}",
        format_flag(result.host_llm_instructions_included)
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
        host_context_included: false,
        claude_rules_included: false,
        claude_skills_included: false,
        llm_instructions_included: false,
        host_llm_instructions_included: false,
        codex_skills_included: false,
        docs_bundle_included: false,
    };

    let guidance_context = prepare_ai_pack_guidance_context(
        repo_root,
        &ai_pack_root,
        output_dir,
        host_project_root,
        &mut result,
    )?;
    copy_file(
        &guidance_context.effective_claude_source,
        &output_dir.join("CLAUDE.md"),
    )?;
    copy_file(
        &guidance_context.effective_claude_source,
        &output_dir.join("AGENTS.md"),
    )?;

    copy_llm_instructions(
        &ai_pack_root,
        output_dir,
        guidance_context.host_root.as_deref(),
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
        "SETUP.md",
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

fn prepare_ai_pack_guidance_context(
    repo_root: &Path,
    ai_pack_root: &Path,
    output_dir: &Path,
    host_project_root: Option<&Path>,
    result: &mut AiPackBuildResult,
) -> Result<AiPackGuidanceContext> {
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

    let mut effective_claude_source = ai_pack_claude.clone();
    let mut host_context_root = None;
    if let Some(host_root) = detect_host_context_root(repo_root, host_project_root)?
        && !paths_equivalent(&host_root, repo_root)?
    {
        copy_file(&ai_pack_claude, &output_dir.join("WIKITOOL_CLAUDE.md"))?;
        effective_claude_source = host_root.join("CLAUDE.md");
        host_context_root = Some(host_root.clone());
        copy_dir_recursive(
            &host_root.join(".claude/rules"),
            &output_dir.join(".claude/rules"),
        )?;
        copy_dir_recursive(
            &host_root.join(".claude/skills"),
            &output_dir.join(".claude/skills"),
        )?;
        result.host_context_included = true;
    }

    Ok(AiPackGuidanceContext {
        effective_claude_source,
        host_root: host_context_root,
    })
}

fn copy_llm_instructions(
    ai_pack_root: &Path,
    output_dir: &Path,
    host_root: Option<&Path>,
    result: &mut AiPackBuildResult,
) -> Result<()> {
    let llm_source = ai_pack_root.join("llm_instructions");
    require_dir(&llm_source, "missing llm_instructions directory")?;
    let llm_output = output_dir.join("llm_instructions");
    let llm_count = copy_markdown_files(&llm_source, &llm_output)?;
    if llm_count == 0 {
        bail!("no ai-pack/llm_instructions/*.md files found");
    }
    result.llm_instructions_included = true;

    let Some(host_root) = host_root else {
        return Ok(());
    };
    let host_llm_source = host_root.join("llm_instructions");
    if !host_llm_source.is_dir() {
        return Ok(());
    }

    copy_dir_recursive(&llm_output, &output_dir.join("WIKITOOL_LLM_INSTRUCTIONS"))?;
    reset_directory(&llm_output)?;
    let host_llm_count = copy_markdown_files(&host_llm_source, &llm_output)?;
    if host_llm_count == 0 {
        bail!(
            "host llm_instructions directory has no markdown files: {}",
            normalize_path(&host_llm_source)
        );
    }
    result.host_llm_instructions_included = true;
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
        "schema_version": 1,
        "generated_at_unix": now_unix,
        "host_context_included": result.host_context_included,
        "claude_rules_included": result.claude_rules_included,
        "claude_skills_included": result.claude_skills_included,
        "llm_instructions_included": result.llm_instructions_included,
        "host_llm_instructions_included": result.host_llm_instructions_included,
        "codex_skills_included": result.codex_skills_included,
        "docs_bundle_included": result.docs_bundle_included,
        "notes": "AI companion pack for wikitool; content is intentionally shipped outside the binary."
    });

    let manifest_path = result.output_dir.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", normalize_path(&manifest_path)))?;
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
        for file in [
            "SETUP.md",
            "README.md",
            "LICENSE",
            "LICENSE-SSL",
            "LICENSE-VPL",
        ] {
            write_file(&root.join(file), file);
        }
        write_file(&root.join("ai-pack/CLAUDE.md"), "# Packaged CLAUDE\n");
        write_file(&root.join("ai-pack/AGENTS.md"), "# Packaged AGENTS\n");
        write_file(
            &root.join("ai-pack/llm_instructions/writing_guide.md"),
            "# Guide\n",
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
    fn build_ai_pack_host_overlay_uses_host_claude_for_both_guidance_files() {
        let temp = TestDir::new("fallback");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", None);

        build_ai_pack(&repo_root, &output_dir, Some(&host_root)).expect("build ai pack");

        assert_eq!(
            fs::read_to_string(output_dir.join("CLAUDE.md")).expect("read packaged CLAUDE"),
            "# Host CLAUDE\n"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("AGENTS.md")).expect("read packaged AGENTS"),
            "# Host CLAUDE\n"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("WIKITOOL_CLAUDE.md"))
                .expect("read preserved wikitool guidance"),
            "# Packaged CLAUDE\n"
        );
    }

    #[test]
    fn build_ai_pack_host_overlay_ignores_distinct_host_agents_file() {
        let temp = TestDir::new("distinct-agents");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", Some("# Host AGENTS\n"));

        build_ai_pack(&repo_root, &output_dir, Some(&host_root)).expect("build ai pack");

        assert_eq!(
            fs::read_to_string(output_dir.join("CLAUDE.md")).expect("read packaged CLAUDE"),
            "# Host CLAUDE\n"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("AGENTS.md")).expect("read packaged AGENTS"),
            "# Host CLAUDE\n"
        );
    }

    #[test]
    fn build_ai_pack_host_overlay_can_replace_llm_instructions() {
        let temp = TestDir::new("host-llm");
        let repo_root = temp.path.join("repo");
        let host_root = temp.path.join("host");
        let output_dir = temp.path.join("out");
        create_repo(&repo_root);
        create_host(&host_root, "# Host CLAUDE\n", None);
        write_file(
            &host_root.join("llm_instructions/writing_guide.md"),
            "# Host Guide\n",
        );
        write_file(
            &host_root.join("llm_instructions/site_contract.md"),
            "# Host Contract\n",
        );

        build_ai_pack(&repo_root, &output_dir, Some(&host_root)).expect("build ai pack");

        assert_eq!(
            fs::read_to_string(output_dir.join("llm_instructions/writing_guide.md"))
                .expect("read host guide"),
            "# Host Guide\n"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("llm_instructions/site_contract.md"))
                .expect("read host contract"),
            "# Host Contract\n"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("WIKITOOL_LLM_INSTRUCTIONS/writing_guide.md"))
                .expect("read preserved default guide"),
            "# Guide\n"
        );
        let manifest = fs::read_to_string(output_dir.join("manifest.json")).expect("read manifest");
        assert!(
            manifest.contains("\"host_llm_instructions_included\": true"),
            "manifest must record host LLM instruction overlay"
        );
    }
}
