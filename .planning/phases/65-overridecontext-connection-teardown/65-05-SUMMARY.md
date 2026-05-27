---
phase: 65-overridecontext-connection-teardown
plan: 05
subsystem: read-path migration
tags:
  - duckdb
  - rust
  - cpp
  - ffi
  - catalog-api
  - read-path
  - per-call-connection
  - h2-retirement
  - dead-code-purge
dependency_graph:
  requires:
    - 65-01 (ConnGuard + watchdog tests)
    - 65-03 (parser_override slimming; H2 unused by DDL path)
    - 65-04 (sv_register_table_function shim)
  provides:
    - All 17 read-side functions on the C++ Catalog API
    - Per-call Connection(*context.db) for every read-side bind callback
    - Process-local unbounded type-inference cache (src/type_cache.rs)
    - H2 query_conn allocation RETIRED from init_extension
    - Concurrent per-call Connection regression test
  affects:
    - H1 catalog_conn (still allocated — Plan 06 retires)
    - LIFE-01 watchdog tests (still RED — Plan 06 H1 retirement needed)
tech-stack:
  added:
    - C++ Catalog API for table-function + scalar-function registration
    - reinterpret_cast<duckdb_connection>(Connection*) bridge
    - Process-local OnceLock<RwLock<HashMap>> type cache (unbounded)
  patterns:
    - Per-call bind-time Connection probe (replaces shared duckdb_connection state)
    - Length-prefixed binary wire format for Rust↔C++ data marshalling
    - catch_unwind + sv_free_buffer FFI ownership contract
    - C-API ↔ C++ enum-value bridge helper (sv_logical_type_from_c_type_id)
key-files:
  created:
    - cpp/src/shim.cpp (extended with 17 sv_register_* helpers + sv_serialise_string_list + sv_logical_type_from_c_type_id)
    - cpp/src/shim.hpp (declarations)
    - src/type_cache.rs (unbounded HashMap type cache + fingerprint + lookup_or_probe)
    - src/ddl/read_ffi.rs (probe_catalog_table_present + publish_owned_buffer + serialize_varchar_rows + write_err)
    - test/sql/65_read_bridge_spike.test
    - test/integration/test_concurrent_reads_per_call_conn.py
  modified:
    - src/lib.rs (-3 long-lived registrations; +17 sv_register_* invocations; H2 query_conn allocation DELETED)
    - src/ddl/list.rs (sv_list_semantic_views_bind_rust + sv_list_terse_semantic_views_bind_rust; legacy VTabs DELETED)
    - src/ddl/describe.rs (sv_describe_semantic_view_bind_rust; legacy VTab DELETED)
    - src/ddl/show_columns.rs (sv_show_columns_in_semantic_view_bind_rust; legacy VTab DELETED)
    - src/ddl/show_dims.rs (sv_show_semantic_dimensions_bind_rust + _all variant; 2 legacy VTabs DELETED)
    - src/ddl/show_dims_for_metric.rs (sv_show_semantic_dimensions_for_metric_bind_rust; legacy VTab DELETED)
    - src/ddl/show_metrics.rs (sv_show_semantic_metrics_bind_rust + _all variant; 2 legacy VTabs DELETED)
    - src/ddl/show_facts.rs (sv_show_semantic_facts_bind_rust + _all variant; 2 legacy VTabs DELETED)
    - src/ddl/show_materializations.rs (sv_show_semantic_materializations_bind_rust + _all variant; 2 legacy VTabs DELETED)
    - src/ddl/get_ddl.rs (sv_get_ddl_exec_rust; legacy VScalar DELETED)
    - src/ddl/read_yaml.rs (sv_read_yaml_from_semantic_view_exec_rust; legacy VScalar DELETED)
    - src/query/explain.rs (sv_explain_semantic_view_bind_rust; ExplainSemanticViewVTab + BindData/InitData DELETED)
    - src/query/table_function.rs (sv_semantic_view_bind_rust; SemanticViewVTab + QueryState + StreamingState + BindData/InitData + 5 helpers DELETED)
