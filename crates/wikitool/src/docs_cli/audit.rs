use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::Serialize;

use crate::cli_support::{OutputFormat, normalize_path};

use super::reference::{generate_docs_reference_markdown, source_repo_root};

#[derive(Debug, Args)]
pub(crate) struct DocsAuditArgs {
    #[arg(
        long,
        value_name = "PATH",
        help = "Optional host project root whose redirect stubs should be audited"
    )]
    pub(crate) host_project_root: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value_t = OutputFormat::Json,
        value_name = "FORMAT",
        help = "Output format: text|json"
    )]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Serialize)]
struct DocsAuditReport {
    schema_version: &'static str,
    status: &'static str,
    repo_root: String,
    host_project_root: Option<String>,
    check_count: usize,
    failure_count: usize,
    checks: Vec<DocsAuditCheck>,
}

#[derive(Debug, Serialize)]
struct DocsAuditCheck {
    id: &'static str,
    status: &'static str,
    path: Option<String>,
    message: String,
}

pub(crate) fn run_docs_audit(args: DocsAuditArgs) -> Result<()> {
    let repo_root = source_repo_root()?;
    let host_project_root = args
        .host_project_root
        .as_ref()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()
                    .context("failed to resolve current directory")
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| path.clone())
            }
        })
        .map(|path| path.canonicalize().unwrap_or(path));

    let mut checks = Vec::new();
    audit_reference(&repo_root, &mut checks);
    audit_default_features(&repo_root, &mut checks);
    audit_packaged_guidance(&repo_root, &mut checks);
    audit_no_retired_public_terms(&repo_root, &mut checks);
    audit_brief_guidance(&repo_root, &mut checks);
    audit_workflow_guidance(&repo_root, &mut checks);
    audit_interview_direction_guidance(&repo_root, &mut checks);
    if let Some(host_root) = host_project_root.as_ref() {
        audit_host_project(host_root, &mut checks);
    }

    let failure_count = checks.iter().filter(|check| check.status == "fail").count();
    let report = DocsAuditReport {
        schema_version: "docs_audit_v1",
        status: if failure_count == 0 { "pass" } else { "fail" },
        repo_root: normalize_path(&repo_root),
        host_project_root: host_project_root.as_ref().map(normalize_path),
        check_count: checks.len(),
        failure_count,
        checks,
    };

    if args.format.is_json() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_docs_audit_report(&report);
    }

    if report.failure_count == 0 {
        Ok(())
    } else {
        bail!("docs audit failed with {} failure(s)", report.failure_count)
    }
}

fn audit_reference(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let path = repo_root.join("docs/wikitool/reference.md");
    let actual = read_to_string(&path);
    let expected = generate_docs_reference_markdown();
    match (actual, expected) {
        (Ok(actual), Ok(expected)) => {
            let actual = normalize_newlines(&actual);
            let expected = normalize_newlines(&expected);
            push_check(
                checks,
                "reference.generated",
                actual == expected,
                Some(&path),
                if actual == expected {
                    "generated CLI reference is current".to_string()
                } else {
                    "generated CLI reference is stale; run `cargo run --features maintainer -- docs generate-reference`".to_string()
                },
            );
        }
        (Err(error), _) => push_check(
            checks,
            "reference.generated",
            false,
            Some(&path),
            format!("failed to read generated reference: {error}"),
        ),
        (_, Err(error)) => push_check(
            checks,
            "reference.generated",
            false,
            Some(&path),
            format!("failed to render generated reference: {error}"),
        ),
    }
}

fn audit_default_features(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let path = repo_root.join("crates/wikitool/Cargo.toml");
    match read_to_string(&path) {
        Ok(body) => {
            let default_is_empty = body.lines().any(|line| line.trim() == "default = []");
            push_check(
                checks,
                "cargo.default_surface",
                default_is_empty,
                Some(&path),
                if default_is_empty {
                    "normal source and release builds use the end-user surface".to_string()
                } else {
                    "Cargo default features must stay empty; maintainer commands require `--features maintainer`".to_string()
                },
            );
        }
        Err(error) => push_check(
            checks,
            "cargo.default_surface",
            false,
            Some(&path),
            format!("failed to read Cargo.toml: {error}"),
        ),
    }
}

