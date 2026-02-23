# Project Research Summary

**Project:** DuckDB Semantic Views Extension
**Domain:** DuckDB extension (Rust) — semantic layer / query expansion engine
**Researched:** 2026-02-23
**Confidence:** MEDIUM (stack and features are well-established; DuckDB extension Rust APIs have documented gaps that require early prototype validation)

## Executive Summary

This project builds a DuckDB extension in Rust that implements a semantic layer: users define named "semantic views" via SQL DDL specifying dimensions, metrics, relationships, and row filters, then query them with an explicit dimension+metric selection syntax. The extension intercepts the query, resolves join paths, infers GROUP BY clauses, and emits a concrete SQL query that DuckDB executes. This is a preprocessor, not a query engine — DuckDB does all computation; the extension does only SQL expansion. The closest prior art is Snowflake's SEMANTIC_VIEW feature (SQL DDL syntax, table function query interface) combined with dbt MetricFlow's semantic model concepts (entities, dimensions, measures, relationships).

The recommended approach is a staged implementation anchored by a pragmatic v0.1 DDL workaround: implement `CREATE SEMANTIC VIEW` semantics initially via a function-based interface (`SELECT define_semantic_view(...)`) rather than native DDL parser hooks, since DuckDB's C API does not expose parser extension hooks to Rust. The core expansion logic — dimension/metric resolution, join graph traversal, SQL building, GROUP BY inference — is implemented in safe Rust against the `duckdb-rs` vtab API, with persistence via a regular DuckDB table in a `semantic_layer` schema. Native `CREATE SEMANTIC VIEW` DDL is added in a subsequent phase using C++ FFI for parser hooks once the expansion engine is validated.

The key risk is the mismatch between the project's ambition for SQL-native DDL and the actual Rust extension API surface. Parser extension hooks exist in DuckDB's C++ SDK but are not exposed in the C API that `duckdb-rs` wraps. This forces either a DDL workaround in v0.1 or early investment in unsafe FFI. A secondary risk is correctness: GROUP BY inference, join fan-out, and WHERE clause placement are failure modes that return wrong answers silently. These must be gated by a rigorous integration test suite against known datasets before any release.

---

## Key Findings

### Recommended Stack

The extension must be built with `duckdb/extension-template-rs` as the scaffold — this is the official Rust extension template under the DuckDB GitHub org, providing the CMake+Cargo build system, GitHub Actions CI, and community extension signing workflow. There is no pure-Cargo path for DuckDB extensions. The `duckdb` crate (duckdb-rs, v0.10+) provides the Rust API layer; its `vtab` module covers table function registration adequately. Anything beyond table functions (parser hooks, replacement scans, custom catalog entries) requires dropping into `libduckdb-sys` raw FFI — plan for this explicitly.

For persistence, the extension creates a `semantic_layer` schema with a `_definitions` table in the user's DuckDB database file. This is a standard DuckDB table; it persists via DuckDB's WAL and block storage automatically with no special integration. SQL expressions in definitions are stored as opaque strings (not parsed by the extension) and interpolated into expanded SQL at query time, letting DuckDB validate them. This eliminates the `sqlparser-rs` dependency and its DuckDB dialect gap concerns.

**Core technologies:**
- `duckdb` crate (duckdb-rs) with `extensions` + `vtab` features — the only Rust SDK for DuckDB extension development
- `duckdb/extension-template-rs` — official build scaffold; CMake wraps Cargo; mandatory for community extension distribution
- `serde` + `serde_json` — JSON serialization of semantic view definitions stored in the catalog table; `#[serde(default)]` for forward compatibility
- `thiserror` — structured extension error types that propagate as DuckDB error messages
- `libduckdb-sys` (raw FFI) — required for replacement scan registration and parser hooks not yet exposed in `duckdb-rs`

**What to avoid:** `arrow-rs` (extension is a preprocessor, not a data transformer), `datafusion` (competing engine), `egg` (query rewriting for diverse clients — not needed here; inputs are structured), `sqlparser-rs` for SQL expression validation (use opaque strings and let DuckDB validate at execution time).

