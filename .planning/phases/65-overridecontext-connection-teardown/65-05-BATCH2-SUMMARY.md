# Phase 65 Plan 05 — Batch 2 Summary (Waves 5–6 of 6)

**Phase:** 65-overridecontext-connection-teardown
**Plan:** 05 (read-path migration wave)
**Batch:** 2 of 3 (Waves 5 + 6 — 16 of 17 migrations complete; H2 still in tree)
**Status:** **DONE — awaiting orchestrator dispatch of Batch 3 (H2 retirement + dead-VTab cleanup commit)**
**Branch:** `milestone/v0.10.0`
**HEAD:** `1616649`

---

## One-liner

The final two read-side migrations (`explain_semantic_view` and
`semantic_view` — the highest-blast-radius of the 17) are now on the C++
Catalog API via per-call `Connection probe(*context.db)` bridges. 16 of
17 read-side registrations done; only the H2 `query_conn` allocation +
the 17 dead legacy VTab/VScalar carcasses remain for Batch 3's atomic
cleanup commit. Zero regressions across the 53-test sqllogictest suite,
the 6-test ADBC transactional contract, and the 3-test multi-DB
isolation suite.

---

## Migration inventory (Batch 2 additions, 15-16 of 17)

| #  | Function | Type | Wave | Commit | Notes |
|----|----------|------|------|--------|-------|
| 16 | `explain_semantic_view` | TF(VARCHAR, dimensions/metrics/facts := LIST(VARCHAR)) | 5 | `690dd67` | Varargs + named-param handling (new for Plan 05) |
| 17 | `semantic_view`         | TF(VARCHAR, dimensions/metrics/facts := LIST(VARCHAR)) | 6 | `1616649` | Main expansion path; streaming via MaterializedQueryResult in GlobalState |

**Batch 1** (commits `2db2b9b` through `5ef41fd`, summary `cc83c2c`)
migrated the other 14: list_semantic_views (Wave 0), list_terse + 4
"_all" siblings (Wave 1), 6 single-view SHOW + describe + show_columns +
show_semantic_dimensions_for_metric (Wave 2), get_ddl +
read_yaml_from_semantic_view scalars (Wave 3).

---

## Bridge mechanism (unchanged across all 16 migrations)

```cpp
// Wave 5/6 bind callbacks:
Connection probe(*context.db);
duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

// Wave 6 init_global callback (additionally):
Connection probe(*context.db);  // separate per-call Connection
auto qresult = probe.Query(execution_sql);  // materialised result
// Connection drops here; ColumnDataCollection owns its storage
```

**Borrow contract**: Rust dispatcher never calls `duckdb_disconnect`.
C++ scope `~Connection()` handles teardown. Identical convention to all
14 Batch-1 migrations.

**New for Wave 6**: TWO per-call Connections per `semantic_view(...)`
invocation — one for bind (catalog probe + LIMIT 0 type inference), one
for init_global (the actual materialised query). Both drop before any
exec call runs. The `MaterializedQueryResult` is self-contained (its
`ColumnDataCollection` owns blocks independently of the Connection per
duckdb.hpp:18801-18813), so the per-call Connection model is safe.

---

## Named LIST(VARCHAR) parameter handling (new for Plan 05)

Waves 5 + 6 both register table functions with three named LIST(VARCHAR)
parameters (`dimensions`, `metrics`, `facts`). The existing generic
`sv_register_table_function` shim does not accept a named-parameter
spec, so both migrations construct `TableFunction` by hand in dedicated
`sv_register_<name>_impl` helpers:

```cpp
TableFunction tf(name, args, exec_cb, bind_cb, init_global, init_local);
tf.named_parameters["dimensions"] = LogicalType::LIST(LogicalType::VARCHAR);
tf.named_parameters["metrics"]    = LogicalType::LIST(LogicalType::VARCHAR);
tf.named_parameters["facts"]      = LogicalType::LIST(LogicalType::VARCHAR);
CreateTableFunctionInfo info(tf);
info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;
Catalog::GetSystemCatalog(db).CreateTableFunction(
    CatalogTransaction::GetSystemTransaction(db), info);
```

