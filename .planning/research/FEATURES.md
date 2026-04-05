# Feature Landscape: v0.5.5 SHOW/DESCRIBE Alignment & Refactoring

**Domain:** DuckDB Rust extension -- Snowflake-aligned SHOW/DESCRIBE output formats + module directory refactoring
**Researched:** 2026-04-01
**Milestone:** v0.5.5 -- Align all 6 SHOW/DESCRIBE output formats with Snowflake; split expand.rs and graph.rs into module directories
**Status:** Subsequent milestone research (v0.5.4 shipped 2026-03-27)
**Overall confidence:** HIGH (Snowflake output schemas verified from official docs for all 6 commands; existing codebase analyzed directly)

---

## Scope

This document covers the feature surface for v0.5.5: restructuring DESCRIBE to Snowflake's property-per-row format, aligning all SHOW command column schemas, adding `created_on`/`database_name`/`schema_name` metadata, and splitting the two largest modules into clean directories.

**What already exists (NOT in scope for research):**
- DESCRIBE SEMANTIC VIEW: 6 columns (name, base_table, dimensions JSON, metrics JSON, joins JSON, facts JSON), single row
- SHOW SEMANTIC VIEWS: 2 columns (name, base_table)
- SHOW SEMANTIC DIMENSIONS: 5 columns (semantic_view_name, name, expr, source_table, data_type)
- SHOW SEMANTIC METRICS: 5 columns (same pattern)
- SHOW SEMANTIC FACTS: 4 columns (semantic_view_name, name, expr, source_table -- no data_type)
- SHOW SEMANTIC DIMENSIONS FOR METRIC: 5 columns (same as SHOW DIMS)
- LIKE, STARTS WITH, LIMIT filtering on all SHOW commands
- 482 tests, 15.8K LOC
- expand.rs (4,440 lines), graph.rs (2,333 lines)

**Focus:** Output format alignment, metadata storage changes, `required` column semantics, and module decomposition.

---

## Table Stakes

Features users expect. Missing = output format diverges from Snowflake alignment goal.

### T1: DESCRIBE SEMANTIC VIEW -- Property-Per-Row Format

| Aspect | Detail |
|--------|--------|
| **Feature** | Restructure DESCRIBE output from 1 row with 6 JSON blob columns to N rows with 5 columns: `object_kind`, `object_name`, `parent_entity`, `property`, `property_value` |
| **Why Expected** | This is the core format change. Snowflake's DESCRIBE returns one row per property, enabling SELECT/WHERE filtering on individual properties. The current JSON blob format requires client-side JSON parsing. |
| **Complexity** | **Medium** -- complete rewrite of `DescribeSemanticViewVTab` |
| **Dependencies** | None -- self-contained rewrite of `src/ddl/describe.rs` |

**Snowflake's exact schema (verified from official docs):**

| Column | Type | Description |
|--------|------|-------------|
| `object_kind` | VARCHAR | Type of object: TABLE, RELATIONSHIP, DIMENSION, FACT, METRIC, DERIVED_METRIC, or NULL (view-level) |
| `object_name` | VARCHAR | Name of the dimension, fact, metric, logical table, or relationship |
| `parent_entity` | VARCHAR | Parent table name for dims/facts/metrics/relationships; NULL for tables/derived metrics/view-level |
| `property` | VARCHAR | Property name (TABLE, EXPRESSION, DATA_TYPE, PRIMARY_KEY, FOREIGN_KEY, REF_TABLE, REF_KEY, etc.) |
| `property_value` | VARCHAR | Property value as string |