### Expected Features

Research against four reference systems (Snowflake SEMANTIC_VIEW, Databricks Metric Views, Cube.dev, dbt/MetricFlow) confirmed which features are universal table stakes vs. what is genuinely differentiating vs. what to defer.

**Must have (table stakes) — all 10 are achievable in v0.1:**
- TS-1: Dimension definitions with SQL expressions — name-to-expression mapping, the base primitive
- TS-2: Metric definitions with aggregation type — SUM/COUNT/AVG/MIN/MAX/COUNT DISTINCT; includes additivity classification for future pre-aggregation compatibility
- TS-3: Relationship / join declarations — single-hop only in v0.1; multi-hop is AF-4 (deferred)
- TS-4: Automatic GROUP BY inference — GROUP BY = all requested dimensions; the core value proposition
- TS-5: SQL DDL definition syntax (`CREATE SEMANTIC VIEW`) — DuckDB users expect SQL DDL; function-based workaround in v0.1, native DDL in v0.2
- TS-6: Table function query syntax — `FROM view_name(dimensions=..., metrics=...)` via DuckDB table function + replacement scan
- TS-7: Time dimensions with granularity — ISO-standard granularities only (second/minute/hour/day/week/month/quarter/year); no fiscal granularities
- TS-8: Expansion-time validation with clear errors — dimension membership check, join path reachability, actionable error messages referencing the semantic view definition
- TS-9: Row-level filter predicates — stored per table, always AND-composed with user WHERE clauses
- TS-10: Persistence of definitions — internal DuckDB table in `semantic_layer` schema

**Should have (competitive differentiators — all are architecturally free or low-cost):**
- D-1: In-process / embedded operation — inherent consequence of being a DuckDB extension; no other semantic layer runs in-process
- D-2: SQL-native definition — SQL DDL with no YAML toolchain; unique among OSS semantic layers
- D-3: SQL composability — expanded result is a derived table usable in CTEs, JOINs, PIVOT, WINDOW functions
- D-4: Works with local files (Parquet, CSV) — DuckDB resolves table names in definitions to any DuckDB data source
- D-5: Zero-dependency install (`INSTALL semantic_views FROM community; LOAD semantic_views`) — genuine competitive moat
- D-6: Inspectable SQL expansion — `EXPLAIN SEMANTIC VIEW` or equivalent that returns the generated SQL rather than executing it

**Defer (v2+):**
- AF-1: Pre-aggregation / materialization selection — Cube's flagship feature; doubles scope; correct metric additivity metadata should be stored in v0.1 to enable this later
- AF-2: YAML definition format — two parsers, two validation paths; no v0.1 value
- AF-3: Derived metrics (metric-on-metric) — requires multi-level SQL planning
- AF-4: Multi-hop join resolution — requires graph search, cycle detection, path disambiguation
- AF-5: Hierarchies — rare even in reference systems; post-v1
- AF-6: Multi-stage/nested aggregations — requires a multi-level query planner
- AF-7: BI tool metadata / HTTP API — belongs to a separate server process, not a DuckDB extension

### Architecture Approach

The extension integrates with DuckDB at two injection points: the **parser extension hook** for `CREATE SEMANTIC VIEW` DDL (requires C++ SDK FFI, deferred to Phase 5), and the **replacement scan callback** for query interception (available via `duckdb_add_replacement_scan` in the C API). When a user writes `FROM my_semantic_view(dimensions=..., metrics=...)`, the replacement scan fires, recognizes the name, and substitutes a `semantic_view_scan` table function call. The table function's bind phase loads the definition from the in-memory registry, resolves the requested dimensions and metrics, computes the minimal join set, builds the expanded SQL string, and returns output column metadata. The execute phase runs the expanded SQL and streams results.

The in-memory registry is a `HashMap<String, SemanticViewDef>` populated from the `semantic_layer._definitions` table at extension load. Writes go to the persistence table first (within a DuckDB transaction), then update the in-memory cache. This makes the persistence table the source of truth; the in-memory cache is always reconstructable.

