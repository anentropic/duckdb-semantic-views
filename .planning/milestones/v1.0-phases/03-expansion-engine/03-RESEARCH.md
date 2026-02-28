# Phase 3: Expansion Engine - Research

**Researched:** 2026-02-25
**Domain:** SQL generation engine in Rust -- CTE-based query expansion with GROUP BY inference, JOIN resolution, identifier quoting, and property-based testing
**Confidence:** HIGH (core patterns are well-understood string manipulation; proptest API verified via Context7; DuckDB quoting rules verified via official docs)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Generated SQL shape:**
- CTE-wrapped structure: base query in `WITH "_base" AS (...)`, then `SELECT ... FROM "_base" GROUP BY ...`
- Human-readable formatting with indentation and newlines (user will see this via EXPLAIN in Phase 4)
- `SELECT *` in the base CTE -- DuckDB's optimizer prunes unused columns
- Fixed CTE name `_base` -- no view-name derivation needed for v0.1

**Join inclusion strategy:**
- Only include joins needed by the requested dimensions, metrics, or filters -- not all declared joins
- Add optional `source_table` field to both `Dimension` and `Metric` structs -- declares which join table the expression comes from
- Declaration-order chain resolution: user declares joins in dependency order, engine resolves transitive dependencies (if `regions` join references `customers` in its ON clause, include `customers` too)
- Filters that reference a joined table also trigger that join's inclusion

**Filter composition:**
- Multiple filter entries are AND-composed
- Each filter expression wrapped in parentheses for safety: `WHERE (filter1) AND (filter2)`
- Filters can reference any column from the base table or any declared join (all columns are in scope within the CTE)
- No filter-level OR composition -- user writes OR logic within a single filter string

**Error messages:**
- Unknown dimension/metric: show the bad name, the view name, list available names, and suggest the closest match ("Did you mean 'region'?")
- Empty dimensions array is allowed -- produces a global aggregate query (no GROUP BY)
- Empty metrics array is an error -- at least one metric required
- Duplicate dimension/metric names in a single request produce an error
- No validation of metric expressions as aggregates -- trust the definition author, let DuckDB catch issues at query time

### Claude's Discretion

- Fuzzy matching algorithm for "did you mean" suggestions (edit distance, etc.)
- Internal representation of the join dependency graph
- proptest strategy design for property-based tests
- Test dataset schemas and known-answer values

### Deferred Ideas (OUT OF SCOPE)

- Named, reusable filters (Snowflake-style with name/description/synonyms) -- future milestone
- Pre-aggregation / materialized view matching -- future milestone (PERF-FUT-01)
- Derived/ratio metrics referencing other metrics -- future milestone (MODEL-FUT-01)
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MODEL-01 | User can define named dimensions as arbitrary SQL column expressions | Model struct update: add `source_table: Option<String>` to `Dimension`; expansion engine reads `name` and `expr` from definition |
| MODEL-02 | User can define named metrics as aggregation expressions | Model struct update: add `source_table: Option<String>` to `Metric`; expansion places metrics in SELECT with their aggregate expressions |
| MODEL-03 | User can specify a base table and define explicit JOIN relationships | Join resolution algorithm resolves transitive dependencies; CTE includes only needed JOINs |
| MODEL-04 | User can define row-level filter conditions that are always applied | Filter expressions AND-composed in WHERE clause of the base CTE, each parenthesized |
| EXPAND-01 | Extension automatically generates a GROUP BY clause containing all requested dimensions | Expansion engine collects dimension expressions, emits GROUP BY with matching ordinal positions or expressions |
| EXPAND-02 | Extension infers JOIN clauses from entity relationships | Join dependency graph: walk requested dimensions/metrics, collect source_table references, resolve transitive join chains in declaration order |
| EXPAND-03 | Extension validates dimension and metric names; invalid names produce clear errors | Validation layer with strsim-based fuzzy matching for "did you mean" suggestions |
| EXPAND-04 | All generated SQL identifiers are quoted with double-quotes | `quote_ident()` utility wraps identifiers in `"..."` with internal `"` escaped to `""` |
| TEST-01 | Unit tests cover expansion engine without DuckDB runtime | Pure Rust tests: `expand()` takes `SemanticViewDefinition` + selection, returns SQL string; no `Connection` needed |
| TEST-02 | Property-based tests (proptest) verify expansion invariants | proptest with `subsequence()` strategy to generate valid dimension/metric subsets; assert GROUP BY inclusion and SQL syntax validity |
</phase_requirements>

