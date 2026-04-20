# Phase 56: YAML Export - Research

**Researched:** 2026-04-20
**Domain:** YAML serialization, scalar function registration, round-trip fidelity
**Confidence:** HIGH

## Summary

Phase 56 adds a new scalar function `READ_YAML_FROM_SEMANTIC_VIEW('name')` that exports a stored semantic view definition as a YAML string. The core challenge is straightforward: the `SemanticViewDefinition` struct already has serde `Serialize` derives, and `yaml_serde::to_string` produces valid YAML output. The main technical concern is field filtering -- the stored JSON includes internal runtime fields (`column_type_names`, `column_types_inferred`, `created_on`, `database_name`, `schema_name`) that must be excluded from the YAML export to produce clean, user-facing output that can round-trip through `CREATE SEMANTIC VIEW ... FROM YAML`.

The architecture pattern is well-established: the existing `GetDdlScalar` (`get_ddl.rs`) provides an exact template for implementing a new VScalar function. The function reads from `CatalogState`, deserializes the stored JSON into `SemanticViewDefinition`, strips internal fields, serializes to YAML, and returns the string. Round-trip fidelity (YAML-08) requires that the exported YAML, when fed back through `FROM YAML $$ ... $$`, produces a `SemanticViewDefinition` that is semantically identical to the original (excluding the internal runtime fields that are re-populated at define time).

**Primary recommendation:** Create a `render_yaml.rs` module (parallel to `render_ddl.rs`) with a `render_yaml_export` function that clones the definition, zeros out internal fields, and calls `yaml_serde::to_string`. Register a new `ReadYamlFromSemanticViewScalar` VScalar in `ddl/` following the `GetDdlScalar` pattern exactly.

## Project Constraints (from CLAUDE.md)

- **Quality gate**: `just test-all` (Rust unit tests + property-based tests + sqllogictest + DuckLake CI) [VERIFIED: CLAUDE.md]
- **Testing completeness**: sqllogictest required -- covers integration paths that Rust tests do not [VERIFIED: CLAUDE.md]
- **Build**: `just build` for debug extension binary; `cargo test` runs without extension feature [VERIFIED: CLAUDE.md]
- **Snowflake reference**: When in doubt about SQL syntax or behaviour, refer to Snowflake semantic views [VERIFIED: CLAUDE.md]

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| YAML-04 | User can export a stored semantic view as YAML via `SELECT READ_YAML_FROM_SEMANTIC_VIEW('name')` (supports fully qualified names) | VScalar pattern from `GetDdlScalar`; `yaml_serde::to_string` for serialization; catalog state read for name lookup |
| YAML-08 | YAML round-trip is lossless -- `READ_YAML_FROM_SEMANTIC_VIEW` output can recreate an identical semantic view | Field stripping of internal fields; `PartialEq` on model structs for comparison; existing YAML FROM parser for re-import |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| yaml_serde | 0.10.4 | YAML serialization/deserialization | Already in Cargo.toml; serde-compatible YAML library [VERIFIED: Cargo.lock] |
| serde | 1.x | Serialization framework | Already in Cargo.toml; all model structs derive Serialize/Deserialize [VERIFIED: Cargo.toml] |
| duckdb (Rust bindings) | 1.10500.0 | DuckDB VScalar registration | Already in Cargo.toml; `VScalar` trait for scalar functions [VERIFIED: Cargo.toml] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde_json | 1.x | JSON deserialization of stored definitions | Already in Cargo.toml; stored definitions are JSON strings [VERIFIED: Cargo.toml] |

**No new dependencies required.** All libraries are already in the project.

## Architecture Patterns

### Recommended Project Structure
```
src/
  ddl/
    read_yaml.rs      # NEW: VScalar implementation (extension-gated)
    mod.rs             # ADD: pub mod read_yaml;
  render_yaml.rs       # NEW: YAML export logic (always compiled, unit-testable)
  lib.rs               # ADD: registration of read_yaml_from_semantic_view scalar
```

### Pattern 1: VScalar Registration (from GetDdlScalar)
**What:** Register a scalar function that takes a VARCHAR argument and returns a VARCHAR YAML string
**When to use:** This is the exact pattern for Phase 56
**Example:**
```rust
// Source: src/ddl/get_ddl.rs (existing code, verified)
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
                let name = DuckString::new(&mut { names[i] }).as_str().to_string();
                let guard = state.read()
                    .map_err(|_| Box::<dyn std::error::Error>::from("catalog lock poisoned"))?;
                let json = guard.get(&name)
                    .ok_or_else(|| format!("semantic view '{}' does not exist", name))?;
                let def: SemanticViewDefinition = serde_json::from_str(json)?;
                let yaml = render_yaml_export(&def)?;
                out_vec.insert(i, yaml.as_str());
            }
            Ok(())
        }))
    }

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)],  // name only (1 arg)
            LogicalTypeHandle::from(LogicalTypeId::Varchar),        // return: YAML string
        )]
    }
}
```

