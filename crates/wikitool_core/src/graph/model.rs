use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphKind {
    Redirects,
    Transclusion,
    ArticleLinksFiltered,
    Categories,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EdgeKind {
    Redirect,
    Link,
    CategoryMembership,
    TemplateTransclusion,
    ModuleInvocation,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: NodeId,
    pub title: String,
    pub namespace: String,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Default)]
pub struct GraphFilter {
    pub include_namespaces: Option<BTreeSet<String>>,
    pub include_edge_kinds: Option<BTreeSet<EdgeKind>>,
    pub exclude_self_loops: bool,
}

#[derive(Debug, Clone)]
pub struct DirectedGraph {
    pub kind: GraphKind,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub adjacency: Vec<Vec<NodeId>>,
}

impl DirectedGraph {
    pub fn new(kind: GraphKind) -> Self {
        Self {
            kind,
            nodes: Vec::new(),
            edges: Vec::new(),
            adjacency: Vec::new(),
        }
    }

    pub fn add_node(&mut self, title: impl Into<String>, namespace: impl Into<String>) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(GraphNode {
            id,
            title: title.into(),
            namespace: namespace.into(),
        });
        self.adjacency.push(Vec::new());
        id
    }

    pub fn add_edge(&mut self, from: NodeId, to: NodeId, kind: EdgeKind) {
        self.edges.push(GraphEdge { from, to, kind });
        self.adjacency[from.0 as usize].push(to);
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

#[derive(Debug, Clone)]
pub struct SccComponent {
    pub component_id: usize,
    pub members: Vec<NodeId>,
    pub is_cyclic: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SccIndex {
    pub components: Vec<SccComponent>,
    pub component_by_node: BTreeMap<NodeId, usize>,
}

impl SccIndex {
    pub fn component_of(&self, node: NodeId) -> Option<&SccComponent> {
        let component_id = *self.component_by_node.get(&node)?;
        self.components.get(component_id)
    }
}
