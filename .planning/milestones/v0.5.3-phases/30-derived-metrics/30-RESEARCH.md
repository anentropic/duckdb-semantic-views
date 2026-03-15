# Phase 30: Derived Metrics - Research

**Researched:** 2026-03-14
**Domain:** DDL parsing (unqualified metric entries), expression inlining (metric-to-metric), define-time validation (cycles, aggregation detection), expansion pipeline changes
**Confidence:** HIGH

## Summary

Phase 30 adds derived metrics: metrics defined without a table prefix that compose other metrics' aggregate expressions using arithmetic operators. Snowflake introduced derived metrics on 2025-09-30, defining them as "view-level metrics not tied to a specific logical table" that "can combine metrics from multiple logical tables." The syntax distinguisher is the absence of `table_alias.` in the name -- `profit AS revenue - cost` rather than `o.profit AS SUM(...)`.

The core implementation challenge is **expression inlining at the metric level**: when a derived metric references `revenue`, the expansion engine must replace `revenue` with the full aggregate expression of the base metric (e.g., `SUM(li.extended_price * (1 - li.discount))`), after that base metric's expression has already had facts inlined. This is structurally identical to how facts are inlined into metrics today (Phase 29), and the existing `replace_word_boundary`, `toposort_facts`, and `inline_facts` patterns can be directly reused. Derived metrics that reference other derived metrics ("stacking") require topological resolution, which is the same Kahn's algorithm pattern used for fact dependency ordering.

The key distinction from facts: facts produce row-level expressions that get wrapped in aggregates, while derived metrics produce aggregate-level expressions that get composed arithmetically. A derived metric expression `revenue - cost` expands to `SUM(li.net_price) - SUM(li.cost)` -- the referenced metric names are replaced with their fully-resolved aggregate expressions (with facts already inlined).

**Primary recommendation:** Implement derived metrics as a three-step addition: (1) parsing of unqualified metric entries in body_parser.rs, (2) define-time validation in graph.rs (cycle detection, aggregation function rejection, reference validation), (3) expansion-time metric expression inlining in expand.rs (after fact inlining, before SELECT construction). The Metric struct already has `source_table: Option<String>` -- derived metrics simply have `source_table: None`. No new struct is needed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DRV-01 | User can declare derived metrics without a table prefix (`metric_name AS metric_a - metric_b`) | Parsing requires new `parse_unqualified_entry` in body_parser.rs alongside existing `parse_qualified_entries`. Metric struct already supports `source_table: None`. Snowflake syntax verified: omit `table_alias.` from name. |
| DRV-02 | Derived metrics expand by inlining referenced metrics' aggregate expressions | Expansion pipeline in expand.rs extended: after fact inlining, resolve derived metrics by replacing metric name references with their fully-resolved aggregate expressions. Reuses `replace_word_boundary` and topological sort. |
| DRV-03 | Derived metrics can reference other derived metrics (stacking); expansion resolves in topological order | Same Kahn's algorithm pattern as `toposort_facts` in expand.rs and `check_fact_cycles` in graph.rs. Build metric dependency DAG, sort topologically, inline in order. Snowflake confirms stacking is supported. |
| DRV-04 | Define-time validation rejects derived metric cycles and references to non-existent metrics | Reuse `build_fact_dag`/`check_fact_cycles` pattern from graph.rs. Build derived metric DAG, detect cycles via Kahn's leftover nodes, validate references exist against combined base+derived metric name set. |
| DRV-05 | Derived metrics cannot contain aggregation functions (define-time validation) | New `contains_aggregate_function` scanner needed. Scan expression for known SQL aggregate function names (SUM, COUNT, AVG, MIN, MAX, etc.) at word boundaries followed by `(`. Reject at CREATE time with clear error. |
</phase_requirements>

## Snowflake Alignment Analysis

