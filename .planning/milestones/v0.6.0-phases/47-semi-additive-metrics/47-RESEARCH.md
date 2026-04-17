# Phase 47: Semi-Additive Metrics - Research

**Researched:** 2026-04-12
**Domain:** Semi-additive metric expansion (NON ADDITIVE BY), DDL parsing, CTE-based SQL generation
**Confidence:** HIGH

## Summary

Semi-additive metrics are the most structurally significant expansion pipeline change in v0.6.0. The feature requires: (1) new model types for `NonAdditiveDim` with sort order/nulls, (2) body parser extension to recognize `NON ADDITIVE BY (dim [ASC|DESC] [NULLS FIRST|LAST])` between the metric name and `AS` keyword, (3) a new `expand/semi_additive.rs` submodule that generates CTE-based SQL with `ROW_NUMBER() OVER (PARTITION BY ... ORDER BY ...)` for snapshot selection before aggregation, and (4) updates to fan trap detection, DESCRIBE, SHOW, GET_DDL, and test infrastructure.

The critical design decision -- already locked in STATE.md -- is to use `ROW_NUMBER()` instead of `LAST_VALUE IGNORE NULLS` due to a DuckDB LTS crash bug. The mixed-metric strategy (regular + semi-additive in one query) follows Strategy A from architecture research: separate CTE per semi-additive metric group, joined with regular metric aggregation on shared dimensions. When all non-additive dimensions are present in the query, the metric is treated as effectively regular (no CTE needed) -- this matches Snowflake semantics where "when the non-additive dimension is included in the query, the metric is calculated as a standard additive metric."

**Primary recommendation:** Implement as a new `expand/semi_additive.rs` submodule that wraps `expand()` output with CTE-based snapshot selection, keeping the existing single-pass expansion path untouched for queries with no semi-additive metrics.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SEMI-01 | User can declare NON ADDITIVE BY (dimension [ASC|DESC]) on a metric in DDL and the view stores successfully | Model types (NonAdditiveDim, SortOrder, NullsOrder), body_parser extension, serde backward compat |
| SEMI-02 | Semi-additive metrics use CTE-based snapshot selection (ROW_NUMBER) before aggregation at query time | New expand/semi_additive.rs submodule, ROW_NUMBER CTE pattern, locked decision to avoid LAST_VALUE |
| SEMI-03 | Queries mixing regular and semi-additive metrics produce correct results | Strategy A: separate CTE for semi-additive snapshot + outer join with regular aggregation |
| SEMI-04 | Semi-additive metrics interact correctly with fan trap detection (no false positives or missed fan traps) | Skip fan trap for semi-additive metrics (snapshot selection handles the dedup inherently) |
| SEMI-05 | Semi-additive metrics work with multi-table JOINs and USING RELATIONSHIPS | CTE inner query includes full FROM/JOIN clause; USING scoped aliases carried through |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- If in doubt about SQL syntax or behaviour, refer to what Snowflake semantic views does
- Quality gate: `just test-all` (Rust unit + proptest + sqllogictest + DuckLake CI)
- `cargo test` alone is incomplete -- sqllogictest covers integration paths
- `just test-sql` requires fresh `just build` to pick up code changes

## Standard Stack

### Core

No new libraries required. This phase is pure Rust code changes within the existing codebase.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde/serde_json | 1.x | Serialize/deserialize NonAdditiveDim, SortOrder, NullsOrder | Already used for all model types [VERIFIED: codebase] |
| proptest | 1.x | Property-based tests for semi-additive expansion | Already used for 36 output type PBTs [VERIFIED: codebase] |

### Supporting

