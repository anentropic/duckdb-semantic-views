mod facts;
mod fan_trap;
mod join_resolver;
mod materialization;
mod resolution;
mod role_playing;
mod select_spec;
mod semi_additive;
mod sql_gen;
mod types;
// Live under the `extension` feature (wildcard expansion in the query/explain
// FFI paths); dead only in the default build, so scope the allow accordingly
// (ST-8) rather than blanket-suppressing dead_code in the extension build too.
#[cfg_attr(not(feature = "extension"), allow(dead_code))]
pub(crate) mod wildcard;
mod window;

#[cfg(test)]
mod test_helpers;

// Behaviour-named expansion test modules, extracted from sql_gen.rs's monolithic
// phase-named `mod tests` (§6.2 move 6, code-review 2026-07-11).
#[cfg(test)]
mod tests_cast;
#[cfg(test)]
mod tests_count_star_rewrite;
#[cfg(test)]
mod tests_derived_metric;
#[cfg(test)]
mod tests_expand;
#[cfg(test)]
mod tests_expand_basic;
#[cfg(test)]
mod tests_fact_inlining;
#[cfg(test)]
mod tests_fact_query;
#[cfg(test)]
mod tests_facts_awareness;
#[cfg(test)]
mod tests_facts_path_role_playing;
#[cfg(test)]
mod tests_fan_trap;
#[cfg(test)]
mod tests_join_emission_regression;
#[cfg(test)]
mod tests_pkfk_expand;
#[cfg(test)]
mod tests_private_access;
#[cfg(test)]
mod tests_qualified_name_resolution;
#[cfg(test)]
mod tests_qualified_refs;
#[cfg(test)]
mod tests_role_playing;

// Public API (the pre-split expand.rs surface, plus the boxed fan-trap detail
// structs re-exported for R-9).
pub use resolution::{quote_ident, quote_ident_if_needed, quote_table_ref};
pub use sql_gen::expand;
pub use types::{
    DimensionName, ExpandError, FanTrapError, MetricFanTrapError, MetricName, QueryRequest,
};

// Crate-internal API (used by ddl/show_dims_for_metric.rs under extension feature)
#[cfg(feature = "extension")]
pub(crate) use facts::collect_derived_metric_source_tables;
#[cfg(feature = "extension")]
pub(crate) use fan_trap::ancestors_to_root;
#[cfg(feature = "extension")]
pub(crate) use materialization::find_routing_materialization_name;
