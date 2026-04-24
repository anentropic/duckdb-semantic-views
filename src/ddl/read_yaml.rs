//! `READ_YAML_FROM_SEMANTIC_VIEW` scalar function: wraps
//! [`crate::render_yaml::render_yaml_export`] as a `VScalar` so that
//! `SELECT READ_YAML_FROM_SEMANTIC_VIEW('name')` works inside `DuckDB`.
//!
//! The render logic itself lives in [`crate::render_yaml`] (always compiled,
//! unit-tested under `cargo test`). This module adds the extension-only
//! `VScalar` registration.

use duckdb::core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId};
use duckdb::types::DuckString;
use duckdb::vscalar::{ScalarFunctionSignature, VScalar};
use duckdb::vtab::arrow::WritableVector;
use libduckdb_sys::duckdb_string_t;

use crate::catalog::CatalogState;
use crate::model::SemanticViewDefinition;
use crate::render_yaml::render_yaml_export;

/// Extract the bare view name from a potentially qualified name.
/// Supports: `"view_name"`, `"schema.view_name"`, `"database.schema.view_name"`.
fn resolve_bare_name(input: &str) -> &str {
    input.rsplit('.').next().unwrap_or(input)
}

pub struct ReadYamlFromSemanticViewScalar;

impl VScalar for ReadYamlFromSemanticViewScalar {
    type State = CatalogState;

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            let len = input.len();
            let name_vec = input.flat_vector(0);
            let names = name_vec.as_slice_with_len::<duckdb_string_t>(len);
            let out_vec = output.flat_vector();

            for i in 0..len {
                let raw_name = DuckString::new(&mut { names[i] }).as_str().to_string();
                let bare_name = resolve_bare_name(&raw_name);

                let guard = state
                    .read()
                    .map_err(|_| Box::<dyn std::error::Error>::from("catalog lock poisoned"))?;
                let json = guard
                    .get(bare_name)
                    .ok_or_else(|| format!("semantic view '{}' does not exist", bare_name))?;
                let def: SemanticViewDefinition = serde_json::from_str(json)?;
                let yaml = render_yaml_export(&def)
                    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                out_vec.insert(i, yaml.as_str());
            }
            Ok(())
        }))
    }

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)], // name only (1 arg)
            LogicalTypeHandle::from(LogicalTypeId::Varchar),       // return: YAML string
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_bare_name_unqualified() {
        assert_eq!(resolve_bare_name("my_view"), "my_view");
    }

    #[test]
    fn resolve_bare_name_schema_qualified() {
        assert_eq!(resolve_bare_name("main.my_view"), "my_view");
    }

    #[test]
    fn resolve_bare_name_fully_qualified() {
        assert_eq!(resolve_bare_name("memory.main.my_view"), "my_view");
    }

    #[test]
    fn resolve_bare_name_empty() {
        assert_eq!(resolve_bare_name(""), "");
    }
}