The three optional named parameters are flattened on the C++ side via
the new `sv_serialise_string_list` helper into the standard
length-prefixed wire format (`u32 count; for each: u32 byte_len +
bytes`) and threaded through to the Rust dispatcher as `(ptr, len)`
tuples. Missing named params pass as `nullptr+0` (treated as an empty
list by Rust's `sv_parse_string_list`).

**TECH-DEBT note** (non-blocking): the generic `sv_register_table_function`
shim could be extended to accept an optional `named_parameters` map.
Today the two named-param migrations duplicate ~50 lines of
boilerplate each. Tracked as a small refactor opportunity for v0.10.1;
not blocking Plan 06 or v0.10.0 release.

---

## Streaming model (Wave 6 only — semantic_view specifics)

The legacy `SemanticViewVTab::func()` used a Mutex-guarded `Option<StreamingState>`
that held the raw `ffi::duckdb_result` from the long-lived `query_conn`.
First exec call would run `duckdb_query(state.conn, execution_sql, &result)`;
subsequent calls fetched chunks via `duckdb_result_get_chunk(result, idx)`
and zero-copy referenced source vectors into the output DataChunk via
`duckdb_vector_reference_vector`.

The new model uses the C++ MaterializedQueryResult instead:

1. **Bind** returns schema + execution_sql via wire format. C++ side
   parses schema, runs DECIMAL/LIST logical-type probe (LIMIT 0 on a
   per-call Connection — same lifecycle as bind's primary Connection),
   declares output columns, stashes execution_sql in BindData.

2. **init_global** opens a per-call `Connection probe(*context.db)`,
   runs `probe.Query(execution_sql)`. The returned
   `unique_ptr<QueryResult>` is downcast to
   `unique_ptr<MaterializedQueryResult>` and stashed in
   `SemanticViewGlobalState`. The Connection drops at end of scope;
   the result's `ColumnDataCollection` owns its blocks (per
   duckdb.hpp:18810-18813 — `data scanned will be valid even after the
   column data collection is destroyed` when allocator outlives the
   blocks).

3. **func** calls `gs.result->Fetch()` per invocation. Returns nullptr
   when exhausted. For each column, `dst.Reference(src)` for zero-copy
   when types match (which they always do because bind declares output
   types from the same schema the query produces); falls back to
   `VectorOperations::DefaultCast` defensively.

**Behavioural delta from legacy**: the Rust impl had a separate
type-mismatch check (`QueryError::TypeMismatch`) that returned an error
when src and dst type IDs diverged. The new C++ path falls back to
`DefaultCast` instead, which is strictly more permissive (avoids a
test-surface regression for queries where bind's declared type is
slightly wider than the runtime type after optimisation — the
build_execution_sql cast wrapper handles the common cases).

---

## C-API ↔ C++ enum-value mismatch (discovered + fixed during Wave 6)

The Rust dispatcher returns C-API `DUCKDB_TYPE_*` enum values (from
`ffi::duckdb_column_type`) but the C++ `LogicalTypeId` enum has
DIFFERENT integer values:

| Name | C-API value | C++ LogicalTypeId value |
|------|-------------|-------------------------|
| DECIMAL | 19 | 21 |
| LIST | 24 | 101 |
| ENUM | 23 | 104 |
| BOOLEAN | 1 | 10 |
| INTEGER | 4 | 13 |
| BIGINT | 5 | 14 |
| ... | ... | ... |

A naive `static_cast<LogicalTypeId>(c_api_type_id)` would silently
mis-type every column. The Rust side has equivalent logic in
`type_from_duckdb_type_u32` (relies on `LogicalTypeId::from(u32)` —
duckdb-rs's conversion from C-API enum values).

**Fix**: new `sv_logical_type_from_c_type_id(uint32_t)` C++ helper that
is the single source of truth for the conversion. Mirrors
`type_from_duckdb_type_u32` / `declare_output_type` in
`src/query/table_function.rs`. Normalises HUGEINT/UHUGEINT to
BIGINT/UBIGINT, declares ENUM/STRUCT/MAP/INVALID as VARCHAR fallback,
returns placeholder LogicalType for DECIMAL/LIST so the LIMIT-0 probe
loop can detect them and substitute the probed LogicalType.

**This is the highest-impact discovery in Batch 2** — silently
mis-typing DECIMAL columns to TIMESTAMP_S (LogicalTypeId 17) would have
been a nasty production bug. Caught at build/test time because the
LIMIT-0 type-probe path exercises every column type the test suite
covers; the helper landed before the first test run.

---

## Type cache (intentionally not consumed in Batch 2)

The `src/type_cache.rs` module from Wave 1 prep (commit `895727d`) was
designed to memoise LIMIT-0 type-inference probes keyed on `(view_name,
schema_fingerprint)`. Wave 6's `semantic_view` dispatcher does NOT
consume it.

**Rationale**:
- The LIMIT-0 probe is well under a millisecond per call on the existing
  test surface (53 sqllogictests + 6 ADBC tests complete in seconds).
- Wiring through `type_cache::lookup_or_probe(view_name,
  fingerprint(&json_str), || try_infer_schema(conn, &limit0_sql))` adds
  a `Vec<(String, String)>` clone per cache hit (vs the current native
  `(Vec<String>, Vec<duckdb_type>)` shape from `try_infer_schema`).
  Would require an adapter layer to convert.
- No measured perf win on the existing test surface to justify the
  complexity.

The module stays in tree, documented + unit-tested, ready for a future
follow-up. **Tracked as deferred optimisation** in this summary; not a
TECH-DEBT entry because no regression was traded — the per-bind probe
matches the legacy CREATE-time persisted-types lookup's effective cost.

If post-v0.10.0 telemetry shows the LIMIT-0 probe as a hot path for any
realistic workload, the integration shape is:

```rust
// in sv_semantic_view_bind_rust, in place of the inline try_infer_schema call:
let fp = crate::type_cache::fingerprint(&json_str);
let cached = crate::type_cache::lookup_or_probe(&view_name, fp, || {
    let limit0_sql = format!("{expanded_sql} LIMIT 0");
    let (names, types) = unsafe { try_infer_schema(conn, &limit0_sql) }
        .ok_or_else(|| "LIMIT-0 probe failed".to_string())?;
    Ok(crate::type_cache::InferredTypes {
        column_types: names.into_iter().zip(types.iter().map(|t| format!("{:?}", t))).collect(),
    })
})?;
```

---

## Deviations from the spike + batch-1 templates

**None at the bridge-mechanism level.** Both Wave 5 and Wave 6 use
exactly the Choice A `reinterpret_cast` pattern + the same Rust
`catch_unwind` + `publish_owned_buffer` + `write_err` shape established
by Wave 0.

**Three structural variants** introduced for Batch 2:

1. **Hand-built TableFunction registration** for named LIST(VARCHAR)
   parameters (both Wave 5 + Wave 6). The generic
   `sv_register_table_function` shim doesn't accept named-params spec;
   each migration has its own `sv_register_<name>_impl` that constructs
   the `TableFunction` and calls `Catalog::CreateTableFunction` directly.
   Could be refactored into a generic `sv_register_table_function_with_
   named_params` shim in a follow-up commit (not for v0.10.0 release).

2. **init_global callback** (Wave 6 only). The first 15 migrations
   register only `init_local`; Wave 6 needs `init_global` because the
   materialised query result must live across multiple exec calls and
   is naturally global state (not thread-local).

3. **Two per-call Connections per invocation** (Wave 6 only). Bind opens
   one for catalog lookup + LIMIT 0 type inference; init_global opens a
   separate one for the materialised query. Both drop before any exec
   call. The lifecycle is well-defined — neither Connection outlives its
   own scope, and the MaterializedQueryResult is self-contained.

**No Rule-4 architectural escalations.** No auth gates. No checkpoint
failures. No new package installs (T-65-05-SC threat-model row passes
trivially).

---

## LOC delta

```
$ git diff --shortstat 690dd67^..HEAD
 5 files changed, 1398 insertions(+), 19 deletions(-)
```

Per-migration:
- **Wave 5 (`690dd67`):** ~590 insertions across `cpp/src/shim.cpp` (+220),
  `cpp/src/shim.hpp` (+22), `src/lib.rs` (+34), `src/query/explain.rs`
  (+321). Includes the new `sv_serialise_string_list` C++ helper and the
  generic dispatcher pattern reused by Wave 6.
- **Wave 6 (`1616649`):** ~808 insertions across `cpp/src/shim.cpp` (+372,
  including `sv_logical_type_from_c_type_id` enum-bridge helper),
  `src/lib.rs` (+34), `src/query/table_function.rs` (+388). The Rust
  dispatcher is ~400 LOC including doc headers; the C++ side adds the
  enum-bridge helper, `sv_resolve_output_logical_types` (DECIMAL/LIST
  probe), `sv_semantic_view_bind` + `sv_semantic_view_init_global` +
  `sv_semantic_view_function` + `sv_register_semantic_view_impl`.

**Spike summary's extrapolation** (`65-05-SPIKE-SUMMARY.md` §e) predicted
~250-400 LOC for Wave 5 + Wave 6 each. Actual: 590 + 808 = ~1400 LOC
total. Higher than the high-end estimate primarily because the named-
param + init_global wiring did not exist in the spike or Batch 1
templates; both new patterns landed in Wave 5 + Wave 6 simultaneously
and required hand-built TableFunction registration. The
`sv_logical_type_from_c_type_id` enum-bridge helper (~50 LOC) was an
unanticipated discovery — the spike and Batch 1 dispatchers all returned
VARCHAR-only rows, so the C-API ↔ C++ enum-value mismatch never surfaced.

**Net retirement so far:** 0 LOC retired in Batch 2 — legacy VTab
carcasses still in tree under `#[allow(dead_code)]`. Batch 3 cleanup
commit deletes them together (estimated ~2000-2500 LOC retirement,
plus the H2 query_conn allocation block at `src/lib.rs:570-589`).

---

## Test gate evidence

**`just build`** — clean (extension binary materialised at
`build/debug/semantic_views.duckdb_extension`).

**`just test-sql`** — `53 tests run, 0 failed` after each of the two
Batch-2 commits. Specifically:

- `test/sql/phase4_query.test` (basic semantic_view query) — PASS.
- `test/sql/phase28_e2e.test` (full e2e exercising semantic_view +
  explain_semantic_view together) — PASS.
- `test/sql/phase46_wildcard.test` (wildcard expansion in
  semantic_view) — PASS.
- `test/sql/phase46_fact_query.test` (fact-query path through
  semantic_view) — PASS.
- `test/sql/phase47_semi_additive.test` (semi-additive metrics through
  semantic_view) — PASS.
- `test/sql/phase48_window_metrics.test` (window-function metrics) —
  PASS.
- `test/sql/phase55_materialization_routing.test` (materialisation
  routing via semantic_view) — PASS.
- `test/sql/phase57_introspection.test` (broad introspection coverage
  including explain_semantic_view) — PASS.
- `test/sql/phase64_quoted_idents.test` (Phase 64 D-20 invariant —
  qualify_and_quote_table_ref wired sites untouched) — PASS.

**`uv run test/integration/test_adbc_transactions.py`** — 6/6 PASS
(D-21 transactional invariant). Confirms the read-path migration did
NOT regress the write-path connection model — write-side DDL still
participates in the caller's transaction (parser_override path is
unchanged).

**`uv run test/integration/test_multi_db_isolation.py`** — 3/3 PASS.
This is the most sensitive surface for the per-call Connection model
because per-call Connections inherit the caller's
`ClientContext.catalog/search_path`. If the per-call model broke
cross-database resolution, this would catch it. (Phase 66's EXPAND-CTX-01
investigation is now likely a no-op verification — the root cause was
expected to dissolve when H2 retired. With semantic_view migrated, it
appears to have dissolved already; full verification belongs in Plan 06
after H1 also retires.)

**Acceptance grep checks:**

```
$ grep -nE 'register_table_function_with_extra_info|register_scalar_function_with_state' src/lib.rs
380:        // Replaces the duckdb-rs `register_table_function_with_extra_info`
388:        // Replaces the duckdb-rs `register_table_function_with_extra_info`
593:        // the duckdb-rs `register_table_function_with_extra_info` path
604:        // off the duckdb-rs `register_table_function_with_extra_info` path
(All matches are COMMENTS only — no live `register_table_function_with_extra_info`
 or `register_scalar_function_with_state` calls remain in src/lib.rs.)

$ grep -nE 'let mut query_conn' src/lib.rs
577:        let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
(H2 query_conn still allocated — Plan 05 Batch 3 cleanup deletes the
 entire allocation block + the dead VTab carcasses that reference QueryState.)

$ grep -nE 'register_table_function_with_extra_info' src/query/table_function.rs src/query/explain.rs
(no matches — both legacy VTab impl blocks rely on the
 `impl VTab for ...` trait registration which is now unreached because
 src/lib.rs no longer calls register_table_function_with_extra_info.)
```

---

## Cross-database EXPAND-CTX investigation (preliminary finding)

The Phase 66 plan tracks EXPAND-CTX-01..03 as a follow-up — the v0.9.1
root cause was H2's separate connection diverging from the caller's
catalog/search path. With H2 retired in this Batch (in spirit — the
allocation is still present but no live code path consumes it),
`semantic_view` and `explain_semantic_view` now open per-call
Connections that inherit the caller's `ClientContext`.

**`test/integration/test_multi_db_isolation.py` 3/3 PASS** confirms the
per-call Connection model resolves cross-database catalog/search-path
correctly for the existing test surface. Phase 66's full EXPAND-CTX
verification belongs in Plan 06's wake (after H1 also retires + the
structural guard test ensures no future regression re-introduces a
long-lived connection).

