use super::model::{EdgeKind, GraphFilter, GraphKind};

pub(crate) fn edge_kind_allowed(kind: GraphKind, edge_kind: EdgeKind) -> bool {
    match kind {
        GraphKind::Redirects => edge_kind == EdgeKind::Redirect,
        GraphKind::Transclusion => {
            matches!(
                edge_kind,
                EdgeKind::TemplateTransclusion | EdgeKind::ModuleInvocation
            )
        }
        GraphKind::ArticleLinksFiltered => edge_kind == EdgeKind::Link,
        GraphKind::Categories => edge_kind == EdgeKind::CategoryMembership,
    }
}

pub(crate) fn filter_allows(
    filter: &GraphFilter,
    edge_kind: EdgeKind,
    from_namespace: &str,
    to_namespace: &str,
    same_node: bool,
) -> bool {
    if filter.exclude_self_loops && same_node {
        return false;
    }
    if let Some(allowed_edge_kinds) = &filter.include_edge_kinds
        && !allowed_edge_kinds.contains(&edge_kind)
    {
        return false;
    }
    if let Some(namespaces) = &filter.include_namespaces
        && (!namespaces.contains(from_namespace) || !namespaces.contains(to_namespace))
    {
        return false;
    }
    true
}
