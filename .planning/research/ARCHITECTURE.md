# Architecture: v0.5.5 SHOW/DESCRIBE Alignment & Module Refactoring

**Domain:** DuckDB semantic views extension -- Snowflake-aligned output formats + module directory refactoring
**Researched:** 2026-04-01
**Confidence:** HIGH (direct codebase analysis, Snowflake official docs verified via WebFetch)

## Executive Summary

v0.5.5 has two independent work streams: (1) aligning all 6 SHOW/DESCRIBE output formats with Snowflake's column schemas, and (2) splitting the two largest modules (expand.rs at 4,490 lines, graph.rs at 2,502 lines) into module directories. These streams are architecturally independent -- output format changes touch ddl/ VTab files and the catalog/model layer, while the refactoring touches expand.rs, graph.rs, and their import sites. This independence enables parallel development or strict sequencing (refactoring first) without conflict.

The SHOW/DESCRIBE alignment is a schema-breaking change to 6 VTab implementations. Snowflake's output format differs significantly from the current implementation: DESCRIBE uses a property-per-row format (5 columns: object_kind, object_name, parent_entity, property, property_value) instead of the current single-row-per-view format. SHOW commands need additional columns (database_name, schema_name, synonyms, comment) and must drop the `expr` column that Snowflake does not expose. A `created_on` timestamp must be stored at define time, requiring a catalog schema migration.

The module refactoring is a zero-behavior-change restructuring that breaks two circular dependencies and splits monoliths into single-responsibility files. The key insight is that `suggest_closest` (Levenshtein utility) and `replace_word_boundary` (string substitution) are pure utilities with no semantic dependency on expansion -- extracting them to `util.rs` breaks the `expand <-> graph` cycle cleanly.

## Recommended Architecture

### Work Stream 1: SHOW/DESCRIBE Snowflake Alignment

#### Current vs Target: DESCRIBE SEMANTIC VIEW

**Current** (single row, 6 columns):
```
| name | base_table | dimensions | metrics | joins | facts |
| sales | orders | [{...}] | [{...}] | [{...}] | [{...}] |
```

**Target** (Snowflake property-per-row, 5 columns):
```
| object_kind | object_name | parent_entity | property | property_value |
| NULL | NULL | NULL | COMMENT | ... |
| TABLE | orders | NULL | BASE_TABLE_NAME | orders |
| TABLE | orders | NULL | PRIMARY_KEY | ["order_id"] |
| DIMENSION | region | orders | EXPRESSION | o.region |
| DIMENSION | region | orders | DATA_TYPE | VARCHAR |
| METRIC | revenue | orders | EXPRESSION | SUM(o.amount) |
| METRIC | revenue | orders | DATA_TYPE | DOUBLE |
| RELATIONSHIP | orders_to_customers | orders | FOREIGN_KEY | ["o_custkey"] |
| RELATIONSHIP | orders_to_customers | orders | REF_TABLE | customers |
| RELATIONSHIP | orders_to_customers | orders | REF_KEY | ["c_custkey"] |
| FACT | is_returned | orders | EXPRESSION | o.status = 'returned' |
```

This is the most significant schema change. The current `DescribeSemanticViewVTab` emits 1 row with JSON blob columns. The new format emits N rows (one per property per object), where each semantic view element generates multiple rows (one per property). This better matches Snowflake and is more queryable -- users can `WHERE object_kind = 'DIMENSION'` to filter.

**Implementation approach:** Rebuild `describe.rs` entirely. The `DescribeBindData` struct changes from 6 string fields to a `Vec<DescribeRow>` where each row has the 5 Snowflake columns. The `func()` emitter loops over rows like the SHOW VTabs already do. Parse the stored JSON into `SemanticViewDefinition`, then iterate tables, relationships, dimensions, metrics, facts to generate property rows.

#### Current vs Target: SHOW SEMANTIC VIEWS

**Current** (2 columns):
```
| name | base_table |
```

**Snowflake target** (8 columns):
```
| created_on | name | kind | database_name | schema_name | comment | owner | owner_role_type |
```

**Our target** (subset -- no owner/ACL in DuckDB extensions):
```
| created_on | name | kind | database_name | schema_name | comment |
```

