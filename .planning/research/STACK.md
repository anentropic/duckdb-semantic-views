# Technology Stack: v0.5.3 Advanced Semantic Features

**Project:** DuckDB Semantic Views Extension
**Researched:** 2026-03-14
**Milestone:** v0.5.3 -- FACTS clause, derived metrics, hierarchies, fan trap detection, role-playing dimensions, semi-additive metrics (NON ADDITIVE BY), multiple join paths (USING RELATIONSHIPS)
**Scope:** What library/crate additions or changes are needed for the new features. Existing validated stack (duckdb-rs 1.4.4, serde_json, strsim, proptest, cc) is NOT re-evaluated.

---

## Bottom Line Up Front

**Zero new Cargo dependencies.** All seven v0.5.3 features are implementable by extending the existing codebase -- the body parser (`body_parser.rs`), model (`model.rs`), graph validator (`graph.rs`), and expansion engine (`expand.rs`). No external crate provides meaningful value that the existing code cannot deliver at the scale of this extension's data structures.

The features fall into three categories:

1. **Parser extensions** (FACTS clause, derived metrics, NON ADDITIVE BY, USING RELATIONSHIPS, hierarchies) -- extend `body_parser.rs` clause vocabulary and `model.rs` struct fields. Same depth-0 comma splitting, same bracket tracking, same serde pattern.

2. **Graph relaxation** (role-playing dimensions, multiple join paths) -- modify `graph.rs` to allow multiple edges between node pairs (relax diamond rejection), add relationship name tracking to disambiguate at expansion time.

3. **Expansion engine changes** (derived metrics, fan trap detection/warning, semi-additive SQL generation, USING RELATIONSHIPS path selection) -- extend `expand.rs` with metric dependency resolution (topological sort on metric DAG), fan trap heuristic detection (warning-only, not blocking), and window function SQL generation for NON ADDITIVE BY.

---

## Existing Dependency Inventory (Unchanged)

| Crate | Version | Sufficient For v0.5.3 | Why |
|-------|---------|----------------------|-----|
| `duckdb` | `=1.4.4` | Yes | VTab, BindInfo -- same DDL/query pipeline |
| `libduckdb-sys` | `=1.4.4` | Yes | Raw FFI unchanged |
| `serde` + `serde_json` | `1` | Yes | New model fields (facts, non_additive_by, using_relationships, hierarchies) use `#[serde(default)]` -- same pattern as v0.5.2 |
| `strsim` | `0.11` | Yes | "Did you mean" suggestions for new clause keywords and relationship names |
| `cc` | `1` (build-dep, optional) | Yes | C++ shim unchanged -- no new FFI entry points |
| `proptest` | `1.9` (dev-dep) | Yes | PBTs for new parser clauses and expansion logic |
| `cargo-husky` | `1` (dev-dep) | Yes | Pre-commit hooks unchanged |
| `arbitrary` | `1` (optional) | Yes | Derive `Arbitrary` for new model types for fuzz targets |

---

## Feature-by-Feature Stack Analysis

### 1. FACTS Clause

**What it is:** Named row-level sub-expressions scoped to a table alias. Facts are referenced by metrics and other facts but are NOT aggregates themselves. Example: `FACTS (li.net_price AS l_extendedprice * (1 - l_discount))` lets a metric say `SUM(li.net_price)` instead of repeating the expression.

**What already exists:**
- `model.rs`: `Fact { name, expr, source_table }` struct is already defined and serialized
- `model.rs`: `SemanticViewDefinition.facts: Vec<Fact>` field exists with `#[serde(default)]`
- `body_parser.rs`: `CLAUSE_KEYWORDS` currently `["tables", "relationships", "dimensions", "metrics"]`

**What v0.5.3 adds:**

| Component | Change | Complexity |
|-----------|--------|------------|
| `body_parser.rs` | Add `"facts"` to `CLAUSE_KEYWORDS` and `CLAUSE_ORDER` (between RELATIONSHIPS and DIMENSIONS). Parse `alias.name AS expr` entries -- identical pattern to DIMENSIONS parsing. | Low (~30 lines) |
| `expand.rs` | Before building the SELECT, substitute fact references in metric expressions. For each metric expr, replace `alias.fact_name` with the fact's underlying expr (in parentheses for safety). This is string substitution, not expression parsing. | Medium (~40 lines) |
| `graph.rs` | Add `check_source_tables_reachable` validation for facts (currently only checks dims/metrics). | Low (~10 lines) |

**Stack requirement:** None new. The `Fact` model type exists. Parsing uses the same `split_at_depth0_commas` + `alias.name AS expr` pattern as dimensions. Expression substitution is a string operation.

**Snowflake reference (verified):**
```sql
FACTS (
  line_items.discounted_price AS l_extendedprice * (1 - l_discount)
)
METRICS (
  orders.total_revenue AS SUM(line_items.discounted_price)
)
```

