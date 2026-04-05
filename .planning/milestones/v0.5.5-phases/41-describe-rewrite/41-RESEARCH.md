# Phase 41: DESCRIBE Rewrite - Research

**Researched:** 2026-04-01
**Domain:** VTab rewrite, Snowflake DESCRIBE SEMANTIC VIEW alignment
**Confidence:** HIGH

## Summary

Phase 41 replaces the current single-row JSON-blob DESCRIBE output (6 columns: name, base_table, dimensions, metrics, joins, facts) with a Snowflake-aligned property-per-row format (5 columns: object_kind, object_name, parent_entity, property, property_value). This is a complete rewrite of `src/ddl/describe.rs` -- the DescribeBindData struct changes from storing 6 strings to storing a `Vec<DescribeRow>` where each row represents one property of one object.

The Snowflake documentation has been verified and provides a clear, unambiguous specification for the output format. The key implementation challenge is: (1) building the row collection logic that iterates over tables, relationships, dimensions, facts, metrics, and derived metrics to produce the correct property rows, and (2) updating 8+ sqllogictest assertions across 7 test files that currently expect the old 6-column format.

**Primary recommendation:** Implement as a complete `describe.rs` rewrite following the existing SHOW VTab pattern (Vec of row structs populated at bind time, emitted in func), with a new dedicated sqllogictest file for comprehensive coverage. Update all existing test files atomically.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DESC-01 | DESCRIBE SEMANTIC VIEW outputs property-per-row format with 5 columns: object_kind, object_name, parent_entity, property, property_value | Snowflake docs verified: exactly these 5 columns. See Architecture section for DescribeRow struct design. |
| DESC-02 | TABLE objects emit BASE_TABLE_NAME, PRIMARY_KEY, BASE_TABLE_DATABASE_NAME, BASE_TABLE_SCHEMA_NAME properties | Snowflake docs verified: TABLE parent_entity is empty string (NULL in Snowflake). PRIMARY_KEY format is JSON array: `["col1","col2"]`. See Code Examples section. |
| DESC-03 | RELATIONSHIP objects emit TABLE, REF_TABLE, FOREIGN_KEY, REF_KEY properties | Snowflake docs verified: parent_entity = from-table alias. FOREIGN_KEY and REF_KEY are JSON arrays: `["col"]`. TABLE property value = from-table alias. |
| DESC-04 | DIMENSION objects emit TABLE, EXPRESSION, DATA_TYPE properties | Snowflake docs verified: parent_entity = table alias. TABLE property value = table alias. |
| DESC-05 | FACT objects emit TABLE, EXPRESSION, DATA_TYPE properties | Same as DESC-04. Facts use source_table alias for parent_entity and TABLE. |
| DESC-06 | METRIC objects emit TABLE, EXPRESSION, DATA_TYPE properties | Same as DESC-04. Metrics with source_table = Some(...) are METRIC. |
| DESC-07 | DERIVED_METRIC objects emit EXPRESSION, DATA_TYPE properties (no TABLE) | Snowflake docs verified: parent_entity is empty (NULL in Snowflake). No TABLE property. Metrics with source_table = None are DERIVED_METRIC. |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- Quality gate: `just test-all` must pass (Rust unit tests + sqllogictest + DuckLake CI)
- `cargo test` alone is incomplete -- sqllogictest covers integration paths
- `just test-sql` requires `just build` first
- When in doubt about SQL syntax or behavior, refer to what Snowflake semantic views does

## Snowflake DESCRIBE SEMANTIC VIEW Specification

**Source:** [Snowflake DESCRIBE SEMANTIC VIEW docs](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) (fetched 2026-04-01)
**Confidence:** HIGH -- verified via multiple reads of official documentation

### Output Schema

5 columns, all VARCHAR:

| Column | Description |
|--------|-------------|
| `object_kind` | Type of object: TABLE, RELATIONSHIP, DIMENSION, FACT, METRIC, DERIVED_METRIC |
| `object_name` | Name of the object (table alias, dim/metric/fact name, relationship name) |
| `parent_entity` | Parent table alias for dims/facts/metrics/relationships; empty for TABLE and DERIVED_METRIC |
| `property` | Property name (e.g., BASE_TABLE_NAME, EXPRESSION, DATA_TYPE) |
| `property_value` | Property value as string |

### Object Kinds and Their Properties

