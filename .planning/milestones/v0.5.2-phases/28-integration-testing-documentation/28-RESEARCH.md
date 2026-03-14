# Phase 28: Integration Testing & Documentation - Research

**Researched:** 2026-03-13
**Domain:** Rust DuckDB extension -- function DDL removal, E2E integration testing, README rewrite
**Confidence:** HIGH

## Summary

Phase 28 has three pillars: (1) remove the function-based CREATE DDL interface (`create_semantic_view()`, `create_or_replace_semantic_view()`, `create_semantic_view_if_not_exists()`) and its supporting code, (2) write end-to-end integration tests using the AS-body PK/FK syntax with full result verification, and (3) rewrite README.md to show only the new SQL DDL syntax.

The removal is well-scoped. The function DDL path is isolated: `DefineSemanticViewVTab` (in `src/ddl/define.rs`), `parse_args.rs` (argument parser), 3 function registrations in `src/lib.rs`, and several test/integration files that use the old syntax. The `_from_json` variants (`DefineFromJsonVTab`) stay because they are the backend for native DDL rewriting. Non-CREATE functions (`drop_semantic_view()`, `explain_semantic_view()`, `semantic_view()`, `list_semantic_views()`, `describe_semantic_view()`, `drop_semantic_view_if_exists()`) stay unchanged.

**Primary recommendation:** Execute in 3 waves -- (1) remove code + fix broken tests, (2) add E2E tests, (3) rewrite README. Wave 1 must come first because broken tests block `just test-all`.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Retire the 3 CREATE function DDL variants: `create_semantic_view()`, `create_or_replace_semantic_view()`, `create_semantic_view_if_not_exists()`
- Remove `DefineSemanticViewVTab`, `parse_args.rs`, and the 3 function registrations in `lib.rs`
- Keep all non-CREATE functions: `explain_semantic_view()`, `semantic_view()`, `drop_semantic_view()`, `drop_semantic_view_if_exists()`, `list_semantic_views()`, `describe_semantic_view()`
- Keep `explain_semantic_view()` as a table function (DuckDB EXPLAIN sees semantic_view as a black box)
- The `_from_json` VTab variants stay -- they are the backend for native DDL rewriting
- Cancel Phase 24 entirely -- mark as cancelled/superseded, close DDL-06 and MDL-01 through MDL-05 as won't-do
- Rewrite test files that exercise unique scenarios (restart persistence, error reporting, etc.) to use AS-body syntax
- Delete test files that overlap with newer phase test files
- README: clean slate, show only AS-body PK/FK syntax, no mention of function DDL
- README structure: How it works, Quick start (single table), Multi-table (PK/FK relationships), DDL reference, Building
- Update version line to "v0.5.2 -- early-stage, not yet on the community registry"
- Use orders/customers/products e-commerce domain for examples
- E2E: full result verification with known inserted data -- assert exact result rows, not just execution success
- E2E: 3+ table PK/FK semantic view scenario (orders/customers/products domain)
- E2E: test both `semantic_view()` queries and `explain_semantic_view()` output

### Claude's Discretion
- Test file organization (single comprehensive vs split by concern)
- Which existing test files are "valuable" vs redundant
- README section ordering and exact content structure
- Whether to keep the "Function syntax" note in README or omit entirely

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DOC-01 | README updated with new SQL DDL syntax reference, PK/FK relationship examples, and qualified column usage | README rewrite section below covers structure, content, and e-commerce example domain |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| sqllogictest | runner v0.22+ | SQL integration tests (.test files) | Project's existing test framework for extension-level testing |
| cargo test | stable | Rust unit + proptest | Project standard for non-extension testing |
| uv | latest | Python integration test runner | Project standard for Python-based integration tests |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| just | latest | Task runner | `just test-all` is the quality gate |
| serde_json | existing | JSON serialization for model | Used by DefineFromJsonVTab, no changes needed |

