use anyhow::Result;

use crate::runtime::ResolvedPaths;

pub use crate::index::{
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
    crate::index::build_authoring_knowledge_pack(paths, topic, stub_content, options)
}