**TABLE** (one set of rows per table alias):
- `parent_entity`: empty string (NULL in Snowflake, we use empty string since all columns are VARCHAR)
- `object_name`: table alias (e.g., "o", "c")
- Properties:
  - `BASE_TABLE_DATABASE_NAME` -- database_name from SemanticViewDefinition
  - `BASE_TABLE_SCHEMA_NAME` -- schema_name from SemanticViewDefinition
  - `BASE_TABLE_NAME` -- the physical table name (TableRef.table)
  - `PRIMARY_KEY` -- JSON array format: `["col1","col2"]` (no spaces after commas)

**RELATIONSHIP** (one set per Join with a name):
- `parent_entity`: from_alias (the source table alias in the FK declaration)
- `object_name`: relationship name (Join.name)
- Properties:
  - `TABLE` -- from_alias (same as parent_entity)
  - `REF_TABLE` -- target table alias (Join.table)
  - `FOREIGN_KEY` -- JSON array of FK columns: `["customer_id"]`
  - `REF_KEY` -- JSON array of referenced columns: `["id"]`

**DIMENSION** (one set per dimension):
- `parent_entity`: source_table alias (or base table alias if source_table is None)
- `object_name`: dimension name
- Properties:
  - `TABLE` -- source_table alias (same as parent_entity)
  - `EXPRESSION` -- the expr string
  - `DATA_TYPE` -- output_type if available, else empty string

**FACT** (one set per fact):
- `parent_entity`: source_table alias (or base table alias if source_table is None)
- `object_name`: fact name
- Properties:
  - `TABLE` -- source_table alias (same as parent_entity)
  - `EXPRESSION` -- the expr string
  - `DATA_TYPE` -- output_type if available, else empty string

**METRIC** (one set per metric with source_table = Some):
- `parent_entity`: source_table alias
- `object_name`: metric name
- Properties:
  - `TABLE` -- source_table alias (same as parent_entity)
  - `EXPRESSION` -- the expr string
  - `DATA_TYPE` -- output_type if available, else empty string

**DERIVED_METRIC** (one set per metric with source_table = None):
- `parent_entity`: empty string (NULL in Snowflake)
- `object_name`: metric name
- Properties:
  - `EXPRESSION` -- the expr string
  - `DATA_TYPE` -- output_type if available, else empty string
  - No TABLE property (explicitly documented by Snowflake)

### Key Format Details Verified Against Snowflake Docs

| Detail | Snowflake Format | Our Adaptation |
|--------|-----------------|----------------|
| PRIMARY_KEY | `["C_CUSTKEY"]`, `["L_ORDERKEY","L_LINENUMBER"]` | Same JSON array, no spaces after commas |
| FOREIGN_KEY | `["L_ORDERKEY"]` | Same JSON array format |
| REF_KEY | `["O_ORDERKEY"]` | Same JSON array format |
| DATA_TYPE | `VARCHAR(25)`, `NUMBER(25,4)`, `DATE` | Use output_type as-is; empty string when None |
| EXPRESSION | `customers.c_name`, `SUM(orders.o_totalprice)` | Use expr as stored in model |
| parent_entity for TABLE | NULL | Empty string (VARCHAR column) |
| parent_entity for DERIVED_METRIC | NULL | Empty string (VARCHAR column) |

### Properties We Intentionally Omit (Out of Scope per REQUIREMENTS.md)

- `COMMENT` -- no DDL support yet (DDL-COMMENT deferred)
- `SYNONYMS` -- no DDL support yet (DDL-SYNONYM deferred)
- `ACCESS_MODIFIER` -- DuckDB has no access control
- `CUSTOM_INSTRUCTIONS` -- Snowflake Cortex AI specific
- `CONSTRAINT` (DISTINCT_RANGE) -- Snowflake specific
- Cortex Search Service properties -- Snowflake specific

## Architecture Patterns

### Current describe.rs Structure (to be replaced)

```rust
// Current: single row with 6 VARCHAR columns
pub struct DescribeBindData {
    name: String,
    base_table: String,
    dimensions: String,  // JSON blob
    metrics: String,     // JSON blob
    joins: String,       // JSON blob
    facts: String,       // JSON blob
}
```

### New describe.rs Structure

Follow the pattern established by `show_dims.rs`, `list.rs`, and other SHOW VTabs:

