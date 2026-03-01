# Phase 11: CREATE SEMANTIC VIEW Parser Hook - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement native SQL DDL syntax for creating and dropping semantic views via the DuckDB C++ parser extension mechanism. Users gain `CREATE SEMANTIC VIEW` / `DROP SEMANTIC VIEW` SQL. The scalar function API (`define_semantic_view`, `drop_semantic_view`) is removed. The existing pragma persistence path (Phase 10) is reused by the parser hook.

</domain>

<decisions>
## Implementation Decisions

### DDL Syntax — Snowflake-compatible

The DDL follows Snowflake's `CREATE SEMANTIC VIEW` syntax as closely as our model permits.

Full clause structure:
```sql
CREATE [OR REPLACE] SEMANTIC VIEW [IF NOT EXISTS] <name>
  TABLES (
    alias AS physical_table [PRIMARY KEY (col [, ...])]
    [, ...]
  )
  [RELATIONSHIPS (
    from_alias(fk_col [, ...]) REFERENCES ref_alias
    [, ...]
  )]
  [FACTS (
    alias.name AS sql_expr
    [, ...]
  )]
  [DIMENSIONS (
    alias.name AS sql_expr
    [, ...]
  )]
  [METRICS (
    alias.name AS sql_expr
    [, ...]
  )]
```

- **TABLES clause** — all physical tables declared together (base + joined). Table alias is the name used in subsequent clauses.
- **RELATIONSHIPS** — Snowflake FK-style: `from_alias(fk_col) REFERENCES ref_alias`. The parser infers the equi-join condition. No raw SQL ON clause.
- **FACTS** — unaggregated computed values, scoped to a table alias. Add now for Snowflake parity (requires adding `facts: Vec<Fact>` to the model).
- **DIMENSIONS** — SQL expressions, scoped to a table alias. No special TIME GRANULARITY annotation — time grouping is expressed via plain SQL functions (`DATE_TRUNC('month', col)`, `YEAR(col)`).
- **METRICS** — aggregation expressions, scoped to a table alias.
- **FILTERS** — NOT surfaced in DDL. Snowflake has no FILTERS clause. Existing JSON definitions that have filters continue to work; new DDL definitions simply don't set filters.
- `DROP SEMANTIC VIEW [IF EXISTS] <name>` — mirrors DuckDB DDL convention.

### Legacy Function Removal

- `define_semantic_view()` and `drop_semantic_view()` scalar functions **removed in this phase** (DDL-05). Hard removal — no deprecation period.
- `src/ddl/define.rs` and `src/ddl/drop.rs` are deleted.
- Registration of those functions in `lib.rs` `init_extension` is removed.
- Existing JSON definitions in `semantic_layer._definitions` are **kept as-is** — the model is backwards-compatible; old definitions load and work.
- PRAGMA callbacks (`define_semantic_view_internal`, `drop_semantic_view_internal`) **remain registered** — used internally by the parser hook, not advertised but available as escape hatch.

### Error Messaging

- **Syntax errors** — standard DuckDB parser error style. No custom descriptive messages.
- **Success** — silent, no output rows. Matches DuckDB `CREATE TABLE` / `CREATE VIEW` convention.
- **DROP on non-existent view** — error by default; `DROP SEMANTIC VIEW IF EXISTS name` succeeds silently.
- **Parse-time validation** — unknown table aliases in DIMENSIONS/METRICS/FACTS (not declared in TABLES) are caught at parse time, not deferred to query time.

### Claude's Discretion

- Connection strategy for the parser hook (whether to call PRAGMA via `persist_conn` or use a different mechanism — DuckDB parser extensions run in a different context than scalar invoke; researcher should determine the correct approach).
- How to determine the "base table" from the TABLES clause (Snowflake has no concept of a base table; our model requires one — researcher should determine the right convention, e.g., first table declared, or the table not referenced as a REFERENCES target).

</decisions>

<specifics>
## Specific Ideas

- Target syntax is Snowflake's `CREATE SEMANTIC VIEW` as documented at https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view
- The user's primary mental model is Snowflake's BI semantic layer
- Divergences from Snowflake accepted so far: no COMMENT, no SYNONYMS, no AI_SQL_GENERATION, no PRIVATE/PUBLIC visibility modifiers, no window function metrics (all deferred/out of scope)

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets

- `src/shim/shim.cpp` — already imports `duckdb/parser/parser_extension.hpp` (Phase 8). `semantic_views_register_shim()` is the registration entry point — Phase 11 adds parser hook registration here alongside the pragma callbacks.
- `src/catalog.rs` — `catalog_insert()` / `catalog_delete()` are the in-memory catalog mutation functions. The parser hook must call these after persisting.
- PRAGMA callbacks (`PragmaDefineSemanticView`, `PragmaDropSemanticView` in `shim.cpp`) — existing persist path. The parser hook routes through these.
- `src/shim/mod.rs` — Rust FFI declarations for shim functions. Any new C++ functions called from Rust need entries here.

### Established Patterns

- `src/model.rs` — `SemanticViewDefinition` is the JSON serialization model. **Needs changes for Phase 11:**
  - `Join { table, on }` struct needs updating: `on: String` (raw SQL) → FK pair representation (`from_cols: Vec<String>`, `to_table: String`) to match REFERENCES syntax.
  - New `Fact { name, expr, source_table }` struct needed for FACTS clause.
  - `filters: Vec<String>` field stays in model (for backwards compat) but is not populated by the DDL parser.
- JSON stored in `semantic_layer._definitions` — existing definitions must deserialize correctly after model changes (serde `#[serde(default)]` and rename guards already in place).
- The Rust model is the source of truth; the C++ parser hook parses DDL → builds a JSON blob → calls the PRAGMA to persist it.

### Integration Points

- `lib.rs` `init_extension()` — remove `DefineSemanticView` and `DropSemanticView` scalar function registrations; the shim registration call (`semantic_views_register_shim`) already exists and will now register the parser hook in addition to the PRAGMAs.
- `build.rs` — C++ shim already compiled with `extension` feature; no build changes expected.
- Test suite — any tests that use `define_semantic_view()` / `drop_semantic_view()` must be rewritten to use native DDL syntax.

</code_context>

<deferred>
## Deferred Ideas

- COMMENT, SYNONYMS, AI_SQL_GENERATION, AI_QUESTION_CATEGORIZATION clauses (Snowflake features not needed for v0.2.0)
- PRIVATE/PUBLIC visibility modifiers on FACTS and METRICS
- Window function METRICS (`AS metric OVER (PARTITION BY ...)`)
- ASOF join support in RELATIONSHIPS (temporal matching)
- Derived metrics (view-level metrics that reference other metrics without table scoping)
- FILTERS in DDL — not surfaced (Snowflake has no FILTERS clause)

</deferred>

---

*Phase: 11-create-semantic-view-parser-hook*
*Context gathered: 2026-03-01*