`kind` is always `SEMANTIC_VIEW`. `comment` is currently not stored (always empty string initially, or add COMMENT support later). `database_name` and `schema_name` come from DuckDB context at bind time. `created_on` requires storing a timestamp at define time.

#### Current vs Target: SHOW SEMANTIC DIMENSIONS / METRICS

**Current** (5 columns):
```
| semantic_view_name | name | expr | source_table | data_type |
```

**Snowflake target** (8 columns):
```
| database_name | schema_name | semantic_view_name | table_name | name | data_type | synonyms | comment |
```

Key changes: (a) drop `expr` (Snowflake does not expose expressions in SHOW output), (b) rename `source_table` to `table_name`, (c) add `database_name`, `schema_name`, `synonyms`, `comment` columns, (d) reorder columns to match Snowflake.

#### Current vs Target: SHOW SEMANTIC FACTS

Same pattern as dimensions/metrics but facts have no `data_type` in current output. Snowflake SHOW SEMANTIC FACTS likely follows the same 8-column schema as dimensions/metrics (with data_type for facts).

#### Current vs Target: SHOW SEMANTIC DIMENSIONS FOR METRIC

**Current** (5 columns):
```
| semantic_view_name | name | expr | source_table | data_type |
```

**Snowflake target** (6 columns):
```
| table_name | name | data_type | required | synonyms | comment |
```

Key changes: (a) drop `expr`, `source_table` renamed to `table_name`, (b) add `required` boolean column (whether metric mandates this dimension), (c) add `synonyms`, `comment`, (d) remove `semantic_view_name` (single-view-only command).

### Work Stream 2: Module Directory Refactoring

#### Target Module Structure

```
src/
  expand/
    mod.rs          - pub fn expand(), QueryRequest, ExpandError re-exports
    validate.rs     - request validation, duplicate checks
    resolve.rs      - find_dimension, find_metric resolution
    facts.rs        - toposort_facts, inline_facts, toposort_derived, inline_derived_metrics
    fan_trap.rs     - check_fan_traps, ancestors_to_root
    role_playing.rs - find_using_context, dim_scoped_aliases
    join_resolver.rs - resolve_joins_pkfk, synthesize_on_clause
    sql_gen.rs      - SELECT/FROM/JOIN/WHERE/GROUP BY assembly, quote_ident, quote_table_ref
  graph/
    mod.rs          - RelationshipGraph struct, from_definition, toposort, check_no_diamonds, check_no_orphans
    relationship.rs - validate_graph (orchestrator calling graph methods)
    facts.rs        - validate_facts, find_fact_references
    derived_metrics.rs - validate_derived_metrics, contains_aggregate_function
    using.rs        - validate_using_relationships
  util.rs           - suggest_closest, replace_word_boundary (NEW)
  errors.rs         - ParseError (NEW, extracted from parse.rs)
```

#### Dependency Graph After Refactoring

```
errors.rs     <- parse.rs, body_parser.rs  (breaks parse <-> body_parser cycle)
util.rs       <- expand/*, graph/*, ddl/show_dims_for_metric.rs  (breaks expand <-> graph cycle)
model.rs      <- expand/*, graph/*, catalog.rs, parse.rs, body_parser.rs, ddl/*, query/*
graph/mod.rs  <- expand/fan_trap.rs, expand/join_resolver.rs, ddl/define.rs, ddl/show_dims_for_metric.rs
expand/mod.rs <- query/table_function.rs, query/explain.rs, query/error.rs, ddl/define.rs
catalog.rs    <- ddl/define.rs, ddl/drop.rs, ddl/list.rs, ddl/describe.rs, query/table_function.rs
```

All arrows flow one direction. No circular dependencies.

