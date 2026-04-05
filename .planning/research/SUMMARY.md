# Project Research Summary

**Project:** DuckDB Semantic Views v0.5.5 — SHOW/DESCRIBE Alignment & Refactoring
**Domain:** DuckDB Rust extension — Snowflake output format parity + module decomposition
**Researched:** 2026-04-01
**Confidence:** HIGH

## Executive Summary

v0.5.5 has two independent work streams that must be sequenced carefully but share no runtime coupling. The first is Snowflake output format alignment: all 6 SHOW/DESCRIBE commands currently diverge from Snowflake's documented column schemas. DESCRIBE must be completely rewritten from a single-row JSON-blob format to a property-per-row format (5 columns, N rows). SHOW commands need additional metadata columns (`database_name`, `schema_name`, `created_on`, `kind`) and must drop `expr` (not exposed by Snowflake). A `created_on` timestamp and database/schema context must be stored at define time and surfaced at query time. The second work stream is module refactoring: `expand.rs` (4,440 lines) and `graph.rs` (2,333 lines) must be split into module directories to resolve circular dependencies and establish single-responsibility boundaries ahead of a planned `graph/` extraction into a standalone PyO3/Maturin crate.

The recommended approach is strict phase sequencing: refactoring before output format changes, utilities extraction before module splitting, and metadata storage before SHOW column changes. This ordering ensures each phase is either behavior-preserving (and independently testable with the full suite) or has a clearly scoped set of intentional test breakage. The DESCRIBE rewrite is the highest-risk change — it replaces a fundamentally different output structure — and should come last among the SHOW/DESCRIBE changes so that `expr` values are already moved to DESCRIBE by the time SHOW commands drop them. No new dependencies are required; all capabilities exist in the current stack (`serde_json`, the `execute_sql_raw` pattern, and `#[serde(default)]`).

The primary risk is test suite synchronization: the C++ shim propagates VTab schema changes transparently but sqllogictest assertions are column-count rigid. Every VTab schema change must update all affected `.test` files in the same commit. A secondary risk is backward-compatibility of stored JSON: `created_on`, `database_name`, and `schema_name` must use `Option<String>` with `#[serde(default)]` to avoid runtime panics on pre-v0.5.5 views. Both risks have clear prevention strategies with direct precedents in the existing codebase.

---

## Key Findings

### Recommended Stack

No new crates are required for v0.5.5. The existing stack handles everything: `serde_json` for JSON model changes, the `execute_sql_raw` pattern for DuckDB timestamp and context queries at init time, and standard Rust module reorganization (file moves only). One discrepancy exists between research files on timestamp source: FEATURES.md mentions `std::time::SystemTime` while STACK.md recommends capturing `now()` via DuckDB SQL (`execute_sql_raw`). STACK.md is correct — DuckDB's `now()` gives transaction time rather than OS time and avoids a `chrono` dependency.

**Core technologies:**
- `serde` / `serde_json` 1.x: JSON model changes — `#[serde(default)]` pattern already proven for backward-compatible field additions in this codebase
- `execute_sql_raw` + `duckdb_value_varchar`: timestamp and context retrieval — already used in `define.rs` for PK resolution; same pattern extends to `now()` capture
- Rust module system (`mod.rs` directory style): module decomposition — consistent with existing `ddl/mod.rs` and `query/mod.rs` patterns in the codebase

### Expected Features

All table-stakes features are well-defined with exact Snowflake documentation references. Output schemas are verified from official Snowflake docs for all 6 commands. See [FEATURES.md](FEATURES.md) for full column-by-column analysis.

**Must have (table stakes):**
- T1: DESCRIBE property-per-row (5 columns, N rows per view) — Snowflake alignment; enables SQL `WHERE object_kind = 'DIMENSION'` filtering on output
- T2: SHOW SEMANTIC VIEWS expanded schema (created_on, name, kind, database_name, schema_name) — replaces 2-column stub
- T3: SHOW DIMS/METRICS/FACTS aligned schema (6 columns: db, schema, view, table, name, data_type) — drops `expr`, renames `source_table` to `table_name`
- T4: SHOW DIMS FOR METRIC with `required` column — constant FALSE; window metrics not yet supported
- T5: Metadata storage for `created_on`, `database_name`, `schema_name` — prerequisite for T2 and T3
- T6: Module directory refactoring (expand/ and graph/) — debt retirement; establishes boundaries for future PyO3 extraction