No new dependencies required. This phase is purely code removal, test authoring, and documentation.

## Architecture Patterns

### Recommended Project Structure (changes only)
```
src/
├── ddl/
│   ├── define.rs       # REMOVE DefineSemanticViewVTab, KEEP DefineFromJsonVTab
│   ├── mod.rs          # REMOVE parse_args module declaration
│   ├── parse_args.rs   # DELETE entirely
│   ├── describe.rs     # unchanged
│   ├── drop.rs         # unchanged
│   └── list.rs         # unchanged
├── lib.rs              # REMOVE 3 create_semantic_view registrations + import
├── parse.rs            # REMOVE dead CREATE arms from function_name() match
└── ...                 # everything else unchanged

test/sql/
├── phase2_ddl.test           # DELETE (entirely function DDL; overlaps with phase20/25)
├── phase2_restart.test       # REWRITE to AS-body syntax (unique: restart persistence)
├── phase4_query.test         # REWRITE remaining function DDL to native DDL
├── semantic_views.test       # DELETE (smoke test only; require directive tested elsewhere)
├── phase20_extended_ddl.test # REWRITE backward compat section (remove function DDL test)
├── phase28_e2e.test          # NEW: 3-table E2E integration test
├── phase25_keyword_body.test # unchanged
├── phase26_join_resolution.test # unchanged
├── phase27_qualified_refs.test  # unchanged
└── phase21_error_reporting.test # unchanged

test/integration/
├── test_ducklake_ci.py       # REWRITE to native DDL (currently uses create_semantic_view())
├── test_vtab_crash.py        # REWRITE to native DDL (currently uses create_semantic_view())
└── test_ducklake.py          # REWRITE to native DDL (currently uses create_semantic_view())
```

### Pattern 1: Function DDL Removal
**What:** Remove `DefineSemanticViewVTab` struct, its `VTab` impl, and `parse_args.rs`
**When to use:** This is the primary removal pattern for this phase.

The import chain is:
1. `src/lib.rs` imports `DefineSemanticViewVTab` from `src/ddl/define.rs`
2. `src/ddl/define.rs` imports `parse_define_args_from_bind` from `src/ddl/parse_args.rs`
3. `src/ddl/parse_args.rs` imports `value_raw_ptr` from `src/query/table_function.rs`