**Confidence:** HIGH -- model type exists, parser pattern exists, only wiring needed.

### 2. Derived Metrics (Metric Referencing Other Metrics)

**What it is:** A metric whose expression references other metrics rather than raw column aggregates. Derived metrics have no `source_table` (they are scoped to the semantic view, not a logical table). Example: `profit AS orders.revenue - orders.cost`.

**What already exists:**
- `model.rs`: `Metric { name, expr, source_table, output_type }` -- `source_table` is already `Option<String>`, so `None` naturally represents a view-scoped derived metric.
- `expand.rs`: metrics are expanded into SELECT expressions in `expand()`.

**What v0.5.3 adds:**

| Component | Change | Complexity |
|-----------|--------|------------|
| `body_parser.rs` | Detect metrics without `alias.` prefix as derived metrics (set `source_table: None`). Already natural -- the parser splits on `.` to get alias. No dot = no alias = derived. | Low (~5 lines) |
| `expand.rs` | **Metric dependency resolution.** Before building the SELECT, topologically sort metrics: derived metrics depend on base metrics. Replace metric references in derived metric expressions with the base metric's SQL expression. If a derived metric references `orders.revenue`, substitute with `SUM(o.amount)`. | Medium-High (~80 lines) |
| `expand.rs` | **Cycle detection in metric DAG.** If `profit` references `revenue` which references `profit`, report an error. Use same Kahn's algorithm pattern as `graph.rs` toposort. | Low (~30 lines) |
| `model.rs` | No changes needed -- `source_table: None` already represents "view-scoped". | None |

**Key design decision: expression substitution vs. SQL subquery.**

Two approaches exist for derived metrics:

| Approach | Pros | Cons |
|----------|------|------|
| **A: Expression inlining** -- replace `orders.revenue` in derived metric expr with `SUM(o.amount)` | Simple string substitution; single-pass SQL; works with DuckDB's SQL engine | Derived metric referencing another derived metric requires multi-pass substitution; nested aggregates (SUM of SUM) are invalid SQL |
| **B: CTE layering** -- base metrics in inner CTE, derived metrics in outer SELECT | Clean SQL separation; no nested aggregate problem | More complex SQL generation; requires tracking which metrics are base vs. derived |

**Recommendation: Approach B (CTE layering)** because DuckDB (like all SQL engines) rejects nested aggregates (`SUM(SUM(x))` is an error). When `profit = revenue - cost` and `revenue = SUM(amount)`, inlining produces `SUM(amount) - SUM(cost)` which works. But when `margin = profit / revenue`, inlining produces `(SUM(amount) - SUM(cost)) / SUM(amount)` -- this also works at this depth. However, any metric referencing an already-derived metric that itself references aggregates would require careful parenthesization.

Actually, reconsider: Snowflake's documentation states that a derived metric references other metrics by name, and the system replaces the reference with the metric's aggregate expression. Since each base metric is a single aggregate, and derived metrics combine them with scalar operators (+, -, *, /), the resulting expression at depth-1 derivation is always valid SQL: `SUM(a) - SUM(b)`. At depth-2: `(SUM(a) - SUM(b)) / SUM(a)` -- still valid. The key insight is that **aggregate functions are never nested** because derived metrics reference the output of other metrics (which are already aggregates), not their input.

**Revised recommendation: Approach A (expression inlining)** with parenthesized substitution. Each metric reference `metric_name` is replaced with `(metric_expression)`. This produces valid SQL at any derivation depth because:
- Base metric: `SUM(o.amount)` -- an aggregate
- Derived depth 1: `(SUM(o.amount)) - (SUM(o.cost))` -- scalar ops on aggregates
- Derived depth 2: `((SUM(o.amount)) - (SUM(o.cost))) / (SUM(o.amount))` -- still valid

No CTE layering needed. The metric dependency DAG ensures substitution order (base metrics first, then derived in topological order).

**Stack requirement:** None new. Topological sort of the metric dependency graph uses the same `HashMap + in_degree + BFS` pattern already in `graph.rs`. String substitution is `str::replace`.

**Snowflake reference (verified Sep 30, 2025 release notes):**
```sql
METRICS (
  orders.revenue AS SUM(o_totalprice),
  orders.cost AS SUM(o_cost),
  profit AS orders.revenue - orders.cost
)
```

**Confidence:** HIGH -- the approach is well-defined; expression inlining with topological sort is a standard pattern; the model already supports `source_table: None`.

### 3. Hierarchies / Drill-Down Paths

**What it is:** An ordered list of dimensions forming a drill-down path. Example: `HIERARCHIES (geography AS (country, region, city))`. This is metadata-only -- it does not change query expansion. It enables BI tools and DESCRIBE output to show drill-down paths.

**What already exists:**
- `model.rs`: No hierarchy type exists.
- `body_parser.rs`: No HIERARCHIES clause.

