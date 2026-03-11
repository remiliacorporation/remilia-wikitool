use anyhow::Result;

use crate::runtime::ResolvedPaths;

pub use super::model::{
    AuthoringDocsContext, AuthoringInventory, AuthoringKnowledgePack,
    AuthoringKnowledgePackOptions, AuthoringKnowledgePackResult, AuthoringPageCandidate,
    AuthoringSuggestion, ModuleFunctionUsage, ModuleInvocationExample, ModuleUsageSummary,
    StubTemplateHint,
};

pub fn build_authoring_knowledge_pack(
    paths: &ResolvedPaths,
    topic: Option<&str>,
    stub_content: Option<&str>,
    options: &AuthoringKnowledgePackOptions,
) -> Result<AuthoringKnowledgePack> {
    crate::index::authoring::build_authoring_knowledge_pack(paths, topic, stub_content, options)
}
