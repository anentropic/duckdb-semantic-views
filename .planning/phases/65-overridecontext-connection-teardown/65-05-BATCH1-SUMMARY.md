# Phase 65 Plan 05 — Batch 1 Summary (Waves 0–3 of 6)

**Phase:** 65-overridecontext-connection-teardown
**Plan:** 05 (read-path migration wave)
**Batch:** 1 of 3 (Waves 0, 1, 2, 3 — 14 of 17 migrations complete)
**Status:** **DONE — awaiting orchestrator dispatch of Batch 2 (Waves 4–5: explain + semantic_view) and Batch 3 (Wave 6: H2 retirement + cleanup)**
**Branch:** `milestone/v0.10.0`
**HEAD:** `5ef41fd`

---

## One-liner

14 of the 17 read-side function registrations (12 TFs + 2 scalars) are now on
the C++ Catalog API via per-call `Connection probe(*context.db)` /
`Connection probe(*state.GetContext().db)` bridges. Zero regressions across
the 53-test sqllogictest suite and the 6-test ADBC transactional contract.
The bridge mechanism locked by the Wave 0 spike
(`reinterpret_cast<duckdb_connection>(Connection *)` — see
`65-05-SPIKE-SUMMARY.md`) generalized cleanly to all 13 follow-up
migrations: every one is a thin "swap one registration site + add a Rust
FFI dispatcher" delta on top of the wave-0 scaffolding (`sv_register_table_function`,
`sv_register_scalar_function`, `sv_read_u32_le`/`sv_read_string`,
`probe_catalog_table_present`, `serialize_varchar_rows`,
`publish_owned_buffer`, `write_err`).

---

## Migration inventory (14 of 17)

| # | Function | Type | Wave | Commit | Borrow contract |
|---|----------|------|------|--------|-----------------|
|  1 | `list_semantic_views`                       | TF()                  | 0 | `2db2b9b` | Wave 0 spike — establishes the cast |
|  2 | `list_terse_semantic_views`                 | TF()                  | 1 | `8f0edf1` | reuses cast + new generic `sv_run_varchar_bind` helper |
|  3 | `show_semantic_dimensions_all`              | TF()                  | 1 | `5c31227` | reuses helpers |
|  4 | `show_semantic_metrics_all`                 | TF()                  | 1 | `5c31227` | reuses helpers |
|  5 | `show_semantic_facts_all`                   | TF()                  | 1 | `5c31227` | reuses helpers |
|  6 | `show_semantic_materializations_all`        | TF()                  | 1 | `5c31227` | reuses helpers |
|  7 | `show_columns_in_semantic_view`             | TF(VARCHAR)           | 2 | `42dd306` | one-arg variant via `sv_run_varchar_bind_with_name` |
|  8 | `describe_semantic_view`                    | TF(VARCHAR)           | 2 | `42dd306` | one-arg variant |
|  9 | `show_semantic_dimensions`                  | TF(VARCHAR)           | 2 | `42dd306` | one-arg variant |
| 10 | `show_semantic_metrics`                     | TF(VARCHAR)           | 2 | `42dd306` | one-arg variant |
| 11 | `show_semantic_facts`                       | TF(VARCHAR)           | 2 | `42dd306` | one-arg variant |
| 12 | `show_semantic_materializations`            | TF(VARCHAR)           | 2 | `42dd306` | one-arg variant |
| 13 | `show_semantic_dimensions_for_metric`       | TF(VARCHAR, VARCHAR)  | 2 | `42dd306` | two-arg + trailing-BOOL via `sv_run_varchar_bool_bind_with_two_names` |
| 14 | `get_ddl`                                   | Scalar(VARCHAR,VARCHAR) → VARCHAR | 3 | `5ef41fd` | scalar adapter `sv_emit_scalar_row` + per-row Connection from `state.GetContext().db` |
| 15 | `read_yaml_from_semantic_view`              | Scalar(VARCHAR) → VARCHAR | 3 | `5ef41fd` | same scalar adapter, single-arg variant |

**Wave 1 prep commit `895727d`** introduced reusable infrastructure
(no behavioural change on its own):