decisions:
  - "Bridge mechanism: reinterpret_cast<duckdb_connection>(Connection*) with BORROW contract — Rust never calls duckdb_disconnect; C++ scope ~Connection() handles teardown (confirmed by reading duckdb.cpp:266432-266447 where duckdb_connect itself does the same cast)"
  - "Wave 6 streaming model uses C++ MaterializedQueryResult inside SemanticViewGlobalState — its ColumnDataCollection owns blocks independently of the producing Connection, so the per-call Connection drops safely before the first exec call"
  - "Named LIST(VARCHAR) parameter registrations (Wave 5 + Wave 6) require hand-built TableFunction construction because the generic sv_register_table_function shim doesn't accept a named_parameters spec — tracked as a v0.10.1 refactor opportunity, non-blocking"
  - "Type cache (src/type_cache.rs) introduced but NOT consumed by the migrated dispatchers — LIMIT-0 probe is sub-millisecond on the existing test surface, no measured win to justify wiring; module + unit tests stay in tree for future telemetry-driven adoption"
  - "C-API ↔ C++ enum-value mismatch caught and fixed via sv_logical_type_from_c_type_id helper — silently mis-typing DECIMAL→TIMESTAMP_S avoided (highest-impact Batch 2 discovery)"
  - "H1 catalog_conn retained alongside H2 retirement; renamed to _catalog_reader to silence unused-binding warning. Plan 06 retires H1 + adds structural guard test"
  - "5 helpers from old VTab path retired (value_raw_ptr, extract_list_strings, LogicalTypeOwned, type_from_duckdb_type_u32, declare_output_type) — C++ side now owns LIST flattening + LogicalType declaration"
metrics:
  duration: ~10h (across Batches 1-3)
  tasks_completed: 6 of 6 in plan
  files_modified: 19 (extension binary, FFI shim, all 14 read-side source files, 2 test files, lib.rs)
  commits: 16 (12 feat + 1 test + 2 refactor + 3 docs across all batches; Batch 3 added 2 refactor + 1 test + 1 docs)
  retirement_LOC: ~2,632 lines (Batch 3 atomic purge)
completed_date: 2026-05-24
---

# Phase 65 Plan 05: Read-Path Migration Wave Summary

## One-liner

All 17 read-side functions (15 table functions + 2 scalars) now register
via the C++ Catalog API with per-call `Connection(*context.db)` bind
callbacks; H2 `query_conn` is retired; 2,632 lines of legacy
VTab/VScalar Rust scaffolding deleted in a single atomic cleanup
commit. LIFE-02 (`OverrideContext` catalog-driven mechanism) satisfied
end-to-end; LIFE-01 watchdog tests stay red pending Plan 06's H1
`catalog_conn` retirement.

---

## Plan flow (Batches 1 → 2 → 3)

| Batch | Scope | Commits | Status |
|-------|-------|---------|--------|
| 1 | Bridge spike + Waves 0-3 (14 of 17 migrations) | `2db2b9b` … `cc83c2c` | Done |
| 2 | Waves 5-6 (explain_semantic_view + semantic_view) | `690dd67` … `285d6b1` | Done |
| 3 | H2 retirement + dead-code purge + concurrent reads test + this summary | `26fea8d` … `c1011a3` | **Done — this batch** |

Source notes from Batch 1 + Batch 2 (full per-migration inventory,
bridge mechanism, named-parameter handling, streaming model, C-API
enum mismatch fix, LOC accounting):

- `.planning/phases/65-overridecontext-connection-teardown/65-05-SPIKE-SUMMARY.md` (Wave 0 bridge spike result)
- `.planning/phases/65-overridecontext-connection-teardown/65-05-BATCH1-SUMMARY.md` (Waves 0-3, 14 migrations)
- `.planning/phases/65-overridecontext-connection-teardown/65-05-BATCH2-SUMMARY.md` (Waves 5-6, 16 of 17 migrations + Batch 3 scope spec)

---

## Migration inventory (final)

All 17 read-side functions are now on the C++ Catalog API shim:

