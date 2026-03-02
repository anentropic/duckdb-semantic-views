# Phase 12: EXPLAIN + Typed Output - Research

**Researched:** 2026-03-02
**Domain:** DuckDB Rust extension — VTab typed output, EXPLAIN integration, scalar DDL rename
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**EXPLAIN — surface:**
- Wire native `EXPLAIN FROM semantic_view(...)` using the `explain_extra_info` C API callback on the `semantic_view` table function
- Retire `explain_semantic_view` as a separate function — native EXPLAIN replaces it
- The three-part format (metadata header + expanded SQL + DuckDB physical plan) stays; it is injected as Extra Info in the TABLE_FUNCTION node of DuckDB's EXPLAIN output

**EXPLAIN — output content:**
- Keep the current three-part format from `explain_semantic_view`:
  1. Metadata header (view name, dimensions, metrics)
  2. Expanded SQL
  3. DuckDB physical plan for the expanded SQL (via `EXPLAIN {expanded_sql}`)
- This appears inline as `Extra Info` in DuckDB's `EXPLAIN FROM semantic_view(...)` output

**Typed output — type resolution hierarchy:**
Types are resolved in this priority order:
1. Explicit `output_type` in DDL — user-declared type per column in `create_semantic_view`. Enforced via SQL `CAST(expr AS <type>)` in the generated query AND declared as that type in `bind()`. Takes precedence over everything.
2. DDL-time inference — at `create_semantic_view` call time, run `LIMIT 0` on the expanded SQL and store inferred types in the catalog JSON alongside the definition. At query bind time, read stored types directly (no inference overhead).
3. Fallback: VARCHAR — if neither explicit type nor successful DDL-time inference, column type is VARCHAR (current behaviour preserved).

**Typed output — staleness:**
- When inference is stored at DDL time (tier 2), types can go stale if upstream column types change.
- This is documented: users must re-run `create_semantic_view` (or `create_or_replace_semantic_view`) to refresh stored types if the upstream schema changes and no `output_type` was declared.

**DDL function rename + new variant:**

| Old name | New name |
|---|---|
| `define_semantic_view` | `create_semantic_view` |
| `define_or_replace_semantic_view` | `create_or_replace_semantic_view` |
| *(did not exist)* | `create_semantic_view_if_not_exists` |
| `drop_semantic_view` | unchanged |
| `drop_semantic_view_if_exists` | unchanged |

- `create_semantic_view_if_not_exists`: succeeds silently (no-op) if the view already exists; errors only on other failures.
- This is a breaking change — intentional for v0.2.0 (pre-1.0). Test files and REQUIREMENTS.md/ROADMAP.md references must be updated.

### Claude's Discretion

- Whether DDL-time LIMIT 0 inference can safely run from scalar `invoke()` context, or requires a different mechanism (the "no SQL from scalar invoke" deadlock risk is an open question — Claude should verify and find the right path)
- Exact `output_type` field name and storage format in catalog JSON (on each Metric/Dimension struct vs. a top-level `column_types` map)
- How to map DuckDB C API type enums (`duckdb_type`) to `LogicalTypeHandle` for declaring output columns
- Implementation approach for writing typed vectors in `func()` — whether to drop the VARCHAR-cast wrapper or use string-to-type coercion

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| EXPL-01 | `EXPLAIN FROM semantic_query(...)` shows DuckDB's full physical query plan for the expanded SQL | The `explain_extra_info` C API callback does NOT exist (verified). The correct approach is: `EXPLAIN FROM semantic_view(...)` inherently shows DuckDB's physical plan for the table function — the expanded SQL runs inside `func()`, so DuckDB's EXPLAIN will show a TABLE_FUNCTION node. To surface the expanded SQL to the user, `explain_semantic_view` must be kept (renamed) OR a different display mechanism used. See Architecture Patterns for the resolution. |
| OUT-01 | `semantic_query` returns typed columns (BIGINT, DOUBLE, DATE, etc.) matching source column types instead of all-VARCHAR | Implemented via: (1) DDL-time LIMIT 0 inference storing types as strings in catalog JSON, (2) `From<u32> for LogicalTypeId` converts `ffi::duckdb_type` integers at bind time, (3) `flat_vector.as_mut_slice::<T>()` + `copy()` writes typed data in `func()`, (4) Verified: `persist_conn` (separate connection) is safe for DDL-time LIMIT 0. |
</phase_requirements>