**Tentative conclusion** (subject to Plan 06 verification): EXPAND-CTX-01
is likely a no-op verification after Plan 05 + Plan 06 ship. No
follow-up phase needed beyond Phase 66's already-planned watchdog test
extensions. **Not silently absorbed** — explicitly documented as a
finding here per D-22 bounded-scope rule. If Plan 06's watchdog suite
surfaces a residual divergence, file as a Phase 67 follow-up.

---

## Forward pointer: Batch 3 (H2 retirement + dead-VTab cleanup commit)

Batch 3 is a single atomic commit that performs the following deletions:

### H2 query_conn allocation block (`src/lib.rs`)

Delete lines ~570-589 of `src/lib.rs` (the block that allocates
`query_conn` via `duckdb_connect`, builds `_query_state`, and currently
has the comment "STILL allocated here for one wave"). After deletion:

```rust
// (deleted): let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
// (deleted): let rc = unsafe { ffi::duckdb_connect(db_handle, &mut query_conn) };
// (deleted): if rc != ffi::DuckDBSuccess { return Err(...); }
// (deleted): let _query_state = QueryState { catalog: catalog_reader, conn: query_conn };
```

`catalog_reader` is still consumed by `sv_register_parser_hooks` (Phase
62 OverrideContext bundle) so its allocation at line ~448 stays. Plan
06 retires H1 (`catalog_conn` at lines 421-446) + adds the structural
guard test.