**Should have (differentiators):**
- D1: Hierarchies in DESCRIBE output — already supported in model but invisible in all introspection commands; low effort to fold into T1
- D3: Lexicographic sort by (database, schema, name) — trivially correct for single-db scenario; can be folded into T2/T3 at no cost

**Defer (v0.5.6+):**
- D2: SHOW TERSE SEMANTIC VIEWS — parser complexity for marginal scripting benefit
- `comment` / `synonyms` columns with real values — no DDL support yet; omit entirely per user decision (not even NULL placeholders)
- `owner` / `owner_role_type` columns — Snowflake-specific RBAC; no DuckDB equivalent
- `IN ACCOUNT / IN DATABASE` scoping on SHOW — single-database extension; not applicable

### Architecture Approach

The two work streams are architecturally independent: output format changes touch `ddl/` VTab files and the catalog/model layer; module refactoring touches `expand.rs`, `graph.rs`, and their import sites. The recommended build order runs all refactoring phases first (behavior-preserving, test suite stays stable) then all output format phases (intentional test breakage is isolated and predictable). See [ARCHITECTURE.md](ARCHITECTURE.md) for full component boundary diagrams and the 8-phase build order.

**Major components and responsibilities:**
1. `src/util.rs` (NEW) — leaf module: `suggest_closest`, `replace_word_boundary`; breaks the expand/graph circular dependency
2. `src/errors.rs` (NEW) — leaf module: `ParseError`; breaks the parse/body_parser circular dependency
3. `src/expand/` (replaces expand.rs) — 8 submodules; `mod.rs` re-exports the full prior public API unchanged
4. `src/graph/` (replaces graph.rs) — 5 submodules; `mod.rs` re-exports `RelationshipGraph` and all public items
5. `SemanticViewDefinition` model — 3 new `Option<String>` fields with `#[serde(default)]`; zero catalog schema migration needed
6. `ShowState` struct — combines `CatalogState` + init-time db/schema context; injected into SHOW VTabs via `extra_info`
7. `src/ddl/describe.rs` — complete rewrite using a `Vec<DescribeRow>` pattern; replaces single-row JSON blobs

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for the full taxonomy (4 critical, 5 moderate, 5 minor).

1. **C1: sqllogictest schema rigidity with transparent C++ shim** — the shim propagates column count/name changes automatically, but every `.test` file asserting SHOW/DESCRIBE output uses a column-count-sensitive `query TTTTT` prefix. Schema changes and test updates must be atomic. Affected files: `phase34_1_show_commands.test`, `phase34_1_show_filtering.test`, `phase34_1_show_dims_for_metric.test`, `phase20_extended_ddl.test`. Run `just test-all` (not just `cargo test`) after every schema change.

2. **C2: `created_on` backward-compat JSON deserialization** — pre-v0.5.5 stored definitions lack `created_on`. Any `.unwrap()` on `Option<String>` panics at runtime on old views. Use `#[serde(default, skip_serializing_if = "Option::is_none")]`. Render `None` as empty string in VTab output. No schema migration needed; old views show blank timestamps.

3. **C3: DESCRIBE is a complete rewrite, not incremental** — the single-row 6-column format cannot be migrated to property-per-row without replacing the entire VTab. `DescribeSemanticViewVTab`, `DescribeBindData`, `DescribeInitData`, and all test expectations must be rebuilt from scratch. Isolate the logic in a unit-testable `collect_describe_rows(&SemanticViewDefinition) -> Vec<DescribeRow>` helper before touching the VTab machinery.

