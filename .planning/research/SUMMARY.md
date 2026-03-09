# Project Research Summary

**Project:** DuckDB Semantic Views Extension — v0.5.2
**Domain:** DuckDB Rust extension with SQL DDL parser and PK/FK semantic layer
**Researched:** 2026-03-09
**Confidence:** HIGH

## Executive Summary

v0.5.2 is a targeted milestone that upgrades the DDL surface and join model of an existing, working DuckDB extension. The core work is (1) replacing the function-call syntax inside `CREATE SEMANTIC VIEW` bodies with Snowflake-style SQL keyword clauses (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS), and (2) replacing the ON-clause substring-matching join heuristic with explicit PK/FK graph traversal. Both changes are additive — existing stored definitions and function-call DDL must continue working via automatic syntax detection at parse time. Snowflake's `CREATE SEMANTIC VIEW` grammar is the clear reference standard, and the research confirmed the extension's existing parser infrastructure (`scan_clause_keywords`, bracket-depth tracking) is already 80% of the way to the new parser.

The recommended implementation strategy is the **translator approach**: parse the SQL keyword body into a `SemanticViewDefinition` struct, then emit the existing function-call rewrite syntax that flows through the proven `create_semantic_view()` execution path. This keeps the C++ shim untouched, requires zero new Cargo dependencies, and limits the blast radius to `src/parse.rs` (new body parser), `src/model.rs` (two new fields with `#[serde(default)]`), and `src/expand.rs` (dual expansion dispatch). A companion change drops the CTE-based expansion strategy in favor of direct FROM+JOIN SQL for PK/FK definitions — this is the prerequisite for qualified column name support (`alias.column` in expressions) and also resolves two explicit TECH-DEBT items (#6 and #7).

The primary risk is the **CTE-to-direct-expansion migration**, rated Medium-High complexity. The expansion engine touches type inference, backward compatibility, and fan-trap semantics simultaneously. Research recommends: (1) keep the legacy CTE path alive for old definitions via feature detection on the `from_table` field, (2) validate FK graphs are trees (no diamonds, no cycles) at define time — matching Snowflake's own restriction, and (3) document the fan-trap constraint rather than solving it in v0.5.2. Property-based cross-path equivalence tests (SQL keyword DDL vs function-call DDL producing identical JSON) are essential to prevent silent behavioral divergence between the two DDL interfaces.

## Key Findings

### Recommended Stack

Zero new Cargo dependencies are needed. The existing crate inventory (`duckdb`, `libduckdb-sys`, `serde`, `serde_json`, `strsim`, `cc`, `proptest`) is fully sufficient. The DDL body grammar is a small, closed DSL (4 clause keywords, ~200 lines of hand-written Rust) — not arbitrary SQL. `sqlparser-rs` was rejected because it cannot parse `CREATE SEMANTIC VIEW` syntax and would need a custom `Statement` variant fork; the existing bracket-depth scanner is already 80% there. `petgraph` was rejected because the FK join graph has 2–8 nodes; Kahn's topological sort is ~30 lines using `std::collections::HashMap`. Both rejections are HIGH confidence based on direct source inspection and crate API review.

**Core technologies:**
- `duckdb = "=1.4.4"` — version-pinned VTab and parser hook; no change
- `serde` + `serde_json` — model struct serialization; `#[serde(default)]` handles all backward compat for new fields
- `strsim 0.11` — "did you mean" suggestions for clause keywords; already used, no change needed
- `proptest 1.9` — property-based tests for parser roundtrip and cross-path equivalence; already a dev-dep

### Expected Features

The milestone's must-have features are a complete replacement of the DDL body syntax with SQL keywords, plus the PK/FK join engine that makes those declarations semantically meaningful. The differentiators (transitive relationship resolution, composite PKs, multi-column FKs, backward-compatible function syntax) fall out naturally from the core implementation with low additional effort.

**Must have (table stakes):**
- TABLES clause — `alias AS physical_table PRIMARY KEY (col, ...)` — users expect SQL, not `:=` struct literals in a DDL statement
- RELATIONSHIPS clause — `alias(fk_col) REFERENCES other_alias` — makes join semantics explicit; replaces ON-clause heuristic (TECH-DEBT item 6)
- DIMENSIONS clause — `alias.dim_name AS sql_expr` — qualified prefix encodes source table naturally
- METRICS clause — `alias.metric_name AS agg_expr` — same pattern as dimensions
- PK/FK join inference — generates ON clauses from declarations; deterministic and correct
- Qualified column references — `alias.column` in expressions; requires dropping CTE flattening (TECH-DEBT item 7)

**Should have (competitive differentiators):**
- Transitive relationship resolution — `A -> B -> C` auto-includes B when A and C are needed (already partially implemented in `collect_transitive_dependencies`)
- Relationship naming — `name AS from(fk) REFERENCES to(pk)` — informational; parse and store but not required at query time
- Composite PKs and multi-column FKs — natural from `Vec<String>` representation; model already supports `join_columns: Vec<JoinColumn>`
- Backward-compatible function syntax — preserved automatically via syntax detection, no extra work

**Defer (v2+):**
- FACTS clause — model struct exists, parser and expansion can be added in a future milestone
- ASOF / temporal relationships — complex range-join semantics; Snowflake supports in preview
- NON ADDITIVE BY — query-time metric validation required
- Fan-trap deduplication — Cube-style pre-aggregation subqueries; significant expansion complexity
- Role-playing dimensions (same physical table joined multiple times via different FK columns)
- Derived metrics, dimensional hierarchies, qualified query-time names

### Architecture Approach

The architecture is a translator pipeline: SQL keyword body text flows into a new `src/parse_sql_body.rs` module that converts it to the existing function-call rewrite syntax, which then executes through the proven `create_semantic_view()` path unchanged. The C++ shim stays untouched. The expansion engine gains a dual-dispatch mechanism — definitions with `from_table` populated on their `Join` structs use new direct FROM+JOIN expansion; definitions without it keep the legacy CTE expansion. Both old and new definitions coexist without catalog migration.

**Major components:**
1. **SQL body translator** (`src/parse_sql_body.rs`, new) — parses TABLES/RELATIONSHIPS/DIMENSIONS/METRICS keyword clauses and emits STRUCT/LIST literal function-call syntax; hooked into `rewrite_ddl()` via `body_uses_sql_syntax()` detection
2. **PK/FK model** (`src/model.rs`, modified) — two new `#[serde(default)]` fields: `primary_key: Vec<String>` on `TableRef`, `from_table: String` on `Join`; `parse_define_args_from_bind()` updated to preserve the previously-discarded `from_alias` value
3. **Dual expansion engine** (`src/expand.rs`, modified) — `expand()` dispatches to `expand_direct()` (no CTE, direct FROM+JOIN, qualified names work) or `expand_cte()` (existing, unchanged); join resolution dispatches to `resolve_joins_pkfk()` (FK graph traversal + topological sort) or `resolve_joins_legacy()` (existing ON-clause substring matching)

**Key patterns to follow:**
- `#[serde(default)]` on all new struct fields — this pattern is used 6+ times in the current model and guarantees backward compat without migration
- Dual-path dispatch via feature detection (`has_pkfk_joins()`, `uses_direct_expansion()`) — select execution strategy based on definition content, not creation timestamp
- Body syntax discrimination first — `body_uses_sql_syntax()` must be built and tested before any keyword parsing runs on production DDL

**Anti-patterns to avoid:**
- Parsing SQL expressions (treat expressions as opaque text bounded by depth-0 delimiters)
- Removing legacy CTE/ON-clause paths (old definitions stored in the catalog must continue to work)
- Pre-computing ON clauses and storing them in the catalog (compute at expansion time from PK/FK data)
- Modifying `shim.cpp` (all new parsing logic stays in Rust; C++ shim stays as-is)

### Critical Pitfalls

1. **Backward-compatibility break on parser upgrade (C1, HIGH confidence)** — the new keyword parser must not consume existing `:=` function-call bodies. Build the `body_uses_sql_syntax()` discriminator before any keyword parsing. Required test: a view created with old `:=` syntax must still work end-to-end after v0.5.2 changes. This is the #1 backward-compatibility risk.

2. **Diamond join graphs produce silently wrong results (C2, HIGH confidence)** — the FK graph must be validated as a tree at define time. Reject definitions where any table is reachable via multiple paths. This matches Snowflake's own restriction and prevents metric inflation without requiring complex deduplication logic. Algorithm: DFS from each node; if any node is visited twice, reject with a clear error.

3. **CTE flattening breaks qualified column references (C3, HIGH confidence)** — the CTE `SELECT *` pattern destroys table aliases in the outer query scope. The outer SELECT sees only `_base`, not `o` or `c`. Drop the CTE entirely for PK/FK definitions and use direct FROM+JOIN SQL. Keep the legacy CTE path for old definitions via `uses_direct_expansion()` feature detection.

4. **Dual DDL interface desynchronization (M3, HIGH confidence)** — SQL keyword DDL and function-call DDL must produce identical `SemanticViewDefinition` JSON. Add property-based roundtrip tests: generate random valid definitions, verify both DDL paths serialize to semantically equivalent JSON. The shared model struct is the normalization point.

5. **Stored ON-clause definitions lose transitive join resolution (C4, HIGH confidence)** — keep both resolution strategies (`resolve_joins_pkfk` and `resolve_joins_legacy`) selected by `has_pkfk_joins()`. Never mix strategies within a single definition execution.

## Implications for Roadmap

The feature dependencies discovered in research establish a clear sequential build order. The model must be stable before the translator can reference its types. The translator must work before integration tests can run. Define-time validation must run before the expansion engine can assume graph correctness. This is a 5-phase sequential build.

### Phase 1: Model Changes

**Rationale:** The model is the prerequisite for everything else. No translator code can be written until struct fields are defined. These changes carry the lowest risk — additive `#[serde(default)]` fields that do not touch any existing code path. All existing tests must pass after this phase.

**Delivers:** `TableRef.primary_key: Vec<String>`, `Join.from_table: String`, updated `parse_define_args_from_bind()` to preserve the currently-discarded `from_alias` value. Backward compat test: old JSON without new fields must deserialize correctly.

**Addresses:** Foundation for T1 (TABLES), T2 (RELATIONSHIPS), T5 (PK/FK join inference)

**Avoids:** C4 (stored definition compat) — serde defaults guarantee backward compat from day 1

### Phase 2: SQL Body Translator

**Rationale:** The translator is a pure `&str -> Result<String, String>` function testable entirely via `cargo test` without the extension infrastructure. It can be written and tested before the expansion engine changes. The syntax discriminator (`body_uses_sql_syntax()`) must be the first thing implemented — before any keyword clause parsing touches real DDL.

**Delivers:** New `src/parse_sql_body.rs` with `translate_sql_body()`; `body_uses_sql_syntax()` discriminator integrated into `rewrite_ddl()`; TABLES, RELATIONSHIPS, DIMENSIONS, and METRICS clause parsers; updated `validate_clauses()` for new syntax patterns.

**Addresses:** T1, T2, T3, T4 (all four SQL clauses); TECH-DEBT item 8 (statement rewrite syntax gap)

**Avoids:** C1 (backward compat break — discriminator first); m4 (keyword name collision — positional parsing); M1 (nested parens — reuse existing bracket-depth tracking from `scan_clause_keywords`)

### Phase 3: Define-Time Graph Validation

**Rationale:** Graph validation must happen before the expansion engine is changed, because the expansion engine relies on the FK graph being a DAG. Catching bad graphs at define time prevents silent wrong results. This phase is short (cycle detection is O(V+E) on a small graph) but architecturally critical.

**Delivers:** Cycle detection (DFS) and diamond detection (multi-path DFS) run during `create_semantic_view` bind; clear error messages for self-references, cycles, and ambiguous join paths with the form "Circular relationship detected: A -> B -> A."

**Addresses:** C2 (diamond joins), M2 (cycles and self-references)

**Avoids:** Silent metric inflation from fan traps; infinite loops in join resolution from cyclic graphs

### Phase 4: PK/FK Expansion Engine

**Rationale:** The expansion engine is the highest-complexity change (Medium-High). It requires the model (Phase 1) to be stable and benefits from validated graphs (Phase 3). The dual-dispatch pattern allows incremental rollout — the new expansion path only runs for definitions where `from_table` is populated.

**Delivers:** `expand_direct()` for direct FROM+JOIN SQL (no CTE, supports qualified names); `resolve_joins_pkfk()` with directed graph traversal + topological sort; `uses_direct_expansion()` feature detection; qualified column reference support (`alias.column`) in dimension and metric expressions.

**Addresses:** T5 (PK/FK join inference), T6 (qualified column references); resolves TECH-DEBT items 6 and 7

**Avoids:** C3 (CTE breaks qualified names — CTE dropped for new definitions); C4 (old definitions keep legacy path); M6 (type inference — direct expansion is compatible with the `build_execution_sql` wrapper since it wraps the whole SQL string regardless of shape)

### Phase 5: Integration Testing

**Rationale:** End-to-end validation across all combinations: SQL DDL syntax, function-call syntax, legacy stored definitions, mixed queries. Property-based cross-path equivalence tests are the key quality gate for catching M3 (dual interface desynchronization) before it ships.

**Delivers:** New SQL logic tests (`.slt`) for Snowflake-style DDL, multi-table join scenarios, qualified column references, backward compat with legacy definitions. Property-based roundtrip tests (parse SQL DDL -> model -> function-call syntax -> parse back, assert round-trip identity). Cross-path equivalence tests (same definition via both DDL interfaces, assert identical JSON).

**Addresses:** M3 (cross-path equivalence); m1 (expression boundary edge cases); m2 (empty `tables` field for legacy definitions); m3 (semicolons in string literals — document-only)

**Avoids:** Silent regressions on existing views; behavioral divergence between DDL interfaces discovered by users

### Phase Ordering Rationale

- Model first because all other source files import model types; changing structs late causes cascading churn and test failures during active development
- Translator before expansion because translation tests run via `cargo test` (fast); expansion requires `just build` + `just test-sql` (slower); faster feedback means more iteration
- Graph validation before expansion because the expansion engine's graph traversal assumes a valid DAG; enforcing that invariant at define time simplifies the expansion code
- Integration last because it requires all prior phases to be complete; failures at this stage are integration issues, not component issues, and are easier to diagnose with solid unit test coverage from prior phases

### Research Flags

Phases likely needing a focused design spike before or during planning:

- **Phase 4 (Expansion Engine):** The interaction between the `build_execution_sql` type-cast wrapper and direct FROM+JOIN SQL (vs the CTE it was designed around) has not been empirically verified. Run a quick spike: generate a direct FROM+JOIN SQL, wrap it with `build_execution_sql`, execute it, verify types are inferred correctly. Do this before writing any Phase 4 implementation code.
- **Phase 1 (Model — `Join` struct shape):** Role-playing dimensions are deferred but the model should not preclude them. Decide whether to add a `relationship_name: String` field now (low cost, future-proofs) or defer it (avoids premature abstraction). This decision affects the final struct shape before Phase 2 translator code is written.

Phases with well-established patterns (skip additional research):

- **Phase 1 (Model Changes):** The `#[serde(default)]` field extension pattern is used 6+ times in `src/model.rs`; no unknowns.
- **Phase 2 (SQL Body Translator):** Bracket-depth scanning already exists in `scan_clause_keywords`; this is extension of a proven pattern.
- **Phase 3 (Graph Validation):** DFS cycle detection is a standard algorithm; the graph has at most ~20 nodes/edges.
- **Phase 5 (Integration Testing):** sqllogictest `.slt` patterns and proptest patterns are established in this project.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Zero new deps confirmed; sqlparser-rs and petgraph rejected based on verified crate API analysis and source code inspection |
| Features | HIGH | Snowflake DDL grammar verified from official docs; existing model structs (`TableRef`, `Join`, `JoinColumn`) map directly to new clause requirements |
| Architecture | HIGH | Translator approach confirmed viable via direct analysis of `parse.rs`, `expand.rs`, `model.rs`, `ddl/parse_args.rs`; all integration points identified |
| Pitfalls | HIGH | Backward compat and CTE risks from direct code review of known TECH-DEBT items; diamond/fan-trap risks from Cube.dev, Snowflake, MetricFlow, and Holistics documentation |

**Overall confidence:** HIGH

### Gaps to Address

- **Fan-trap cardinality warning**: Research recommends documenting the fan-trap constraint for v0.5.2 rather than solving it. The exact user-facing language (documentation note vs. a runtime warning on create or query) should be decided during planning.
- **Role-playing dimensions**: Deferred from v0.5.2 but the `Join` struct shape (add `relationship_name` now or later) should be settled in Phase 1 before any translator code is written against it.
- **DDL body buffer size**: The C++ shim has a 4096-byte buffer for DDL text. SQL keyword bodies will be longer than equivalent function-call bodies for complex views. Measure representative definitions (e.g., TPC-H 6-table view) during Phase 2 testing to confirm they stay within the limit.

## Sources

### Primary (HIGH confidence)
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) — full DDL grammar; TABLES/RELATIONSHIPS/DIMENSIONS/METRICS/FACTS clause syntax with PRIMARY KEY and REFERENCES
- [Snowflake Semantic View TPC-H Example](https://docs.snowflake.com/en/user-guide/views-semantic/example) — worked example with PK/FK declarations; validates grammar at scale
- [Snowflake Validation Rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) — circular relationship prohibition, diamond join prohibition, self-reference prohibition
- [Snowflake SEMANTIC_VIEW query syntax](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) — qualified dimension/metric reference patterns
- [petgraph v0.8.3 — docs.rs](https://docs.rs/crate/petgraph/latest) — dependency count and API surface; basis for rejection
- [sqlparser-rs v0.61.0 Statement enum — docs.rs](https://docs.rs/sqlparser/latest/sqlparser/ast/enum.Statement.html) — no `CREATE SEMANTIC VIEW` variant; basis for rejection
- Project source: `src/parse.rs`, `src/expand.rs`, `src/model.rs`, `src/ddl/parse_args.rs`, `src/ddl/define.rs`, `Cargo.toml`, `TECH-DEBT.md` — first-party code inspection (all HIGH confidence)

### Secondary (MEDIUM confidence)
- [Cube.dev: Working with Joins](https://cube.dev/docs/product/data-modeling/concepts/working-with-joins) — diamond subgraph detection; Dijkstra join path selection; fan/chasm trap prevention via PK deduplication
- [Cube.dev: Joins Reference](https://cube.dev/docs/product/data-modeling/reference/joins) — relationship cardinality types; LEFT JOIN as default
- [dbt MetricFlow Join Logic](https://docs.getdbt.com/docs/build/join-logic) — multi-hop join limits; fan-out prevention via entity types
- [Holistics: Path Ambiguity in Datasets](https://docs.holistics.io/docs/dataset-path-ambiguity) — role-playing dimension handling; tiered path ranking for disambiguation
- [sqlparser-rs custom parser docs](https://github.com/sqlparser-rs/sqlparser-rs/blob/main/docs/custom_sql_parser.md) — extensibility limitations (rate-limited during research fetch; partial)

### Tertiary (context, LOW confidence)
- [datacadamia: Fan Trap](https://www.datacadamia.com/data/type/cube/semantic/fan_trap) — fan trap definition and measure inflation mechanics
- [datacadamia: Chasm Trap](https://datacadamia.com/data/type/cube/semantic/chasm_trap) — chasm trap via multiple FK references
- [Sisense: Chasm and Fan Traps](https://docs.sisense.com/main/SisenseLinux/chasm-and-fan-traps.htm) — detection and resolution patterns in BI tools

---
*Research completed: 2026-03-09*
*Ready for roadmap: yes*
