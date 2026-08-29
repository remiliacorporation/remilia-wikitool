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
        help = "Optional host project root whose redirects and site adapter should be audited"
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
                    .context("failed to resolve current directory")?
                    .join(path)
            }
            .canonicalize()
            .context("failed to resolve host project root")
        })
        .transpose()?;

    let mut checks = Vec::new();
    audit_reference(&repo_root, &mut checks);
    audit_default_features(&repo_root, &mut checks);
    audit_ai_pack_layout(&repo_root, &mut checks);
    audit_canonical_skills(&repo_root, &mut checks);
    audit_source_root_skill_routes(&repo_root, &mut checks);
    audit_generic_boundary(&repo_root, &mut checks);
    audit_no_retired_public_terms(&repo_root, &mut checks);
    if let Some(host_root) = host_project_root.as_ref() {
        audit_host_project(host_root, &mut checks);
    }

    let failure_count = checks.iter().filter(|check| check.status == "fail").count();
    let report = DocsAuditReport {
        schema_version: "docs_audit_v2",
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
    let ok = matches!((&actual, &expected), (Ok(left), Ok(right)) if normalize_newlines(left) == normalize_newlines(right));
    let message = match (actual, expected) {
        (Ok(_), Ok(_)) if ok => "generated CLI reference is current".to_string(),
        (Ok(_), Ok(_)) => {
            "generated CLI reference is stale; run `cargo run --package wikitool --features maintainer -- docs generate-reference`".to_string()
        }
        (Err(error), _) => format!("failed to read generated reference: {error}"),
        (_, Err(error)) => format!("failed to render generated reference: {error}"),
    };
    push_check(checks, "reference.generated", ok, Some(&path), message);
}