```rust
/// A single property row in the DESCRIBE output.
struct DescribeRow {
    object_kind: String,
    object_name: String,
    parent_entity: String,
    property: String,
    property_value: String,
}

/// Bind-time data: pre-collected property rows.
pub struct DescribeBindData {
    rows: Vec<DescribeRow>,
}

// SAFETY: all fields are owned String, Send + Sync.
unsafe impl Send for DescribeBindData {}
unsafe impl Sync for DescribeBindData {}

/// Init data: tracks whether rows have been emitted.
pub struct DescribeInitData {
    done: AtomicBool,
}

unsafe impl Send for DescribeInitData {}
unsafe impl Sync for DescribeInitData {}
```

### Row Collection Logic (bind function)

The bind function must:

1. Read the view name from parameter 0
2. Look up the JSON in CatalogState
3. Parse into SemanticViewDefinition via `from_json`
4. Iterate over definition components in order:
   - For each `tables` entry: emit BASE_TABLE_DATABASE_NAME, BASE_TABLE_SCHEMA_NAME, BASE_TABLE_NAME, PRIMARY_KEY
   - For each `joins` entry (with a name): emit TABLE, REF_TABLE, FOREIGN_KEY, REF_KEY
   - For each `facts` entry: emit TABLE, EXPRESSION, DATA_TYPE
   - For each `dimensions` entry: emit TABLE, EXPRESSION, DATA_TYPE
   - For each `metrics` entry with source_table Some: emit TABLE, EXPRESSION, DATA_TYPE (object_kind = METRIC)
   - For each `metrics` entry with source_table None: emit EXPRESSION, DATA_TYPE (object_kind = DERIVED_METRIC)
5. Return DescribeBindData with collected rows

### Emit Pattern (func function)

Same as list.rs/show_dims.rs:

```rust
fn func(func: &TableFunctionInfo<Self>, output: &mut DataChunkHandle) -> Result<...> {
    let init_data = func.get_init_data();
    if init_data.done.swap(true, Ordering::Relaxed) {
        output.set_len(0);
        return Ok(());
    }
    let bind_data = func.get_bind_data();
    let n = bind_data.rows.len();
    // Get flat vectors for each of 5 columns
    // Insert each row
    output.set_len(n);
    Ok(())
}
```

### Output Column Declaration

```rust
fn bind_output_columns(bind: &BindInfo) {
    bind.add_result_column("object_kind", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("object_name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("parent_entity", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("property", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("property_value", LogicalTypeHandle::from(LogicalTypeId::Varchar));
}
```

### C++ Shim Compatibility

No C++ changes needed. The `sv_ddl_bind` function in `cpp/src/shim.cpp` dynamically reads `duckdb_column_count(&result)` and `duckdb_column_name(&result, c)` from the VTab result. Changing from 6 columns to 5 columns flows through automatically.

### Base Table Alias Fallback

For dimensions/facts/metrics with `source_table: None`, we need a fallback for parent_entity and TABLE property. The base table alias is `tables[0].alias` if tables is non-empty, otherwise use the base_table name itself. This handles old stored definitions that may not have the tables Vec populated.

```rust
let base_alias = def.tables.first()
    .map(|t| t.alias.clone())
    .unwrap_or_else(|| def.base_table.clone());
```

### PRIMARY_KEY JSON Array Formatting

```rust
fn format_pk_array(columns: &[String]) -> String {
    let items: Vec<String> = columns.iter()
        .map(|c| format!("\"{}\"", c))
        .collect();
    format!("[{}]", items.join(","))
}
// ["id"] or ["first_name","last_name"] -- no spaces after commas
```

Same formatting for FOREIGN_KEY and REF_KEY.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON array formatting | Custom serializer | Simple `format!` with `join(",")` | Only need `["col1","col2"]` format, serde_json would add spaces |
| VTab multi-row emission | Custom chunking | AtomicBool done flag pattern | Established pattern in all SHOW VTabs |
| Definition parsing | Manual JSON field access | `SemanticViewDefinition::from_json` | Already handles all backward compat |

## Common Pitfalls

