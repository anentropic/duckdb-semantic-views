// Write-side DDL (CREATE/DROP/ALTER) is handled by parser_override directly
// in `crate::parse`. The legacy table-function vtabs (define/drop/alter) and
// their `persist::execute_parameterized` helper were removed in v0.8.0's full
// architectural unification. Only `define::enrich_definition_for_create`
// remains — called by the parser_override CREATE rewrite.
pub mod alter_helpers_ffi;
pub mod define;
pub mod describe;
pub mod get_ddl;
pub mod list;
pub mod read_yaml;
pub mod show_columns;
pub mod show_dims;
pub mod show_dims_for_metric;
pub mod show_facts;
pub mod show_materializations;
pub mod show_metrics;