fn audit_default_features(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let path = repo_root.join("crates/wikitool/Cargo.toml");
    match read_to_string(&path) {
        Ok(body) => {
            let ok = body.lines().any(|line| line.trim() == "default = []");
            push_check(
                checks,
                "cargo.default_surface",
                ok,
                Some(&path),
                if ok {
                    "normal builds use the end-user surface".to_string()
                } else {
                    "Cargo default features must stay empty".to_string()
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

fn audit_ai_pack_layout(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let claude_path = repo_root.join("ai-pack/CLAUDE.md");
    let agents_path = repo_root.join("ai-pack/AGENTS.md");
    let mirrored = read_to_string(&claude_path)
        .and_then(|claude| read_to_string(&agents_path).map(|agents| claude == agents))
        .unwrap_or(false);
    push_check(
        checks,
        "ai_pack.agent_guidance_mirror",
        mirrored,
        Some(&agents_path),
        if mirrored {
            "packaged CLAUDE.md and AGENTS.md are identical".to_string()
        } else {
            "packaged CLAUDE.md and AGENTS.md must remain identical".to_string()
        },
    );

    for relative in [
        "ai-pack/integration/agent_integration.md",
        "ai-pack/integration/site_adapters.md",
        "config/generic-site-adapter.toml",
    ] {
        let path = repo_root.join(relative);
        push_check(
            checks,
            "ai_pack.layered_layout",
            path.is_file(),
            Some(&path),
            if path.is_file() {
                format!("{relative} is present")
            } else {
                format!("required layered AI-pack resource is missing: {relative}")
            },
        );
    }

    let retired = repo_root.join("ai-pack/writing_context");
    push_check(
        checks,
        "ai_pack.no_retired_writing_context",
        !retired.exists(),
        Some(&retired),
        if retired.exists() {
            "retired writing_context directory must stay removed".to_string()
        } else {
            "prose procedures live in agent skills, not writing_context".to_string()
        },
    );
}

fn audit_canonical_skills(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    for (name, references) in [
        (
            "wiki-writing",
            &[
                "evidence-to-prose.md",
                "human-notes.md",
                "mediawiki-structure.md",
            ][..],
        ),
        (
            "prose-review",
            &["source-fidelity.md", "reader-value.md", "blp-sensitive.md"][..],
        ),
        ("wiki-interview", &["interview-ledger.md"][..]),
        ("wikitool-operator", &[][..]),
    ] {
        let root = repo_root.join("ai-pack/codex_skills").join(name);
        let skill_path = root.join("SKILL.md");
        let mut failures = Vec::new();
        match read_to_string(&skill_path) {
            Ok(body) => {
                if !valid_skill_frontmatter(&body, name) {
                    failures.push("invalid name/description-only frontmatter".to_string());
                }
                if !body.contains("## Procedure") || !body.contains("## Exit conditions") {
                    failures.push("missing procedure or exit conditions".to_string());
                }
                for reference in references {
                    if !body.contains(reference)
                        || !root.join("references").join(reference).is_file()
                    {
                        failures.push(format!("missing routed reference {reference}"));
                    }
                }
            }
            Err(error) => failures.push(format!("failed to read SKILL.md: {error}")),
        }
        if !root.join("agents/openai.yaml").is_file() {
            failures.push("missing agents/openai.yaml".to_string());
        }
        push_check(
            checks,
            "skills.canonical_shape",
            failures.is_empty(),
            Some(&skill_path),
            if failures.is_empty() {
                format!("{name} has a substantive canonical skill package")
            } else {
                format!("{name}: {}", failures.join("; "))
            },
        );
    }
}

fn audit_source_root_skill_routes(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let directory = repo_root.join(".claude/skills/wikitool");
    let path = directory.join("SKILL.md");
    let directory = path.parent().expect("source-root skill directory");
    let canonical = "ai-pack/codex_skills/wikitool-operator/SKILL.md";
    match read_to_string(&path) {
        Ok(body) => {
            let line_count = body.lines().count();
            let only_canonical_entrypoint = std::fs::read_dir(directory).is_ok_and(|entries| {
                let mut names = entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.file_name())
                    .collect::<Vec<_>>();
                names.sort();
                names == ["SKILL.md"]
            });
            let routes_to_canonical =
                body.contains(canonical) && line_count <= 16 && only_canonical_entrypoint;
            push_check(
                checks,
                "skills.source_root_route",
                routes_to_canonical,
                Some(&path),
                if routes_to_canonical {
                    "source-root Claude entrypoint is a thin canonical route".to_string()
                } else {
                    format!(
                        "source-root Claude entrypoint must route to {canonical} without a parallel command tree"
                    )
                },
            );
        }
        Err(error) => push_check(
            checks,
            "skills.source_root_route",
            false,
            Some(&path),
            format!("failed to read source-root Claude entrypoint: {error}"),
        ),
    }

    let entries = std::fs::read_dir(directory).and_then(|entries| {
        entries
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()
    });
    match entries {
        Ok(entries) => {
            let single_entrypoint = entries.len() == 1 && entries[0] == "SKILL.md";
            push_check(
                checks,
                "skills.source_root_single_entrypoint",
                single_entrypoint,
                Some(directory),
                if single_entrypoint {
                    "source-root Claude adapter contains only its canonical entrypoint".to_string()
                } else {
                    format!(
                        "source-root Claude adapter must contain only SKILL.md, found: {}",
                        entries
                            .iter()
                            .map(|entry| entry.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            );
        }
        Err(error) => push_check(
            checks,
            "skills.source_root_single_entrypoint",
            false,
            Some(directory),
            format!("failed to inspect source-root Claude adapter: {error}"),
        ),
    }
}

fn valid_skill_frontmatter(body: &str, expected_name: &str) -> bool {
    let mut lines = body.lines();
    if lines.next() != Some("---") {
        return false;
    }
    let mut fields = Vec::new();
    for line in &mut lines {
        if line == "---" {
            break;
        }
        if !line.trim().is_empty() {
            fields.push(line);
        }
    }
    fields.len() == 2
        && fields
            .iter()
            .any(|line| *line == format!("name: {expected_name}"))
        && fields.iter().any(|line| line.starts_with("description: "))
}

fn audit_generic_boundary(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let root = repo_root.join("ai-pack");
    let mut leaks = Vec::new();
    for path in text_files(&root) {
        let Ok(body) = read_to_string(&path) else {
            continue;
        };
        let lowered = body.to_ascii_lowercase();
        for token in [
            "remilia",
            "charlotte fang",
            "milady maker",
            "wiki.remilia.org",
            "d3chart",
        ] {
            if lowered.contains(token) {
                leaks.push(format!("{} contains {token:?}", normalize_path(&path)));
            }
        }
    }
    push_check(
        checks,
        "ai_pack.target_neutral",
        leaks.is_empty(),
        Some(&root),
        if leaks.is_empty() {
            "generic AI pack contains no target-specific policy".to_string()
        } else {
            leaks.join("; ")
        },
    );
}

fn audit_no_retired_public_terms(repo_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let mut failures = Vec::new();
    for path in markdown_files(repo_root) {
        let Ok(body) = read_to_string(&path) else {
            continue;
        };
        if path.file_name().is_some_and(|name| name == "CHANGELOG.md") {
            continue;
        }
        let lowered = body.to_ascii_lowercase();
        for term in [
            "wikitool search",
            "wikitool fetch",
            "wikitool seo",
            "wikitool net",
            "--view agent-card",
            "function-card",
            "function-context",
            "minibeast",
        ] {
            if lowered.contains(term) {
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
            "guidance does not mention retired or private public surfaces".to_string()
        } else {
            failures.join("; ")
        },
    );
}

fn audit_host_project(host_root: &Path, checks: &mut Vec<DocsAuditCheck>) {
    let adapter = host_root.join("wikitool_adapter/site-adapter.toml");
    let adapter_ok = read_to_string(&adapter)
        .is_ok_and(|body| body.contains("schema_version = \"site_adapter_v2\""));
    push_check(
        checks,
        "host.site_adapter",
        adapter_ok,
        Some(&adapter),
        if adapter_ok {
            "host owns an explicit typed site adapter".to_string()
        } else {
            "host must own wikitool_adapter/site-adapter.toml with site_adapter_v2".to_string()
        },
    );

    for relative in [
        ".claude/skills/wikitool.md",
        ".claude/skills/wiki-writing.md",
        ".claude/skills/prose-review.md",
        ".claude/skills/wiki-interview.md",
    ] {
        let path = host_root.join(relative);
        let ok = read_to_string(&path).is_ok_and(|body| {
            body.contains("tools/wikitool/ai-pack/") && !body.contains("wikitool search")
        });
        push_check(
            checks,
            "host.skill_redirects",
            ok,
            Some(&path),
            if ok {
                format!("{relative} routes to the public AI pack")
            } else {
                format!("{relative} must route to the public AI pack")
            },
        );
    }

    for relative in [
        ".claude/skills/review.md",
        ".claude/skills/knowledge-interview.md",
    ] {
        let path = host_root.join(relative);
        push_check(
            checks,
            "host.retired_skill_aliases",
            !path.exists(),
            Some(&path),
            if path.exists() {
                format!("retired skill alias {relative} must be removed")
            } else {
                format!("retired skill alias {relative} is absent")
            },
        );
    }
}

fn text_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_files(root, &mut out, &["md", "yaml", "toml"]);
    out
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_files(root, &mut out, &["md"]);
    out
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>, extensions: &[&str]) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if path.is_dir() {
            if !matches!(name, ".git" | "target" | ".wikitool" | "dist") {
                collect_files(&path, out, extensions);
            }
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extensions.iter().any(|item| item == &extension))
        {
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

#[cfg(test)]
mod tests {
    use super::valid_skill_frontmatter;

    #[test]
    fn skill_frontmatter_rejects_extra_policy_keys() {
        assert!(valid_skill_frontmatter(
            "---\nname: demo\ndescription: A useful demo skill.\n---\n",
            "demo"
        ));
        assert!(!valid_skill_frontmatter(
            "---\nname: demo\ndescription: A useful demo skill.\nallowed-tools: Bash\n---\n",
            "demo"
        ));
    }
}