4. **C4: Module splitting breaks import paths across 10+ files** — `expand::suggest_closest`, `expand::QueryRequest`, `expand::expand()` are consumed by `graph.rs`, `query/table_function.rs`, `query/explain.rs`, and `query/error.rs`. All must still resolve after splitting. Mitigation: `expand/mod.rs` must `pub use` the full prior API surface before changing any callers. Extract `util.rs` first to break the circular dependency before splitting expand.

5. **M2: Database/schema name retrieval deadlocks if called from VTab bind()** — the ClientContext lock is held during bind. Calling `duckdb_query` on the main connection from within `bind()` deadlocks silently. Query `current_database()` / `current_schema()` once at extension init time, cache in a `ShowState` struct, inject via `extra_info`. Never execute SQL from a VTab bind callback.

---

## Implications for Roadmap

Based on combined research, the natural phase structure is 8 phases across 2 work streams. Refactoring phases complete first to keep behavior-preserving changes distinct from intentional test breakage.

### Phase 1: Extract Shared Utilities (util.rs + errors.rs)

**Rationale:** Breaks both circular dependencies before any module splitting. Smallest change with highest leverage for subsequent phases. Must be first.
**Delivers:** `src/util.rs` (suggest_closest, replace_word_boundary) and `src/errors.rs` (ParseError). All 482 existing tests pass. Zero behavior change.
**Addresses:** Prerequisite for Phases 2 and 3
**Avoids:** M1 (circular dep during refactoring), M4 (ParseError extraction order)

### Phase 2: Split expand.rs into expand/ Module Directory

**Rationale:** Largest refactoring item; do before graph/ because expand has more external consumers. Behavior-preserving; test suite validates correctness.
**Delivers:** `src/expand/` with 8 submodules (validate, resolve, facts, fan_trap, role_playing, join_resolver, sql_gen, mod.rs). Public API unchanged at `crate::expand::*`. 99 embedded tests migrate with their functions.
**Addresses:** T6 (partial — expand half)
**Avoids:** C4 (full re-exports in mod.rs preserve all import paths), M5 (tests migrate with functions, not to a central tests.rs)

### Phase 3: Split graph.rs into graph/ Module Directory

**Rationale:** Simpler than expand/ split (fewer external consumers); completes the refactoring work stream cleanly.
**Delivers:** `src/graph/` with 5 submodules (relationship, facts, derived_metrics, using, mod.rs). Public API unchanged.
**Addresses:** T6 (completes module refactoring)
**Avoids:** C4 (same re-export strategy), N4 (git rm expand.rs / graph.rs to avoid Rust ambiguity error)

### Phase 4: Catalog Metadata Storage (created_on, database_name, schema_name)

**Rationale:** Foundational prerequisite for all SHOW column changes; pure additive model change. No catalog schema migration required.
**Delivers:** 3 new `Option<String>` fields in `SemanticViewDefinition`; timestamp captured at define time via `execute_sql_raw("SELECT now()::VARCHAR")`; `ShowState` struct for VTab injection; `Fact.output_type` field added (needed by Phase 6).
**Addresses:** T5; unblocks T2, T3
**Avoids:** C2 (backward-compat via serde(default)); M2 (init-time cache, not bind-time query)

### Phase 5: SHOW SEMANTIC VIEWS Column Expansion

**Rationale:** Simplest SHOW command (list.rs, 101 lines); establishes the ShowState injection pattern and atomic VTab+test update discipline for subsequent phases.
**Delivers:** `list.rs` rewritten to 5-column output (created_on, name, kind, database_name, schema_name). `base_table` column dropped.
**Addresses:** T2
**Avoids:** C1 (atomic VTab + test file update in same commit)

### Phase 6: SHOW DIMS/METRICS/FACTS Column Alignment

**Rationale:** Three structurally identical commands updated together; follows ShowState pattern established in Phase 5.
**Delivers:** All three SHOW commands at 6-column output (database_name, schema_name, semantic_view_name, table_name, name, data_type). `expr` dropped. `source_table` renamed to `table_name`. Facts gain `data_type` via `Fact.output_type` field added in Phase 4.
**Addresses:** T3
**Avoids:** C1 (all three .test files updated in same commit), M3 (expr removal documented as breaking change)

