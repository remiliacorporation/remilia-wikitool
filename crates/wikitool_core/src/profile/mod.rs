pub mod wiki_capabilities;

pub use wiki_capabilities::{
    ExtensionInfo, NamespaceInfo, WikiCapabilityManifest, load_latest_wiki_capabilities,
    load_wiki_capabilities_with_config, sync_wiki_capabilities_with_config,
};
