# Phase 55: Materialization Routing Engine - Research

**Researched:** 2026-04-19
**Domain:** Rust query expansion engine, set-containment matching, SQL generation
**Confidence:** HIGH

## Summary

Phase 55 adds query-time materialization routing to the expansion engine. When a user queries `semantic_view('v', dimensions := ['region'], metrics := ['revenue'])`, the engine checks whether any declared materialization exactly covers all requested dimensions and metrics. If one does, the generated SQL reads from the materialization's pre-aggregated table instead of expanding the raw source tables with JOINs and GROUP BY. If no materialization matches, or if the query involves semi-additive or window function metrics, the engine falls back to standard expansion with no visible behavior change.

The implementation is architecturally clean: the `expand()` function in `src/expand/sql_gen.rs` is the single entry point for all query SQL generation. Materialization routing is an early-exit path inserted after name resolution (steps 2-3) but before SQL generation (step 5). If a matching materialization is found and no excluded metric types are present, the function returns a simple `SELECT dim1, dim2, ..., met1, met2, ... FROM mat_table` instead of the full expansion pipeline. The `explain_semantic_view()` function uses the same `expand()` call, so routing decisions will automatically appear in explain output once Phase 57 adds routing metadata.

The Materialization struct, MATERIALIZATIONS clause parser, persistence, and YAML support were all completed in Phase 54. The data model is already in `SemanticViewDefinition.materializations: Vec<Materialization>` where each `Materialization` has `name`, `table`, `dimensions: Vec<String>`, and `metrics: Vec<String>`. This phase only needs to consume that data at query time.

**Primary recommendation:** Implement materialization routing as a new `expand/materialization.rs` submodule with a `try_route_materialization()` function, called from `expand()` after name resolution. Keep the routing logic pure (no side effects, no DB access) -- it takes the resolved dims/metrics/def and returns `Option<String>` (the routed SQL or None for fallback).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| MAT-02 | At query time, the engine routes to a materialization when it exactly covers the requested dimensions and metrics | Set-containment matching function in new `materialization.rs` submodule, called from `expand()` after step 3 (name resolution) |
| MAT-03 | When no materialization matches, the query falls back to raw table expansion (no error) | `try_route_materialization()` returns `Option<String>` -- `None` means no match, `expand()` continues normally |
| MAT-04 | Semi-additive and window function metrics are excluded from materialization routing (always expand from raw) | Check `non_additive_by` and `window_spec` on resolved metrics before attempting routing; any presence = skip routing entirely |
| MAT-05 | Materialization routing is transparent -- no user-visible behavior change without matching materializations | Empty `materializations` vec -> `try_route_materialization()` returns `None` immediately, zero cost path |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Quality gate**: `just test-all` must pass (Rust tests + sqllogictest + DuckLake CI)
- **Test coverage**: Every phase needs unit tests, proptests, sqllogictest, and fuzz target consideration
- **Build**: `just build` for extension, `cargo test` for unit tests (no extension feature), `just test-sql` requires fresh build
- **Snowflake reference**: Use Snowflake semantic view behavior as guide when in doubt about SQL syntax (note: Snowflake does NOT have materializations -- this is a custom extension inspired by Cube.dev)

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| (no new dependencies) | -- | -- | All routing logic uses existing Rust stdlib (HashSet for set ops) and codebase utilities (quote_ident, quote_table_ref) |

### Supporting
No new dependencies needed. The routing engine is pure Rust logic operating on the existing `Materialization` struct and `SemanticViewDefinition`.

## Architecture Patterns

### Recommended Project Structure

```
src/
  expand/
    mod.rs               # Add `mod materialization;` declaration
    materialization.rs   # NEW: try_route_materialization() + unit tests
    sql_gen.rs           # Modify expand() to call try_route_materialization()
    test_helpers.rs      # Add with_materialization() builder method
test/sql/
  phase55_materialization_routing.test  # NEW: sqllogictest integration tests
```

### Pattern 1: Materialization Routing as Early Exit in expand()