---

## Summary

Phase 3 is a pure Rust module with zero DuckDB runtime dependency. The `expand()` function takes a `SemanticViewDefinition` (already defined in `src/model.rs`) and a query request (selected dimensions, metrics) and produces a SQL string. The generated SQL uses a CTE-wrapped structure: the base CTE contains the `FROM` clause with JOINs and `WHERE` filters, while the outer query handles `SELECT` (dimensions + metric aggregations) and `GROUP BY` (all dimensions).

The core technical challenges are: (1) join dependency resolution -- only including JOINs needed by the requested columns, and resolving transitive chains when join B depends on join A; (2) correct identifier quoting to prevent reserved-word conflicts and injection; (3) clear error messages with fuzzy "did you mean" suggestions when dimension/metric names are invalid.

This phase has no external library requirements beyond `strsim` (for edit distance) and `proptest` (for property-based testing). The SQL generation is pure string building -- no SQL builder library is warranted because the output shape is fixed (one CTE pattern), the dialect is DuckDB-specific, and a library would add complexity without meaningful correctness benefit.

**Primary recommendation:** Build `expand()` as a pure function in a new `src/expand.rs` module. Use simple string formatting (`format!`, `write!`) for SQL generation. Add `strsim` for fuzzy matching and `proptest` as a dev-dependency for property-based tests.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| (no new runtime deps) | -- | SQL generation is pure string building | The output is a single fixed CTE pattern; a SQL builder library would add dep weight and dialect-mismatch risk for no benefit |
| strsim | 0.11.1 | Edit distance for "did you mean" suggestions | 75M+ downloads; standard Rust crate for string similarity; provides Levenshtein, Jaro-Winkler, Damerau-Levenshtein |

### Supporting (dev-dependencies)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| proptest | 1.9.0 | Property-based testing | TEST-02: generate arbitrary dimension/metric subsets and verify GROUP BY invariants and SQL syntax |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| strsim | Hand-rolled Levenshtein | strsim is 0-dependency, battle-tested, covers multiple algorithms; no reason to hand-roll |
| String formatting | sea-query / sql_query_builder | SQL builder adds dialect-mismatch risk (DuckDB quirks vs generic SQL); the CTE shape is fixed and simple; string formatting is more transparent and debuggable |
| proptest | quickcheck | proptest has better shrinking, regex-based string strategies, `subsequence()` for subset generation; more ergonomic for this use case |

**Installation:**
```toml
# Cargo.toml additions
[dependencies]
strsim = "0.11"

[dev-dependencies]
proptest = "1.9"
```

---

## Architecture Patterns

### Recommended Module Structure

```
src/
  model.rs          # SemanticViewDefinition, Dimension, Metric, Join (exists -- needs source_table field)
  expand.rs         # NEW: expand() function, QueryRequest, SQL generation, quote_ident()
  catalog.rs        # (exists -- no changes)
  ddl/              # (exists -- no changes)
  lib.rs            # (exists -- add `pub mod expand;`)
```

### Pattern 1: Pure Function Expansion

**What:** `expand()` is a pure function: `(&SemanticViewDefinition, &QueryRequest) -> Result<String, ExpandError>`. No side effects, no DuckDB connection, no I/O.

**When to use:** Always. This is the only expansion pattern for v0.1.

**Example:**
```rust
/// A request to expand a semantic view into SQL.
pub struct QueryRequest {
    pub dimensions: Vec<String>,  // names of requested dimensions
    pub metrics: Vec<String>,     // names of requested metrics
}

/// Errors that can occur during expansion.
pub enum ExpandError {
    EmptyMetrics { view_name: String },
    UnknownDimension { view_name: String, name: String, available: Vec<String>, suggestion: Option<String> },
    UnknownMetric { view_name: String, name: String, available: Vec<String>, suggestion: Option<String> },
    DuplicateDimension { view_name: String, name: String },
    DuplicateMetric { view_name: String, name: String },
}

/// Expand a semantic view definition into a SQL query string.
pub fn expand(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
) -> Result<String, ExpandError> {
    // 1. Validate request (no empty metrics, no unknown names, no duplicates)
    // 2. Resolve which joins are needed
    // 3. Build base CTE: FROM + JOINs + WHERE
    // 4. Build outer SELECT: dimension exprs + metric exprs
    // 5. Build GROUP BY: all dimension expressions
    // 6. Assemble final SQL string
    todo!()
}
```

