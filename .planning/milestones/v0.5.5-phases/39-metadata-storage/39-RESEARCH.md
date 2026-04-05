# Phase 39: Metadata Storage - Research

**Researched:** 2026-04-01
**Domain:** Rust serde model extension, DuckDB C API metadata capture, backward-compatible JSON deserialization
**Confidence:** HIGH

## Summary

Phase 39 adds three categories of metadata to the `SemanticViewDefinition` model: (1) a `created_on` ISO 8601 timestamp captured at define time, (2) `database_name` and `schema_name` from the DuckDB connection context at define time, and (3) an `output_type` field on the `Fact` struct for downstream SHOW FACTS display. All new fields are `Option<String>` with `#[serde(default)]` to ensure old stored JSON without these fields deserializes cleanly.

This is a pure model + storage phase. No SHOW/DESCRIBE output changes occur here -- those are Phase 40 (SHOW alignment) and Phase 41 (DESCRIBE rewrite). The changes are confined to `src/model.rs` (struct fields), `src/ddl/define.rs` (metadata capture at bind time), and `src/body_parser.rs` (new field initialization). No catalog table schema changes are needed because definitions are stored as raw JSON VARCHAR blobs.

**Primary recommendation:** Add all four new fields to the model structs with `#[serde(default)]`, capture `created_on`/`database_name`/`schema_name` via `execute_sql_raw` on the `catalog_conn` connection in `DefineFromJsonVTab::bind()`, and infer fact `output_type` from the existing `column_type_names` / `column_types_inferred` map where possible.

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass (Rust unit tests + proptest + sqllogictest + DuckLake CI)
- **Build:** `just build` for debug extension; `cargo test` for unit tests; `just test-sql` requires fresh `just build`
- **SQL syntax reference:** Snowflake semantic views behavior when in doubt
- **Testing completeness:** A phase verification that only runs `cargo test` is incomplete -- sqllogictest covers integration paths that Rust tests do not

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| META-01 | SemanticViewDefinition stores created_on timestamp (Option String, ISO 8601) set at define time | Use `strftime(now(), '%Y-%m-%dT%H:%M:%SZ')` via `execute_sql_raw` on `catalog_conn`; add `created_on: Option<String>` with `#[serde(default)]` |
| META-02 | SemanticViewDefinition stores database_name (Option String) set at define time | Use `SELECT current_database()` via `execute_sql_raw` on `catalog_conn`; add `database_name: Option<String>` with `#[serde(default)]` |
| META-03 | SemanticViewDefinition stores schema_name (Option String) set at define time | Use `SELECT current_schema()` via `execute_sql_raw` on `catalog_conn`; add `schema_name: Option<String>` with `#[serde(default)]` |
| META-04 | Fact model gains output_type field (Option String) for data_type in SHOW FACTS | Add `output_type: Option<String>` with `#[serde(default)]` to `Fact` struct; populate from DDL-time type inference where possible |
| META-05 | Old stored JSON without new fields deserializes via serde(default) with no migration | All fields use `#[serde(default)]` on `Option<String>` -- `None` is the default for `Option` |
</phase_requirements>

## Architecture Patterns

### Current Model Structure

```
SemanticViewDefinition
  base_table: String
  tables: Vec<TableRef>
  dimensions: Vec<Dimension>     # has output_type: Option<String>
  metrics: Vec<Metric>           # has output_type: Option<String>
  joins: Vec<Join>
  facts: Vec<Fact>               # NO output_type -- META-04 adds it
  column_type_names: Vec<String>
  column_types_inferred: Vec<u32>
  -- NEW: created_on, database_name, schema_name
```

### Pattern 1: serde(default) for Backward Compatibility

**What:** Every new `Option<String>` field gets `#[serde(default)]` so that old stored JSON (without the field) deserializes to `None`.
**When to use:** Every time a field is added to a persisted struct.
**Established pattern:** Already used extensively in this codebase (Dimension.output_type, Metric.output_type, Join.name, Join.cardinality, etc.).