**What:** Insert materialization routing check after name resolution (steps 2-3) and metric type classification, but before SQL generation (step 5). If a match is found and all metrics are safe for routing, return the materialized SQL immediately.
**When to use:** This exact location in the expand() control flow.
**Example:**
```rust
// Source: [expand/sql_gen.rs lines 267-559, analyzed for insertion point]

// In expand(), after step 3 (resolve metrics) and before step 4:

// Phase 55: Materialization routing.
// Check BEFORE semi-additive / window classification so those paths
// short-circuit routing (MAT-04 requirement).
if let Some(routed_sql) = super::materialization::try_route_materialization(
    def,
    &resolved_dims,
    &resolved_mets,
) {
    return Ok(routed_sql);
}

// Existing code continues unchanged from here...
```

**Critical design detail:** The routing check must happen AFTER metric resolution but BEFORE the semi-additive and window dispatch. However, the routing function itself must check for semi-additive and window metrics and refuse to route them. This way, if any requested metric is semi-additive or window-based, routing returns `None` and the existing expansion paths handle them.

### Pattern 2: Pure Matching Function

**What:** `try_route_materialization()` is a pure function taking definition + resolved names, returning `Option<String>`.
**When to use:** For the routing decision.
**Example:**
```rust
// Source: [design doc lines 119-150, adapted for v0.7.0 exact-match-only scope]
use std::collections::HashSet;

use crate::model::{Dimension, Metric, SemanticViewDefinition};
use super::resolution::{quote_ident, quote_table_ref};

/// Attempt to route a query to a materialization table.
///
/// Returns `Some(sql)` if an exact-match materialization is found,
/// `None` if no match or if routing is excluded (semi-additive/window metrics).
///
/// # Matching rules (v0.7.0 -- exact match only)
///
/// A materialization matches when:
/// 1. Its dimension set EXACTLY equals the requested dimension set (case-insensitive)
/// 2. Its metric set EXACTLY equals the requested metric set (case-insensitive)
/// 3. No requested metric has `non_additive_by` (semi-additive exclusion)
/// 4. No requested metric has `window_spec` (window function exclusion)
///
/// Re-aggregation routing (materialization covers a SUPERSET of requested dims)
/// is deferred to v2 (MAT-F01).
pub(crate) fn try_route_materialization(
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> Option<String> {
    // Fast path: no materializations declared -> None
    if def.materializations.is_empty() {
        return None;
    }

    // MAT-04: Exclude semi-additive and window metrics from routing
    if resolved_mets.iter().any(|m| !m.non_additive_by.is_empty()) {
        return None;
    }
    if resolved_mets.iter().any(|m| m.is_window()) {
        return None;
    }

    // Build requested dimension/metric name sets (lowercase for case-insensitive matching)
    let req_dims: HashSet<String> = resolved_dims
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();
    let req_mets: HashSet<String> = resolved_mets
        .iter()
        .map(|m| m.name.to_ascii_lowercase())
        .collect();

    // Scan materializations in definition order (first match wins)
    for mat in &def.materializations {
        let mat_dims: HashSet<String> = mat.dimensions
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        let mat_mets: HashSet<String> = mat.metrics
            .iter()
            .map(|m| m.to_ascii_lowercase())
            .collect();

        if mat_dims == req_dims && mat_mets == req_mets {
            return Some(build_materialized_sql(
                &mat.table,
                resolved_dims,
                resolved_mets,
            ));
        }
    }

    None
}

/// Generate a SELECT from the materialization table.
///
/// The materialization table is expected to have columns named after the
/// dimension and metric names. The SQL simply selects them by name.
fn build_materialized_sql(
    table: &str,
    dims: &[&Dimension],
    mets: &[&Metric],
) -> String {
    let mut sql = String::with_capacity(128);
    sql.push_str("SELECT\n");

    let mut items: Vec<String> = Vec::new();
    for dim in dims {
        let col = quote_ident(&dim.name);
        // Apply output_type cast if declared
        if let Some(ref type_str) = dim.output_type {
            items.push(format!("    CAST({col} AS {type_str}) AS {col}"));
        } else {
            items.push(format!("    {col}"));
        }
    }
    for met in mets {
        let col = quote_ident(&met.name);
        if let Some(ref type_str) = met.output_type {
            items.push(format!("    CAST({col} AS {type_str}) AS {col}"));
        } else {
            items.push(format!("    {col}"));
        }
    }
    sql.push_str(&items.join(",\n"));
    sql.push_str("\nFROM ");
    sql.push_str(&quote_table_ref(table));

    sql
}
```