### Phase 7: SHOW DIMS FOR METRIC Column Alignment

**Rationale:** Most nuanced SHOW change due to `required` column semantics; isolated phase allows clean reasoning about the design decision.
**Delivers:** `show_dims_for_metric.rs` at 4-column output (table_name, name, data_type, required). `required` constant FALSE with explanatory comment.
**Addresses:** T4
**Avoids:** N3 (required always false — honest given no window metric support), C1 (test sync)

### Phase 8: DESCRIBE SEMANTIC VIEW Complete Rewrite

**Rationale:** Most radical change (single-row to N-row property-per-row); placed last so DESCRIBE becomes the canonical surface for `expr` values after SHOW has already dropped them. Highest test impact.
**Delivers:** `describe.rs` fully rewritten to 5-column property-per-row format. `collect_describe_rows()` helper unit-tested independently. D1 (hierarchies in DESCRIBE) folded in if scope allows.
**Addresses:** T1; optionally D1
**Avoids:** C3 (clean replacement — build new VTab from scratch, do not edit the existing one), C1 (comprehensive new sqllogictest cases cover all object_kind variants)

### Phase Ordering Rationale

- **Phases 1-3 complete before Phases 4-8:** Refactoring is behavior-preserving; failing tests during this work stream indicate mistakes, not intentional changes. Output format phases intentionally break test expectations. Mixing the streams makes failures ambiguous.
- **Phase 4 gates Phases 5-6:** `created_on`, `database_name`, `schema_name` must exist in the model before SHOW VTabs can emit them.
- **Phase 5 before Phase 6:** Simpler command (1 file vs 3 files) establishes the ShowState injection pattern and validates the discipline before tackling three parallel changes.
- **Phase 7 isolated:** The `required` column semantics are a design decision (constant FALSE) that could become non-trivial when window metrics are added. Keeping it separate makes that future change easy to locate.
- **Phase 8 last:** (a) Highest risk — complete rewrite of the only multi-row introspection command. (b) `expr` values must already be absent from SHOW output before DESCRIBE becomes the canonical source for them. If DESCRIBE is done first, there is a window where neither surface exposes expressions.

### Research Flags

Phases with standard patterns (research-phase not needed):
- **Phase 1:** Pure file extraction; standard Rust module pattern
- **Phase 2:** `ddl/mod.rs` is the in-codebase template; same technique at larger scale
- **Phase 3:** Same pattern as Phase 2; smaller surface area
- **Phase 4:** `#[serde(default)]` backward-compat already proven for `pk_columns`, `column_type_names`, `unique_constraints`
- **Phases 5-7:** Column schemas verified from Snowflake official docs; ShowState injection pattern established in Phase 5

Phases that warrant detailed task-level design before coding:
- **Phase 8:** DESCRIBE rewrite is novel territory. The `collect_describe_rows` helper design needs a decision on: NULL vs empty-string for `object_kind`/`parent_entity`, canonical property row ordering, how HIERARCHY objects are represented, and whether D1 (hierarchies) is included. Design the schema and write sqllogictest expectations first, then implement.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | No new dependencies. All patterns (`execute_sql_raw`, `serde(default)`, `mod.rs` directories) verified against existing codebase. |
| Features | HIGH | All 6 Snowflake command schemas verified against official docs. User decisions (no NULL placeholders, no RBAC columns) explicitly captured in FEATURES.md. |
| Architecture | HIGH | Phase ordering and component boundaries derived from direct codebase analysis. One minor discrepancy on timestamp approach between files — STACK.md's DuckDB `now()` approach is authoritative. |
| Pitfalls | HIGH | All pitfalls derived from the existing codebase: shim.cpp schema forwarding behavior, `serde(default)` usage in model.rs, `execute_sql_raw` deadlock risk. Verified by direct code inspection. |

