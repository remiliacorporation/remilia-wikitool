pub use super::model::{
    AuthoringDocsContext, AuthoringInventory, AuthoringKnowledgePack,
    AuthoringKnowledgePackOptions, AuthoringKnowledgePackResult, AuthoringPageCandidate,
    AuthoringSuggestion, AuthoringTopicAssessment, ModuleFunctionUsage, ModuleInvocationExample,
    ModuleUsageSummary, StubTemplateHint,
};

pub use crate::authoring::{build_authoring_knowledge_pack, push_authoring_query_term};