**Major components:**
1. **Extension scaffold** — cdylib with `semantic_views_init` / `semantic_views_version` C exports; registers table function, replacement scan, and DDL handler
2. **In-memory registry** — `HashMap<String, SemanticViewDef>` with serde-serializable `SemanticViewDef` struct; loaded from DuckDB table at init
3. **Persistence layer** — `semantic_layer._definitions` DuckDB table; JSON blob column; serde with `#[serde(default)]` for forward compatibility; version field per row for migration
4. **DDL handler** — v0.1: scalar function `define_semantic_view(name, json)` / `drop_semantic_view(name)`; v0.2: parser extension hook for native `CREATE SEMANTIC VIEW` syntax
5. **Expansion engine** — pure Rust function: `expand(def, dimensions, metrics, filter) -> String`; join resolution, SQL builder, GROUP BY inference, WHERE placement
6. **Query interface** — replacement scan callback + `semantic_view_scan` table function; bind phase drives expansion; execute phase streams results
7. **Validation layer** — at DDL time: table/column existence checks; at query time: dimension/metric membership, join reachability, clear error messages

### Critical Pitfalls

1. **ABI lock: pin `duckdb-rs` to exact DuckDB runtime version** — Extension binaries are ABI-specific; mismatches cause silent segfaults or load failures. Pin the crate version, document it prominently, set up CI matrix against target DuckDB version from day one. Address in Phase 1.

2. **Silent wrong answers from GROUP BY / join inference** — Ambiguous column references, fan-out joins, and incorrect WHERE placement return wrong numbers with no error. Prevention: fully qualify all column references in emitted SQL, assert metric totals are stable across different dimension combinations, build a known-answer test suite (TPC-H or jaffle-shop) before shipping. Address in Phase 3.

3. **Parser hooks not available in C API** — `AddParserExtension` exists in the C++ SDK but not in `duckdb.h`. Native `CREATE SEMANTIC VIEW` DDL in v0.1 is not achievable purely in `duckdb-rs`. Use function-based DDL (`SELECT define_semantic_view(...)`) for v0.1; invest in C++ FFI for parser hooks in Phase 5. Address architecture decision in Phase 1.

4. **Persistence model: catalog does not auto-persist extension objects** — Extension-registered objects do not survive restart unless explicitly stored. Use a plain DuckDB table as the persistence store; always reconstruct in-memory state from it at load. Never cache across restarts. Include a "create → close → reopen → query" test as a v0.1 acceptance criterion. Address in Phase 2.

5. **Serialization forward compatibility** — Adding fields to `SemanticViewDef` without `#[serde(default)]` breaks deserialization of older stored definitions. Use JSON with `#[serde(default)]`, store extension version per row, write migration tests from v0.1 to v0.2 format before any schema change. Address in Phase 2.

---

## Implications for Roadmap

Based on combined research, the natural phase structure follows the build order derived from component dependencies and the risk mitigation priorities identified in PITFALLS.md.

### Phase 1: Project Scaffold and Architecture Decisions
**Rationale:** All architectural risks — ABI lock, duckdb-rs coverage gaps, build system, DDL strategy — must be resolved before any business logic is written. Wrong decisions here require expensive rework. This phase produces a loadable "hello world" extension and documents the key architectural choices.
**Delivers:** CMake+Cargo build producing a loadable `.duckdb_extension`; `LOAD` smoke test passing in CI; ABI version pinned; DDL strategy decided (function-based for v0.1, parser hook for v0.2); replacement scan API evaluated against `libduckdb-sys`
**Addresses:** D-5 (community extension pipeline setup), TS-5/TS-6 (architecture decision on DDL and query syntax)
**Avoids:** P1.1 (ABI lock), P1.2 (duckdb-rs coverage), P1.3 (parser hook design), P1.4 (build system), P4.1 (extension API version)
**Research flag:** Needs prototype validation — the replacement scan and named parameter APIs in `duckdb-rs` may have gaps that require raw FFI; discover this in Phase 1, not Phase 3.

