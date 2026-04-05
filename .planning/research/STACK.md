# Technology Stack

**Project:** DuckDB Semantic Views v0.5.5 -- SHOW/DESCRIBE Alignment & Refactoring
**Researched:** 2026-04-01
**Scope:** Stack additions/changes for Snowflake-aligned output formats + module refactoring

## Key Finding: No New Dependencies Required

This milestone requires **zero new crates**. All capabilities are available through the existing stack:

- Timestamps: DuckDB's `now()` via SQL, stored as VARCHAR in JSON, rendered as VARCHAR in VTab output
- Database/schema names: DuckDB's `current_database()` / `current_schema()` via existing `execute_sql_raw` pattern
- Module refactoring: Pure Rust file reorganization, no library involvement
- DESCRIBE restructuring: serde_json (already a dependency) for JSON-to-row flattening

## Current Stack (Unchanged)

### Core Dependencies
| Technology | Version | Purpose | Status for v0.5.5 |
|------------|---------|---------|-------------------|
| duckdb (Rust) | =1.10500.0 | DuckDB C API bindings, VTab trait, `LogicalTypeId` | No change needed |
| libduckdb-sys | =1.10500.0 | Raw FFI bindings (`duckdb_query`, `duckdb_value_varchar`) | No change needed |
| serde | 1.x | Derive `Serialize`/`Deserialize` on model types | No change needed |
| serde_json | 1.x | JSON serialization of `SemanticViewDefinition` | No change needed -- used more heavily for DESCRIBE restructuring |
| strsim | 0.11 | Levenshtein distance for "did you mean?" suggestions | No change needed; moves to `util.rs` during refactor |
| cc | 1.x (optional) | C++ shim compilation for extension builds | No change needed |

### Dev Dependencies
| Technology | Version | Purpose | Status for v0.5.5 |
|------------|---------|---------|-------------------|
| proptest | 1.11 | Property-based testing | No change needed |
| cargo-husky | 1.x | Git hooks | No change needed |

## What's Needed and How It's Addressed

### 1. Storing `created_on` Timestamps

**Need:** SHOW SEMANTIC VIEWS must include a `created_on` column matching Snowflake's format.

**Approach:** Store as ISO 8601 VARCHAR string in the `SemanticViewDefinition` JSON.

**Why VARCHAR, not a timestamp type:**
- The timestamp is **metadata about the definition**, not query data. It flows through the JSON persistence layer (`semantic_layer._definitions.definition` column), not the typed query pipeline.
- The VTab output for SHOW commands already uses all-VARCHAR columns (established pattern in `list.rs`, `show_dims.rs`, `show_metrics.rs`, `show_facts.rs`). Adding one more VARCHAR column is consistent.
- DuckDB's `now()` returns `TIMESTAMPTZ` which `duckdb_value_varchar` renders as a human-readable string (e.g., `2026-04-01 10:30:00.000-07`). We capture this at define time and store it verbatim.
- No `chrono` crate needed. No `std::time::SystemTime` needed. No C API timestamp conversion functions needed.

**Implementation pattern (uses existing `execute_sql_raw`):**
```rust
// In define.rs bind(), before serializing the definition:
let timestamp_sql = "SELECT now()::VARCHAR";
let ts = unsafe { execute_sql_raw(state.catalog_conn, timestamp_sql) };
// Extract varchar from row 0, col 0 with duckdb_value_varchar
def.created_on = Some(timestamp_string);
```

**Model change:**
```rust
// In model.rs, add to SemanticViewDefinition:
#[serde(default)]
pub created_on: Option<String>,
```

The `Option<String>` with `#[serde(default)]` ensures backward-compatible deserialization of existing stored definitions (they get `None`, displayed as empty string in SHOW output).

**Confidence:** HIGH -- `execute_sql_raw` + `duckdb_value_varchar` is the exact pattern already used in `define.rs` for PK resolution via `duckdb_constraints()`. DuckDB's `now()` is documented to return `TIMESTAMPTZ` (verified: [DuckDB timestamptz docs](https://duckdb.org/docs/current/sql/functions/timestamptz)).