### Dead VTab/VScalar carcasses (17 structs across 14 files)

Delete the following `#[allow(dead_code)]`-marked items together:

**`src/query/table_function.rs`:**
- `QueryState` (lines 32-37)
- `SemanticViewBindData` (lines ~424-440)
- `StreamingState` (lines ~445-460)
- `SemanticViewInitData` (lines ~463-470)
- `SemanticViewVTab` (struct + `impl VTab` block, lines ~480-880)

**`src/query/explain.rs`:**
- `ExplainBindData` (lines ~38-44)
- `ExplainInitData` (lines ~47-54)
- `ExplainSemanticViewVTab` (struct + `impl VTab` block, lines
  ~125-330)

**`src/ddl/list.rs`:**
- `ListRow`, `ListBindData`, `ListInitData`, `ListSemanticViewsVTab`
  (lines ~249-391)
- `ListTerseRow`, `ListTerseBindData`, `ListTerseInitData`,
  `ListTerseSemanticViewsVTab` (lines ~492-621)

**`src/ddl/describe.rs`:**
- `DescribeBindData`, `DescribeInitData`, `DescribeSemanticViewVTab`
  (legacy impl block — search for `#[allow(dead_code)]` markers; the
  exact line numbers will have shifted slightly after Batch 2 lands).

