use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::Value;

pub const DEFAULT_USER_AGENT: &str = "wikitool/0.1";
pub const DEFAULT_ARTICLE_PATH: &str = "/$1";

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct WikiConfig {
    #[serde(default)]
    pub wiki: WikiSection,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct WikiSection {
    pub url: Option<String>,
    pub api_url: Option<String>,
    pub article_path: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub custom_namespaces: Vec<CustomNamespace>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CustomNamespace {
    pub name: String,
    pub id: i32,
    pub folder: Option<String>,
}

impl CustomNamespace {
    pub fn folder(&self) -> &str {
        self.folder.as_deref().unwrap_or(&self.name)
    }
}

impl WikiConfig {
    /// Resolve the wiki API URL with owned return: env > config > None.
    pub fn api_url_owned(&self) -> Option<String> {
        if let Ok(value) = env::var("WIKI_API_URL") {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        self.wiki.api_url.clone()
    }

    /// Resolve the wiki base URL: env WIKI_URL > config > derived from api_url.
    pub fn wiki_url(&self) -> Option<String> {
        if let Ok(value) = env::var("WIKI_URL") {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        if let Some(ref url) = self.wiki.url {
            return Some(url.clone());
        }
        // Try to derive from api_url by stripping /api.php
        self.api_url_owned().and_then(|api| derive_wiki_url(&api))
    }

    /// Resolve user agent: env WIKI_USER_AGENT > config > DEFAULT_USER_AGENT.
    pub fn user_agent(&self) -> String {
        if let Ok(value) = env::var("WIKI_USER_AGENT") {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        self.wiki
            .user_agent
            .clone()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string())
    }

    /// Resolve article path: env WIKI_ARTICLE_PATH > config > DEFAULT_ARTICLE_PATH.
    pub fn article_path(&self) -> &str {
        // Can't do env for borrowed return; check config then default.
        self.wiki
            .article_path
            .as_deref()
            .unwrap_or(DEFAULT_ARTICLE_PATH)
    }

    /// Resolve article path with env override (owned).
    pub fn article_path_owned(&self) -> String {
        if let Ok(value) = env::var("WIKI_ARTICLE_PATH") {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        self.article_path().to_string()
    }
}

/// Load and parse a WikiConfig from a TOML file. Returns default if file doesn't exist.
pub fn load_config(config_path: &Path) -> Result<WikiConfig> {
    if !config_path.exists() {
        return Ok(WikiConfig::default());
    }
    let content = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let parsed: WikiConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    Ok(parsed)
}

#[derive(Debug, Clone, Default)]
pub struct WikiConfigPatch {
    pub set_url: Option<String>,
    pub set_api_url: Option<String>,
    pub set_custom_namespaces: Option<Vec<CustomNamespace>>,
}

/// Update selected keys under `[wiki]` while preserving all other config sections.
/// Returns `true` when a write occurred.
pub fn patch_wiki_config(config_path: &Path, patch: &WikiConfigPatch) -> Result<bool> {
    if patch.set_url.is_none()
        && patch.set_api_url.is_none()
        && patch.set_custom_namespaces.is_none()
    {
        return Ok(false);
    }

    let mut root = if config_path.exists() {
        let content = fs::read_to_string(config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        toml::from_str::<Value>(&content)
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    } else {
        Value::Table(Default::default())
    };
    let original = root.clone();

    let root_table = root.as_table_mut().ok_or_else(|| {
        anyhow::anyhow!(
            "top-level TOML must be a table in {}",
            config_path.display()
        )
    })?;
    let wiki_entry = root_table
        .entry("wiki".to_string())
        .or_insert_with(|| Value::Table(Default::default()));
    let wiki_table = wiki_entry
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[wiki] must be a table in {}", config_path.display()))?;

    if let Some(url) = &patch.set_url {
        wiki_table.insert("url".to_string(), Value::String(url.clone()));
    }
    if let Some(api_url) = &patch.set_api_url {
        wiki_table.insert("api_url".to_string(), Value::String(api_url.clone()));
    }
    if let Some(custom_namespaces) = &patch.set_custom_namespaces {
        if custom_namespaces.is_empty() {
            wiki_table.remove("custom_namespaces");
        } else {
            let mut array = Vec::with_capacity(custom_namespaces.len());
            for ns in custom_namespaces {
                if ns.name.trim().is_empty() {
                    bail!("custom namespace name cannot be empty");
                }
                let mut table = toml::map::Map::new();
                table.insert("name".to_string(), Value::String(ns.name.clone()));
                table.insert("id".to_string(), Value::Integer(i64::from(ns.id)));
                if let Some(folder) = &ns.folder
                    && !folder.trim().is_empty()
                {
                    table.insert("folder".to_string(), Value::String(folder.clone()));
                }
                array.push(Value::Table(table));
            }
            wiki_table.insert("custom_namespaces".to_string(), Value::Array(array));
        }
    }

    if root == original {
        return Ok(false);
    }

    let parent = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent: {}", config_path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let rendered = toml::to_string_pretty(&root).context("failed to serialize config TOML")?;
    fs::write(config_path, rendered)
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    Ok(true)
}

/// Derive wiki base URL from an API URL by stripping `/api.php` or `/w/api.php`.
pub fn derive_wiki_url(api_url: &str) -> Option<String> {
    let trimmed = api_url.trim();
    let stripped = trimmed
        .strip_suffix("/api.php")
        .or_else(|| trimmed.strip_suffix("/w/api.php"))
        .unwrap_or(trimmed);
    let result = stripped.trim_end_matches('/').to_string();
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_config_has_no_urls() {
        let config = WikiConfig::default();
        assert!(config.wiki.url.is_none());
        assert!(config.wiki.api_url.is_none());
        assert!(config.wiki.custom_namespaces.is_empty());
    }

    #[test]
    fn load_config_returns_default_for_missing_file() {
        let config = load_config(Path::new("/nonexistent/config.toml")).expect("load config");
        assert!(config.wiki.url.is_none());
    }

    #[test]
    fn load_config_parses_wiki_section() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
[wiki]
url = "https://example.wiki"
api_url = "https://example.wiki/api.php"
article_path = "/wiki/$1"
user_agent = "test-agent/1.0"

[[wiki.custom_namespaces]]
name = "Custom"
id = 3000
folder = "Custom"
"#,
        )
        .expect("write config");

        let config = load_config(&config_path).expect("load config");
        assert_eq!(config.wiki.url.as_deref(), Some("https://example.wiki"));
        assert_eq!(
            config.wiki.api_url.as_deref(),
            Some("https://example.wiki/api.php")
        );
        assert_eq!(config.wiki.article_path.as_deref(), Some("/wiki/$1"));
        assert_eq!(config.wiki.user_agent.as_deref(), Some("test-agent/1.0"));
        assert_eq!(config.wiki.custom_namespaces.len(), 1);
        assert_eq!(config.wiki.custom_namespaces[0].name, "Custom");
        assert_eq!(config.wiki.custom_namespaces[0].id, 3000);
    }

    #[test]
    fn load_config_tolerates_partial_toml() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        fs::write(&config_path, "[paths]\nproject_root = \"/foo\"\n").expect("write config");

        let config = load_config(&config_path).expect("load config");
        assert!(config.wiki.url.is_none());
        assert!(config.wiki.custom_namespaces.is_empty());
    }

    #[test]
    fn load_config_returns_error_for_invalid_toml() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        fs::write(&config_path, "[wiki\nurl = \"oops\"").expect("write config");
        let error = load_config(&config_path).expect_err("must fail");
        assert!(error.to_string().contains("failed to parse"));
    }

    #[test]
    fn patch_wiki_config_updates_custom_namespaces() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        fs::write(&config_path, "[paths]\nproject_root = \"/repo\"\n").expect("write config");

        let wrote = patch_wiki_config(
            &config_path,
            &WikiConfigPatch {
                set_url: Some("https://wiki.example.org".to_string()),
                set_api_url: Some("https://wiki.example.org/api.php".to_string()),
                set_custom_namespaces: Some(vec![CustomNamespace {
                    name: "Lore".to_string(),
                    id: 3000,
                    folder: Some("Lore".to_string()),
                }]),
            },
        )
        .expect("patch");
        assert!(wrote);

        let config = load_config(&config_path).expect("load config");
        assert_eq!(config.wiki.url.as_deref(), Some("https://wiki.example.org"));
        assert_eq!(
            config.wiki.api_url.as_deref(),
            Some("https://wiki.example.org/api.php")
        );
        assert_eq!(config.wiki.custom_namespaces.len(), 1);
        assert_eq!(config.wiki.custom_namespaces[0].name, "Lore");
    }

    #[test]
    fn derive_wiki_url_strips_api_php() {
        assert_eq!(
            derive_wiki_url("https://wiki.example.org/api.php"),
            Some("https://wiki.example.org".to_string())
        );
        assert_eq!(
            derive_wiki_url("https://wiki.example.org/w/api.php"),
            Some("https://wiki.example.org/w".to_string())
        );
    }

    #[test]
    fn default_article_path() {
        let config = WikiConfig::default();
        assert_eq!(config.article_path(), "/$1");
    }

    #[test]
    fn default_user_agent() {
        let config = WikiConfig::default();
        assert_eq!(config.user_agent(), "wikitool/0.1");
    }

    #[test]
    fn custom_namespace_folder_defaults_to_name() {
        let ns = CustomNamespace {
            name: "Goldenlight".to_string(),
            id: 3000,
            folder: None,
        };
        assert_eq!(ns.folder(), "Goldenlight");
    }

    #[test]
    fn custom_namespace_folder_uses_explicit_value() {
        let ns = CustomNamespace {
            name: "Goldenlight".to_string(),
            id: 3000,
            folder: Some("GL".to_string()),
        };
        assert_eq!(ns.folder(), "GL");
    }
}
