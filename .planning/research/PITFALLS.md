# Domain Pitfalls -- SHOW/DESCRIBE Alignment & Module Refactoring (v0.5.5)

**Domain:** Snowflake-aligned SHOW/DESCRIBE output formats + module refactoring in DuckDB Rust extension
**Researched:** 2026-04-01
**Context:** The extension has 482 tests (Rust unit, proptest, sqllogictest, DuckLake CI), 15,786 LOC, stores semantic view definitions as JSON in `semantic_layer._definitions`, and uses a C++ shim that dynamically forwards VTab output columns as all-VARCHAR. The C++ shim reads `duckdb_column_count()` and `duckdb_column_name()` at runtime from the underlying Rust VTab result, so column count/name changes propagate automatically -- but test assertions do not.

---

## Critical Pitfalls

Mistakes that cause rewrites, test suite failures, or breaking changes.

### C1: C++ Shim Schema Forwarding Is Transparent but Tests Are Rigid

**What goes wrong:** The C++ `sv_ddl_bind` (shim.cpp:134-201) dynamically reads column count and names from the result of executing rewritten DDL SQL. It declares ALL output columns as VARCHAR. When you change the output schema of any VTab that feeds a DDL command (e.g., adding `created_on`, `database_name`, `schema_name`, `kind` to `ListSemanticViewsVTab`, or restructuring DESCRIBE to property-per-row format), the C++ side picks up the new schema automatically. **However**, sqllogictest assertions are column-count-sensitive. The `query TTTTT` prefix specifies expected column types and count. Every `.test` file that asserts SHOW or DESCRIBE output must be updated atomically with the schema change.

**Why it happens:** The C++ shim is schema-agnostic (good), but the test harness is schema-rigid (by design). Changing the VTab output columns in Rust without simultaneously updating every `.test` file that references those commands creates a guaranteed test failure across the entire sqllogictest suite.

**Consequences:** All 18 sqllogictest files run per-process (one DuckDB per file due to parser extension lifecycle segfault workaround). A single column-count mismatch in one file fails that file's entire test run. If SHOW SEMANTIC DIMENSIONS changes from 5 columns (`TTTTT`) to 8 columns (`TTTTTTTT`), every test asserting that schema breaks.

**Prevention:**
1. Identify ALL test files referencing each changed command before coding: `phase34_1_show_commands.test`, `phase34_1_show_filtering.test`, `phase34_1_show_dims_for_metric.test`, `phase20_extended_ddl.test` (DESCRIBE assertions).
2. Change VTab output columns and ALL corresponding test expectations in the same commit.
3. Run `just test-all` (not just `cargo test`) after every schema change -- sqllogictest catches column-count mismatches that Rust unit tests never see.

**Detection:** `just test-sql` fails with "expected N columns but got M" errors. These are obvious once you look for them but easy to miss if you only run `cargo test`.

**Phase assignment:** Every phase that changes output columns. Must be a hard rule: VTab + tests = atomic.

---

### C2: Backward-Incompatible JSON Deserialization When Adding `created_on`

**What goes wrong:** Adding a `created_on` field to `SemanticViewDefinition` is safe for deserialization if done correctly (`#[serde(default)]` handles missing fields). But the catalog persistence table (`semantic_layer._definitions`) stores raw JSON strings. Existing stored JSON from v0.5.4 databases will not contain `created_on`. If any code path assumes `created_on` is always populated (e.g., `.unwrap()` on it), it will panic on views created before the upgrade.

**Why it happens:** The project has a deliberate "no `deny_unknown_fields`" policy on `SemanticViewDefinition` (see model.rs line 160-161 comment). This means new fields can be added without breaking deserialization of old JSON. But the policy protects against deserialization failure, not against code that assumes the new field is populated.

**Consequences:** Views defined before the `created_on` field was added will have `created_on: None`. If SHOW SEMANTIC VIEWS renders `created_on` by calling `.unwrap()`, it panics at query time -- a runtime crash, not a compile-time error.