**Important finding:** Snowflake semantic views do NOT have a HIERARCHIES clause as of March 2026. Hierarchies are a concept from OLAP cubes (SSAS, Power BI) and dbt semantic models, not from Snowflake's semantic view syntax. This is a custom extension beyond Snowflake parity.

**What v0.5.3 adds:**

| Component | Change | Complexity |
|-----------|--------|------------|
| `model.rs` | New `Hierarchy { name: String, dimensions: Vec<String> }` struct with serde derives. Add `hierarchies: Vec<Hierarchy>` to `SemanticViewDefinition` with `#[serde(default)]`. | Low (~15 lines) |
| `body_parser.rs` | Add `"hierarchies"` to `CLAUSE_KEYWORDS`. Parse `name AS (dim1, dim2, ...)` -- a name followed by a parenthesized comma-separated list of dimension names. | Low (~30 lines) |
| `graph.rs` | Validate that all dimension names in hierarchies exist in the definition's dimensions list. | Low (~15 lines) |
| `expand.rs` | No changes. Hierarchies are metadata-only in v0.5.3. They appear in DESCRIBE output but do not affect query SQL. | None |
| `ddl/describe.rs` | Show hierarchies in DESCRIBE output. | Low (~20 lines) |

**Stack requirement:** None new.

**Confidence:** MEDIUM -- this is a custom feature not based on Snowflake. Design is straightforward but there is no external reference implementation to validate against. The "metadata-only" approach is safe (no expansion changes).

### 4. Fan Trap Detection and Deduplication Warnings

**What it is:** A fan trap occurs when a one-to-many join causes measure duplication. If `orders` has many `line_items`, and you query `SUM(orders.amount)` alongside `line_items.*`, each order amount is counted once per line item.

**How other systems handle it:**
- **Cube.dev:** Requires `primary_key: true` on a dimension; uses the PK to generate deduplication subqueries (pre-aggregate per PK before joining). Automatic, built into SQL generation.
- **Snowflake:** Validates granularity at CREATE time -- "the logical table for the dimension must have an equal or lower level of granularity than the logical table for the metric." Does not generate deduplication SQL.
- **Sisense/BO:** Universe-level fan trap detection with warning dialogs.

**What v0.5.3 adds:**

| Component | Change | Complexity |
|-----------|--------|------------|
| `graph.rs` | Add relationship cardinality tracking. Each relationship edge stores whether it is one-to-many (FK side has duplicates) or many-to-one (FK side is unique). For v0.5.3, **all relationships are treated as many-to-one** (FK table references PK table) -- the FK side may have duplicates for the same PK. | Low (~10 lines, field on Join) |
| `expand.rs` | **Warning detection (not blocking).** At expansion time, check if a requested metric's `source_table` is on the "one" side of a one-to-many relationship while a dimension's `source_table` is on the "many" side (or vice versa). If so, emit a warning via DuckDB's `LogicalType::VARCHAR` result column or print to stderr. | Medium (~50 lines) |
| `model.rs` | Optional: add `cardinality: Option<String>` to `Join` (values: `"one_to_many"`, `"many_to_one"`, `"one_to_one"`). Use `#[serde(default)]` -- unset means unknown (no warning). | Low (~5 lines) |

**Design decision: Warning vs. blocking.**

| Approach | Pros | Cons |
|----------|------|------|
| **Block** fan trap queries | Prevents incorrect results | False positives (user may intend the fan-out); overly restrictive for power users |
| **Warn** about fan trap risk | Informs without blocking; user retains control | Users may ignore warnings |
| **Auto-dedup** via PK subquery | Correct results automatically (Cube.dev approach) | Complex SQL generation; may produce unexpected results if user intends fan-out |

**Recommendation: Warning-only for v0.5.3.** The extension is a preprocessor -- it should expand correct SQL and warn about potential pitfalls, not silently rewrite queries. Auto-dedup is a future milestone feature (requires cardinality metadata that users may not provide).

**Stack requirement:** None new. Warning output can use the existing `ExpandError` enum or a new `ExpandWarning` type.

**Confidence:** MEDIUM -- fan trap detection logic is well-understood, but the UX of warnings in DuckDB extensions is uncertain (no standard warning channel exists; warnings would likely be stderr or an extra result column).

### 5. Role-Playing Dimensions (Same Table Joined via Different Relationships)

**What it is:** A single physical table joined to the fact table via different foreign keys, each representing a different "role." Example: `airports` joined as both `departure_airport` and `arrival_airport` to `flights`.

**What already exists:**
- `graph.rs`: `check_no_diamonds` rejects any node reachable via multiple parents.
- `model.rs`: `Join` has `name: Option<String>` for relationship naming.
- `body_parser.rs`: RELATIONSHIPS clause parses `name AS from_alias(fk_cols) REFERENCES to_alias`.

**The key insight:** Role-playing dimensions are NOT diamonds. A diamond is when two different intermediate tables both lead to the same target. Role-playing is when the SAME source table has two different FK columns pointing to the SAME target table. In graph terms:

