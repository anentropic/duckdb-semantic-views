# Phase 13: Type-mapping and property-based tests for typed column dispatch - Context

**Gathered:** 2026-03-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Refactor the typed output pipeline in `semantic_view()` to read column values directly from DuckDB result chunk binary data (instead of the current VARCHAR-cast intermediary), fix silent bugs for TIMESTAMP and BOOLEAN, add native DECIMAL and LIST dispatch, and verify the full pipeline with property-based tests.

No new user-facing query syntax or DDL. Users experience this as correct types in their results.

</domain>

<decisions>
## Implementation Decisions

### Architecture: replace VARCHAR intermediary with binary reads

Replace the current `build_varchar_cast_sql()` wrapper (which casts all columns to VARCHAR before reading) with direct typed binary reads from DuckDB result chunk vectors using `duckdb_vector_get_data()` + type-specific dispatch. This eliminates the string parse/reformat layer for all types that have flat binary representations.

Keep VARCHAR path only for types with no native flat representation (STRUCT, MAP, LIST of STRUCT, BLOB, BIT, VARINT).

### Bugs to fix (silent data corruption)

- **TIMESTAMP**: currently declared as TIMESTAMP in schema but all values come back NULL (VARCHAR string "2024-01-15 10:30:00" fails `parse::<i64>()` → TypedValue::Null). Fix: read i64 microseconds directly from binary chunk.
- **BOOLEAN**: currently declared as BOOLEAN in schema but written via string path — type mismatch at FFI level, undefined behaviour. Fix: read u8 (0/1) directly, write to BOOLEAN slot.

### Type dispatch scope

**Native binary dispatch (Phase 13):**
- All integer types: TINYINT, SMALLINT, INTEGER, BIGINT, UTINYINT, USMALLINT, UINTEGER, UBIGINT
- HUGEINT / UHUGEINT: read as 128-bit, output as BIGINT (truncated) — acceptable for real-world aggregate values
- Float types: FLOAT, DOUBLE
- BOOLEAN: u8 read, BOOLEAN output
- Temporal: DATE (i32 days), TIMESTAMP (i64 µs), TIMESTAMP_S/MS/NS, TIMESTAMP_TZ, TIME
- DECIMAL: read HUGEINT + scale from logical type metadata, declare DECIMAL(width, scale) output
- UUID: 16-byte copy, declare UUID output type
- ENUM: read ordinal, decode via `duckdb_enum_dictionary_value()`, output as VARCHAR (string values are correct semantic output)
- LIST with scalar element types (INTEGER, BIGINT, DOUBLE, BOOLEAN, DATE, TIMESTAMP, VARCHAR): read offset/length parent + child vector, declare LIST(element_type) output. Covers ARRAY_AGG use cases.

**VARCHAR fallback (intentional, deferred to future milestone):**
- STRUCT
- MAP
- LIST where element type is STRUCT or MAP
- INTERVAL (compound struct type)
- BLOB
- BIT, VARINT (exotic)

### User-facing impact

This phase is motivated by actual data loss:
- DECIMAL metrics/dimensions returned as VARCHAR — arithmetic on results breaks without explicit CAST
- TIMESTAMP columns returned as all-NULL — silent data loss
- BOOLEAN columns — undefined FFI behaviour
- LIST (ARRAY_AGG) returned as "[1, 2, 3]" string — list functions (list_contains, len, unnest) don't work on results

After this phase, users get correctly typed columns they can use in further DuckDB queries without manual casting.

### Property-based test strategy

**Two layers:**

1. **Unit PBTs** — test binary-read helper functions in isolation. Generate arbitrary in-bounds (chunk data, type_id) combinations. Property: never panics. Also test boundary values (i64::MAX, i32::MIN, -0.0, date epoch extremes, empty string → NULL).

2. **Integration PBTs** — full roundtrip via in-memory DuckDB. Strategy:
   - Proptest generates a Vec of typed values for each supported type
   - Create a DuckDB in-memory table with those values
   - Define a semantic_view over it with those columns as dimensions/metrics
   - Query via `semantic_view()`
   - Assert: output column type matches source type, output values match input values
   - NULL values: assert null rows in source → null rows in output

