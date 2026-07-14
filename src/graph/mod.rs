//! Relationship graph validation and topological sort for semantic view definitions.

mod cardinality;
mod derived_metrics;
mod facts;
mod join_tree;
mod names;
mod relationship;
mod toposort;
mod using;

#[cfg(test)]
mod test_helpers;

// Public API (matches prior graph.rs surface exactly)
pub(crate) use cardinality::infer_cardinality;
pub use derived_metrics::{contains_aggregate_function, validate_derived_metrics};
pub use facts::{find_fact_references, validate_facts};
pub(crate) use join_tree::JoinTree;
pub use names::validate_name_uniqueness;
pub use relationship::{validate_graph, RelationshipGraph};
pub use using::validate_using_relationships;