After removal:
- `DefineFromJsonVTab` in `define.rs` has NO dependency on `parse_args.rs`
- `parse_define_args_from_bind` has NO other callers
- `value_raw_ptr` in `table_function.rs` is STILL used by `parse_args.rs` callers -- but after removing `parse_args.rs`, check if `value_raw_ptr` has any remaining callers. If not, it can also be removed (or `allow(dead_code)` if it's part of the test_helpers module).

**Key checks after removal:**
```bash
cargo test    # verify compilation + unit tests
just build    # verify extension builds
```

### Pattern 2: Test File Evaluation
**What:** Determine which test files to delete vs rewrite
**Decision matrix:**

| File | Unique Value | Function DDL? | Action |
|------|-------------|---------------|--------|
| `phase2_ddl.test` | DDL round-trip | 100% function DDL | DELETE -- phase20+25 cover all DDL verbs via native syntax |
| `phase2_restart.test` | Restart persistence | Uses function DDL | REWRITE -- unique scenario, not covered elsewhere |
| `phase4_query.test` | Query round-trip, WHERE, explain, typed output | Mixed (joined_orders already native DDL) | REWRITE -- has unique query scenarios (WHERE, typed output, error cases) |
| `semantic_views.test` | Smoke test (LOAD) | N/A (just SELECT 42) | DELETE -- phase25 already does `require semantic_views` |
| `phase20_extended_ddl.test` | All 7 DDL verbs via native syntax | Mostly native; one backward-compat section uses function DDL | REWRITE -- remove the backward-compat section (lines 182-201) |

### Pattern 3: Python Integration Test Rewrite
**What:** Convert `create_semantic_view()` calls to `CREATE SEMANTIC VIEW ... AS ...` in Python
**Key difference:** Python `con.execute()` can run native DDL directly -- just change the SQL string.

Before:
```python
con.execute("""
    SELECT * FROM create_semantic_view(
        'orders',
        tables := [{'alias': 'o', 'table': 'orders'}],
        dimensions := [{'name': 'region', 'expr': 'region', 'source_table': 'o'}],
        metrics := [{'name': 'revenue', 'expr': 'sum(amount)', 'source_table': 'o'}]
    )
""")
```

After:
```python
con.execute("""
    CREATE SEMANTIC VIEW orders AS
    TABLES (
        o AS orders PRIMARY KEY (id)
    )
    DIMENSIONS (
        o.region AS region
    )
    METRICS (
        o.revenue AS sum(amount)
    )
""")
```

**Note:** The native DDL path requires `PRIMARY KEY` on each table. The function DDL path did not. Existing Python tests must add PK declarations.

Similarly, `drop_semantic_view('name')` calls change to `DROP SEMANTIC VIEW name`.

### Pattern 4: E2E Test Design
**What:** New sqllogictest file with known-data, exact-result assertions
**Domain:** orders/customers/products e-commerce (matching CONTEXT.md decision)

```sql
-- Setup: 3 related tables with known data
CREATE TABLE p28_orders (...);
CREATE TABLE p28_customers (...);
CREATE TABLE p28_products (...);

INSERT INTO p28_orders VALUES (...);  -- known data, hand-computed expected results
INSERT INTO p28_customers VALUES (...);
INSERT INTO p28_products VALUES (...);

-- Define: 3-table PK/FK semantic view
CREATE SEMANTIC VIEW p28_analytics AS
  TABLES (
    o AS p28_orders PRIMARY KEY (id),
    c AS p28_customers PRIMARY KEY (id),
    p AS p28_products PRIMARY KEY (id)
  )
  RELATIONSHIPS (
    order_to_customer AS o(customer_id) REFERENCES c,
    order_to_product AS o(product_id) REFERENCES p
  )
  DIMENSIONS (
    o.region AS o.region,
    c.customer_name AS c.name,
    p.product_name AS p.name
  )
  METRICS (
    o.total_revenue AS sum(o.amount),
    o.order_count AS count(*)
  );

-- Query: full result verification (not just execution success)
query TTT rowsort
SELECT * FROM semantic_view('p28_analytics',
    dimensions := ['customer_name'], metrics := ['total_revenue']);
----
Alice	300.00
Bob	50.00

-- Explain: verify expanded SQL contains expected clauses
query I
SELECT count(*) FROM explain_semantic_view('p28_analytics',
    dimensions := ['customer_name'], metrics := ['total_revenue'])
WHERE explain_output LIKE '%LEFT JOIN%';
----
1
```

### Anti-Patterns to Avoid
- **Removing too much from parse.rs:** The `function_name()` match arms for CREATE forms are dead code (rewrite_ddl rejects them), but they are harmless. If removing, ensure proptests still compile -- they reference `DdlKind::Create` etc. which are parser enum variants, not function registrations.
- **Breaking _from_json path:** The `_from_json` variants use `DefineBindData`/`DefineInitData`/`DefineState` defined in `define.rs`. When removing `DefineSemanticViewVTab`, do NOT remove these shared types.
- **Forgetting Python tests:** The `just test-all` command runs `test-vtab-crash` and `test-caret` and `test-ducklake-ci`. All 3 Python integration tests that use `create_semantic_view()` will FAIL after removal if not updated.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SQL integration testing | Custom test harness | sqllogictest `.test` files | Project standard, handles extension LOAD, result comparison |
| README formatting | Complex markdown generation | Hand-written markdown | Only 1 file, simple structure, no tooling needed |

## Common Pitfalls

### Pitfall 1: Compilation Order
**What goes wrong:** Removing `DefineSemanticViewVTab` before removing its callers causes compile errors in `lib.rs`.
**Why it happens:** `lib.rs` imports `DefineSemanticViewVTab` and uses it in 3 registrations.
**How to avoid:** Remove in dependency order: (1) remove registrations in `lib.rs`, (2) remove `DefineSemanticViewVTab` from `define.rs`, (3) remove `parse_args.rs`, (4) remove `parse_args` from `ddl/mod.rs`.
**Warning signs:** Compiler errors about missing types or imports.

### Pitfall 2: Shared Types in define.rs
**What goes wrong:** Accidentally removing `DefineBindData`, `DefineInitData`, `DefineState`, or `persist_define()` which are used by `DefineFromJsonVTab`.
**Why it happens:** They're defined near `DefineSemanticViewVTab` and seem related.
**How to avoid:** Only remove the `DefineSemanticViewVTab` struct and its `impl VTab` block. Everything else in `define.rs` is shared.
**Warning signs:** Compile errors referencing `DefineBindData` or `DefineState` from `DefineFromJsonVTab`.

### Pitfall 3: parse.rs Dead Code After Removal
**What goes wrong:** The `function_name()` match arms for CREATE kinds become dead code after removal, but `rewrite_ddl` already rejects them.
**Why it happens:** These arms were kept for the `rewrite_ddl` function which was already restricted to non-CREATE forms.
**How to avoid:** Optionally clean up `function_name()` to remove the 3 CREATE arms. But note: this function is ONLY called by `rewrite_ddl()` which already rejects CREATE forms. The cleanup is cosmetic, not functional.
**Warning signs:** `cargo clippy` may warn about unreachable patterns if the CREATE arms in `function_name()` are the only remaining references.

### Pitfall 4: Python Test PK Requirement
**What goes wrong:** Converting function DDL to native DDL but forgetting to add `PRIMARY KEY (...)` to TABLES clause.
**Why it happens:** Function DDL had no PK concept -- tables were just `{'alias': 'o', 'table': 'orders'}`. Native DDL requires `o AS orders PRIMARY KEY (col)`.
**How to avoid:** Every table in a native DDL TABLES clause must have `PRIMARY KEY (...)`. Pick a column that makes sense (usually the id column).
**Warning signs:** Parse errors mentioning "PRIMARY KEY" in test output.

### Pitfall 5: TEST_LIST Not Updated
**What goes wrong:** New test file added to `test/sql/` but not added to `test/sql/TEST_LIST`.
**Why it happens:** sqllogictest runner uses TEST_LIST to determine which files to run.
**How to avoid:** Add any new `.test` file to TEST_LIST. Remove deleted files from TEST_LIST.
**Warning signs:** New test not executing during `just test-sql`.

### Pitfall 6: DuckLake Test Uses DuckLake-Qualified Tables
**What goes wrong:** `test_ducklake_ci.py` uses `jaffle.raw_orders` as the table name. Native DDL needs this exact name in the TABLES clause.
**Why it happens:** DuckLake tables are in the `jaffle` catalog, accessed via `jaffle.raw_orders`.
**How to avoid:** Use the full qualified name in the AS clause: `o AS jaffle.raw_orders PRIMARY KEY (id)`.
**Warning signs:** "Table not found" errors in DuckLake CI test.

### Pitfall 7: value_raw_ptr Becomes Dead Code
**What goes wrong:** After removing `parse_args.rs`, the `value_raw_ptr` function in `table_function.rs` may lose all callers within the extension feature gate.
**Why it happens:** `parse_args.rs` was its only caller from DDL code.
**How to avoid:** Check remaining callers of `value_raw_ptr`. It is `pub(crate)` -- if no other callers exist under `#[cfg(feature = "extension")]`, it may need `#[allow(dead_code)]` or be removed.
**Warning signs:** `cargo clippy` warning about unused function.

## Code Examples

### Example 1: lib.rs Registration Removal

Current code to remove (3 blocks like this):
```rust
// In src/lib.rs, init_extension function:

// REMOVE these 3 blocks:
let define_state = DefineState { ... or_replace: false, if_not_exists: false };
con.register_table_function_with_extra_info::<DefineSemanticViewVTab, _>(
    "create_semantic_view", &define_state)?;

let define_or_replace_state = DefineState { ... or_replace: true, if_not_exists: false };
con.register_table_function_with_extra_info::<DefineSemanticViewVTab, _>(
    "create_or_replace_semantic_view", &define_or_replace_state)?;

let define_if_not_exists_state = DefineState { ... or_replace: false, if_not_exists: true };
con.register_table_function_with_extra_info::<DefineSemanticViewVTab, _>(
    "create_semantic_view_if_not_exists", &define_if_not_exists_state)?;

// KEEP these 3 blocks (the _from_json variants):
con.register_table_function_with_extra_info::<DefineFromJsonVTab, _>(
    "create_semantic_view_from_json", &define_state)?;
// ... etc
```

After: The `DefineState` instances created for the `_from_json` registrations still need `or_replace` and `if_not_exists` flags. The 3 `DefineState` instances can be reused as-is for `_from_json` registrations.

**Critical detail:** The `_from_json` registrations use the SAME `DefineState` as the removed registrations. After removing the 3 function DDL registrations, the `_from_json` registrations still need their own `DefineState` with the correct flags. Currently, `define_state` (used by both `create_semantic_view` and `create_semantic_view_from_json`) is defined once and shared. After removing `create_semantic_view`, `define_state` is still needed for `create_semantic_view_from_json`.

### Example 2: define.rs Cleanup

Remove:
```rust
// The entire DefineSemanticViewVTab struct and impl VTab block (lines ~102-244)
pub struct DefineSemanticViewVTab;
impl VTab for DefineSemanticViewVTab { ... }
```

Also remove the import:
```rust
use crate::ddl::parse_args::parse_define_args_from_bind;
```

Keep everything else: `DefineState`, `DefineBindData`, `DefineInitData`, `persist_define()`, `DefineFromJsonVTab`.

### Example 3: E2E Test With Exact Result Verification

```sql
# Create tables with known data
statement ok
CREATE TABLE p28_customers (id INTEGER, name VARCHAR, tier VARCHAR);

statement ok
INSERT INTO p28_customers VALUES (1, 'Alice', 'gold'), (2, 'Bob', 'silver'), (3, 'Charlie', 'gold');

statement ok
CREATE TABLE p28_products (id INTEGER, name VARCHAR, category VARCHAR);

statement ok
INSERT INTO p28_products VALUES (10, 'Widget', 'hardware'), (20, 'Gadget', 'electronics');

statement ok
CREATE TABLE p28_orders (
    id INTEGER, customer_id INTEGER, product_id INTEGER,
    amount DECIMAL(10,2), region VARCHAR
);

statement ok
INSERT INTO p28_orders VALUES
    (1, 1, 10, 100.00, 'East'),
    (2, 1, 20, 200.00, 'East'),
    (3, 2, 10, 50.00, 'West'),
    (4, 3, 10, 150.00, 'East'),
    (5, 3, 20, 75.00, 'West');

# Define 3-table PK/FK semantic view
statement ok
CREATE SEMANTIC VIEW p28_analytics AS
  TABLES (
    o AS p28_orders PRIMARY KEY (id),
    c AS p28_customers PRIMARY KEY (id),
    p AS p28_products PRIMARY KEY (id)
  )
  RELATIONSHIPS (
    order_to_customer AS o(customer_id) REFERENCES c,
    order_to_product AS o(product_id) REFERENCES p
  )
  DIMENSIONS (
    c.customer_name AS c.name,
    p.product_name AS p.name,
    o.region AS o.region
  )
  METRICS (
    o.total_revenue AS sum(o.amount),
    o.order_count AS count(*)
  );

# Cross-table query: customer_name from c, total_revenue from o
# Alice: 100+200=300, Bob: 50, Charlie: 150+75=225
query TR rowsort
SELECT * FROM semantic_view('p28_analytics',
    dimensions := ['customer_name'], metrics := ['total_revenue']);
----
Alice	300.00
Bob	50.00
Charlie	225.00

# 3-table transitive: product_name -> o -> c via customer
query TI rowsort
SELECT * FROM semantic_view('p28_analytics',
    dimensions := ['product_name'], metrics := ['order_count']);
----
Gadget	2
Widget	3
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Function DDL: `create_semantic_view()` | Native DDL: `CREATE SEMANTIC VIEW ... AS ...` | v0.5.0 (Phase 25) | Both paths coexisted; Phase 28 removes function DDL |
| Paren-body syntax: `(tables := [...])` | AS-body syntax: `AS TABLES (...) ...` | v0.5.2 (Phase 25/CLN-01) | Paren-body removed in Phase 27 |
| ON-clause join heuristic | PK/FK relationship graph | v0.5.2 (Phase 26) | Old heuristic removed in Phase 27 |
| CTE-based expansion | Flat FROM/JOIN expansion | v0.5.2 (Phase 26) | CTE wrapper removed |

**Deprecated/outdated:**
- `create_semantic_view()` function: Being removed in this phase
- `create_or_replace_semantic_view()` function: Being removed in this phase
- `create_semantic_view_if_not_exists()` function: Being removed in this phase
- `parse_args.rs`: Being deleted in this phase

## Impact Analysis

### Files to Modify (source code)

| File | Change | Risk |
|------|--------|------|
| `src/lib.rs` | Remove 3 function registrations + `DefineSemanticViewVTab` import | LOW -- isolated removal |
| `src/ddl/define.rs` | Remove `DefineSemanticViewVTab` struct + impl + `parse_args` import | LOW -- shared types stay |
| `src/ddl/mod.rs` | Remove `pub mod parse_args;` line | LOW -- one line |
| `src/ddl/parse_args.rs` | DELETE file | LOW -- no other callers |
| `src/parse.rs` | Optional: remove CREATE arms from `function_name()` | LOW -- cosmetic |

### Files to Modify (tests)

| File | Change | Risk |
|------|--------|------|
| `test/sql/phase2_ddl.test` | DELETE | LOW -- coverage exists elsewhere |
| `test/sql/semantic_views.test` | DELETE | LOW -- smoke test redundant |
| `test/sql/phase2_restart.test` | REWRITE to AS-body DDL | MEDIUM -- restart test is unique |
| `test/sql/phase4_query.test` | REWRITE remaining function DDL calls | MEDIUM -- many scenarios |
| `test/sql/phase20_extended_ddl.test` | Remove backward-compat section (lines 182-201) | LOW -- small change |
| `test/sql/TEST_LIST` | Remove deleted files, add new file | LOW |
| `test/integration/test_ducklake_ci.py` | REWRITE to native DDL | MEDIUM -- DuckLake table naming |
| `test/integration/test_vtab_crash.py` | REWRITE to native DDL | MEDIUM -- 13 test vectors |
| `test/integration/test_ducklake.py` | REWRITE to native DDL | LOW -- similar to CI test |

### Files to Create

| File | Purpose |
|------|---------|
| `test/sql/phase28_e2e.test` | 3-table E2E integration test with exact result verification |
| `README.md` (rewrite) | Clean-slate documentation with AS-body PK/FK syntax |

### Files NOT Changed

These stay as-is:
- `src/ddl/describe.rs`, `src/ddl/drop.rs`, `src/ddl/list.rs`
- `src/query/table_function.rs`, `src/query/explain.rs`
- `src/body_parser.rs`, `src/catalog.rs`, `src/expand.rs`, `src/graph.rs`, `src/model.rs`
- `test/sql/phase25_keyword_body.test`, `test/sql/phase26_join_resolution.test`, `test/sql/phase27_qualified_refs.test`
- `test/sql/phase21_error_reporting.test`
- `tests/parse_proptest.rs`, `tests/expand_proptest.rs`, `tests/output_proptest.rs`, `tests/vector_reference_test.rs`

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest (Python runner) + cargo test (Rust) + uv (Python integration) |
| Config file | `test/sql/TEST_LIST` (sqllogictest file list) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DOC-01 | README updated with new DDL syntax, PK/FK examples, qualified columns | manual review | N/A (documentation review) | Will create README.md |
| (implicit) | Function DDL removal compiles | unit | `cargo test` | Existing |
| (implicit) | 3-table E2E with exact results | integration | `just test-sql` | Wave 0: `test/sql/phase28_e2e.test` |
| (implicit) | explain_semantic_view output verified | integration | `just test-sql` | Wave 0: `test/sql/phase28_e2e.test` |
| (implicit) | All existing tests pass after rewrite | integration | `just test-all` | Existing (rewritten) |

### Sampling Rate
- **Per task commit:** `cargo test` (fast, catches compilation + unit regressions)
- **Per wave merge:** `just test-all` (full suite: Rust + sqllogictest + DuckLake CI + vtab crash + caret)
- **Phase gate:** `just test-all` green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase28_e2e.test` -- 3-table E2E integration test (NEW)
- [ ] Update `test/sql/TEST_LIST` -- add `phase28_e2e.test`, remove `phase2_ddl.test` and `semantic_views.test`

## Open Questions

1. **value_raw_ptr after parse_args removal**
   - What we know: `value_raw_ptr` is in `src/query/table_function.rs`, called by `parse_args.rs` and potentially by `table_function.rs` itself
   - What's unclear: Whether any callers remain after `parse_args.rs` deletion
   - Recommendation: Check with `cargo test` after removal; if dead code warning appears, add `#[allow(dead_code)]` or remove if truly unused

2. **parse_proptest.rs CREATE_FORMS constant**
   - What we know: `CREATE_FORMS` references `"create_semantic_view"` as a string, but tests only check `detect_ddl_kind()` (parser detection), not function registration
   - What's unclear: Whether any test assertions compare against the function name string
   - Recommendation: Leave `CREATE_FORMS` as-is -- it tests parser detection, not function existence. Verify with `cargo test` after removal.

3. **phase2_restart.test exclusion from TEST_LIST**
   - What we know: This file is deliberately excluded from TEST_LIST (per comment in file: Python runner can't reload extensions after `restart` directive)
   - What's unclear: Whether the same limitation applies after rewriting to AS-body syntax
   - Recommendation: Keep excluded from TEST_LIST after rewrite. The restart scenario is still tested via `cargo test` Rust integration tests. Add a comment to the rewritten file explaining this.

## Sources

### Primary (HIGH confidence)
- Direct code inspection of `src/lib.rs`, `src/ddl/define.rs`, `src/ddl/parse_args.rs`, `src/ddl/mod.rs`
- Direct code inspection of all test files in `test/sql/` and `test/integration/`
- `CONTEXT.md` user decisions
- `CLAUDE.md` project instructions (quality gate: `just test-all`)

### Secondary (MEDIUM confidence)
- `REQUIREMENTS.md` for DOC-01 requirement definition
- `STATE.md` for phase history and accumulated decisions

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, existing project tooling
- Architecture: HIGH -- direct code analysis of all files involved
- Pitfalls: HIGH -- identified from concrete code dependencies and cross-references
- Test strategy: HIGH -- follows existing project patterns (sqllogictest + Python integration)

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable -- no external dependency changes)
