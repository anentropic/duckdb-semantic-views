# Roadmap: DuckDB Semantic Views

## Milestones

- ✅ **v0.1.0 MVP** — Phases 1-7 (shipped 2026-02-28)
- 🚧 **v0.2.0 Native DDL + Time Dimensions** — Phases 8-12 (in progress)

## Phases

<details>
<summary>✅ v0.1.0 MVP (Phases 1-7) — SHIPPED 2026-02-28</summary>

- [x] Phase 1: Scaffold (3/3 plans) — completed 2026-02-24
- [x] Phase 2: Storage and DDL (4/4 plans) — completed 2026-02-24
- [x] Phase 3: Expansion Engine (3/3 plans) — completed 2026-02-25
- [x] Phase 4: Query Interface (3/3 plans) — completed 2026-02-25
- [x] Phase 5: Hardening and Docs (2/2 plans) — completed 2026-02-26
- [x] Phase 6: Tech Debt Code Cleanup (1/1 plan) — completed 2026-02-26
- [x] Phase 7: Verification & Formal Closure (2/2 plans) — completed 2026-02-27

Full details: [milestones/v0.1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md)

</details>

### 🚧 v0.2.0 Native DDL + Time Dimensions (In Progress)

**Milestone Goal:** Introduce a C++ shim to unlock native `CREATE SEMANTIC VIEW` DDL and `pragma_query_t` persistence, add time dimension granularity coarsening, and expose EXPLAIN for expanded SQL.

- [x] **Phase 8: C++ Shim Infrastructure** - Validate the build mechanics of the Rust+C++ boundary in isolation before any parser logic is added — completed 2026-03-01
- [x] **Phase 9: Time Dimensions** - Add time-typed dimensions with granularity coarsening and per-query granularity override (pure Rust, delivers user value early) — completed 2026-03-01
- [x] **Phase 10: pragma_query_t Catalog Persistence** - Replace the sidecar file with DuckDB-native table persistence via the C++ shim's PRAGMA callback (completed 2026-03-01)
- [ ] **Phase 11: CREATE SEMANTIC VIEW Parser Hook** - Implement native SQL DDL syntax for creating and dropping semantic views via the C++ parser extension
- [ ] **Phase 12: EXPLAIN + Typed Output** - Wire EXPLAIN support for expanded SQL and replace all-VARCHAR output with typed columns

## Phase Details

### Phase 8: C++ Shim Infrastructure
**Goal**: The Rust+C++ build boundary is validated and the extension loads cleanly after C++ is added
**Depends on**: Phase 7 (v0.1.0)
**Requirements**: INFRA-01
**Success Criteria** (what must be TRUE):
  1. `cargo build --features extension` compiles the C++ shim on all 5 CI targets without errors
  2. `cargo test` (no extension feature) continues to pass — shim compilation is fully gated behind `CARGO_FEATURE_EXTENSION`
  3. `LOAD 'semantic_views'` in DuckDB succeeds after the C++ addition — existing v0.1.0 functionality is unaffected
  4. The loaded extension exports exactly the three DuckDB entry point symbols — no Rust standard library symbols leak into the binary
**Plans:** 2 plans

Plans:
- [x] 08-01-PLAN.md — Vendor duckdb.hpp, create C++ shim skeleton (shim.cpp, shim.h, mod.rs), update Cargo.toml and Justfile
- [x] 08-02-PLAN.md — Create build.rs with feature-gated C++ compilation and symbol visibility; wire lib.rs extern C call; verify full build

### Phase 9: Time Dimensions
**Goal**: Users can declare time-typed dimensions in semantic view definitions and query them with automatic or overridden granularity
**Depends on**: Phase 8
**Requirements**: TIME-01, TIME-02, TIME-03, TIME-04
**Success Criteria** (what must be TRUE):
  1. User can declare a dimension as `type: "time"` with a granularity (day, week, month, year) in `define_semantic_view()` and the definition round-trips through serialization correctly
  2. `SELECT * FROM semantic_query('view', dimensions := ['order_date'])` returns dates truncated to the declared granularity using `date_trunc`
  3. User can pass `granularities := {'order_date': 'month'}` to `semantic_query` to override the declared granularity at query time
  4. A time dimension backed by a DATE source column returns DATE values — not TIMESTAMP strings like `2024-01-01 00:00:00`
**Plans**: 2 plans

Plans:
- [x] 09-01-PLAN.md — Extend Dimension struct with dim_type + granularity fields; from_json validation for time dimensions
- [x] 09-02-PLAN.md — date_trunc codegen in expand(); granularities MAP named parameter in semantic_query; bind-time validation

### Phase 10: pragma_query_t Catalog Persistence
**Goal**: Semantic view definitions persist via DuckDB-native tables and the sidecar `.semantic_views` file is gone from the codebase
**Depends on**: Phase 8
**Requirements**: PERSIST-01, PERSIST-02, PERSIST-03
**Success Criteria** (what must be TRUE):
  1. A semantic view defined in one DuckDB session is still queryable after closing and reopening the database — no `.semantic_views` file exists on disk
  2. `BEGIN; PRAGMA define_semantic_view_internal(...); ROLLBACK;` leaves the catalog unchanged — both the persistent table and in-memory catalog reflect the pre-transaction state
  3. No reference to sidecar file logic exists anywhere in the codebase — `grep -r "semantic_views"` on file paths returns nothing
**Plans**: 3 plans

