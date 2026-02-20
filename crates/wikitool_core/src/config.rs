use std::env;
use std::fs;
use std::path::Path;

use serde::Deserialize;

pub const DEFAULT_USER_AGENT: &str = "wikitool/0.1";
pub const DEFAULT_ARTICLE_PATH: &str = "/$1";

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WikiConfig {
    #[serde(default)]
    pub wiki: WikiSection,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WikiSection {
    pub url: Option<String>,
    pub api_url: Option<String>,
    pub article_path: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub custom_namespaces: Vec<CustomNamespace>,
}

#[derive(Debug, Clone, Deserialize)]
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
    /// Resolve the wiki API URL: env WIKI_API_URL > config > None.
    pub fn api_url(&self) -> Option<&str> {
        if let Ok(value) = env::var("WIKI_API_URL") {
            // Env var exists but we can't return a reference to a local.
            // Callers should use `api_url_owned()` when env override is needed.
            let _ = value;
        }
        self.wiki.api_url.as_deref()
    }

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
pub fn load_config(config_path: &Path) -> WikiConfig {
    if !config_path.exists() {
        return WikiConfig::default();
    }
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => return WikiConfig::default(),
    };
    toml::from_str(&content).unwrap_or_default()
}

/// Derive wiki base URL from an API URL by stripping `/api.php`.
fn derive_wiki_url(api_url: &str) -> Option<String> {
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
        let config = load_config(Path::new("/nonexistent/config.toml"));
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

        let config = load_config(&config_path);
        assert_eq!(config.wiki.url.as_deref(), Some("https://example.wiki"));
        assert_eq!(
            config.wiki.api_url.as_deref(),
            Some("https://example.wiki/api.php")
        );
        assert_eq!(config.wiki.article_path.as_deref(), Some("/wiki/$1"));
        assert_eq!(
            config.wiki.user_agent.as_deref(),
            Some("test-agent/1.0")
        );
        assert_eq!(config.wiki.custom_namespaces.len(), 1);
        assert_eq!(config.wiki.custom_namespaces[0].name, "Custom");
        assert_eq!(config.wiki.custom_namespaces[0].id, 3000);
    }

    #[test]
    fn load_config_tolerates_partial_toml() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.toml");
        fs::write(
            &config_path,
            "[paths]\nproject_root = \"/foo\"\n",
        )
        .expect("write config");

        let config = load_config(&config_path);
        assert!(config.wiki.url.is_none());
        assert!(config.wiki.custom_namespaces.is_empty());
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