### Component Boundaries

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| `util.rs` (NEW) | String similarity (Levenshtein), word-boundary replacement | None (leaf module) |
| `errors.rs` (NEW) | `ParseError` struct with byte-offset position | None (leaf module) |
| `expand/mod.rs` | Public API: `expand()`, `QueryRequest`, `ExpandError` | `expand/*` submodules, `graph/mod.rs`, `model.rs` |
| `expand/validate.rs` | Request validation (empty check, duplicate dims/metrics) | `model.rs`, `util.rs` |
| `expand/resolve.rs` | Dimension/metric name resolution | `model.rs`, `util.rs` |
| `expand/facts.rs` | Fact DAG toposort, expression inlining, derived metric resolution | `model.rs`, `util.rs` |
| `expand/fan_trap.rs` | Cardinality-aware fan trap detection + `ancestors_to_root` | `model.rs`, `graph/mod.rs` |
| `expand/role_playing.rs` | USING RELATIONSHIPS + scoped aliases | `model.rs` |
| `expand/join_resolver.rs` | PK/FK join resolution, ON clause synthesis | `model.rs`, `graph/mod.rs` |
| `expand/sql_gen.rs` | SQL string assembly, identifier quoting | `model.rs` |
| `graph/mod.rs` | `RelationshipGraph` struct + construction | `model.rs` |
| `graph/relationship.rs` | Graph validation orchestrator | `graph/mod.rs`, `model.rs`, `util.rs` |
| `graph/facts.rs` | Fact reference detection + validation | `model.rs`, `util.rs` |
| `graph/derived_metrics.rs` | Derived metric validation, aggregate detection | `model.rs`, `util.rs` |
| `graph/using.rs` | USING RELATIONSHIPS validation | `model.rs`, `util.rs` |
| `catalog.rs` | In-memory cache + `_definitions` table persistence | `model.rs` |
| `ddl/describe.rs` | DESCRIBE SEMANTIC VIEW (property-per-row format) | `catalog.rs`, `model.rs` |
| `ddl/list.rs` | SHOW SEMANTIC VIEWS | `catalog.rs`, `model.rs` |
| `ddl/show_dims.rs` | SHOW SEMANTIC DIMENSIONS | `catalog.rs`, `model.rs` |
| `ddl/show_metrics.rs` | SHOW SEMANTIC METRICS | `catalog.rs`, `model.rs` |
| `ddl/show_facts.rs` | SHOW SEMANTIC FACTS | `catalog.rs`, `model.rs` |
| `ddl/show_dims_for_metric.rs` | SHOW SEMANTIC DIMS FOR METRIC | `catalog.rs`, `model.rs`, `graph/mod.rs`, `util.rs` |

### Data Flow

**SHOW/DESCRIBE flow:**
```
DDL SQL -> parse.rs (detect_ddl_prefix) -> rewrite to SELECT * FROM vtab_fn(...)
-> DuckDB calls VTab::bind() -> catalog.read().get(name) -> JSON string
-> SemanticViewDefinition::from_json() -> collect rows from definition
-> VTab::func() -> emit rows into DataChunkHandle
```

**New data needed for SHOW/DESCRIBE alignment:**
```
created_on:     stored in catalog at define time (new field in JSON or new DB column)
database_name:  extracted from DuckDB connection context at bind time
schema_name:    extracted from DuckDB connection context at bind time
comment:        stored in definition (empty string initially; support COMMENT ON later)
synonyms:       stored in definition (empty array initially; support SYNONYMS later)
required:       computed at bind time from metric definition (FOR METRIC command only)
```

## Patterns to Follow

### Pattern 1: Module Directory with Re-exports

**What:** Convert `foo.rs` to `foo/mod.rs` that re-exports the public API, with internal submodules for each responsibility.

**When:** A single file exceeds ~500 lines and contains 3+ distinct responsibilities.

**Example (expand/mod.rs):**
```rust
mod validate;
mod resolve;
mod facts;
mod fan_trap;
mod role_playing;
mod join_resolver;
mod sql_gen;

// Re-export public API -- external callers see no change
pub use self::validate::QueryRequest;
pub use self::resolve::ExpandError;
pub use self::sql_gen::{quote_ident, quote_table_ref};

// The main expand function orchestrates submodules
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    request: &QueryRequest,
) -> Result<String, ExpandError> {
    validate::check_request(request, view_name)?;
    let dims = resolve::resolve_dimensions(request, def, view_name)?;
    let mets = resolve::resolve_metrics(request, def, view_name)?;
    // ... orchestrate pipeline steps
}

// Re-export internal items needed by ddl/show_dims_for_metric.rs
pub(crate) use self::fan_trap::ancestors_to_root;
pub(crate) use self::facts::collect_derived_metric_source_tables;
```

