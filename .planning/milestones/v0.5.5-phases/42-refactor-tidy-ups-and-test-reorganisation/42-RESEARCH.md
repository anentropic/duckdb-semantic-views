# Phase 42: Refactor, Tidy-ups, and Test Reorganisation - Research

**Researched:** 2026-04-04
**Domain:** Rust refactoring, DuckDB C API parameterized queries, test infrastructure
**Confidence:** HIGH

## Summary

Phase 42 addresses eight actionable items identified in a comprehensive code review (2026-04-04). All items are behavior-preserving refactors, correctness hardening, or test coverage improvements -- no new features. The scope is well-defined: four code fixes (catalog TOCTOU, parameterized persistence queries, parallel Vec antipattern, body_parser invariant comments), and four test improvements (sql_gen fixture extraction, file-backed catalog round-trip, transmute layout guard, error suggestion property test).

The codebase is mature at 482+ tests across unit, proptest, sqllogictest, and integration layers. Every change in this phase can be validated against the existing `just test-all` quality gate. The refactoring items are independent of each other (no ordering dependency), though grouping related changes (e.g., all persistence parameterization in one plan) is natural.

Three items were explicitly deferred: large-schema stress tests, concurrent access tests, and build.rs splitting. These are documented in `.planning/todos/pending/2026-04-04-test-hardening-stress-and-concurrency.md`.

**Primary recommendation:** Group the eight items into 3-4 plans by affinity: (1) correctness fixes (TOCTOU + parameterized queries), (2) model/code tidying (parallel Vec + body_parser comments), (3) test infrastructure (fixture extraction + new tests).

## Project Constraints (from CLAUDE.md)

- Quality gate: `just test-all` must pass (Rust unit tests + proptest + sqllogictest + DuckLake CI)
- `cargo test` alone is insufficient -- sqllogictest covers integration paths
- `just test-sql` requires a fresh `just build` to pick up code changes
- If in doubt about SQL syntax or behaviour, refer to Snowflake semantic views

## Standard Stack

This phase is purely internal refactoring within the existing project. No new dependencies are introduced.

### Core (existing, unchanged)
| Library | Version | Purpose | Notes |
|---------|---------|---------|-------|
| `libduckdb-sys` | pinned | DuckDB C API FFI bindings | Provides `duckdb_prepare`, `duckdb_bind_varchar`, `duckdb_execute_prepared`, `duckdb_destroy_prepare` |
| `duckdb` | =1.4.4 | Rust DuckDB crate | Layout dependency for `value_raw_ptr` transmute |
| `serde` / `serde_json` | existing | Model serialization | Used for `SemanticViewDefinition` |
| `strsim` | existing | Levenshtein distance | Used in `suggest_closest` |
| `proptest` | existing | Property-based testing | Test dependency |

### No New Dependencies Required

All eight items use existing crate functionality. The parameterized query work uses `ffi::duckdb_prepare`, `ffi::duckdb_bind_varchar`, `ffi::duckdb_execute_prepared`, and `ffi::duckdb_destroy_prepare` -- all already available in the `libduckdb-sys` loadable extension function pointer table.

## Architecture Patterns

### Item-by-Item Technical Analysis

#### 1. Catalog TOCTOU Fix (`catalog.rs`)

**Current code** (lines 95-119):
```rust
// Read lock to check existence
{
    let guard = state.read().unwrap();
    if guard.contains_key(name) { return Err(...) }
}
// Write lock to insert (separate lock acquisition)
state.write().unwrap().insert(name.to_string(), json.to_string());
```

**Target pattern** (already used in `catalog_rename`, line 174):
```rust
let mut guard = state.write().unwrap();
if guard.contains_key(name) { return Err(...) }
guard.insert(name.to_string(), json.to_string());
```

**Scope:** `catalog_insert` (lines 95-119) and `catalog_delete` (lines 124-137) both have this pattern. `catalog_upsert` and `catalog_delete_if_exists` are already correct (single lock).

**Risk:** LOW. The fix is 3 lines changed per function. `catalog_rename` already demonstrates the correct pattern.

#### 2. Parameterized Persistence Queries (`ddl/define.rs`, `ddl/drop.rs`, `ddl/alter.rs`)