### Derived Metrics -- Maps Cleanly
| Snowflake Feature | This Extension | Status |
|-------------------|----------------|--------|
| Omit `table_alias.` from name to define derived metric | Same -- `source_table: None` in Metric struct | **Aligned** |
| Derived metrics reference other metrics by name | Same -- word-boundary inlining | **Aligned** |
| Derived metrics can reference other derived metrics (stacking) | Same -- topological sort resolution | **Aligned** |
| Cannot use aggregation functions in derived metrics | Same -- define-time validation (DRV-05) | **Aligned** |
| Cannot use window functions | Not checked -- window functions not supported in metrics at all | **Aligned** (by omission) |
| Cannot reference physical columns | Not enforced -- but derived metrics with `source_table: None` naturally lack column context | **Partial** |
| USING clause for derived metrics | Snowflake says derived metrics cannot use USING; USING is Phase 32 | **Deferred** |
| Derived metrics cannot be referenced by non-derived metrics | Not enforced -- but practically impossible since base metrics use aggregate expressions | **Natural** |

### Key Snowflake Behaviors to Match
1. **No table prefix**: `profit AS revenue - cost` (not `o.profit AS ...`)
2. **Expression is pure arithmetic on metric names**: `revenue - cost`, `revenue / order_count * 100`
3. **Stacking**: `net_profit AS revenue - cost`, then `margin AS net_profit / revenue` -- multi-level chaining
4. **Aggregate prohibition**: `SUM(revenue)` in a derived metric is an error -- derived metrics compose already-aggregated values

### Simplifications vs Snowflake
1. **No PRIVATE/PUBLIC modifiers** -- no access control in DuckDB extensions
2. **No USING clause** -- deferred to Phase 32
3. **No NON ADDITIVE BY** -- deferred to v0.5.4
4. **No COMMENT** -- deferred
5. **No physical column reference checking** -- derived metrics only reference metric names; referencing a column name that happens not to be a metric will be caught by "unknown metric reference" validation

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde/serde_json | 1.x | Metric serialization (unchanged) | Already used for all model structs |
| strsim | 0.11.x | "Did you mean?" suggestions for unknown metric refs | Already used for fact/dim/metric/clause suggestions |
| proptest | 1.x | Property-based testing for expression substitution | Already used for parse/expand proptests |

### Supporting
No new dependencies required. All needed crates are already in Cargo.toml.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| String scanning for aggregate detection | SQL parser (sqlparser-rs) | Full parser is overkill; aggregate names are a fixed known set; word-boundary + `(` check is sufficient and keeps zero new dependencies |
| New DerivedMetric struct | Existing Metric struct with `source_table: None` | Separate struct adds type safety but creates parallel code paths; Snowflake treats derived metrics as regular metrics without table scope; using existing Metric struct is simpler and Snowflake-aligned |

**Installation:**
No new dependencies required. All needed crates are already in Cargo.toml.

## Architecture Patterns

### Modified Components
```
src/
  body_parser.rs  [MODIFY] -- accept unqualified entries in METRICS clause (name AS expr, no dot)
  expand.rs       [MODIFY] -- add derived metric inlining after fact inlining, before SELECT
  graph.rs        [MODIFY] -- add validate_derived_metrics (cycles, unknown refs, aggregate check)
  ddl/
    define.rs     [MODIFY] -- wire validate_derived_metrics into bind()
```