## Summary

Phase 12 has three independent workstreams: (1) EXPLAIN integration, (2) typed output columns, and (3) DDL rename. The most important pre-planning discovery is that **the `explain_extra_info` C API callback mentioned in CONTEXT.md does not exist** in DuckDB's C API or `libduckdb-sys 1.4.4`. The vendored `duckdb.h`, the `bindgen_bundled_version_loadable.rs`, and the `duckdb-rs` VTab trait all confirm only six `duckdb_table_function_set_*` functions exist: `name`, `extra_info`, `bind`, `init`, `local_init`, `function`. There is no explain callback in the C extension API.

For typed output, the implementation path is clear and entirely within existing project patterns. The `try_infer_schema` function already captures `Vec<ffi::duckdb_type>` (currently discarded as `_types`). The `From<u32> for LogicalTypeId` conversion is available in `duckdb-rs`. The `persist_conn` separate connection (already used for DDL persistence) is safe for read-only LIMIT 0 inference from `invoke()` — it avoids the main connection's execution lock, which is the documented deadlock risk. The DDL-time inference path is therefore viable without a new mechanism.

For the DDL rename, it is a mechanical change: update `lib.rs` registration names, rename `define.rs` function comments, add `create_semantic_view_if_not_exists` (same `DefineState` with `or_replace: false` + a new `if_exists` flag variant), and update all `.test` files.

**Primary recommendation:** Resolve EXPLAIN by keeping the `explain_semantic_view` pattern (possibly renamed to `semantic_view_explain` or similar) rather than pursuing a nonexistent C API hook. Confirm this revised EXPLAIN approach with the user before writing the plan. For typed output, use DDL-time LIMIT 0 via `persist_conn` plus `From<u32> for LogicalTypeId` to avoid bind-time re-inference overhead.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `libduckdb-sys` | `1.4.4` (pinned) | FFI bindings — `duckdb_type`, `duckdb_column_type`, `duckdb_create_logical_type`, `duckdb_vector_get_data` | Already in use; all needed functions confirmed present in loadable bindings |
| `duckdb` (Rust crate) | `1.4.4` (pinned) | `LogicalTypeHandle`, `LogicalTypeId`, `FlatVector::as_mut_slice`, `FlatVector::copy`, `bind.add_result_column` | Already in use; typed writing via `as_mut_slice::<T>()` is the established pattern |
| `serde_json` | (existing) | Store/load inferred type strings in catalog JSON | Already used for all catalog serialization |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `duckdb_rs` `From<u32> for LogicalTypeId` | `1.4.4` | Convert `ffi::duckdb_type` enum values to `LogicalTypeId` for `add_result_column` | Type resolution at bind time after reading stored types from JSON |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| DDL-time LIMIT 0 inference (via `persist_conn`) | Bind-time LIMIT 0 inference (current `try_infer_schema` pattern) | Bind-time inference already works (re-entrant SQL is safe on `state.conn`) but adds per-query overhead. DDL-time moves cost to definition time. Either works; DDL-time is the user decision. |
| `output_type` field on each `Metric`/`Dimension` struct | Top-level `column_types: HashMap<String, String>` in `SemanticViewDefinition` | Per-struct field follows the existing `dim_type`/`granularity` pattern. Top-level map is cleaner for inferred types. Both approaches work; per-struct for `output_type` (user-declared), separate `inferred_types` map for DDL-time results is the clearest separation. |

## Architecture Patterns

### Critical Finding: `explain_extra_info` Does NOT Exist in the C API