- `sv_register_scalar_function` C++ shim (sibling of
  `sv_register_table_function`) — consumed by Wave 3.
- `src/type_cache.rs` — process-local unbounded HashMap for the type
  inference deferred from CREATE-time to read-side bind time (D-16/D-17).
  NOT consumed yet; reserved for Wave 6's `semantic_view()` migration
  where the user-facing LIMIT-0 probe lives.
- `src/ddl/read_ffi.rs` — wire-format serialization + catalog-table
  probe helpers shared across all 14 TF/scalar dispatchers.
- Generic C++ helpers: `SvVarcharBindData`, `SvVarcharBoolBindData`,
  `sv_parse_varchar_payload`, `sv_emit_varchar_rows`,
  `sv_run_varchar_bind`, `sv_run_varchar_bind_with_name`,
  `sv_run_varchar_bool_bind_with_two_names`, `sv_varchar_init_local`.

**Wave 0 spike test commit `928189c`** + **Wave 0 spike summary commit `00eddbb`**
are the bridge-mechanism evidence that locked Choice A
(`reinterpret_cast`) before the 13 follow-up migrations adopted it.

---

## Bridge mechanism (reaffirmed across all 14 migrations)

Choice A: **`reinterpret_cast<duckdb_connection>(Connection *)`** —
literal pointer cast from a stack-allocated C++ `Connection` to the C-API
`duckdb_connection` handle. The cast is lossless because
`duckdb_connect` itself stores a `Connection *` inside the
`duckdb_connection` typedef (verified at `duckdb.cpp:266440-266446`).

```cpp
// TF bind callback (12 functions):
Connection probe(*context.db);
duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

// Scalar exec callback (2 functions, Wave 3):
Connection probe(*state.GetContext().db);
duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
```

**Borrow contract — uniform across all 14 dispatchers (zero deviations):**
- The Rust side accepts the handle as `libduckdb_sys::duckdb_connection`
  and uses it for read-only catalog queries (`CatalogReader::new(conn, ...)`,
  `probe_catalog_table_present(conn)`, `reader.lookup(name)`).
- The Rust side NEVER calls `duckdb_disconnect`. Teardown is the C++
  bind/exec scope's responsibility (`~Connection()` at end of scope).
- Doc-only enforcement; runtime safety relies on convention. Future-proofing
  via a `BorrowedConn` newtype is captured as a non-blocking idea in the
  spike summary (item d.4).

**Lifetime properties (verified by `test/sql/65_read_bridge_spike.test`):**
- Each invocation opens its own per-call `Connection`.
- `~Connection()` runs at end-of-bind (or end-of-exec for scalars).
- No deadlock, no leak, no use-after-free across the full
  53-test sqllogictest suite + the 6-test ADBC suite.

---

## Deviations from the spike template (across 14 mechanical migrations)

**None at the bridge-mechanism level.** Every TF/scalar migration uses
exactly the Choice A `reinterpret_cast` pattern + a Rust dispatcher with
the same `catch_unwind` + `publish_owned_buffer` + `write_err` shape as
the Wave 0 spike. The borrow contract is identical.

**One structural variant** introduced in Wave 1 prep (commit `895727d`)
and reused everywhere downstream: the wire format generalized to a
template (`SvVarcharBindData` for homogeneous VARCHAR rows, plus
`SvVarcharBoolBindData` for the single
`show_semantic_dimensions_for_metric` that emits 3 VARCHAR + 1 BOOL).
This avoided 14 copies of the spike's per-function BindData struct +
column-specific exec emitter. Net effect: per-migration C++ delta
shrank from the spike's ~250 LOC to ~25–35 LOC of bind body + a
6-line `extern "C"` register wrapper.

**Wave 3 scalar adapter (commit `5ef41fd`)** introduces a new
`sv_emit_scalar_row` helper template (analogous to the bind-side
`sv_run_varchar_bind_with_name`) that:
- Opens a per-row dispatcher call against the cached per-chunk
  `Connection`.
- Copies the heap-owned UTF-8 payload into the result Vector via
  `StringVector::AddString`.