### Pattern 1: Mixed Qualified/Unqualified Metric Parsing in body_parser.rs
**What:** The METRICS clause currently uses `parse_qualified_entries` which requires `alias.name AS expr` format. Derived metrics use `name AS expr` (no dot). The parser must handle both forms in the same clause.
**When to use:** DRV-01.
**Design:**
```rust
// Current: parse_qualified_entries requires alias.name format (dot is mandatory)
// New: parse_metrics_clause tries qualified first, falls back to unqualified

/// Parse METRICS clause content, handling both qualified (alias.name AS expr)
/// and unqualified (name AS expr) entries for derived metrics.
pub(crate) fn parse_metrics_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<(Option<String>, String, String)>, ParseError> {
    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();
    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let entry = entry.trim();
        if entry.is_empty() { continue; }

        // Try to parse as qualified (has dot before AS)
        // If dot exists before AS keyword: qualified entry -> Some(alias)
        // If no dot before AS: unqualified entry (derived metric) -> None
        let parsed = parse_metric_entry(entry, entry_offset)?;
        result.push(parsed);
    }
    Ok(result)
}

/// Parse a single metric entry. Returns (Option<alias>, name, expr).
/// Qualified: `alias.name AS expr` -> (Some("alias"), "name", "expr")
/// Unqualified: `name AS expr` -> (None, "name", "expr")
fn parse_metric_entry(
    entry: &str,
    entry_offset: usize,
) -> Result<(Option<String>, String, String), ParseError> {
    // Find AS keyword first
    let upper = entry.to_ascii_uppercase();
    let as_pos = find_keyword_ci(&upper, "AS")?;
    let before_as = entry[..as_pos].trim();
    let expr = entry[as_pos + 2..].trim();

    if let Some(dot_pos) = before_as.find('.') {
        // Qualified: alias.name
        let alias = before_as[..dot_pos].trim();
        let name = before_as[dot_pos + 1..].trim();
        Ok((Some(alias.to_string()), name.to_string(), expr.to_string()))
    } else {
        // Unqualified: derived metric
        Ok((None, before_as.to_string(), expr.to_string()))
    }
}
```
**Note:** The return type changes from `Vec<(String, String, String)>` to `Vec<(Option<String>, String, String)>`. The caller in the match block for "metrics" must be updated to handle `Option<String>` for source_table.

### Pattern 2: Derived Metric Inlining in expand.rs
**What:** After fact inlining, scan derived metrics' expressions for references to other metrics (both base and derived), replace with their fully-resolved aggregate expressions.
**When to use:** DRV-02, DRV-03.
**Design:**
```rust
/// Inline derived metrics by replacing metric name references with their
/// fully-resolved aggregate expressions.
///
/// Processing order:
/// 1. Facts are inlined into ALL metric expressions (base + derived)
/// 2. Base metrics are "resolved" -- their expressions are final after fact inlining
/// 3. Derived metrics are topologically sorted by inter-metric dependencies
/// 4. Each derived metric's expression has metric name references replaced
///    with the referenced metric's resolved aggregate expression
///
/// Example:
///   Base: revenue AS SUM(li.net_price)  [after fact inlining: SUM((li.extended_price * (1 - li.discount)))]
///   Base: cost AS SUM(li.cost)
///   Derived: profit AS revenue - cost
///   -> profit expands to: (SUM((li.extended_price * (1 - li.discount)))) - (SUM(li.cost))
fn inline_derived_metrics(
    metrics: &[Metric],
    facts: &[Fact],
    fact_topo_order: &[usize],
) -> HashMap<String, String> {
    // Step 1: Resolve ALL metrics' expressions with facts inlined
    let mut resolved: HashMap<String, String> = HashMap::new();

    // Base metrics (have source_table) -- resolve facts, store
    for met in metrics.iter().filter(|m| m.source_table.is_some()) {
        let expr = inline_facts(&met.expr, facts, fact_topo_order);
        resolved.insert(met.name.to_ascii_lowercase(), expr);
    }

    // Derived metrics (no source_table) -- toposort, then inline
    let derived: Vec<(usize, &Metric)> = metrics.iter()
        .enumerate()
        .filter(|(_, m)| m.source_table.is_none())
        .collect();

    if derived.is_empty() {
        return resolved;
    }

    // Build derived metric dependency order (Kahn's algorithm)
    let derived_topo = toposort_derived(&derived, &resolved);

    // Inline in topological order
    for idx in derived_topo {
        let met = &derived[idx].1;
        let mut expr = met.expr.clone();
        // Replace each known metric name with its resolved expression (parenthesized)
        for (name, replacement) in &resolved {
            expr = replace_word_boundary(&expr, name, &format!("({replacement})"));
        }
        resolved.insert(met.name.to_ascii_lowercase(), expr);
    }

    resolved
}
```
**Key insight:** The expansion step in `expand()` currently does `inline_facts(&met.expr, ...)` per metric. For derived metrics, the expression must have metric references inlined instead. The simplest approach: resolve all metric expressions upfront into a `HashMap<name, resolved_expr>`, then use the resolved expression when building SELECT items.