```rust
// Existing pattern from Dimension:
#[serde(default)]
pub output_type: Option<String>,

// Same pattern for new fields:
#[serde(default)]
pub created_on: Option<String>,

#[serde(default)]
pub database_name: Option<String>,

#[serde(default)]
pub schema_name: Option<String>,
```

**Confidence:** HIGH -- this is the exact pattern used for every optional field added since Phase 11.

### Pattern 2: Metadata Capture via execute_sql_raw

**What:** Use `execute_sql_raw` on the `catalog_conn` connection to run SQL that returns metadata values, then read them with `duckdb_value_varchar`.
**When to use:** When metadata depends on DuckDB runtime state (timestamps, connection context).
**Established pattern:** `resolve_pk_from_catalog()` in define.rs already uses `execute_sql_raw` on `catalog_conn` with `duckdb_value_varchar` to read constraint columns.

The metadata capture should happen in `DefineFromJsonVTab::bind()`, after JSON deserialization and before serialization to `json_out`. The `catalog_conn` is always available (created for both file-backed and in-memory databases).

**Connection choice:** Use `catalog_conn` (not `persist_conn`). Rationale:
- `catalog_conn` is always available (file-backed AND in-memory)
- `persist_conn` is `None` for in-memory databases
- `catalog_conn` is already used for define-time queries (PK resolution)
- All three metadata SQL functions (`strftime(now(), ...)`, `current_database()`, `current_schema()`) are lightweight and safe on any connection

**Confidence:** HIGH -- same FFI pattern as existing `resolve_pk_from_catalog`.

### Pattern 3: Fact output_type Population

**What:** Add `output_type: Option<String>` to `Fact` struct; populate it during define-time type inference.
**Approach:** Facts are row-level expressions inlined into metrics -- they do NOT appear as columns in the `expand()` output. Therefore, the existing `column_type_names`/`column_types_inferred` map does NOT contain fact types. Two options:

**Option A (recommended): Separate type inference per fact expression.**
After the existing `try_infer_schema` for dims/metrics, run a small additional query for each fact:
```sql
SELECT typeof(fact_expr) FROM table_name LIMIT 0
```
This leverages the same `execute_sql_raw` on `persist_conn` (file-backed only) or `catalog_conn`. The `typeof()` function returns the type as a VARCHAR string (e.g., "BIGINT", "DOUBLE").

However, this is complex because:
- Each fact has a `source_table` alias that must be resolved to a physical table name
- The expression may reference other facts (DAG), requiring inlining first
- Multiple queries per view may slow down define-time

**Option B (simpler, sufficient for META-04): Add the field but do not populate at define time.**
Add the field to the Fact struct with `#[serde(default)]` so it's `None` by default. Phase 40 (SHOW FACTS) can display an empty string for `data_type` when `output_type` is `None`, matching the current behavior. Fact type inference can be added later as an enhancement.

**Recommendation:** Option A with a pragmatic scope. Run a combined `SELECT typeof(expr1), typeof(expr2), ... FROM base_table LIMIT 1` query using `catalog_conn` for all facts on the same table. This avoids per-fact queries while resolving types. Fall back to `None` if the inference fails (e.g., complex expressions, unavailable table).

The key simplification: facts already have their `source_table` resolved to a table alias. The `tables` registry maps aliases to physical table names. Use this mapping to construct the inference query.

**Confidence:** MEDIUM -- the field addition is straightforward (HIGH), but the type inference approach needs validation during implementation.

### Connection Architecture at Define Time

```
DefineState
  catalog: CatalogState (Arc<RwLock<HashMap>>)
  persist_conn: Option<ffi::duckdb_connection>  -- file-backed only
  catalog_conn: ffi::duckdb_connection           -- ALWAYS available
  or_replace: bool
  if_not_exists: bool
```

