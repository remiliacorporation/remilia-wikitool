use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCENARIO_SCHEMA: &str = "wikitest.scenario.v1";
pub const SUITE_SCHEMA: &str = "wikitest.suite.v1";
pub const RECEIPT_SCHEMA: &str = "wikitest.run-receipt.v2";
pub const SUITE_RECEIPT_SCHEMA: &str = "wikitest.suite-receipt.v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioKind {
    Mechanical,
    Knowledge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioEnvironment {
    Isolated,
    HostReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingDisposition {
    Fail,
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioManifest {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: ScenarioKind,
    pub environment: ScenarioEnvironment,
    pub timeout_ms: u64,
    pub coverage: Vec<String>,
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteManifest {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub required_coverage: Vec<String>,
    pub scenarios: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Requirement {
    PathExists {
        path: String,
        on_missing: MissingDisposition,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScenarioStep {
    Copy {
        id: String,
        source: String,
        target: String,
        sha256: String,
    },
    Command {
        id: String,
        argv: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        environment: BTreeMap<String, String>,
        expect: CommandExpectation,
    },
}

impl ScenarioStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Copy { id, .. } | Self::Command { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandExpectation {
    pub exit_code: i32,
    #[serde(default)]
    pub stdout: Vec<OutputAssertion>,
    #[serde(default)]
    pub stderr: Vec<OutputAssertion>,
    #[serde(default)]
    pub files: Vec<FileAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputAssertion {
    Contains {
        value: String,
    },
    NotContains {
        value: String,
    },
    JsonPointerExists {
        pointer: String,
    },
    JsonPointerEquals {
        pointer: String,
        value: Value,
    },
    JsonArrayContains {
        pointer: String,
        value: Value,
    },
    JsonArrayItemPointerEquals {
        pointer: String,
        item_pointer: String,
        value: Value,
    },
    JsonPointerU64AtLeast {
        pointer: String,
        value: u64,
    },
    JsonPointerNonBlank {
        pointer: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileAssertion {
    Exists { path: String },
    Missing { path: String },
    Contains { path: String, value: String },
    NotContains { path: String, value: String },
    Sha256 { path: String, value: String },
}

impl FileAssertion {
    pub fn path(&self) -> &str {
        match self {
            Self::Exists { path }
            | Self::Missing { path }
            | Self::Contains { path, .. }
            | Self::NotContains { path, .. }
            | Self::Sha256 { path, .. } => path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Passed,
    Failed,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunReceipt {
    pub schema: String,
    pub run_id: String,
    pub driver: ToolIdentity,
    pub scenario: ScenarioIdentity,
    pub tool: ToolIdentity,
    pub status: RunStatus,
    pub started_at_unix_ms: u128,
    pub finished_at_unix_ms: u128,
    pub duration_ms: u128,
    pub complete: bool,
    pub output_truncated: bool,
    pub inputs: Vec<ArtifactIdentity>,
    pub requirements: Vec<RequirementReceipt>,
    pub steps: Vec<StepReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteReceipt {
    pub schema: String,
    pub run_id: String,
    pub driver: ToolIdentity,
    pub suite: SuiteIdentity,
    pub status: RunStatus,
    pub require_all: bool,
    pub started_at_unix_ms: u128,
    pub finished_at_unix_ms: u128,
    pub duration_ms: u128,
    pub complete: bool,
    pub required_coverage: Vec<String>,
    pub observed_coverage: Vec<String>,
    pub runs: Vec<SuiteRunEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteIdentity {
    pub id: String,
    pub title: String,
    pub locator: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteRunEntry {
    pub scenario: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario_id: Option<String>,
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_locator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioIdentity {
    pub id: String,
    pub title: String,
    pub kind: ScenarioKind,
    pub environment: ScenarioEnvironment,
    pub coverage: Vec<String>,
    pub locator: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    pub locator: String,
    pub sha256: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIdentity {
    pub locator: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementReceipt {
    pub kind: String,
    pub passed: bool,
    pub disposition: MissingDisposition,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepReceipt {
    pub id: String,
    pub action: String,
    pub status: RunStatus,
    pub duration_ms: u128,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timed_out: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<OutputArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<OutputArtifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<AssertionReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputArtifact {
    pub locator: String,
    pub sha256: String,
    pub stored_sha256: String,
    pub observed_bytes: u64,
    pub stored_bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionReceipt {
    pub target: String,
    pub assertion: String,
    pub passed: bool,
    pub detail: String,
}

impl ScenarioManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCENARIO_SCHEMA {
            bail!("unsupported scenario schema '{}'", self.schema);
        }
        validate_key(&self.id, "scenario.id")?;
        non_blank(&self.title, "scenario.title")?;
        non_blank(&self.description, "scenario.description")?;
        bounded_timeout(self.timeout_ms, "scenario.timeout_ms")?;
        validate_unique_keys(&self.coverage, "scenario.coverage")?;
        if self.steps.is_empty() || self.steps.len() > 256 {
            bail!("scenario.steps must contain 1-256 steps");
        }

        let mut ids = BTreeSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            validate_key(step.id(), &format!("scenario.steps[{index}].id"))?;
            if !ids.insert(step.id()) {
                bail!("scenario repeats step id '{}'", step.id());
            }
            self.validate_step(step, index)?;
        }
        for (index, requirement) in self.requirements.iter().enumerate() {
            match requirement {
                Requirement::PathExists { path, .. } => {
                    validate_interpolated_path(
                        path,
                        &format!("scenario.requirements[{index}].path"),
                    )?;
                }
            }
        }
        Ok(())
    }

    fn validate_step(&self, step: &ScenarioStep, index: usize) -> Result<()> {
        let source = format!("scenario.steps[{index}]");
        match step {
            ScenarioStep::Copy {
                source: input,
                target,
                sha256,
                ..
            } => {
                if self.environment == ScenarioEnvironment::HostReadOnly {
                    bail!("host_read_only scenarios cannot contain copy steps");
                }
                validate_relative_path(input, &format!("{source}.source"))?;
                validate_relative_path(target, &format!("{source}.target"))?;
                validate_sha256(sha256, &format!("{source}.sha256"))?;
            }
            ScenarioStep::Command {
                argv,
                cwd,
                timeout_ms,
                environment,
                expect,
                ..
            } => {
                if argv.is_empty() || argv.len() > 128 {
                    bail!("{source}.argv must contain 1-128 arguments");
                }
                if argv.iter().any(|arg| arg.is_empty()) {
                    bail!("{source}.argv contains an empty argument");
                }
                if argv.iter().any(|arg| {
                    ["--project-root", "--data-dir", "--config"]
                        .iter()
                        .any(|owned| arg == owned || arg.starts_with(&format!("{owned}=")))
                }) {
                    bail!("{source}.argv cannot override runner-owned paths");
                }
                if let Some(path) = cwd {
                    validate_relative_path(path, &format!("{source}.cwd"))?;
                }
                if let Some(timeout) = timeout_ms {
                    bounded_timeout(*timeout, &format!("{source}.timeout_ms"))?;
                }
                for key in environment.keys() {
                    validate_environment_key(key, &format!("{source}.environment"))?;
                }
                validate_expectation(expect, &format!("{source}.expect"))?;
                if self.environment == ScenarioEnvironment::HostReadOnly {
                    validate_read_only_argv(argv)?;
                }
            }
        }
        Ok(())
    }
}

impl SuiteManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema != SUITE_SCHEMA {
            bail!("unsupported suite schema '{}'", self.schema);
        }
        validate_key(&self.id, "suite.id")?;
        non_blank(&self.title, "suite.title")?;
        validate_unique_keys(&self.required_coverage, "suite.required_coverage")?;
        if self.scenarios.is_empty() || self.scenarios.len() > 256 {
            bail!("suite.scenarios must contain 1-256 locators");
        }
        let mut unique = BTreeSet::new();
        for (index, scenario) in self.scenarios.iter().enumerate() {
            validate_key(scenario, &format!("suite.scenarios[{index}]"))?;
            if !unique.insert(scenario) {
                bail!("suite repeats scenario id '{scenario}'");
            }
        }
        Ok(())
    }
}

fn validate_expectation(expect: &CommandExpectation, source: &str) -> Result<()> {
    for (index, assertion) in expect.stdout.iter().enumerate() {
        validate_output_assertion(assertion, &format!("{source}.stdout[{index}]"))?;
    }
    for (index, assertion) in expect.stderr.iter().enumerate() {
        validate_output_assertion(assertion, &format!("{source}.stderr[{index}]"))?;
    }
    for (index, assertion) in expect.files.iter().enumerate() {
        validate_relative_path(assertion.path(), &format!("{source}.files[{index}].path"))?;
        match assertion {
            FileAssertion::Contains { value, .. } | FileAssertion::NotContains { value, .. } => {
                non_blank(value, &format!("{source}.files[{index}].value"))?;
            }
            FileAssertion::Sha256 { value, .. } => {
                validate_sha256(value, &format!("{source}.files[{index}].value"))?;
            }
            FileAssertion::Exists { .. } | FileAssertion::Missing { .. } => {}
        }
    }
    Ok(())
}

fn validate_output_assertion(assertion: &OutputAssertion, source: &str) -> Result<()> {
    match assertion {
        OutputAssertion::Contains { value } | OutputAssertion::NotContains { value } => {
            non_blank(value, &format!("{source}.value"))?;
        }
        OutputAssertion::JsonPointerExists { pointer }
        | OutputAssertion::JsonPointerEquals { pointer, .. }
        | OutputAssertion::JsonArrayContains { pointer, .. }
        | OutputAssertion::JsonPointerU64AtLeast { pointer, .. }
        | OutputAssertion::JsonPointerNonBlank { pointer } => {
            if !pointer.is_empty() && !pointer.starts_with('/') {
                bail!("{source}.pointer must be empty or start with '/'");
            }
        }
        OutputAssertion::JsonArrayItemPointerEquals {
            pointer,
            item_pointer,
            ..
        } => {
            for (field, value) in [("pointer", pointer), ("item_pointer", item_pointer)] {
                if !value.is_empty() && !value.starts_with('/') {
                    bail!("{source}.{field} must be empty or start with '/'");
                }
            }
        }
    }
    Ok(())
}

pub fn validate_key(value: &str, source: &str) -> Result<()> {
    if value.is_empty() || value.len() > 80 {
        bail!("{source} must contain 1-80 characters");
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("{source} is empty");
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        bail!("{source} must begin with a lowercase ASCII letter or digit");
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
        bail!("{source} may contain only lowercase ASCII letters, digits, and hyphens");
    }
    Ok(())
}

pub fn validate_relative_path(value: &str, source: &str) -> Result<()> {
    non_blank(value, source)?;
    if value.contains('\\') || value.starts_with('/') || value.contains(':') {
        bail!("{source} must be a forward-slash relative path");
    }
    let components = value.split('/').collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| component.is_empty() || *component == "." || *component == "..")
    {
        bail!("{source} contains an empty, current, or parent component");
    }
    Ok(())
}

fn validate_interpolated_path(value: &str, source: &str) -> Result<()> {
    non_blank(value, source)?;
    if value.contains('\0') {
        bail!("{source} contains NUL");
    }
    Ok(())
}

fn validate_environment_key(value: &str, source: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        bail!("{source} key '{value}' must use uppercase ASCII letters, digits, or underscore");
    }
    Ok(())
}

fn validate_read_only_argv(argv: &[String]) -> Result<()> {
    let allowed = match argv {
        [command, ..] if command == "config" || command == "status" => true,
        [command, subcommand, ..] if command == "db" && subcommand == "stats" => true,
        [command, subcommand, ..]
            if command == "knowledge"
                && (subcommand == "status"
                    || subcommand == "inspect"
                    || subcommand == "article-start") =>
        {
            true
        }
        [command, subcommand, ..]
            if command == "templates" && (subcommand == "show" || subcommand == "examples") =>
        {
            true
        }
        [command, subcommand, ..] if command == "article" && subcommand == "lint" => true,
        [command, ..] if command == "validate" => true,
        [command, subcommand, ..] if command == "research" && subcommand == "wiki-search" => true,
        [command, profile, subcommand, ..]
            if command == "wiki" && profile == "profile" && subcommand == "show" =>
        {
            true
        }
        _ => false,
    };
    if !allowed {
        bail!(
            "host_read_only scenario command is outside the read-only allowlist: {:?}",
            argv
        );
    }
    Ok(())
}

fn bounded_timeout(value: u64, source: &str) -> Result<()> {
    if !(1_000..=1_800_000).contains(&value) {
        bail!("{source} must be in [1000, 1800000]");
    }
    Ok(())
}

fn non_blank(value: &str, source: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{source} must be nonblank");
    }
    Ok(())
}

fn validate_sha256(value: &str, source: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        bail!("{source} must be a lowercase hexadecimal SHA-256 digest");
    }
    Ok(())
}

fn validate_unique_keys(values: &[String], source: &str) -> Result<()> {
    if values.is_empty() || values.len() > 256 {
        bail!("{source} must contain 1-256 values");
    }
    let mut unique = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        validate_key(value, &format!("{source}[{index}]"))?;
        if !unique.insert(value) {
            bail!("{source} repeats '{value}'");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scenario() -> ScenarioManifest {
        serde_json::from_value(json!({
            "schema": SCENARIO_SCHEMA,
            "id": "mechanical-smoke",
            "title": "Mechanical smoke",
            "description": "Exercises a deterministic command.",
            "kind": "mechanical",
            "environment": "isolated",
            "timeout_ms": 10_000,
            "coverage": ["public-cli"],
            "steps": [{
                "action": "command",
                "id": "status",
                "argv": ["status"],
                "expect": {"exit_code": 0}
            }]
        }))
        .expect("scenario JSON")
    }

    #[test]
    fn strict_scenario_validates() {
        scenario().validate().expect("valid scenario");
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value = serde_json::to_value(scenario()).expect("serialize");
        value["prompt"] = json!("hidden doctrine");
        let error = serde_json::from_value::<ScenarioManifest>(value).expect_err("unknown field");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn host_mode_refuses_mutating_commands() {
        let mut value = scenario();
        value.environment = ScenarioEnvironment::HostReadOnly;
        let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        *argv = vec!["knowledge".into(), "build".into()];
        let error = value.validate().expect_err("mutating command must fail");
        assert!(error.to_string().contains("read-only allowlist"));
    }

    #[test]
    fn copy_paths_cannot_escape() {
        let mut value = scenario();
        value.steps[0] = ScenarioStep::Copy {
            id: "escape".into(),
            source: "../secret".into(),
            target: "wiki_content/Main/Secret.wiki".into(),
            sha256: "0".repeat(64),
        };
        assert!(value.validate().is_err());
    }

    #[test]
    fn host_mode_accepts_knowledge_inspection() {
        let mut value = scenario();
        value.environment = ScenarioEnvironment::HostReadOnly;
        let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        *argv = vec![
            "knowledge".into(),
            "inspect".into(),
            "stats".into(),
            "--format".into(),
            "json".into(),
        ];
        value.validate().expect("read-only knowledge command");
    }

    #[test]
    fn host_mode_accepts_article_start() {
        let mut value = scenario();
        value.environment = ScenarioEnvironment::HostReadOnly;
        let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        *argv = vec![
            "knowledge".into(),
            "article-start".into(),
            "Remilia Corporation".into(),
            "--intent".into(),
            "audit".into(),
            "--format".into(),
            "json".into(),
        ];
        value.validate().expect("read-only article start");
    }
}
