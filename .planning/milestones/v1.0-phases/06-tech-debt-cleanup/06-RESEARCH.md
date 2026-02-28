# Phase 6: Tech Debt Code Cleanup - Research

**Researched:** 2026-02-26
**Domain:** Rust code cleanup, feature gating, test reliability
**Confidence:** HIGH

## Summary

Phase 6 is a focused tech debt closure phase that addresses four specific issues identified by the v1.0 milestone audit. None of these issues affect runtime correctness -- all 28 v0.1 requirements are already satisfied. The work falls into three categories: (1) dead code removal in `table_function.rs`, (2) feature-gate consistency for `pub mod query` in `lib.rs`, and (3) test reliability fixes for both the SQLLogicTest restart section and the Rust unit tests that use hardcoded `/tmp/` paths.

All four issues have been directly investigated in the codebase. The dead code items are confirmed unused. The feature-gate inconsistency is confirmed by comparing `pub mod query` (ungated) with `pub mod ddl` (gated with `#[cfg(feature = "extension")]`). The test failures are confirmed reproducible -- the 3 catalog sidecar tests fail with `PermissionDenied` when the sandbox prevents writes to `/tmp/`, and the SQLLogicTest restart section leaves behind `restart_test.db`, `restart_test.db.wal`, and `restart_test.db.semantic_views` files that cause failures on re-run.

**Primary recommendation:** This is a single-plan phase. All four changes are small, independent edits to existing files with no new dependencies or architectural decisions.

## Standard Stack

### Core

No new libraries required. All changes use existing project dependencies.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| std::env::temp_dir | stdlib | Portable temp directory resolution | Reads `TMPDIR` on macOS/Linux, sandbox-aware |

### Supporting

None.

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `std::env::temp_dir()` | `tempfile` crate | `tempfile` provides auto-cleanup and unique naming; overkill for 3 test functions that already do manual cleanup |

**Installation:** No new dependencies needed.

## Architecture Patterns

### Recommended Project Structure

No structural changes. All edits are in existing files:

```
src/
  lib.rs                    # Feature-gate fix for pub mod query
  query/
    table_function.rs       # Dead code removal (field + function)
  catalog.rs                # Test path fix (std::env::temp_dir)
test/sql/
  phase2_ddl.test           # Sidecar cleanup at section 10 start
```

### Pattern 1: Feature-Gate Consistency

**What:** All modules that depend on `duckdb/loadable-extension` or `duckdb/vscalar` are gated with `#[cfg(feature = "extension")]` at the `lib.rs` level.

**When to use:** When a module's submodules are ALL internally gated with `#[cfg(feature = "extension")]`.

**Current state (inconsistent):**
```rust
// lib.rs
pub mod query;                     // NOT gated -- exposes empty module under default features

#[cfg(feature = "extension")]
pub mod ddl;                       // Correctly gated
```

**Target state (consistent):**
```rust
// lib.rs
#[cfg(feature = "extension")]
pub mod query;                     // Now gated, consistent with ddl

#[cfg(feature = "extension")]
pub mod ddl;
```

**Key detail:** The `query/mod.rs` already gates all three submodules (`error`, `explain`, `table_function`) with `#[cfg(feature = "extension")]`. Gating `pub mod query` at the `lib.rs` level is therefore safe -- the module is already empty under default features.

**Verification:** `cargo test` (default features) must still compile and pass. `cargo build --no-default-features --features extension` must still build the cdylib.

### Pattern 2: Portable Temp Paths in Tests

**What:** Use `std::env::temp_dir()` instead of hardcoded `/tmp/` paths in tests that create file-backed DuckDB databases.

**When to use:** Any test that writes files to a temporary directory.

**Example:**
```rust
// Before (hardcoded, fails in sandbox):
let tmpfile = "/tmp/test_pragma_rust_check.duckdb";

// After (portable, works in sandbox):
let tmpfile = std::env::temp_dir().join("test_pragma_rust_check.duckdb");
let tmpfile = tmpfile.to_str().expect("temp dir path is valid UTF-8");
```