**Prevention:**
1. Add `created_on` as `Option<String>` with `#[serde(default)]` and `#[serde(skip_serializing_if = "Option::is_none")]` to match the existing pattern used by `pk_columns`, `unique_constraints`, etc.
2. In SHOW SEMANTIC VIEWS output, render `None` as empty string (`""`) -- never unwrap.
3. Write a test that deserializes old-format JSON (without `created_on`) and confirms the VTab emits a row without panicking.
4. Do NOT add a schema migration to `init_catalog` to backfill `created_on` into stored JSON -- it is unnecessary and risky. Old views simply show blank timestamps.
5. Store `created_on` inside the JSON definition, not as a separate SQL column in `semantic_layer._definitions`. The JSON approach requires zero schema migration, works with existing `catalog_insert`/`catalog_upsert`, and aligns with how all other definition fields are stored.

**Detection:** If you `.unwrap()` on a `None` `created_on`, `cargo test` will catch it if any test loads a JSON fixture without the field. Existing test fixtures naturally lack the field, so this pitfall is self-detecting if you test against existing fixtures.

**Phase assignment:** Must be addressed in the phase that adds `created_on` to the model. The field addition and its consumers must be reviewed together.

---

### C3: DESCRIBE Restructuring Is a Complete Rewrite, Not an Incremental Change

**What goes wrong:** The current DESCRIBE returns a single row with 6 VARCHAR columns (`name`, `base_table`, `dimensions`, `metrics`, `joins`, `facts`). Snowflake's DESCRIBE returns multiple rows in a property-per-row format with 5 columns (`object_kind`, `object_name`, `parent_entity`, `property`, `property_value`). This is not a column rename -- it is a fundamentally different output structure. Every consumer of DESCRIBE output must be rewritten: the VTab bind, the VTab func, the bind data struct, the init data struct, and every test.

**Why it happens:** The original DESCRIBE was designed for quick inspection, not Snowflake compatibility. The Snowflake format is more powerful (queryable with RESULT_SCAN/pipe operator, filterable by object_kind) but radically different.

**Consequences:**
1. `DescribeSemanticViewVTab` must be completely rewritten (not incrementally modified).
2. The sqllogictest `phase20_extended_ddl.test` asserts the current 6-column schema -- must be rewritten.
3. Any Python examples or documentation that reference DESCRIBE output format must be updated.
4. The `DescribeBindData` struct changes from holding scalar strings to holding a `Vec<PropertyRow>`.
5. The VTab changes from emitting 1 row to emitting potentially 100+ rows for complex views.

**Prevention:**
1. Plan DESCRIBE restructuring as a discrete, atomic phase -- do not interleave it with other changes.
2. Write the new DESCRIBE VTab as a clean replacement, not an incremental edit. The old code provides no reusable structure.
3. Build a helper function (e.g., `build_describe_rows(&SemanticViewDefinition) -> Vec<PropertyRow>`) that is unit-testable independently of the VTab machinery.
4. Update the example Python files (`basic_ddl_and_query.py`, `advanced_features.py`) if they print DESCRIBE output.

**Detection:** `just test-sql` failure on any test file asserting DESCRIBE output.

**Phase assignment:** Dedicated DESCRIBE restructuring phase. Do not combine with SHOW column changes.

---

### C4: Module Refactoring Breaks `use crate::expand::` Import Paths Across 10+ Files

**What goes wrong:** When splitting `expand.rs` (4,490 lines, 99 tests) into `expand/mod.rs` + submodules, every `use crate::expand::` import across the codebase must resolve to the new module structure. Rust's module system means `expand::suggest_closest` must either be re-exported from `expand/mod.rs` or callers must change to the new path. Same for `expand::ExpandError`, `expand::QueryRequest`, `expand::expand()`.

**Why it happens:** Rust module refactoring is not purely structural -- it changes the public API surface. Moving a function from `expand.rs` to `expand/resolve.rs` means it is now at `crate::expand::resolve::function_name` unless explicitly re-exported via `pub use` in `expand/mod.rs`.

**Consequences:** If re-exports are missed, compilation fails with "unresolved import" errors. These are caught at compile time (good) but can cascade to many files:
- `graph.rs` imports `expand::suggest_closest`
- `query/table_function.rs` imports `expand::expand` and `expand::QueryRequest`
- `query/explain.rs` imports from `expand`
- `query/error.rs` imports from `expand`
- All 99 tests in `expand.rs` must move to the correct submodule or a shared `tests.rs`