```
Diamond (rejected):   flights -> dep_airport -> airports
                      flights -> carrier -> airports    (different intermediate)

Role-playing:         flights(departure_airport) REFERENCES airports   (edge 1)
                      flights(arrival_airport) REFERENCES airports     (edge 2)
                      Same source, same target, different FK columns
```

Currently `check_no_diamonds` rejects this because `airports` has two parents (`flights` via two edges). But role-playing is safe because the relationship name disambiguates which path to use.

**What v0.5.3 adds:**

| Component | Change | Complexity |
|-----------|--------|------------|
| `graph.rs` | Relax `check_no_diamonds` to allow multiple edges between the SAME pair of nodes (same `from_alias` and same `to_alias`) when each edge has a distinct relationship `name`. Continue rejecting diamonds where the paths go through DIFFERENT intermediate nodes. | Medium (~30 lines) |
| `expand.rs` | When expanding a metric/dimension that uses a role-playing table, the relationship name determines which FK columns to use for the ON clause. Add `using_relationships: Vec<String>` to `Metric` model to specify which named relationship to traverse. | Medium (~40 lines) |
| `body_parser.rs` | No changes -- relationship naming already parsed. | None |
| `model.rs` | Add `using_relationships: Option<Vec<String>>` to `Metric` with `#[serde(default)]`. | Low (~5 lines) |

**Snowflake reference (verified):**
```sql
TABLES (
  flights PRIMARY KEY (flight_id),
  airports PRIMARY KEY (airport_code)
)
RELATIONSHIPS (
  flight_departure_airport AS flights(departure_airport) REFERENCES airports(airport_code),
  flight_arrival_airport AS flights(arrival_airport) REFERENCES airports(airport_code)
)
METRICS (
  flights.m_departure_count USING (flight_departure_airport) AS COUNT(flight_id),
  flights.m_arrival_count USING (flight_arrival_airport) AS COUNT(flight_id)
)
```

**Stack requirement:** None new.

**Confidence:** HIGH -- Snowflake's USING clause provides a clear reference implementation. The graph change is localized to `check_no_diamonds`. The expansion change is scoped to ON clause selection.

### 6. Semi-Additive Metrics (NON ADDITIVE BY)

**What it is:** A metric that should not be aggregated across certain dimensions. Classic example: account balance should not be summed across time periods -- only the latest balance per period is meaningful.

**How it works (Snowflake, verified Mar 5, 2026 release):**

1. At CREATE time, the metric declares `NON ADDITIVE BY (year_dim DESC, month_dim DESC, day_dim DESC)`.
2. At query time, if the query requests dimensions that are in the NON ADDITIVE BY list, the engine:
   a. Sorts rows by the non-additive dimensions (using the declared sort order)
   b. Takes the last row per group (latest snapshot)
   c. Aggregates the result

**SQL expansion strategy for v0.5.3:**

Given:
```sql
METRICS (
  accounts.balance NON ADDITIVE BY (year_dim DESC, month_dim DESC) AS SUM(balance)
)
```

When a query requests `dimensions := ['customer_id'], metrics := ['balance']`, the expansion generates:

```sql
WITH "_ranked" AS (
  SELECT "a"."customer_id",
         "a"."balance",
         ROW_NUMBER() OVER (
           PARTITION BY "a"."customer_id"
           ORDER BY "a"."year" DESC, "a"."month" DESC
         ) AS "_rn"
  FROM "accounts" AS "a"
)
SELECT "customer_id", SUM("balance") AS "balance"
FROM "_ranked"
WHERE "_rn" = 1
GROUP BY 1
```

**What v0.5.3 adds:**

| Component | Change | Complexity |
|-----------|--------|------------|
| `model.rs` | New `NonAdditiveDimension { dimension: String, sort_direction: SortDirection, null_order: NullOrder }` struct. Add `non_additive_by: Vec<NonAdditiveDimension>` to `Metric` with `#[serde(default)]`. `SortDirection` enum: `Asc`, `Desc`. `NullOrder` enum: `First`, `Last`. | Low (~25 lines) |
| `body_parser.rs` | Parse `NON ADDITIVE BY (dim [ASC|DESC] [NULLS {FIRST|LAST}], ...)` between metric name and `AS` keyword. Depth-0 comma splitting reused. | Medium (~50 lines) |
| `expand.rs` | For metrics with non-empty `non_additive_by`: generate CTE with `ROW_NUMBER() OVER (PARTITION BY non_na_dims ORDER BY na_dims) AS _rn`, then outer query filters `WHERE _rn = 1` and aggregates. | Medium-High (~80 lines) |
| `expand.rs` | Determine which requested dimensions overlap with `non_additive_by` dimensions to build the PARTITION BY and ORDER BY clauses correctly. | Medium (~30 lines) |