**Why this works:** `std::env::temp_dir()` reads the `TMPDIR` environment variable on macOS/Linux. In the Claude Code sandbox, `TMPDIR=/tmp/claude` which is writable. On CI (Linux), `TMPDIR` typically points to `/tmp` which is writable. On vanilla macOS, it points to `/var/folders/.../T/` which is writable.

### Pattern 3: SQLLogicTest Idempotent Cleanup

**What:** Add sidecar file deletion SQL/statements at the start of section 10 to handle leftover state from previous runs.

**Limitation:** SQLLogicTest cannot execute filesystem operations directly. The cleanup must be done via SQL or by restructuring the test.

**Approach:** After `load __TEST_DIR__/restart_test.db`, add a `statement ok` block that drops any existing view before defining the new one. This handles the case where the sidecar file from a previous run causes `init_catalog` to populate the view already.

```sql
# Idempotent cleanup: if a previous run left state behind, remove it
statement ok
SELECT CASE WHEN EXISTS (SELECT 1 FROM list_semantic_views() WHERE name = 'restart_test')
       THEN drop_semantic_view('restart_test')
       ELSE 'no cleanup needed'
END;
```

**Alternative approach:** Use `try` or `statement maybe` if the test framework supports it. However, the DuckDB SQLLogicTest runner may not support conditional execution. A safer approach is:

```sql
# If leftover state exists from a previous run, drop it silently
statement ok
SELECT COALESCE(
    (SELECT drop_semantic_view('restart_test') WHERE EXISTS (SELECT 1 FROM list_semantic_views() WHERE name = 'restart_test')),
    'clean'
);
```

**Simplest reliable approach:** Since `drop_semantic_view` errors on nonexistent views, and we cannot use `IF EXISTS` in the scalar function, the test should handle this by:
1. Attempting to drop `restart_test` and tolerating the error
2. Using `statement error` or `statement ok` as appropriate

Actually, the most reliable pattern is simply:
```sql
statement ok
SELECT CASE WHEN (SELECT count(*) FROM list_semantic_views() WHERE name = 'restart_test') > 0
       THEN drop_semantic_view('restart_test')
       ELSE 'no-op'
END;
```

**Implementation note:** The exact SQL syntax needs testing against the DuckDB SQLLogicTest runner. The planner should verify this works. An even simpler fallback: just make the `define_semantic_view` call use a unique name incorporating a timestamp or accept that cleanup must happen outside the test runner.

### Anti-Patterns to Avoid

- **Keeping dead code "for future use":** The `logical_type_from_duckdb_type` function and `column_type_ids` field were retained "for potential future use if typed output columns are re-enabled." This is speculative. If typed output columns are needed in v0.2, the function can be rewritten then. Remove it now to eliminate the `#[allow(dead_code)]` suppressions.
- **Feature-gating only internal submodules:** If ALL submodules of a module are feature-gated, the parent module should be gated too. An ungated empty module is confusing and inconsistent.
- **Hardcoded `/tmp/` in tests:** Always use `std::env::temp_dir()` for portability across platforms and sandboxed environments.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Temporary file paths | Hardcoded `/tmp/` | `std::env::temp_dir()` | Portable across macOS, Linux, Windows, sandboxed environments |
| Conditional SQL execution | Complex CASE/WHEN in scalar context | Straightforward drop-before-define pattern | Simpler, more reliable |

**Key insight:** This phase requires no new solutions -- only removal of dead code, alignment of existing patterns, and path portability fixes.

## Common Pitfalls

### Pitfall 1: Breaking `cargo test` by Gating `pub mod query`

**What goes wrong:** If `pub mod query` is gated with `#[cfg(feature = "extension")]` but any test module imports from `crate::query::*`, compilation fails under default features.

**Why it happens:** Integration tests or unit tests might reference types from the query module.