**Overall confidence:** HIGH

### Gaps to Address

- **`database_name` storage vs runtime retrieval:** STACK.md recommends init-time caching in `ShowState`; FEATURES.md and ARCHITECTURE.md also recommend init-time caching; all three files agree. However, ARCHITECTURE.md also notes "store in JSON at define time" as an alternative. The correct choice is init-time caching: stored DB names become wrong if a `.duckdb` file is re-attached under a different alias. Confirm this explicitly in Phase 4 planning.

- **Fact `output_type` field:** Adding `data_type` to SHOW SEMANTIC FACTS (T3, Phase 6) requires a new `output_type: Option<String>` on the `Fact` struct and type inference updates. This is flagged in FEATURES.md but has no detailed design in ARCHITECTURE.md. Phase 4 planning (where the field is added) should include the type inference path for facts.

- **DESCRIBE row ordering:** FEATURES.md specifies ordering as: view-level, TABLE, RELATIONSHIP, DIMENSION, FACT, METRIC. ARCHITECTURE.md specifies the same order but with HIERARCHY inserted. Confirm the canonical ordering and encode it in sqllogictest assertions before Phase 8 coding begins.

- **D1 (hierarchies in DESCRIBE) scope decision:** Research recommends folding D1 into Phase 8 (~30 LOC estimate, low risk). This is not in Snowflake's schema. Confirm during Phase 8 planning whether to include or defer to v0.5.6.

---

## Sources

### Primary (HIGH confidence)

- [Snowflake DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) — 5-column property-per-row format, object_kind/property combinations, parent_entity rules
- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) — 8-column schema (created_on, name, kind, database_name, schema_name, comment, owner, owner_role_type)
- [Snowflake SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) — 8-column schema
- [Snowflake SHOW SEMANTIC METRICS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-metrics) — same 8-column schema as dimensions
- [Snowflake SHOW SEMANTIC FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts) — same 8-column schema; facts include data_type
- [Snowflake SHOW SEMANTIC DIMENSIONS FOR METRIC](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions-for-metric) — 6-column schema; `required` semantics for window metrics with PARTITION BY EXCLUDING
- [DuckDB Timestamp functions](https://duckdb.org/docs/current/sql/functions/timestamptz) — `now()` returns TIMESTAMPTZ, `duckdb_value_varchar` renders as human-readable string
- [DuckDB Utility functions](https://duckdb.org/docs/stable/sql/functions/utility.md) — `current_database()`, `current_schema()`
- [Rust module system](https://doc.rust-lang.org/book/ch07-05-separating-modules-into-different-files.html) — `mod.rs` directory pattern, edition 2021 support confirmed

### Secondary (HIGH confidence — direct codebase analysis)

- `src/ddl/describe.rs` — current 6-column single-row VTab (146 lines)
- `src/ddl/list.rs` — current 2-column ListSemanticViewsVTab (101 lines)
- `src/ddl/show_dims.rs`, `show_metrics.rs`, `show_facts.rs`, `show_dims_for_metric.rs` — current 4-5 column SHOW VTab implementations
- `src/expand.rs` — 4,440 lines; circular dependency with graph.rs via `suggest_closest`
- `src/graph.rs` — 2,333 lines; circular dependency with expand.rs
- `src/model.rs` — SemanticViewDefinition serde attributes; `#[serde(default)]` pattern on pk_columns, column_type_names, unique_constraints
- `src/catalog.rs` — CatalogState (HashMap<String, String>), init_catalog, catalog_insert
- `src/ddl/define.rs` — existing `execute_sql_raw` + `current_database()` usage at line 113
- `cpp/src/shim.cpp` sv_ddl_bind (lines 134-201) — dynamic all-VARCHAR column forwarding; schema-agnostic
- `_notes/architecture.md` — refactoring proposals C1-C6 with exact file decomposition targets
- `TECH-DEBT.md` item 12 — DDL pipeline all-VARCHAR forwarding

---
*Research completed: 2026-04-01*
*Ready for roadmap: yes*