**Why:** External callers (`query/table_function.rs`, `ddl/define.rs`) see the same `crate::expand::expand`, `crate::expand::QueryRequest` paths. Zero breaking changes to import sites. Internal complexity is hidden.

### Pattern 2: Leaf Utility Module

**What:** Extract pure functions with no domain dependencies into a shared utility module.

**When:** A function is imported across module boundaries and creates a circular dependency.

**Example (util.rs):**
```rust
/// Suggest the closest matching name using Levenshtein distance.
/// Returns Some(name) if edit distance <= 3.
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    // ... (moved from expand.rs, unchanged)
}

/// Replace all word-boundary-delimited occurrences of needle with replacement.
pub(crate) fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String {
    // ... (moved from expand.rs, unchanged)
}
```

**Why:** Breaks `expand <-> graph` circular dependency. Both modules import from `util` instead of each other. The dependency graph becomes a clean DAG.

### Pattern 3: Property-Per-Row VTab Output

**What:** Emit N rows per entity, one per property, instead of one row with all properties.

**When:** Aligning with Snowflake's DESCRIBE output format.

**Example (describe.rs new pattern):**
```rust
struct DescribeRow {
    object_kind: Option<String>,   // NULL, TABLE, DIMENSION, METRIC, etc.
    object_name: Option<String>,
    parent_entity: Option<String>,
    property: String,
    property_value: String,
}

fn collect_describe_rows(def: &SemanticViewDefinition) -> Vec<DescribeRow> {
    let mut rows = Vec::new();
    // Semantic view-level properties
    rows.push(DescribeRow {
        object_kind: None, object_name: None, parent_entity: None,
        property: "COMMENT".into(), property_value: "".into(),
    });
    // Table-level properties
    for table in &def.tables {
        rows.push(DescribeRow {
            object_kind: Some("TABLE".into()),
            object_name: Some(table.alias.clone()),
            parent_entity: None,
            property: "BASE_TABLE_NAME".into(),
            property_value: table.table.clone(),
        });
        if !table.pk_columns.is_empty() {
            rows.push(DescribeRow {
                object_kind: Some("TABLE".into()),
                object_name: Some(table.alias.clone()),
                parent_entity: None,
                property: "PRIMARY_KEY".into(),
                property_value: format!("{:?}", table.pk_columns),
            });
        }
    }
    // Dimension properties (EXPRESSION + DATA_TYPE per dimension)
    for dim in &def.dimensions {
        let parent = dim.source_table.clone().unwrap_or_else(|| {
            def.tables.first().map(|t| t.alias.clone()).unwrap_or_default()
        });
        rows.push(DescribeRow {
            object_kind: Some("DIMENSION".into()),
            object_name: Some(dim.name.clone()),
            parent_entity: Some(parent.clone()),
            property: "EXPRESSION".into(),
            property_value: dim.expr.clone(),
        });
        if let Some(ref dt) = dim.output_type {
            rows.push(DescribeRow {
                object_kind: Some("DIMENSION".into()),
                object_name: Some(dim.name.clone()),
                parent_entity: Some(parent),
                property: "DATA_TYPE".into(),
                property_value: dt.clone(),
            });
        }
    }
    // ... metrics, relationships, facts similarly
    rows
}
```

### Pattern 4: Catalog Schema Migration for created_on

**What:** Add a `created_on` timestamp field to stored definitions.

**When:** SHOW SEMANTIC VIEWS needs a `created_on` column.