### Pattern 3: Aggregate Function Detection for DRV-05
**What:** Scan a derived metric expression for SQL aggregate function calls. Reject at CREATE time.
**When to use:** DRV-05.
**Design:**
```rust
/// Known SQL aggregate function names.
const AGGREGATE_FUNCTIONS: &[&str] = &[
    "sum", "count", "avg", "min", "max",
    "stddev", "stddev_pop", "stddev_samp",
    "variance", "var_pop", "var_samp",
    "string_agg", "listagg", "group_concat",
    "array_agg", "any_value", "approx_count_distinct",
    "median", "mode", "percentile_cont", "percentile_disc",
    "corr", "covar_pop", "covar_samp",
    "regr_avgx", "regr_avgy", "regr_count",
    "regr_intercept", "regr_r2", "regr_slope",
    "regr_sxx", "regr_sxy", "regr_syy",
    "bit_and", "bit_or", "bit_xor",
    "bool_and", "bool_or",
];

/// Check if an expression contains any aggregate function call.
/// Returns `Some(function_name)` if found, `None` if clean.
///
/// Detects `func_name(` patterns at word boundaries (case-insensitive).
/// Skips matches inside string literals.
fn contains_aggregate_function(expr: &str) -> Option<&'static str> {
    let lower = expr.to_ascii_lowercase();
    for &func in AGGREGATE_FUNCTIONS {
        // Find func at word boundary followed by '('
        // ... (word-boundary scan similar to find_fact_references)
    }
    None
}
```
**Note:** This should handle string literals to avoid false positives (e.g., `'SUM of values'`), but for Phase 30 a simple word-boundary + `(` check is sufficient since metric expressions rarely contain string literals.

### Pattern 4: Derived Metric Validation in graph.rs
**What:** Validate derived metrics at CREATE time: cycle detection, reference validation, aggregate prohibition.
**When to use:** DRV-04, DRV-05.
**Design:**
```rust
/// Validate derived metrics in a semantic view definition.
///
/// Checks:
/// 1. Derived metrics (source_table is None) cannot contain aggregate functions
/// 2. Derived metric references must resolve to existing metric names
/// 3. No cycles in derived metric dependency graph
pub fn validate_derived_metrics(def: &SemanticViewDefinition) -> Result<(), String> {
    let derived: Vec<&Metric> = def.metrics.iter()
        .filter(|m| m.source_table.is_none())
        .collect();

    if derived.is_empty() {
        return Ok(());
    }

    // 1. Check aggregate function prohibition (DRV-05)
    for met in &derived {
        if let Some(func) = contains_aggregate_function(&met.expr) {
            return Err(format!(
                "derived metric '{}' must not contain aggregate function '{}'. \
                 Derived metrics compose other metrics; use a regular metric for aggregation.",
                met.name, func
            ));
        }
    }

    // 2. Build dependency DAG and check references (DRV-04)
    let all_metric_names: Vec<&str> = def.metrics.iter()
        .map(|m| m.name.as_str())
        .collect();
    // ... (reuse find_fact_references pattern for metric names)

    // 3. Cycle detection (DRV-04)
    // ... (reuse check_fact_cycles pattern)

    Ok(())
}
```

### Anti-Patterns to Avoid
- **Treating derived metrics as a separate model struct:** Derived metrics are `Metric { source_table: None }`. Creating a separate `DerivedMetric` struct would fork the expansion pipeline, the DESCRIBE output, the query function, and the serialization. Keep it simple -- one struct, one Vec, distinguished by `source_table.is_none()`.
- **Inlining derived metric expressions at parse time:** The expression `revenue - cost` must be stored as-is in the definition JSON. Inlining happens at expansion time, just like facts. Storing the inlined expression would prevent definition updates.
- **Checking metric name references against column names:** Derived metrics reference metric names, not column names. The validation should check against `def.metrics[*].name`, not against table columns.
- **Forgetting to inline facts into derived metrics:** A derived metric `profit AS revenue - cost` where `revenue = SUM(net_price)` and `net_price` is a fact. The chain is: fact -> base metric expr -> derived metric expr. Facts must be inlined into base metric expressions BEFORE those expressions are used to resolve derived metrics.
- **Naive string replace instead of word-boundary:** Same pitfall as Phase 29. `revenue` must not match `revenue_total`. Use `replace_word_boundary`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Derived metric DAG cycle detection | Custom DFS with visited set | Kahn's algorithm (same as graph.rs fact validation) | Kahn's naturally detects cycles via leftover nodes; proven pattern in this codebase |
| Word-boundary metric name substitution | Naive `str::replace` | Existing `replace_word_boundary` from expand.rs | Already handles substring collisions, tested extensively in Phase 29 |
| Topological sort for derived metrics | New sort implementation | Clone the `toposort_facts` pattern | Same algorithm, same data shape (name -> expression -> dependencies) |
| "Did you mean?" for unknown metric refs | Custom string distance | `strsim::levenshtein` (already in dependencies) | Consistent with existing suggestion UX |
| Aggregate function detection | SQL parser dependency | Word-boundary scan + `(` check on known function names | Zero new dependencies; aggregate function names are a fixed set |

**Key insight:** Phase 30 reuses every pattern from Phase 29 (facts). The only new code is: (1) mixed qualified/unqualified parsing in body_parser.rs, (2) aggregate function detection for DRV-05, and (3) the metric-level inlining step in expand.rs. Everything else is pattern reuse.

## Common Pitfalls

### Pitfall 1: Mixed Qualified/Unqualified Parsing Breaks Existing Metrics
**What goes wrong:** Changing `parse_qualified_entries` to accept unqualified entries breaks the FACTS and DIMENSIONS clauses, which MUST have qualified names. Or worse, a typo like `revnue AS SUM(x)` (missing dot) gets parsed as a derived metric instead of erroring.
**Why it happens:** The METRICS clause is the ONLY clause that allows both forms.
**How to avoid:** Create a new `parse_metrics_clause` function specifically for METRICS. Keep `parse_qualified_entries` unchanged for FACTS and DIMENSIONS. The METRICS clause handler switches from `parse_qualified_entries` to `parse_metrics_clause`.
**Warning signs:** FACTS or DIMENSIONS tests start failing; or missing-dot typos silently create derived metrics.

### Pitfall 2: Derived Metric Inlining Produces Double-Aggregated SQL
**What goes wrong:** A derived metric `profit AS revenue - cost` expands to `SUM(SUM(li.net_price)) - SUM(SUM(li.cost))` instead of `SUM((li.net_price)) - SUM(li.cost)`.
**Why it happens:** The derived metric's expression gets passed through `inline_facts` AND then has its references replaced with already-aggregated expressions, and then the whole thing gets wrapped in another aggregate.
**How to avoid:** Derived metric expressions are NOT aggregates themselves -- they are arithmetic combinations of aggregates. The inlining replaces `revenue` with `(SUM(li.net_price))` (parenthesized). The derived metric's resolved expression IS the final SELECT expression -- it must not be wrapped in any additional aggregate.
**Warning signs:** Query results are wrong or DuckDB errors with "nested aggregate" messages.

### Pitfall 3: Derived Metrics Don't Contribute to Join Resolution
**What goes wrong:** A derived metric references `revenue` (which needs table `li`), but the join to `li` is not included because the derived metric has `source_table: None`.
**Why it happens:** `resolve_joins_pkfk` collects needed aliases from `source_table` fields. Derived metrics have `source_table: None`, so their transitive table dependencies are invisible.
**How to avoid:** Join resolution must look through derived metric references to find the source tables of the referenced base metrics. When building the needed-aliases set, for each derived metric, recursively collect source tables from all referenced base metrics.
**Warning signs:** Queries with only derived metrics produce "missing table" errors or incorrect results because JOINs are omitted.

### Pitfall 4: Metric Name Collision Between Base and Derived
**What goes wrong:** A base metric and a derived metric have the same name. The expansion engine picks the wrong one for inlining.
**Why it happens:** No uniqueness check across base and derived metrics in the same Vec.
**How to avoid:** Validate at define time that no two metrics share the same name (case-insensitive), regardless of whether they are base or derived. This is already implicitly handled by the current `parse_qualified_entries` deduplication, but with mixed parsing it needs explicit validation.
**Warning signs:** Ambiguous metric resolution; wrong expression inlined.

### Pitfall 5: Topological Sort Uses Wrong Name Set for Reference Detection
**What goes wrong:** The derived metric DAG builder scans for references against ALL metric names (including base metrics), but the topological sort only orders derived metrics. A derived metric referencing a base metric gets an unresolved in-degree.
**Why it happens:** Confusion between "reference targets" (all metrics) and "sort subjects" (derived metrics only).
**How to avoid:** The DAG for topological sorting should only have derived metric nodes. References to base metrics are edges FROM derived metrics to external nodes -- they don't contribute to in-degree because base metrics are already resolved before derived metric processing begins.
**Warning signs:** Derived metrics with base metric references fail topological sort with false "cycle detected" errors.

### Pitfall 6: Parenthesization of Inlined Metric Expressions
**What goes wrong:** `profit AS revenue - cost` inlines to `SUM(net_price) - SUM(cost)`. Then `margin AS profit / revenue` inlines to `SUM(net_price) - SUM(cost) / SUM(net_price)` -- operator precedence error (division before subtraction).
**Why it happens:** Inlined expressions are not parenthesized.
**How to avoid:** Always parenthesize inlined metric expressions: `margin` = `(SUM(net_price) - SUM(cost)) / (SUM(net_price))`. Same pattern as fact inlining in Phase 29.
**Warning signs:** Arithmetic results differ from expected values when derived metrics contain mixed operators.

## Code Examples

### Example 1: Full DDL with Derived Metrics
```sql
-- Snowflake-aligned derived metrics syntax
CREATE SEMANTIC VIEW sales_kpis AS
  TABLES (
    o AS orders PRIMARY KEY (id),
    li AS line_items PRIMARY KEY (id)
  )
  RELATIONSHIPS (
    order_items AS li(order_id) REFERENCES o
  )
  FACTS (
    li.net_price AS li.extended_price * (1 - li.discount)
  )
  DIMENSIONS (
    o.region AS o.region,
    o.order_date AS o.order_date
  )
  METRICS (
    li.revenue AS SUM(li.net_price),
    li.cost AS SUM(li.unit_cost),
    li.order_count AS COUNT(DISTINCT li.order_id),
    profit AS revenue - cost,
    margin AS profit / revenue * 100,
    avg_order_value AS revenue / order_count
  );
```

### Example 2: Expansion of Derived Metric Query
```sql
-- Query: dimensions=['region'], metrics=['profit', 'margin']
-- Step 1: Inline facts into base metrics:
--   revenue expr: SUM(li.net_price) -> SUM((li.extended_price * (1 - li.discount)))
--   cost expr: SUM(li.unit_cost) (no facts)
-- Step 2: Inline base metrics into derived metrics:
--   profit: revenue - cost -> (SUM((li.extended_price * (1 - li.discount)))) - (SUM(li.unit_cost))
--   margin: profit / revenue * 100
--     -> ((SUM((li.extended_price * (1 - li.discount)))) - (SUM(li.unit_cost))) / (SUM((li.extended_price * (1 - li.discount)))) * 100
-- Step 3: Generated SQL:
SELECT
    "o"."region" AS "region",
    (SUM((li.extended_price * (1 - li.discount)))) - (SUM(li.unit_cost)) AS "profit",
    ((SUM((li.extended_price * (1 - li.discount)))) - (SUM(li.unit_cost))) / (SUM((li.extended_price * (1 - li.discount)))) * 100 AS "margin"
FROM "orders" AS "o"
LEFT JOIN "line_items" AS "li" ON "li"."order_id" = "o"."id"
GROUP BY
    1
```

### Example 3: Error Cases
```sql
-- Error: aggregate function in derived metric (DRV-05)
METRICS (
    li.revenue AS SUM(li.amount),
    bad_derived AS SUM(revenue)  -- ERROR: derived metric must not contain aggregate function 'SUM'
)

-- Error: cycle in derived metrics (DRV-04)
METRICS (
    li.revenue AS SUM(li.amount),
    a AS b + 1,
    b AS a + 1  -- ERROR: cycle detected in derived metrics: a -> b -> a
)

-- Error: unknown metric reference (DRV-04)
METRICS (
    li.revenue AS SUM(li.amount),
    profit AS revenue - nonexistent  -- ERROR: unknown metric 'nonexistent' referenced in derived metric 'profit'
)
```

### Example 4: Join Resolution for Derived Metrics
```sql
-- Derived metric 'profit' references 'revenue' (source_table=li) and 'cost' (source_table=li)
-- Even though profit has source_table=None, the JOIN to li must be included
-- because the inlined expression contains li.* references.

-- Resolution: when expanding, look through derived metric references to find
-- source_tables of referenced base metrics. Include those in join resolution.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| All metrics require `alias.name` prefix | METRICS clause accepts both `alias.name` (base) and `name` (derived) | Phase 30 (now) | Users can compose metrics from other metrics without writing raw aggregates |
| Metric expressions used directly in SELECT | Derived metric expressions inlined with referenced metrics' aggregate expressions | Phase 30 (now) | Expression resolution chain: facts -> base metrics -> derived metrics |
| No metric-level dependency graph | Derived metric DAG with cycle detection | Phase 30 (now) | Prevents circular metric definitions |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (unit + proptest), sqllogictest, DuckLake CI |
| Config file | Cargo.toml, test/sql/TEST_LIST, justfile |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DRV-01 | Parse unqualified metric entries (derived metrics) | unit | `cargo test body_parser::tests::parse_derived_metric` | Wave 0 |
| DRV-01 | Mixed qualified + unqualified in same METRICS clause | unit | `cargo test body_parser::tests::parse_mixed_metrics` | Wave 0 |
| DRV-01 | Derived metric stored with `source_table: None` | unit | `cargo test model::tests::derived_metric_no_source_table` | Wave 0 |
| DRV-02 | Derived metric expression inlined with base metric expr | unit | `cargo test expand::tests::inline_derived_metric` | Wave 0 |
| DRV-02 | Facts inlined first, then derived metrics composed | unit | `cargo test expand::tests::facts_then_derived` | Wave 0 |
| DRV-03 | Stacked derived metrics resolve in topological order | unit | `cargo test expand::tests::derived_metric_stacking` | Wave 0 |
| DRV-04 | Derived metric cycle rejection at define time | unit | `cargo test graph::tests::derived_metric_cycle` | Wave 0 |
| DRV-04 | Non-existent metric reference rejection | unit | `cargo test graph::tests::derived_metric_unknown_ref` | Wave 0 |
| DRV-05 | Aggregate function in derived metric rejected | unit | `cargo test graph::tests::derived_metric_has_aggregate` | Wave 0 |
| DRV-05 | Non-aggregate expressions pass validation | unit | `cargo test graph::tests::derived_metric_no_aggregate_ok` | Wave 0 |
| ALL | Derived metrics DDL + query + errors end-to-end | sqllogictest | `just test-sql` (phase30 test) | Wave 0 |
| ALL | Join resolution includes tables needed by derived metrics | unit | `cargo test expand::tests::derived_metric_join_resolution` | Wave 0 |
| ALL | Adversarial derived metric parsing | proptest | `cargo test body_parser::tests::proptest_derived_metric` | Wave 0 |
| ALL | DESCRIBE shows derived metrics alongside base metrics | sqllogictest | `just test-sql` (phase30 test) | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase30_derived_metrics.test` -- end-to-end derived metrics DDL, query, stacking, and error cases
- [ ] Unit tests for mixed qualified/unqualified metric parsing in body_parser.rs
- [ ] Unit tests for derived metric inlining (single-level, multi-level stacking) in expand.rs
- [ ] Unit tests for derived metric cycle detection, unknown reference validation, aggregate function rejection in graph.rs
- [ ] Unit tests for join resolution with derived metrics (source_table: None but referenced metrics need joins)
- [ ] Proptest for derived metric expression substitution edge cases
- [ ] Update TEST_LIST with phase30_derived_metrics.test

## Open Questions

1. **Should derived metrics that reference only a single base metric be allowed?**
   - What we know: Snowflake allows derived metrics that reference a single metric (e.g., `negative_revenue AS -revenue`). Our requirements don't restrict this.
   - What's unclear: Is there value in a derived metric that just wraps one metric with arithmetic?
   - Recommendation: Allow it. No reason to restrict. A simple negation or multiplication is a valid use case.

2. **Should DESCRIBE show derived metrics differently from base metrics?**
   - What we know: Currently DESCRIBE shows a JSON array of all metrics. Derived metrics will have `source_table: null` in the JSON. Snowflake's DESCRIBE SEMANTIC VIEW shows a `table` column that is null for derived metrics.
   - What's unclear: Whether to add a separate column or just rely on `source_table` being null.
   - Recommendation: No change to DESCRIBE schema. Derived metrics appear in the metrics JSON array with `source_table: null`. Users can distinguish them by checking `source_table`. This matches Snowflake's approach.

3. **How should join resolution handle derived metrics that reference metrics from different tables?**
   - What we know: `resolve_joins_pkfk` uses `source_table` to determine needed joins. Derived metrics have `source_table: None`.
   - What's unclear: Whether to resolve joins transitively through derived metric references or whether to simply include all joins.
   - Recommendation: When a derived metric is requested, find all base metrics it references (directly or transitively through stacked derived metrics), collect their `source_table` values, and include those in the needed-aliases set. This is the minimal set of joins needed for correct expansion.

## Sources

### Primary (HIGH confidence)
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- Derived metric syntax: "omit table_alias from the name"
- [Snowflake Using SQL for Semantic Views](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- Derived metric examples: stacking, expression rules (no aggregation, no window functions)
- [Snowflake Derived Metrics Release Note](https://docs.snowflake.com/en/release-notes/2025/other/2025-09-30-semantic-view-derived-metrics) -- Feature announcement Sep 30, 2025
- [Snowflake Semantic View Validation Rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- "A metric that is not a derived metric must use an aggregate function"
- Project source code: `src/model.rs` (Metric struct with optional source_table), `src/body_parser.rs` (parse_qualified_entries), `src/expand.rs` (inline_facts, toposort_facts, replace_word_boundary, resolve_joins_pkfk), `src/graph.rs` (validate_facts, find_fact_references, build_fact_dag, check_fact_cycles), `src/ddl/define.rs` (bind validation pipeline), `src/ddl/describe.rs` (DESCRIBE output)

### Secondary (MEDIUM confidence)
- [Snowflake YAML Spec for Semantic Views](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- Derived metrics as "view-level metrics not tied to a specific table"
- Phase 29 RESEARCH.md -- Fact inlining patterns, word-boundary substitution, topological sort

### Tertiary (LOW confidence)
- None -- all findings verified against official Snowflake docs or direct codebase analysis.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all patterns exist in codebase
- Architecture: HIGH -- direct analysis of body_parser.rs, expand.rs, graph.rs, define.rs; all patterns are extensions of proven Phase 29 code
- Pitfalls: HIGH -- identified through tracing the expansion pipeline end-to-end; join resolution gap is the most subtle and critical finding
- Snowflake alignment: HIGH -- verified against official docs; derived metric feature launched Sep 2025

**Research date:** 2026-03-14
**Valid until:** 2026-04-14 (stable -- no external dependency changes expected)