**Prevention:**
1. **Re-export everything from `expand/mod.rs`** during the initial split. The `mod.rs` file should contain `pub use` for every public item that was previously accessible as `crate::expand::X`. Preserve the external API before changing it.
2. Use `cargo test` continuously during the split -- it catches unresolved imports immediately.
3. Extract `suggest_closest` and `replace_word_boundary` to `src/util.rs` FIRST (breaking the circular dependency) before splitting `expand.rs` into a module directory. This isolates the most dangerous change.
4. Do the `graph.rs` split AFTER `expand.rs` because `graph.rs` has fewer external dependents.

**Detection:** `cargo test` -- Rust compilation errors are immediate and precise. The risk is not silent breakage but cascading fix-up across many files.

**Phase assignment:** Module refactoring phase. Must happen in a specific order: util.rs extraction -> expand/ split -> graph/ split.

---

## Moderate Pitfalls

### M1: Circular Dependency During Incremental Refactoring

**What goes wrong:** The current codebase has a known circular dependency: `expand.rs` exports `suggest_closest` which `graph.rs` imports (line 13), while `expand.rs` imports `graph::RelationshipGraph` (line 4). During refactoring, if you split `expand.rs` into submodules before extracting `suggest_closest` to `util.rs`, you may create a temporary state where `graph/` submodules import from `expand/` submodules and vice versa, making the dependency graph harder to reason about.

**Prevention:**
1. Extract `util.rs` (containing `suggest_closest` and `replace_word_boundary`) FIRST -- this is a small, well-bounded extraction.
2. Update `graph.rs` to import from `crate::util` instead of `crate::expand`.
3. Verify `cargo test` passes.
4. THEN proceed with the `expand/` module split.
5. THEN proceed with the `graph/` module split.

The order must be: break the cycle, then split the modules.

**Phase assignment:** First step of the refactoring phase. Single commit.

---

### M2: DuckDB Database/Schema Name Retrieval in Extension Context

**What goes wrong:** Snowflake's SHOW output includes `database_name` and `schema_name`. In DuckDB, the equivalent metadata comes from `current_database()` / `current_schema()` SQL functions. But these SQL functions cannot be called from within a VTab `bind()` because the ClientContext lock is held. The extension already has a `persist_conn` and `query_conn` for this purpose, but the SHOW VTab implementations (list.rs, show_dims.rs, etc.) only access `CatalogState` via `extra_info` -- they have no connection handle.

**Why it happens:** The existing SHOW VTabs were designed to only need the in-memory catalog. Adding database/schema metadata requires either: (a) storing the database/schema name at init time, or (b) passing an additional connection to the VTab via extra_info for metadata queries.

**Prevention:**
1. At `init_catalog` time (or extension init), query `SELECT current_database(), current_schema()` and store the results alongside the catalog state. Create a new wrapper struct (e.g., `ExtensionState { catalog: CatalogState, database_name: String, schema_name: String }`).
2. Pass this enriched state through `extra_info` to the VTabs.
3. Do NOT try to execute SQL from within a VTab `bind()` on the main connection -- it will deadlock silently.
4. For in-memory databases, `current_database()` returns `'memory'` and `current_schema()` returns `'main'` -- these are correct values to display.

**Detection:** If you try to call `duckdb_query` on the main connection from within `bind()`, the process will hang (deadlock) rather than error. This is silent and hard to debug.

**Phase assignment:** SHOW SEMANTIC VIEWS column expansion phase. Must precede the VTab changes that need the data.

---

### M3: Dropping `expr` Column from SHOW SEMANTIC DIMENSIONS/METRICS/FACTS

**What goes wrong:** The current SHOW SEMANTIC DIMENSIONS has 5 columns: `semantic_view_name`, `name`, `expr`, `source_table`, `data_type`. Snowflake's equivalent has 8 columns: `database_name`, `schema_name`, `semantic_view_name`, `table_name`, `name`, `data_type`, `synonyms`, `comment`. Snowflake does NOT expose `expr` in SHOW output (expressions are visible in DESCRIBE via the `EXPRESSION` property). Dropping `expr` from SHOW output removes information that users may rely on for debugging.