### Pattern 2: Field Stripping for Clean Export
**What:** Clone the stored definition and zero out internal runtime fields before serialization
**When to use:** The stored JSON includes fields that are populated at define time and should not appear in exported YAML
**Example:**
```rust
// Source: derived from model.rs analysis [VERIFIED: model.rs field list]
pub fn render_yaml_export(def: &SemanticViewDefinition) -> Result<String, String> {
    // Clone and strip internal fields that are populated at define time
    let mut export = def.clone();
    export.column_type_names.clear();
    export.column_types_inferred.clear();
    export.created_on = None;
    export.database_name = None;
    export.schema_name = None;

    yaml_serde::to_string(&export)
        .map_err(|e| format!("YAML serialization error: {e}"))
}
```

### Pattern 3: Fully Qualified Name Support
**What:** The function argument supports `database.schema.view_name` format
**When to use:** When looking up views from a specific database/schema context
**Analysis:** The existing `GetDdlScalar` does a simple `guard.get(&name)` lookup, which is a bare name match against the CatalogState HashMap. Fully qualified names (e.g., `memory.main.my_view`) need to be handled by stripping the `database.schema.` prefix to match the stored key. [VERIFIED: catalog.rs stores bare names]

**Important consideration:** The CatalogState stores views by bare name (no schema/database prefix). For fully qualified names, we need to extract the last component. The approach should match how Snowflake handles this -- the simple approach is to split on `.` and take the last segment. However, we must be careful about names containing dots.

The simplest reliable approach: split the input name on `.`, and if it has 2 or 3 parts, use the last part as the lookup key. This matches how `GET_DDL` handles it (it currently does not support FQN, but this phase requires it).

### Anti-Patterns to Avoid
- **Serializing internal fields**: Never export `column_type_names`, `column_types_inferred`, `created_on`, `database_name`, `schema_name` -- these are runtime-populated and break round-trip semantics
- **Modifying model.rs serde attributes**: Adding `skip_serializing` would affect the JSON persistence format, which must remain backward-compatible. Field stripping must happen in the export function, not in the serde annotations.
- **Creating a separate "export" struct**: The model already has serde derives; cloning and clearing fields is simpler and less maintenance than maintaining a parallel struct hierarchy

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| YAML serialization | Custom YAML string builder | `yaml_serde::to_string` | Handles all serde attributes, edge cases, escaping |
| Field filtering for export | Manual field-by-field serialization | Clone + clear internal fields | serde `skip_serializing_if` already handles empty/default values |
| Scalar function plumbing | Custom FFI boilerplate | `VScalar` trait implementation | Existing pattern in `get_ddl.rs`, handles chunks, types, error propagation |

**Key insight:** The model structs already have extensive `skip_serializing_if` annotations that automatically omit empty/default fields. When we clone a definition and clear internal fields, the resulting YAML will already be clean because the cleared fields will trigger their skip conditions.

## Common Pitfalls

### Pitfall 1: Internal Fields Leaking into Export
**What goes wrong:** The stored JSON includes `column_type_names`, `column_types_inferred`, `created_on`, `database_name`, `schema_name`. If exported as-is to YAML, the round-trip import will accept them (unknown fields are allowed) but they will be wrong values when the view is re-created (they get re-populated at define time).
**Why it happens:** These fields use `#[serde(default)]` without `skip_serializing_if`, so `yaml_serde::to_string` will include them in output.
**How to avoid:** Clone the definition and clear these fields before serialization.
**Warning signs:** Round-trip test shows extra fields in YAML output; re-imported view has stale `created_on` timestamp.

### Pitfall 2: Round-Trip Comparison Including Internal Fields
**What goes wrong:** Testing round-trip by comparing the original stored definition with the re-created one using `PartialEq` fails because internal fields differ (e.g., `created_on` timestamp is different, `column_types_inferred` may be repopulated differently).
**Why it happens:** The round-trip only needs semantic equivalence, not byte-for-byte identity of the stored JSON.
**How to avoid:** Compare only the user-facing fields: tables, dimensions, metrics, joins, facts, materializations, comment. The test should strip internal fields from both sides before comparison, OR the sqllogictest should verify by querying the re-created view and checking it produces identical results.
**Warning signs:** Unit tests pass but sqllogictest round-trip comparison fails.

