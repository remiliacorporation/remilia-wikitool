use std::collections::BTreeSet;

use super::model::{DirectedGraph, NodeId, SccIndex};

#[derive(Debug, Clone)]
pub struct CondensationNode {
    pub component_id: usize,
    pub size: usize,
    pub out_neighbors: BTreeSet<usize>,
    pub in_neighbors: BTreeSet<usize>,
}

pub fn build_condensation(graph: &DirectedGraph, scc: &SccIndex) -> Vec<CondensationNode> {
    let mut out_neighbors = vec![BTreeSet::<usize>::new(); scc.components.len()];
    let mut in_neighbors = vec![BTreeSet::<usize>::new(); scc.components.len()];

    for (src_idx, edges) in graph.adjacency.iter().enumerate() {
        let src_node = NodeId(src_idx as u32);
        let Some(&src_component) = scc.component_by_node.get(&src_node) else {
            continue;
        };

        for dst_node in edges {
            let Some(&dst_component) = scc.component_by_node.get(dst_node) else {
                continue;
            };
            if src_component == dst_component {
                continue;
            }
            out_neighbors[src_component].insert(dst_component);
            in_neighbors[dst_component].insert(src_component);
        }
    }

    scc.components
        .iter()
        .map(|component| CondensationNode {
            component_id: component.component_id,
            size: component.members.len(),
            out_neighbors: out_neighbors[component.component_id].clone(),
            in_neighbors: in_neighbors[component.component_id].clone(),
        })
        .collect()
}