### Pattern 2: CTE-Based SQL Shape

**What:** All generated SQL follows this fixed template:
```sql
WITH "_base" AS (
    SELECT *
    FROM "base_table"
    JOIN "joined_table" ON (on_clause)
    WHERE (filter1) AND (filter2)
)
SELECT
    "dim_expr" AS "dim_name",
    metric_expr AS "metric_name"
FROM "_base"
GROUP BY
    "dim_expr"
```

**When to use:** Every expansion. The CTE isolates the source data (with joins and filters) from the aggregation layer.

**Key details:**
- Dimension expressions appear both in SELECT and GROUP BY
- Metric expressions (aggregations) appear only in SELECT
- If dimensions list is empty, omit the GROUP BY clause entirely (global aggregate)
- All identifiers (table names, aliases, column names) are double-quoted
- Metric expressions are NOT quoted as identifiers -- they are SQL expressions like `sum("amount")`; only the alias (`AS "metric_name"`) is quoted

### Pattern 3: Join Dependency Resolution

**What:** Walk the requested dimensions and metrics, collect which `source_table` values they reference, then walk the declared joins in declaration order to include the needed ones plus any transitive dependencies.

**When to use:** When the request involves columns from joined tables.

**Algorithm:**
1. Build a set `needed_tables` from dimensions/metrics that have `source_table` set
2. Also scan filter expressions to detect references to joined tables (filters can reference any join)
3. For each join in declaration order, if `join.table` is in `needed_tables`, mark it as included
4. Transitive resolution: for each included join, check if its `ON` clause references another join's table; if so, add that table to `needed_tables` and re-scan (fixed-point loop)
5. Emit included joins in their original declaration order

**Implementation note on filter-to-join resolution:** Since filter expressions are opaque SQL strings, the engine cannot reliably parse them to detect table references. Two approaches:
- **Simple (recommended for v0.1):** Filters always go in the base CTE WHERE clause after all included joins. If a filter references a table that is not included, DuckDB will produce a clear column-not-found error at query time. This avoids brittle SQL parsing.
- **Advanced (deferred):** Parse filter expressions to extract table references. Not needed for v0.1 because the user controls which filters they define.

### Pattern 4: Identifier Quoting

**What:** A `quote_ident()` utility that wraps an identifier in double quotes, escaping any embedded double quotes by doubling them.

**Example:**
```rust
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

// quote_ident("my table") => "\"my table\""
// quote_ident("col\"name") => "\"col\"\"name\""
```

**When to use:** Every table name, column name, alias, and CTE name in emitted SQL. Never use on raw SQL expressions (dimension `expr`, metric `expr`, filter strings, join `ON` clauses) -- these are user-provided SQL fragments that should be emitted as-is.

### Anti-Patterns to Avoid

- **Quoting expressions as identifiers:** Dimension `expr` values like `date_trunc('month', created_at)` must NOT be wrapped in double quotes. Only the alias (`AS "month"`) gets quoted. The `expr` is raw SQL.
- **Quoting metric aggregation expressions:** `sum("amount")` must NOT be double-quoted as a whole. Only the alias (`AS "total_revenue"`) gets quoted.
- **Including all declared joins:** If the user requests only dimensions/metrics from the base table, no joins should appear in the CTE. Including unnecessary joins risks fan-out and performance degradation.
- **Parsing user SQL expressions:** Do not attempt to parse or validate dimension/metric/filter SQL expressions. They are opaque strings; DuckDB validates them at execution time.
- **Using GROUP BY ordinals:** Use explicit expressions in GROUP BY, not ordinal positions. Ordinals are fragile if the SELECT column order changes.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| String similarity / edit distance | Custom Levenshtein implementation | `strsim` crate (0.11.1) | Battle-tested, zero dependencies, covers multiple algorithms; Levenshtein is deceptively tricky to get right for Unicode |
| Property-based test generation | Manual random generation in tests | `proptest` crate (1.9.0) | Automatic shrinking, `subsequence()` for subset generation, regex strategies for strings; hand-rolled random tests miss edge cases and don't shrink |
| SQL escaping for identifiers | Ad-hoc string replacement | Centralized `quote_ident()` function | Must be correct and consistent everywhere; a single function ensures one source of truth for the quoting rule |