### Pitfall 1: Column Count Mismatch in Existing Tests
**What goes wrong:** Existing sqllogictest files assert `query TTTTTT` (6 columns) for DESCRIBE. The new format has 5 columns and multiple rows.
**Why it happens:** Every test file with a DESCRIBE assertion will fail if not updated atomically.
**How to avoid:** Update ALL test files in the same commit as the describe.rs rewrite. The affected files are:
- `test/sql/phase20_extended_ddl.test` -- 5 DESCRIBE assertions (lines 199-208, 297-300, 322-325) + 2 error assertions
- `test/sql/phase21_error_reporting.test` -- 1 assertion (line 97-100)
- `test/sql/phase25_keyword_body.test` -- 1 assertion (line 84-87)
- `test/sql/phase28_e2e.test` -- 1 assertion (line 176-179)
- `test/sql/phase29_facts.test` -- 1 assertion (line 108-111)
- `test/sql/phase30_derived_metrics.test` -- 1 assertion (line 198-201)
- `test/sql/phase33_cardinality_inference.test` -- 2 `SELECT COUNT(*) FROM describe_semantic_view(...)` assertions (lines 420, 675)
- `test/sql/phase34_1_alter_rename.test` -- 1 error assertion (line 43-45, this one stays the same since it just checks "does not exist")
**Warning signs:** Any `query TTTTTT` or `query T` assertion near `DESCRIBE SEMANTIC VIEW` is affected.

### Pitfall 2: Derived Metric vs Metric Classification
**What goes wrong:** A metric with `source_table: None` might be incorrectly classified as METRIC instead of DERIVED_METRIC.
**Why it happens:** The distinction depends solely on whether `source_table` is `Some(...)` or `None`.
**How to avoid:** Clear conditional: `if metric.source_table.is_some() { "METRIC" } else { "DERIVED_METRIC" }`.
**Warning signs:** Derived metrics appearing with a TABLE property row.

### Pitfall 3: Empty Primary Key
**What goes wrong:** Tables without PRIMARY KEY (optional since quick task 260322-1zx) would emit `PRIMARY_KEY: []`.
**How to avoid:** Only emit the PRIMARY_KEY property row when `pk_columns` is non-empty. Snowflake always requires PK, but our extension made it optional.

### Pitfall 4: Unnamed Relationships
**What goes wrong:** Old stored JSON with `Join.name: None` has no relationship name for `object_name`.
**Why it happens:** Before Phase 24, relationships might not have explicit names.
**How to avoid:** Skip RELATIONSHIP rows for joins without names, or generate a synthetic name. Recommendation: skip unnamed joins (they are legacy pre-Phase-24 data).

### Pitfall 5: NULL vs Empty String in VTab Output
**What goes wrong:** Snowflake uses NULL for parent_entity on TABLEs and DERIVED_METRICs. DuckDB VTabs emit VARCHAR columns.
**Why it happens:** The existing VTab pattern uses empty strings for missing values (see show_dims.rs `unwrap_or_default()`).
**How to avoid:** Use empty string consistently, matching the established pattern. The C++ shim reads values via `duckdb_value_varchar` which handles both. In sqllogictest, empty string renders as empty (no special NULL handling needed for VARCHAR columns).

### Pitfall 6: Row Count Changes Break COUNT(*) Tests
**What goes wrong:** `phase33_cardinality_inference.test` asserts `SELECT COUNT(*) FROM describe_semantic_view('view_name')` expecting 1 row. New format returns many rows.
**Why it happens:** Old format was always exactly 1 row per view.
**How to avoid:** Update the COUNT assertions to match the expected number of property rows for each test view.

### Pitfall 7: DATA_TYPE Is Often Empty
**What goes wrong:** Tests expect a DATA_TYPE value but the model stores `output_type: None` for most dims/metrics.
**Why it happens:** Define-time type inference (Phase 39) is best-effort. Many dims/metrics have `output_type: None`.
**How to avoid:** Emit empty string for DATA_TYPE when `output_type` is None. This matches the SHOW commands behavior established in Phase 40.

## Code Examples

### Row Collection for TABLE Objects