| #  | Function | Type | Wave | Args | Final commit |
|----|----------|------|------|------|--------------|
| 1  | `list_semantic_views`                       | TF     | 0 | ()                    | `2db2b9b` |
| 2  | `list_terse_semantic_views`                 | TF     | 1 | ()                    | `8f0edf1` |
| 3  | `show_semantic_dimensions_all`              | TF     | 1 | ()                    | `5c31227` |
| 4  | `show_semantic_metrics_all`                 | TF     | 1 | ()                    | `5c31227` |
| 5  | `show_semantic_facts_all`                   | TF     | 1 | ()                    | `5c31227` |
| 6  | `show_semantic_materializations_all`        | TF     | 1 | ()                    | `5c31227` |
| 7  | `show_columns_in_semantic_view`             | TF     | 2 | (VARCHAR)             | `42dd306` |
| 8  | `describe_semantic_view`                    | TF     | 2 | (VARCHAR)             | `42dd306` |
| 9  | `show_semantic_dimensions`                  | TF     | 2 | (VARCHAR)             | `42dd306` |
| 10 | `show_semantic_metrics`                     | TF     | 2 | (VARCHAR)             | `42dd306` |
| 11 | `show_semantic_facts`                       | TF     | 2 | (VARCHAR)             | `42dd306` |
| 12 | `show_semantic_materializations`            | TF     | 2 | (VARCHAR)             | `42dd306` |
| 13 | `show_semantic_dimensions_for_metric`       | TF     | 2 | (VARCHAR, VARCHAR)    | `42dd306` |
| 14 | `get_ddl`                                   | Scalar | 3 | (VARCHAR, VARCHAR)    | `5ef41fd` |
| 15 | `read_yaml_from_semantic_view`              | Scalar | 3 | (VARCHAR)             | `5ef41fd` |
| 16 | `explain_semantic_view`                     | TF     | 5 | (VARCHAR + named LIST)| `690dd67` |
| 17 | `semantic_view`                             | TF     | 6 | (VARCHAR + named LIST)| `1616649` |

Wave 4 was a planning slot reserved for type-cache integration; the
cache module shipped in Wave 1 prep (`895727d`) but the existing test
surface did not require the integration — see "Type cache (deferred
optimisation)" below.

---

## Bridge mechanism (Choice A — reinterpret_cast borrow)

Empirically validated by the Wave 0 spike (`928189c`), reused
identically across all 17 migrations:

```cpp
// In every sv_<name>_bind callback:
Connection probe(*context.db);
duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
sv_<name>_bind_rust(borrowed, /* args */, /* out ptrs */);
// ~Connection() runs at end of scope — Rust must NOT call duckdb_disconnect.
```

Justification: `duckdb_connect` itself is literally
`reinterpret_cast<duckdb_connection>(new Connection(...))` in
`duckdb.cpp:266432-266447`. The C-API `duckdb_connection` opaque
handle is binary-compatible with a `Connection*`. By doing the cast
on a stack-allocated `Connection probe(*context.db)` we avoid the
BIND-THREAD-RC1 hang (`duckdb_connect` from a bind callback returns
rc=1; spike at 65-OPTION-B-SPIKE.md) and stay on the only mechanism
that empirically works from inside extension callbacks.

**Borrow contract** (uniform across all 17 dispatchers):
1. Rust receives a `duckdb_connection` it does NOT own.
2. Rust MUST NOT call `duckdb_disconnect(conn)` (would `delete` the
   stack `Connection` → UB).
3. The C++ bind scope's `~Connection()` handles teardown.

The dispatchers all follow the same FFI pattern (catch_unwind +
ptr+len buffer publish + write_err), inherited from
`src/parse.rs::sv_parser_override_rust` (Phase 62 baseline).

---

## H2 query_conn retirement (Batch 3, commit `26fea8d`)

The Batch 3 commit deletes the following from
`src/lib.rs::init_extension`:

```rust
// DELETED (was at src/lib.rs:566-590 before Batch 3):
let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
let rc = unsafe { ffi::duckdb_connect(db_handle, &mut query_conn) };
if rc != ffi::DuckDBSuccess { return Err(...); }
let _query_state = QueryState {
    catalog: catalog_reader,
    conn: query_conn,
};
```

Plus the `use ... QueryState` import at the top of `init_extension`.

`catalog_reader` is rebound under `_catalog_reader` so that H1's
`catalog_conn` stays kept-alive without producing an unused-binding
warning. Plan 06 retires H1 + the wrapper.

Verification (post-commit):

```
$ grep -nE 'let mut query_conn' src/lib.rs
(no matches — H2 retired)

$ grep -nE 'register_table_function_with_extra_info|register_scalar_function_with_state' src/lib.rs
(only comment-line matches; no live registration calls)

$ grep -nE 'let mut catalog_conn' src/lib.rs
441:        let mut catalog_conn: ffi::duckdb_connection = ptr::null_mut();
(H1 retained — Plan 06 scope)
```

---