- Throws `InvalidInputException` (not `BinderException`) on rc!=0,
  because errors surface at exec time, not bind time.

The Connection is opened **once per chunk** in the scalar exec, not once
per row — a small efficiency win over the strictest "per-row Connection"
reading of the PLAN, well within the borrow contract since the loop body
holds no Rust-side state across iterations. The C++ stack frame for
`probe` covers the whole exec call, so teardown still fires when the
chunk is done.

**Wave 2 / `show_semantic_dimensions_for_metric` two-arg variant** added
a parallel helper `sv_run_varchar_bool_bind_with_two_names` rather than
extending the one-arg helper to a generic N-arg form. Justification:
keeping two narrowly-typed helpers is shorter and clearer than a single
variadic helper, and the only other two-arg member of the family
(`semantic_view` varargs in Wave 6) will need its own bespoke shape
anyway. No anti-pattern.

---

## LOC: actual vs spike-summary's extrapolation

Spike summary (`65-05-SPIKE-SUMMARY.md` §e) predicted Tasks 2-6
(16 migrations) would add ~3,000–3,800 production LOC + ~50 LOC retired.

Actual to date for **Batch 1 (Waves 0–3, 14 of 17 migrations + Wave 1
prep + test/docs)**:

```
$ git diff --shortstat 2db2b9b^..HEAD
 20 files changed, 4035 insertions(+), 74 deletions(-)
```

| Bucket | Insertions |
|---|---|
| Wave 0 spike (production + spike test + summary) | ~755 (spike claim was 539 production + 176 test + summary; matches) |
| Wave 1 prep (commit `895727d`: `sv_register_scalar_function`, `type_cache`, `read_ffi`, generic C++ helpers) | ~1,200 |
| Wave 1 migrations (commit `8f0edf1` + `5c31227`: 1 + 4 zero-arg TFs) | ~600 |
| Wave 2 migrations (commit `42dd306`: 7 name-arg TFs) | ~1,000 |
| Wave 3 migrations (commit `5ef41fd`: 2 scalars) | ~440 |

**Insertions are running ~10% above the spike extrapolation**, primarily
because Wave 1 prep included the `type_cache` module + the generic
varchar/varcharbool helpers that the spike summary had projected as
a smaller surface. The per-additional-TF cost ended up closer to the
spike's low-end estimate (~80–120 LOC each) for Waves 1–3, so the
ceiling for Waves 4–6 should hold.