### Pattern 3: Test Fixture Extension

**What:** Add `with_materialization()` to the `TestFixtureExt` trait.
**When to use:** For all routing unit tests.
**Example:**
```rust
// Source: [follows existing TestFixtureExt pattern in test_helpers.rs]
fn with_materialization(
    self,
    name: &str,
    table: &str,
    dimensions: &[&str],
    metrics: &[&str],
) -> Self;

// Implementation:
fn with_materialization(
    mut self,
    name: &str,
    table: &str,
    dimensions: &[&str],
    metrics: &[&str],
) -> Self {
    self.materializations.push(Materialization {
        name: name.to_string(),
        table: table.to_string(),
        dimensions: dimensions.iter().map(|s| s.to_string()).collect(),
        metrics: metrics.iter().map(|s| s.to_string()).collect(),
    });
    self
}
```

### Anti-Patterns to Avoid

- **Routing semi-additive or window metrics to materializations:** These metrics require CTE-based expansion with ROW_NUMBER or window functions. A pre-aggregated table cannot replicate this logic. The routing function must refuse to match when any requested metric has `non_additive_by` or `window_spec`. [VERIFIED: requirements MAT-04, design doc line 241]
- **Re-aggregation routing in v0.7.0:** The design doc describes re-aggregation (materializations with superset dimensions where the query does `GROUP BY` on a subset). This is explicitly deferred to v2 (MAT-F01 in REQUIREMENTS.md). v0.7.0 is exact-match only. [VERIFIED: REQUIREMENTS.md lines 44-45]
- **Validating materialization table existence at routing time:** The table may not exist (created later by dbt). Let DuckDB raise the error naturally when it tries to execute the SQL. Adding a pre-check would require a database query during expansion, breaking the pure-function design. [VERIFIED: Phase 54 decision -- STATE.md blocker about "define-time vs query-time validation TBD"]
- **Modifying the routing function to have side effects:** `try_route_materialization()` must be a pure function (no DB access, no logging, no mutation). Side effects for explain/introspection are Phase 57 concerns.
- **Routing fact queries:** The `expand_facts()` path handles pre-aggregation fact queries (row-level, no GROUP BY). Materializations are aggregation-level artifacts and should NOT apply to fact queries. The dispatch to `expand_facts()` happens before routing would be called (line 288 in sql_gen.rs), so this is handled naturally by control flow.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Case-insensitive set comparison | Custom string matching loops | `HashSet<String>` with `to_ascii_lowercase()` | Standard Rust pattern, already used throughout codebase (e.g., `queried_dim_names` in sql_gen.rs line 387-390) [VERIFIED: codebase] |
| Table name quoting | Manual dot-splitting and quoting | `quote_table_ref()` from `expand/resolution.rs` | Handles multi-part names (catalog.schema.table), already used in all FROM clauses [VERIFIED: resolution.rs line 29-35] |
| Column name quoting | Manual escaping | `quote_ident()` from `expand/resolution.rs` | Standard identifier quoting used everywhere [VERIFIED: resolution.rs line 1-27] |
| Test fixture setup | Manual struct construction per test | `TestFixtureExt::with_materialization()` builder | Follows established pattern -- 12 builder methods already exist [VERIFIED: test_helpers.rs] |

**Key insight:** The routing engine is intentionally simple for v0.7.0 -- exact set equality, no subset matching, no re-aggregation. This means the implementation is ~80 lines of logic plus ~200 lines of tests, all using existing utilities.

## Common Pitfalls