**Current pattern:** String interpolation with manual quote escaping:
```rust
let safe_name = name.replace('\'', "''");
let sql = format!("INSERT OR REPLACE INTO ... VALUES ('{safe_name}', ...)");
ffi::duckdb_query(conn, c_sql.as_ptr(), &mut result);
```

**Target pattern:** Prepared statements via C API:
```rust
// 1. Prepare
let mut stmt: ffi::duckdb_prepared_statement = std::ptr::null_mut();
let state = ffi::duckdb_prepare(conn, c_sql.as_ptr(), &mut stmt);
// 2. Bind
ffi::duckdb_bind_varchar(stmt, 1, c_name.as_ptr());
ffi::duckdb_bind_varchar(stmt, 2, c_json.as_ptr());
// 3. Execute
let mut result: ffi::duckdb_result = std::mem::zeroed();
let state = ffi::duckdb_execute_prepared(stmt, &mut result);
// 4. Cleanup
ffi::duckdb_destroy_result(&mut result);
ffi::duckdb_destroy_prepare(&mut stmt);
```

**Affected functions (4 call sites):**
- `persist_define` in `ddl/define.rs:48-70` -- INSERT OR REPLACE with 2 params (name, json)
- `persist_drop` in `ddl/drop.rs:31-44` -- DELETE with 1 param (name)
- `persist_rename` in `ddl/alter.rs:30-56` -- DELETE with 1 param + INSERT with 2 params

**Implementation approach:** Extract a shared helper function (e.g., `execute_parameterized`) in a common location (perhaps a new `ddl/persist.rs` or in `catalog.rs`) to avoid duplicating the prepare/bind/execute/cleanup boilerplate. The SQL templates become:
- `INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) VALUES ($1, $2)`
- `DELETE FROM semantic_layer._definitions WHERE name = $1`
- `INSERT INTO semantic_layer._definitions (name, definition) VALUES ($1, $2)`

**DuckDB parameter syntax:** DuckDB uses `$1`, `$2` etc. for positional parameters in prepared statements. The `duckdb_bind_varchar` index is 1-based.

**Risk:** MEDIUM. This changes the FFI interaction pattern but the existing string-escaping code is correct -- this is defense-in-depth, not a bug fix. The prepare/bind/execute pattern is well-established in DuckDB's C API.

#### 3. Parallel Vec Antipattern (`model.rs`)

**Current** (lines 181-192):
```rust
pub column_type_names: Vec<String>,
pub column_types_inferred: Vec<u32>,
```

**Target:** A dedicated struct that makes the invariant explicit:
```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InferredColumnType {
    pub name: String,
    pub duckdb_type: u32,
}
pub inferred_column_types: Vec<InferredColumnType>,
```

**Serde compatibility:** The new field name (`inferred_column_types`) differs from the old JSON keys (`column_type_names`, `column_types_inferred`). Two approaches:
1. **Custom deserializer** that reads old format and maps to new struct
2. **Keep old field names** with `#[serde(rename)]` on the struct fields -- but this doesn't work for parallel-to-struct conversion

**Recommended approach:** Keep the old field names in the serde representation using a custom `Deserialize` impl or `#[serde(from)]` pattern for backward compatibility. Add `#[serde(default)]` so old JSON without either field still works. The new struct should serialize to a different key (`inferred_column_types`) and the deserializer should handle both old and new formats.

**Alternatively (simpler):** Since this is a cosmetic/readability improvement, consider deferring the serde migration and just introducing a helper method like `fn inferred_types(&self) -> impl Iterator<Item = (&str, u32)>` that zips the two vecs. This avoids JSON format changes entirely.

**Scope of ripple:** The grep shows ~60 locations across 8 files that reference `column_type_names` or `column_types_inferred`. Most are test fixtures setting them to `vec![]`. If the struct approach is taken, all these need updating.

**Risk:** LOW for the helper method approach, MEDIUM for the full struct migration (many file touches, serde compat).

#### 4. Test Fixture Extraction (`expand/sql_gen.rs`)