### Phase 2: Storage Layer and Definition Schema
**Rationale:** Everything downstream (DDL, expansion, query) depends on a stable definition data model and persistence mechanism. Getting the JSON schema and serde representation right early prevents costly migrations. The persistence model (DuckDB table as catalog) is the architectural decision with the largest downstream impact.
**Delivers:** `SemanticViewDef` Rust struct with serde; `semantic_layer._definitions` table created at init; load-from-table at startup; CRUD functions (`define_semantic_view`, `drop_semantic_view`); round-trip tests (create → close → reopen → query definition); correct `DROP` transaction safety
**Addresses:** TS-10 (persistence), TS-5 (DDL — function-based), TS-1/TS-2/TS-3/TS-7/TS-9 (definition schema captures all these)
**Avoids:** P3.1 (catalog persistence model), P3.2 (serialization format), P3.3 (multi-connection), P3.4 (DROP safety)
**Research flag:** Standard patterns — DuckDB table persistence is well-documented; no phase-level research needed.

### Phase 3: Expansion Engine
**Rationale:** The core algorithmic logic is isolated from DuckDB integration concerns. Build it as a pure Rust function (`expand(def, dims, metrics, filter) -> String`) with comprehensive tests against known datasets. This is where correctness must be established before any user-facing interface is wired up.
**Delivers:** `expand()` function covering all metric types, single-hop join resolution, GROUP BY inference, time granularity coarsening, row filter composition, WHERE placement (pre vs. post aggregation), fully qualified identifiers, SQL identifier quoting; test suite with known-answer assertions (TPC-H or jaffle-shop); D-6 (inspectable expansion — return SQL string without executing)
**Addresses:** TS-1, TS-2, TS-3, TS-4, TS-7, TS-8, TS-9, D-3, D-6
**Avoids:** P2.1 (GROUP BY correctness), P2.2 (non-additive metrics), P2.3 (join fan-out), P2.4 (WHERE placement), P2.5 (identifier quoting), P2.6 (time granularity)
**Research flag:** Standard patterns — SQL string building is well-understood; the correctness requirements are clear. No research phase needed, but allocate substantial time for test authorship.

### Phase 4: Query Interface (Replacement Scan + Table Function)
**Rationale:** Wire the expansion engine to DuckDB's query pipeline. This phase makes the extension actually queryable. The replacement scan + table function pattern is well-documented in the DuckDB C API; the main risk is re-entrant query execution in the table function bind phase, which may require schema inference from definition metadata rather than SQL execution.
**Delivers:** `semantic_view_scan` table function registered with DuckDB; replacement scan callback recognizing semantic view names; bind phase calling expansion engine and returning output schema; execute phase streaming results; end-to-end query tests (`define_semantic_view` → `FROM view(dimensions=..., metrics=...)` → correct results)
**Addresses:** TS-6 (query syntax), TS-8 (validation at query time), D-1, D-2, D-4
**Avoids:** P1.2 (duckdb-rs coverage — verify vtab API covers named parameters; fall back to raw FFI if needed)
**Research flag:** Needs early prototype — re-entrant query execution in the bind phase is documented as problematic in ARCHITECTURE.md (Q3). Prototype output schema inference from definition metadata as a mitigation before committing to the full implementation.

### Phase 5: Native DDL (`CREATE SEMANTIC VIEW` syntax)
**Rationale:** The function-based DDL (`SELECT define_semantic_view(...)`) from Phase 2 is functional but not ergonomic. Native SQL DDL is the project's stated identity differentiator (D-2) and a design commitment in the project brief. Once the expansion engine is proven, invest in the C++ FFI for parser extension hooks to deliver `CREATE SEMANTIC VIEW` / `DROP SEMANTIC VIEW` / `SHOW SEMANTIC VIEWS` syntax.
**Delivers:** Parser extension hook via C++ FFI or thin C++ shim calling Rust; `CREATE SEMANTIC VIEW name (...)` DDL parsed natively; `DROP SEMANTIC VIEW`; `DESCRIBE SEMANTIC VIEW`; `SHOW SEMANTIC VIEWS`
**Addresses:** TS-5 (native DDL — full implementation), D-2
**Avoids:** P1.3 (parser hook fragility — study spatial extension DDL as reference; write integration tests that run CREATE in a fresh DuckDB process)
**Research flag:** Needs phase-level research — no widely-used community extension implements custom DDL via parser hooks in Rust. Must study C++ extension examples (spatial extension) and determine whether a thin C++ shim or raw FFI is the right approach. Budget 1-2 weeks research before implementation.

