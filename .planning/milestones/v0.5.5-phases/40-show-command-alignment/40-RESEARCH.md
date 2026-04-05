# Phase 40: SHOW Command Alignment - Research

**Researched:** 2026-04-01
**Domain:** DuckDB VTab column schema changes, Rust FFI vector insertion, sqllogictest updates
**Confidence:** HIGH

## Summary

Phase 40 aligns all six SHOW SEMANTIC commands with Snowflake-style output schemas. The changes are purely to the VTab `bind()` column declarations and `func()` row emission logic -- no new table functions, no parse.rs changes, no model changes. Phase 39 already added the metadata fields (`created_on`, `database_name`, `schema_name`, `output_type` on Fact) that Phase 40 now surfaces.

The core work is in five VTab files: `list.rs` (SHOW VIEWS), `show_dims.rs` (SHOW DIMS), `show_metrics.rs` (SHOW METRICS), `show_facts.rs` (SHOW FACTS), and `show_dims_for_metric.rs` (SHOW DIMS FOR METRIC). Each file needs its row struct, `bind_output_columns`, and `emit_rows` functions updated. Three sqllogictest files need updated expectations. The `build_filter_suffix` function in parse.rs generates `WHERE name ILIKE/LIKE ...` which references the `name` column -- all new schemas still have a `name` column, so filtering continues to work without parse.rs changes.