```rust
// Source: project model.rs TableRef struct + Snowflake DESCRIBE docs
fn collect_table_rows(
    def: &SemanticViewDefinition,
    rows: &mut Vec<DescribeRow>,
) {
    let db_name = def.database_name.clone().unwrap_or_default();
    let sch_name = def.schema_name.clone().unwrap_or_default();

    for table in &def.tables {
        let obj_name = table.alias.clone();

        rows.push(DescribeRow {
            object_kind: "TABLE".to_string(),
            object_name: obj_name.clone(),
            parent_entity: String::new(),
            property: "BASE_TABLE_DATABASE_NAME".to_string(),
            property_value: db_name.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "TABLE".to_string(),
            object_name: obj_name.clone(),
            parent_entity: String::new(),
            property: "BASE_TABLE_SCHEMA_NAME".to_string(),
            property_value: sch_name.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "TABLE".to_string(),
            object_name: obj_name.clone(),
            parent_entity: String::new(),
            property: "BASE_TABLE_NAME".to_string(),
            property_value: table.table.clone(),
        });
        if !table.pk_columns.is_empty() {
            rows.push(DescribeRow {
                object_kind: "TABLE".to_string(),
                object_name: obj_name.clone(),
                parent_entity: String::new(),
                property: "PRIMARY_KEY".to_string(),
                property_value: format_json_array(&table.pk_columns),
            });
        }
    }
}
```

### Row Collection for RELATIONSHIP Objects

```rust
// Source: project model.rs Join struct + Snowflake DESCRIBE docs
fn collect_relationship_rows(
    def: &SemanticViewDefinition,
    rows: &mut Vec<DescribeRow>,
) {
    for join in &def.joins {
        let rel_name = match &join.name {
            Some(n) => n.clone(),
            None => continue, // skip unnamed legacy joins
        };
        let from_alias = join.from_alias.clone();

        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name.clone(),
            parent_entity: from_alias.clone(),
            property: "TABLE".to_string(),
            property_value: from_alias.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name.clone(),
            parent_entity: from_alias.clone(),
            property: "REF_TABLE".to_string(),
            property_value: join.table.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name.clone(),
            parent_entity: from_alias.clone(),
            property: "FOREIGN_KEY".to_string(),
            property_value: format_json_array(&join.fk_columns),
        });
        rows.push(DescribeRow {
            object_kind: "RELATIONSHIP".to_string(),
            object_name: rel_name.clone(),
            parent_entity: from_alias.clone(),
            property: "REF_KEY".to_string(),
            property_value: format_json_array(&join.ref_columns),
        });
    }
}
```

### JSON Array Formatting Helper

```rust
/// Format column names as a JSON array: ["col1","col2"]
/// Matches Snowflake format: no spaces after commas.
fn format_json_array(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter()
        .map(|s| format!("\"{}\"", s))
        .collect();
    format!("[{}]", quoted.join(","))
}
```

### Metric vs Derived Metric Classification

```rust
// Source: Snowflake docs + project model.rs Metric struct
fn collect_metric_rows(
    def: &SemanticViewDefinition,
    base_alias: &str,
    rows: &mut Vec<DescribeRow>,
) {
    for metric in &def.metrics {
        let is_derived = metric.source_table.is_none();
        let object_kind = if is_derived { "DERIVED_METRIC" } else { "METRIC" };
        let parent = if is_derived {
            String::new()
        } else {
            metric.source_table.clone().unwrap_or_else(|| base_alias.to_string())
        };

        if !is_derived {
            rows.push(DescribeRow {
                object_kind: object_kind.to_string(),
                object_name: metric.name.clone(),
                parent_entity: parent.clone(),
                property: "TABLE".to_string(),
                property_value: parent.clone(),
            });
        }
        rows.push(DescribeRow {
            object_kind: object_kind.to_string(),
            object_name: metric.name.clone(),
            parent_entity: parent.clone(),
            property: "EXPRESSION".to_string(),
            property_value: metric.expr.clone(),
        });
        rows.push(DescribeRow {
            object_kind: object_kind.to_string(),
            object_name: metric.name.clone(),
            parent_entity: parent.clone(),
            property: "DATA_TYPE".to_string(),
            property_value: metric.output_type.clone().unwrap_or_default(),
        });
    }
}
```

## Affected Test Files Inventory

Files requiring DESCRIBE assertion updates:

| File | Assertions | Type of Change |
|------|------------|----------------|
| `test/sql/phase20_extended_ddl.test` | 5 `query TTTTTT` + 2 `statement error` | Rewrite to `query TTTTT` multi-row; errors stay as-is |
| `test/sql/phase21_error_reporting.test` | 1 `query TTTTTT` | Rewrite to `query TTTTT` multi-row |
| `test/sql/phase25_keyword_body.test` | 1 `query TTTTTT` | Rewrite to `query TTTTT` multi-row |
| `test/sql/phase28_e2e.test` | 1 `query TTTTTT` | Rewrite to `query TTTTT` multi-row |
| `test/sql/phase29_facts.test` | 1 `query TTTTTT` | Rewrite to `query TTTTT` multi-row |
| `test/sql/phase30_derived_metrics.test` | 1 `query TTTTTT` | Rewrite to `query TTTTT` multi-row |
| `test/sql/phase33_cardinality_inference.test` | 2 `COUNT(*)` | Update expected count values |
| `test/sql/phase34_1_alter_rename.test` | 1 `statement error` | No change needed (error message unchanged) |
| `src/query/error.rs` | 1 inline help message | Update reference from "FROM describe_semantic_view" to match new format guidance |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest (Python runner) + cargo test |
| Config file | test/sql/*.test + Cargo.toml |
| Quick run command | `cargo test -- describe` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DESC-01 | 5-column property-per-row output | sqllogictest | `just test-sql` | Wave 0: new test/sql/phase41_describe.test |
| DESC-02 | TABLE object properties | sqllogictest | `just test-sql` | Wave 0: new test |
| DESC-03 | RELATIONSHIP object properties | sqllogictest | `just test-sql` | Wave 0: new test |
| DESC-04 | DIMENSION object properties | sqllogictest | `just test-sql` | Wave 0: new test |
| DESC-05 | FACT object properties | sqllogictest | `just test-sql` | Wave 0: new test |
| DESC-06 | METRIC object properties | sqllogictest | `just test-sql` | Wave 0: new test |
| DESC-07 | DERIVED_METRIC object properties | sqllogictest | `just test-sql` | Wave 0: new test |

### Sampling Rate
- **Per task commit:** `cargo test -- describe`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase41_describe.test` -- comprehensive DESCRIBE coverage for all object kinds
- [ ] Unit tests in describe.rs for format_json_array helper

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Single-row JSON blob DESCRIBE | Property-per-row DESCRIBE | Phase 41 (this phase) | Breaking change to DESCRIBE output format |
| 6 VARCHAR columns | 5 VARCHAR columns | Phase 41 | All downstream consumers affected |

## Open Questions

1. **Row ordering within DESCRIBE output**
   - What we know: Snowflake docs do not specify ordering. Our SHOW commands sort by name.
   - What's unclear: Should we sort by (object_kind, object_name, property)? Or emit in definition order (tables first, then relationships, then facts, then dims, then metrics)?
   - Recommendation: Emit in definition order (tables, relationships, facts, dimensions, metrics) to match the natural reading order of a semantic view definition. Within each kind, preserve the order from the DDL. Tests should use `rowsort` only if needed, or assert exact order.

2. **Base table fallback for old definitions without tables Vec**
   - What we know: Pre-Phase-24 definitions may have `tables: []` with only `base_table` set.
   - What's unclear: Should these emit TABLE rows at all?
   - Recommendation: If `tables` is empty, emit a single TABLE block using `base_table` as both alias and table name, with empty pk_columns (no PRIMARY_KEY row). This handles legacy definitions gracefully.

3. **Relationship from_alias vs table for legacy joins**
   - What we know: Old joins used `from_cols` and `on` fields. Modern joins use `from_alias` + `fk_columns`.
   - What's unclear: How to handle joins with `from_alias: ""` (empty).
   - Recommendation: Skip RELATIONSHIP rows for joins without both a name and a from_alias. These are legacy data.

## Sources

### Primary (HIGH confidence)
- [Snowflake DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- complete output schema, all object kinds, all property names, example output
- [Snowflake YAML specification](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- derived metric definition details
- Project source code: `src/ddl/describe.rs` (current implementation), `src/model.rs` (data structures), `src/ddl/show_dims.rs` (reference VTab pattern)

### Secondary (MEDIUM confidence)
- [Snowflake derived metrics release note](https://docs.snowflake.com/en/release-notes/2025/other/2025-09-30-semantic-view-derived-metrics) -- confirms DERIVED_METRIC is separate from METRIC

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, pure Rust rewrite of existing VTab
- Architecture: HIGH -- follows established VTab pattern from show_dims.rs, list.rs
- Pitfalls: HIGH -- comprehensive inventory of all 7+ affected test files, verified column count changes
- Snowflake alignment: HIGH -- 3 separate reads of official documentation with cross-verification

**Research date:** 2026-04-01
**Valid until:** 2026-05-01 (stable -- Snowflake DESCRIBE format unlikely to change)