### Phase 6: Community Extension Packaging and Distribution
**Rationale:** The signing requirement, multi-platform CI, and community registry submission process are non-trivial and must be addressed before any users can install the extension. This is a distinct phase because it involves the `duckdb/community-extensions` repository and the DuckDB team's review process.
**Delivers:** Extension passing community extension CI on all five target platforms (Linux x86_64/ARM64, macOS x86_64/ARM64, Windows x86_64); PR to `duckdb/community-extensions`; `INSTALL semantic_views FROM community; LOAD semantic_views` working end-to-end; unsigned distribution documented for development use
**Addresses:** D-5 (zero-dependency install)
**Avoids:** P1.5 (registry requirements), P4.2 (multi-version builds), P4.3 (trust model)
**Research flag:** Verify current community registry process — submission requirements and CI configuration in `duckdb/community-extensions` may have changed since August 2025.

### Phase Ordering Rationale

- Phase 1 before everything: architectural decisions (ABI lock, DDL strategy, build system) have the largest rewrite cost if discovered late.
- Phase 2 (storage) before Phase 3 (expansion): the expansion engine needs the definition schema to exist; getting the Rust struct design right early prevents serialization migrations.
- Phase 3 (expansion) before Phase 4 (query interface): isolating correctness tests in pure Rust without DuckDB integration is faster and produces a more complete test suite.
- Phase 4 (query interface) before Phase 5 (native DDL): validates the full stack with function-based DDL first; avoids spending time on parser hooks if the expansion engine has blocking issues.
- Phase 5 (native DDL) after Phase 4: parser hooks are the highest-risk implementation task; doing it after the rest is proven is correct sequencing.
- Phase 6 (distribution) last: packaging and registry submission are a one-time process after the feature set is stable.

### Research Flags

Phases needing deeper research during planning:
- **Phase 1:** Prototype the replacement scan and named-parameter table function APIs in `duckdb-rs` immediately. If gaps exist (likely based on ARCHITECTURE.md Q5), the raw FFI approach needs to be scoped before feature planning begins.
- **Phase 4:** Prototype bind-phase schema inference without SQL re-execution (ARCHITECTURE.md Q3). The re-entrant query execution limitation could force a significant design change if discovered late.
- **Phase 5:** Research C++ parser extension hooks via the `spatial` extension source code. The custom DDL path has no well-documented Rust examples; budget research time before writing implementation estimates.
- **Phase 6:** Verify current `duckdb/community-extensions` submission requirements. CI toolchain versions and the DuckDB version matrix will determine compatibility constraints for the release.

Phases with standard patterns (can proceed without research phase):
- **Phase 2:** DuckDB table persistence, serde/JSON serialization, and DuckDB schema creation at extension init are all well-documented patterns used by existing extensions.
- **Phase 3:** SQL string building with GROUP BY inference is a well-understood problem; the primary investment is test authorship, not design research.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Official DuckDB extension template, `duckdb-rs` crate, and community registry are well-documented. Known gap: `duckdb-rs` vtab/replacement-scan coverage requires live verification against current crate version (training cutoff August 2025). |
| Features | HIGH | Four reference systems surveyed in depth. Table stakes / differentiator / anti-feature classification is well-grounded. COUNT DISTINCT additivity modeling is a concrete v0.1 requirement with clear implementation path. |
| Architecture | MEDIUM | DuckDB C API injection points are documented. Core uncertainty: re-entrant query execution in the bind phase (Q3) and `duckdb-rs` API completeness (Q5) are open questions that require prototype validation in Phase 1. |
| Pitfalls | HIGH | Pitfall catalog is comprehensive, phase-specific, and actionable. Each pitfall has clear prevention strategies. Group-by correctness and persistence model pitfalls are particularly critical. |