### Pitfall 1: Insertion Point in expand() Control Flow
**What goes wrong:** Placing the routing check at the wrong point in `expand()` causes either (a) routing semi-additive/window queries that should fall back, or (b) duplicating name resolution logic.
**Why it happens:** `expand()` has a specific pipeline: validation -> name resolution -> metric type classification -> semi-additive dispatch -> window dispatch -> standard SQL gen. The routing check must interact correctly with all branches.
**How to avoid:** Place the routing check AFTER step 3 (metric resolution, line 356) and BEFORE step 4 (derived metric inlining, line 360). The routing function itself handles the semi-additive/window exclusion checks, so even if placed before those dispatches, it returns `None` for excluded metrics.
**Warning signs:** Semi-additive or window metric queries return wrong results (reading from materialization instead of CTE expansion).

### Pitfall 2: Case Sensitivity Mismatch
**What goes wrong:** Materialization dimension/metric names from DDL are stored as-typed by the user. Requested dimension/metric names from the query are also as-typed. If cases don't match (e.g., "Region" in DDL vs "region" in query), the set comparison fails.
**Why it happens:** The codebase uses case-insensitive matching everywhere (dimension/metric resolution via `eq_ignore_ascii_case`), but naive `HashSet<String>` comparison is case-sensitive.
**How to avoid:** Lowercase all names before building HashSets: `d.name.to_ascii_lowercase()` for both sides.
**Warning signs:** Queries that should match a materialization don't, and case differences are the only difference.

### Pitfall 3: Output Type Casts in Materialized SQL
**What goes wrong:** Dimensions and metrics may have `output_type` set. The standard expansion pipeline wraps expressions in `CAST(expr AS type)`. If the materialized SQL omits these casts, the output types differ between routed and non-routed queries.
**Why it happens:** Forgetting that `output_type` is an attribute of the semantic view definition, not just the SQL expression.
**How to avoid:** In `build_materialized_sql()`, check each dimension/metric's `output_type` and emit `CAST("col" AS type) AS "col"` when set.
**Warning signs:** Type mismatch errors or different column types between materialized and raw expansion for the same query.

### Pitfall 4: Derived Metrics in Materialization Coverage
**What goes wrong:** A materialization declares `metrics: ['revenue']` where `revenue` is a derived metric (e.g., `revenue = base_revenue * exchange_rate`). The routing function matches by name only, which is correct -- the materialization table's `revenue` column should already contain the fully computed value.
**Why it happens:** Temptation to resolve derived metric expressions before matching.
**How to avoid:** Match by name only, not by expression. The materialization table is expected to have pre-computed values. Name matching after resolution (step 3) ensures the metric exists and is accessible.
**Warning signs:** None expected -- name-based matching is inherently correct for pre-computed tables.

