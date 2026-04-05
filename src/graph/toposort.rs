//! Topological sort for the relationship graph.

use std::collections::{HashMap, HashSet, VecDeque};

use super::relationship::RelationshipGraph;

impl RelationshipGraph {
    /// Topological sort via Kahn's algorithm.
    ///
    /// Returns aliases in topological order (root first), or `Err` with a
    /// cycle path description if the graph contains cycles.
    ///
    /// Deterministic: the root is always first, and other zero-in-degree nodes
    /// are added in sorted order.
    pub fn toposort(&self) -> Result<Vec<String>, String> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for node in &self.all_nodes {
            in_degree.entry(node.as_str()).or_insert(0);
        }
        for targets in self.edges.values() {
            for t in targets {
                *in_degree.entry(t.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<String> = VecDeque::new();
        // Seed with root first for determinism (if it has in-degree 0).
        if in_degree.get(self.root.as_str()) == Some(&0) {
            queue.push_back(self.root.clone());
        }
        // Add other zero-in-degree nodes in sorted order for determinism.
        let mut others: Vec<&str> = in_degree
            .iter()
            .filter(|(k, v)| **v == 0 && **k != self.root.as_str())
            .map(|(k, _)| *k)
            .collect();
        others.sort_unstable();
        for o in others {
            queue.push_back(o.to_string());
        }

        let mut order = Vec::new();
        while let Some(node) = queue.pop_front() {
            order.push(node.clone());
            if let Some(neighbors) = self.edges.get(&node) {
                // Sort neighbors for determinism before processing.
                let mut sorted_neighbors: Vec<&String> = neighbors.iter().collect();
                sorted_neighbors.sort();
                for next in sorted_neighbors {
                    if let Some(deg) = in_degree.get_mut(next.as_str()) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(next.clone());
                        }
                    }
                }
            }
        }

        if order.len() == self.all_nodes.len() {
            Ok(order)
        } else {
            // Remaining nodes are in a cycle -- find and report the cycle path.
            let visited: HashSet<&str> = order.iter().map(String::as_str).collect();
            let cycle_path = find_cycle_path(&self.edges, &visited, &self.all_nodes);
            Err(format!("cycle detected in relationships: {cycle_path}"))
        }
    }
}

/// Find a cycle path among unvisited nodes by following edges.
fn find_cycle_path(
    edges: &HashMap<String, Vec<String>>,
    visited: &HashSet<&str>,
    all_nodes: &HashSet<String>,
) -> String {
    // Find an unvisited node to start from.
    let start = match all_nodes.iter().find(|n| !visited.contains(n.as_str())) {
        Some(n) => n.clone(),
        None => return "unknown cycle".to_string(),
    };

    // Follow edges from start until we revisit a node.
    let mut path = vec![start.clone()];
    let mut current = start;
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        seen.insert(current.clone());
        if let Some(neighbors) = edges.get(&current) {
            // Pick the first unvisited-by-toposort neighbor.
            if let Some(next) = neighbors.iter().find(|n| !visited.contains(n.as_str())) {
                if seen.contains(next.as_str()) {
                    // Found the cycle -- trim path to start from the cycle entry point.
                    if let Some(pos) = path.iter().position(|p| p == next) {
                        path = path[pos..].to_vec();
                        path.push(next.clone());
                        return path.join(" -> ");
                    }
                }
                path.push(next.clone());
                current = next.clone();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    path.join(" -> ")
}