Plans:
- [ ] 10-01-PLAN.md — C++ shim pragma registration + semantic_views_pragma_define/drop C functions + Rust FFI declarations in mod.rs
- [ ] 10-02-PLAN.md — Replace write_sidecar in define.rs/drop.rs with pragma FFI write-first pattern; lib.rs creates persist_conn
- [ ] 10-03-PLAN.md — Delete sidecar functions from catalog.rs, add migration, update test files, delete physical sidecar data files

### Phase 11: CREATE SEMANTIC VIEW Parser Hook
**Goal**: Users can create and drop semantic views using native SQL DDL syntax (`CREATE SEMANTIC VIEW`, `DROP SEMANTIC VIEW`)
**Depends on**: Phase 10
**Requirements**: DDL-01, DDL-02, DDL-03, DDL-04, DDL-05, DDL-06
**Success Criteria** (what must be TRUE):
  1. `CREATE SEMANTIC VIEW sales_summary (DIMENSIONS ..., METRICS ...)` succeeds and the view is immediately queryable via `semantic_query`
  2. `DROP SEMANTIC VIEW sales_summary` removes the definition and subsequent `semantic_query` calls return an error naming the view as unknown
  3. `CREATE OR REPLACE SEMANTIC VIEW sales_summary (...)` overwrites the existing definition without error
  4. All capabilities available via `define_semantic_view()` (dimensions, metrics, joins, filters) are expressible in the native DDL syntax
  5. `SELECT 1`, `CREATE TABLE`, and all other non-semantic-view SQL executes identically before and after loading the extension — the parser hook passes through cleanly
**Plans**: 4 plans

Plans:
- [ ] 11-01-PLAN.md — Update model.rs (Fact struct, Join evolution) + catalog.rs (catalog_upsert, catalog_delete_if_exists, FFI catalog functions)
- [ ] 11-02-PLAN.md — C++ parser hook in shim.cpp (parse_function_t, plan_function_t, DDL scan, tokenizer, shim.h catalog FFI declarations)
- [ ] 11-03-PLAN.md — Rust cleanup: delete define.rs/drop.rs, trim ddl/mod.rs, update lib.rs shim call signature (DDL-05)
- [ ] 11-04-PLAN.md — Rewrite phase2_ddl.test + new phase11_ddl.test (DDL-01 through DDL-06 integration coverage)

### Phase 11.1: review possible DDL and query syntax options and bring it as close as we can to Snowflake semantic views (INSERTED)

**Goal:** Reshape DDL from raw JSON string to typed STRUCT/LIST parameters and rename semantic_query to semantic_view, aligning the interface with Snowflake semantic view concepts without adding new capabilities
**Requirements**: DDL-03, DDL-04, DDL-05, DDL-06
**Depends on:** Phase 11
**Plans:** 1/5 plans executed

Plans:
- [ ] 11.1-01-PLAN.md — DESIGN.md + model.rs additions (TableRef, JoinColumn, Join.join_columns, SemanticViewDefinition.tables)
- [ ] 11.1-02-PLAN.md — src/ddl/parse_args.rs — STRUCT/LIST → SemanticViewDefinition parser (TDD)
- [ ] 11.1-03-PLAN.md — src/expand.rs — join_columns codegen + table-qualified dim/metric name resolution
- [ ] 11.1-04-PLAN.md — Wire parse_args into define.rs (6-arg signature); rename semantic_query → semantic_view in lib.rs
- [ ] 11.1-05-PLAN.md — Update all .test files to new syntax; make test passes

### Phase 12: EXPLAIN + Typed Output
**Goal**: `EXPLAIN FROM semantic_query(...)` shows the expanded SQL, and `semantic_query` returns typed columns instead of all-VARCHAR
**Depends on**: Phase 11
**Requirements**: EXPL-01, OUT-01
**Success Criteria** (what must be TRUE):
  1. `EXPLAIN FROM semantic_query('view', dimensions := [...], metrics := [...])` outputs the expanded SQL that the extension generates — the result is readable and matches what `semantic_query` would execute
  2. A metric defined as a BIGINT aggregate returns a BIGINT column from `semantic_query` — not a VARCHAR
  3. A time dimension backed by a DATE column returns a DATE column from `semantic_query` — not a VARCHAR
**Plans**: TBD

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v0.1.0 | 3/3 | Complete | 2026-02-24 |
| 2. Storage and DDL | v0.1.0 | 4/4 | Complete | 2026-02-24 |
| 3. Expansion Engine | v0.1.0 | 3/3 | Complete | 2026-02-25 |
| 4. Query Interface | v0.1.0 | 3/3 | Complete | 2026-02-25 |
| 5. Hardening and Docs | v0.1.0 | 2/2 | Complete | 2026-02-26 |
| 6. Tech Debt Code Cleanup | v0.1.0 | 1/1 | Complete | 2026-02-26 |
| 7. Verification & Formal Closure | v0.1.0 | 2/2 | Complete | 2026-02-27 |
| 8. C++ Shim Infrastructure | v0.2.0 | 2/2 | Complete | 2026-03-01 |
| 9. Time Dimensions | v0.2.0 | 0/? | Not started | - |
| 10. pragma_query_t Catalog Persistence | 3/3 | Complete    | 2026-03-01 | - |
| 11. CREATE SEMANTIC VIEW Parser Hook | 2/4 | In Progress|  | - |
| 12. EXPLAIN + Typed Output | v0.2.0 | 0/? | Not started | - |