**Key insight:** The expansion engine IS hand-rolled SQL generation -- and that is correct for this use case. The output is a single fixed CTE pattern for one specific dialect (DuckDB). SQL builder libraries abstract over multiple dialects and query shapes, which adds complexity without benefit here. The identifier quoting, however, must be centralized (not hand-rolled at each call site).

---

## Common Pitfalls

### Pitfall 1: Fan-Out Join Multiplication
**What goes wrong:** When a one-to-many join is included, aggregate metrics from the "one" side get multiplied by the number of matching rows on the "many" side. Example: joining `orders` to `line_items` and computing `count(*)` counts line items, not orders.
**Why it happens:** The CTE materializes the joined result set before aggregation. If the join fans out, aggregates see duplicated rows.
**How to avoid:** For v0.1, this is explicitly out of scope -- the user is responsible for defining correct expressions (e.g., `count(DISTINCT order_id)` instead of `count(*)`). Document this limitation. MetricFlow and Cube solve this with grain-locked measures, which is a future milestone.
**Warning signs:** Test results where `count(*)` or `sum(amount)` return unexpectedly large values.

### Pitfall 2: Quoting Expressions vs Identifiers
**What goes wrong:** The engine double-quotes a dimension expression like `date_trunc('month', created_at)`, turning it into `"date_trunc('month', created_at)"` which DuckDB interprets as a column name, not a function call. Query fails with "column not found."
**Why it happens:** Mixing up "things that are identifiers" (table names, column aliases) with "things that are SQL expressions" (dimension expr, metric expr, filter strings, join ON clauses).
**How to avoid:** Strict rule: `quote_ident()` is called ONLY on identifiers (names used for table references and aliases). Expressions from the definition JSON are emitted verbatim.
**Warning signs:** Queries fail with "column not found" errors for expressions that contain function calls or operators.

### Pitfall 3: GROUP BY Mismatch
**What goes wrong:** The GROUP BY clause does not exactly match the non-aggregate expressions in SELECT, causing DuckDB to reject the query.
**Why it happens:** Dimension expressions appear in SELECT but a different form appears in GROUP BY (e.g., SELECT uses the alias but GROUP BY uses the raw expression, or vice versa).
**How to avoid:** Use the same dimension expression string in both SELECT and GROUP BY. DuckDB allows GROUP BY to reference SELECT aliases, but using the raw expression in both locations is more reliable and explicit.
**Warning signs:** DuckDB error "column must appear in the GROUP BY clause or be used in an aggregate function."

### Pitfall 4: Transitive Join Dependencies Not Resolved
**What goes wrong:** The user requests a dimension from table C, which is joined to table B, which is joined to table A. The engine includes the C join but not the B join (which C depends on), producing a SQL error.
**Why it happens:** The dependency resolver only looks at direct `source_table` references, not the transitive chain.
**How to avoid:** Fixed-point resolution: after marking directly needed joins, check each included join's ON clause for references to other join tables. Repeat until no new joins are added. Since joins are declared in dependency order, this converges quickly.
**Warning signs:** "table not found" errors when querying dimensions from deeply joined tables.

### Pitfall 5: serde deny_unknown_fields Blocks Model Evolution
**What goes wrong:** Adding `source_table: Option<String>` to `Dimension` and `Metric` is safe for new definitions, but existing definitions stored in the catalog (from Phase 2) do not have this field. If deserialization is strict, old definitions break.
**Why it happens:** `#[serde(deny_unknown_fields)]` is on `SemanticViewDefinition`, but the new field is on child structs. The `Option<String>` with `#[serde(default)]` handles missing fields correctly -- serde treats absent optional fields as `None`. This is NOT a problem as long as `#[serde(default)]` is used on the new field.
**How to avoid:** Use `#[serde(default)]` on `source_table` fields. Verify with a test that old JSON (without `source_table`) still parses correctly.
**Warning signs:** Existing semantic view definitions from Phase 2 tests fail to deserialize after model changes.

### Pitfall 6: DuckDB Identifiers Are Case-Insensitive
**What goes wrong:** DuckDB treats all identifiers (quoted and unquoted) as case-insensitive, unlike PostgreSQL where quoted identifiers preserve case. A user defining dimension name `"Region"` and requesting `"region"` should match.
**Why it happens:** DuckDB preserves the original casing for display but resolves identifiers case-insensitively.
**How to avoid:** Perform case-insensitive comparison when matching requested dimension/metric names to definition names. Use `.eq_ignore_ascii_case()` or normalize to lowercase for lookups.
**Warning signs:** "Unknown dimension 'region'" when the definition has `"Region"`.