fn audit_packaged_guidance(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let claude_path = repo_root.join("ai-pack/CLAUDE.md");
    let agents_path = repo_root.join("ai-pack/AGENTS.md");
    match (read_to_string(&claude_path), read_to_string(&agents_path)) {
        (Ok(claude), Ok(agents)) => {
            push_check(
                checks,
                "ai_pack.agent_guidance_mirror",
                claude == agents,
                Some(&agents_path),
                if claude == agents {
                    "packaged CLAUDE.md and AGENTS.md are identical".to_string()
                } else {
                    "packaged CLAUDE.md and AGENTS.md must remain identical".to_string()
                },
            );
        }
        (Err(error), _) => push_check(
            checks,
            "ai_pack.agent_guidance_mirror",
            false,
            Some(&claude_path),
            format!("failed to read ai-pack CLAUDE.md: {error}"),
        ),
        (_, Err(error)) => push_check(
            checks,
            "ai_pack.agent_guidance_mirror",
            false,
            Some(&agents_path),
            format!("failed to read ai-pack AGENTS.md: {error}"),
        ),
    }

    for (id, left, right, required) in [
        (
            "skills.operator_alignment",
            "ai-pack/.claude/skills/wikitool.md",
            "ai-pack/codex_skills/wikitool-operator/SKILL.md",
            "brief",
        ),
        (
            "skills.review_alignment",
            "ai-pack/.claude/skills/review.md",
            "ai-pack/codex_skills/wikitool-content-gate/SKILL.md",
            "review --format json",
        ),
    ] {
        let left_path = repo_root.join(left);
        let right_path = repo_root.join(right);
        match (read_to_string(&left_path), read_to_string(&right_path)) {
            (Ok(left_body), Ok(right_body)) => {
                let ok = left_body.contains(required) && right_body.contains(required);
                push_check(
                    checks,
                    id,
                    ok,
                    Some(&right_path),
                    if ok {
                        format!("Claude and Codex skill wrappers both mention `{required}`")
                    } else {
                        format!("Claude and Codex skill wrappers must both mention `{required}`")
                    },
                );
            }
            (Err(error), _) => push_check(
                checks,
                id,
                false,
                Some(&left_path),
                format!("failed to read Claude skill: {error}"),
            ),
            (_, Err(error)) => push_check(
                checks,
                id,
                false,
                Some(&right_path),
                format!("failed to read Codex skill: {error}"),
            ),
        }
    }
}

fn audit_no_retired_public_terms(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let mut failures = Vec::new();
    for path in markdown_files(repo_root) {
        let Ok(body) = read_to_string(&path) else {
            continue;
        };
        for term in [
            "wikitool context",
            "wikitool search",
            "wikitool fetch",
            "wikitool seo",
            "wikitool net",
            "--view agent-card",
            "agent-card",
            "function-card",
            "function-context",
        ] {
            if body.contains(term) {
                failures.push(format!("{} contains `{term}`", normalize_path(&path)));
            }
        }
    }
    push_check(
        checks,
        "guidance.no_retired_surface",
        failures.is_empty(),
        Some(repo_root),
        if failures.is_empty() {
            "guidance and docs do not mention retired public surfaces or rejected brief names"
                .to_string()
        } else {
            failures.join("; ")
        },
    );
}

fn audit_brief_guidance(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let required = [
        ("ai-pack/CLAUDE.md", "--view brief"),
        ("ai-pack/AGENTS.md", "--view brief"),
        (
            "ai-pack/codex_skills/wikitool-operator/SKILL.md",
            "--view brief",
        ),
        ("ai-pack/.claude/skills/wikitool.md", "--view brief"),
        ("docs/wikitool/guide.md", "--view brief"),
        ("docs/wikitool/architecture.md", "wikitool brief"),
    ];
    for (relative, needle) in required {
        let path = repo_root.join(relative);
        match read_to_string(&path) {
            Ok(body) => push_check(
                checks,
                "guidance.brief_surface",
                body.contains(needle),
                Some(&path),
                if body.contains(needle) {
                    format!("{relative} documents `{needle}`")
                } else {
                    format!("{relative} must document `{needle}`")
                },
            ),
            Err(error) => push_check(
                checks,
                "guidance.brief_surface",
                false,
                Some(&path),
                format!("failed to read {relative}: {error}"),
            ),
        }
    }
}