### Pitfall 5: Fact Queries Accidentally Routed
**What goes wrong:** A fact query (`facts := [...]`) gets matched to a materialization, producing incorrect aggregated results for what should be a row-level query.
**Why it happens:** Routing check placed before the facts dispatch (line 287-289).
**How to avoid:** The natural control flow handles this -- `expand_facts()` is called and returned at line 288, before routing would be reached. Alternatively, `try_route_materialization()` can explicitly return `None` when `resolved_mets` is empty (facts path doesn't resolve metrics).
**Warning signs:** Fact queries against views with materializations return aggregated data instead of row-level data.

### Pitfall 6: Dimensions-Only or Metrics-Only Queries
**What goes wrong:** A query with only dimensions (no metrics) or only metrics (no dimensions) may or may not match a materialization depending on how the matching logic handles empty sets.
**Why it happens:** A materialization with `dimensions: ['region'], metrics: ['revenue']` should NOT match a dimensions-only query for `['region']` because the metric set comparison would fail (`{} != {revenue}`).
**How to avoid:** Exact set equality (`==` on HashSets) naturally handles this -- an empty set never equals a non-empty set. A materialization with metrics-only (empty dimensions) would match a metrics-only query if the metric sets match, which is correct (it's a pre-computed global aggregate table).
**Warning signs:** Dimensions-only queries unexpectedly routing to materializations, or metrics-only queries failing to route.

## Code Examples

### Integration Point in expand()
```rust
// Source: [expand/sql_gen.rs line 356-360, modified for Phase 55]
// After step 3 (resolve metrics, line 356) and before step 4 (toposort_facts):

    // Phase 55: Materialization routing.
    // Attempt to route to a pre-aggregated table if an exact match exists.
    // Returns None if no match, or if any metric is semi-additive / window.
    if let Some(routed_sql) = super::materialization::try_route_materialization(
        def,
        &resolved_dims,
        &resolved_mets,
    ) {
        return Ok(routed_sql);
    }

    // 4. Pre-compute all metric expressions... (existing code unchanged)
```

### Materialized SQL Output Example
```sql
-- Query: semantic_view('v', dimensions := ['region'], metrics := ['revenue'])
-- Materialization match: daily_rev covers region + revenue

SELECT
    "region",
    "revenue"
FROM "analytics"."agg"."daily_revenue_agg"
```

### No-Match Fallback (Standard Expansion)
```sql
-- Query: semantic_view('v', dimensions := ['region', 'status'], metrics := ['revenue'])
-- No materialization covers region + status + revenue -> standard expansion

SELECT
    region AS "region",
    status AS "status",
    SUM(amount) AS "revenue"
FROM "orders"
GROUP BY
    1,
    2
```

### Semi-Additive Exclusion
```rust
// Source: [model.rs lines 186-203 -- non_additive_by and is_window() checks]
// In try_route_materialization():

// MAT-04: Exclude semi-additive and window function metrics
if resolved_mets.iter().any(|m| !m.non_additive_by.is_empty()) {
    return None;
}
if resolved_mets.iter().any(|m| m.is_window()) {
    return None;
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No materialization support | Materialization data model (Phase 54) | v0.7.0 Phase 54 | Definition only, no query-time effect |
| All queries expand from raw sources | Exact-match routing to materializations (this phase) | v0.7.0 Phase 55 | Transparent acceleration when materializations match |
| Cube.dev: subset matching + re-aggregation | v0.7.0: exact match only | v0.7.0 decision | Re-aggregation deferred to v2 (MAT-F01) |

**Design note:** The exact-match-only approach is deliberately conservative. It avoids correctness risks from re-aggregation of non-additive metrics (e.g., you cannot re-aggregate `AVG` by summing -- you need `SUM` and `COUNT` separately). The v2 re-aggregation feature (MAT-F01) will require additivity classification on the Metric struct (MAT-F02). [VERIFIED: REQUIREMENTS.md lines 44-45]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Materialization table columns are named identically to the semantic view's dimension/metric names | Pattern 2 (build_materialized_sql) | Medium -- if column names in the mat table differ from dim/metric names, the generated SELECT will fail. Users must ensure name alignment. This is the standard convention for pre-aggregated tables. |
| A2 | First matching materialization wins (definition order) | Pattern 2 (scan loop) | Low -- consistent with Cube.dev algorithm and the design doc (line 33: "Scan pre-aggregations in definition order"). No ambiguity for exact-match since at most one can match exactly. |
| A3 | Metrics-only materializations (empty dimensions) are valid and should match metrics-only queries | Pitfall 6 | Low -- a metrics-only materialization represents a pre-computed global aggregate table, which is a valid use case |
| A4 | output_type casts should be applied even when reading from materialization table | Pitfall 3 | Low -- maintaining type consistency is important for the vtab type binding pipeline |

## Open Questions

1. **Materialization table column names**
   - What we know: The generated SQL uses `quote_ident(&dim.name)` and `quote_ident(&met.name)` to reference columns in the materialization table. This assumes the table's columns are named identically to the semantic view's dimensions/metrics.
   - What's unclear: Should we support column name mapping (mat table column != semantic name)?
   - Recommendation: For v0.7.0, require name alignment (standard convention). Add column mapping in a future version if users need it. Document this assumption.

2. **Private metrics in materializations**
   - What we know: Private metrics cannot be queried directly (expand() rejects them at step 3). A materialization that covers a private metric would never match because the metric would be rejected before routing.
   - What's unclear: Should we validate at define time that materializations don't reference private metrics?
   - Recommendation: No -- define-time validation is Phase 54's concern and is already done for existence. Runtime behavior is correct (private metrics are rejected before routing). Add a note in docs.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + proptest + sqllogictest-rs |
| Config file | justfile + Cargo.toml `[dev-dependencies]` |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MAT-02 | Exact-match routing generates SELECT from mat table | unit | `cargo test materialization` | Wave 0 |
| MAT-02 | End-to-end routing via DDL + query | sqllogictest | `just test-sql` | Wave 0 |
| MAT-03 | No-match fallback produces standard expansion | unit | `cargo test materialization` | Wave 0 |
| MAT-03 | End-to-end fallback via DDL + query | sqllogictest | `just test-sql` | Wave 0 |
| MAT-04 | Semi-additive metrics excluded from routing | unit | `cargo test materialization` | Wave 0 |
| MAT-04 | Window function metrics excluded from routing | unit | `cargo test materialization` | Wave 0 |
| MAT-05 | Empty materializations = zero behavior change | unit | `cargo test materialization` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `src/expand/materialization.rs` -- new module with `try_route_materialization()` and unit tests
- [ ] `src/expand/test_helpers.rs` -- add `with_materialization()` builder method
- [ ] `test/sql/phase55_materialization_routing.test` -- sqllogictest integration tests
- [ ] `test/sql/TEST_LIST` -- add phase55 test entry

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A (materialization table access is DuckDB's concern) |
| V5 Input Validation | yes | Materialization table names are stored as data (not interpolated raw); output uses `quote_table_ref()` and `quote_ident()` for SQL-safe quoting |
| V6 Cryptography | no | N/A |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via materialization table name | Tampering | `quote_table_ref()` splits on '.' and wraps each part in `"..."` -- standard DuckDB identifier quoting. The table name comes from the stored definition (JSON in catalog), not from user query input at routing time. |
| Routing to unauthorized table | Information Disclosure | DuckDB's own access control applies when executing the generated SQL. The extension does not bypass DuckDB's security model. |

## Sources

### Primary (HIGH confidence)
- `src/expand/sql_gen.rs` -- `expand()` function control flow (lines 267-559), insertion point analysis
- `src/expand/types.rs` -- QueryRequest, ExpandError types
- `src/expand/test_helpers.rs` -- TestFixtureExt builder pattern
- `src/expand/semi_additive.rs` -- semi-additive expansion path (exclusion logic)
- `src/expand/window.rs` -- window metric expansion path (exclusion logic)
- `src/expand/resolution.rs` -- `quote_ident()`, `quote_table_ref()` utilities
- `src/model.rs` -- Materialization struct (lines 205-227), Metric fields `non_additive_by` and `window_spec`
- `src/query/table_function.rs` -- vtab invocation of `expand()` (lines 486-498)
- `src/query/explain.rs` -- explain invocation of `expand()` (lines 179-191)
- Phase 54 summary and research -- materialization data model and DDL implementation
- `_notes/semantic-views-duckdb-design-doc.md` -- pre-aggregation selection algorithm (lines 119-155)
- `.planning/REQUIREMENTS.md` -- MAT-02 through MAT-05, MAT-F01 (deferred)
- `.planning/STATE.md` -- decisions on semi-additive exclusion and exact-match-only scope

### Secondary (MEDIUM confidence)
- Cube.dev pre-aggregation matching algorithm -- design doc reference, adapted for exact-match-only scope [CITED: design doc lines 29-41]

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all utilities exist in codebase
- Architecture: HIGH -- single insertion point clearly identified in expand(), pure function design matches design doc
- Pitfalls: HIGH -- documented from direct control flow analysis of 3,956-line sql_gen.rs

**Research date:** 2026-04-19
**Valid until:** 2026-05-19 (stable -- no external dependency changes expected)