---

## Code Examples

### Example 1: quote_ident Utility

```rust
/// Double-quote a SQL identifier, escaping embedded double quotes.
///
/// DuckDB uses `"` for identifier quoting. Internal `"` must be escaped
/// as `""` per SQL standard.
fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_identifier() {
        assert_eq!(quote_ident("orders"), "\"orders\"");
    }

    #[test]
    fn reserved_word() {
        assert_eq!(quote_ident("select"), "\"select\"");
    }

    #[test]
    fn embedded_double_quote() {
        assert_eq!(quote_ident("col\"name"), "\"col\"\"name\"");
    }

    #[test]
    fn identifier_with_spaces() {
        assert_eq!(quote_ident("my table"), "\"my table\"");
    }
}
```

### Example 2: Fuzzy Match with strsim

```rust
use strsim::levenshtein;

/// Find the closest match for `name` in `available`, returning it if
/// the edit distance is <= 3 (reasonable threshold for typo suggestions).
fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    let name_lower = name.to_lowercase();
    available
        .iter()
        .map(|a| (a, levenshtein(&name_lower, &a.to_lowercase())))
        .filter(|(_, dist)| *dist <= 3)
        .min_by_key(|(_, dist)| *dist)
        .map(|(a, _)| a.clone())
}
```

### Example 3: Expansion Output (Known-Answer)

Given this definition:
```json
{
    "base_table": "orders",
    "dimensions": [
        {"name": "region", "expr": "region"},
        {"name": "month", "expr": "date_trunc('month', created_at)"}
    ],
    "metrics": [
        {"name": "total_revenue", "expr": "sum(amount)"},
        {"name": "order_count", "expr": "count(*)"}
    ],
    "joins": [
        {"table": "customers", "on": "orders.customer_id = customers.id"}
    ],
    "filters": ["status = 'completed'"]
}
```

Requesting `dimensions: ["region"]`, `metrics: ["total_revenue"]` should produce:
```sql
WITH "_base" AS (
    SELECT *
    FROM "orders"
    WHERE ("status" = 'completed')
)
SELECT
    region AS "region",
    sum(amount) AS "total_revenue"
FROM "_base"
GROUP BY
    region
```

Note: the `customers` join is NOT included because neither `region` nor `total_revenue` has `source_table: "customers"`.

Requesting `dimensions: []`, `metrics: ["total_revenue"]` should produce a global aggregate (no GROUP BY):
```sql
WITH "_base" AS (
    SELECT *
    FROM "orders"
    WHERE ("status" = 'completed')
)
SELECT
    sum(amount) AS "total_revenue"
FROM "_base"
```

### Example 4: proptest Strategy for Subset Generation

```rust
use proptest::prelude::*;
use proptest::sample::subsequence;

/// Generate a valid QueryRequest from a definition:
/// - Pick a non-empty subset of metric names
/// - Pick any subset (possibly empty) of dimension names
fn arb_query_request(def: &SemanticViewDefinition) -> impl Strategy<Value = QueryRequest> {
    let dim_names: Vec<String> = def.dimensions.iter().map(|d| d.name.clone()).collect();
    let met_names: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();

    let dims_strategy = subsequence(dim_names, 0..=def.dimensions.len());
    // At least 1 metric required
    let mets_strategy = subsequence(met_names, 1..=def.metrics.len());

    (dims_strategy, mets_strategy).prop_map(|(dimensions, metrics)| {
        QueryRequest { dimensions, metrics }
    })
}
```

### Example 5: proptest Invariant -- All Dimensions in GROUP BY