**Prevention:**
1. Drop `expr` from SHOW output to align with Snowflake. DESCRIBE (in property-per-row format) still exposes expressions via the `EXPRESSION` property.
2. Add `database_name` and `schema_name` as the first two columns (matching Snowflake column order).
3. For `synonyms` and `comment`: Snowflake has these; DuckDB semantic views do not support them yet. Emit empty strings for these columns to maintain schema compatibility.
4. Rename `source_table` to `table_name` to match Snowflake naming.
5. Document the column changes in release notes -- users scripting against SHOW output will break.

**Detection:** Any test or script that references `expr` in SHOW output or uses column positions will fail.

**Phase assignment:** SHOW DIMS/METRICS/FACTS alignment phase. Must update all three SHOW commands together for consistency.

---

### M4: `parse.rs` Error Type Extraction Order Matters

**What goes wrong:** `body_parser.rs` imports `parse::ParseError`. If you extract `ParseError` to `errors.rs` before splitting `expand.rs`/`graph.rs`, the extraction is clean. If you do it after or interleaved, you have to update import paths twice -- once for the initial extraction, and again when the modules that use it are restructured.

**Prevention:** Extract `ParseError` to `errors.rs` at the same time as extracting `suggest_closest` to `util.rs`. Both are "break the dependency cycle" changes that should happen in a single preparatory phase before any module directory splitting.

**Phase assignment:** Same initial refactoring step as M1.

---

### M5: Test Migration Is the Largest Work Item

**What goes wrong:** The codebase has 482 tests. Module refactoring affects the location and import paths of tests embedded within the refactored files. Schema changes to SHOW/DESCRIBE affect sqllogictest assertions. The actual code changes (adding columns, restructuring output) are straightforward, but migrating every affected test is labor-intensive and error-prone.

**Why it happens:** Tests for `expand.rs` (99 unit tests) and `graph.rs` (numerous tests) are embedded at the bottom of each file with `#[cfg(test)] mod tests { ... }`. When splitting into module directories, these tests must move to the correct submodule (close to the code they test). Each moved test must have its `use` imports updated.

**Prevention:**
1. For module splits: keep `#[cfg(test)] mod tests` blocks in each submodule file. Move tests with the function they test, not to a central `tests.rs`.
2. For sqllogictest: identify all affected `.test` files upfront and update them in the same commit as the schema change.
3. Run `just test-all` after every individual refactoring step -- do not batch multiple changes before testing.
4. Consider keeping a `expand/tests.rs` for integration-style tests that exercise the full `expand()` pipeline, while unit tests for individual functions live in their respective submodules.

**Detection:** `cargo test` for Rust test migration errors. `just test-sql` for sqllogictest assertion failures. `just test-all` catches both.

**Phase assignment:** Embedded in every phase. Budget time for test migration in each phase estimate.

---

## Minor Pitfalls

### N1: VTab Chunk Size on Large DESCRIBE Output

**What goes wrong:** The current VTab pattern uses `init_data.done.swap(true)` to emit all rows in a single `func()` call. The Snowflake property-per-row DESCRIBE format can produce 100+ rows for complex views. The DuckDB standard vector size is 2048, so this is unlikely to be a problem in practice, but the pattern should handle it.

**Prevention:** Implement proper chunked emission in the DESCRIBE VTab: track an offset in `InitData`, emit up to chunk size rows per `func()` call, return `set_len(0)` when offset >= total rows. The existing SHOW VTabs also use single-emission; flag this as tech debt for future scalability.

---

### N2: `#[serde(skip_serializing_if)]` Inconsistency on `created_on`

**What goes wrong:** If you forget `skip_serializing_if` on `created_on` and always serialize `created_on: null`, old definitions re-serialized through `catalog_upsert` will gain the new field. This is harmless (serde handles null gracefully) but creates unnecessary JSON churn in stored definitions.

**Prevention:** Follow the existing pattern in `TableRef` (model.rs lines 13, 19): use `#[serde(default, skip_serializing_if = "Option::is_none")]` for the new field.

---

### N3: FOR METRIC `required` Column Semantics