### Pitfall 3: `column_type_names` and `column_types_inferred` Default Serialization
**What goes wrong:** These fields have `#[serde(default)]` but NO `skip_serializing_if`. An empty `Vec` will serialize as `column_type_names: []` in YAML, which is harmless for round-trip (deserializes back to empty vec) but makes the output verbose.
**Why it happens:** These fields were added early and never needed skip logic for JSON persistence.
**How to avoid:** Clear them in the export function (already in the recommendation). The cleared empty vec with `#[serde(default)]` but without `skip_serializing_if` will still serialize as `[]` unless we also handle this. Since we are cloning for export, we can also consider adding `skip_serializing_if = "Vec::is_empty"` to these fields in model.rs -- this is safe because empty is the default and JSON backward compat is maintained.
**Warning signs:** Exported YAML contains `column_type_names: []` and `column_types_inferred: []` lines.

**Resolution:** The best approach is to add `skip_serializing_if = "Vec::is_empty"` to `column_type_names` and `column_types_inferred`, and `skip_serializing_if = "Option::is_none"` to `created_on`, `database_name`, `schema_name` in model.rs. This is safe because: (a) empty/None is the default, (b) stored JSON without these fields already deserializes correctly, (c) new JSON written with these fields absent is handled by `#[serde(default)]`. This change makes both JSON and YAML output cleaner.

### Pitfall 4: Fully Qualified Name Parsing Edge Cases
**What goes wrong:** Naively splitting on `.` fails for quoted identifiers that contain dots.
**Why it happens:** DuckDB allows quoted identifiers like `"my.database"."my.schema"."my.view"`.
**How to avoid:** For v0.7.0, support simple unquoted `database.schema.view` format. Quoted identifiers with embedded dots are an edge case that can be deferred. The simple split-on-dot approach handles the success criteria requirement.
**Warning signs:** Test with dotted names in quotes fails.

## Code Examples

### YAML Export Function (render_yaml.rs)
```rust
// Source: derived from render_ddl.rs pattern + model.rs analysis [VERIFIED: codebase]
use crate::model::SemanticViewDefinition;

/// Export a semantic view definition as a YAML string suitable for
/// round-trip through `CREATE SEMANTIC VIEW ... FROM YAML $$ ... $$`.
///
/// Internal fields populated at define time (column types, timestamps,
/// database/schema context) are stripped before serialization.
pub fn render_yaml_export(def: &SemanticViewDefinition) -> Result<String, String> {
    let mut export = def.clone();
    // Strip internal runtime fields that are repopulated at define time
    export.column_type_names.clear();
    export.column_types_inferred.clear();
    export.created_on = None;
    export.database_name = None;
    export.schema_name = None;

    yaml_serde::to_string(&export)
        .map_err(|e| format!("YAML serialization error: {e}"))
}
```

### VScalar Registration in lib.rs
```rust
// Source: derived from lib.rs line 564 pattern [VERIFIED: lib.rs]
con.register_scalar_function_with_state::<ReadYamlFromSemanticViewScalar>(
    "read_yaml_from_semantic_view",
    &catalog_state,
)?;
```

### Fully Qualified Name Resolution
```rust
// Source: new code, simple approach [ASSUMED]
/// Extract the bare view name from a potentially qualified name.
/// Supports: "view_name", "schema.view_name", "database.schema.view_name"
fn resolve_bare_name(input: &str) -> &str {
    input.rsplit('.').next().unwrap_or(input)
}
```

### sqllogictest Round-Trip Pattern
```sql
-- Source: derived from phase45_get_ddl.test [VERIFIED: test/sql/phase45_get_ddl.test]

-- Create a view, export to YAML, drop, re-create from YAML, verify identical
statement ok
CREATE SEMANTIC VIEW p56_roundtrip AS
TABLES ( o AS test_orders PRIMARY KEY (id) )
DIMENSIONS ( o.region AS o.region )
METRICS ( o.total AS SUM(o.amount) )

-- Export YAML and store for comparison
statement ok
CREATE TABLE p56_yaml_store AS
SELECT READ_YAML_FROM_SEMANTIC_VIEW('p56_roundtrip') AS yaml_text;

-- Drop the original view
statement ok
DROP SEMANTIC VIEW p56_roundtrip

-- Re-create from the exported YAML (manual copy of expected output)
-- Note: DuckDB has no dynamic SQL EXECUTE, so we manually verify format
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `serde_yaml` (dtolnay) | `yaml_serde` 0.10 | 2024 (serde_yaml archived) | Drop-in replacement, same API [VERIFIED: Cargo.lock] |
| JSON-only export (GET_DDL) | JSON + YAML export | Phase 56 (this phase) | Users get version-control-friendly YAML format |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Simple `rsplit('.')` handles FQN parsing for unquoted identifiers | Architecture Patterns / Pattern 3 | Quoted identifiers with dots would fail; but success criteria only specifies "fully qualified names" without quoted-dot edge cases. LOW risk. |
| A2 | Adding `skip_serializing_if` to internal fields in model.rs is safe for backward compat | Common Pitfalls / Pitfall 3 | If existing stored JSON relies on explicit empty arrays being written, deserialization could differ. However, `#[serde(default)]` already handles missing fields. VERY LOW risk. |

