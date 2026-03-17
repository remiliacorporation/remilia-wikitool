use super::siteinfo::SiteInfoNamespace;

pub const NS_MAIN: i32 = 0;
pub const NS_CATEGORY: i32 = 14;
pub const NS_TEMPLATE: i32 = 10;
pub const NS_MODULE: i32 = 828;
pub const NS_MEDIAWIKI: i32 = 8;

pub fn namespace_name_to_id(namespace: &str) -> Option<i32> {
    match namespace {
        "Main" => Some(NS_MAIN),
        "Category" => Some(NS_CATEGORY),
        "Template" => Some(NS_TEMPLATE),
        "Module" => Some(NS_MODULE),
        "MediaWiki" => Some(NS_MEDIAWIKI),
        _ => None,
    }
}

pub fn is_template_namespace_id(namespace: i32) -> bool {
    matches!(namespace, NS_TEMPLATE | NS_MODULE | NS_MEDIAWIKI)
}

pub fn should_include_discovered_namespace(namespace: &SiteInfoNamespace) -> bool {
    if namespace.id < 0 || namespace.id % 2 != 0 {
        return false;
    }
    if is_builtin_namespace_id(namespace.id) {
        return false;
    }
    let Some(name) = namespace_display_name(namespace) else {
        return false;
    };
    if is_builtin_namespace_name(&name) {
        return false;
    }
    namespace_is_content(namespace) || namespace.id >= 3000
}

pub fn namespace_display_name(namespace: &SiteInfoNamespace) -> Option<String> {
    [
        namespace.canonical.as_deref(),
        namespace.name.as_deref(),
        namespace.star_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.replace('_', " ").trim().to_string())
    .find(|value| !value.is_empty())
}

pub fn namespace_is_content(namespace: &SiteInfoNamespace) -> bool {
    match namespace.content.as_ref() {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(serde_json::Value::Number(value)) => value.as_i64().unwrap_or_default() != 0,
        Some(serde_json::Value::String(value)) => {
            matches!(value.trim(), "1" | "true" | "yes")
        }
        _ => false,
    }
}

fn is_builtin_namespace_id(id: i32) -> bool {
    matches!(
        id,
        -2 | -1 | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 828 | 829
    )
}

fn is_builtin_namespace_name(name: &str) -> bool {
    matches!(
        name,
        "Media"
            | "Special"
            | "Talk"
            | "User"
            | "User talk"
            | "Project"
            | "Project talk"
            | "File"
            | "File talk"
            | "MediaWiki"
            | "MediaWiki talk"
            | "Template"
            | "Template talk"
            | "Help"
            | "Help talk"
            | "Category"
            | "Category talk"
            | "Module"
            | "Module talk"
    )
}