**Overall confidence:** MEDIUM-HIGH

The semantic model design and feature scope are clear. The implementation risks are known and bounded. The main uncertainty is in the Rust/DuckDB integration layer (parser hooks, replacement scan API surface in `duckdb-rs`), which can only be resolved by prototyping in Phase 1. The recommended strategy — start with function-based DDL, validate the expansion engine, then invest in native DDL — is the correct risk-mitigation sequence.

### Gaps to Address

- **`duckdb-rs` API surface for replacement scan and named parameters:** Must prototype in Phase 1. If `duckdb-rs` does not expose `duckdb_add_replacement_scan` or named parameter handling, raw `libduckdb-sys` FFI wrappers must be scoped before feature planning.
- **Re-entrant query execution in bind phase:** Must prototype in Phase 1/4. If DuckDB prevents query re-execution during bind, output schema must be inferred from definition metadata. This affects how dimension/metric type information is stored in `SemanticViewDef`.
- **DuckDB version to target:** Must decide before Phase 1. Pin to a stable DuckDB release (1.0+ recommended). Verify which DuckDB version the current `duckdb-rs` crate tracks.
- **Parser extension hook availability in C API vs. C++ SDK:** Must verify during Phase 5 planning. If parser hooks are strictly C++ only (not in `duckdb.h`), the options are: (a) thin C++ shim calling Rust static library, or (b) mix C++/Rust in the extension template. Scope this decision before Phase 5 implementation starts.
- **Community extension registry current requirements:** Verify `duckdb/community-extensions` CI configuration before Phase 6. Rust toolchain version, DuckDB version matrix, and any new submission requirements may have changed.

---

## Sources

### Primary (HIGH confidence)
- `duckdb/duckdb-rs` — https://github.com/duckdb/duckdb-rs — Rust extension development API, vtab module, libduckdb-sys bindings
- `duckdb/extension-template-rs` — https://github.com/duckdb/extension-template-rs — Official Rust extension scaffold, build system, CI
- DuckDB extension API docs — https://duckdb.org/docs/dev/extensions/creating_extensions.html — Extension lifecycle, C API hooks
- DuckDB parser extension API — https://duckdb.org/docs/dev/extensions/parser_extensions.html — Custom DDL injection points (C++ SDK)
- Snowflake Semantic Views — https://docs.snowflake.com/en/user-guide/views-semantic/overview — Primary prior art for DDL and query syntax design
- dbt MetricFlow — https://docs.getdbt.com/docs/build/about-metricflow — Semantic model concepts (entities, dimensions, measures, relationships)

### Secondary (MEDIUM confidence)
- Databricks Metric Views — https://docs.databricks.com/en/data-governance/metric-views/index.html — Ratio metrics, derived metrics, entity model
- Cube.dev data model docs — https://cube.dev/docs/product/data-modeling/overview — Pre-aggregation model, measure additivity, multi-stage aggregations (features scoped to v2+)
- `duckdb/community-extensions` — https://github.com/duckdb/community-extensions — Registry submission process, signing requirements, CI matrix
- `apache/datafusion-sqlparser-rs` — https://github.com/apache/datafusion-sqlparser-rs — Evaluated and rejected for v0.1 (opaque string approach preferred)
- `_notes/semantic-views-duckdb-design-doc.md` — Prior art research in this repo — informs parser hook complexity assessment and Cube pre-aggregation understanding

### Tertiary (LOW confidence — requires live verification)
- `duckdb-rs` vtab module named parameter support — training data cutoff August 2025; verify against current crate source
- Current DuckDB stable version (1.x) to target — verify on duckdb.org before pinning
- Community extension CI Rust toolchain version — verify in `duckdb/community-extensions` repo

---
*Research completed: 2026-02-23*
*Ready for roadmap: yes*