### 2. Retrieving `database_name` and `schema_name`

**Need:** SHOW SEMANTIC VIEWS/DIMENSIONS/METRICS/FACTS must include `database_name` and `schema_name` columns.

**Approach:** Query at runtime via `SELECT current_database(), current_schema()` using existing `execute_sql_raw`.

**Why runtime, not stored:**
- Database and schema names are **context-dependent**. The same `.duckdb` file can be attached under different names. Storing the name at define time would be wrong if the database is later attached with a different alias.
- `current_database()` returns `'memory'` for in-memory databases and the database name for file-backed databases. `current_schema()` returns `'main'` by default. Both are documented DuckDB utility functions (verified: [DuckDB utility functions](https://duckdb.org/docs/stable/sql/functions/utility.md)).
- The existing code already uses `current_database()` in `define.rs:113` for catalog PK lookups, confirming this pattern works within the extension.

**Connection choice:** Use `catalog_conn` (available in `DefineState`) for SHOW commands that access it via `extra_info`. For VTab functions that only have `CatalogState`, either:
- (a) Cache the values at extension init time and inject alongside `CatalogState`, or
- (b) Add a `catalog_conn` to the SHOW VTabs' extra_info (same pattern as `DefineState`)

**Recommendation: option (a).** Query `current_database()` / `current_schema()` once at init time, store as a `DbContext { database_name: String, schema_name: String }` struct alongside the `CatalogState` Arc. This avoids adding a raw connection handle to every SHOW VTab. The values don't change during an extension session.

**Injection mechanism:** Create a shared state struct that combines `CatalogState` + `DbContext`:
```rust
pub struct ShowState {
    pub catalog: CatalogState,
    pub database_name: String,
    pub schema_name: String,
}
```
Register SHOW VTabs with `ShowState` as extra_info instead of bare `CatalogState`.

**Confidence:** HIGH -- `current_database()` and `current_schema()` are already used in this codebase (`define.rs:113`).

### 3. DESCRIBE Restructuring to Property-Per-Row Format

**Need:** DESCRIBE SEMANTIC VIEW must output Snowflake's 5-column format: `(object_kind, object_name, parent_entity, property, property_value)`.

**Approach:** Iterate over the deserialized `SemanticViewDefinition` and emit one row per property.

**No new technology needed:**
- `serde_json::Value` is already used in `describe.rs:81` for parsing stored JSON
- The new format is just a different row-emission pattern in the VTab `func()` callback
- Each TABLE, DIMENSION, METRIC, FACT, RELATIONSHIP becomes multiple rows (one per property)

**Snowflake property rows (target from [DESCRIBE docs](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view)):**

| object_kind | object_name | parent_entity | property | property_value |
|-------------|-------------|---------------|----------|----------------|
| NULL | NULL | NULL | COMMENT | (empty) |
| TABLE | orders | NULL | BASE_TABLE_NAME | orders |
| TABLE | orders | NULL | PRIMARY_KEY | id |
| DIMENSION | region | orders | EXPRESSION | o.region |
| DIMENSION | region | orders | DATA_TYPE | VARCHAR |
| METRIC | revenue | orders | EXPRESSION | SUM(o.amount) |
| METRIC | revenue | orders | DATA_TYPE | DOUBLE |
| FACT | amount | orders | EXPRESSION | o.amount |
| RELATIONSHIP | orders_customers | NULL | TABLE | customers |
| RELATIONSHIP | orders_customers | NULL | FOREIGN_KEY | customer_id |

**Row count estimate:** A typical semantic view with 3 tables, 5 dimensions, 3 metrics, 2 facts, 2 relationships produces ~35-45 rows. DuckDB VTab output chunks are 2048 rows, so this fits in one chunk. No pagination logic needed.

**Confidence:** HIGH -- pure Rust iteration over existing model types. No API calls needed.

### 4. SHOW SEMANTIC DIMENSIONS/METRICS/FACTS Column Schema Changes

**Need:** Align output columns with Snowflake's schema.

**Snowflake target columns (verified from [SHOW DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions), [SHOW METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics), [SHOW FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts)):**
- SHOW SEMANTIC DIMENSIONS: `database_name, schema_name, semantic_view_name, table_name, name, data_type, synonyms, comment`
- SHOW SEMANTIC METRICS: same column set
- SHOW SEMANTIC FACTS: same column set
- SHOW SEMANTIC DIMENSIONS FOR METRIC ([docs](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric)): `table_name, name, data_type, required, synonyms, comment`

**Changes from current schema:**

| Change | Current | Target | Impact |
|--------|---------|--------|--------|
| ADD | -- | `database_name` (col 0) | New column from `ShowState.database_name` |
| ADD | -- | `schema_name` (col 1) | New column from `ShowState.schema_name` |
| KEEP | `semantic_view_name` (col 0) | `semantic_view_name` (col 2) | Position shift |
| RENAME | `source_table` | `table_name` | Aligns with Snowflake naming |
| KEEP | `name` | `name` | Unchanged |
| KEEP | `data_type` | `data_type` | Unchanged |
| ADD | -- | `synonyms` | Empty string (future-proofing) |
| ADD | -- | `comment` | Empty string (future-proofing) |
| REMOVE | `expr` | -- | Snowflake does not expose raw expressions in SHOW |
| ADD (FOR METRIC only) | -- | `required` | Boolean as VARCHAR `'true'`/`'false'` |

**Breaking change note:** Removing `expr` and reordering columns is a breaking change for SHOW output consumers. This is acceptable because the extension is pre-1.0 (`v0.5.x`).

**Confidence:** HIGH -- column schema changes are pure VTab output modifications.

### 5. SHOW SEMANTIC VIEWS Column Schema Changes

**Need:** Expand from 2 columns `(name, base_table)` to Snowflake-aligned schema.

**Snowflake target columns (verified from [SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views)):**

| Column | Source | Notes |
|--------|--------|-------|
| `created_on` | `def.created_on` from JSON | `Option<String>`, empty for pre-v0.5.5 views |
| `name` | HashMap key | Already available |
| `kind` | Literal `"SEMANTIC_VIEW"` | Hardcoded constant |
| `database_name` | `ShowState.database_name` | From init-time cache |
| `schema_name` | `ShowState.schema_name` | From init-time cache |
| `comment` | Empty string | Future-proofing, no comment support yet |

**Snowflake also has:** `owner`, `owner_role_type`, `extension` -- these are Snowflake-specific RBAC columns. We omit them (DuckDB extensions don't have owner/role concepts).

**Confidence:** HIGH.

### 6. Module Refactoring (expand.rs -> expand/, graph.rs -> graph/)

**Need:** Split 4,440-line `expand.rs` and 2,333-line `graph.rs` into module directories.

**Rust module style:** Use `mod.rs` style (not modern `expand.rs` + `expand/` style) because:
- The codebase already uses `mod.rs` for `ddl/mod.rs` and `query/mod.rs`
- Consistency within the project outweighs the general community preference for modern style
- The files are being removed entirely (replaced by directories), not split into a file + directory pair
- Verified: Rust reference confirms `mod.rs` is fully supported in edition 2021 ([Rust module docs](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html))

**File transformations:**
```
BEFORE:                    AFTER:
src/expand.rs (4,440)  ->  src/expand/mod.rs (public API + re-exports)
                            src/expand/validate.rs
                            src/expand/resolve.rs
                            src/expand/facts.rs
                            src/expand/fan_trap.rs
                            src/expand/role_playing.rs
                            src/expand/join_resolver.rs
                            src/expand/sql_gen.rs

src/graph.rs (2,333)   ->  src/graph/mod.rs (RelationshipGraph + re-exports)
                            src/graph/relationship.rs
                            src/graph/facts.rs
                            src/graph/hierarchies.rs
                            src/graph/derived_metrics.rs
                            src/graph/using.rs

NEW:
src/util.rs                 (suggest_closest, replace_word_boundary)
src/errors.rs               (ParseError -- breaks parse<->body_parser cycle)
```

**No `lib.rs` changes needed** for `expand` and `graph` module declarations. `pub mod expand;` resolves to `expand/mod.rs` automatically. Same for `pub mod graph;`. Two new lines added for `pub mod util;` and `pub mod errors;`.

**Dependency cycle resolution:**
- Current: `expand.rs` exports `suggest_closest` -> `graph.rs` imports it. `graph.rs` exports `RelationshipGraph` -> `expand.rs` imports it. Circular dependency.
- Fix: Extract `suggest_closest` and `replace_word_boundary` to new `src/util.rs`. Both `expand/` and `graph/` import from `util`. Clean DAG.
- Current: `body_parser.rs` imports `parse::ParseError`. `parse.rs` imports `body_parser::parse_keyword_body`. Bidirectional.
- Fix: Extract `ParseError` to new `src/errors.rs`. Both `parse.rs` and `body_parser.rs` import from `errors`. Clean DAG.

**Re-export strategy for backward compatibility:** `expand/mod.rs` re-exports all public items so that external callers (`use crate::expand::expand`, `use crate::expand::QueryRequest`) continue to work without path changes. Same for `graph/mod.rs`.

**Confidence:** HIGH -- pure file reorganization. The architecture document (`_notes/architecture.md`) already specifies the exact split. No functional changes.

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Timestamp storage | VARCHAR in JSON via `now()` SQL | `chrono` crate | Adds a dependency for one `now()` call. DuckDB already provides `now()`. Over-engineering. |
| Timestamp storage | VARCHAR in JSON via `now()` SQL | `std::time::SystemTime` | Would give Rust-local time, not DuckDB transaction time. Semantically different. |
| Timestamp storage | VARCHAR in JSON via `now()` SQL | Separate `created_on` column in `_definitions` table | Schema migration needed for the DuckDB catalog table. Storing in JSON avoids ALTER TABLE and is backward-compatible via `serde(default)`. |
| DB/schema retrieval | Init-time cache in `ShowState` | Store at define time | Wrong semantics -- database name can change if `.duckdb` file is re-attached under different name. |
| DB/schema retrieval | Init-time cache in `ShowState` | Per-query SQL call | Unnecessary overhead. DB/schema names don't change during an extension session. |
| DB/schema retrieval | Init-time cache in `ShowState` | Add `catalog_conn` to all SHOW VTabs | More complex wiring; raw connection handles require unsafe. Cache is simpler. |
| Module style | `mod.rs` (directory) | Modern `expand.rs` + `expand/` | Inconsistent with existing `ddl/mod.rs` and `query/mod.rs` pattern in this codebase. |
| DESCRIBE format | Property-per-row | Keep current JSON-blob columns | Doesn't align with Snowflake. JSON blobs are not queryable with SQL WHERE clauses. |
| `expr` in SHOW | Remove (Snowflake alignment) | Keep alongside Snowflake columns | Snowflake doesn't expose `expr` in SHOW; keeping it diverges. DESCRIBE now exposes it via property rows. |

## What NOT to Add

| Technology | Why Not |
|------------|---------|
| `chrono` crate | One timestamp per CREATE -- `now()` via SQL is sufficient |
| `time` crate | Same reason as `chrono` |
| `uuid` crate | No UUID generation needed |
| `thiserror` crate | Error types are simple string-based; extracting `ParseError` to `errors.rs` doesn't require derive macros |
| `anyhow` crate | Extension code needs typed errors, not erased errors |
| Schema migration framework | No schema changes to `_definitions` table -- `created_on` lives inside the JSON column |
| New C API functions | No `duckdb_from_timestamp` or `duckdb_to_timestamp` needed -- VARCHAR storage avoids timestamp type conversion entirely |
| `LogicalTypeId::Timestamp` in SHOW VTabs | All SHOW output columns are VARCHAR (consistent with Snowflake and existing pattern) |

## Catalog Schema: No Migration Required

The `semantic_layer._definitions` table currently has 2 columns: `(name VARCHAR PRIMARY KEY, definition VARCHAR)`. The `created_on` timestamp is stored **inside** the JSON `definition` column as a new field, not as a separate table column. `#[serde(default)]` on the `Option<String>` field ensures backward-compatible deserialization of existing stored definitions.

For the `database_name` and `schema_name` values, they are queried at runtime and never persisted. **No schema migration needed for either feature.**

## Integration Points Summary

| Feature | Existing Code Touched | New Code |
|---------|----------------------|----------|
| `created_on` | `model.rs` (add field), `define.rs` (capture timestamp) | None |
| `database_name`/`schema_name` | `lib.rs` init (query + cache), `list.rs`, `show_dims.rs`, `show_metrics.rs`, `show_facts.rs`, `show_dims_for_metric.rs` (use `ShowState`) | `ShowState` struct (likely in `catalog.rs` or new shared module) |
| DESCRIBE restructure | `describe.rs` (rewrite VTab output) | None |
| SHOW column changes | `list.rs`, `show_dims.rs`, `show_metrics.rs`, `show_facts.rs`, `show_dims_for_metric.rs` | None |
| expand/ split | Delete `expand.rs`, create `expand/` directory | `expand/mod.rs` + 7 submodules |
| graph/ split | Delete `graph.rs`, create `graph/` directory | `graph/mod.rs` + 5 submodules |
| util.rs extraction | Remove `suggest_closest` from `expand.rs`, update imports in `graph.rs` | `src/util.rs` |
| errors.rs extraction | Remove `ParseError` from `parse.rs`, update imports in `body_parser.rs` | `src/errors.rs` |

## Sources

- [DuckDB Timestamp with Time Zone Functions](https://duckdb.org/docs/current/sql/functions/timestamptz) -- `now()`, `current_timestamp`, `get_current_timestamp()` all return TIMESTAMPTZ (HIGH confidence)
- [DuckDB Utility Functions](https://duckdb.org/docs/stable/sql/functions/utility.md) -- `current_database()`, `current_schema()` (HIGH confidence)
- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- output column schema: `created_on, name, kind, database_name, schema_name, comment, owner, owner_role_type, extension` (HIGH confidence)
- [Snowflake DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) -- property-per-row format: `object_kind, object_name, parent_entity, property, property_value` (HIGH confidence)
- [Snowflake SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) -- output: `database_name, schema_name, semantic_view_name, table_name, name, data_type, synonyms, comment` (HIGH confidence)
- [Snowflake SHOW SEMANTIC METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics) -- same column layout as dimensions (HIGH confidence)
- [Snowflake SHOW SEMANTIC FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts) -- same column layout as dimensions (HIGH confidence)
- [Snowflake SHOW SEMANTIC DIMENSIONS FOR METRIC](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric) -- adds `required` boolean column (HIGH confidence)
- [duckdb-rs LogicalTypeId docs](https://docs.rs/duckdb/1.10500.0/duckdb/core/enum.LogicalTypeId.html) -- Timestamp, TimestampTZ variants available but not needed (HIGH confidence)
- [Rust module system -- Separating Modules into Files](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html) -- `mod.rs` directory pattern (HIGH confidence)
- [Architecture notes](_notes/architecture.md) -- existing refactoring proposals C1-C6 (internal, HIGH confidence)
- Existing codebase: `define.rs:113` already uses `current_database()` via SQL (HIGH confidence, verified by reading source)