**Important constraint:** If ALL dimensions in the query are in the NON ADDITIVE BY list, there is nothing to partition by -- the metric degenerates to picking the single latest row and aggregating. If NO dimensions in the query are in the NON ADDITIVE BY list, the metric behaves as a normal additive metric (no ROW_NUMBER needed).

**Stack requirement:** None new. `ROW_NUMBER() OVER (...)` is standard DuckDB SQL. The CTE pattern already exists in the expansion engine's toolbox.

**Confidence:** MEDIUM -- the SQL expansion pattern is clear, but the interaction between semi-additive metrics and other features (derived metrics, fan traps) adds complexity. Edge cases around mixed additive/semi-additive metrics in the same query need careful testing.

### 7. Multiple Join Paths (USING RELATIONSHIPS)

**What it is:** When the relationship graph has multiple paths between two tables (relaxing the current diamond rejection), the user must specify which path to use via `USING (relationship_name, ...)` on the metric.

**What already exists:**
- `graph.rs`: `check_no_diamonds` rejects all diamonds.
- `expand.rs`: `resolve_joins_pkfk` walks reverse edges to find transitive intermediaries.
- `model.rs`: `Join.name: Option<String>` stores relationship name.

**What v0.5.3 adds:**

| Component | Change | Complexity |
|-----------|--------|------------|
| `graph.rs` | Replace `check_no_diamonds` with `check_ambiguous_paths` -- a diamond is only an error if no metric disambiguates it with `USING RELATIONSHIPS`. If all metrics that require the diamond path specify `USING`, the diamond is allowed. | Medium (~40 lines) |
| `expand.rs` | When a metric has `using_relationships`, use only the named relationships for join path resolution instead of the full graph. Build a subgraph from the specified relationship names and resolve joins within that subgraph. | Medium (~50 lines) |
| `body_parser.rs` | Parse `USING (rel_name, ...)` between metric name and `AS` keyword. Same position as `NON ADDITIVE BY` -- the two clauses are mutually ordered (USING first, then NON ADDITIVE BY, per Snowflake syntax). | Medium (~30 lines) |
| `model.rs` | Add `using_relationships: Option<Vec<String>>` to `Metric` with `#[serde(default)]`. (Same field as used by role-playing dimensions -- they are the same feature.) | Low (~5 lines) |

**Key insight:** Role-playing dimensions (feature 5) and multiple join paths (feature 7) share the same mechanism. Both use `USING (relationship_name)` on metrics to disambiguate which relationship to traverse. The difference is:
- Role-playing: same source+target, different FK columns
- Multiple paths: different intermediate tables forming a diamond

Both are resolved by `USING RELATIONSHIPS` filtering the graph to only the named edges.

**Stack requirement:** None new. Graph subgraph filtering is `edges.iter().filter(|e| names.contains(&e.name))`.

**Snowflake reference (verified):**
```sql
METRICS (
  flights.m_departure_count USING (flight_departure_airport) AS COUNT(flight_id)
)
```

**Confidence:** HIGH -- the mechanism is well-defined by Snowflake. The graph relaxation is straightforward. The shared implementation with role-playing dimensions reduces total work.

---

## What NOT to Add

| Candidate | Version | Why Not |
|-----------|---------|---------|
| `sqlparser` | 0.61+ | Same reasoning as v0.5.2: the DDL body grammar is domain-specific, not SQL. New clauses (FACTS, HIERARCHIES, NON ADDITIVE BY, USING) are custom keywords that sqlparser cannot parse. |
| `petgraph` | 0.8+ | Same reasoning as v0.5.2: the relationship graph has 2-8 nodes. The new features (metric dependency DAG, graph subgraph filtering) are ~30-50 lines each with HashMap. petgraph's NodeIndex indirection adds overhead at this scale. |
| `regex` | 1.x | Not needed for any feature. NON ADDITIVE BY parsing and USING clause parsing are structural (keyword + parenthesized list), not pattern-based. |
| `indexmap` | 2.x | Ordered maps could replace HashMap for deterministic iteration in metric dependency resolution. But the existing pattern (collect into Vec, sort) works and avoids adding a dependency. |
| `daggy` | 0.8 | DAG-specific petgraph wrapper. Same dependency chain objection. The metric dependency DAG has typically 2-10 nodes. |

---

## Recommended Stack

### Core Framework (Unchanged)

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| Rust | 2021 edition | Extension language | Existing. Memory safety + C FFI for DuckDB integration. |
| C++ shim | Vendored DuckDB amalgamation | Parser hook registration | Existing. No changes needed for v0.5.3 features -- all new DDL clauses are parsed in Rust. |
| `duckdb` crate | `=1.4.4` | DuckDB Rust bindings | Existing. VTab, BindInfo, Connection for DDL/query pipeline. |
| `libduckdb-sys` | `=1.4.4` | Raw FFI types | Existing. `duckdb_vector`, `duckdb_type` for typed output. |