**Properties to emit per object_kind (our extension's subset):**

| object_kind | Properties | Source in Model |
|-------------|-----------|-----------------|
| TABLE | BASE_TABLE_NAME, PRIMARY_KEY | `TableRef.table`, `TableRef.pk_columns` joined by comma |
| TABLE | BASE_TABLE_DATABASE_NAME, BASE_TABLE_SCHEMA_NAME | Stored metadata (see T5 below) |
| RELATIONSHIP | TABLE, REF_TABLE, FOREIGN_KEY, REF_KEY | `Join.from_alias`, `Join.table`, `Join.fk_columns`, `Join.pk_columns` |
| DIMENSION | TABLE, EXPRESSION, DATA_TYPE | `Dimension.source_table`, `Dimension.expr`, `Dimension.output_type` |
| FACT | TABLE, EXPRESSION, DATA_TYPE | `Fact.source_table`, `Fact.expr`, (fact type if available) |
| METRIC | TABLE, EXPRESSION, DATA_TYPE | `Metric.source_table`, `Metric.expr`, `Metric.output_type` |
| DERIVED_METRIC | EXPRESSION, DATA_TYPE | `Metric.expr` (no TABLE -- derived metrics reference other metrics) |

**Properties we skip (per user decision or N/A):**
- COMMENT, SYNONYMS (user decided: no NULL placeholders)
- ACCESS_MODIFIER (DuckDB has no RBAC)
- CONSTRAINT (not implemented -- PK expressed as TABLE property)
- CUSTOM_INSTRUCTIONS (Snowflake Cortex AI specific)
- Cortex Search Service properties (Snowflake specific)

**parent_entity mapping:**
- TABLE: NULL (tables are top-level)
- RELATIONSHIP: NULL (relationships reference two tables, not one parent)
- DIMENSION: `source_table` (the logical table this dim belongs to)
- FACT: `source_table` (the logical table this fact belongs to)
- METRIC: `source_table` (the logical table this metric belongs to)
- DERIVED_METRIC: NULL (no table association)

**Row ordering:** View-level properties first (if any), then TABLEs, then RELATIONSHIPs, then DIMENSIONs, then FACTs, then METRICs. Within each group, alphabetical by object_name, then by property name.

**Row count estimate:** A typical view with 2 tables, 1 relationship, 5 dims, 3 facts, 4 metrics produces ~48 rows. Comfortably within DuckDB's 2048-row chunk size.

**Implementation pattern:** The bind function parses the stored JSON into `SemanticViewDefinition`, walks each component in order, emits one row per property into a `Vec<DescribeRow>`, and the func callback emits them. Same pattern as existing SHOW commands.

**Confidence:** HIGH (schema verified from [DESCRIBE SEMANTIC VIEW docs](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view))

---

### T2: SHOW SEMANTIC VIEWS -- Expanded Column Schema

| Aspect | Detail |
|--------|--------|
| **Feature** | Expand from 2 columns (name, base_table) to 5 columns: `created_on`, `name`, `kind`, `database_name`, `schema_name` |
| **Why Expected** | Snowflake returns 8 columns; our subset drops owner/role/extension/comment columns that have no DuckDB equivalent. The remaining 5 provide essential metadata for programmatic consumption. |
| **Complexity** | **Medium** -- requires metadata storage changes (T5) + rewrite of `ListSemanticViewsVTab` |
| **Dependencies** | T5 (created_on + database_name + schema_name storage) |

**Target schema:**

| Column | Type | Source |
|--------|------|--------|
| `created_on` | TIMESTAMP | Stored at define time (see T5) |
| `name` | VARCHAR | Already available |
| `kind` | VARCHAR | Constant: 'SEMANTIC_VIEW' |
| `database_name` | VARCHAR | Stored at define time (see T5) |
| `schema_name` | VARCHAR | Stored at define time (see T5) |

**What changes from current:**
- ADD: `created_on`, `kind`, `database_name`, `schema_name`
- DROP: `base_table` (this information moves to DESCRIBE as TABLE properties)

**Breaking change:** Users currently referencing `base_table` from SHOW SEMANTIC VIEWS must switch to DESCRIBE. This is acceptable for a pre-1.0 extension.

**Confidence:** HIGH (schema verified from [SHOW SEMANTIC VIEWS docs](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views))

---

### T3: SHOW SEMANTIC DIMENSIONS/METRICS/FACTS -- Aligned Column Schema

| Aspect | Detail |
|--------|--------|
| **Feature** | Align all three SHOW commands from 5/4 columns to 6 columns: `database_name`, `schema_name`, `semantic_view_name`, `table_name`, `name`, `data_type` |
| **Why Expected** | Snowflake uses an 8-column schema (adding synonyms, comment). Our subset drops those two per user decision but must include the remaining 6. |
| **Complexity** | **Low** -- additive column changes + column rename + column removal |
| **Dependencies** | T5 (database_name + schema_name storage) |

**Target schema (same for all three commands):**

| Column | Type | Source |
|--------|------|--------|
| `database_name` | VARCHAR | Stored at define time |
| `schema_name` | VARCHAR | Stored at define time |
| `semantic_view_name` | VARCHAR | Already available |
| `table_name` | VARCHAR | Currently `source_table` -- renamed |
| `name` | VARCHAR | Already available |
| `data_type` | VARCHAR | Already available for dims/metrics; needs addition for facts |

**What changes from current:**
- ADD: `database_name`, `schema_name` (prepended)
- RENAME: `source_table` to `table_name`
- DROP: `expr` (user decision: implementation detail, available via DESCRIBE EXPRESSION property)
- ADD to FACTS: `data_type` (Snowflake includes it; current facts output omits it)

**Breaking change:** `expr` column removal and `source_table` rename. Acceptable for pre-1.0.

**SHOW SEMANTIC FACTS data_type:** Facts currently have no `output_type` field in the model. The `Fact` struct stores `name`, `expr`, and `source_table` only. Adding `data_type` to facts requires either:
1. Adding `output_type: Option<String>` to the `Fact` struct and populating it during type inference (same as dims/metrics)
2. Emitting empty string for facts without type info

Option 1 is cleaner and consistent with dims/metrics. Facts are row-level expressions with deterministic types, so LIMIT 0 inference can resolve them.

**Confidence:** HIGH (schemas verified from [SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions), [SHOW SEMANTIC METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics), [SHOW SEMANTIC FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts))

---

### T4: SHOW SEMANTIC DIMENSIONS FOR METRIC -- Aligned with `required` Column

| Aspect | Detail |
|--------|--------|
| **Feature** | Align from 5 columns to 4 columns: `table_name`, `name`, `data_type`, `required` |
| **Why Expected** | Snowflake's FOR METRIC output has 6 columns (adding synonyms, comment); our subset drops those two. The `required` column is Snowflake-specific and meaningful. |
| **Complexity** | **Medium** -- column changes are simple, but `required` semantics need a design decision |
| **Dependencies** | None beyond existing fan-trap filtering logic |

**Target schema:**

| Column | Type | Source |
|--------|------|--------|
| `table_name` | VARCHAR | Currently `source_table` -- renamed |
| `name` | VARCHAR | Already available |
| `data_type` | VARCHAR | Already available |
| `required` | BOOLEAN | New -- see analysis below |

**What changes from current:**
- DROP: `semantic_view_name` (scoped to single view by the command itself)
- DROP: `expr` (consistent with other SHOW commands)
- RENAME: `source_table` to `table_name`
- ADD: `required` BOOLEAN column

**The `required` column -- what Snowflake does (verified from official docs):**

In Snowflake, `required` is TRUE when a metric's definition includes a `PARTITION BY EXCLUDING` clause naming that dimension. This means the metric (typically a window function metric) cannot be computed without grouping by that dimension.

**The `required` column -- what this extension should do:**

This extension does not support `PARTITION BY EXCLUDING` (that is a window function metric feature, out of scope per PROJECT.md). Since only aggregate metrics are supported, no dimension is ever structurally required -- any subset of fan-trap-safe dimensions is valid.

**Decision: `required` = constant FALSE for all rows.**

Rationale:
- Honest: no dimension is structurally required for aggregate metrics
- Snowflake-compatible: the column exists in the schema
- Future-proof: when window function metrics are added, `required` gains real meaning via `PARTITION BY EXCLUDING`
- No false requirements: avoids confusing users into thinking they *must* include certain dimensions

Alternative considered and rejected: "infer required from fan-trap safety" -- this conflates "available" (what FOR METRIC already filters to) with "required" (what the metric definition demands). These are different concepts.

**Confidence:** HIGH (schema and semantics verified from [SHOW SEMANTIC DIMENSIONS FOR METRIC docs](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric))

---

### T5: Metadata Storage -- created_on, database_name, schema_name

| Aspect | Detail |
|--------|--------|
| **Feature** | Store `created_on` timestamp, `database_name`, and `schema_name` at define time in the catalog JSON. Surface in SHOW/DESCRIBE output. |
| **Why Expected** | Prerequisite for T2 and T3. Without stored metadata, SHOW commands cannot populate these columns. |
| **Complexity** | **Medium** -- model change, catalog write-path change, migration for existing definitions |
| **Dependencies** | None -- foundational change that others depend on |

**Implementation approach:**

1. **New fields in `SemanticViewDefinition`:**
   ```rust
   #[serde(default)]
   pub created_on: Option<String>,   // ISO 8601: "2026-04-01T12:34:56Z"
   #[serde(default)]
   pub database_name: Option<String>,
   #[serde(default)]
   pub schema_name: Option<String>,
   ```

2. **Set at define time:** In the `catalog_insert` / `catalog_insert_or_replace` paths, populate before JSON serialization.

3. **Timestamp source:** Use `std::time::SystemTime` formatted to ISO 8601. Avoids adding `chrono` as a dependency. Second precision is sufficient for display.
   ```rust
   use std::time::{SystemTime, UNIX_EPOCH};
   let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
   // Format: "2026-04-01T12:34:56Z"
   ```

4. **database_name source:** The `db_path` string passed through extension init. For `:memory:` databases, use `"memory"`. For file-backed databases, extract the database name (typically the filename without extension, matching DuckDB's `current_database()` behavior).

5. **schema_name source:** Always `"semantic_layer"` -- definitions live in the `semantic_layer._definitions` table. This is the schema context where the semantic view metadata resides.

6. **Migration for existing definitions:** Old stored JSON without these fields deserializes as `None` via `#[serde(default)]`. SHOW output renders NULL for `created_on` on old definitions. `database_name` and `schema_name` can be backfilled at load time from the current DuckDB context.

**Alternative considered and rejected:** Querying DuckDB `SELECT current_database()` at bind time instead of storing. This avoids model changes but is incorrect when a database is re-attached under a different alias. Storing at define time is the correct approach.

**Confidence:** HIGH (straightforward model extension; `#[serde(default)]` migration pattern already used for `facts`, `tables`, `column_type_names`)

---

### T6: Module Directory Refactoring -- expand.rs and graph.rs

| Aspect | Detail |
|--------|--------|
| **Feature** | Split `expand.rs` (4,440 lines) into `expand/` module directory and `graph.rs` (2,333 lines) into `graph/` module directory. Extract shared `util.rs` and `errors.rs`. |
| **Why Expected** | These are the two largest files in the codebase. `graph.rs` is targeted for future PyO3/Maturin extraction. Module directories establish clean boundaries. |
| **Complexity** | **Medium-High** -- mechanical but high-volume; must preserve all 482 tests |
| **Dependencies** | None -- pure refactoring, no behavior changes |

**Current state:**
- `expand.rs`: 4,440 lines -- query expansion, SQL generation, fact inlining, derived metric resolution, USING relationship scoping, type inference
- `graph.rs`: 2,333 lines -- `RelationshipGraph`, topological sort, fan trap detection, parent/child maps, cardinality tracking, cycle/diamond validation

**Suggested decomposition for expand/:**
| File | Contents | Approx Lines |
|------|----------|--------------|
| `expand/mod.rs` | Public API re-exports, `SemanticExpander` struct | ~200 |
| `expand/sql_gen.rs` | `build_execution_sql`, FROM/JOIN clause generation, GROUP BY | ~800 |
| `expand/fact_inlining.rs` | Fact expression substitution, word-boundary matching, DAG resolution | ~600 |
| `expand/metric_resolution.rs` | Derived metric inlining, `collect_derived_metric_source_tables`, stacking | ~500 |
| `expand/using_relationships.rs` | USING clause handling, scoped alias generation, ambiguity detection | ~400 |
| `expand/type_inference.rs` | LIMIT 0 type inference, type map construction, cast wrapping | ~500 |
| `expand/helpers.rs` | `suggest_closest`, `ancestors_to_root`, shared utilities | ~300 |
| `expand/tests.rs` or inline `#[cfg(test)]` | Test modules (likely the largest chunk) | ~1,100 |

**Suggested decomposition for graph/:**
| File | Contents | Approx Lines |
|------|----------|--------------|
| `graph/mod.rs` | Public API re-exports, `RelationshipGraph` struct | ~200 |
| `graph/builder.rs` | `from_definition()`, adjacency list construction, reverse edges | ~400 |
| `graph/validation.rs` | Cycle detection, diamond detection, tree structure validation | ~400 |
| `graph/toposort.rs` | Kahn's algorithm, topological ordering | ~300 |
| `graph/fan_trap.rs` | Fan trap detection, LCA path analysis, cardinality edge checking | ~400 |
| `graph/join_synthesis.rs` | `synthesize_on_clause()`, PK/FK matching, ON clause generation | ~300 |
| `graph/tests.rs` or inline `#[cfg(test)]` | Test modules | ~330 |

**Shared extractions:**
- `util.rs`: `suggest_closest()` (strsim fuzzy matching) -- currently in `expand.rs` but used by `show_dims_for_metric.rs` too
- `errors.rs`: Common error types if circular dependencies exist between expand/ and graph/

**Key constraint:** `show_dims_for_metric.rs` imports `ancestors_to_root` and `collect_derived_metric_source_tables` from `expand.rs`. After splitting, these must remain accessible -- either via `expand::helpers` re-export or by moving to a shared `util.rs`.

**Risk:** Module boundary decisions may need adjustment during implementation if hidden coupling surfaces. The test suite (482 tests) is the safety net.

**Confidence:** HIGH (straightforward mechanical refactoring with a comprehensive test suite)

---

## Differentiators

Features that set the product apart. Not expected, but valued.

### D1: Hierarchies in DESCRIBE Output

| Aspect | Detail |
|--------|--------|
| **Feature** | Emit HIERARCHY as an `object_kind` in DESCRIBE with a DIMENSIONS property listing the drill path |
| **Value Proposition** | Snowflake does not have a `SHOW SEMANTIC HIERARCHIES` command. Exposing hierarchy metadata via DESCRIBE makes it discoverable. |
| **Complexity** | Low -- hierarchies already in model, just needs property emission |
| **Dependencies** | T1 (DESCRIBE rewrite must be done first) |

Not in Snowflake's schema, but the extension already supports hierarchies and they should be visible somewhere. DESCRIBE is the natural home. Emit as:
- object_kind: `HIERARCHY`
- object_name: hierarchy name
- parent_entity: NULL
- property: `DIMENSIONS`
- property_value: comma-separated dimension names in drill order

### D2: TERSE Mode for SHOW SEMANTIC VIEWS

| Aspect | Detail |
|--------|--------|
| **Feature** | Support `SHOW TERSE SEMANTIC VIEWS` returning only 3 columns: created_on, name, kind |
| **Value Proposition** | Snowflake supports TERSE mode; useful for scripting where full metadata is not needed |
| **Complexity** | Medium -- requires parser to detect TERSE keyword |
| **Dependencies** | T2 (SHOW VIEWS alignment) |

**Recommendation: Defer.** Adds parser complexity for marginal value. The standard SHOW output is only 5 columns.

### D3: Lexicographic Sort by (database, schema, name)

| Aspect | Detail |
|--------|--------|
| **Feature** | Sort all SHOW output lexicographically by database_name, schema_name, then object name |
| **Value Proposition** | Matches Snowflake's documented ordering guarantee |
| **Complexity** | Low -- already sorted by name; add database/schema as prefix sort keys |
| **Dependencies** | T5 (database_name + schema_name must be available) |

Trivial to implement since there is only one database and schema context. The sort effectively remains by name only, but the code should be structured to support multi-database scenarios if they ever arise.

---

## Anti-Features

Features to explicitly NOT build in v0.5.5.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| `comment` / `synonyms` columns with NULL placeholders | User explicitly decided against NULL placeholder columns. They add visual noise without value until comments/synonyms are first-class features. | Omit entirely. Add columns when comments/synonyms become a DDL feature. |
| `owner` / `owner_role_type` columns in SHOW VIEWS | DuckDB has no RBAC model. These are Snowflake-specific. | Omit. Not applicable to DuckDB extensions. |
| `access_modifier` property in DESCRIBE | Snowflake PRIVATE/PUBLIC access; DuckDB has no access control. | Omit. All objects are implicitly public. |
| `extension` column in SHOW VIEWS | Snowflake-specific column for semantic view extensions (a Snowflake concept). | Omit. Not applicable. |
| CONSTRAINT object_kind in DESCRIBE | Snowflake uses constraints for time-range boundaries (START_COLUMN/END_COLUMN). | Omit. PK/UNIQUE expressed as TABLE properties. |
| CUSTOM_INSTRUCTIONS object_kind | Snowflake Cortex AI integration. | Omit entirely. |
| Cortex Search Service dimension properties | Snowflake-specific AI/search integration. | Omit entirely. |
| `SHOW COLUMNS` command | Snowflake has a separate SHOW COLUMNS returning dims/facts/metrics with a `kind` column. | Not needed. SHOW SEMANTIC DIMENSIONS/METRICS/FACTS already covers this. Avoid duplicate interfaces. |
| `IN ACCOUNT / IN DATABASE` scoping | Snowflake scopes SHOW across databases/accounts. DuckDB extension operates in a single database. | Omit. `IN semantic_view_name` is the only meaningful scope. |
| `SHOW TERSE SEMANTIC VIEWS` | Adds parser complexity for marginal scripting benefit. | Defer to future milestone. |

---

## Feature Dependencies

```
T5: Metadata storage (created_on, database_name, schema_name)
  |
  +--> T2: SHOW SEMANTIC VIEWS (uses stored created_on, database_name, schema_name)
  |
  +--> T3: SHOW SEMANTIC DIMENSIONS/METRICS/FACTS (uses stored database_name, schema_name)

T1: DESCRIBE property-per-row rewrite (independent, no prerequisites)

T4: SHOW DIMS FOR METRIC alignment (independent, no prerequisites)

T6: Module directory refactoring (independent, no prerequisites)
    (But should be done BEFORE or AFTER the SHOW/DESCRIBE changes, not during,
     to avoid merge conflicts in the same files being restructured)

Fact data_type (needed by T3 for SHOW FACTS)
  +--> Requires `output_type` field added to Fact model
  +--> Requires type inference update to populate fact types
```

**Critical insight:** T6 (module refactoring) should be either the first or last phase. Doing it mid-milestone while SHOW/DESCRIBE files are also changing creates merge complexity. Recommend doing it first since it does not change behavior and provides a cleaner code structure for the subsequent SHOW/DESCRIBE changes.

---

## Complexity Assessment Summary

| Feature | Complexity | Est. LOC Delta | Risk | Phase Order |
|---------|------------|----------------|------|-------------|
| T6: Module refactoring (expand/, graph/) | Medium-High | ~0 net (reorganization) | Low (test suite as safety net) | 1st |
| T5: Metadata storage (created_on, db, schema) | Medium | ~80 (model + catalog) | Low (proven #[serde(default)] pattern) | 2nd |
| T1: DESCRIBE property-per-row | Medium | ~150 (complete rewrite of describe.rs) | Low-Medium (new output format) | 3rd |
| T2: SHOW VIEWS alignment | Low-Medium | ~60 (rewrite list.rs) | Low | 4th |
| T3: SHOW DIMS/METRICS/FACTS alignment | Low | ~80 (column changes across 3 files) | Low | 5th |
| T4: SHOW DIMS FOR METRIC alignment | Medium | ~40 (column changes + required) | Low | 6th |
| D1: Hierarchies in DESCRIBE | Low | ~30 | None | With T1 |
| **Total** | **Medium** | **~440 LOC delta** | **Low-Medium** | |

---

## MVP Recommendation

Prioritize:

1. **T6: Module directory refactoring** -- do first while no other changes are in flight. Provides clean structure for subsequent work. Test suite validates correctness.

2. **T5: Metadata storage at define time** -- prerequisite for multiple SHOW changes. Small, foundational change that unblocks T2 and T3. Include fact `output_type` addition here too.

3. **T1: DESCRIBE property-per-row rewrite** -- the largest structural change. Independent of SHOW changes. Should be done before SHOW changes so DESCRIBE becomes the canonical place to find `expr` values (which are being dropped from SHOW).

4. **T2: SHOW SEMANTIC VIEWS alignment** -- uses new stored fields. Establishes the pattern for remaining SHOW changes.

5. **T3: SHOW DIMS/METRICS/FACTS alignment** -- mechanical column changes following the pattern established in T2.

6. **T4: SHOW DIMS FOR METRIC alignment with `required` column** -- last because the `required` semantics are the most nuanced decision (constant FALSE).

Defer:
- **D2: TERSE mode** -- parser complexity for marginal value. Future milestone.
- **D3: Lexicographic sort** -- trivially correct already (single database/schema). Can fold into T2/T3 if desired.
- **D1: Hierarchies in DESCRIBE** -- fold into T1 if scope allows, otherwise defer.

---

## Sources

### Snowflake Official Documentation (HIGH confidence)

- [DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- Property-per-row format, 5-column schema, object_kind/property combinations, parent_entity rules
- [SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- 8-column schema (created_on, name, kind, database_name, schema_name, comment, owner, owner_role_type), TERSE mode, filtering
- [SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) -- 8-column schema (database_name, schema_name, semantic_view_name, table_name, name, data_type, synonyms, comment)
- [SHOW SEMANTIC METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics) -- Same 8-column schema as dimensions
- [SHOW SEMANTIC FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts) -- Same 8-column schema; facts DO have data_type
- [SHOW SEMANTIC DIMENSIONS FOR METRIC](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric) -- 6-column schema (table_name, name, data_type, required, synonyms, comment); `required` = TRUE when PARTITION BY EXCLUDING names the dimension
- [Using SQL commands for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- Complete command listing, SHOW COLUMNS as alternative

### Project Source Code (HIGH confidence -- direct analysis)

- `src/ddl/describe.rs` -- Current 6-column single-row DescribeSemanticViewVTab (146 lines)
- `src/ddl/list.rs` -- Current 2-column ListSemanticViewsVTab (102 lines)
- `src/ddl/show_dims.rs` -- Current 5-column ShowSemanticDimensionsVTab (199 lines)
- `src/ddl/show_metrics.rs` -- Current 5-column ShowSemanticMetricsVTab (201 lines)
- `src/ddl/show_facts.rs` -- Current 4-column ShowSemanticFactsVTab (194 lines)
- `src/ddl/show_dims_for_metric.rs` -- Current 5-column ShowDimensionsForMetricVTab (336 lines)
- `src/model.rs` -- SemanticViewDefinition struct, Dimension/Metric/Fact/Join/TableRef structs
- `src/catalog.rs` -- CatalogState (HashMap<String, String>), init_catalog, catalog_insert
- `src/expand.rs` -- 4,440 lines, query expansion engine
- `src/graph.rs` -- 2,333 lines, relationship graph and validation
- `TECH-DEBT.md` -- Item 12: DDL pipeline uses all-VARCHAR result forwarding (relevant to output type decisions)