**Current state:** 3,039 lines total, with ~1,800 lines of inline tests. Each test builds a full `SemanticViewDefinition` struct literal. The existing `orders_view()` helper (line 228) is used by some tests but not consistently.

**Pattern to follow:** `graph/test_helpers.rs` already demonstrates the right approach with `make_def()`, `make_def_with_facts()`, etc.

**Target:** Create `expand/test_helpers.rs` (currently a 5-line placeholder) with:
- `orders_view()` -- single-table, 2 dims, 2 metrics (already exists inline, extract)
- `orders_with_customers()` -- two-table join, most common multi-table fixture
- Builder-pattern mutation methods: `.with_join(...)`, `.with_fact(...)`, `.with_using(...)`

**Impact:** Each test fixture currently occupies ~30 lines of struct literal. Replacing with `orders_view()` reduces to 1 line. Estimated reduction: ~1,200 lines.

**Risk:** LOW. Pure test refactoring. Every test still exercises the same logic paths.

#### 5. File-Backed Catalog Round-Trip Test

**Current coverage gap:** All catalog tests use `:memory:`. The persistence path (`persist_define`, `persist_drop`, `persist_rename`) is only exercised via sqllogictest (which uses LOAD).

**Target:** A sqllogictest (`.test` file) that:
1. Creates a file-backed DuckDB database
2. LOADs the extension
3. Defines a semantic view
4. Disconnects/reconnects (or uses a separate statement block)
5. Verifies the view persists via SHOW/DESCRIBE

**Note:** sqllogictest has `restart` support for testing persistence. The existing `test/sql/phase2_restart.test` file already demonstrates this pattern.

**Risk:** LOW. New test only, no code changes.

#### 6. Transmute Layout Guard Test (`query/table_function.rs`)

**Current code** (lines 130-148):
```rust
pub(crate) unsafe fn value_raw_ptr(value: &Value) -> ffi::duckdb_value {
    let ptr_to_value = std::ptr::from_ref(value).cast::<ffi::duckdb_value>();
    std::ptr::read(ptr_to_value)
}
```

**Target test:** Verify that `Value` and `duckdb_value` have the same size and alignment:
```rust
#[test]
fn value_layout_matches_duckdb_value() {
    assert_eq!(
        std::mem::size_of::<duckdb::vtab::Value>(),
        std::mem::size_of::<ffi::duckdb_value>(),
        "Value size changed -- value_raw_ptr transmute is broken"
    );
    assert_eq!(
        std::mem::align_of::<duckdb::vtab::Value>(),
        std::mem::align_of::<ffi::duckdb_value>(),
        "Value alignment changed -- value_raw_ptr transmute is broken"
    );
}
```

**Placement:** This test needs `duckdb::vtab::Value` which is only available with the `extension` feature (or actually the `bundled` feature should also expose it through the `duckdb` crate). The test should go in a location that compiles under `cargo test` (the default `bundled` feature).

**Checking availability:** `duckdb::vtab::Value` is public in the `duckdb` crate. `ffi::duckdb_value` is `*mut c_void`. The test verifies `size_of::<Value>() == size_of::<*mut c_void>()` (pointer width).

**Risk:** LOW. New test only. If it fails, it catches a real correctness issue before it manifests as UB.

#### 7. Body Parser Invariant Comments (`body_parser.rs`)

**Three locations:**
- Line 448: `after_pk.find(')').unwrap()` -- safe because `extract_paren_content` already found matching parens
- Line 500: `after_unique_kw.find(')').unwrap()` -- same invariant
- Line 796: `after_to.find(')').unwrap()` -- same invariant

**Target:** Add comments explaining why each unwrap is safe:
```rust
// SAFETY: extract_paren_content succeeded above, confirming balanced parens.
// The closing ')' must exist at or after the position returned.
let close = after_pk.find(')').unwrap();
```

**Risk:** NONE. Comments only, no code changes.

#### 8. Error Suggestion Property Test

**Current state:** `suggest_closest` is spot-checked but not property-tested. The function is called from 11 call sites across 6 files.