## Open Questions

1. **Fully Qualified Name Semantics**
   - What we know: CatalogState stores bare names. The success criteria says "fully qualified names (database.schema.view_name) are supported".
   - What's unclear: Whether the function should validate the database/schema prefix against the actual context, or just strip it.
   - Recommendation: Strip the prefix and look up by bare name. This matches the simplest useful behavior and can be refined later. If the user specifies the wrong database/schema, they still get the view if it exists by that bare name.

2. **Whether to add `skip_serializing_if` to model.rs internal fields**
   - What we know: It would make both JSON and YAML serialization cleaner by omitting empty/None internal fields.
   - What's unclear: Whether this changes any stored JSON behavior.
   - Recommendation: Add it. The fields already have `#[serde(default)]` for deserialization, so omitting them from serialization is backward-compatible. This is a small model improvement that benefits all output paths.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust unit/proptest) + sqllogictest |
| Config file | justfile (build/test commands) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| YAML-04 | `READ_YAML_FROM_SEMANTIC_VIEW('name')` returns YAML string | unit + sqllogictest | `cargo test render_yaml` + `just test-sql` | Wave 0 |
| YAML-04 | Fully qualified name support | unit + sqllogictest | `cargo test render_yaml` + `just test-sql` | Wave 0 |
| YAML-04 | Materializations included in export | unit | `cargo test render_yaml` | Wave 0 |
| YAML-04 | Error on nonexistent view | sqllogictest | `just test-sql` | Wave 0 |
| YAML-08 | Lossless round-trip | unit + proptest + sqllogictest | `cargo test render_yaml` + `just test-sql` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `src/render_yaml.rs` -- YAML export function with field stripping
- [ ] `src/ddl/read_yaml.rs` -- VScalar implementation (extension-gated)
- [ ] `test/sql/phase56_yaml_export.test` -- sqllogictest integration tests
- [ ] Unit tests in render_yaml.rs for field stripping and round-trip
- [ ] Proptest extension in yaml_proptest.rs for round-trip via YAML export

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | Read-only function, same access as GET_DDL |
| V5 Input Validation | yes | View name validated against catalog; no user-controlled SQL injection surface |
| V6 Cryptography | no | N/A |

### Known Threat Patterns for this phase

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| View name injection | Tampering | Name used only as HashMap key lookup; no SQL interpolation [VERIFIED: catalog.rs] |
| Information disclosure via internal fields | Information Disclosure | Field stripping removes timestamps and database context from output |

## Sources

### Primary (HIGH confidence)
- `src/ddl/get_ddl.rs` -- VScalar pattern, scalar function registration, catalog read [VERIFIED: codebase]
- `src/model.rs` -- SemanticViewDefinition struct, serde attributes, internal field list [VERIFIED: codebase]
- `src/render_ddl.rs` -- DDL reconstruction pattern, clause ordering [VERIFIED: codebase]
- `src/catalog.rs` -- CatalogState type, HashMap<String, String> storage [VERIFIED: codebase]
- `src/lib.rs` -- Scalar function registration via `register_scalar_function_with_state` [VERIFIED: codebase]
- `test/sql/phase45_get_ddl.test` -- Round-trip sqllogictest pattern [VERIFIED: codebase]
- `test/sql/phase52_yaml_ddl.test` -- YAML DDL sqllogictest pattern [VERIFIED: codebase]
- `tests/yaml_proptest.rs` -- Property-based test for YAML/JSON equivalence [VERIFIED: codebase]
- `Cargo.toml` + `Cargo.lock` -- `yaml_serde` 0.10.4 dependency [VERIFIED: codebase]

### Secondary (MEDIUM confidence)
- [yaml_serde crate](https://crates.io/crates/yaml_serde) -- Maintained fork of serde_yaml [CITED: crates.io search]

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies already in project, no new libraries needed
- Architecture: HIGH -- exact VScalar pattern exists in `get_ddl.rs`; field stripping approach verified against model.rs serde attributes
- Pitfalls: HIGH -- internal field list verified by reading model.rs; round-trip semantics understood from existing YAML/JSON tests

**Research date:** 2026-04-20
**Valid until:** 2026-05-20 (stable domain, no external API dependencies)