**`src/ddl/show_columns.rs`, `src/ddl/show_dims.rs`,
`src/ddl/show_dims_for_metric.rs`, `src/ddl/show_metrics.rs`,
`src/ddl/show_facts.rs`, `src/ddl/show_materializations.rs`:**
- Each file has 1-2 legacy VTab structs + their BindData/InitData
  marked `#[allow(dead_code)]`. Delete all of them and any
  `impl VTab for ...` blocks.

**`src/ddl/get_ddl.rs`:**
- `GetDdlScalar` + any associated `VScalar` impl block under
  `#[allow(dead_code)]`.

**`src/ddl/read_yaml.rs`:**
- `ReadYamlFromSemanticViewScalar` + `VScalar` impl block under
  `#[allow(dead_code)]`.

Recommended discovery command:

```bash
grep -rn '#\[allow(dead_code)\]' src/ddl/*.rs src/query/*.rs
```

Each match identifies a struct or impl block that Batch 3 deletes.

### Other Batch-3 deliverables (per PLAN Task 6 Step D)

- **`test/integration/test_concurrent_reads_per_call_conn.py`** — 8
  parallel Python threads × 10 calls each = 80 calls of `SHOW SEMANTIC
  DIMENSIONS FROM v1`. Asserts no contention, all 80 calls succeed,
  identical row sets across threads. Pattern from
  `test_concurrent_ddl.py`.