**Target property test:**
```rust
proptest! {
    #[test]
    fn suggestion_is_always_valid_name(
        query in "[a-z_]{1,20}",
        names in prop::collection::vec("[a-z_]{1,20}", 1..20)
    ) {
        if let Some(suggestion) = suggest_closest(&query, &names) {
            // Suggestion must be one of the available names
            assert!(names.contains(&suggestion));
        }
    }

    #[test]
    fn exact_match_always_suggests(
        name in "[a-z_]{1,20}",
        others in prop::collection::vec("[a-z_]{1,20}", 0..10)
    ) {
        let mut names = others;
        names.push(name.clone());
        let suggestion = suggest_closest(&name, &names);
        // Exact match has distance 0, always within threshold
        assert!(suggestion.is_some());
        assert_eq!(suggestion.unwrap(), name);
    }
}
```

**Placement:** In `src/util.rs` tests module or as a new proptest in `tests/`.

**Risk:** LOW. New test only.

### Anti-Patterns to Avoid
- **Changing JSON serialization format without backward compat:** The parallel Vec fix must preserve deserialization of old stored JSON. Prefer a helper method over a struct migration to avoid format changes.
- **Testing persistence in `:memory:` mode:** File-backed persistence cannot be tested with `:memory:` -- it needs an actual file path. Use sqllogictest `restart` or temp file.
- **Changing `persist_*` function signatures without updating all three DDL files:** The parameterization work touches `define.rs`, `drop.rs`, and `alter.rs` -- all must be updated atomically.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Parameterized SQL queries | Custom string escaping | `duckdb_prepare` + `duckdb_bind_varchar` | Eliminates entire class of injection/escaping bugs |
| Test fixture builders | Per-test struct literals | Shared `test_helpers.rs` functions | `graph/test_helpers.rs` already proves the pattern |
| Layout assertions | Manual pointer arithmetic tests | `std::mem::size_of` / `std::mem::align_of` | Compiler-guaranteed to detect layout changes |

## Common Pitfalls

### Pitfall 1: Serde Backward Compatibility Breakage
**What goes wrong:** Changing `column_type_names`/`column_types_inferred` to a struct breaks deserialization of old stored JSON.
**Why it happens:** Old JSON has separate array keys; new struct expects a different shape.
**How to avoid:** Either (a) add a helper method without changing the serde representation, or (b) implement a custom `Deserialize` that handles both old and new formats. Test with old JSON fixtures.
**Warning signs:** `serde_json::from_str` fails on existing test JSON strings.

### Pitfall 2: Prepared Statement Cleanup Leaks
**What goes wrong:** Forgetting `duckdb_destroy_prepare` after `duckdb_prepare` leaks the prepared statement handle.
**Why it happens:** C API requires manual cleanup; no RAII in FFI code.
**How to avoid:** Create a small RAII wrapper or ensure every code path (success and error) calls `duckdb_destroy_prepare`. Consider a helper function that encapsulates the full prepare/bind/execute/cleanup cycle.
**Warning signs:** Valgrind/ASAN reports after running persistence tests.

### Pitfall 3: Test Fixture Extraction Breaking Test Independence
**What goes wrong:** Shared fixtures accidentally couple tests -- changing a fixture breaks unrelated tests.
**Why it happens:** Extracting too many fields into the shared builder, making tests depend on specific fixture state.
**How to avoid:** Keep shared fixtures minimal (base case only). Each test should override only what it needs. Follow the `graph/test_helpers.rs` pattern where `make_def()` takes parameters for the varying parts.
**Warning signs:** Changing one test fixture function breaks many unrelated tests.

### Pitfall 4: Extension Feature Gate Confusion
**What goes wrong:** New tests fail under `cargo test` because they need `extension` feature, or fail under `just test-sql` because they need `bundled` feature.
**Why it happens:** The crate has a split feature model: `bundled` for unit tests, `extension` for loadable builds.
**How to avoid:** The transmute layout test uses only `duckdb::vtab::Value` and `ffi::duckdb_value` types, both available under `bundled`. The file-backed round-trip test should be a sqllogictest (`.test` file) that exercises the full LOAD path.
**Warning signs:** `cargo test` compiles but test is skipped or panics on missing symbols.

## Code Examples