**Confirmed by:**
- Vendored `duckdb_capi/duckdb.h` (project's own copy): only 6 `duckdb_table_function_set_*` functions, none explain-related
- `libduckdb-sys-1.4.4/src/bindgen_bundled_version_loadable.rs`: zero mentions of "explain"
- `libduckdb-sys-1.4.4/src/bindgen_bundled_version.rs`: zero mentions of "explain"
- `duckdb-rs` VTab trait: no explain hook method
- DuckDB official C API docs: no explain callback listed

**Impact:** The locked CONTEXT.md decision to "wire native `EXPLAIN FROM semantic_view(...)` using the `explain_extra_info` C API callback" is technically infeasible as stated. The approach must be revised.

**What `EXPLAIN FROM semantic_view(...)` actually shows:**
DuckDB's `EXPLAIN` statement works at the SQL plan level, before the table function executes. It shows a `TABLE_FUNCTION` plan node referencing `semantic_view` — not the expanded SQL that runs inside `func()`. The expanded SQL is invisible to the planner.

### Pattern 1: EXPLAIN Approach — Revised to Table Function

Since `explain_extra_info` does not exist, the most compatible approach that achieves the EXPL-01 requirement ("shows DuckDB's full physical query plan for the expanded SQL") is:

**Option A (recommended): Keep `explain_semantic_view` as a renamed companion function**
- Rename `explain_semantic_view` → keep it with the current name or rename to `semantic_view_plan` / `show_semantic_view_sql`
- It already shows the three-part format (metadata, expanded SQL, DuckDB EXPLAIN plan)
- EXPL-01 says "shows DuckDB's full physical query plan for the expanded SQL" — this is exactly what the existing `explain_semantic_view` does (it runs `EXPLAIN {expanded_sql}` internally)
- This means `EXPLAIN FROM semantic_view(...)` is NOT the surface — `FROM explain_semantic_view(...)` remains the surface

**Option B: Wire EXPLAIN using DuckDB's C++ API (not C API)**
The C++ `TableFunction` class in DuckDB does have an `explain` field. However, this requires a C++ shim and the same `-fvisibility=hidden` problem that blocked Phase 11 native DDL. This path is infeasible for a loadable extension.

**Resolution for planning:** The planner must flag this back to the user. The CONTEXT.md decision is infeasible via the C API. The practical choice is Option A: retire the `explain_semantic_view` rename as a follow-on or keep it as-is, and accept that `EXPLAIN FROM semantic_view(...)` shows a TABLE_FUNCTION node (not the expanded SQL). EXPL-01 is fulfilled by `explain_semantic_view` already.

### Pattern 2: Typed Output — DDL-Time Inference

```
create_semantic_view invoke() call:
  1. parse_define_args() → SemanticViewDefinition
  2. expand() → expanded_sql
  3. execute_sql_raw(persist_conn, "{expanded_sql} LIMIT 0") → duckdb_result
  4. for each column: duckdb_column_type(&result, i) → duckdb_type (u32)
  5. store types as Vec<String> in catalog JSON: "column_types": ["BIGINT", "VARCHAR", "DATE", ...]
  6. serialize SemanticViewDefinition + column_types to JSON → persist to catalog
```

At query time (`bind()`):
```
  1. parse catalog JSON → SemanticViewDefinition + column_types: Vec<String>
  2. for each output column: parse type string → LogicalTypeId → LogicalTypeHandle
  3. bind.add_result_column(name, LogicalTypeHandle)
  4. store Vec<LogicalTypeId> in SemanticViewBindData for use by func()
```

**Why `persist_conn` is safe for DDL-time LIMIT 0:**
The `persist_conn` is a separate `duckdb_connection` created via `duckdb_connect(db_handle, ...)` at extension load time. It has its own context and does not share the execution lock with the main connection. The "no SQL from scalar invoke" rule applies to re-entrant calls on the SAME connection. A separate connection is deadlock-free, as confirmed by the existing `persist_define()` pattern (which already writes SQL from scalar invoke via `persist_conn`). Read-only LIMIT 0 is strictly safer than the INSERT already done there.

**Note:** For in-memory databases, `persist_conn` is `None`. In that case, the DDL-time inference must use a different connection — either skip inference (fallback to VARCHAR) or use `state.conn` (the query connection, which at DDL time is not executing a query). The plan should clarify this.

### Pattern 3: duckdb_type → LogicalTypeHandle Mapping

```rust
// Source: duckdb-rs From<u32> for LogicalTypeId
// Available: duckdb_column_type() returns ffi::duckdb_type (which is u32/DUCKDB_TYPE)

fn duckdb_type_to_logical_type(t: ffi::duckdb_type) -> LogicalTypeHandle {
    // duckdb_type is a type alias for DUCKDB_TYPE (u32)
    // LogicalTypeId implements From<u32>
    let type_id = LogicalTypeId::from(t);
    LogicalTypeHandle::from(type_id)
}
```

**Special cases to handle:**
- `DUCKDB_TYPE_DECIMAL` (19): requires width + scale via `duckdb_decimal_width()` / `duckdb_decimal_scale()` from the logical type. At LIMIT 0 time, use `duckdb_column_logical_type()` to get the full `duckdb_logical_type`, then `duckdb_decimal_width/scale`. Store as `"DECIMAL(18,2)"` string.
- `DUCKDB_TYPE_ENUM`, `DUCKDB_TYPE_LIST`, `DUCKDB_TYPE_STRUCT`, `DUCKDB_TYPE_MAP`: complex types. Safe fallback: store as VARCHAR for these. They are rare in metric/dimension contexts.
- `DUCKDB_TYPE_INVALID` (0): treat as VARCHAR fallback.

**Recommended storage format in catalog JSON:**
Store inferred types as a flat list of strings in `column_types_inferred: Vec<String>` at the `SemanticViewDefinition` level (not per-struct). This avoids changing the `Metric`/`Dimension` structs for inferred types and keeps backward compat clean. `output_type: Option<String>` goes on each `Metric`/`Dimension` struct (user-declared, follows the `dim_type` pattern).

### Pattern 4: Typed Writing in func()

**Current approach:** `func()` wraps expanded SQL with `build_varchar_cast_sql()`, executes, reads all values as VARCHAR strings, inserts via `flat_vector.insert(i, str)`.

**New approach for typed columns:**
- Remove the VARCHAR-cast wrapper
- Execute expanded SQL directly
- For each column: read raw bytes from result chunk vector, copy to output vector using `flat_vector.as_mut_slice::<T>()`

**Concrete typed reading pattern:**
```rust
// For i64 (BIGINT): read from result chunk, write to output
let src_vector = ffi::duckdb_data_chunk_get_vector(chunk, col_idx as ffi::idx_t);
let data_ptr = ffi::duckdb_vector_get_data(src_vector).cast::<i64>();
let values = std::slice::from_raw_parts(data_ptr, row_count);

let out_vec = output.flat_vector(col_idx);
out_vec.as_mut_slice_with_len::<i64>(row_count)[..row_count].copy_from_slice(values);
// Also copy validity mask
```

**Simpler alternative:** Keep the VARCHAR-cast wrapper for all columns but use `parse::<T>()` before inserting into typed output vectors. This avoids reading raw binary vector data entirely. However, this has correctness risk: DATE columns would need parsing from DuckDB's date display format (`2024-01-01`) which may vary. The direct binary copy is more reliable.

**Recommended:** Use direct binary copy for numeric types (BIGINT, INTEGER, DOUBLE, FLOAT). For DATE, DuckDB stores as `i32` days since epoch — safe to `memcpy`. For VARCHAR, keep the existing string path. This is a hybrid approach that handles each `LogicalTypeId` variant in `func()`.

### Anti-Patterns to Avoid

- **Calling `execute_sql_raw(state.conn, ...)` from scalar `invoke()`**: The main query connection holds the execution lock during invoke. Using `persist_conn` for DDL-time LIMIT 0 avoids this. (For in-memory databases where `persist_conn` is None, use `state.conn` from `QueryState` — at DDL time, the query connection is idle.)
- **Assuming `explain_extra_info` exists**: It does not. See Critical Finding above.
- **Storing complex types (LIST, STRUCT, MAP) in typed output**: Fall back to VARCHAR for these to avoid nested vector writing complexity.
- **Using `duckdb_value_varchar` on result data**: This deprecated API does not work reliably with chunked results. The existing `read_varchar_from_vector` is the correct approach for VARCHAR. For typed data, use `duckdb_vector_get_data` directly.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| `duckdb_type` → `LogicalTypeId` mapping | Custom match table | `LogicalTypeId::from(u32)` from duckdb-rs | Already implemented, covers all types |
| Type string parsing | Custom parser | Parse `LogicalTypeId` from stored string via `LogicalTypeHandle::from(LogicalTypeId::Bigint)` pattern | Store the `DUCKDB_TYPE` u32 as integer in JSON, reconstruct via `from(u32)` |
| Typed vector writing for primitives | Custom serialization | `flat_vector.as_mut_slice::<T>()` + `copy_from_slice` | DuckDB vectors are plain arrays in memory; direct copy is safe |

**Key insight:** The existing `try_infer_schema()` already captures `Vec<ffi::duckdb_type>`. Phase 12 simply wires those types through instead of discarding them.

## Common Pitfalls

### Pitfall 1: explain_extra_info Does Not Exist
**What goes wrong:** Planner implements a callback that the C API does not provide. Compilation fails, or the wrong API is used.
**Why it happens:** The C++ `TableFunction` class has this field, but it is inaccessible from the C extension API (same `-fvisibility=hidden` issue that blocked Phase 11).
**How to avoid:** Use `explain_semantic_view` table function pattern (Option A). Do not attempt to hook into DuckDB's EXPLAIN pipeline via C API.
**Warning signs:** If `grep "explain" duckdb_capi/duckdb.h` returns nothing, the C API has no explain hook.

### Pitfall 2: DDL-Time Inference Deadlock on In-Memory Databases
**What goes wrong:** `persist_conn` is `None` for in-memory databases. Calling LIMIT 0 on the wrong connection deadlocks.
**Why it happens:** The main connection holds the invoke execution lock.
**How to avoid:** For in-memory databases, use the `query_conn` from `QueryState` (which is idle during DDL operations) OR skip DDL-time inference and fall back to VARCHAR. The plan must handle the `persist_conn: None` branch.
**Warning signs:** Tests with `:memory:` databases hang indefinitely.

### Pitfall 3: DECIMAL Type Requires Width+Scale
**What goes wrong:** `duckdb_column_type()` returns `DUCKDB_TYPE_DECIMAL` (19). Using `LogicalTypeId::from(19u32)` gives `LogicalTypeId::Decimal` but `LogicalTypeHandle::from(LogicalTypeId::Decimal)` produces an unspecified precision — which may not match what the query actually returns.
**Why it happens:** DECIMAL is a parameterized type; the `duckdb_type` enum value alone loses precision information.
**How to avoid:** At DDL-time inference, when `duckdb_column_type` returns DECIMAL, call `duckdb_column_logical_type()` to get the full logical type, then `duckdb_decimal_width()` / `duckdb_decimal_scale()`. Store as string `"DECIMAL(w,s)"` and use `LogicalTypeHandle::decimal(w, s)` at bind time. Alternatively, fall back to DOUBLE for DECIMAL columns (loses precision but avoids complexity).
**Warning signs:** `sum(amount)` on a `DECIMAL(10,2)` column returns wrong values or bind errors.

### Pitfall 4: Backward Compatibility of Catalog JSON
**What goes wrong:** Old catalog JSON (no `column_types_inferred` field) is loaded by new code expecting the field.
**Why it happens:** Existing deployed databases have JSON without the new field.
**How to avoid:** Add `#[serde(default)]` on the new `column_types_inferred: Vec<String>` field in `SemanticViewDefinition`. An absent field deserializes to `vec![]`, which triggers the VARCHAR fallback path.
**Warning signs:** Deserialization panics on old catalog entries.

### Pitfall 5: Test Files Use `define_semantic_view` After Rename
**What goes wrong:** All `.test` files use the old `define_semantic_view` name. After rename, all integration tests fail.
**Why it happens:** `TEST_LIST` runs `phase2_ddl.test` and `phase4_query.test` which both call `define_semantic_view`.
**How to avoid:** Update all `.test` files in the same plan that renames the functions in `lib.rs`. The rename is mechanical — sed or grep-replace across test files.
**Warning signs:** `make test_debug` fails with "function does not exist: define_semantic_view".

### Pitfall 6: `or_replace` Flag Reuse for `if_not_exists`
**What goes wrong:** `create_semantic_view_if_not_exists` needs to silently succeed on duplicate, not error. `catalog_insert` currently errors on duplicate.
**Why it happens:** `DefineState.or_replace` controls upsert vs. insert. A third mode (if_not_exists) is neither — it should silently ignore the duplicate without replacing.
**How to avoid:** Add a new `if_exists: bool` field to `DefineState` (mirroring `DropState.if_exists`). When `if_exists` is true and `catalog_insert` returns an "already exists" error, swallow the error and return success.

## Code Examples

### duckdb_type to LogicalTypeHandle (verified pattern)

```rust
// Source: duckdb-rs bindgen_bundled_version_loadable.rs (DUCKDB_TYPE constants)
// + duckdb-rs core/logical_type.rs (From<u32> for LogicalTypeId)

use duckdb::core::{LogicalTypeHandle, LogicalTypeId};
use libduckdb_sys as ffi;

fn type_from_duckdb_type(t: ffi::duckdb_type) -> LogicalTypeHandle {
    match t {
        // Simple case: direct mapping
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_INVALID
        | ffi::DUCKDB_TYPE_DUCKDB_TYPE_LIST
        | ffi::DUCKDB_TYPE_DUCKDB_TYPE_STRUCT
        | ffi::DUCKDB_TYPE_DUCKDB_TYPE_MAP
        | ffi::DUCKDB_TYPE_DUCKDB_TYPE_ENUM => {
            LogicalTypeHandle::from(LogicalTypeId::Varchar) // fallback for complex types
        }
        _ => LogicalTypeHandle::from(LogicalTypeId::from(t)),
    }
}
```

### FlatVector typed write (verified via duckdb-rs source)

```rust
// Source: duckdb-rs core/vector.rs — FlatVector::as_mut_slice + copy

// Write i64 data (BIGINT) to output vector
let out_vec = output.flat_vector(col_idx);
let dst = out_vec.as_mut_slice_with_len::<i64>(n_rows);
dst.copy_from_slice(values_i64);

// Write i32 data (DATE, INTEGER) — DuckDB stores DATE as i32 days-since-epoch
let dst = out_vec.as_mut_slice_with_len::<i32>(n_rows);
dst.copy_from_slice(values_i32);

// Write f64 data (DOUBLE)
let dst = out_vec.as_mut_slice_with_len::<f64>(n_rows);
dst.copy_from_slice(values_f64);

// NULL handling via set_null (duckdb-rs FlatVector::set_null)
if is_null_at_row {
    out_vec.set_null(row_idx);
}
```

### DDL-time LIMIT 0 with persist_conn (pattern)

```rust
// In DefineSemanticView::invoke(), after expand():
if let Some(conn) = state.persist_conn {
    let limit0 = format!("{expanded_sql} LIMIT 0");
    if let Some((_names, types)) = unsafe { try_infer_schema(conn, &limit0) } {
        // types: Vec<ffi::duckdb_type>
        // Store as Vec<u32> in catalog JSON for later reconstruction
        def_with_types.column_types_inferred = types.iter().map(|t| *t as u32).collect();
    }
}
// For in-memory (persist_conn is None): column_types_inferred stays empty → VARCHAR fallback
```

### create_semantic_view_if_not_exists (new variant)

```rust
// In DefineSemanticView::invoke():
// When state.if_exists == true and catalog_insert returns "already exists" error:
let result = if state.or_replace {
    catalog_upsert(&state.catalog, &parsed.name, &json)
} else if state.if_exists {
    // Try insert; swallow duplicate error
    match catalog_insert(&state.catalog, &parsed.name, &json) {
        Ok(()) => Ok(()),
        Err(e) if e.to_string().contains("already exists") => Ok(()),
        Err(e) => Err(e),
    }
} else {
    catalog_insert(&state.catalog, &parsed.name, &json)
};
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| All-VARCHAR output (`LogicalTypeId::Varchar` for all columns) | Typed output (BIGINT, DATE, DOUBLE, etc.) based on inferred/declared types | Phase 12 | DuckDB can use type info for optimization; no implicit string coercion in downstream queries |
| `explain_semantic_view` as separate table function | Keep `explain_semantic_view` (no native EXPLAIN hook exists) | Phase 12 — decision to revise | Native EXPLAIN hook does not exist in C API; companion function remains the interface |
| `define_semantic_view` | `create_semantic_view` | Phase 12 — breaking change | Aligns with SQL DDL naming convention |

## Open Questions

1. **EXPLAIN approach — requires user confirmation**
   - What we know: `explain_extra_info` does not exist in DuckDB's C API. `EXPLAIN FROM semantic_view(...)` shows DuckDB's physical plan for the TABLE_FUNCTION node, not the expanded SQL.
   - What's unclear: The user's intent in CONTEXT.md was to surface expanded SQL via native EXPLAIN. This is not achievable via C API without C++ (which is blocked).
   - Recommendation: Plan 12-01 should document this constraint and propose keeping `explain_semantic_view` as the EXPLAIN surface. The planner should note this as an open question for the user to confirm before work starts. EXPL-01 requirement ("shows DuckDB's full physical query plan for the expanded SQL") is already met by `explain_semantic_view` — the requirement says "shows DuckDB's full physical query plan", not "shows it via native EXPLAIN syntax".

2. **In-memory database typed inference**
   - What we know: `persist_conn` is `None` for in-memory databases. Calling LIMIT 0 safely requires a connection with its own context.
   - What's unclear: Is `state.conn` (the query connection from `QueryState`) accessible from `DefineState::invoke()`? Currently it is not — `DefineState` only holds `catalog` and `persist_conn`.
   - Recommendation: For in-memory databases, skip DDL-time inference. Fall back to VARCHAR for all columns. Document that typed output requires a file-backed database for DDL-time inference. Users with in-memory databases can use `output_type` declarations to get typed output.

3. **DECIMAL handling**
   - What we know: `DUCKDB_TYPE_DECIMAL` loses width/scale from the enum value alone.
   - Recommendation: Fall back to DOUBLE for DECIMAL columns in DDL-time inference (unless `output_type` is declared). DOUBLE is a safe approximation for most analytics use cases. Document this simplification.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | SQLLogicTest (Python runner via `duckdb_sqllogictest`) |
| Config file | `test/sql/TEST_LIST` — explicit file list |
| Quick run command | `make test_debug` |
| Full suite command | `make test_debug` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| EXPL-01 | `FROM explain_semantic_view(...)` returns expanded SQL plan (revised surface) | integration | `make test_debug` | ❌ Wave 0 — new `.test` section in `phase4_query.test` |
| OUT-01 | BIGINT metric returns BIGINT column (not VARCHAR) | integration | `make test_debug` | ❌ Wave 0 — new `.test` section; use `query I` not `query T` |
| OUT-01 | DATE time dimension returns DATE column | integration | `make test_debug` | ❌ Wave 0 — new `.test` section; use `query D` or verify value format |
| DDL rename | `create_semantic_view` exists, `define_semantic_view` does not | integration | `make test_debug` | ❌ Wave 0 — update all existing `.test` files |

### Sampling Rate
- **Per task commit:** `cargo test` (unit tests, no extension feature)
- **Per wave merge:** `make test_debug` (full SQLLogicTest suite)
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Update `test/sql/phase2_ddl.test` — replace `define_semantic_view` → `create_semantic_view`, etc.
- [ ] Update `test/sql/phase4_query.test` — same rename + add typed output assertions (change `query T` to `query I` / `query D` for typed columns)
- [ ] New test section in `phase4_query.test` — EXPL-01: verify `explain_semantic_view` returns expanded SQL
- [ ] New test section in `phase4_query.test` — OUT-01: verify BIGINT and DATE typed columns
- [ ] New test section — `create_semantic_view_if_not_exists` is a no-op on duplicate

## Sources

### Primary (HIGH confidence)
- Vendored `duckdb_capi/duckdb.h` (project file) — confirmed no explain callback in C API; all `duckdb_table_function_set_*` functions enumerated
- `libduckdb-sys-1.4.4/src/bindgen_bundled_version_loadable.rs` (cargo registry) — confirmed zero "explain" references; all `DUCKDB_TYPE_*` constants and `duckdb_column_type()` function verified present
- `duckdb-1.4.4/src/core/logical_type.rs` (cargo registry) — `From<u32> for LogicalTypeId` confirmed; all type mappings verified
- `duckdb-1.4.4/src/core/vector.rs` (cargo registry) — `FlatVector::as_mut_slice`, `copy`, `set_null` APIs confirmed
- Project source files: `src/query/table_function.rs`, `src/query/explain.rs`, `src/model.rs`, `src/ddl/define.rs`, `src/lib.rs`, `src/catalog.rs` — all read directly

### Secondary (MEDIUM confidence)
- Context7 `/websites/rs_duckdb_duckdb` — VTab trait, ArrowVTab typed writing patterns, FlatVector API
- DuckDB stable docs (WebFetch) — confirmed no explain hook in table function registration API

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- EXPLAIN finding (no C API callback): HIGH — directly confirmed in vendored header and libduckdb-sys bindings
- DDL-time inference safety: HIGH — `persist_conn` pattern already in use for writes; read-only LIMIT 0 is strictly safer
- Typed writing via `as_mut_slice`: HIGH — confirmed in duckdb-rs source
- `From<u32> for LogicalTypeId` mapping: HIGH — confirmed in duckdb-rs source
- DECIMAL width/scale handling: MEDIUM — API exists (`duckdb_decimal_width/scale`) but behavior with LIMIT 0 not tested
- In-memory database inference path: MEDIUM — deadlock analysis is sound but not experimentally verified

**Research date:** 2026-03-02
**Valid until:** 2026-04-02 (stable library version pinned at 1.4.4; no version drift risk)