No additional libraries needed. DuckDB natively supports all required SQL constructs:
- `ROW_NUMBER() OVER (PARTITION BY ... ORDER BY ...)` [CITED: https://duckdb.org/docs/stable/sql/query_syntax/orderby.md]
- CTEs via `WITH ... AS (...)` [VERIFIED: used in existing sqllogictest files]
- `NULLS FIRST` / `NULLS LAST` in ORDER BY [CITED: https://duckdb.org/docs/stable/sql/query_syntax/orderby.md]

## Architecture Patterns

### Recommended Changes

```
src/
  model.rs              # +NonAdditiveDim, SortOrder, NullsOrder structs/enums
  body_parser.rs         # +parse NON ADDITIVE BY clause in metric entries
  render_ddl.rs          # +emit NON ADDITIVE BY in GET_DDL output
  expand/
    mod.rs               # +semi_additive submodule declaration
    semi_additive.rs     # NEW: CTE generation for snapshot selection
    sql_gen.rs           # expand() branches on semi-additive presence
    fan_trap.rs          # skip fan trap check for semi-additive metrics
    types.rs             # (no change expected)
    test_helpers.rs      # +with_non_additive_by() builder method
  ddl/
    describe.rs          # +NON_ADDITIVE_BY property row for metrics
    show_dims_for_metric.rs  # consider semi-additive dim interaction
```

### Pattern 1: Model Extension with Backward-Compatible Serde

**What:** Add `non_additive_by: Vec<NonAdditiveDim>` to `Metric` struct with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
**When to use:** Every new field on any stored model type.
**Example:**
```rust
// Source: existing codebase pattern (model.rs lines 78-79 using_relationships)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct NonAdditiveDim {
    pub dimension: String,
    #[serde(default)]
    pub order: SortOrder,
    #[serde(default)]
    pub nulls: NullsOrder,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum NullsOrder {
    #[default]
    Last,
    First,
}
```

**Critical:** The `SortOrder` default is `Asc` to match Snowflake's documented default. The `NullsOrder` default is `Last` to match DuckDB's ASC default behavior. [CITED: https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view]

### Pattern 2: CTE-Based Snapshot Selection (ROW_NUMBER)

**What:** Generate a CTE that selects the "latest snapshot" row per partition using ROW_NUMBER before the outer aggregation query.
**When to use:** When any resolved metric has non-empty `non_additive_by` AND at least one NA dim is NOT in the queried dimensions.
**Key semantic:** When ALL NA dims of a metric ARE in the queried dimensions, that metric is treated as effectively regular for that query (Snowflake semantics). The CTE path is only entered when at least one semi-additive metric has an absent NA dim.
**Example:** For `SUM(balance) NON ADDITIVE BY (date_dim DESC)` with dimensions `[customer_id]` (date_dim NOT queried):

```sql
-- Non-additive dim not requested: snapshot selection active
WITH __sv_snapshot AS (
    SELECT
        c.name AS "customer_id",
        a.balance AS "__sv_semi_balance",
        ROW_NUMBER() OVER (
            PARTITION BY c.name
            ORDER BY a.report_date DESC NULLS FIRST
        ) AS __sv_rn
    FROM "accounts" AS "a"
    LEFT JOIN "customers" AS "c" ON "a"."customer_id" = "c"."id"
)
SELECT
    "customer_id",
    SUM("__sv_semi_balance") AS "total_balance"
FROM __sv_snapshot
WHERE __sv_rn = 1
GROUP BY 1
```

When `date_dim` IS in the query (e.g., dims `[customer_id, date_dim]`):
The metric is treated as effectively regular -- no CTE is generated. The standard expansion path produces a regular `SELECT ... GROUP BY` query, and each (customer, date) group naturally has the correct balance value.

### Pattern 3: Mixed Regular + Semi-Additive Metrics (Strategy A)

**What:** When a query mixes regular and semi-additive metrics, generate separate CTEs.
**When to use:** Query requests both regular aggregates and semi-additive aggregates.
**Example:** For `SUM(amount)` (regular) + `SUM(balance) NON ADDITIVE BY (date_dim)` with dims `[customer_id]`:

```sql
WITH __sv_snapshot AS (
    SELECT
        c.name AS "customer_id",
        a.balance AS "__sv_semi_balance",
        ROW_NUMBER() OVER (
            PARTITION BY c.name
            ORDER BY a.report_date DESC NULLS FIRST
        ) AS __sv_rn
    FROM "accounts" AS "a"
    LEFT JOIN "customers" AS "c" ON "a"."customer_id" = "c"."id"
),
__sv_semi AS (
    SELECT
        "customer_id",
        SUM("__sv_semi_balance") AS "total_balance"
    FROM __sv_snapshot
    WHERE __sv_rn = 1
    GROUP BY 1
),
__sv_regular AS (
    SELECT
        c.name AS "customer_id",
        SUM(o.amount) AS "total_revenue"
    FROM "orders" AS "o"
    LEFT JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
    GROUP BY 1
)
SELECT
    COALESCE(__sv_regular."customer_id", __sv_semi."customer_id") AS "customer_id",
    __sv_regular."total_revenue",
    __sv_semi."total_balance"
FROM __sv_regular
FULL OUTER JOIN __sv_semi ON __sv_regular."customer_id" = __sv_semi."customer_id"
```

**Complexity note:** This is the hardest case. An alternative simpler approach for single-table views: compute everything in one CTE with ROW_NUMBER, then the outer query uses `SUM(CASE WHEN __sv_rn = 1 THEN balance END)` for semi-additive and `SUM(amount)` for regular. This avoids the JOIN but only works when all metrics share the same FROM/JOIN clause.

**Recommended simplification:** For the initial implementation, when semi-additive and regular metrics share the same table lineage (common case), use the single-CTE approach with conditional aggregation:

```sql
WITH __sv_snapshot AS (
    SELECT
        customer_id,
        amount,
        balance,
        ROW_NUMBER() OVER (
            PARTITION BY customer_id
            ORDER BY date_dim DESC NULLS FIRST
        ) AS __sv_rn
    FROM "accounts"
)
SELECT
    "customer_id",
    SUM(amount) AS "total_revenue",
    SUM(CASE WHEN __sv_rn = 1 THEN balance END) AS "total_balance"
FROM __sv_snapshot
GROUP BY 1
```

This is simpler, produces correct results, and avoids the FULL OUTER JOIN complexity. When metrics come from different table lineages (multi-table JOINs), the multi-CTE approach is needed.

### Pattern 4: DDL Parse Position

**What:** `NON ADDITIVE BY (...)` appears between USING and AS in the metric entry syntax.
**Syntax (Snowflake-aligned):**
```
[PRIVATE] alias.name [USING (...)] [NON ADDITIVE BY (...)] AS expr [COMMENT = '...'] [WITH SYNONYMS = (...)]
```
**Parse flow in `parse_single_metric_entry`:**
1. Strip leading PRIVATE/PUBLIC
2. Find AS keyword
3. Split: before_as | AS | expr+annotations
4. In before_as: find USING -> find NON ADDITIVE BY -> remaining is name_portion
5. Parse NON ADDITIVE BY parenthesized list: `dim [ASC|DESC] [NULLS FIRST|LAST], ...`

[CITED: https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view]

### Anti-Patterns to Avoid

- **Inline branching in expand():** Do NOT add if/else branches inside the existing `expand()` function for semi-additive logic. Use a wrapper/dispatcher pattern that calls the existing path for regular metrics and the new CTE path for semi-additive metrics.
- **LAST_VALUE IGNORE NULLS:** Locked decision (STATE.md) -- use ROW_NUMBER() instead. DuckDB LTS 1.4.x crashes on all-NULL partitions with LAST_VALUE IGNORE NULLS. [VERIFIED: STATE.md line 65]
- **Skipping serde(default):** Every new field on Metric must have `#[serde(default)]` and appropriate `skip_serializing_if`. A single missing annotation makes all existing stored views unloadable. [VERIFIED: PITFALLS.md C4]
- **NON ADDITIVE BY on derived metrics:** Snowflake syntax requires `<table_alias>.<metric>` before NON ADDITIVE BY, meaning it's only for qualified (base) metrics. Derived metrics (no dot) should produce a ParseError if NON ADDITIVE BY is present. [CITED: https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Snapshot selection | Custom window function logic | ROW_NUMBER() OVER (...) | Standard SQL, crash-safe, well-tested in DuckDB |
| Sort order parsing | Ad-hoc string matching | Keyword detection with word boundaries | Existing `find_keyword_ci` handles this pattern |
| Dimension validation | Runtime-only checks | Define-time cross-reference against parsed dimensions | Catch errors early, consistent with existing PK/FK validation |
| CTE generation | String concatenation inline | Dedicated `semi_additive.rs` module | Testable in isolation, follows expand/ module pattern |

## Common Pitfalls

### Pitfall 1: Incorrect Aggregation Scope for Mixed Metrics

**What goes wrong:** A query with both `SUM(amount)` (regular) and `SUM(balance) NON ADDITIVE BY (date_dim)` produces the same value for both -- the semi-additive logic is not activating.
**Why it happens:** The CTE wrapping was applied but the regular metric also reads from the filtered CTE (WHERE __sv_rn = 1), losing non-snapshot rows.
**How to avoid:** Regular metrics must aggregate over ALL rows; semi-additive metrics aggregate only over snapshot-selected rows. Either use conditional aggregation (`SUM(CASE WHEN __sv_rn = 1 THEN balance END)`) in the single-CTE approach, or use separate CTEs with a JOIN.
**Warning signs:** Regular metric values change when a semi-additive metric is added to the same query.

### Pitfall 2: NON ADDITIVE BY Dimension References Not Validated at Define Time

**What goes wrong:** User writes `NON ADDITIVE BY (nonexistent_dim)` and it stores successfully, but fails at query time with a confusing error.
**Why it happens:** The parser accepts dimension names without cross-referencing against the parsed dimensions list.
**How to avoid:** After parsing the full body (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS), validate each `non_additive_by` dimension name against the view's dimension list. Use the existing `suggest_closest` pattern for "did you mean" suggestions.
**Warning signs:** Any test where NON ADDITIVE BY references a dimension name that doesn't exist should produce a clear define-time error.

### Pitfall 3: Fan Trap False Positives on Semi-Additive Metrics

**What goes wrong:** A semi-additive metric that correctly handles snapshot aggregation is blocked by fan trap detection, even though the ROW_NUMBER CTE prevents fan-out.
**Why it happens:** The existing `check_fan_traps` function checks ALL metrics, including semi-additive ones that have their own dedup mechanism.
**How to avoid:** Skip fan trap checking for metrics with non-empty `non_additive_by`. The ROW_NUMBER CTE inherently handles the one-to-many boundary by selecting one row per partition.
**Warning signs:** A semi-additive metric query against a multi-table view returns a FanTrap error instead of results.

### Pitfall 4: DuckDB NULLS Order Default Divergence

**What goes wrong:** Generated SQL omits NULLS FIRST/LAST, relying on DuckDB's default. If the default changes or differs from Snowflake's, snapshot selection picks the wrong row.
**Why it happens:** DuckDB's default NULLS order has changed between versions (older: NULLS FIRST for ASC; current: NULLS LAST for ASC, matching PostgreSQL).
**How to avoid:** ALWAYS emit explicit NULLS FIRST or NULLS LAST in the generated ORDER BY clause. Never rely on implicit defaults.
**Warning signs:** Semi-additive results differ between DuckDB versions or between DuckDB and Snowflake.

### Pitfall 5: MetricEntry Tuple Width Explosion

**What goes wrong:** The `MetricEntry` type alias is already a 7-element tuple. Adding `non_additive_by` makes it 8+ elements, which is fragile and error-prone.
**Why it happens:** The original `parse_metrics_clause` used a positional tuple for parsed metric data.
**How to avoid:** Consider converting `MetricEntry` from a type alias tuple to a named struct (e.g., `ParsedMetricEntry`). This makes the code self-documenting and reduces positional errors. This is a small refactor that pays for itself immediately.
**Warning signs:** Tuple field access by position (`.0`, `.1`, etc.) becomes impossible to read.

### Pitfall 6: ROW_NUMBER with All-NULL Metric Columns

**What goes wrong:** When all values of the metric column are NULL in a partition, ROW_NUMBER still selects a row (row with rn=1), and SUM(NULL) correctly returns NULL. This is the correct behavior.
**Why it happens:** ROW_NUMBER assigns row numbers regardless of NULL values -- this is exactly why it was chosen over LAST_VALUE.
**How to avoid:** This is actually correct behavior. Test it explicitly to prove correctness.
**Warning signs:** None -- this is a non-issue with ROW_NUMBER, but add a test to document it.

## Code Examples

### Model Types

```rust
// Source: architecture research + Snowflake DDL reference
/// Sort order for NON ADDITIVE BY dimension ordering.
/// Default: Asc (matches Snowflake default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Asc)
    }
}

/// NULLS placement for NON ADDITIVE BY dimension ordering.
/// Default: Last (matches DuckDB ASC default and Snowflake ASC default).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum NullsOrder {
    #[default]
    Last,
    First,
}

impl NullsOrder {
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Last)
    }
}

/// A dimension reference in a NON ADDITIVE BY clause.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct NonAdditiveDim {
    pub dimension: String,
    #[serde(default, skip_serializing_if = "SortOrder::is_default")]
    pub order: SortOrder,
    #[serde(default, skip_serializing_if = "NullsOrder::is_default")]
    pub nulls: NullsOrder,
}

// In Metric struct:
pub struct Metric {
    // ... existing fields ...
    /// Dimensions this metric is non-additive by (snapshot aggregation).
    /// When non-empty, expansion uses ROW_NUMBER CTE for snapshot selection.
    /// Old stored JSON without this field deserializes with empty Vec.
    /// Not serialized when empty to preserve backward-compatible JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_additive_by: Vec<NonAdditiveDim>,
}
```

### Parser Extension

```rust
// Source: existing parse_single_metric_entry pattern in body_parser.rs
// After extracting USING clause from before_as, check for NON ADDITIVE BY:
//
// The parse order in before_as is:
//   [PRIVATE] alias.name [USING (...)] [NON ADDITIVE BY (...)]
//
// After stripping USING, the remaining before_as portion is checked for
// "NON ADDITIVE BY" keyword sequence. If found, extract the parenthesized
// dimension list with optional ASC/DESC and NULLS FIRST/LAST modifiers.

fn parse_non_additive_by(text: &str, entry_offset: usize) -> Result<Vec<NonAdditiveDim>, ParseError> {
    // text is the content inside parentheses, e.g.:
    // "year_dim DESC NULLS FIRST, month_dim, day_dim ASC"
    let entries = split_at_depth0_commas(text);
    let mut result = Vec::new();
    for (start, entry) in entries {
        let entry = entry.trim();
        let upper = entry.to_ascii_uppercase();
        let parts: Vec<&str> = upper.split_whitespace().collect();
        let dim_name = entry.split_whitespace().next().unwrap_or("").to_string();
        if dim_name.is_empty() {
            return Err(ParseError {
                message: "Empty dimension in NON ADDITIVE BY clause".to_string(),
                position: Some(entry_offset + start),
            });
        }
        let mut order = SortOrder::Asc;
        let mut nulls = NullsOrder::Last; // default for ASC
        // Parse optional ASC/DESC
        // Parse optional NULLS FIRST/LAST
        // ... (standard keyword scanning)
        result.push(NonAdditiveDim { dimension: dim_name, order, nulls });
    }
    Ok(result)
}
```

### CTE Generation (semi_additive.rs)

```rust
// Source: architecture research Pattern 2 + locked decision (STATE.md)
/// Generate CTE-based expansion SQL for queries containing semi-additive metrics.
///
/// The generated SQL has this structure:
/// ```sql
/// WITH __sv_snapshot AS (
///     SELECT dim_exprs, raw_metric_exprs,
///            ROW_NUMBER() OVER (
///                PARTITION BY non_na_dims
///                ORDER BY na_dim1 DESC NULLS FIRST, ...
///            ) AS __sv_rn
///     FROM base LEFT JOIN ...
/// )
/// SELECT dims,
///        agg(CASE WHEN __sv_rn = 1 THEN raw_val END) AS semi_metric,
///        agg(regular_metric_expr) AS regular_metric
/// FROM __sv_snapshot
/// GROUP BY 1, 2, ...
/// ```
pub(super) fn expand_semi_additive(
    view_name: &str,
    def: &SemanticViewDefinition,
    req: &QueryRequest,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
    resolved_exprs: &HashMap<String, String>,
    dim_scoped_aliases: &[Option<String>],
) -> Result<String, ExpandError> {
    // Implementation here
    todo!()
}
```

### Test Helper Extension

```rust
// Source: existing test_helpers.rs pattern
fn with_non_additive_by(
    mut self,
    metric_name: &str,
    dims: &[(&str, SortOrder, NullsOrder)],
) -> Self {
    if let Some(m) = self.metrics.iter_mut().find(|m| m.name == metric_name) {
        m.non_additive_by = dims
            .iter()
            .map(|(dim, order, nulls)| NonAdditiveDim {
                dimension: dim.to_string(),
                order: *order,
                nulls: *nulls,
            })
            .collect();
    }
    self
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| LAST_VALUE IGNORE NULLS | ROW_NUMBER() CTE | Locked in v0.6.0 planning | Avoids DuckDB LTS crash on all-NULL partitions |
| Single-pass SELECT...GROUP BY | CTE wrapper for semi-additive | Phase 47 | First CTE usage in expansion pipeline |
| All metrics treated identically | Metrics classified by additivity | Phase 47 | Expansion path branches based on metric type |

**Deprecated/outdated:**
- LAST_VALUE approach: explicitly rejected due to DuckDB crash bug (#20136). Do not use.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | NON ADDITIVE BY is only valid on qualified metrics (alias.name), not derived metrics | Architecture Patterns / Anti-Patterns | If Snowflake allows it on derived, we'd need to support it; LOW risk since derived metrics have no source_table and the NON ADDITIVE BY dimension reference implies table context |
| A2 | Single-CTE with conditional aggregation is correct for mixed metrics on same table lineage | Pattern 3 | If conditional aggregation produces wrong results with complex JOINs, fallback to multi-CTE approach; MEDIUM risk, needs thorough testing |
| A3 | Fan trap detection should be skipped entirely for semi-additive metrics | Pitfall 3 | If some fan trap scenarios still produce incorrect results after snapshot selection, we'd need partial checking; LOW risk since ROW_NUMBER dedup addresses the core fan-out issue |
| A4 | Default NullsOrder should be Last (matching DuckDB ASC default) | Model Types | If Snowflake defaults to something else, results could differ; LOW risk since we always emit explicit NULLS in SQL |

## Open Questions (RESOLVED)

1. **Multiple semi-additive metrics with different NON ADDITIVE BY dimensions**
   - What we know: Each metric can have a different set of non-additive dimensions. The CTE must handle heterogeneous partitioning.
   - What's unclear: When two semi-additive metrics have different NON ADDITIVE BY dims (e.g., metric A is non-additive by date_dim, metric B is non-additive by month_dim), do they share a CTE or need separate CTEs?
   - Recommendation: Start with one ROW_NUMBER column per unique NON ADDITIVE BY dimension set. If metrics A and B have the same NA dims, they share a CTE/RN column. If different, they need separate RN columns. The conditional aggregation approach handles this naturally: `SUM(CASE WHEN __sv_rn_1 = 1 THEN a END)`, `SUM(CASE WHEN __sv_rn_2 = 1 THEN b END)`.
   - RESOLVED: Adopted per recommendation. `collect_na_groups` in `semi_additive.rs` groups metrics by NA dim set, producing one `__sv_rn_N` column per unique set.

2. **Derived metrics referencing semi-additive base metrics**
   - What we know: A derived metric like `profit AS revenue - cost` references other metrics by name. If `revenue` is semi-additive, the derived metric's expansion must apply snapshot selection to `revenue`'s base expression.
   - What's unclear: Should `profit` inherit the semi-additive behavior?
   - Recommendation: The inline_derived_metrics resolution already replaces metric names with their expressions. The semi-additive behavior is applied at the SQL generation level, not the expression level. A derived metric that references a semi-additive base should work correctly because the CTE applies ROW_NUMBER to the raw data, and the derived expression operates on the snapshot-selected rows.
   - RESOLVED: Adopted per recommendation. Derived metrics operate on snapshot-selected rows via the CTE. NON ADDITIVE BY is disallowed on derived metrics (no dot prefix) at parse time -- this is enforced in `parse_single_metric_entry`.

3. **SHOW SEMANTIC DIMENSIONS FOR METRIC interaction**
   - What we know: This command shows which dimensions are safe to query with a given metric (fan-trap-filtered).
   - What's unclear: Should NON ADDITIVE BY dimensions be marked differently in the output (e.g., a new column)?
   - Recommendation: No schema change needed -- NON ADDITIVE BY dimensions are valid dimensions for the metric. The fan trap check will skip semi-additive metrics, so all dimensions are potentially safe.
   - RESOLVED: Adopted per recommendation. No schema change to SHOW output. Fan trap check skips semi-additive metrics, so all dimensions appear as safe.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust test (cargo test) + sqllogictest |
| Config file | `Cargo.toml`, `test/sql/TEST_LIST` |
| Quick run command | `cargo test semi_additive` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SEMI-01 | DDL parsing + storage of NON ADDITIVE BY | unit | `cargo test parse_metrics -- non_additive` | No -- Wave 0 |
| SEMI-01 | Backward-compat deserialization | unit | `cargo test backward_compat` | No -- Wave 0 |
| SEMI-01 | render_ddl round-trip with NON ADDITIVE BY | unit | `cargo test render_ddl -- non_additive` | No -- Wave 0 |
| SEMI-02 | CTE snapshot selection produces correct SQL | unit | `cargo test semi_additive -- expand` | No -- Wave 0 |
| SEMI-02 | Expansion with all NA dims in query | unit | `cargo test semi_additive -- all_na_dims` | No -- Wave 0 |
| SEMI-02 | Expansion with no NA dims in query | unit | `cargo test semi_additive -- no_na_dims` | No -- Wave 0 |
| SEMI-03 | Mixed regular + semi-additive query | unit + slt | `cargo test semi_additive -- mixed` | No -- Wave 0 |
| SEMI-04 | Fan trap skipped for semi-additive metrics | unit | `cargo test fan_trap -- semi_additive` | No -- Wave 0 |
| SEMI-05 | Multi-table JOIN + USING with semi-additive | unit + slt | `cargo test semi_additive -- join` | No -- Wave 0 |
| SEMI-02 | All-NULL partition with ROW_NUMBER | unit + slt | `cargo test semi_additive -- null` | No -- Wave 0 |
| SEMI-01 | NON ADDITIVE BY dim validation at define time | unit | `cargo test parse -- non_additive_validation` | No -- Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps

- [ ] `test/sql/phase47_semi_additive.test` -- sqllogictest for end-to-end semi-additive queries
- [ ] Unit tests in `src/expand/semi_additive.rs` -- CTE generation correctness
- [ ] Unit tests in `src/body_parser.rs` -- NON ADDITIVE BY parsing
- [ ] Unit tests in `src/render_ddl.rs` -- GET_DDL with NON ADDITIVE BY
- [ ] Backward-compat test in `src/model.rs` -- deserialize pre-Phase-47 JSON
- [ ] PropTest for semi-additive expansion -- varied partition sizes, NULL rates

## Security Domain

Security enforcement is not explicitly disabled in config.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A |
| V5 Input Validation | yes | Body parser validates dimension references at define time; malicious dimension names rejected by existing identifier quoting |
| V6 Cryptography | no | N/A |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via dimension names in NON ADDITIVE BY | Tampering | All identifiers quoted via `quote_ident()` in generated SQL [VERIFIED: codebase] |
| Denial of service via large NON ADDITIVE BY list | Denial of Service | DuckDB handles query complexity limits; no special mitigation needed |

## Sources

### Primary (HIGH confidence)
- [Codebase grep] -- src/model.rs, src/expand/sql_gen.rs, src/body_parser.rs, src/expand/fan_trap.rs, src/render_ddl.rs, src/expand/test_helpers.rs
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- Full BNF syntax for NON ADDITIVE BY
- [Snowflake semi-additive release note](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- Feature behavior specification
- [Snowflake YAML spec](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- non_additive_dimensions fields, sort_direction, null_order
- [DuckDB ORDER BY docs](https://duckdb.org/docs/stable/sql/query_syntax/orderby.md) -- NULLS FIRST/LAST default behavior

### Secondary (MEDIUM confidence)
- [Architecture research] -- .planning/research/ARCHITECTURE.md sections on semi-additive (Strategy A/B analysis)
- [Pitfalls research] -- .planning/research/PITFALLS.md C1 (expansion path), C2 (LAST_VALUE crash), M5 (fan trap interaction), N1 (dim validation)
- [Stack research] -- .planning/research/STACK.md semi-additive expansion strategy

### Tertiary (LOW confidence)
- None -- all claims verified against codebase or official documentation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new libraries, existing patterns extended
- Architecture: HIGH -- detailed expansion strategy from architecture research + codebase analysis, locked ROW_NUMBER decision
- Pitfalls: HIGH -- all 6 pitfalls verified against codebase patterns and existing research
- Parser changes: HIGH -- follows exact pattern of existing USING clause parsing

**Research date:** 2026-04-12
**Valid until:** 2026-05-12 (stable domain, no external dependencies)