### Parameterized Query Helper Pattern
```rust
// Source: DuckDB C API documentation + libduckdb-sys bindgen output
use libduckdb_sys as ffi;
use std::ffi::CString;

/// Execute a parameterized query with VARCHAR bindings via the C API.
///
/// # Safety
/// `conn` must be a valid, open duckdb_connection.
unsafe fn execute_parameterized(
    conn: ffi::duckdb_connection,
    sql: &str,
    params: &[&str],
) -> Result<(), String> {
    let c_sql = CString::new(sql).map_err(|_| "SQL contains null byte".to_string())?;
    let mut stmt: ffi::duckdb_prepared_statement = std::ptr::null_mut();

    let rc = ffi::duckdb_prepare(conn, c_sql.as_ptr(), &mut stmt);
    if rc != ffi::DuckDBSuccess {
        let err = ffi::duckdb_prepare_error(stmt);
        let msg = if err.is_null() {
            "unknown prepare error".to_string()
        } else {
            std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned()
        };
        ffi::duckdb_destroy_prepare(&mut stmt);
        return Err(msg);
    }

    for (i, param) in params.iter().enumerate() {
        let c_param = CString::new(*param).map_err(|_| "param contains null byte".to_string())?;
        let rc = ffi::duckdb_bind_varchar(stmt, (i + 1) as ffi::idx_t, c_param.as_ptr());
        if rc != ffi::DuckDBSuccess {
            ffi::duckdb_destroy_prepare(&mut stmt);
            return Err(format!("failed to bind parameter {}", i + 1));
        }
    }

    let mut result: ffi::duckdb_result = std::mem::zeroed();
    let rc = ffi::duckdb_execute_prepared(stmt, &mut result);
    let success = rc == ffi::DuckDBSuccess;
    ffi::duckdb_destroy_result(&mut result);
    ffi::duckdb_destroy_prepare(&mut stmt);

    if success {
        Ok(())
    } else {
        Err("prepared statement execution failed".to_string())
    }
}
```

### Catalog TOCTOU Fix Pattern
```rust
// Source: existing catalog_rename in catalog.rs:169-185
pub fn catalog_insert(
    state: &CatalogState,
    name: &str,
    json: &str,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    SemanticViewDefinition::from_json(name, json)
        .map_err(Box::<dyn std::error::Error>::from)?;

    let mut guard = state.write().unwrap();
    if guard.contains_key(name) {
        return Err(format!(
            "semantic view '{name}' already exists; use CREATE OR REPLACE SEMANTIC VIEW to overwrite"
        ).into());
    }
    guard.insert(name.to_string(), json.to_string());
    Ok(())
}
```

### Test Fixture Builder Pattern
```rust
// Source: existing graph/test_helpers.rs pattern
use crate::model::{Dimension, Metric, SemanticViewDefinition, TableRef};

/// Base orders view: single table, 2 dimensions, 2 metrics.
pub(super) fn orders_view() -> SemanticViewDefinition {
    SemanticViewDefinition {
        base_table: "orders".to_string(),
        tables: vec![],
        dimensions: vec![
            Dimension { name: "region".into(), expr: "region".into(), ..Default::default() },
            Dimension { name: "status".into(), expr: "status".into(), ..Default::default() },
        ],
        metrics: vec![
            Metric { name: "total_revenue".into(), expr: "sum(amount)".into(), ..Default::default() },
            Metric { name: "order_count".into(), expr: "count(*)".into(), ..Default::default() },
        ],
        ..Default::default()
    }
}

/// Extend a definition with a join to a customers table.
pub(super) fn with_customers_join(mut def: SemanticViewDefinition) -> SemanticViewDefinition {
    // ... add tables, join, customer dimensions
    def
}
```

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) + proptest 1.x + sqllogictest (Python runner) |
| Config file | Cargo.toml `[dev-dependencies]` + Makefile test targets |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map

Since Phase 42 has no formal requirement IDs yet, mapping by item:

| Item | Behavior | Test Type | Automated Command | File Exists? |
|------|----------|-----------|-------------------|-------------|
| TOCTOU fix | `catalog_insert` uses single write lock | unit | `cargo test catalog` | Existing tests cover insert/delete |
| Parameterized queries | Persistence uses prepared statements | integration (sqllogictest) | `just test-sql` | Existing sqllogictest files exercise DDL persistence |
| Parallel Vec | Model unchanged externally | unit | `cargo test model` | Existing serde roundtrip tests |
| Body parser comments | No behavior change | N/A | N/A | N/A |
| Test fixtures | Same test coverage, less code | unit | `cargo test expand` | Existing 77 tests in sql_gen.rs |
| File-backed round-trip | Create, close, reopen, verify | integration | `just test-sql` | NEW: phase42_persistence.test |
| Transmute guard | Layout assertion | unit | `cargo test value_layout` | NEW: in table_function.rs or lib.rs |
| Suggestion proptest | Suggestions are valid names | proptest | `cargo test suggest` | NEW: in util.rs tests |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase42_persistence.test` -- file-backed catalog round-trip test
- [ ] Transmute layout assertion test (in existing test module)
- [ ] `suggest_closest` property test (in `src/util.rs` tests or `tests/`)
- Framework install: None needed -- all test infrastructure exists

## Open Questions

1. **Parallel Vec: Helper method vs struct migration?**
   - What we know: The struct migration touches ~60 locations and requires serde compat handling. A helper method touches 0 locations and adds a convenience API.
   - What's unclear: Whether the planner/user prefers the full migration for long-term cleanliness or the minimal helper for safety.
   - Recommendation: Start with the helper method approach (`fn inferred_types(&self) -> impl Iterator<Item = (&str, u32)>`). The struct migration can be a future follow-up if desired. This avoids JSON format changes and minimizes risk.

2. **Parameterized query helper location?**
   - What we know: The persist functions are in `ddl/define.rs`, `ddl/drop.rs`, `ddl/alter.rs`. The helper could live in any of: a new `ddl/persist.rs`, or in `catalog.rs`, or in `lib.rs`.
   - What's unclear: Which module best owns this helper.
   - Recommendation: Create `ddl/persist.rs` as a shared module for persistence utilities. This keeps it with the DDL code that uses it and avoids bloating `catalog.rs` with FFI code.

## Sources

### Primary (HIGH confidence)
- `src/catalog.rs` -- read directly, verified TOCTOU pattern and correct `catalog_rename` pattern
- `src/ddl/define.rs`, `src/ddl/drop.rs`, `src/ddl/alter.rs` -- read directly, identified 4 `ffi::duckdb_query` call sites
- `src/model.rs` -- read directly, verified parallel Vec fields at lines 181-192
- `src/body_parser.rs` -- read directly, verified unwrap locations at lines 448, 500, 796
- `src/expand/sql_gen.rs` -- read directly, verified 77 tests, ~1,800 lines of inline test fixtures
- `src/graph/test_helpers.rs` -- read directly, verified existing fixture pattern (199 lines, 4 helper functions)
- `src/query/table_function.rs` -- read directly, verified `value_raw_ptr` transmute at lines 130-148
- `src/util.rs` -- read directly, verified `suggest_closest` implementation
- `_notes/code-review-2026-04-04.md` -- full code review, primary input for phase scope
- `libduckdb-sys` bindgen output -- verified `duckdb_prepare`, `duckdb_bind_varchar`, `duckdb_execute_prepared`, `duckdb_destroy_prepare` all available as loadable-extension function pointers

### Secondary (MEDIUM confidence)
- DuckDB C API parameter syntax (`$1`, `$2` positional) -- based on DuckDB documentation and bindgen signatures

## Metadata

**Confidence breakdown:**
- Catalog TOCTOU fix: HIGH -- pattern already exists in `catalog_rename`, trivial change
- Parameterized queries: HIGH -- FFI functions verified in bindgen output, pattern well-understood
- Parallel Vec: HIGH -- code read directly, two approaches analyzed with tradeoffs
- Test fixtures: HIGH -- existing pattern in `graph/test_helpers.rs`, clear scope
- Body parser comments: HIGH -- code read directly, invariants verified
- New tests (round-trip, transmute, proptest): HIGH -- existing test infrastructure supports all three

**Research date:** 2026-04-04
**Valid until:** 2026-05-04 (stable codebase, no external dependency changes expected)
