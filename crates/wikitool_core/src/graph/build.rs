use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::Connection;

use super::filters::{edge_kind_allowed, filter_allows};
use super::model::{DirectedGraph, EdgeKind, GraphFilter, GraphKind, NodeId};

pub fn build_graph(
    connection: &Connection,
    kind: GraphKind,
    filter: &GraphFilter,
) -> Result<DirectedGraph> {
    let mut graph = DirectedGraph::new(kind);
    let mut node_ids = BTreeMap::<String, NodeId>::new();
    let mut node_namespaces = BTreeMap::<String, String>::new();

    let mut page_statement = connection
        .prepare("SELECT title, namespace FROM indexed_pages ORDER BY title ASC")
        .context("failed to prepare indexed_pages graph query")?;
    let page_rows = page_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to query indexed_pages for graph build")?;
    for row in page_rows {
        let (title, namespace) = row.context("failed to decode indexed_pages graph row")?;
        let id = graph.add_node(title.clone(), namespace.clone());
        node_ids.insert(title.clone(), id);
        node_namespaces.insert(title, namespace);
    }

    if edge_kind_allowed(kind, EdgeKind::Redirect) {
        load_redirect_edges(connection, &mut graph, &node_ids, &node_namespaces, filter)?;
    }
    if edge_kind_allowed(kind, EdgeKind::Link)
        || edge_kind_allowed(kind, EdgeKind::CategoryMembership)
    {
        load_link_edges(
            connection,
            &mut graph,
            &node_ids,
            &node_namespaces,
            kind,
            filter,
        )?;
    }
    if edge_kind_allowed(kind, EdgeKind::TemplateTransclusion)
        || edge_kind_allowed(kind, EdgeKind::ModuleInvocation)
    {
        load_transclusion_edges(connection, &mut graph, &node_ids, &node_namespaces, filter)?;
    }

    Ok(graph)
}

fn load_redirect_edges(
    connection: &Connection,
    graph: &mut DirectedGraph,
    node_ids: &BTreeMap<String, NodeId>,
    node_namespaces: &BTreeMap<String, String>,
    filter: &GraphFilter,
) -> Result<()> {
    let mut statement = connection
        .prepare(
            "SELECT title, namespace, redirect_target
             FROM indexed_pages
             WHERE is_redirect = 1 AND redirect_target IS NOT NULL",
        )
        .context("failed to prepare redirect graph query")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("failed to query redirect graph rows")?;
    for row in rows {
        let (from_title, from_namespace, target_title) =
            row.context("failed to decode redirect graph row")?;
        let Some(&from) = node_ids.get(&from_title) else {
            continue;
        };
        let Some(&to) = node_ids.get(&target_title) else {
            continue;
        };
        let to_namespace = node_namespaces
            .get(&target_title)
            .map(String::as_str)
            .unwrap_or_default();
        if filter_allows(
            filter,
            EdgeKind::Redirect,
            &from_namespace,
            to_namespace,
            from == to,
        ) {
            graph.add_edge(from, to, EdgeKind::Redirect);
        }
    }
    Ok(())
}

fn load_link_edges(
    connection: &Connection,
    graph: &mut DirectedGraph,
    node_ids: &BTreeMap<String, NodeId>,
    _node_namespaces: &BTreeMap<String, String>,
    kind: GraphKind,
    filter: &GraphFilter,
) -> Result<()> {
    let category_membership = if matches!(kind, GraphKind::Categories) {
        1i64
    } else {
        0i64
    };
    let mut statement = connection
        .prepare(
            "SELECT source_title, target_title, target_namespace
             FROM indexed_links
             WHERE is_category_membership = ?1",
        )
        .context("failed to prepare indexed_links graph query")?;
    let rows = statement
        .query_map([category_membership], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("failed to query indexed_links graph rows")?;
    let edge_kind = if category_membership == 1 {
        EdgeKind::CategoryMembership
    } else {
        EdgeKind::Link
    };
    for row in rows {
        let (from_title, target_title, target_namespace) =
            row.context("failed to decode indexed_links graph row")?;
        let Some(&from) = node_ids.get(&from_title) else {
            continue;
        };
        let Some(&to) = node_ids.get(&target_title) else {
            continue;
        };
        let from_namespace = &graph.nodes[from.0 as usize].namespace;
        if filter_allows(
            filter,
            edge_kind,
            from_namespace,
            &target_namespace,
            from == to,
        ) {
            graph.add_edge(from, to, edge_kind);
        }
    }
    Ok(())
}

fn load_transclusion_edges(
    connection: &Connection,
    graph: &mut DirectedGraph,
    node_ids: &BTreeMap<String, NodeId>,
    node_namespaces: &BTreeMap<String, String>,
    filter: &GraphFilter,
) -> Result<()> {
    let mut template_statement = connection
        .prepare("SELECT source_title, template_title FROM indexed_template_invocations")
        .context("failed to prepare template transclusion graph query")?;
    let template_rows = template_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to query template transclusion graph rows")?;
    for row in template_rows {
        let (from_title, target_title) = row.context("failed to decode template graph row")?;
        let Some(&from) = node_ids.get(&from_title) else {
            continue;
        };
        let Some(&to) = node_ids.get(&target_title) else {
            continue;
        };
        let from_namespace = &graph.nodes[from.0 as usize].namespace;
        let to_namespace = node_namespaces
            .get(&target_title)
            .map(String::as_str)
            .unwrap_or("Template");
        if filter_allows(
            filter,
            EdgeKind::TemplateTransclusion,
            from_namespace,
            to_namespace,
            from == to,
        ) {
            graph.add_edge(from, to, EdgeKind::TemplateTransclusion);
        }
    }

    let mut module_statement = connection
        .prepare("SELECT source_title, module_title FROM indexed_module_invocations")
        .context("failed to prepare module invocation graph query")?;
    let module_rows = module_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .context("failed to query module invocation graph rows")?;
    for row in module_rows {
        let (from_title, target_title) = row.context("failed to decode module graph row")?;
        let Some(&from) = node_ids.get(&from_title) else {
            continue;
        };
        let Some(&to) = node_ids.get(&target_title) else {
            continue;
        };
        let from_namespace = &graph.nodes[from.0 as usize].namespace;
        let to_namespace = node_namespaces
            .get(&target_title)
            .map(String::as_str)
            .unwrap_or("Module");
        if filter_allows(
            filter,
            EdgeKind::ModuleInvocation,
            from_namespace,
            to_namespace,
            from == to,
        ) {
            graph.add_edge(from, to, EdgeKind::ModuleInvocation);
        }
    }

    Ok(())
}