The `catalog_conn` is the right connection for all three metadata captures:
- `strftime(now(), '%Y-%m-%dT%H:%M:%SZ')` -- timestamp
- `current_database()` -- returns "memory" for in-memory, DB name for file-backed
- `current_schema()` -- returns "main" (default schema)

All three can be combined into a single SQL query:
```sql
SELECT strftime(now(), '%Y-%m-%dT%H:%M:%SZ'), current_database(), current_schema()
```

This avoids three separate `execute_sql_raw` calls.

**Confidence:** HIGH

### Verified DuckDB Behavior

Tested against project's DuckDB version (1.5.0):

| Expression | Result | Type |
|-----------|--------|------|
| `typeof(now())` | `TIMESTAMP WITH TIME ZONE` | -- |
| `now()::VARCHAR` | `2026-04-01 23:44:32.40965+01` | Non-ISO 8601 (no T separator) |
| `strftime(now(), '%Y-%m-%dT%H:%M:%SZ')` | `2026-04-01T23:44:32Z` | Proper ISO 8601 |
| `current_database()` (in-memory) | `memory` | VARCHAR |
| `current_schema()` | `main` | VARCHAR |

**Critical:** Do NOT use `now()::VARCHAR` -- the DuckDB timestamp-to-varchar cast does NOT produce ISO 8601 format (uses space separator, includes timezone offset without T). Use `strftime(now(), '%Y-%m-%dT%H:%M:%SZ')` for proper ISO 8601.

**Confidence:** HIGH -- tested on project's DuckDB version

### Design Decision: database_name/schema_name Staleness

STATE.md records: "[Research]: Init-time caching of database_name/schema_name (not stored in JSON) -- stored names become wrong if DB file re-attached under different alias"

This refers to a concern that if a DB file is created as `mydb.duckdb`, the `current_database()` at define time returns `mydb`, but if the same file is later attached as `analytics`, the stored `database_name` is stale.

**Resolution for Phase 39:** The requirements (META-02, META-03) and success criteria explicitly say "stores database_name/schema_name from the DuckDB connection context at define time." Phase 39 stores them in the model as written. Phase 40 (SHOW alignment) has the option of using stored values or live connection values for display -- that is Phase 40's design decision, not Phase 39's.

