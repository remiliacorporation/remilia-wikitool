use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCENARIO_SCHEMA: &str = "wikitest.scenario.v4";
pub const SUITE_SCHEMA: &str = "wikitest.suite.v1";
pub const RECEIPT_SCHEMA: &str = "wikitest.run-receipt.v5";
pub const SUITE_RECEIPT_SCHEMA: &str = "wikitest.suite-receipt.v3";
pub const REQUIREMENT_OBSERVATION_SCHEMA: &str = "wikitest.requirement-observation.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioKind {
    Mechanical,
    Catalog,
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
    pub coverage: Vec<CoverageBinding>,
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mediawiki_fixture: Option<MediaWikiFixtureRef>,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageBinding {
    pub capability: String,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiFixtureRef {
    pub source: String,
    pub sha256: String,
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        captures: Vec<JsonScalarCapture>,
        expect: CommandExpectation,
    },
    #[serde(rename = "mediawiki_update")]
    MediaWikiUpdate { id: String, page: MediaWikiPage },
    #[serde(rename = "mediawiki_assert")]
    MediaWikiAssert {
        id: String,
        expect: MediaWikiExpectation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonScalarCapture {
    pub name: String,
    pub pointer: String,
}

impl ScenarioStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Copy { id, .. }
            | Self::Command { id, .. }
            | Self::MediaWikiUpdate { id, .. }
            | Self::MediaWikiAssert { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiPage {
    pub title: String,
    pub namespace: i32,
    pub page_id: i64,
    pub revision_id: i64,
    pub timestamp: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiExpectation {
    #[serde(default)]
    pub requests: Vec<MediaWikiRequestExpectation>,
    #[serde(default)]
    pub pages: Vec<MediaWikiPageExpectation>,
    #[serde(default)]
    pub missing_pages: Vec<String>,
    #[serde(default)]
    pub delete_logs: Vec<MediaWikiDeleteLogExpectation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiRequestExpectation {
    pub method: String,
    pub params: BTreeMap<String, String>,
    #[serde(default = "one")]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiPageExpectation {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaWikiDeleteLogExpectation {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_hidden: Option<bool>,
    #[serde(default = "one")]
    pub count: u64,
}

const fn one() -> u64 {
    1
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
    pub coverage: Vec<CoverageBinding>,
    pub locator: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    pub locator: String,
    pub sha256: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub observation: ArtifactIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementObservation {
    pub schema: String,
    pub kind: String,
    pub declared_path: String,
    pub expanded_path: String,
    pub exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_kind: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captures: Vec<JsonScalarCaptureReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonScalarCaptureReceipt {
    pub name: String,
    pub source_step: String,
    pub pointer: String,
    pub value: String,
    pub source_stdout_sha256: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_evidence: Option<FileObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileObservation {
    pub state: FileObservationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileObservationState {
    Missing,
    PlainFile,
    Other,
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
        if self.steps.is_empty() || self.steps.len() > 256 {
            bail!("scenario.steps must contain 1-256 steps");
        }

        let mut ids = BTreeSet::new();
        let mut available_captures = BTreeSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            validate_key(step.id(), &format!("scenario.steps[{index}].id"))?;
            if !ids.insert(step.id()) {
                bail!("scenario repeats step id '{}'", step.id());
            }
            self.validate_step(step, index)?;
            if let ScenarioStep::Command {
                argv,
                environment,
                captures,
                ..
            } = step
            {
                for (value_index, value) in argv.iter().enumerate() {
                    validate_capture_references(
                        value,
                        &format!("scenario.steps[{index}].argv[{value_index}]"),
                        &available_captures,
                    )?;
                }
                for (key, value) in environment {
                    validate_capture_references(
                        value,
                        &format!("scenario.steps[{index}].environment[{key:?}]"),
                        &available_captures,
                    )?;
                }
                for capture in captures {
                    if !available_captures.insert(capture.name.as_str()) {
                        bail!(
                            "scenario.steps[{index}] redefines capture '{}'",
                            capture.name
                        );
                    }
                }
            }
        }
        let mut capabilities = BTreeSet::new();
        for (index, binding) in self.coverage.iter().enumerate() {
            validate_key(
                &binding.capability,
                &format!("scenario.coverage[{index}].capability"),
            )?;
            if !capabilities.insert(binding.capability.as_str()) {
                bail!(
                    "scenario repeats coverage capability '{}'",
                    binding.capability
                );
            }
            if binding.steps.is_empty() {
                bail!("scenario.coverage[{index}].steps must not be empty");
            }
            validate_unique_keys(&binding.steps, &format!("scenario.coverage[{index}].steps"))?;
            for step in &binding.steps {
                if !ids.contains(step.as_str()) {
                    bail!(
                        "scenario coverage capability '{}' references unknown step '{}'",
                        binding.capability,
                        step
                    );
                }
            }
        }
        if let Some(fixture) = &self.mediawiki_fixture {
            if self.environment != ScenarioEnvironment::Isolated {
                bail!("mediawiki fixtures require an isolated scenario");
            }
            validate_relative_path(&fixture.source, "scenario.mediawiki_fixture.source")?;
            validate_sha256(&fixture.sha256, "scenario.mediawiki_fixture.sha256")?;
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
                captures,
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
                    validate_scenario_environment_key(key, &format!("{source}.environment"))?;
                }
                if captures.len() > 32 {
                    bail!("{source}.captures must contain at most 32 entries");
                }
                if self.environment == ScenarioEnvironment::HostReadOnly && !captures.is_empty() {
                    bail!(
                        "{source}.captures are unavailable in host_read_only scenarios because dynamic argv would bypass the static isolation audit"
                    );
                }
                for (capture_index, capture) in captures.iter().enumerate() {
                    let capture_source = format!("{source}.captures[{capture_index}]");
                    validate_capture_name(&capture.name, &format!("{capture_source}.name"))?;
                    validate_json_pointer(&capture.pointer, &format!("{capture_source}.pointer"))?;
                }
                validate_expectation(expect, &format!("{source}.expect"))?;
                if self.environment == ScenarioEnvironment::HostReadOnly {
                    validate_read_only_argv(argv)?;
                    validate_host_command_isolation(argv, environment)?;
                }
            }
            ScenarioStep::MediaWikiUpdate { page, .. } => {
                self.require_mediawiki_fixture(&source)?;
                validate_mediawiki_page(page, &format!("{source}.page"))?;
            }
            ScenarioStep::MediaWikiAssert { expect, .. } => {
                self.require_mediawiki_fixture(&source)?;
                validate_mediawiki_expectation(expect, &format!("{source}.expect"))?;
            }
        }
        Ok(())
    }

    fn require_mediawiki_fixture(&self, source: &str) -> Result<()> {
        if self.environment != ScenarioEnvironment::Isolated {
            bail!("{source} is only valid in isolated scenarios");
        }
        if self.mediawiki_fixture.is_none() {
            bail!("{source} requires scenario.mediawiki_fixture");
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

fn validate_mediawiki_page(page: &MediaWikiPage, source: &str) -> Result<()> {
    non_blank(&page.title, &format!("{source}.title"))?;
    if page.page_id <= 0 {
        bail!("{source}.page_id must be positive");
    }
    if page.revision_id <= 0 {
        bail!("{source}.revision_id must be positive");
    }
    non_blank(&page.timestamp, &format!("{source}.timestamp"))?;
    Ok(())
}

fn validate_mediawiki_expectation(expect: &MediaWikiExpectation, source: &str) -> Result<()> {
    if expect.requests.is_empty()
        && expect.pages.is_empty()
        && expect.missing_pages.is_empty()
        && expect.delete_logs.is_empty()
    {
        bail!("{source} must declare at least one request, page, or delete-log expectation");
    }
    for (index, request) in expect.requests.iter().enumerate() {
        let request_source = format!("{source}.requests[{index}]");
        match request.method.as_str() {
            "GET" | "POST" => {}
            other => bail!("{request_source}.method must be GET or POST, got {other:?}"),
        }
        if request.params.is_empty() {
            bail!("{request_source}.params must not be empty");
        }
        for (key, value) in &request.params {
            non_blank(key, &format!("{request_source}.params key"))?;
            non_blank(value, &format!("{request_source}.params[{key:?}]"))?;
        }
        if request.count == 0 {
            bail!("{request_source}.count must be nonzero");
        }
    }
    for (index, page) in expect.pages.iter().enumerate() {
        let page_source = format!("{source}.pages[{index}]");
        non_blank(&page.title, &format!("{page_source}.title"))?;
        if let Some(revision_id) = page.revision_id
            && revision_id <= 0
        {
            bail!("{page_source}.revision_id must be positive");
        }
        if let Some(digest) = &page.content_sha256 {
            validate_sha256(digest, &format!("{page_source}.content_sha256"))?;
        }
        if page.revision_id.is_none() && page.content_sha256.is_none() {
            bail!("{page_source} must check revision_id, content_sha256, or both");
        }
    }
    let mut page_titles = BTreeSet::new();
    for page in &expect.pages {
        if !page_titles.insert(page.title.as_str()) {
            bail!("{source}.pages repeats title {:?}", page.title);
        }
    }
    for (index, title) in expect.missing_pages.iter().enumerate() {
        non_blank(title, &format!("{source}.missing_pages[{index}]"))?;
        if !page_titles.insert(title) {
            bail!("{source} declares page {title:?} as both present and missing");
        }
    }
    for (index, log) in expect.delete_logs.iter().enumerate() {
        let log_source = format!("{source}.delete_logs[{index}]");
        non_blank(&log.title, &format!("{log_source}.title"))?;
        if let Some(value) = &log.comment_contains {
            non_blank(value, &format!("{log_source}.comment_contains"))?;
            if log.comment_hidden == Some(true) {
                bail!("{log_source}.comment_contains cannot be combined with comment_hidden=true");
            }
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

fn validate_json_pointer(value: &str, source: &str) -> Result<()> {
    if !value.is_empty() && !value.starts_with('/') {
        bail!("{source} must be empty or start with '/'");
    }
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            if index + 1 >= bytes.len() || !matches!(bytes[index + 1], b'0' | b'1') {
                bail!("{source} contains an invalid JSON Pointer escape");
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn validate_capture_name(value: &str, source: &str) -> Result<()> {
    if value.is_empty() || value.len() > 80 {
        bail!("{source} must contain 1-80 characters");
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("{source} is empty");
    };
    if !first.is_ascii_uppercase() {
        bail!("{source} must begin with an uppercase ASCII letter");
    }
    if !chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_') {
        bail!("{source} may contain only uppercase ASCII letters, digits, and underscores");
    }
    Ok(())
}

fn validate_capture_references(
    value: &str,
    source: &str,
    available: &BTreeSet<&str>,
) -> Result<()> {
    let mut remaining = value;
    while let Some(start) = remaining.find("${") {
        let after_start = &remaining[start + 2..];
        let end = after_start
            .find('}')
            .with_context(|| format!("{source} contains an unterminated capture reference"))?;
        let name = &after_start[..end];
        validate_capture_name(name, &format!("{source} capture reference"))?;
        if !available.contains(name) {
            bail!("{source} references capture '{name}' before it is defined");
        }
        remaining = &after_start[end + 1..];
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

fn validate_scenario_environment_key(value: &str, source: &str) -> Result<()> {
    if value.starts_with("WIKITOOL_") && !matches!(value, "WIKITOOL_BOT_USER" | "WIKITOOL_BOT_PASS")
    {
        bail!("{source} key '{value}' cannot re-enable Wikitool path, API, or runtime overrides");
    }
    Ok(())
}

fn validate_read_only_argv(argv: &[String]) -> Result<()> {
    let allowed = match argv {
        [command, ..] if command == "config" || command == "status" => true,
        [command, subcommand, ..] if command == "db" && subcommand == "stats" => true,
        [command, subcommand, ..]
            if command == "catalog" && (subcommand == "status" || subcommand == "inspect") =>
        {
            true
        }
        [command, subcommand, ..]
            if command == "templates" && (subcommand == "show" || subcommand == "examples") =>
        {
            true
        }
        [command, subcommand, ..] if command == "article" && subcommand == "lint" => true,
        [command, subcommand, ..] if command == "article" && subcommand == "scout" => true,
        [command, ..] if command == "validate" => true,
        [command, subcommand, ..] if command == "source" && subcommand == "wiki-search" => true,
        [command, capability, subcommand, ..]
            if command == "wiki" && capability == "capabilities" && subcommand == "show" =>
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

fn validate_host_command_isolation(
    argv: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    const EXTERNAL_ROOT_TOKENS: &[&str] =
        &["{HOST_ROOT}", "{REPO_ROOT}", "{SCENARIO_DIR}", "{RUN_DIR}"];
    if argv.iter().chain(environment.values()).any(|value| {
        EXTERNAL_ROOT_TOKENS
            .iter()
            .any(|token| value.contains(token))
    }) {
        bail!("host_read_only commands may reference only their isolated {{WORKSPACE}} snapshot");
    }
    if argv
        .iter()
        .chain(environment.values())
        .any(|value| value_contains_external_path(value))
    {
        bail!(
            "host_read_only commands cannot contain absolute paths or parent-directory traversal"
        );
    }
    Ok(())
}

fn value_contains_external_path(value: &str) -> bool {
    let candidate = value.split_once('=').map_or(value, |(_, suffix)| suffix);
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return false;
    }
    let path = std::path::Path::new(candidate);
    path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
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
            "coverage": [{"capability": "public-cli", "steps": ["status"]}],
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
        *argv = vec!["catalog".into(), "build".into()];
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
    fn host_mode_accepts_catalog_inspection() {
        let mut value = scenario();
        value.environment = ScenarioEnvironment::HostReadOnly;
        let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        *argv = vec![
            "catalog".into(),
            "inspect".into(),
            "stats".into(),
            "--format".into(),
            "json".into(),
        ];
        value.validate().expect("read-only catalog command");
    }

    #[test]
    fn host_mode_accepts_article_scout() {
        let mut value = scenario();
        value.environment = ScenarioEnvironment::HostReadOnly;
        let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        *argv = vec![
            "article".into(),
            "scout".into(),
            "Remilia Corporation".into(),
            "--intent".into(),
            "audit".into(),
            "--format".into(),
            "json".into(),
        ];
        value.validate().expect("read-only article scout");
    }

    #[test]
    fn host_mode_refuses_paths_outside_the_isolated_snapshot() {
        let mut value = scenario();
        value.environment = ScenarioEnvironment::HostReadOnly;
        let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        *argv = vec![
            "catalog".into(),
            "status".into(),
            "--probe".into(),
            "{HOST_ROOT}/.wikitool/data/wikitool.db".into(),
        ];
        let error = value.validate().expect_err("external host path must fail");
        assert!(error.to_string().contains("isolated {WORKSPACE} snapshot"));
    }

    #[test]
    fn host_mode_refuses_literal_absolute_and_parent_paths() {
        for external in ["C:/outside/wiki.db", "../outside/wiki.db"] {
            let mut value = scenario();
            value.environment = ScenarioEnvironment::HostReadOnly;
            let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
                panic!("command step");
            };
            *argv = vec![
                "catalog".into(),
                "status".into(),
                "--probe".into(),
                external.into(),
            ];
            let error = value.validate().expect_err("external path must fail");
            assert!(error.to_string().contains("absolute paths"));
        }
    }

    #[test]
    fn scenario_environment_refuses_wikitool_runtime_overrides() {
        let mut value = scenario();
        let ScenarioStep::Command { environment, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        environment.insert(
            "WIKITOOL_WIKI_API_URL".to_owned(),
            "https://hostile.invalid/api.php".to_owned(),
        );
        let error = value.validate().expect_err("runtime override must fail");
        assert!(error.to_string().contains("cannot re-enable Wikitool"));
    }

    #[test]
    fn scenario_environment_allows_fixture_credentials() {
        let mut value = scenario();
        let ScenarioStep::Command { environment, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        environment.insert("WIKITOOL_BOT_USER".to_owned(), "fixture-bot".to_owned());
        environment.insert("WIKITOOL_BOT_PASS".to_owned(), "fixture-pass".to_owned());
        value.validate().expect("fixture credentials");
    }

    #[test]
    fn captures_must_be_defined_once_before_use() {
        let mut value = scenario();
        let ScenarioStep::Command { captures, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        captures.push(JsonScalarCapture {
            name: "PLAN_ID".to_owned(),
            pointer: "/report/plan_id".to_owned(),
        });
        value.steps.push(
            serde_json::from_value(json!({
                "action": "command",
                "id": "apply",
                "argv": ["push", "--apply", "${PLAN_ID}"],
                "expect": {"exit_code": 0}
            }))
            .expect("apply step"),
        );
        value.validate().expect("prior capture may be used");

        let ScenarioStep::Command { captures, .. } = &mut value.steps[1] else {
            panic!("command step");
        };
        captures.push(JsonScalarCapture {
            name: "PLAN_ID".to_owned(),
            pointer: "/report/plan_id".to_owned(),
        });
        let error = value
            .validate()
            .expect_err("capture redefinition must fail");
        assert!(error.to_string().contains("redefines capture 'PLAN_ID'"));

        let mut value = scenario();
        let ScenarioStep::Command { argv, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        argv.push("${FUTURE_PLAN_ID}".to_owned());
        let error = value
            .validate()
            .expect_err("capture use before definition must fail");
        assert!(error.to_string().contains("before it is defined"));

        let mut value = scenario();
        value.environment = ScenarioEnvironment::HostReadOnly;
        let ScenarioStep::Command { captures, .. } = &mut value.steps[0] else {
            panic!("command step");
        };
        captures.push(JsonScalarCapture {
            name: "DYNAMIC_ARG".to_owned(),
            pointer: "/value".to_owned(),
        });
        let error = value
            .validate()
            .expect_err("host capture must not bypass static isolation");
        assert!(error.to_string().contains("static isolation audit"));
    }
}