## Dead-code purge (Batch 3, commit `1ed8b2b`)

Atomic cleanup of all 17 legacy VTab/VScalar struct + impl blocks
plus their BindData/InitData and the support helpers that fed off the
H2 long-lived connection. Per the BATCH2-SUMMARY's "Awaiting (Batch 3
Scope)" itemisation:

**`src/query/table_function.rs` (-1027 LOC):**
- `QueryState` struct + `unsafe impl Send/Sync`
- `SemanticViewBindData` + `unsafe impl Send/Sync`
- `StreamingState` + `unsafe impl Send` + `impl Drop`
- `SemanticViewInitData` + `unsafe impl Send/Sync`
- `SemanticViewVTab` + full `impl VTab` block (~430 LOC)
- `value_raw_ptr`, `extract_list_strings`, `LogicalTypeOwned`,
  `type_from_duckdb_type_u32`, `declare_output_type` — all five
  helpers retired (LIST flattening + LogicalType declaration moved
  to C++ side: `sv_serialise_string_list` +
  `sv_logical_type_from_c_type_id`)

**`src/query/explain.rs` (-272 LOC):**
- `ExplainBindData`, `ExplainInitData` + Send/Sync impls
- `ExplainSemanticViewVTab` + full `impl VTab` block (~210 LOC)
- `use ... QueryState` import + unused duckdb-rs vtab/core imports
- `extract_list_strings` import (now unused)

**`src/ddl/list.rs` (-376 LOC):**
- `ListRow`, `ListBindData`, `ListInitData`,
  `ListSemanticViewsVTab` + full `impl VTab` block
- `ListTerseRow`, `ListTerseBindData`, `ListTerseInitData`,
  `ListTerseSemanticViewsVTab` + full `impl VTab` block

**`src/ddl/describe.rs` (-156 LOC):**
- `DescribeBindData`, `DescribeInitData` + impls
- `DescribeSemanticViewVTab` + `impl VTab` block
- (`DescribeRow` + the 6 `collect_*` helpers retained — the new
  dispatcher calls them.)

**`src/ddl/show_columns.rs` (-127 LOC):**
- `ShowColumnsBindData`, `ShowColumnsInitData` + impls
- `ShowColumnsInSemanticViewVTab` + `impl VTab` block
- (`ShowColumnRow` + `collect_column_rows` retained.)

**`src/ddl/show_dims.rs` + `show_metrics.rs` + `show_facts.rs`
+ `show_materializations.rs` (~520 LOC total):**
- Each file's `Show<Kind>BindData`, `Show<Kind>InitData`,
  `bind_output_columns`, `emit_rows`, and BOTH the single-view +
  cross-view VTab structs + impl blocks