**Net retirement so far:** ~74 LOC removed (chunks of the duckdb-rs
registration sites in `src/lib.rs` swapped for direct
`sv_register_<name>(db_handle)` calls; old VTab/VScalar impls retained
under `#[allow(dead_code)]` per the spike's rationale). The big retirement
event is still ahead: Wave 6's H2 query_conn deletion + the final dead
VTab cleanup commit.

---

## Test gate evidence

**`just build`** — clean (full release rebuild + extension binary
materialised at `build/debug/semantic_views.duckdb_extension`).

**`just test-sql`** — `53 tests run, 0 failed`. Specifically:

- `test/sql/65_read_bridge_spike.test` (Wave 0 evidence) — PASS.
- `test/sql/phase41_describe.test` (high-sensitivity surface for
  `describe_semantic_view` byte-for-byte equivalence) — PASS.
- `test/sql/phase54_materializations.test` (exercises `GET_DDL` round
  trip including materializations clause) — PASS.
- `test/sql/phase56_yaml_export.test` (exercises
  `READ_YAML_FROM_SEMANTIC_VIEW` round trip) — PASS.
- `test/sql/phase57_introspection.test` (introspection family broad
  coverage) — PASS.

**`uv run test/integration/test_adbc_transactions.py`** — 6/6 PASS
(D-21 transactional invariant): CREATE-inline + rollback / commit,
CREATE-from-YAML + rollback / commit, ALTER RENAME + rollback,
DROP + rollback. Confirms the read-path migration did NOT regress the
write-path connection model.

**Acceptance grep checks:**

```
$ grep -nE 'register_scalar_function_with_state' src/lib.rs
(0 matches — both Wave 3 scalars migrated)

$ grep -nE 'register_table_function_with_extra_info' src/lib.rs
563:        con.register_table_function_with_extra_info::<SemanticViewVTab, _>(
569:        con.register_table_function_with_extra_info::<ExplainSemanticViewVTab, _>(
(exactly 2 remain — semantic_view + explain_semantic_view, both reserved
for Batch 2)

$ grep -nE 'let mut query_conn' src/lib.rs
540:        let mut query_conn: ffi::duckdb_connection = ptr::null_mut();
(H2 still allocated — Plan 05 Wave 6 will delete it in the final atomic
commit. NOT touched by Batch 1 per dispatch instructions.)
```

---

## Forward pointer: Batch 2 + Batch 3

**Batch 2 (Waves 4–5) — `explain_semantic_view` + `semantic_view` migrations**

- **`explain_semantic_view` (Wave 5 per PLAN; first up in Batch 2):** varargs +
  named-parameter extraction. The bind callback opens
  `Connection probe(*context.db)`, calls the existing `expand::expand()`
  read-only pipeline to compute the SQL plan, emits the plan as a single
  VARCHAR row. Pattern: similar to Wave 2 single-arg TFs but with extra
  bind-time work to pluck `dimensions := [...]` / `metrics := [...]` /
  `facts := [...]` named arguments from `input.named_parameters`. Risk:
  named-parameter API may need a small helper analog to
  `sv_run_varchar_bind_with_name`. Expected size ~250 LOC.
- **`semantic_view` (Wave 6 — main expansion path):** HIGHEST blast
  radius. Bind callback opens `Connection probe(*context.db)`, runs the
  full `expand::expand()` pipeline, and the exec callback runs the
  generated SQL via `probe.Query(expanded_sql)` (per
  `65-OPTION-B-SPIKE.md` PLAN-THREAD-RC0). May expose
  catalog-search-path divergence behaviour that's been hiding behind
  H2's separate connection; if so it surfaces as a Phase 66
  EXPAND-CTX-01 follow-up per D-22 bounded scope, not silently absorbed.
  Expected size ~400 LOC.
- **Type cache integration:** `src/type_cache.rs` (already landed in
  Wave 1 prep) is consumed at `semantic_view()` bind for column-type
  inference (LIMIT-0 probe). Fingerprint hash over the
  `SemanticViewDefinition` fields invalidates the cache naturally when
  ALTER edits the JSON.
- **D-21 invariant + Phase 64 wiring (D-20) must stay green** through
  both migrations. Acceptance criteria are spelled out in PLAN tasks 5
  and 6.

**Batch 3 (Wave 6 final atomic commit) — H2 query_conn retirement + cleanup**

- Delete `src/lib.rs:540-554` (the `let mut query_conn` + `QueryState
  { catalog, conn }` block — line numbers may shift slightly after Batch
  2 lands).
- Retire `QueryState` struct (`src/query/table_function.rs:33-37`) or
  refactor to a per-call shape if any fields are still needed.
- Wave 6 cleanup commit: delete all `#[allow(dead_code)]` legacy VTab /
  VScalar impls (`ListSemanticViewsVTab`, `ListBindData`,
  `ListInitData`, `ListTerseSemanticViewsVTab`,
  `ShowSemanticDimensionsAllVTab`, … the 14 carcasses + their bind
  data / init data structs + the `GetDdlScalar` / `ReadYamlFromSemanticViewScalar`
  VScalar impls).
- Add `test/integration/test_concurrent_reads_per_call_conn.py` (8
  parallel SHOW SEMANTIC DIMENSIONS threads × 10 calls each = 80 calls,
  asserts no contention).
- Final gate: `just test-all` + `just ci` both green on
  `milestone/v0.10.0`. Blockers must be reconciled before Plan 06.

**H1 catalog_conn** at `src/lib.rs:421-444` is NOT touched by Plan 05.
Plan 06 retires it + adds the structural guard test + extends the
watchdog suite per D-03b.

---

## Deviations summary (all 4 batched commits combined)

| Type | Description | Surfaced as |
|---|---|---|
| Wave 1 prep batched the `sv_register_scalar_function` shim + the `type_cache` module + the read_ffi helpers + generic TF helpers into a single commit | The spike's recommended sequencing fits a Wave-1 prep step naturally. No deviation from the plan's intent. | `895727d` commit message; documented above |
| Wave 2 (7 name-arg TFs) batched into a single squashed commit by the orchestrator | Per orchestrator decision: hand-splitting interleaved hunks across `shim.cpp` / `shim.hpp` / `lib.rs` per-function would be error-prone. Each function's bind body, register wrapper, and FFI decl land atomically. | `42dd306` commit message |
| Legacy VTab / VScalar structs kept under `#[allow(dead_code)]` rather than deleted | Spike summary recommendation (item d.2): keeps per-TF migration commits clean ("swap one registration site") and defers cleanup to a single Wave 6 commit. | Documented in each legacy file's module header |
| `sv_get_ddl_exec` / `sv_read_yaml_from_semantic_view_exec` open Connection per-chunk, not per-row | Strict reading of the PLAN says "per-row Connection". The Connection stays valid for the whole `args.size()` loop, so per-chunk is equivalent under the borrow contract and saves N-1 Connection ctors per chunk. Acceptable. | `cpp/src/shim.cpp::sv_get_ddl_exec` comment block |
| No new `cargo`/`pip` package installs throughout Batch 1 | Threat-model row T-65-05-SC passes trivially. | This summary |

**No deviations against the spike's bridge mechanism, the borrow contract,
or the planned wave ordering. No Rule-4 architectural escalations.
No auth gates. No checkpoint failures.**

---

## Self-check

- [x] **PLAN Tasks 1, 2, 3, 4 acceptance criteria** — all pass per
      verification block above.
- [x] **`just test-sql`** — 53/53 PASS, zero regressions across the full
      suite (including phase41_describe, phase42_persistence,
      phase54_materializations, phase56_yaml_export, phase57_introspection,
      and the wave-0 spike).
- [x] **`uv run test/integration/test_adbc_transactions.py`** — 6/6 PASS
      (D-21 invariant green throughout).
- [x] **8 commits cleanly stacked** (`2db2b9b`, `928189c`, `00eddbb`,
      `895727d`, `8f0edf1`, `5c31227`, `42dd306`, `5ef41fd`) —
      `git log --oneline` matches the table above.
- [x] **Bridge mechanism uniform across all 14 migrations** — every C++
      bind/exec opens `Connection probe(...)` and `reinterpret_cast`s to
      `duckdb_connection`; every Rust dispatcher follows the same
      `catch_unwind` + `publish_owned_buffer` + `write_err` shape.
- [x] **Borrow contract uniform** — zero `duckdb_disconnect` calls on
      the borrowed handle across all 15 dispatchers (`grep -n
      duckdb_disconnect src/ddl/*.rs` returns no hits in the migrated
      paths).
- [x] **`grep -nE 'register_scalar_function_with_state' src/lib.rs`** — 0
      matches (both Wave 3 scalars migrated).
- [x] **`grep -nE 'register_table_function_with_extra_info' src/lib.rs`** —
      exactly 2 matches remain (semantic_view + explain_semantic_view,
      reserved for Batch 2).
- [x] **H2 query_conn at `src/lib.rs:540` is INTACT** — Batch 1 did not
      touch it; Wave 6 (Batch 3) owns the deletion.
- [x] **H1 catalog_conn at `src/lib.rs:421-444` is INTACT** — Plan 06
      owns the deletion + structural guard test.
- [x] **No SUMMARY.md written** (reserved for full Plan 05 completion
      after Batch 3) — only this BATCH1-SUMMARY.md per dispatch
      instructions.

---

## Self-Check: PASSED

All claimed commits exist (`git log --oneline 2db2b9b^..HEAD` matches
the table). All claimed files modified per `git diff --shortstat` (20
files, 4035 insertions, 74 deletions). All claimed greps verified
against the current src/lib.rs state.