### Serialization (Unchanged)

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `serde` | `1` | Derive Serialize/Deserialize | Existing. New model fields (`non_additive_by`, `using_relationships`, `hierarchies`) use `#[serde(default)]`. |
| `serde_json` | `1` | JSON catalog persistence | Existing. `SemanticViewDefinition` stored as JSON in `semantic_layer._definitions`. |

### Supporting Libraries (Unchanged)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `strsim` | `0.11` | Levenshtein distance for "did you mean" suggestions | Error messages for unknown relationship names, hierarchy dimension references, clause keyword typos |
| `cc` | `1` (optional) | C++ amalgamation compilation | Extension builds only (gated behind `extension` feature) |
| `proptest` | `1.9` (dev-dep) | Property-based tests | New parser clauses, metric dependency resolution, graph relaxation |
| `cargo-husky` | `1` (dev-dep) | Pre-commit hooks | Unchanged |
| `arbitrary` | `1` (optional) | Fuzz target derives | New model types need `#[derive(Arbitrary)]` for existing fuzz targets |

---

## Model Changes (All via `#[serde(default)]` -- No Migration)

### New Types

```rust
/// Sort direction for NON ADDITIVE BY dimensions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

/// Null ordering for NON ADDITIVE BY dimensions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum NullOrder {
    First,
    #[default]
    Last,
}

/// A dimension constraint for semi-additive metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NonAdditiveDimension {
    pub dimension: String,
    #[serde(default)]
    pub sort_direction: SortDirection,
    #[serde(default)]
    pub null_order: NullOrder,
}

/// A named hierarchy (ordered drill-down path through dimensions).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hierarchy {
    pub name: String,
    pub dimensions: Vec<String>,
}
```

### Modified Types

```rust
// Metric gains two optional fields:
pub struct Metric {
    pub name: String,
    pub expr: String,
    pub source_table: Option<String>,
    pub output_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_additive_by: Vec<NonAdditiveDimension>,  // NEW
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub using_relationships: Vec<String>,              // NEW
}

// SemanticViewDefinition gains one field:
pub struct SemanticViewDefinition {
    // ... existing fields ...
    #[serde(default)]
    pub hierarchies: Vec<Hierarchy>,  // NEW
}
```

### Backward Compatibility

All new fields use `#[serde(default)]` with `skip_serializing_if` where appropriate. Old stored JSON without these fields deserializes correctly:
- `non_additive_by` defaults to empty Vec (metric is fully additive)
- `using_relationships` defaults to empty Vec (use default join path)
- `hierarchies` defaults to empty Vec (no drill-down paths)

This is the same backward-compatibility pattern used successfully 10+ times in the existing model.

---

## Parser Changes (`body_parser.rs`)

### New Clause Keywords

```rust
const CLAUSE_KEYWORDS: &[&str] = &[
    "tables", "relationships", "facts", "dimensions", "metrics", "hierarchies"
];
const CLAUSE_ORDER: &[&str] = &[
    "tables", "relationships", "facts", "dimensions", "metrics", "hierarchies"
];
```

FACTS is positioned between RELATIONSHIPS and DIMENSIONS because facts are referenced by dimensions/metrics and must be defined first. HIERARCHIES is last because it references dimensions.

### New Metric Modifiers

The metric entry parser must handle two optional modifiers between the metric name and `AS`:

```
metric_def ::= [alias.] metric_name
               [USING '(' rel_name {',' rel_name} ')']
               [NON ADDITIVE BY '(' dim [ASC|DESC] [NULLS {FIRST|LAST}] {',' ...} ')']
               AS sql_expr
```

This is parsed by scanning for the `USING` and `NON ADDITIVE BY` keywords before `AS`, using the same depth-0 keyword detection pattern as `find_clause_bounds`.

---

## Expansion Engine Changes (`expand.rs`)

### Metric Dependency Resolution (Derived Metrics)

Before building SELECT expressions, topologically sort metrics:

