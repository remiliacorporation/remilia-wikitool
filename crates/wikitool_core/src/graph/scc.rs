use std::collections::BTreeMap;

use super::model::{DirectedGraph, NodeId, SccComponent, SccIndex};

pub fn compute_scc(graph: &DirectedGraph) -> SccIndex {
    let n = graph.node_count();
    let mut next_index = 0usize;
    let mut index_of: Vec<Option<usize>> = vec![None; n];
    let mut lowlink: Vec<usize> = vec![0; n];
    let mut stack: Vec<NodeId> = Vec::new();
    let mut on_stack = vec![false; n];

    let mut components = Vec::<SccComponent>::new();
    let mut component_by_node = BTreeMap::<NodeId, usize>::new();

    for start in 0..n {
        let start = NodeId(start as u32);
        if index_of[start.0 as usize].is_some() {
            continue;
        }
        strongconnect(
            start,
            graph,
            &mut next_index,
            &mut index_of,
            &mut lowlink,
            &mut stack,
            &mut on_stack,
            &mut components,
            &mut component_by_node,
        );
    }

    SccIndex {
        components,
        component_by_node,
    }
}

#[allow(clippy::too_many_arguments)]
fn strongconnect(
    node: NodeId,
    graph: &DirectedGraph,
    next_index: &mut usize,
    index_of: &mut [Option<usize>],
    lowlink: &mut [usize],
    stack: &mut Vec<NodeId>,
    on_stack: &mut [bool],
    components: &mut Vec<SccComponent>,
    component_by_node: &mut BTreeMap<NodeId, usize>,
) {
    let node_idx = node.0 as usize;
    index_of[node_idx] = Some(*next_index);
    lowlink[node_idx] = *next_index;
    *next_index += 1;
    stack.push(node);
    on_stack[node_idx] = true;

    for succ in &graph.adjacency[node_idx] {
        let succ_idx = succ.0 as usize;
        if index_of[succ_idx].is_none() {
            strongconnect(
                *succ,
                graph,
                next_index,
                index_of,
                lowlink,
                stack,
                on_stack,
                components,
                component_by_node,
            );
            lowlink[node_idx] = lowlink[node_idx].min(lowlink[succ_idx]);
        } else if on_stack[succ_idx] {
            lowlink[node_idx] = lowlink[node_idx].min(index_of[succ_idx].unwrap());
        }
    }

    if lowlink[node_idx] == index_of[node_idx].unwrap() {
        let component_id = components.len();
        let mut members = Vec::<NodeId>::new();
        let mut saw_self_loop = false;

        loop {
            let member = stack.pop().expect("tarjan stack underflow");
            let member_idx = member.0 as usize;
            on_stack[member_idx] = false;
            component_by_node.insert(member, component_id);
            if graph.adjacency[member_idx].contains(&member) {
                saw_self_loop = true;
            }
            members.push(member);
            if member == node {
                break;
            }
        }

        components.push(SccComponent {
            component_id,
            is_cyclic: saw_self_loop || members.len() > 1,
            members,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::model::{DirectedGraph, EdgeKind, GraphKind};

    #[test]
    fn compute_scc_marks_cycles() {
        let mut graph = DirectedGraph::new(GraphKind::Redirects);
        let a = graph.add_node("A", "Main");
        let b = graph.add_node("B", "Main");
        let c = graph.add_node("C", "Main");
        graph.add_edge(a, b, EdgeKind::Redirect);
        graph.add_edge(b, a, EdgeKind::Redirect);
        graph.add_edge(b, c, EdgeKind::Redirect);

        let index = compute_scc(&graph);

        assert_eq!(index.components.len(), 2);
        assert!(index.component_of(a).expect("component").is_cyclic);
        assert!(index.component_of(b).expect("component").is_cyclic);
        assert!(!index.component_of(c).expect("component").is_cyclic);
    }
}