**Approach:** Store `created_on` as an ISO 8601 string inside the JSON definition. At define time, inject `Utc::now()` (or DuckDB's `current_timestamp`). For backward compatibility, existing definitions without `created_on` get a fallback value (e.g., empty string or epoch).

```rust
// In model.rs
pub struct SemanticViewDefinition {
    // ... existing fields
    #[serde(default)]
    pub created_on: Option<String>,  // ISO 8601 timestamp, None for pre-v0.5.5 defs
}
```

**Why not a new DB column?** The `_definitions` table stores `(name VARCHAR, definition VARCHAR)`. Adding a column requires ALTER TABLE migration logic. Embedding `created_on` in the JSON definition is simpler and backward-compatible -- `serde(default)` handles missing fields automatically. The tradeoff is that the timestamp is not independently queryable via SQL on the catalog table, but SHOW SEMANTIC VIEWS is the user-facing query path.

### Pattern 5: Database/Schema Context from DuckDB

**What:** Extract `database_name` and `schema_name` from DuckDB connection context at VTab bind time.

**Approach:** Execute `SELECT current_database(), current_schema()` via the query connection, or use pragmas. Since VTab bind has access to `BindInfo` which provides `get_extra_info<CatalogState>()`, the database/schema can be injected as additional context alongside the catalog state.

Alternative: Store `database_name` and `schema_name` in the JSON definition at define time (same approach as `created_on`). This is simpler and avoids runtime SQL execution during bind.

**Recommendation:** Store in JSON at define time. Simpler, deterministic, no bind-time SQL needed. If the database/schema changes after creation (unlikely for extension-managed objects), the stored values reflect creation context (which matches Snowflake behavior).

## Anti-Patterns to Avoid

### Anti-Pattern 1: Big-Bang Refactoring

**What:** Trying to do the module split and output format changes in a single commit.

**Why bad:** If tests break, you cannot tell whether the issue is from the refactoring (should be behavior-preserving) or the output format change (intentionally changes behavior). Debugging is 2x harder.

**Instead:** Refactoring first, then output format changes. Each phase should pass `just test-all` independently.

### Anti-Pattern 2: Changing Public API During Module Split

**What:** Renaming functions, changing signatures, or reorganizing the public API while splitting files.

**Why bad:** Creates unnecessary churn in import sites. Every caller must be updated simultaneously. Increases merge conflict surface.

**Instead:** The `expand/mod.rs` re-exports everything at the same `crate::expand::*` paths. All existing `use crate::expand::{expand, suggest_closest, QueryRequest}` statements continue to work unchanged. Internal reorganization is invisible to callers.

### Anti-Pattern 3: Mixing VTab Schema Changes with Logic Changes

**What:** Changing output columns AND changing the data collection logic (e.g., fan trap filtering in show_dims_for_metric) in the same phase.

**Why bad:** If a sqllogictest fails, unclear whether the column schema is wrong or the data logic is wrong.

**Instead:** Change output columns first (schema alignment), verify all tests pass, then make any logic changes (e.g., adding `required` boolean computation).

### Anti-Pattern 4: Using DuckDB Timestamps for created_on

**What:** Calling DuckDB's `current_timestamp` during VTab bind to populate `created_on`.

**Why bad:** Bind happens at query time, not define time. The timestamp would reflect when SHOW was run, not when the view was created.

**Instead:** Capture the timestamp in the define path (ddl/define.rs) and store it in the JSON definition.

## Scalability Considerations

Not applicable for this milestone -- the changes are structural (module organization) and schema-level (output formats). No performance-sensitive paths are modified. The expansion pipeline, which is the hot path, is only touched by the refactoring (file reorganization), not by behavioral changes.

## Recommended Build Order

The sequencing below minimizes risk by establishing the refactored module structure first (behavior-preserving), then making schema changes on top of the clean structure.

### Phase A: Extract Shared Utilities (C3 + C5)

**New files:** `src/util.rs`, `src/errors.rs`
**Modified files:** `src/lib.rs` (add `mod util; mod errors;`), `src/expand.rs`, `src/graph.rs`, `src/parse.rs`, `src/body_parser.rs`, `src/ddl/show_dims_for_metric.rs`
**Risk:** LOW -- moving 2 functions and 1 struct to new files, updating import paths.
**Validation:** `just test-all` must pass. Zero behavior change.

Steps:
1. Create `src/util.rs` with `suggest_closest` and `replace_word_boundary` (copy from expand.rs).
2. Create `src/errors.rs` with `ParseError` (copy from parse.rs).
3. Update `expand.rs`: remove `suggest_closest` and `replace_word_boundary` definitions, add `use crate::util::{suggest_closest, replace_word_boundary};`
4. Update `graph.rs`: change `use crate::expand::suggest_closest` to `use crate::util::suggest_closest;`
5. Update `parse.rs`: remove `ParseError` definition, add `pub use crate::errors::ParseError;` (re-export for backward compat).
6. Update `body_parser.rs`: change `use crate::parse::ParseError` to `use crate::errors::ParseError;`
7. Update `show_dims_for_metric.rs`: change `use crate::expand::suggest_closest` to `use crate::util::suggest_closest;`
8. Run `just test-all`.

### Phase B: Split expand.rs into expand/ Module Directory (C1)

**New directory:** `src/expand/`
**New files:** `mod.rs`, `validate.rs`, `resolve.rs`, `facts.rs`, `fan_trap.rs`, `role_playing.rs`, `join_resolver.rs`, `sql_gen.rs`
**Deleted file:** `src/expand.rs` (replaced by `src/expand/mod.rs`)
**Modified files:** `src/lib.rs` (no change -- `mod expand;` works for both file and directory)
**Risk:** MEDIUM -- largest refactoring step; 4,490 lines across 8 files. Tests in expand.rs must be distributed to submodules.
**Validation:** `just test-all` must pass. Zero behavior change. All `use crate::expand::*` paths must still resolve.

Key decisions:
- Tests stay with the code they test (e.g., fan trap tests go in `fan_trap.rs`).
- `mod.rs` contains only the `expand()` orchestrator function and re-exports.
- `pub(crate)` functions like `ancestors_to_root` and `collect_derived_metric_source_tables` are re-exported from `mod.rs` for `ddl/show_dims_for_metric.rs`.

### Phase C: Split graph.rs into graph/ Module Directory (C2)

**New directory:** `src/graph/`
**New files:** `mod.rs`, `relationship.rs`, `facts.rs`, `derived_metrics.rs`, `using.rs`
**Deleted file:** `src/graph.rs` (replaced by `src/graph/mod.rs`)
**Risk:** MEDIUM -- 2,502 lines across 5 files. Simpler than expand/ split because graph functions are more self-contained.
**Validation:** `just test-all` must pass. Zero behavior change.

### Phase D: Catalog Schema + created_on Timestamp

**Modified files:** `src/model.rs` (add `created_on`, `database_name`, `schema_name` fields), `src/ddl/define.rs` (inject timestamp/context at define time)
**Risk:** LOW -- additive changes only. `serde(default)` ensures backward compatibility.
**Validation:** Existing tests pass (old JSON without new fields deserializes correctly). New unit tests for timestamp injection.

### Phase E: SHOW SEMANTIC VIEWS Alignment

**Modified file:** `src/ddl/list.rs`
**Schema change:** 2 columns -> 6 columns (created_on, name, kind, database_name, schema_name, comment)
**Risk:** MEDIUM -- breaks existing sqllogictest expectations for SHOW SEMANTIC VIEWS output.
**Validation:** Update sqllogictest files, run `just test-all`.

### Phase F: SHOW SEMANTIC DIMENSIONS / METRICS / FACTS Alignment

**Modified files:** `src/ddl/show_dims.rs`, `src/ddl/show_metrics.rs`, `src/ddl/show_facts.rs`
**Schema change:** Current 5 columns -> 8 columns (database_name, schema_name, semantic_view_name, table_name, name, data_type, synonyms, comment). Drop `expr`.
**Risk:** MEDIUM -- three files with parallel changes, all break existing test expectations.
**Validation:** Update sqllogictest files, run `just test-all`.

### Phase G: SHOW SEMANTIC DIMENSIONS FOR METRIC Alignment

**Modified file:** `src/ddl/show_dims_for_metric.rs`
**Schema change:** 5 columns -> 6 columns (table_name, name, data_type, required, synonyms, comment). Drop `expr`, `semantic_view_name`. Add `required` boolean.
**Risk:** MEDIUM -- the `required` column computation is new logic (currently Snowflake uses it for window function PARTITION BY EXCLUDING; we can default to `false` initially since we don't support window metrics).
**Validation:** Update sqllogictest, run `just test-all`.

### Phase H: DESCRIBE SEMANTIC VIEW Alignment

**Modified file:** `src/ddl/describe.rs`
**Schema change:** 6 columns (single row) -> 5 columns (N rows, property-per-row format). Complete rewrite.
**Risk:** HIGH -- most complex output format change. Largest delta from current behavior. Many test expectations change.
**Validation:** Comprehensive new sqllogictest cases, run `just test-all`.

### Phase Ordering Rationale

1. **A before B/C:** Utility extraction (5 min) breaks circular deps, making the module splits cleaner.
2. **B before C:** expand.rs is larger and has more external consumers; splitting it first reduces risk for the graph split.
3. **A-C before D-H:** Refactoring is behavior-preserving; do it first while the test suite is stable. Output format changes intentionally break tests.
4. **D before E-H:** Catalog schema changes (created_on, database_name, schema_name) must exist before SHOW VTabs can emit those columns.
5. **E before F-G:** SHOW SEMANTIC VIEWS is the simplest SHOW command (list.rs is 101 lines). Proves the pattern before tackling the larger SHOW files.
6. **F before G:** SHOW DIMENSIONS/METRICS/FACTS are structurally identical. Do them together, then the more complex FOR METRIC variant.
7. **H last:** DESCRIBE is the most radical format change (single row to N rows). Do it after all SHOW commands are proven.

## Integration Points

### New Components
| Component | Type | Purpose |
|-----------|------|---------|
| `src/util.rs` | New module | Shared string utilities (suggest_closest, replace_word_boundary) |
| `src/errors.rs` | New module | Shared ParseError struct |
| `src/expand/` | Directory (replaces file) | Module directory for expansion pipeline |
| `src/graph/` | Directory (replaces file) | Module directory for graph validation |

### Modified Components
| Component | Change Type | What Changes |
|-----------|-------------|--------------|
| `src/model.rs` | Additive fields | `created_on`, `database_name`, `schema_name` on SemanticViewDefinition |
| `src/ddl/define.rs` | Additive logic | Inject timestamp + context at define time |
| `src/ddl/list.rs` | Schema change | 2 columns -> 6 columns (Snowflake SHOW SEMANTIC VIEWS format) |
| `src/ddl/show_dims.rs` | Schema change | 5 columns -> 8 columns (drop expr, add db/schema/synonyms/comment) |
| `src/ddl/show_metrics.rs` | Schema change | 5 columns -> 8 columns (same as dims) |
| `src/ddl/show_facts.rs` | Schema change | 4 columns -> 8 columns (add db/schema/data_type/synonyms/comment) |
| `src/ddl/show_dims_for_metric.rs` | Schema change | 5 columns -> 6 columns (drop expr/sv_name, add required/synonyms/comment) |
| `src/ddl/describe.rs` | Full rewrite | Single-row 6-column -> N-row 5-column property-per-row format |
| `src/parse.rs` | Import path | ParseError re-export from errors.rs |
| `src/body_parser.rs` | Import path | ParseError import from errors.rs |

### Unchanged Components
| Component | Why Unchanged |
|-----------|---------------|
| `src/query/table_function.rs` | Query path unaffected by SHOW/DESCRIBE changes |
| `src/query/explain.rs` | Uses expand::expand, which is only reorganized not changed |
| `shim.cpp` | C++ FFI layer unaffected |
| `src/catalog.rs` | Schema (`name, definition`) unchanged; new fields go in JSON |

## Sources

- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- output columns: created_on, name, kind, database_name, schema_name, comment, owner, owner_role_type
- [Snowflake DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- property-per-row format with 5 columns: object_kind, object_name, parent_entity, property, property_value
- [Snowflake SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) -- 8 columns: database_name, schema_name, semantic_view_name, table_name, name, data_type, synonyms, comment
- [Snowflake SHOW SEMANTIC METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics) -- same 8 columns as dimensions
- [Snowflake SHOW SEMANTIC DIMENSIONS FOR METRIC](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric) -- 6 columns: table_name, name, data_type, required, synonyms, comment
- [Rust module system: Separating Modules into Different Files](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html) -- official Rust book on module directories
- Direct codebase analysis of all 6 VTab files, expand.rs (4,490 lines), graph.rs (2,502 lines), parse.rs, catalog.rs, model.rs, and import graph