- **`just test-all` + `just ci`** both green on `milestone/v0.10.0`.
- **65-05-SUMMARY.md** (full plan summary) covering Waves 0-6 + all
  retirement evidence.

### Plan 06 boundary (post-Batch-3)

H1 `catalog_conn` at `src/lib.rs:421-446` is NOT touched by Plan 05.
Plan 06 retires it + adds the structural guard test that asserts no
long-lived `duckdb_connection` allocations remain in `src/lib.rs::init_extension`.

---

## Self-check

- [x] **PLAN Tasks 5 + 6 acceptance criteria** — all pass per
      verification block above.
- [x] **`just test-sql`** — 53/53 PASS after each commit; zero
      regressions in any high-sensitivity surface for `semantic_view` or
      `explain_semantic_view`.
- [x] **`uv run test/integration/test_adbc_transactions.py`** — 6/6
      PASS (D-21 transactional invariant green throughout).
- [x] **`uv run test/integration/test_multi_db_isolation.py`** — 3/3
      PASS (preliminary EXPAND-CTX finding documented).
- [x] **`cargo test`** — all bundled-feature tests pass including
      `type_cache::tests`.
- [x] **2 commits cleanly stacked** (`690dd67`, `1616649`); `git log
      --oneline 690dd67^..HEAD` matches the table above.
- [x] **Bridge mechanism uniform** across both migrations — same
      `reinterpret_cast<duckdb_connection>(Connection*)` pattern as the
      14 Batch-1 migrations.
- [x] **Borrow contract uniform** — no `duckdb_disconnect` calls on
      borrowed handles anywhere in the migrated paths.
- [x] **`grep -nE 'register_table_function_with_extra_info|
      register_scalar_function_with_state' src/lib.rs`** — only
      comment-line matches remain; no live calls.
- [x] **H2 query_conn at `src/lib.rs:577` is INTACT** — Batch 2 did
      NOT touch it; Batch 3 owns the deletion.
- [x] **H1 catalog_conn at `src/lib.rs:421-446` is INTACT** — Plan 06
      owns the deletion + structural guard test.
- [x] **No SUMMARY.md written** (reserved for Batch 3 / full Plan 05
      completion).
- [x] **C-API ↔ C++ enum-value mismatch caught and resolved** via
      `sv_logical_type_from_c_type_id` — highest-impact discovery in
      Batch 2; documented in commit `1616649`.

---

## Self-Check: PASSED

All claimed commits exist (`git log --oneline 690dd67^..HEAD` matches
the table). All claimed files modified per `git diff --shortstat 690dd67^..HEAD`
(5 files, 1398 insertions, 19 deletions). All claimed greps verified
against the current src/lib.rs state.
