mod facts;
mod fan_trap;
mod join_resolver;
mod materialization;
mod resolution;
mod role_playing;
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

// Public API (matches prior expand.rs surface exactly)
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