fn audit_workflow_guidance(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let required = [
        ("README.md", "wikitool workflow session-refresh"),
        (
            "docs/wikitool/guide.md",
            "wikitool workflow session-refresh",
        ),
        ("ai-pack/CLAUDE.md", "wikitool workflow session-refresh"),
        ("ai-pack/AGENTS.md", "wikitool workflow session-refresh"),
        (
            "ai-pack/.claude/skills/wikitool.md",
            "wikitool workflow session-refresh",
        ),
        (
            "ai-pack/codex_skills/wikitool-operator/SKILL.md",
            "wikitool workflow session-refresh",
        ),
        (
            "ai-pack/writing_context/writing_guide.md",
            "wikitool workflow session-refresh",
        ),
    ];
    for (relative, needle) in required {
        let path = repo_root.join(relative);
        match read_to_string(&path) {
            Ok(body) => push_check(
                checks,
                "guidance.workflow_surface",
                body.contains(needle),
                Some(&path),
                if body.contains(needle) {
                    format!("{relative} documents `{needle}`")
                } else {
                    format!("{relative} must document `{needle}` as the public refresh lane")
                },
            ),
            Err(error) => push_check(
                checks,
                "guidance.workflow_surface",
                false,
                Some(&path),
                format!("failed to read {relative}: {error}"),
            ),
        }
    }

    let local_skill = repo_root.join(".claude/skills/wikitool/SKILL.md");
    match read_to_string(&local_skill) {
        Ok(body) => {
            let ok = body.contains("Maintainer-only lanes")
                && !body.contains("`release`, `workflow`")
                && !body.contains("release, workflow");
            push_check(
                checks,
                "guidance.workflow_public",
                ok,
                Some(&local_skill),
                if ok {
                    "local Claude wrapper treats workflow as an end-user lane".to_string()
                } else {
                    "local Claude wrapper must not list workflow as maintainer-only".to_string()
                },
            );
        }
        Err(error) => push_check(
            checks,
            "guidance.workflow_public",
            false,
            Some(&local_skill),
            format!("failed to read local Claude wrapper: {error}"),
        ),
    }
}

fn audit_interview_direction_guidance(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let required = [
        (
            "ai-pack/CLAUDE.md",
            &[
                "normal move",
                "intent, scope, and angle",
                "well-documented subjects",
            ][..],
        ),
        (
            "ai-pack/AGENTS.md",
            &[
                "normal move",
                "intent, scope, and angle",
                "well-documented subjects",
            ][..],
        ),
        (
            "ai-pack/writing_context/interview_playbook.md",
            &[
                "normal move, not an exception",
                "well-documented subjects",
                "This is framing, not a forced",
            ][..],
        ),
        (
            "ai-pack/codex_skills/wikitool-knowledge-interview/SKILL.md",
            &[
                "normal move",
                "intent, scope, and angle",
                "well-documented subjects",
            ][..],
        ),
        (
            "docs/wikitool/guide.md",
            &[
                "interview by default",
                "intent, scope, and angle",
                "well-documented subjects",
            ][..],
        ),
        (
            "docs/wikitool/architecture.md",
            &[
                "knowledge-interview skill by default",
                "sets article intent, scope, and",
                "not limited to cases",
            ][..],
        ),
        (
            "RELEASE_LOG.md",
            &[
                "normal move after the article-start scout",
                "purpose is direction",
                "well-documented subject",
            ][..],
        ),
    ];

    for (relative, needles) in required {
        let path = repo_root.join(relative);
        match read_to_string(&path) {
            Ok(body) => {
                let normalized = normalize_whitespace(&body);
                let missing = needles
                    .iter()
                    .filter(|needle| !normalized.contains(&normalize_whitespace(needle)))
                    .copied()
                    .collect::<Vec<_>>();
                push_check(
                    checks,
                    "guidance.interview_direction",
                    missing.is_empty(),
                    Some(&path),
                    if missing.is_empty() {
                        format!("{relative} preserves direction-first interview framing")
                    } else {
                        format!(
                            "{relative} is missing direction-first interview term(s): {}",
                            missing.join(", ")
                        )
                    },
                );
            }
            Err(error) => push_check(
                checks,
                "guidance.interview_direction",
                false,
                Some(&path),
                format!("failed to read {relative}: {error}"),
            ),
        }
    }

    let retired = [
        "Reach for it when your specific knowledge reaches further than public sources do, and skip it when they do not",
        "Reach for it when an editor knows more than the public sources do, and skip it when they do not",
    ];
    let mut failures = Vec::new();
    for path in markdown_files(repo_root) {
        let Ok(body) = read_to_string(&path) else {
            continue;
        };
        let normalized = normalize_whitespace(&body);
        for phrase in retired {
            if normalized.contains(&normalize_whitespace(phrase)) {
                failures.push(format!(
                    "{} contains retired interview framing",
                    normalize_path(&path)
                ));
            }
        }
    }
    push_check(
        checks,
        "guidance.interview_retired_framing",
        failures.is_empty(),
        Some(repo_root),
        if failures.is_empty() {
            "guidance does not retain the retired public-sources-only interview framing".to_string()
        } else {
            failures.join("; ")
        },
    );
}