**What goes wrong:** Snowflake's `SHOW SEMANTIC DIMENSIONS FOR METRIC` includes a `required` boolean column for window function metrics with `PARTITION BY EXCLUDING`. DuckDB semantic views do not support window function metrics yet, so `required` is always `false`. If you add the column now, it must always emit `false`.

**Prevention:** Add `required` as a BOOLEAN column, always emit `false`, add a code comment: "Always false until window function metrics are implemented."

---

### N4: Forgetting to `git rm` Old Files When Creating Module Directories

**What goes wrong:** When converting `expand.rs` to `expand/mod.rs`, if both files exist in the source tree, the Rust compiler emits an ambiguity error. Git may retain the old file if not explicitly removed.

**Prevention:**
1. `git rm src/expand.rs` when creating `src/expand/mod.rs`.
2. `git rm src/graph.rs` when creating `src/graph/mod.rs`.
3. Verify with `ls src/expand*` and `ls src/graph*` that no orphan files remain.

---

### N5: Renaming `source_table` to `table_name` in Output but Not Struct Field

**What goes wrong:** Snowflake uses `table_name` where the current code uses `source_table` as both the struct field name AND the output column name. If you rename only the output column (in `bind_output_columns`) but not the struct field, the code works but the naming inconsistency confuses contributors.

**Prevention:** Rename the output column to `table_name` to match Snowflake. Keep the internal struct field name (`source_table`) because it matches the DDL syntax. Add a comment explaining the mapping.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Extract util.rs + errors.rs | M1 (circular deps), M4 (error type order) | Do this FIRST before any module splits. Single commit, verify with `cargo test`. |
| Split expand.rs into expand/ | C4 (import path breakage), M5 (test migration) | Re-export everything from mod.rs. Move tests with functions. Run `cargo test` after each file move. |
| Split graph.rs into graph/ | C4 (same pattern) | Smaller surface area than expand. Same re-export strategy. |
| Add created_on to model | C2 (JSON backward compat) | `Option<String>` with `#[serde(default)]`. Test with old JSON fixtures. Store in JSON, not SQL column. |
| Add database_name/schema_name | M2 (connection access deadlock) | Query at init time, store in enriched state struct. |
| SHOW SEMANTIC VIEWS column changes | C1 (test sync), M3 (column removal) | Atomic VTab + test update. Document breaking change. |
| SHOW SEMANTIC DIMS/METRICS/FACTS changes | C1 (test sync), M3 (same) | All three commands updated together for consistency. |
| DESCRIBE restructuring | C3 (complete rewrite) | Discrete phase. New VTab from scratch. |
| SHOW DIMS FOR METRIC required column | N3 (always false) | Add column, emit false, comment the limitation. |

## Sources

- Snowflake [SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- output columns: created_on, name, kind, database_name, schema_name, comment, owner, owner_role_type (HIGH confidence)
- Snowflake [DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- property-per-row format: object_kind, object_name, parent_entity, property, property_value (HIGH confidence)
- Snowflake [SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) -- columns: database_name, schema_name, semantic_view_name, table_name, name, data_type, synonyms, comment (HIGH confidence)
- Snowflake [SHOW SEMANTIC DIMENSIONS FOR METRIC](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric) -- adds `required` boolean column (HIGH confidence)
- Snowflake [SHOW SEMANTIC METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics) -- same 8-column schema as SHOW SEMANTIC DIMENSIONS (HIGH confidence)
- Snowflake [SHOW SEMANTIC FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts) -- same 8-column schema as SHOW SEMANTIC DIMENSIONS (HIGH confidence)
- Codebase: `cpp/src/shim.cpp` sv_ddl_bind (lines 134-201) -- dynamic column forwarding (HIGH confidence, direct code inspection)
- Codebase: `src/model.rs` SemanticViewDefinition serde attributes (HIGH confidence, direct code inspection)
- Codebase: `src/catalog.rs` init_catalog, CatalogState type alias (HIGH confidence, direct code inspection)
- Codebase: `_notes/architecture.md` C1-C6 refactoring proposals (HIGH confidence, project documentation)
- Codebase: TECH-DEBT.md item 12 -- DDL pipeline all-VARCHAR forwarding (HIGH confidence, project documentation)