For Phase 39, store the values. They represent the context at creation time, which is semantically meaningful (analogous to Snowflake's `created_on` representing when the view was created, not the current time).

**Confidence:** HIGH

### Project Structure Impact

Files to modify:
```
src/
  model.rs           # Add fields to SemanticViewDefinition and Fact
  body_parser.rs     # Initialize Fact.output_type = None at parse time
  ddl/define.rs      # Capture metadata in bind(), populate fact output_type
```

Files NOT modified:
```
src/catalog.rs       # No schema change (JSON VARCHAR blob)
src/ddl/show_*.rs    # Output changes are Phase 40
src/ddl/describe.rs  # Output changes are Phase 41
src/parse.rs         # No DDL syntax change
```

Test files to add/modify:
```
src/model.rs         # Unit tests for new field serde roundtrip + backward compat
src/ddl/define.rs    # Integration test for metadata capture
test/sql/            # sqllogictest for end-to-end metadata storage verification
```

### Anti-Patterns to Avoid

- **Using `std::time::SystemTime` instead of DuckDB `now()`:** The STATE.md decision explicitly says use DuckDB's `now()`. This ensures the timestamp is consistent with the DuckDB instance's clock, not the host system clock. Also avoids Rust chrono/time dependency.
- **Using `persist_conn` for metadata queries:** `persist_conn` is `None` for in-memory databases. Use `catalog_conn` which is always available.
- **Modifying catalog table schema:** The `_definitions` table stores raw JSON VARCHAR. No migration needed -- the JSON schema evolves via serde defaults.
- **Changing SHOW/DESCRIBE output in this phase:** Phase 39 is strictly model + storage. Output changes are Phase 40 and Phase 41.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| ISO 8601 formatting | Rust chrono or manual string formatting | DuckDB `strftime(now(), '%Y-%m-%dT%H:%M:%SZ')` | Consistent with DuckDB clock, no new dependency |
| JSON backward compat | Version migration code | serde `#[serde(default)]` on Option fields | Established pattern, zero-cost deserialization |
| Type name strings | Manual type mapping tables | DuckDB `typeof(expr)` function | Returns canonical type name as VARCHAR |

## Common Pitfalls

### Pitfall 1: Non-ISO 8601 Timestamp Format
**What goes wrong:** Using `now()::VARCHAR` produces `2026-04-01 23:44:32.40965+01` instead of ISO 8601.
**Why it happens:** DuckDB's timestamp-to-varchar cast uses space separator and includes timezone offset without T.
**How to avoid:** Always use `strftime(now(), '%Y-%m-%dT%H:%M:%SZ')`.
**Warning signs:** Timestamp string lacks `T` separator or has `+01` suffix.

### Pitfall 2: duckdb_value_varchar Returns NULL for Non-VARCHAR Columns
**What goes wrong:** `duckdb_value_varchar` may return NULL for certain column types in DuckDB 1.5.0 (documented for LIST columns).
**Why it happens:** DuckDB C API behavior for non-scalar types.
**How to avoid:** Ensure the SQL query casts to VARCHAR or uses functions that return VARCHAR (e.g., `strftime` returns VARCHAR, `current_database()` returns VARCHAR). All three metadata functions return VARCHAR natively.
**Warning signs:** NULL pointer from `duckdb_value_varchar`.

### Pitfall 3: Forgetting to Update body_parser.rs
**What goes wrong:** `Fact` struct gains `output_type` field but `body_parser.rs` constructs `Fact` without it, causing compile error.
**Why it happens:** The parser creates `Fact` structs directly (not via Default).
**How to avoid:** Update the `Fact` construction in `body_parser.rs` to include `output_type: None`.
**Warning signs:** Compiler error on `Fact { name, expr, source_table }` missing field.

### Pitfall 4: Forgetting Test Helpers
**What goes wrong:** Test helper functions in `graph/test_helpers.rs` and `expand/sql_gen.rs` construct `Fact` structs without the new `output_type` field.
**Why it happens:** Many test helpers construct model structs inline.
**How to avoid:** Search for all `Fact {` and `Fact{` patterns in the codebase and update them. Use `..Default::default()` where possible, but note that existing code uses explicit field initialization.
**Warning signs:** Compiler errors in test files after model change.

### Pitfall 5: Breaking Proptest Arbitrary Derivation
**What goes wrong:** Adding `output_type: Option<String>` to `Fact` while `#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]` is active.
**Why it happens:** `Option<String>` already implements `Arbitrary`, so this should work. But verify.
**How to avoid:** Run `cargo test` after model change to catch any proptest issues.
**Warning signs:** Proptest compilation or generation failures.

## Code Examples

### Adding Fields to SemanticViewDefinition

```rust
// In src/model.rs - add to SemanticViewDefinition struct:

/// ISO 8601 timestamp of when this semantic view was created.
/// Captured at define time via DuckDB `now()`.
/// Old stored JSON without this field deserializes to None.
#[serde(default)]
pub created_on: Option<String>,

/// Database name from the connection context at define time.
/// Old stored JSON without this field deserializes to None.
#[serde(default)]
pub database_name: Option<String>,

/// Schema name from the connection context at define time.
/// Old stored JSON without this field deserializes to None.
#[serde(default)]
pub schema_name: Option<String>,
```

### Adding output_type to Fact

```rust
// In src/model.rs - add to Fact struct:

/// Optional output type for this fact, used by SHOW FACTS data_type column.
/// Populated at define time via type inference when possible.
/// Old stored JSON without this field deserializes to None.
#[serde(default)]
pub output_type: Option<String>,
```

### Metadata Capture in define.rs bind()

```rust
// In DefineFromJsonVTab::bind(), after deserialization and before json_out:

// Capture metadata: timestamp, database_name, schema_name
// Uses catalog_conn which is always available (file-backed AND in-memory).
let metadata_sql = "SELECT strftime(now(), '%Y-%m-%dT%H:%M:%SZ'), \
                    current_database(), current_schema()";
let metadata_result = unsafe {
    crate::query::table_function::execute_sql_raw(state.catalog_conn, metadata_sql)
};
if let Ok(mut result) = metadata_result {
    unsafe {
        // Column 0: created_on (ISO 8601)
        let ts_ptr = ffi::duckdb_value_varchar(&mut result, 0, 0);
        if !ts_ptr.is_null() {
            def.created_on = Some(
                CStr::from_ptr(ts_ptr).to_string_lossy().into_owned()
            );
            ffi::duckdb_free(ts_ptr as *mut std::ffi::c_void);
        }
        // Column 1: database_name
        let db_ptr = ffi::duckdb_value_varchar(&mut result, 1, 0);
        if !db_ptr.is_null() {
            def.database_name = Some(
                CStr::from_ptr(db_ptr).to_string_lossy().into_owned()
            );
            ffi::duckdb_free(db_ptr as *mut std::ffi::c_void);
        }
        // Column 2: schema_name
        let schema_ptr = ffi::duckdb_value_varchar(&mut result, 2, 0);
        if !schema_ptr.is_null() {
            def.schema_name = Some(
                CStr::from_ptr(schema_ptr).to_string_lossy().into_owned()
            );
            ffi::duckdb_free(schema_ptr as *mut std::ffi::c_void);
        }
        ffi::duckdb_destroy_result(&mut result);
    }
}
```

### Fact Type Inference (Optional Enhancement)

```rust
// After existing type inference, infer fact types via typeof():
// Build a single query: SELECT typeof(expr1) AS t0, typeof(expr2) AS t1, ...
// FROM base_table LIMIT 1
//
// Only attempt if persist_conn or catalog_conn is available and tables are resolvable.
if !def.facts.is_empty() {
    let conn = state.persist_conn.unwrap_or(state.catalog_conn);
    // Resolve alias -> physical table name
    let alias_to_table: HashMap<String, String> = def.tables.iter()
        .map(|t| (t.alias.to_ascii_lowercase(), t.table.clone()))
        .collect();
    
    // Group facts by source table for batched queries
    for fact in &mut def.facts {
        if let Some(alias) = &fact.source_table {
            let table = alias_to_table
                .get(&alias.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| def.base_table.clone());
            let sql = format!(
                "SELECT typeof({}) FROM \"{}\" LIMIT 1",
                fact.expr, table
            );
            if let Ok(mut result) = unsafe {
                crate::query::table_function::execute_sql_raw(conn, &sql)
            } {
                let row_count = unsafe { ffi::duckdb_row_count(&mut result) };
                if row_count > 0 {
                    let val_ptr = unsafe {
                        ffi::duckdb_value_varchar(&mut result, 0, 0)
                    };
                    if !val_ptr.is_null() {
                        let type_name = unsafe {
                            CStr::from_ptr(val_ptr).to_string_lossy().into_owned()
                        };
                        unsafe { ffi::duckdb_free(val_ptr as *mut std::ffi::c_void) };
                        fact.output_type = Some(type_name);
                    }
                }
                unsafe { ffi::duckdb_destroy_result(&mut result) };
            }
        }
    }
}
```

**Note:** Fact type inference via `typeof()` requires `LIMIT 1` (not `LIMIT 0`) because `typeof()` needs at least one row to evaluate. If the table is empty, `typeof()` returns no rows. This is a known limitation -- the `output_type` will be `None` for facts on empty tables. This is acceptable because:
1. The field is `Option<String>` -- `None` gracefully degrades to empty in SHOW output
2. Using `typeof()` is simpler than DuckDB's internal type inference machinery
3. The type inference is best-effort, same as the existing dim/metric inference

**Alternative approach:** Use `SELECT typeof(expr) FROM (SELECT NULL::INT AS col1, NULL::VARCHAR AS col2, ...) AS t LIMIT 1` with synthetic NULL values. This avoids the empty-table problem but requires knowing column types, creating a chicken-and-egg problem.

**Simplest viable approach:** If the above proves too complex during implementation, just add the field as `None` at parse time and skip inference entirely. The SHOW FACTS display can show empty string for data_type, matching current behavior. Type inference for facts can be added as a follow-up enhancement.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) + sqllogictest (Python runner) |
| Config file | Cargo.toml (test configuration) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| META-01 | created_on field stores ISO 8601 timestamp | unit | `cargo test model::tests -- created_on` | Wave 0 |
| META-02 | database_name field stores string | unit | `cargo test model::tests -- database_name` | Wave 0 |
| META-03 | schema_name field stores string | unit | `cargo test model::tests -- schema_name` | Wave 0 |
| META-04 | Fact.output_type field exists and roundtrips | unit | `cargo test model::tests -- fact_output_type` | Wave 0 |
| META-05 | Old JSON without new fields deserializes | unit | `cargo test model::tests -- old_json_without` | Wave 0 |
| META-01-03 | End-to-end: create view, read back JSON, verify metadata | sqllogictest | `just test-sql` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/model.rs` unit tests: roundtrip tests for `created_on`, `database_name`, `schema_name` on `SemanticViewDefinition`
- [ ] `src/model.rs` unit tests: roundtrip test for `output_type` on `Fact`
- [ ] `src/model.rs` unit tests: backward compat test for old JSON without new fields
- [ ] `test/sql/phase39_metadata_storage.test` - sqllogictest for end-to-end metadata storage verification

*(These are new tests that need to be written as part of implementation, following the existing test patterns in model.rs)*

## Open Questions

1. **Fact type inference complexity**
   - What we know: Facts are row-level expressions; their types can be inferred via `typeof(expr)` but this requires table data (LIMIT 1, not LIMIT 0).
   - What's unclear: Whether the implementation complexity is worth it for this phase, or whether the field should just be added as `None` and type inference deferred.
   - Recommendation: Attempt type inference using `typeof()`. Fall back to `None` if it fails. This gives Phase 40 the best possible data while keeping the implementation pragmatic. If inference proves too complex (e.g., fact expressions referencing other facts in DAG order), defer to just adding the field.

2. **database_name for in-memory databases**
   - What we know: `current_database()` returns `"memory"` for in-memory databases.
   - What's unclear: Is `"memory"` the right value to store, or should it be `None`?
   - Recommendation: Store `"memory"` -- it's accurate and consistent. Phase 40 can display it or choose to override.

## Sources

### Primary (HIGH confidence)
- Project source code: `src/model.rs`, `src/ddl/define.rs`, `src/catalog.rs`, `src/body_parser.rs`
- DuckDB 1.5.0 verified behavior: `now()`, `strftime()`, `current_database()`, `current_schema()` tested via project venv
- [DuckDB Timestamp Types](https://duckdb.org/docs/stable/sql/data_types/timestamp) -- timestamp format documentation
- [DuckDB Timestamp Functions](https://duckdb.org/docs/current/sql/functions/timestamp) -- strftime reference

### Secondary (MEDIUM confidence)
- `.planning/research/FEATURES.md` -- fact data_type design analysis
- `.planning/research/ARCHITECTURE.md` -- module structure and change impact
- `.planning/STATE.md` -- locked decisions about timestamp capture and database_name caching

## Metadata

**Confidence breakdown:**
- Model fields (META-01-05): HIGH -- exact pattern match with existing codebase conventions
- Metadata capture (created_on, db/schema): HIGH -- verified DuckDB functions, established FFI pattern
- Fact type inference: MEDIUM -- approach is sound but implementation complexity uncertain
- Backward compat: HIGH -- serde(default) on Option is zero-risk

**Research date:** 2026-04-01
**Valid until:** 2026-05-01 (stable domain, no external dependency changes expected)