fn audit_host_project(host_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    for relative in ["AGENTS.md", "CLAUDE.md"] {
        let path = host_root.join(relative);
        match read_to_string(&path) {
            Ok(body) => {
                let ok = body.contains("tools/wikitool/ai-pack/.claude/skills/")
                    && body.contains("tools/wikitool/docs/wikitool/reference.md")
                    && !body.contains("wikitool search");
                push_check(
                    checks,
                    "host.root_guidance",
                    ok,
                    Some(&path),
                    if ok {
                        format!("{relative} routes to public ai-pack guidance")
                    } else {
                        format!(
                            "{relative} must route to public ai-pack guidance and avoid retired commands"
                        )
                    },
                );
            }
            Err(error) => push_check(
                checks,
                "host.root_guidance",
                false,
                Some(&path),
                format!("failed to read host guidance: {error}"),
            ),
        }
    }

    for relative in [".claude/skills/wikitool.md", ".claude/skills/review.md"] {
        let path = host_root.join(relative);
        match read_to_string(&path) {
            Ok(body) => {
                let ok = body.contains("tools/wikitool/ai-pack/.claude/skills/")
                    && !body.contains("wikitool search");
                push_check(
                    checks,
                    "host.skill_redirects",
                    ok,
                    Some(&path),
                    if ok {
                        format!("{relative} is a thin redirect stub")
                    } else {
                        format!("{relative} must remain a thin redirect stub to ai-pack")
                    },
                );
            }
            Err(error) => push_check(
                checks,
                "host.skill_redirects",
                false,
                Some(&path),
                format!("failed to read host skill redirect: {error}"),
            ),
        }
    }
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_markdown_files(root, root, &mut out);
    out
}

fn collect_markdown_files(root: &Path, path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if matches!(file_name, ".git" | "target" | ".wikitool" | "dist") {
                continue;
            }
            collect_markdown_files(root, &path, out);
            continue;
        }
        let is_markdown = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
        if is_markdown && path.starts_with(root) {
            out.push(path);
        }
    }
}

fn push_check(
    checks: &mut Vec<DocsAuditCheck>,
    id: &'static str,
    ok: bool,
    path: Option<&Path>,
    message: String,
) {
    checks.push(DocsAuditCheck {
        id,
        status: if ok { "pass" } else { "fail" },
        path: path.map(normalize_path),
        message,
    });
}

fn read_to_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", normalize_path(path)))
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn print_docs_audit_report(report: &DocsAuditReport) {
    println!("docs audit");
    println!("status: {}", report.status);
    println!("repo_root: {}", report.repo_root);
    if let Some(host) = &report.host_project_root {
        println!("host_project_root: {host}");
    }
    println!("check_count: {}", report.check_count);
    println!("failure_count: {}", report.failure_count);
    for check in &report.checks {
        println!(
            "check: id={} status={} path={} message={}",
            check.id,
            check.status,
            check.path.as_deref().unwrap_or("<none>"),
            check.message
        );
    }
}