**How to avoid:** Search the codebase for `use crate::query` and `use super::query` outside of `#[cfg(feature = "extension")]` blocks. Verify with `cargo test` after the change.

**Warning signs:** `cargo test` fails with "unresolved import" errors.

**Current status (verified):** No test code under default features imports from `crate::query`. The `query/mod.rs` already gates all contents behind `#[cfg(feature = "extension")]`. Gating the parent module is safe.

### Pitfall 2: Removing `column_type_ids` Without Updating `infer_schema_or_default`

**What goes wrong:** `infer_schema_or_default` returns a tuple `(Vec<String>, Vec<ffi::duckdb_type>)`. Removing `column_type_ids` from the struct requires updating the call site to discard the second return value.

**Why it happens:** The function was designed when typed output columns were planned. The second return value is now unused.

**How to avoid:** Either (a) change `infer_schema_or_default` to return only `Vec<String>`, or (b) destructure with `_` at the call site. Option (a) is cleaner since the entire type-inference path for types is dead code. However, the `try_infer_schema` function also returns types -- those are needed for the names. The least disruptive change: keep `infer_schema_or_default` returning both, but let `_` discard the types at the bind call site.

**Recommendation:** Option (a) -- refactor `infer_schema_or_default` and `try_infer_schema` to return only names. This removes the dead `duckdb_type` computation entirely. The `logical_type_from_duckdb_type` function was the only consumer of those types.

### Pitfall 3: SQLLogicTest CASE Expression Type Mismatch

**What goes wrong:** `CASE WHEN ... THEN drop_semantic_view('restart_test') ELSE 'no-op' END` may fail because `drop_semantic_view` returns a VARCHAR confirmation message while the ELSE branch also needs to be VARCHAR. DuckDB type inference should handle this, but the scalar function's error semantics (raising an error rather than returning NULL) could interact unexpectedly with CASE short-circuit evaluation.

**Why it happens:** DuckDB may evaluate both branches before short-circuiting, or the CASE expression type inference may differ from expectations.

**How to avoid:** Test the cleanup SQL manually before committing. If CASE doesn't work, use a simpler two-statement approach: `statement ok` followed by `statement error` (tolerate both outcomes).

**Warning signs:** Test still fails or errors on second run even after adding cleanup.

### Pitfall 4: `std::env::temp_dir()` Path Not UTF-8

**What goes wrong:** `std::env::temp_dir()` returns a `PathBuf`. Converting to `&str` for DuckDB's `Connection::open(path)` fails if the path is not valid UTF-8.

**Why it happens:** Rare on modern systems, but possible on some Linux configurations.

**How to avoid:** Use `.to_str().expect("temp dir path is valid UTF-8")` -- this is acceptable in tests. Alternatively, use `to_string_lossy()`.

## Code Examples

### Dead Code Removal in table_function.rs

**Before:**
```rust
pub struct SemanticViewBindData {
    expanded_sql: String,
    column_names: Vec<String>,
    #[allow(dead_code)]
    column_type_ids: Vec<ffi::duckdb_type>,
}
```

**After:**
```rust
pub struct SemanticViewBindData {
    expanded_sql: String,
    column_names: Vec<String>,
}
```

**Also remove** the `logical_type_from_duckdb_type` function (lines 103-135) entirely.

**Update bind call site** (line 258-274):
```rust
// Before:
let (column_names, column_type_ids) =
    infer_schema_or_default(state.conn, &expanded_sql, &dimensions, &metrics, &def);
// ...
Ok(SemanticViewBindData {
    expanded_sql,
    column_names,
    column_type_ids,
})

// After:
let column_names =
    infer_schema_or_default(state.conn, &expanded_sql, &dimensions, &metrics, &def);
// ...
Ok(SemanticViewBindData {
    expanded_sql,
    column_names,
})
```

### Feature-Gate Fix in lib.rs

**Before (line 4):**
```rust
pub mod query;
```