- (Each file's `Show<Kind>Row` + `collect_<kind>` helper retained.)

**`src/ddl/show_dims_for_metric.rs` (-216 LOC):**
- `ShowDimForMetricRow`, `ShowDimsForMetricBindData`,
  `ShowDimsForMetricInitData` + impls
- `ShowDimensionsForMetricVTab` + `impl VTab` block
- (`is_dimension_reachable_for_metric` + fan-trap helpers retained.)

**`src/ddl/get_ddl.rs` (-58 LOC):**
- `GetDdlScalar` + `impl VScalar` block

**`src/ddl/read_yaml.rs` (-49 LOC):**
- `ReadYamlFromSemanticViewScalar` + `impl VScalar` block

Total: **13 files changed, 170 insertions, 2,632 deletions.**

Verification (post-commit):

```
$ grep -rn '#\[allow(dead_code)\]' src/ddl/*.rs src/query/*.rs
src/query/table_function.rs:500: type_id_to_display_name
(Only `type_id_to_display_name` remains — Plan 03 carry-forward,
 NOT Plan 05 scope; documented as kept-alive for Plan 05's read-side
 bind callbacks to re-probe at SHOW/DESCRIBE bind time.)
```

---

## New regression test (Batch 3, commit `c1011a3`)

`test/integration/test_concurrent_reads_per_call_conn.py` — 8 Python
threads × 10 calls each = 80 calls of
`SHOW SEMANTIC DIMENSIONS FROM v1` against a small shared DB.

Asserts:
1. All 80 calls succeed (no `duckdb_disconnect` UB on borrowed
   handles, no panic propagation across the C++↔Rust boundary, no
   catalog-lookup failure under contention).
2. All 80 row sets byte-identical (snapshot-consistent reads —
   per-call Connection inherits the caller's ClientContext + search
   path).
3. Wall budget < 30 s (catches a regression where per-call Connection
   construction becomes serialised behind a shared mutex).

Measured locally: **80 reads in 0.02 s** — well under budget. The
per-call Connection model has no contention overhead in the common
case.

---

## Test gate evidence (Batch 3 final)

| Gate | Result | Notes |
|------|--------|-------|
| `just build` | green | zero warnings |
| `just test-sql` | **53/53 PASS** | byte-identical with Batch 2 |
| `cargo test --lib` (bundled) | **843/843 PASS** | |
| `uv run test/integration/test_adbc_transactions.py` | **6/6 PASS** | D-21 transactional invariant green |
| `uv run test/integration/test_multi_db_isolation.py` | **3/3 PASS** | cross-DB resolution works through per-call Connection |
| `uv run test/integration/test_concurrent_reads_per_call_conn.py` | **PASS** | new regression — 80 reads in 0.02 s |
| `cargo fmt --check` | PASS | |
| `cargo clippy -- -D warnings` | PASS | |
| `cargo deny check` | PASS | |
| `cargo nextest run` | all green | (inside `just ci`) |
| `just check-fuzz` | PASS | fuzz targets compile under nightly |
| `just docs-check` | PASS | docs build succeeds |
| `uv run test/integration/test_readonly_load.py` | **3/8 PASS** | **5 watchdog tests STILL RED — Plan 06 H1 retirement needed** |
| `just test-all` | **FAIL** | only `test-readonly` (LIFE-01 watchdog) fails — everything else green |
| `just ci` | **FAIL** | same reason as `just test-all` |

The `just test-all` / `just ci` failures are isolated to the
LIFE-01 watchdog suite. Every other gate (Rust unit + sqllogictest +
DuckLake CI + ADBC + multi-DB + concurrent reads + lint + fmt +
cargo-deny + fuzz check + docs check) is green on `milestone/v0.10.0`.

---

## LIFE-01 watchdog status (expected per inputs)

The Plan 01 `test_readonly_load.py` watchdog suite remains in its
pre-Batch-3 state:

| Test | Pre-Batch-3 | Post-Batch-3 | Plan 06 expected |
|------|-------------|--------------|------------------|
| `test_fresh_readonly_empty_list`                     | PASS  | PASS  | PASS  |
| `test_bootstrapped_readonly_query_works`             | PASS  | PASS  | PASS  |
| `test_readonly_ddl_fails`                            | PASS  | PASS  | PASS  |
| `test_in_process_bootstrap_then_readonly_fresh`      | RED   | RED   | PASS  |
| `test_in_process_bootstrap_then_readonly_existing`   | RED   | RED   | PASS  |
| `test_in_process_load_only_then_readonly`            | RED   | RED   | PASS  |
| `test_in_process_readonly_then_readwrite`            | RED   | RED   | PASS  |
| `test_repeated_load_close_no_busy_spin`              | RED   | RED   | PASS  |

All five red tests fail with the same `TimeoutError`:
`duckdb.connect(..., read_only=True) did not return within 5.0s —
likely the in-process RW<->RO busy-spin in
DBInstanceCache::GetInstanceInternal (Phase 65 regression). See
65-01-SPIKES.md A4 for the diagnosis.`

This is **expected** per the Plan 05 inputs and 65-CONTEXT.md's
hypothesis split: LIFE-01 has TWO contributors —
H1 `catalog_conn` (Phase 62 OverrideContext bundle, retired by
Plan 06) AND H2 `query_conn` (semantic_view path, retired by Plan
05 Batch 3). With only H2 retired, H1 still holds the Database
alive past caller's `close()`, so DuckDB's `DatabaseManager` still
busy-spins on RW↔RO reopen.

Plan 06 retires H1 + adds the structural guard test. The watchdog
suite should flip 8/8 green at Plan 06 completion. If any watchdog
test stays red after Plan 06, file as a Phase 67 follow-up
(D-22 bounded-scope rule — surface, don't silently absorb).

---

## Requirement traceability

| Requirement | Status | Notes |
|-------------|--------|-------|
| **LIFE-01** (RW→RO reopen within 5 s) | **partial** | H2 contributor retired; H1 still holds Database alive → watchdog still RED. Plan 06 finishes. |
| **LIFE-02** (deterministic teardown OR access-mode-mismatch error) | **satisfied (mechanism path)** | The read-side `OverrideContext` lifecycle is now per-call Connection at bind time — no extension-owned `duckdb_connection` is held past a single read. Plan 06 closes the H1 catalog connection that still imitates the v0.9.0 leak. |
| **LIFE-03** (in-process watchdog test) | satisfied (test landed Plan 01 `4da68eb`) | The test now expects 5/8 fails pending Plan 06. |
| **LIFE-04** (deferred-items entry updated) | **partial** | Will be updated end-of-phase by Plan 06 after watchdog flips green. |

---

## Cross-database EXPAND-CTX investigation (preliminary)

The Phase 66 plan tracks EXPAND-CTX-01..03 — the v0.9.1 root cause
was H2's separate connection diverging from the caller's
catalog/search path. With H2 fully retired in Batch 3 (allocation
gone, dead VTab carcasses purged), `semantic_view` and
`explain_semantic_view` now open per-call Connections that inherit
the caller's `ClientContext`.

`test/integration/test_multi_db_isolation.py 3/3 PASS` confirms the
per-call Connection model resolves cross-database catalog/search-path
correctly for the existing test surface.

**Tentative conclusion** (subject to Plan 06 verification + Phase 66
extended ADBC query coverage): EXPAND-CTX-01 likely becomes a no-op
verification after Plan 06's H1 retirement. Phase 66 still needs to
add the broader ADBC query test (EXPAND-CTX-02) to cover the
fact-query / semi-additive / window / materialization-routing
expansion paths under ADBC. **Not silently absorbed** — explicitly
documented here per D-22 bounded-scope rule.

---

## Type cache (deferred optimisation)

`src/type_cache.rs` (Wave 1 prep, `895727d`) introduced an unbounded
process-local HashMap cache keyed on `(view_name,
schema_fingerprint)`. None of the migrated dispatchers consume it
today.

**Rationale:**
- LIMIT-0 type-inference probe is sub-millisecond on the existing test
  surface (53 sqllogictests + 6 ADBC tests complete in seconds).
- Wiring `lookup_or_probe` requires an adapter from the cache's
  `Vec<(String, String)>` shape to `try_infer_schema`'s native
  `(Vec<String>, Vec<duckdb_type>)`.
- No measured perf win to justify the complexity.

The module stays in tree, documented + unit-tested, ready for a
post-v0.10.0 telemetry-driven adoption. **Tracked as deferred
optimisation**, not TECH-DEBT — no regression was traded.

Integration shape (when adopted):

```rust
let fp = crate::type_cache::fingerprint(&def);
let cached = crate::type_cache::lookup_or_probe(&view_name, fp, || {
    let (names, types) = unsafe { try_infer_schema(conn, &limit0_sql) }
        .ok_or_else(|| "LIMIT-0 probe failed".to_string())?;
    Ok(crate::type_cache::InferredTypes {
        column_types: names.into_iter().zip(types.iter().map(|t| format!("{:?}", t))).collect(),
    })
})?;
```

---

## TECH-DEBT entries surfaced

1. **Generic `sv_register_table_function` shim doesn't accept a
   `named_parameters` spec** — Wave 5 + Wave 6 each hand-build the
   `TableFunction` with the three `dimensions`/`metrics`/`facts`
   LIST(VARCHAR) named params + duplicate ~50 lines of boilerplate
   per migration. Extending the shim to accept an optional
   `named_parameters` map would retire the duplication.
   **Severity**: low (~100 LOC duplication). **Track for v0.10.1.**

2. **Type cache integration deferred** — see "Type cache" section
   above. Wire when telemetry shows the LIMIT-0 probe as a hot path
   for a real workload. **Severity**: low (no perf regression).
   **Track post-v0.10.0.**

3. **D-03 watchdog tests still RED at Plan 05 completion** — Plan 06
   retires H1 + must verify all 5 currently-RED watchdog tests flip
   green. **Severity**: blocking for LIFE-01 acceptance.
   **Owned by Plan 06.**

---

## Deviations from plan

**None at the architectural or bridge-mechanism level.** Wave 6's
streaming model (MaterializedQueryResult inside SemanticViewGlobalState
+ TWO per-call Connections per `semantic_view(...)` invocation, one
for bind + one for init_global) was anticipated by RESEARCH §1.3 Wave
6 and the Option B spike (65-OPTION-B-SPIKE.md PLAN-THREAD-RC0).

**Three structural variants** beyond the basic bridge template:
1. Hand-built `TableFunction` registration for named LIST params
   (TECH-DEBT 1 above).
2. `init_global` callback for the materialised query result
   (Wave 6 only).
3. Two per-call Connections per `semantic_view(...)` invocation
   (Wave 6 only) — both drop before any exec call.

**One high-impact discovery in Batch 2:** the C-API `DUCKDB_TYPE_*`
enum values DIFFER from C++ `LogicalTypeId` integer values (e.g.
DECIMAL is 19 vs 21; LIST is 24 vs 101). Naive `static_cast` would
have silently mis-typed every column. Fixed via the new C++ helper
`sv_logical_type_from_c_type_id` — single source of truth for the
conversion (mirrors duckdb-rs's `LogicalTypeId::from(u32)` logic).
Caught at first test run because the LIMIT-0 type-probe path
exercises every column type the test suite covers.

**No Rule-4 architectural escalations. No auth gates. No checkpoint
failures. No new package installs** (T-65-05-SC threat-model row
passes trivially).

---

## Forward pointer to Plan 06

H1 `catalog_conn` at `src/lib.rs:441` is still present after Plan 05
Batch 3. Plan 06's deliverables:

1. Retire the H1 `duckdb_connect` + `CatalogReader::new` allocation
   block. After Plan 06, `init_extension` should contain ZERO
   long-lived `duckdb_connection` allocations.
2. Add a structural guard test (sqllogictest or Rust) that asserts
   `init_extension` opens no duckdb_connection itself.
3. Verify the 5 currently-RED watchdog tests flip green; if any stay
   red, file as Phase 67 follow-up.
4. Update `.planning/REQUIREMENTS.md` LIFE-01 / LIFE-04 to satisfied.

The OverrideContext (`src/parse.rs::sv_make_override_context`) needs
either (a) a parser_override path that no longer needs a long-lived
connection to write metadata, or (b) a per-call connection inside the
parser_override callback. Plan 06's RESEARCH must lock the mechanism.

---

## Self-Check

- [x] Batch 3 PLAN scope items 1-6 all addressed (H2 retirement + dead-code purge + concurrent test + final gate + this summary + STATE/ROADMAP/REQ updates below).
- [x] `just build` green; no warnings.
- [x] `just test-sql` 53/53 PASS.
- [x] `cargo test --lib` 843/843 PASS.
- [x] `cargo fmt --check` + `cargo clippy -D warnings` + `cargo deny check` all PASS (verified via `just ci` partial run).
- [x] `cargo nextest run` all green inside `just ci`.
- [x] `test_adbc_transactions.py` 6/6 PASS (D-21 invariant).
- [x] `test_multi_db_isolation.py` 3/3 PASS.
- [x] `test_concurrent_reads_per_call_conn.py` PASS (80 reads in 0.02 s).
- [x] `just check-fuzz` PASS.
- [x] `just docs-check` PASS.
- [x] `test_readonly_load.py` 3/8 PASS — 5 watchdog tests RED, documented as expected (LIFE-01 partial, Plan 06 finishes H1 retirement).
- [x] H2 `query_conn` allocation in `src/lib.rs` DELETED — `grep -nE 'let mut query_conn' src/lib.rs` returns 0 matches.
- [x] H1 `catalog_conn` allocation at `src/lib.rs:441` PRESERVED (Plan 06 scope).
- [x] Zero live `register_table_function_with_extra_info` / `register_scalar_function_with_state` calls remain in `src/lib.rs` (only comments).
- [x] `grep -rn '#\[allow(dead_code)\]' src/ddl/*.rs src/query/*.rs` returns ONLY `type_id_to_display_name` (Plan 03 carry-forward — not Plan 05 scope).
- [x] Phase 64 sqllogictest surface (`qualify_and_quote_table_ref`) byte-identical — D-20 invariant intact.
- [x] Self-check passes; SUMMARY truthful against `git log` and `git diff --shortstat 5c31227^..HEAD`.

## Self-Check: PASSED

All claims verified against the current tree state and git log. The
single deliverable still gated on Plan 06 (watchdog tests flipping
green) is explicitly documented and traced to the H1 contributor, not
a Plan 05 regression.