One new concern: the `required` column in SHOW DIMS FOR METRIC is BOOLEAN, and the codebase has never used `LogicalTypeId::Boolean` before. The pattern for boolean insertion is `vector.as_mut_slice::<bool>()[i] = false;` (verified from the upstream DuckDB Rust crate's `excel.rs` VTab example).

**Primary recommendation:** Update each VTab file's row struct, column declaration, and emission logic to match the target schemas. Update all three sqllogictest files atomically. No parse.rs changes needed.

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass (Rust unit tests + proptest + sqllogictest + DuckLake CI)
- **Build:** `just build` for debug extension; `cargo test` for unit tests; `just test-sql` requires fresh `just build`
- **SQL syntax reference:** Snowflake semantic views behavior when in doubt
- **Testing completeness:** A phase verification that only runs `cargo test` is incomplete -- sqllogictest covers integration paths that Rust tests do not

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SHOW-01 | SHOW SEMANTIC VIEWS returns 5 columns: created_on, name, kind, database_name, schema_name | Rewrite `list.rs` ListBindData row struct from `(String, String)` to a 5-field struct; `kind` is constant `"SEMANTIC_VIEW"`; `created_on`/`database_name`/`schema_name` from `SemanticViewDefinition` fields added in Phase 39 |
| SHOW-02 | SHOW SEMANTIC DIMENSIONS returns 6 columns: database_name, schema_name, semantic_view_name, table_name, name, data_type | Update `show_dims.rs` ShowDimRow: drop `expr`, rename `source_table` to `table_name`, add `database_name` and `schema_name` from the definition |
| SHOW-03 | SHOW SEMANTIC METRICS returns 6 columns: database_name, schema_name, semantic_view_name, table_name, name, data_type | Update `show_metrics.rs` ShowMetricRow: same pattern as SHOW-02 |
| SHOW-04 | SHOW SEMANTIC FACTS returns 6 columns: database_name, schema_name, semantic_view_name, table_name, name, data_type | Update `show_facts.rs` ShowFactRow: drop `expr`, rename `source_table` to `table_name`, add `database_name`, `schema_name`, `data_type` (from `Fact.output_type` added in META-04) |
| SHOW-05 | SHOW SEMANTIC DIMENSIONS FOR METRIC returns 4 columns: table_name, name, data_type, required (BOOLEAN, constant FALSE) | Rewrite `show_dims_for_metric.rs`: drop `semantic_view_name` and `expr`, rename `source_table` to `table_name`, add `required` BOOLEAN column using `LogicalTypeId::Boolean` and `as_mut_slice::<bool>()` |
| SHOW-06 | expr column removed from all SHOW commands | Verified: `expr` field currently exists in ShowDimRow, ShowMetricRow, ShowFactRow, and ShowDimForMetricRow; all need it removed from struct, bind, and emit |
| SHOW-07 | source_table renamed to table_name in all SHOW commands | Verified: `source_table` field exists in all four row structs; rename field to `table_name`, update bind column name from `"source_table"` to `"table_name"` |
| SHOW-08 | LIKE, STARTS WITH, LIMIT filtering continues to work | No parse.rs changes needed; `build_filter_suffix` generates `WHERE name ILIKE/LIKE ...` and all new schemas retain the `name` column at the same semantic position |
</phase_requirements>

## Architecture Patterns

### Current vs Target Column Schemas

**SHOW SEMANTIC VIEWS (list.rs)**
```
Current:  name, base_table                              (2 cols)
Target:   created_on, name, kind, database_name, schema_name  (5 cols)
```

**SHOW SEMANTIC DIMENSIONS (show_dims.rs)**
```
Current:  semantic_view_name, name, expr, source_table, data_type  (5 cols)
Target:   database_name, schema_name, semantic_view_name, table_name, name, data_type  (6 cols)
```

**SHOW SEMANTIC METRICS (show_metrics.rs)**
```
Current:  semantic_view_name, name, expr, source_table, data_type  (5 cols)
Target:   database_name, schema_name, semantic_view_name, table_name, name, data_type  (6 cols)
```

**SHOW SEMANTIC FACTS (show_facts.rs)**
```
Current:  semantic_view_name, name, expr, source_table  (4 cols)
Target:   database_name, schema_name, semantic_view_name, table_name, name, data_type  (6 cols)
```

**SHOW SEMANTIC DIMENSIONS FOR METRIC (show_dims_for_metric.rs)**
```
Current:  semantic_view_name, name, expr, source_table, data_type  (5 cols)
Target:   table_name, name, data_type, required  (4 cols)
```

### Pattern 1: VTab Column Schema Change

**What:** Each VTab file follows an identical 3-layer pattern: (1) row struct, (2) `bind_output_columns` declares schema, (3) `emit_rows` writes data. All three layers must be updated consistently.

**When to use:** Every VTab in this phase.

**Example (show_dims.rs target state):**
```rust
struct ShowDimRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    table_name: String,  // renamed from source_table
    name: String,
    data_type: String,
    // expr: REMOVED
}

fn bind_output_columns(bind: &BindInfo) {
    bind.add_result_column("database_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("schema_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("semantic_view_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("table_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("data_type", LogicalTypeHandle::from(LogicalTypeId::Varchar));
}
```

### Pattern 2: Boolean Column Insertion (new to codebase)

**What:** The `required` column in SHOW DIMS FOR METRIC uses `LogicalTypeId::Boolean` and writes values via `as_mut_slice::<bool>()` instead of `Inserter::insert()`.

**Why:** The DuckDB Rust crate has no `Inserter<bool>` implementation. The `as_mut_slice` pattern is used in the upstream DuckDB crate's own `excel.rs` VTab.

**Example:**
```rust
// In bind:
bind.add_result_column("required", LogicalTypeHandle::from(LogicalTypeId::Boolean));

// In func/emit_rows:
let mut required_vec = output.flat_vector(3); // 4th column (0-indexed)
let required_slice = required_vec.as_mut_slice::<bool>();
for i in 0..n {
    required_slice[i] = false;  // constant FALSE per requirement
}
```

### Pattern 3: Metadata from SemanticViewDefinition

**What:** `database_name` and `schema_name` are `Option<String>` on `SemanticViewDefinition`, added in Phase 39 (META-02, META-03). `created_on` is also `Option<String>` (META-01). Old stored JSON without these fields deserializes to `None` via `#[serde(default)]`.

**How to access:** In `collect_dims`, `collect_metrics`, `collect_facts`, the definition is already parsed via `SemanticViewDefinition::from_json()`. Access `def.database_name.clone().unwrap_or_default()` etc.

**For list.rs:** Must parse each JSON entry to extract `created_on`/`database_name`/`schema_name` -- currently only extracts `base_table` via raw serde_json. Switch to `SemanticViewDefinition::from_json()` for consistency.

### Pattern 4: Collect Functions Must Propagate View-Level Metadata

**What:** The `collect_dims`, `collect_metrics`, `collect_facts` helper functions currently take `(view_name, json)` and extract per-dimension/metric/fact rows. They must now also propagate `database_name` and `schema_name` from the view definition into each row.

**How:** The definition is already parsed inside `collect_*` functions. Extract `database_name`/`schema_name` from `def` and include in each row.

### Anti-Patterns to Avoid

- **Partial schema updates:** Updating `bind_output_columns` without updating `emit_rows` or the row struct will cause column count mismatches at runtime. All three must change together.
- **Changing column order in bind but not in emit:** `output.flat_vector(N)` is positional. If you reorder columns in bind, you must reorder the vector indices in emit.
- **Updating VTabs without updating sqllogictest:** sqllogictest `query TTTTT` directives encode column count and types. VTab changes and test updates must be atomic.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Boolean value insertion | Custom FFI code | `as_mut_slice::<bool>()` on `FlatVector` | Upstream-tested pattern from DuckDB Rust crate |
| JSON parsing in list.rs | Manual `serde_json::Value` extraction | `SemanticViewDefinition::from_json()` | Type-safe, consistent with other VTabs, handles `serde(default)` for missing fields |

## Common Pitfalls

### Pitfall 1: sqllogictest Column Type Mismatch

**What goes wrong:** sqllogictest `query` directives specify column types as letters (T=text, I=integer, R=real). Adding a BOOLEAN column requires using the correct type letter.
**Why it happens:** Boolean columns in sqllogictest use `I` (integer) type because DuckDB's sqllogictest runner maps booleans to integers (0/1).
**How to avoid:** Use `query TTTI` for SHOW DIMS FOR METRIC (3 VARCHAR + 1 BOOLEAN). Verify by checking how existing DuckDB tests handle boolean columns. If `I` doesn't work, try `T` (DuckDB may render as "true"/"false" text).
**Warning signs:** Test failure with "expected N columns but got M" or type mismatch errors.

### Pitfall 2: SHOW SEMANTIC VIEWS created_on is Non-Deterministic

**What goes wrong:** `created_on` is an ISO 8601 timestamp set at define time. sqllogictest expects exact output, but timestamps vary per test run.
**Why it happens:** DuckDB `now()` returns the current transaction time.
**How to avoid:** Use `query` with a column count check but not exact value matching for `created_on`. Options: (a) use `rowsort` and test only the deterministic columns via a wrapping SELECT, (b) use `statement ok` to verify the command succeeds and a separate query to verify column count, or (c) use `LIKE` pattern matching on the timestamp format. The best approach is to wrap in a SELECT that extracts only deterministic columns for exact matching, and use a separate query to verify `created_on IS NOT NULL`.
**Warning signs:** Tests pass locally but fail in CI due to timestamp differences.

### Pitfall 3: empty/NULL Metadata for Views Created Before Phase 39

**What goes wrong:** Semantic views created before Phase 39 have `None` for `created_on`, `database_name`, `schema_name`, and `output_type`.
**Why it happens:** `#[serde(default)]` makes these fields `None` for old JSON.
**How to avoid:** Use `unwrap_or_default()` which maps `None` to `""` (empty string). This is already the pattern used for `source_table` and `data_type` in current code. In sqllogictest, empty strings render as `(empty)`.
**Warning signs:** NULL-related panics or unexpected output for old views.

### Pitfall 4: `kind` Column Value

**What goes wrong:** User confirmed `kind` should be `"SEMANTIC_VIEW"` (with underscore), not `"Semantic View"` or other formats.
**Why it happens:** Snowflake uses uppercase snake_case for kind values.
**How to avoid:** Use the constant string `"SEMANTIC_VIEW"` in list.rs.

### Pitfall 5: flat_vector Index Mismatch After Column Reorder

**What goes wrong:** Runtime crash or corrupted output because `output.flat_vector(N)` indices don't match the new `bind.add_result_column()` order.
**Why it happens:** Column order changed (e.g., `database_name` moved to position 0) but `emit_rows` still uses old indices.
**How to avoid:** When rewriting `emit_rows`, carefully map each `flat_vector(N)` call to the N-th column declared in `bind_output_columns`. Comment the column index for clarity.

## Code Examples

### Example 1: Updated list.rs Row Struct and Bind

```rust
// Source: target state for list.rs (SHOW SEMANTIC VIEWS)
pub struct ListBindData {
    rows: Vec<ListRow>,
}

struct ListRow {
    created_on: String,
    name: String,
    kind: String,       // constant "SEMANTIC_VIEW"
    database_name: String,
    schema_name: String,
}

fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
    bind.add_result_column("created_on", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("kind", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("database_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("schema_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));

    let state_ptr = bind.get_extra_info::<CatalogState>();
    let guard = unsafe { (*state_ptr).read().expect("catalog RwLock poisoned") };

    let mut rows = Vec::with_capacity(guard.len());
    for (name, json) in guard.iter() {
        let def = SemanticViewDefinition::from_json(name, json).ok();
        let (created_on, database_name, schema_name) = match &def {
            Some(d) => (
                d.created_on.clone().unwrap_or_default(),
                d.database_name.clone().unwrap_or_default(),
                d.schema_name.clone().unwrap_or_default(),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        rows.push(ListRow {
            created_on,
            name: name.clone(),
            kind: "SEMANTIC_VIEW".to_string(),
            database_name,
            schema_name,
        });
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ListBindData { rows })
}
```

### Example 2: Boolean Column for SHOW DIMS FOR METRIC

```rust
// Source: pattern from duckdb-1.10500.0/src/vtab/excel.rs + target state
// In bind:
bind.add_result_column("required", LogicalTypeHandle::from(LogicalTypeId::Boolean));

// In func:
let mut required_vec = output.flat_vector(3);
let required_slice = required_vec.as_mut_slice::<bool>();
for i in 0..n {
    required_slice[i] = false;
}
```

### Example 3: Updated Emit Pattern for 6-Column Schema

```rust
// Source: target state for show_dims.rs emit_rows
fn emit_rows(...) {
    // ...
    let db_vec = output.flat_vector(0);      // database_name
    let schema_vec = output.flat_vector(1);   // schema_name
    let sv_vec = output.flat_vector(2);       // semantic_view_name
    let table_vec = output.flat_vector(3);    // table_name (was source_table)
    let name_vec = output.flat_vector(4);     // name
    let type_vec = output.flat_vector(5);     // data_type

    for (i, row) in bind_data.rows.iter().enumerate() {
        db_vec.insert(i, row.database_name.as_str());
        schema_vec.insert(i, row.schema_name.as_str());
        sv_vec.insert(i, row.semantic_view_name.as_str());
        table_vec.insert(i, row.table_name.as_str());
        name_vec.insert(i, row.name.as_str());
        type_vec.insert(i, row.data_type.as_str());
    }
}
```

## Files That Must Change

| File | Current State | Target State | Change Type |
|------|--------------|--------------|-------------|
| `src/ddl/list.rs` | 2 cols (name, base_table) | 5 cols (created_on, name, kind, database_name, schema_name) | Major rewrite of struct + bind + emit |
| `src/ddl/show_dims.rs` | 5 cols (sv_name, name, expr, source_table, data_type) | 6 cols (db_name, schema_name, sv_name, table_name, name, data_type) | Drop expr, rename source_table, add 2 cols |
| `src/ddl/show_metrics.rs` | 5 cols (sv_name, name, expr, source_table, data_type) | 6 cols (db_name, schema_name, sv_name, table_name, name, data_type) | Same as show_dims.rs |
| `src/ddl/show_facts.rs` | 4 cols (sv_name, name, expr, source_table) | 6 cols (db_name, schema_name, sv_name, table_name, name, data_type) | Drop expr, rename source_table, add 3 cols |
| `src/ddl/show_dims_for_metric.rs` | 5 cols (sv_name, name, expr, source_table, data_type) | 4 cols (table_name, name, data_type, required) | Major schema change, drop 2 cols, add BOOLEAN |
| `test/sql/phase34_1_show_commands.test` | Tests current 5/4-col schemas | Update to new 6-col schemas | Update query types + expected values |
| `test/sql/phase34_1_show_dims_for_metric.test` | Tests current 5-col schema | Update to 4-col schema with BOOLEAN | Update query types + expected values |
| `test/sql/phase34_1_1_show_filtering.test` | Tests current schemas with LIKE/STARTS/LIMIT | Update to new schemas | Update query types + expected values |

### Files That Do NOT Change

| File | Why Not |
|------|---------|
| `src/parse.rs` | `build_filter_suffix` generates `WHERE name ILIKE/LIKE ...`; `name` column still exists in all new schemas |
| `src/model.rs` | All metadata fields already added in Phase 39 |
| `src/lib.rs` | VTab registration unchanged; function names unchanged |
| `src/ddl/define.rs` | Metadata capture already done in Phase 39 |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest (Rust runner) + cargo test |
| Config file | `test/sql/TEST_LIST` |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SHOW-01 | SHOW VIEWS returns 5 cols | integration (sqllogictest) | `just test-sql` | Needs update: `test/sql/phase34_1_1_show_filtering.test` |
| SHOW-02 | SHOW DIMS returns 6 cols | integration (sqllogictest) | `just test-sql` | Needs update: `test/sql/phase34_1_show_commands.test` |
| SHOW-03 | SHOW METRICS returns 6 cols | integration (sqllogictest) | `just test-sql` | Needs update: `test/sql/phase34_1_show_commands.test` |
| SHOW-04 | SHOW FACTS returns 6 cols | integration (sqllogictest) | `just test-sql` | Needs update: `test/sql/phase34_1_show_commands.test` |
| SHOW-05 | SHOW DIMS FOR METRIC returns 4 cols | integration (sqllogictest) | `just test-sql` | Needs update: `test/sql/phase34_1_show_dims_for_metric.test` |
| SHOW-06 | No expr column | integration (sqllogictest) | `just test-sql` | Verified by absence in updated tests |
| SHOW-07 | table_name not source_table | integration (sqllogictest) | `just test-sql` | Verified by column names in updated tests |
| SHOW-08 | Filtering works | integration (sqllogictest) | `just test-sql` | Needs update: `test/sql/phase34_1_1_show_filtering.test` |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Update `test/sql/phase34_1_show_commands.test` -- update query type strings and expected output for new 6-col schemas
- [ ] Update `test/sql/phase34_1_show_dims_for_metric.test` -- update query type strings and expected output for new 4-col schema with BOOLEAN
- [ ] Update `test/sql/phase34_1_1_show_filtering.test` -- update query type strings and expected output for all new schemas
- [ ] Consider adding a dedicated `test/sql/phase40_show_alignment.test` for testing `created_on` non-NULL and `kind` value

## sqllogictest Update Details

### Column Type Strings

Current -> Target:
- SHOW DIMS: `TTTTT` -> `TTTTTT` (6 VARCHAR cols)
- SHOW METRICS: `TTTTT` -> `TTTTTT` (6 VARCHAR cols)
- SHOW FACTS: `TTTT` -> `TTTTTT` (6 VARCHAR cols)
- SHOW DIMS FOR METRIC: `TTTTT` -> `TTTI` (3 VARCHAR + 1 BOOLEAN)
- SHOW VIEWS: `TT` -> `TTTTT` (5 VARCHAR cols)

### Expected Output Value Changes

For SHOW DIMS/METRICS/FACTS:
- New `database_name` column: will be whatever `current_database()` returns at define time (typically `memory` for in-memory sqllogictest)
- New `schema_name` column: will be whatever `current_schema()` returns at define time (typically `main`)
- `expr` column: REMOVED (no longer in output)
- `source_table` -> `table_name`: same values, just renamed column

For SHOW VIEWS:
- `created_on`: non-deterministic timestamp; test with `NOT NULL` check or pattern match
- `kind`: constant `SEMANTIC_VIEW`
- `base_table`: REMOVED

For SHOW DIMS FOR METRIC:
- `semantic_view_name`: REMOVED
- `expr`: REMOVED
- `source_table` -> `table_name`: same values, just renamed
- `required`: constant `false` (boolean)

### Strategy for Non-Deterministic created_on in sqllogictest

The `created_on` column returns a timestamp like `2026-04-01T12:34:56Z`. This cannot be tested with exact value matching. Options:

1. **Best approach:** Use a new dedicated test file (`phase40_show_alignment.test`) that verifies `created_on IS NOT NULL` via a wrapping SELECT. For the existing test files, update the column type strings but test via `query TTTTT rowsort` which sorts lexicographically -- the exact timestamp value will vary but column count and other values are deterministic. Use `skipif` or a wrapping `SELECT` to avoid brittle timestamp matching.

2. **Practical approach for updating existing tests:** Since sqllogictest expects exact output, wrap SHOW VIEWS calls in `SELECT name, kind, database_name, schema_name FROM (SHOW SEMANTIC VIEWS)` to skip the `created_on` column for exact matching, or replace exact row expectations with `query TTTTT` and accept that the test runner may need adjustments.

3. **Simplest approach:** Since all SHOW VIEWS test cases currently test filtering (LIKE/STARTS WITH/LIMIT), and filtering operates on `name`, the `created_on` column is just a pass-through. In existing tests, we can use `statement ok` for basic smoke tests and add targeted queries that check `created_on IS NOT NULL` separately.

**Recommended:** Use approach (2) -- wrap in a subselect that excludes `created_on` for exact matching tests, and add a separate `phase40` test that checks `created_on IS NOT NULL`.

## Sources

### Primary (HIGH confidence)
- Source code analysis: `src/ddl/list.rs`, `src/ddl/show_dims.rs`, `src/ddl/show_metrics.rs`, `src/ddl/show_facts.rs`, `src/ddl/show_dims_for_metric.rs` -- current column schemas and VTab patterns
- Source code analysis: `src/model.rs` -- SemanticViewDefinition fields including Phase 39 additions (created_on, database_name, schema_name, Fact.output_type)
- Source code analysis: `src/parse.rs` -- `build_filter_suffix` uses `name` column for all LIKE/STARTS WITH filtering
- DuckDB Rust crate (v1.10500.0): `core/logical_type.rs` confirms `LogicalTypeId::Boolean` exists; `core/vector.rs` confirms no `Inserter<bool>` impl; `vtab/excel.rs` demonstrates `as_mut_slice::<bool>()` pattern
- sqllogictest files: `phase34_1_show_commands.test`, `phase34_1_show_dims_for_metric.test`, `phase34_1_1_show_filtering.test` -- current test expectations

### Secondary (MEDIUM confidence)
- Phase 39 research (`39-RESEARCH.md`) -- metadata capture patterns confirmed implemented

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, purely internal VTab changes
- Architecture: HIGH -- pattern is well-established in existing VTab files, just changing column schemas
- Pitfalls: HIGH -- identified from direct source code analysis and DuckDB crate inspection

**Research date:** 2026-04-01
**Valid until:** 2026-05-01 (stable -- internal codebase changes only)