1. Build a dependency graph: for each metric, scan its expression for references to other metric names.
2. Topological sort (Kahn's algorithm, same as `graph.rs`).
3. Substitute in topological order: replace metric name references with `(metric_expression)`.

Detection of metric references in expressions uses simple case-insensitive word boundary matching. A metric reference `revenue` in expression `orders.revenue - orders.cost` is detected by checking if any other metric's qualified name (`alias.metric_name`) or unqualified name appears as a substring. This is imprecise but safe -- false positives cause harmless double-parenthesization.

### Fan Trap Warning

At expansion time, after resolving joins:

1. For each resolved metric, check if its `source_table` is the "one" side of a one-to-many join.
2. If so, and if a resolved dimension comes from the "many" side, warn that the metric may be duplicated.

This requires cardinality metadata on joins. For v0.5.3, cardinality is heuristic: the FK side is "many" (it references a PK), the PK side is "one" (it is referenced). A metric on the PK side with a dimension on the FK side is a fan trap risk.

Warning mechanism: return warnings alongside the expanded SQL. The `expand` function signature changes from `Result<String, ExpandError>` to `Result<ExpandResult, ExpandError>` where `ExpandResult { sql: String, warnings: Vec<String> }`.

### Semi-Additive CTE

For metrics with `non_additive_by`:

1. Determine which requested dimensions are in the `non_additive_by` list ("NA dims") and which are not ("partition dims").
2. Generate a CTE with `ROW_NUMBER() OVER (PARTITION BY partition_dims ORDER BY na_dims) AS _rn`.
3. Filter `WHERE _rn = 1` in the outer query.
4. Apply the metric's aggregate expression on the filtered rows.

### USING RELATIONSHIPS Path Selection

For metrics with `using_relationships`:

1. Filter the relationship graph to only edges whose names appear in the USING list.
2. Resolve joins from the filtered graph (same `resolve_joins_pkfk` algorithm).
3. If the filtered graph does not connect the metric's source table to the base table, report an error.

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not Alternative |
|----------|-------------|-------------|---------------------|
| Metric dependency resolution | In-place Kahn's algorithm (~30 lines) | `petgraph` toposort | 2-10 node graph; petgraph overhead unjustified |
| Fan trap detection | Warning heuristic (FK direction) | Full cardinality analysis (data sampling) | Query-time data sampling violates "preprocessor-only" design; too slow |
| Fan trap detection | Warning-only | Auto-dedup subquery (Cube.dev approach) | Complex SQL rewriting; unexpected results; future milestone |
| Semi-additive expansion | ROW_NUMBER CTE | QUALIFY clause (DuckDB supports it) | QUALIFY is DuckDB-specific; ROW_NUMBER CTE is portable and easier to debug via EXPLAIN |
| Semi-additive expansion | ROW_NUMBER CTE | LAST_VALUE window function | LAST_VALUE requires frame specification; ROW_NUMBER + filter is simpler and deterministic |
| Derived metrics | Expression inlining with topological sort | CTE layering (base metrics in inner, derived in outer) | CTE layering adds SQL complexity; expression inlining produces valid SQL because aggregates are never nested (derived metrics reference aggregate results, not inputs) |
| Hierarchy storage | `Vec<Hierarchy>` on definition | Separate catalog table | Hierarchies are small metadata; embedding in the JSON definition avoids schema changes |
| USING RELATIONSHIPS | Graph subgraph filtering | Separate join resolution per metric | Graph filtering reuses existing `resolve_joins_pkfk`; per-metric resolution duplicates code |

---

## Complete v0.5.3 Cargo.toml Changes

**None.** The Cargo.toml is unchanged from v0.5.2:

```toml
# NO CHANGES to [dependencies]:
# duckdb = { version = "=1.4.4", default-features = false }
# libduckdb-sys = "=1.4.4"
# serde = { version = "1", features = ["derive"] }
# serde_json = "1"
# strsim = "0.11"

# NO CHANGES to [build-dependencies]:
# cc = { version = "1", optional = true }

# NO CHANGES to [dev-dependencies]:
# proptest = "1.9"
```

Version bump only: `version = "0.5.0"` -> `version = "0.5.3"` (at milestone completion).

---

## Integration Points

### Where new code touches existing code

| New Feature | Touches | How |
|-------------|---------|-----|
| FACTS clause | `body_parser.rs`, `expand.rs`, `graph.rs` | New clause parser; fact substitution in metric expressions; source_table validation |
| Derived metrics | `body_parser.rs`, `expand.rs` | Detect unqualified metric names; metric dependency DAG + expression inlining |
| Hierarchies | `model.rs`, `body_parser.rs`, `graph.rs`, `ddl/describe.rs` | New Hierarchy type; parser clause; dimension name validation; DESCRIBE output |
| Fan trap detection | `expand.rs` | Warning generation based on FK direction analysis |
| Role-playing dimensions | `graph.rs`, `expand.rs` | Relax diamond check for same-source/same-target edges; USING-filtered join resolution |
| Semi-additive metrics | `model.rs`, `body_parser.rs`, `expand.rs` | New NonAdditiveDimension type; parser for NON ADDITIVE BY clause; CTE with ROW_NUMBER |
| Multiple join paths | `graph.rs`, `expand.rs`, `model.rs` | Same as role-playing dimensions (shared mechanism via USING RELATIONSHIPS) |

### What stays untouched

- `cpp/src/shim.cpp` -- C++ shim unchanged. All new DDL clauses parsed in Rust.
- `src/ddl/define.rs` -- VTab implementation unchanged. Receives same model via JSON.
- `src/ddl/drop.rs`, `src/ddl/list.rs` -- No changes.
- `src/query/table_function.rs` -- Query pipeline unchanged (calls `expand()` which dispatches internally).
- `src/catalog.rs` -- Catalog operations unchanged (stores/loads JSON).
- `build.rs` -- Build script unchanged.
- `src/parse.rs` -- DDL prefix detection and rewrite pipeline unchanged. Body parser is in `body_parser.rs`.

---

## Complexity Budget

| Feature | Parser | Model | Graph | Expand | Total Est. LOC | Risk |
|---------|--------|-------|-------|--------|---------------|------|
| FACTS clause | ~30 | 0 | ~10 | ~40 | ~80 | Low |
| Derived metrics | ~5 | 0 | 0 | ~110 | ~115 | Medium |
| Hierarchies | ~30 | ~15 | ~15 | 0 | ~60 | Low |
| Fan trap detection | 0 | ~5 | 0 | ~50 | ~55 | Medium |
| Role-playing dims | 0 | ~5 | ~30 | ~40 | ~75 | Medium |
| Semi-additive | ~50 | ~25 | 0 | ~110 | ~185 | Medium-High |
| Multiple join paths | ~30 | 0 | ~40 | ~50 | ~120 | Medium |
| **Total** | **~145** | **~50** | **~95** | **~400** | **~690** | **Medium** |

The expansion engine bears the most complexity (400 of 690 estimated lines), particularly for semi-additive CTE generation and derived metric resolution. These two features should be built and tested independently before combining.

---

## Confidence Assessment

| Area | Level | Reason |
|------|-------|--------|
| No new deps needed | HIGH | All features use standard algorithms (toposort, string substitution, CTE SQL generation) on small data structures. No external crate provides meaningful value. |
| FACTS clause | HIGH | Model type exists; parser pattern exists; only wiring. |
| Derived metrics | HIGH | Snowflake reference verified (Sep 2025 release). Expression inlining with topological sort is well-defined. |
| Hierarchies | MEDIUM | Custom feature beyond Snowflake. Metadata-only (no expansion changes) minimizes risk, but no external reference to validate design against. |
| Fan trap detection | MEDIUM | Warning-only approach is safe, but UX of warnings in DuckDB extensions is unvalidated. |
| Role-playing dimensions | HIGH | Snowflake USING clause provides clear reference. Graph change is localized. |
| Semi-additive metrics | MEDIUM | SQL expansion pattern (ROW_NUMBER CTE) is clear, but interaction with other features (derived metrics, multiple metrics in same query) adds edge cases. |
| Multiple join paths | HIGH | Same mechanism as role-playing dimensions (USING RELATIONSHIPS). Graph relaxation is localized. |

---

## Sources

### Snowflake Official Documentation (HIGH confidence)

- [CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- FACTS, METRICS with USING and NON ADDITIVE BY syntax
- [Using SQL commands for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- worked examples: FACTS clause, derived metrics, role-playing dimensions with USING, NON ADDITIVE BY
- [SEMANTIC_VIEW query construct](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- query-time syntax
- [YAML specification](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- non_additive_dimensions and using_relationships YAML structure
- [Derived metrics release notes (Sep 30, 2025)](https://docs.snowflake.com/en/release-notes/2025/other/2025-09-30-semantic-view-derived-metrics) -- derived metrics GA
- [Semi-additive metrics release notes (Mar 5, 2026)](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY GA

### Cube.dev Documentation (MEDIUM confidence)

- [Working with Joins](https://cube.dev/docs/product/data-modeling/concepts/working-with-joins) -- fan trap handling via primary_key requirement, diamond subgraph disambiguation
- [Cube reference](https://cube.dev/docs/product/data-modeling/reference/cube) -- primary_key dimension for deduplication

### dbt/MetricFlow (MEDIUM confidence)

- [Advanced metrics](https://docs.getdbt.com/best-practices/how-we-build-our-metrics/semantic-layer-5-advanced-metrics) -- derived metric expr + input_metrics pattern
- [About MetricFlow](https://docs.getdbt.com/docs/build/about-metricflow) -- dataflow plan DAG for metric SQL generation

### Fan Trap / Chasm Trap Analysis (MEDIUM confidence)

- [Datacadamia: Fan Trap](https://www.datacadamia.com/data/type/cube/semantic/fan_trap) -- fan trap definition and one-to-many duplication
- [Datacadamia: Chasm Trap](https://datacadamia.com/data/type/cube/semantic/chasm_trap) -- chasm trap definition
- [Sisense: Chasm and Fan Traps](https://docs.sisense.com/main/SisenseLinux/chasm-and-fan-traps.htm) -- detection and resolution strategies

### Project Source Code (HIGH confidence -- direct analysis)

- `src/model.rs` -- existing Fact, Metric, Join, SemanticViewDefinition types
- `src/body_parser.rs` -- clause keyword scanning, depth-0 comma splitting, entry parsing
- `src/graph.rs` -- RelationshipGraph, toposort, check_no_diamonds, validate_graph
- `src/expand.rs` -- resolve_joins_pkfk, synthesize_on_clause, expand()
- `Cargo.toml` -- current dependency inventory