Existing `expand_proptest.rs` PBTs (SQL structure properties) are unchanged — they test the expansion engine, not the output layer.

### Claude's Discretion

- Exact Rust types used for DECIMAL intermediate (i128 vs two i64s for HUGEINT backing)
- Whether to use the `proptest` crate (already in use) or add `quickcheck`
- How to create mock DuckDB data chunks for unit PBTs (or whether to use DuckDB in-memory for both layers)
- Whether TIMESTAMP_TZ, TIMESTAMP_S/MS/NS get individual test cases or are grouped with TIMESTAMP
- Test file organisation (extend `expand_proptest.rs` or create new `output_proptest.rs`)

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets

- `read_varchar_from_vector()` in `table_function.rs`: already handles `duckdb_string_t` binary layout — keep this for VARCHAR elements in LIST child vectors and VARCHAR column fallback
- `try_infer_schema()` in `table_function.rs`: runs LIMIT 0 to get column names + type IDs — keep but extend to also return `duckdb_logical_type` handles (needed for DECIMAL scale, ENUM dictionary, LIST child type)
- `date_str_to_epoch_days()` in `table_function.rs`: delete after refactor — DATE values read as i32 binary directly, no string parsing needed
- `parse_typed_from_str()` and `build_varchar_cast_sql()`: delete after refactor — replaced by binary read dispatch
- `expand_proptest.rs`: existing proptest infrastructure, `simple_definition()` / `joined_definition()` fixtures reusable — extend for output type tests

### Established Patterns

- All FFI calls use `libduckdb_sys as ffi` — continue this pattern for new binary read helpers
- `unsafe` blocks are contained and documented — new binary read helpers follow the same pattern
- `#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]` on model types — consider using `arbitrary` feature for unit PBTs if proptest strategy composition gets complex
- Phase 12 added `column_type_names` + `column_types_inferred` (Vec<u32>) to `SemanticViewDefinition` — Phase 13 may need to also store logical type metadata (width/scale for DECIMAL, child type for LIST) if DDL-time inference is to support these types

### Integration Points

- `SemanticViewVTab::bind()` in `table_function.rs`: the main surgery site — replace type_map lookup + `type_from_duckdb_type_u32` declaration with logical-type-aware declarations
- `SemanticViewVTab::func()` in `table_function.rs`: replace `build_varchar_cast_sql` + string-parse pipeline with binary-read dispatch per column
- `try_infer_schema()`: may need a sibling function `try_infer_logical_types()` that returns `duckdb_logical_type` handles for DECIMAL/LIST/ENUM metadata, not just the u32 type enum
- `SemanticViewDefinition` in `model.rs`: if DECIMAL scale / LIST child type must survive DDL-time inference and be stored for query-time use, the model needs new fields (or the LIMIT 0 query is re-run at bind time — acceptable since it's already done as fallback path)

</code_context>

<specifics>
## Specific Ideas

- The motivation is entirely user-facing: "without this we're returning DECIMAL as VARCHAR, TIMESTAMP as all-NULL, BOOLEAN as UB, and LIST as a string" — planner should keep this framing when writing tasks
- ARRAY_AGG is a concrete LIST use case the user cares about
- The "refactor + test" framing: the refactor (binary reads) and the tests (PBTs) are coupled — the tests validate the refactor is correct, not an optional add-on
- User is not familiar with Rust/C++ internals — implementation choices should be explained in comments

</specifics>

<deferred>
## Deferred Ideas

- STRUCT output — future milestone
- MAP output — future milestone
- LIST where element type is STRUCT or MAP — future milestone
- INTERVAL native output (compound struct type) — future milestone
- BLOB output — explicitly not needed
- HUGEINT output type (vs BIGINT truncation) — could revisit if users hit overflow issues

</deferred>

---

*Phase: 13-type-mapping-and-property-based-tests-for-typed-column-dispatch*
*Context gathered: 2026-03-02*
