use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::artifact::join_relative;
use crate::model::{ArtifactIdentity, OutputArtifact, ToolIdentity};

pub const PROSE_ASSIGNMENT_SCHEMA: &str = "wikitest.prose-assignment.v3";
pub const PROSE_SUITE_SCHEMA: &str = "wikitest.prose-suite.v2";
pub const PROSE_PACKET_SCHEMA: &str = "wikitest.prose-packet.v4";
pub const AUTHOR_REQUEST_SCHEMA: &str = "wikitest.author-request.v1";
pub const AUTHOR_SUBMISSION_SCHEMA: &str = "wikitest.author-submission.v2";
pub const CLAIM_MAP_SCHEMA: &str = "wikitest.claim-map.v1";
pub const REVIEW_REQUEST_SCHEMA: &str = "wikitest.review-request.v2";
pub const REVIEW_SUBMISSION_SCHEMA: &str = "wikitest.review-submission.v2";
pub const PROSE_RECEIPT_SCHEMA: &str = "wikitest.prose-receipt.v7";
pub const PROSE_SUITE_RECEIPT_SCHEMA: &str = "wikitest.prose-suite-receipt.v7";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseMode {
    Authoring,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseComplexity {
    Focused,
    Complex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseCampaign {
    Calibration,
    Stress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleDecision {
    Article,
    Redirect,
    Merge,
    NoArticle,
    Hold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRole {
    Primary,
    IndependentSecondary,
    Authoritative,
    HumanStatement,
    Fixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDisposition {
    Accept,
    Revise,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewScope {
    Complete,
    Sampled,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceVerdict {
    Complete,
    Incomplete,
    NotAssessable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    P0,
    P1,
    P2,
    P3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingBasis {
    Verified,
    Inference,
    Limitation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisVerdict {
    Pass,
    Fail,
    Concern,
    NotAssessed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleAxisExpectation {
    pub axis: String,
    pub allowed_verdicts: Vec<AxisVerdict>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantKind {
    Human,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseRunStatus {
    AwaitingAuthor,
    AwaitingReview,
    ReviewedAccept,
    ReviewedRevise,
    ReviewedBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseSuiteStatus {
    Prepared,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProseCoverageStatus {
    AwaitingReview,
    MissingOracle,
    OracleFailed,
    ReviewIncomplete,
    AuthoringIncomplete,
    AuthoringRejected,
    Demonstrated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputRoot {
    Assignment,
    Repository,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProseAssignment {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub mode: ProseMode,
    pub complexity: ProseComplexity,
    pub coverage: Vec<String>,
    pub article: ArticleBrief,
    #[serde(default)]
    pub author_instructions: Vec<BoundInput>,
    pub review_instructions: Vec<BoundInput>,
    #[serde(default)]
    pub context: Vec<BoundInput>,
    pub sources: Vec<SourceInput>,
    #[serde(default)]
    pub do_not_assert: Vec<String>,
    #[serde(default)]
    pub allowed_decisions: Vec<ArticleDecision>,
    pub finding_tag_vocabulary: Vec<String>,
    pub review_axes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<BoundInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_map: Option<BoundInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle: Option<ReviewOracle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProseSuite {
    pub schema: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub campaign: ProseCampaign,
    pub required_coverage: Vec<String>,
    pub assignments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArticleBrief {
    pub title: String,
    pub namespace: String,
    pub article_object: String,
    pub reader_need: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundInput {
    pub id: String,
    pub root: InputRoot,
    pub locator: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub id: String,
    pub title: String,
    pub locator: String,
    pub sha256: String,
    pub role: SourceRole,
    pub citation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewOracle {
    #[serde(default)]
    pub required_finding_tags: Vec<String>,
    #[serde(default)]
    pub forbidden_finding_tags: Vec<String>,
    #[serde(default)]
    pub axis_expectations: Vec<OracleAxisExpectation>,
    pub allowed_dispositions: Vec<ReviewDisposition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Participant {
    pub id: String,
    pub display_name: String,
    pub kind: ParticipantKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<AgentExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentExecution {
    pub provider: String,
    pub model: String,
    pub harness: String,
    pub harness_version: String,
    pub invocation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub access: AgentAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<AgentMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAccess {
    pub network: bool,
    pub ambient_repository: bool,
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionFile {
    pub locator: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorSubmission {
    pub schema: String,
    pub run_id: String,
    pub assignment_id: String,
    pub packet_sha256: String,
    pub author: Participant,
    pub decision: ArticleDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<SubmissionFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_map: Option<SubmissionFile>,
    pub notes: String,
    #[serde(default)]
    pub holds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimMap {
    pub schema: String,
    pub article_title: String,
    pub candidate_sha256: String,
    pub claims: Vec<ClaimEntry>,
    #[serde(default)]
    pub holds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimEntry {
    pub id: String,
    pub claim: String,
    pub evidence: Vec<ClaimEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimEvidence {
    pub source_id: String,
    pub locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewSubmission {
    pub schema: String,
    pub run_id: String,
    pub assignment_id: String,
    pub review_packet_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_sha256: Option<String>,
    pub finding_tag_vocabulary: Vec<String>,
    pub reviewer: Participant,
    pub scope: ReviewScope,
    pub reader_verdict: String,
    pub source_verdict: SourceVerdict,
    pub disposition: ReviewDisposition,
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
    #[serde(default)]
    pub observations: Vec<ReviewObservation>,
    pub axes: Vec<ReviewAxis>,
    pub residual_risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    pub id: String,
    pub severity: FindingSeverity,
    pub location: String,
    pub problem: String,
    pub evidence: String,
    pub impact: String,
    pub repair_direction: String,
    pub basis: FindingBasis,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewObservation {
    pub tag: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAxis {
    pub axis: String,
    pub verdict: AxisVerdict,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketBinding {
    pub schema: String,
    pub assignment: ProseAssignmentIdentity,
    pub inputs: Vec<ArtifactIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPacketBinding {
    pub schema: String,
    pub assignment: ReviewAssignmentProjection,
    pub inputs: Vec<ArtifactIdentity>,
    pub mechanical_observations: Vec<MechanicalObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAssignmentProjection {
    pub participant_assignment_id: String,
    pub article: ArticleBrief,
    pub finding_tag_vocabulary: Vec<String>,
    pub review_axes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketInput {
    pub id: String,
    pub artifact: ArtifactIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketSource {
    pub id: String,
    pub title: String,
    pub role: SourceRole,
    pub citation: String,
    pub artifact: ArtifactIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorRequest {
    pub schema: String,
    pub run_id: String,
    pub assignment_id: String,
    pub packet_sha256: String,
    pub artifact_root: String,
    pub article: ArticleBrief,
    pub instructions: Vec<PacketInput>,
    pub context: Vec<PacketInput>,
    pub sources: Vec<PacketSource>,
    pub do_not_assert: Vec<String>,
    pub allowed_decisions: Vec<ArticleDecision>,
    pub candidate_format: String,
    pub claim_map_schema: String,
    pub submission_schema: String,
    pub submission_template: ArtifactIdentity,
    pub claim_map_template: ArtifactIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRequest {
    pub schema: String,
    pub run_id: String,
    pub assignment_id: String,
    pub review_packet_sha256: String,
    pub artifact_root: String,
    pub article: ArticleBrief,
    pub instructions: Vec<PacketInput>,
    pub context: Vec<PacketInput>,
    pub sources: Vec<PacketSource>,
    pub candidate: Option<ArtifactIdentity>,
    pub mechanical_observations: Vec<MechanicalObservation>,
    pub finding_tag_vocabulary: Vec<String>,
    pub review_axes: Vec<String>,
    pub axis_verdict_values: Vec<AxisVerdict>,
    pub submission_schema: String,
    pub submission_template: ArtifactIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProseAssignmentIdentity {
    pub id: String,
    pub title: String,
    pub mode: ProseMode,
    pub coverage: Vec<String>,
    pub locator: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanicalObservation {
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<ArtifactIdentity>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u128,
    pub stdout: OutputArtifact,
    pub stderr: OutputArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorStageReceipt {
    pub submitted_at_unix_ms: u128,
    pub author: Participant,
    pub decision: ArticleDecision,
    pub submission: ArtifactIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_map: Option<ArtifactIdentity>,
    pub notes: String,
    pub holds: Vec<String>,
    pub mechanical_observations: Vec<MechanicalObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OracleEvaluation {
    pub passed: bool,
    pub missing_required_tags: Vec<String>,
    pub present_forbidden_tags: Vec<String>,
    pub failed_axis_expectations: Vec<String>,
    pub disposition_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewStageReceipt {
    pub submitted_at_unix_ms: u128,
    pub reviewer: Participant,
    pub submission: ArtifactIdentity,
    pub scope: ReviewScope,
    pub reader_verdict: String,
    pub source_verdict: SourceVerdict,
    pub disposition: ReviewDisposition,
    pub finding_count: usize,
    pub residual_risk: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle: Option<OracleEvaluation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProseReceipt {
    pub schema: String,
    pub run_id: String,
    pub participant_assignment_id: String,
    pub driver: ToolIdentity,
    pub public_assignment: ProseAssignmentIdentity,
    pub tool: ToolIdentity,
    pub status: ProseRunStatus,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
    pub evaluation_complete: bool,
    pub authority: ArtifactIdentity,
    pub packet: ArtifactIdentity,
    pub inputs: Vec<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_request: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_export: Option<ParticipantExport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<AuthorStageReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_request: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_export: Option<ParticipantExport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_packet: Option<ArtifactIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewStageReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProseSuiteIdentity {
    pub id: String,
    pub title: String,
    pub locator: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProseSuiteRun {
    pub assignment_id: String,
    pub run_locator: String,
    pub preparation_receipt: ArtifactIdentity,
    pub current_receipt: ArtifactIdentity,
    pub status: ProseRunStatus,
    pub coverage_status: ProseCoverageStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticipantExport {
    pub root: String,
    pub request: ArtifactIdentity,
    pub output_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProseSuiteReceipt {
    pub schema: String,
    pub run_id: String,
    pub driver: ToolIdentity,
    pub suite: ProseSuiteIdentity,
    pub created_at_unix_ms: u128,
    pub updated_at_unix_ms: u128,
    pub status: ProseSuiteStatus,
    pub evaluation_complete: bool,
    pub required_coverage: Vec<String>,
    pub prepared_coverage: Vec<String>,
    pub demonstrated_coverage: Vec<String>,
    pub runs: Vec<ProseSuiteRun>,
}

impl ProseAssignment {
    pub fn validate(&self) -> Result<()> {
        if self.schema != PROSE_ASSIGNMENT_SCHEMA {
            bail!("unsupported prose assignment schema '{}'", self.schema);
        }
        validate_key(&self.id, "assignment.id")?;
        non_blank(&self.title, "assignment.title")?;
        non_blank(&self.description, "assignment.description")?;
        validate_keys(&self.coverage, "assignment.coverage", false)?;
        self.article.validate()?;
        validate_bound_inputs(&self.author_instructions, "assignment.author_instructions")?;
        validate_bound_inputs(&self.review_instructions, "assignment.review_instructions")?;
        validate_bound_inputs(&self.context, "assignment.context")?;
        if self.sources.is_empty() || self.sources.len() > 128 {
            bail!("assignment.sources must contain 1-128 sources");
        }
        let mut source_ids = BTreeSet::new();
        for (index, source) in self.sources.iter().enumerate() {
            source.validate(&format!("assignment.sources[{index}]"))?;
            if !source_ids.insert(source.id.as_str()) {
                bail!("assignment.sources repeats id '{}'", source.id);
            }
        }
        validate_texts(&self.do_not_assert, "assignment.do_not_assert")?;
        validate_keys(
            &self.finding_tag_vocabulary,
            "assignment.finding_tag_vocabulary",
            true,
        )?;
        validate_keys(&self.review_axes, "assignment.review_axes", false)?;
        if self.review_instructions.is_empty() {
            bail!("assignment.review_instructions must not be empty");
        }
        let decisions = self
            .allowed_decisions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if decisions.len() != self.allowed_decisions.len() {
            bail!("assignment.allowed_decisions contains duplicates");
        }
        match self.mode {
            ProseMode::Authoring => {
                if self.author_instructions.is_empty() {
                    bail!("authoring assignment requires author_instructions");
                }
                if self.allowed_decisions.is_empty() {
                    bail!("authoring assignment requires allowed_decisions");
                }
                if self.candidate.is_some() || self.claim_map.is_some() {
                    bail!("authoring assignment cannot embed candidate or claim_map");
                }
            }
            ProseMode::Review => {
                if self.candidate.is_none() {
                    bail!("review assignment requires a candidate");
                }
                if !self.allowed_decisions.is_empty() {
                    bail!("review assignment cannot declare author decisions");
                }
            }
        }
        if let Some(candidate) = &self.candidate {
            candidate.validate("assignment.candidate")?;
        }
        if let Some(claim_map) = &self.claim_map {
            claim_map.validate("assignment.claim_map")?;
        }
        if let Some(oracle) = &self.oracle {
            oracle.validate(&self.review_axes, &self.finding_tag_vocabulary)?;
        }
        Ok(())
    }
}

impl ProseSuite {
    pub fn validate(&self) -> Result<()> {
        if self.schema != PROSE_SUITE_SCHEMA {
            bail!("unsupported prose suite schema '{}'", self.schema);
        }
        validate_key(&self.id, "prose_suite.id")?;
        non_blank(&self.title, "prose_suite.title")?;
        non_blank(&self.description, "prose_suite.description")?;
        validate_keys(
            &self.required_coverage,
            "prose_suite.required_coverage",
            false,
        )?;
        validate_keys(&self.assignments, "prose_suite.assignments", false)
    }
}

impl ArticleBrief {
    fn validate(&self) -> Result<()> {
        non_blank(&self.title, "assignment.article.title")?;
        non_blank(&self.namespace, "assignment.article.namespace")?;
        non_blank(&self.article_object, "assignment.article.article_object")?;
        non_blank(&self.reader_need, "assignment.article.reader_need")
    }
}

impl BoundInput {
    fn validate(&self, source: &str) -> Result<()> {
        validate_key(&self.id, &format!("{source}.id"))?;
        validate_locator(&self.locator, &format!("{source}.locator"))?;
        validate_sha256(&self.sha256, &format!("{source}.sha256"))
    }
}

impl SourceInput {
    fn validate(&self, source: &str) -> Result<()> {
        validate_key(&self.id, &format!("{source}.id"))?;
        non_blank(&self.title, &format!("{source}.title"))?;
        validate_locator(&self.locator, &format!("{source}.locator"))?;
        validate_sha256(&self.sha256, &format!("{source}.sha256"))?;
        non_blank(&self.citation, &format!("{source}.citation"))
    }
}

impl ReviewOracle {
    fn validate(&self, review_axes: &[String], finding_tag_vocabulary: &[String]) -> Result<()> {
        validate_keys(
            &self.required_finding_tags,
            "assignment.oracle.required_finding_tags",
            true,
        )?;
        validate_keys(
            &self.forbidden_finding_tags,
            "assignment.oracle.forbidden_finding_tags",
            true,
        )?;
        if self.allowed_dispositions.is_empty() {
            bail!("assignment.oracle.allowed_dispositions must not be empty");
        }
        let declared_axes = review_axes
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut expected_axes = BTreeSet::new();
        for (index, expectation) in self.axis_expectations.iter().enumerate() {
            validate_key(
                &expectation.axis,
                &format!("assignment.oracle.axis_expectations[{index}].axis"),
            )?;
            if !declared_axes.contains(expectation.axis.as_str()) {
                bail!(
                    "assignment.oracle axis expectation '{}' is not a declared review axis",
                    expectation.axis
                );
            }
            if !expected_axes.insert(expectation.axis.as_str()) {
                bail!(
                    "assignment.oracle repeats axis expectation '{}'",
                    expectation.axis
                );
            }
            if expectation.allowed_verdicts.is_empty() {
                bail!(
                    "assignment.oracle axis expectation '{}' has no allowed verdicts",
                    expectation.axis
                );
            }
            let allowed = expectation
                .allowed_verdicts
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if allowed.len() != expectation.allowed_verdicts.len() {
                bail!(
                    "assignment.oracle axis expectation '{}' repeats an allowed verdict",
                    expectation.axis
                );
            }
        }
        let required = self.required_finding_tags.iter().collect::<BTreeSet<_>>();
        if self
            .forbidden_finding_tags
            .iter()
            .any(|tag| required.contains(tag))
        {
            bail!("oracle finding tags cannot be both required and forbidden");
        }
        let declared_tags = finding_tag_vocabulary
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for tag in self
            .required_finding_tags
            .iter()
            .chain(&self.forbidden_finding_tags)
        {
            if !declared_tags.contains(tag.as_str()) {
                bail!(
                    "assignment.oracle finding tag '{}' is absent from assignment.finding_tag_vocabulary",
                    tag
                );
            }
        }
        let allowed = self
            .allowed_dispositions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if allowed.len() != self.allowed_dispositions.len() {
            bail!("assignment.oracle.allowed_dispositions contains duplicates");
        }
        Ok(())
    }
}

impl Participant {
    pub fn validate(&self, source: &str) -> Result<()> {
        validate_key(&self.id, &format!("{source}.id"))?;
        non_blank(&self.display_name, &format!("{source}.display_name"))?;
        match (self.kind, &self.execution) {
            (ParticipantKind::Agent, Some(execution)) => execution.validate(source),
            (ParticipantKind::Agent, None) => {
                bail!("{source}.execution is required for an agent participant")
            }
            (ParticipantKind::Human, Some(_)) => {
                bail!("{source}.execution is only valid for an agent participant")
            }
            (ParticipantKind::Human, None) => Ok(()),
        }
    }
}

impl AgentExecution {
    fn validate(&self, source: &str) -> Result<()> {
        for (field, value) in [
            ("provider", self.provider.as_str()),
            ("model", self.model.as_str()),
            ("harness", self.harness.as_str()),
            ("harness_version", self.harness_version.as_str()),
            ("invocation_id", self.invocation_id.as_str()),
        ] {
            non_blank(value, &format!("{source}.execution.{field}"))?;
        }
        if let Some(value) = &self.reasoning_effort {
            non_blank(value, &format!("{source}.execution.reasoning_effort"))?;
        }
        validate_texts(
            &self.access.tools,
            &format!("{source}.execution.access.tools"),
        )?;
        let distinct_tools = self.access.tools.iter().collect::<BTreeSet<_>>();
        if distinct_tools.len() != self.access.tools.len() {
            bail!("{source}.execution.access.tools contains duplicates");
        }
        if let Some(metrics) = &self.metrics {
            metrics.validate(source)?;
        }
        Ok(())
    }
}

impl AgentMetrics {
    fn validate(&self, source: &str) -> Result<()> {
        if self.duration_ms.is_none() && self.input_tokens.is_none() && self.output_tokens.is_none()
        {
            bail!("{source}.execution.metrics must record at least one available metric");
        }
        for (field, value) in [
            ("duration_ms", self.duration_ms),
            ("input_tokens", self.input_tokens),
            ("output_tokens", self.output_tokens),
        ] {
            if value == Some(0) {
                bail!("{source}.execution.metrics.{field} must be positive when recorded");
            }
        }
        Ok(())
    }
}

impl SubmissionFile {
    fn validate(&self, source: &str) -> Result<()> {
        validate_locator(&self.locator, &format!("{source}.locator"))?;
        validate_sha256(&self.sha256, &format!("{source}.sha256"))
    }
}

impl AuthorSubmission {
    pub fn validate(&self) -> Result<()> {
        if self.schema != AUTHOR_SUBMISSION_SCHEMA {
            bail!("unsupported author submission schema '{}'", self.schema);
        }
        validate_key(&self.run_id, "author_submission.run_id")?;
        validate_key(&self.assignment_id, "author_submission.assignment_id")?;
        validate_sha256(&self.packet_sha256, "author_submission.packet_sha256")?;
        self.author.validate("author_submission.author")?;
        non_blank(&self.notes, "author_submission.notes")?;
        validate_texts(&self.holds, "author_submission.holds")?;
        match self.decision {
            ArticleDecision::Article => {
                self.candidate
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("article decision requires candidate"))?
                    .validate("author_submission.candidate")?;
                self.claim_map
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("article decision requires claim_map"))?
                    .validate("author_submission.claim_map")?;
            }
            _ => {
                if self.candidate.is_some() || self.claim_map.is_some() {
                    bail!("non-article decision cannot include candidate or claim_map");
                }
            }
        }
        Ok(())
    }
}

impl ClaimMap {
    pub fn validate(&self, assignment: &ProseAssignment, candidate_sha256: &str) -> Result<()> {
        if self.schema != CLAIM_MAP_SCHEMA {
            bail!("unsupported claim map schema '{}'", self.schema);
        }
        if self.article_title != assignment.article.title {
            bail!("claim map article_title does not match assignment");
        }
        if self.candidate_sha256 != candidate_sha256 {
            bail!("claim map candidate_sha256 does not match candidate bytes");
        }
        if self.claims.is_empty() || self.claims.len() > 512 {
            bail!("claim map must contain 1-512 claims");
        }
        validate_texts(&self.holds, "claim_map.holds")?;
        let source_ids = assignment
            .sources
            .iter()
            .map(|source| source.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut claim_ids = BTreeSet::new();
        for (index, claim) in self.claims.iter().enumerate() {
            let source = format!("claim_map.claims[{index}]");
            validate_key(&claim.id, &format!("{source}.id"))?;
            if !claim_ids.insert(claim.id.as_str()) {
                bail!("claim map repeats claim id '{}'", claim.id);
            }
            non_blank(&claim.claim, &format!("{source}.claim"))?;
            if claim.evidence.is_empty() {
                bail!("{source}.evidence must not be empty");
            }
            let mut evidence_sources = BTreeSet::new();
            for evidence in &claim.evidence {
                validate_key(&evidence.source_id, &format!("{source}.source_id"))?;
                if !source_ids.contains(evidence.source_id.as_str()) {
                    bail!("{source} names unknown source '{}'", evidence.source_id);
                }
                if !evidence_sources.insert(evidence.source_id.as_str()) {
                    bail!("{source} repeats source '{}'", evidence.source_id);
                }
                non_blank(&evidence.locator, &format!("{source}.locator"))?;
            }
            if let Some(qualification) = &claim.qualification {
                non_blank(qualification, &format!("{source}.qualification"))?;
            }
        }
        Ok(())
    }
}

impl ReviewSubmission {
    pub fn validate(&self, assignment: &ProseAssignment) -> Result<()> {
        if self.schema != REVIEW_SUBMISSION_SCHEMA {
            bail!("unsupported review submission schema '{}'", self.schema);
        }
        validate_key(&self.run_id, "review_submission.run_id")?;
        validate_key(&self.assignment_id, "review_submission.assignment_id")?;
        validate_sha256(
            &self.review_packet_sha256,
            "review_submission.review_packet_sha256",
        )?;
        if let Some(candidate_sha256) = &self.candidate_sha256 {
            validate_sha256(candidate_sha256, "review_submission.candidate_sha256")?;
        }
        if self.finding_tag_vocabulary != assignment.finding_tag_vocabulary {
            bail!("review submission finding_tag_vocabulary does not match assignment");
        }
        self.reviewer.validate("review_submission.reviewer")?;
        non_blank(&self.reader_verdict, "review_submission.reader_verdict")?;
        non_blank(&self.residual_risk, "review_submission.residual_risk")?;
        let declared_finding_tags = assignment
            .finding_tag_vocabulary
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut finding_ids = BTreeSet::new();
        for (index, finding) in self.findings.iter().enumerate() {
            let source = format!("review_submission.findings[{index}]");
            validate_key(&finding.id, &format!("{source}.id"))?;
            if !finding_ids.insert(finding.id.as_str()) {
                bail!("review submission repeats finding id '{}'", finding.id);
            }
            for (field, value) in [
                ("location", &finding.location),
                ("problem", &finding.problem),
                ("evidence", &finding.evidence),
                ("impact", &finding.impact),
                ("repair_direction", &finding.repair_direction),
            ] {
                non_blank(value, &format!("{source}.{field}"))?;
            }
            validate_keys(&finding.tags, &format!("{source}.tags"), true)?;
            for tag in &finding.tags {
                if !declared_finding_tags.contains(tag.as_str()) {
                    bail!(
                        "{source}.tags contains '{}' outside assignment.finding_tag_vocabulary",
                        tag
                    );
                }
            }
        }
        match self.disposition {
            ReviewDisposition::Revise => {
                if self.findings.iter().any(|finding| {
                    matches!(finding.severity, FindingSeverity::P0 | FindingSeverity::P1)
                }) || !self
                    .findings
                    .iter()
                    .any(|finding| finding.severity == FindingSeverity::P2)
                {
                    bail!("revise disposition requires P2 and cannot retain P0 or P1 findings");
                }
            }
            ReviewDisposition::Block => {
                if !self.findings.iter().any(|finding| {
                    matches!(finding.severity, FindingSeverity::P0 | FindingSeverity::P1)
                }) {
                    bail!("block disposition requires at least one P0 or P1 finding");
                }
            }
            ReviewDisposition::Accept => {
                if self.findings.iter().any(|finding| {
                    matches!(
                        finding.severity,
                        FindingSeverity::P0 | FindingSeverity::P1 | FindingSeverity::P2
                    )
                }) {
                    bail!("accept disposition cannot retain unresolved P0-P2 findings");
                }
            }
        }
        let mut observation_tags = BTreeSet::new();
        for (index, observation) in self.observations.iter().enumerate() {
            validate_key(
                &observation.tag,
                &format!("review_submission.observations[{index}].tag"),
            )?;
            non_blank(
                &observation.evidence,
                &format!("review_submission.observations[{index}].evidence"),
            )?;
            if !observation_tags.insert(observation.tag.as_str()) {
                bail!(
                    "review submission repeats observation tag '{}'",
                    observation.tag
                );
            }
        }
        let expected_axes = assignment
            .review_axes
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut observed_axes = BTreeSet::new();
        for (index, axis) in self.axes.iter().enumerate() {
            validate_key(&axis.axis, &format!("review_submission.axes[{index}].axis"))?;
            non_blank(
                &axis.rationale,
                &format!("review_submission.axes[{index}].rationale"),
            )?;
            if !observed_axes.insert(axis.axis.as_str()) {
                bail!("review submission repeats axis '{}'", axis.axis);
            }
        }
        if observed_axes != expected_axes {
            bail!(
                "review axes do not match assignment: expected {:?}, observed {:?}",
                expected_axes,
                observed_axes
            );
        }
        Ok(())
    }
}

fn validate_bound_inputs(values: &[BoundInput], source: &str) -> Result<()> {
    if values.len() > 128 {
        bail!("{source} may contain at most 128 inputs");
    }
    let mut ids = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        value.validate(&format!("{source}[{index}]"))?;
        if !ids.insert(value.id.as_str()) {
            bail!("{source} repeats id '{}'", value.id);
        }
    }
    Ok(())
}

fn validate_texts(values: &[String], source: &str) -> Result<()> {
    if values.len() > 512 {
        bail!("{source} may contain at most 512 values");
    }
    for (index, value) in values.iter().enumerate() {
        non_blank(value, &format!("{source}[{index}]"))?;
    }
    Ok(())
}

fn validate_keys(values: &[String], source: &str, allow_empty: bool) -> Result<()> {
    if (!allow_empty && values.is_empty()) || values.len() > 256 {
        bail!(
            "{source} must contain {}-256 values",
            if allow_empty { 0 } else { 1 }
        );
    }
    let mut unique = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        validate_key(value, &format!("{source}[{index}]"))?;
        if !unique.insert(value.as_str()) {
            bail!("{source} repeats '{value}'");
        }
    }
    Ok(())
}

fn validate_key(value: &str, source: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
                || character == '.'
        })
    {
        bail!("{source} must use lowercase ASCII letters, digits, '.', '-', or '_'");
    }
    Ok(())
}

fn validate_locator(value: &str, source: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{source} must be nonblank");
    }
    join_relative(Path::new("."), value)
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("{source}: {error}"))
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

fn non_blank(value: &str, source: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        bail!("{source} must be nonblank and contain no NUL");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assignment() -> ProseAssignment {
        serde_json::from_value(json!({
            "schema": PROSE_ASSIGNMENT_SCHEMA,
            "id": "aster-authoring",
            "title": "Aster authoring",
            "description": "Closed-world prose authoring.",
            "mode": "authoring",
            "complexity": "focused",
            "coverage": ["source-fidelity"],
            "article": {
                "title": "Aster Index",
                "namespace": "Main",
                "article_object": "A concise encyclopedia article about the archive.",
                "reader_need": "Explain the archive.",
                "sensitive": false
            },
            "author_instructions": [{"id":"writing-skill","root":"repository","locator":"skills/write.md","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}],
            "review_instructions": [{"id":"review-skill","root":"repository","locator":"skills/review.md","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}],
            "sources": [{"id":"s1","title":"About","locator":"sources/about.txt","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","role":"primary","citation":"Fixture about page"}],
            "allowed_decisions": ["article", "hold"],
            "finding_tag_vocabulary": [],
            "review_axes": ["reader-value", "source-fidelity"]
        }))
        .expect("assignment JSON")
    }

    #[test]
    fn strict_authoring_assignment_validates() {
        assignment().validate().expect("valid assignment");
    }

    #[test]
    fn agent_participant_requires_exact_execution_identity() {
        let participant: Participant = serde_json::from_value(json!({
            "id": "author-agent",
            "display_name": "Author agent",
            "kind": "agent"
        }))
        .expect("participant JSON");
        let error = participant
            .validate("participant")
            .expect_err("unidentified agent execution must fail");
        assert!(error.to_string().contains("execution is required"));

        let participant: Participant = serde_json::from_value(json!({
            "id": "author-agent",
            "display_name": "Author agent",
            "kind": "agent",
            "execution": {
                "provider": "fixture-provider",
                "model": "fixture-model",
                "harness": "fixture-harness",
                "harness_version": "1.2.3",
                "invocation_id": "invocation-1",
                "access": {
                    "network": false,
                    "ambient_repository": false,
                    "tools": ["read", "write"]
                },
                "metrics": {"duration_ms": 10, "input_tokens": 20, "output_tokens": 30}
            }
        }))
        .expect("participant JSON");
        participant
            .validate("participant")
            .expect("identified agent execution");
    }

    #[test]
    fn human_participant_cannot_claim_agent_execution() {
        let participant: Participant = serde_json::from_value(json!({
            "id": "human-editor",
            "display_name": "Human editor",
            "kind": "human",
            "execution": {
                "provider": "fixture-provider",
                "model": "fixture-model",
                "harness": "fixture-harness",
                "harness_version": "1.2.3",
                "invocation_id": "invocation-1",
                "access": {"network": false, "ambient_repository": false, "tools": []}
            }
        }))
        .expect("participant JSON");
        let error = participant
            .validate("participant")
            .expect_err("human execution metadata must fail");
        assert!(error.to_string().contains("only valid for an agent"));
    }

    #[test]
    fn unknown_assignment_fields_are_rejected() {
        let mut value = serde_json::to_value(assignment()).expect("serialize");
        value["prompt"] = json!("hidden prose doctrine");
        assert!(serde_json::from_value::<ProseAssignment>(value).is_err());
    }

    #[test]
    fn review_requires_exact_axes() {
        let assignment = assignment();
        let submission: ReviewSubmission = serde_json::from_value(json!({
            "schema": REVIEW_SUBMISSION_SCHEMA,
            "run_id": "run-1",
            "assignment_id": "aster-authoring",
            "review_packet_sha256": "d".repeat(64),
            "candidate_sha256": "e".repeat(64),
            "finding_tag_vocabulary": [],
            "reviewer": {"id":"reviewer-1","display_name":"Reviewer","kind":"human"},
            "scope": "complete",
            "reader_verdict": "Readable.",
            "source_verdict": "complete",
            "disposition": "accept",
            "observations": [],
            "axes": [{"axis":"reader-value","verdict":"pass","rationale":"Concrete."}],
            "residual_risk": "None identified."
        }))
        .expect("review JSON");
        assert!(submission.validate(&assignment).is_err());
    }

    #[test]
    fn axis_verdict_accepts_explicit_failure() {
        let verdict: AxisVerdict = serde_json::from_str("\"fail\"").expect("failure verdict");
        assert_eq!(verdict, AxisVerdict::Fail);
    }

    #[test]
    fn oracle_tags_must_come_from_the_public_vocabulary() {
        let mut assignment = assignment();
        assignment.oracle = Some(ReviewOracle {
            required_finding_tags: vec!["hidden-expected-label".to_owned()],
            forbidden_finding_tags: Vec::new(),
            axis_expectations: Vec::new(),
            allowed_dispositions: vec![ReviewDisposition::Accept],
        });
        let error = assignment
            .validate()
            .expect_err("hidden oracle tag must fail");
        assert!(error.to_string().contains("finding_tag_vocabulary"));
    }

    #[test]
    fn submitted_finding_tags_must_come_from_the_public_vocabulary() {
        let assignment = assignment();
        let submission: ReviewSubmission = serde_json::from_value(json!({
            "schema": REVIEW_SUBMISSION_SCHEMA,
            "run_id": "run-1",
            "assignment_id": "aster-authoring",
            "review_packet_sha256": "d".repeat(64),
            "candidate_sha256": "e".repeat(64),
            "finding_tag_vocabulary": [],
            "reviewer": {"id":"reviewer-1","display_name":"Reviewer","kind":"human"},
            "scope": "complete",
            "reader_verdict": "Needs repair.",
            "source_verdict": "complete",
            "disposition": "revise",
            "findings": [{
                "id": "finding-1",
                "severity": "p2",
                "location": "Lead",
                "problem": "The lead overstates the record.",
                "evidence": "The supplied source does not support the wording.",
                "impact": "Readers receive a distorted account.",
                "repair_direction": "Narrow the sentence to the source.",
                "basis": "verified",
                "tags": ["hidden-expected-label"]
            }],
            "observations": [],
            "axes": [
                {"axis":"reader-value","verdict":"concern","rationale":"The lead misleads."},
                {"axis":"source-fidelity","verdict":"fail","rationale":"The source does not entail it."}
            ],
            "residual_risk": "None identified."
        }))
        .expect("review JSON");
        let error = submission
            .validate(&assignment)
            .expect_err("undeclared finding tag must fail");
        assert!(error.to_string().contains("finding_tag_vocabulary"));
    }

    #[test]
    fn disposition_must_close_findings_at_the_declared_severity() {
        let assignment = assignment();
        let submission = |disposition: &str, severities: &[&str]| {
            let findings = severities
                .iter()
                .enumerate()
                .map(|(index, severity)| {
                    json!({
                        "id": format!("finding-{}", index + 1),
                        "severity": severity,
                        "location": "Lead",
                        "problem": "The wording does not match the supplied record.",
                        "evidence": "The source packet supports a narrower statement.",
                        "impact": "Readers receive a distorted account.",
                        "repair_direction": "Narrow the statement to the source.",
                        "basis": "verified",
                        "tags": []
                    })
                })
                .collect::<Vec<_>>();
            serde_json::from_value::<ReviewSubmission>(json!({
                "schema": REVIEW_SUBMISSION_SCHEMA,
                "run_id": "run-1",
                "assignment_id": "aster-authoring",
                "review_packet_sha256": "d".repeat(64),
                "candidate_sha256": "e".repeat(64),
                "finding_tag_vocabulary": [],
                "reviewer": {"id":"reviewer-1","display_name":"Reviewer","kind":"human"},
                "scope": "complete",
                "reader_verdict": "Recorded against the supplied packet.",
                "source_verdict": "complete",
                "disposition": disposition,
                "findings": findings,
                "observations": [],
                "axes": [
                    {"axis":"reader-value","verdict":"concern","rationale":"Reader impact assessed."},
                    {"axis":"source-fidelity","verdict":"concern","rationale":"Source fidelity assessed."}
                ],
                "residual_risk": "None identified."
            }))
            .expect("review JSON")
        };

        submission("accept", &[])
            .validate(&assignment)
            .expect("accept without material findings");
        submission("revise", &["p2"])
            .validate(&assignment)
            .expect("revise closes P2");
        submission("block", &["p1"])
            .validate(&assignment)
            .expect("block closes P1");
        assert!(submission("accept", &["p2"]).validate(&assignment).is_err());
        assert!(submission("revise", &["p1"]).validate(&assignment).is_err());
        assert!(submission("block", &["p2"]).validate(&assignment).is_err());
    }
}