```rust
proptest! {
    #[test]
    fn all_dimensions_appear_in_group_by(
        req in arb_query_request(&sample_definition())
    ) {
        let sql = expand("test_view", &sample_definition(), &req).unwrap();

        if req.dimensions.is_empty() {
            // Global aggregate -- no GROUP BY
            prop_assert!(!sql.contains("GROUP BY"));
        } else {
            // Every requested dimension's expression must appear in GROUP BY
            for dim_name in &req.dimensions {
                let dim = sample_definition().dimensions.iter()
                    .find(|d| d.name == *dim_name).unwrap();
                prop_assert!(
                    sql.contains(&format!("GROUP BY")),
                    "Missing GROUP BY clause"
                );
                // The dimension expression should appear after GROUP BY
                let group_by_section = sql.split("GROUP BY").nth(1).unwrap();
                prop_assert!(
                    group_by_section.contains(&dim.expr),
                    "Dimension expr '{}' not in GROUP BY section",
                    dim.expr
                );
            }
        }
    }
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Hand-written GROUP BY + JOIN | Semantic layer auto-expansion (MetricFlow, Cube, Snowflake) | 2022-2025 | Users declare intent, engine generates correct SQL |
| Single-table aggregation only | Multi-table join inference from entity relationships | 2023 (MetricFlow open-source) | Joins are resolved from relationship declarations, not user SQL |
| Global filter application | Conditional, row-level filters composed with AND | Standard in all semantic layers | Filters are declarative, always applied, user cannot bypass |

**Industry context:** MetricFlow (dbt) open-sourced its SQL expansion engine in 2024, validating the approach of "declare relationships, generate SQL." Our v0.1 implements a simpler version of this pattern -- single-hop joins, no grain-locking, no derived metrics. This is appropriate for the use case (personal DuckDB + Iceberg).

---

## Open Questions

1. **Filter-to-join resolution for v0.1**
   - What we know: Filters are opaque SQL strings. Detecting which joins they reference requires SQL parsing.
   - What's unclear: Whether users will write filters that reference joined tables in practice.
   - Recommendation: For v0.1, skip filter-to-join resolution. All declared joins needed by dimensions/metrics are included; filters run in the CTE WHERE clause after all included joins. If a filter references an un-included join table, DuckDB will error clearly. Document this behavior.

2. **Expression quoting in the base CTE WHERE clause**
   - What we know: Filter expressions are user-provided SQL fragments emitted verbatim. The user is responsible for quoting their own identifiers within filter expressions.
   - What's unclear: Whether the filter expression `status = 'completed'` should have `status` auto-quoted to `"status"`.
   - Recommendation: Do NOT auto-quote anything inside filter expressions, dimension expressions, or metric expressions. These are user-authored SQL. Only quote identifiers that the engine itself generates (table names, aliases, CTE names).

3. **Case sensitivity in name matching**
   - What we know: DuckDB identifiers are case-insensitive. Definition names like `"Region"` should match request names like `"region"`.
   - What's unclear: Whether this extends to "did you mean" suggestions.
   - Recommendation: Use case-insensitive matching for name lookups. Apply the same normalization to fuzzy match suggestions.

---

## Sources

### Primary (HIGH confidence)
- [DuckDB Keywords and Identifiers](https://duckdb.org/docs/stable/sql/dialect/keywords_and_identifiers) -- quoting rules, case sensitivity, reserved words
- [Proptest Book (Context7: /websites/altsysrq_github_io_proptest-book)](https://altsysrq.github.io/proptest-book/print) -- prop_compose!, proptest!, strategies, subsequence
- [proptest::sample module (docs.rs)](https://docs.rs/proptest/latest/proptest/sample/) -- Subsequence, Select, Selector strategies
- [strsim-rs (GitHub)](https://github.com/rapidfuzz/strsim-rs) -- v0.11.1, Levenshtein + Jaro-Winkler + Damerau-Levenshtein

### Secondary (MEDIUM confidence)
- [MetricFlow architecture (dbt docs)](https://docs.getdbt.com/docs/build/about-metricflow) -- semantic model patterns, join inference
- [Semantic Layer Architectures 2025 (typedef.ai)](https://www.typedef.ai/resources/semantic-layer-architectures-explained-warehouse-native-vs-dbt-vs-cube) -- industry comparison of expansion approaches
- [Fan-out join problem (Holistics docs)](https://docs.holistics.io/docs/faqs/fan-out-issue) -- aggregation consistency in semantic layers

### Tertiary (LOW confidence)
- [Aggregation Consistency Errors in Semantic Layers (arxiv)](https://arxiv.org/html/2307.00417) -- academic treatment of fan-out; validates our decision to defer grain-locking

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- pure string manipulation, strsim and proptest are well-established crates with verified APIs
- Architecture: HIGH -- CTE pattern is simple and well-understood; user decisions from CONTEXT.md are clear and specific
- Pitfalls: HIGH -- fan-out, quoting, GROUP BY mismatch are well-documented problems in semantic layer literature
- Testing: HIGH -- proptest::sample::subsequence is verified for subset generation; invariants are straightforward

**Research date:** 2026-02-25
**Valid until:** 2026-03-25 (stable domain; proptest and strsim are mature crates)