**After:**
```rust
#[cfg(feature = "extension")]
pub mod query;
```

### Portable Temp Path in catalog.rs Tests

**Before:**
```rust
#[test]
fn sidecar_round_trip() {
    let db_path = "/tmp/test_sidecar_roundtrip.duckdb";
    let sidecar = sidecar_path(db_path);
    let _ = std::fs::remove_file(&sidecar);
    // ...
}
```

**After:**
```rust
#[test]
fn sidecar_round_trip() {
    let tmp = std::env::temp_dir();
    let db_file = tmp.join("test_sidecar_roundtrip.duckdb");
    let db_path = db_file.to_str().expect("temp dir is UTF-8");
    let sidecar = sidecar_path(db_path);
    let _ = std::fs::remove_file(&sidecar);
    // ...
}
```

Apply the same pattern to `pragma_database_list_returns_file_path` and `init_catalog_loads_from_sidecar`.

## State of the Art

Not applicable -- this phase involves standard Rust patterns, not evolving ecosystem choices.

## Open Questions

1. **SQLLogicTest cleanup SQL syntax**
   - What we know: We need to drop an existing view before re-defining it in section 10
   - What's unclear: Whether `CASE WHEN ... THEN drop_semantic_view(...) END` works correctly in DuckDB's SQLLogicTest runner, or whether the scalar function call is evaluated eagerly regardless of the CASE condition
   - Recommendation: Test the CASE approach first. If it fails, try a simpler approach: add `statement ok` with `SELECT drop_semantic_view('restart_test')` followed by `onlyif` or simply tolerating the error with a two-stage cleanup (try drop, ignore error, then define). The planner should verify during implementation.

2. **Whether `infer_schema_or_default` should be fully refactored or minimally changed**
   - What we know: The second return value (`Vec<ffi::duckdb_type>`) is dead code. The `try_infer_schema` function also computes types internally to extract column names.
   - What's unclear: Whether the type computation in `try_infer_schema` has any side effects or whether removing it changes behavior
   - Recommendation: Minimal change -- let `_` discard the second value at the call site. The type computation is cheap and side-effect-free. Full refactoring of the inference functions is out of scope for tech debt cleanup.

## Sources

### Primary (HIGH confidence)

- **Direct codebase inspection** - all findings verified by reading source files:
  - `src/query/table_function.rs` - dead code at lines 53-54, 103-135
  - `src/lib.rs` - ungated `pub mod query` at line 4
  - `src/query/mod.rs` - internal gating confirmed at lines 1-6
  - `src/catalog.rs` - hardcoded `/tmp/` in tests at lines 250, 306, 330
  - `test/sql/phase2_ddl.test` - restart section at lines 144-193
  - `Cargo.toml` - feature definitions at lines 27-29
  - `.planning/v1.0-MILESTONE-AUDIT.md` - tech debt inventory

- **Live test execution** - sidecar test failures confirmed with `cargo test sidecar` and `cargo test pragma_database_list_returns_file_path`:
  - `sidecar_round_trip`: `PermissionDenied` writing to `/tmp/` in sandbox
  - `init_catalog_loads_from_sidecar`: `PermissionDenied` writing to `/tmp/` in sandbox
  - `pragma_database_list_returns_file_path`: `PermissionDenied` opening file-backed DB at `/tmp/` in sandbox

## Metadata

**Confidence breakdown:**
- Dead code removal: HIGH - confirmed by exhaustive search for all usages of `column_type_ids` and `logical_type_from_duckdb_type`
- Feature-gate fix: HIGH - confirmed by reading lib.rs and query/mod.rs; verified no default-feature imports from query module
- Test path fix: HIGH - confirmed by reproducing failures in current sandbox
- SQLLogicTest cleanup: MEDIUM - the approach is clear but exact SQL syntax needs validation during implementation

**Research date:** 2026-02-26
**Valid until:** 2026-03-26 (stable codebase, no external dependencies involved)
