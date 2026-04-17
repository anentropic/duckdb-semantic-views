# Phase 46: Wildcard Selection + Queryable FACTS - Research

**Researched:** 2026-04-11
**Domain:** Semantic view expansion pipeline, SQL generation, DuckDB table function parameters
**Confidence:** HIGH

## Summary

Phase 46 adds two independent capabilities to the semantic view expansion pipeline: (1) wildcard selection via `table_alias.*` syntax for dimensions and metrics parameters, and (2) queryable FACTS via a new `facts` named parameter on the `semantic_view()` table function. Both features modify the query interface but have distinct implementation paths -- wildcards operate as pre-expansion name resolution, while fact queries require a fundamentally different SQL generation mode (no GROUP BY, no aggregation).

The wildcard feature is a straightforward pre-processing step: before `expand()` runs, resolve any `alias.*` patterns in the dimensions/metrics lists into the concrete item names scoped to that table alias, filtering out PRIVATE items. The fact query feature is more involved: it requires a new `facts` field on `QueryRequest`, a new SQL generation path in `expand/sql_gen.rs` that produces `SELECT` without aggregation, mutual exclusion enforcement with metrics, and path validation reusing the existing `fan_trap.rs` ancestor chain infrastructure.

**Primary recommendation:** Implement wildcard expansion as a pre-processing step in `table_function.rs::bind()` before constructing the `QueryRequest`, and implement fact queries as a new branch in `expand/sql_gen.rs::expand()` that produces unaggregated SQL. Add `facts` as a named parameter to both `semantic_view()` and `explain_semantic_view()` table functions.

## Project Constraints (from CLAUDE.md)

- Quality gate: `just test-all` must pass (cargo test + sqllogictest + DuckLake CI)
- `cargo test` alone is incomplete -- sqllogictest covers integration paths
- `just test-sql` requires `just build` first
- When in doubt about SQL syntax, refer to Snowflake semantic views behavior

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| WILD-01 | User can use table_alias.* to select all dimensions scoped to that table | Wildcard expansion pre-processing in bind(), resolving alias.* to concrete dimension names via source_table matching |
| WILD-02 | User can use table_alias.* to select all metrics scoped to that table | Same wildcard expansion for metrics, matching source_table field |
| WILD-03 | Wildcard expansion respects PRIVATE visibility (PRIVATE items excluded) | Filter by `access != AccessModifier::Private` during expansion; dimensions have no access field so always included |
| FACT-01 | User can query facts via semantic_view('v', facts := ['f1', 'f2'], dimensions := ['d1']) | New `facts` named parameter on VTab, new `facts` field on QueryRequest, new expansion path in sql_gen.rs |
| FACT-02 | Fact queries return row-level (unaggregated) results | New SQL generation branch: no GROUP BY, no aggregation -- SELECT with raw fact expressions |
| FACT-03 | Facts and metrics cannot be combined in the same query (blocking error) | New ExpandError variant `FactsMetricsMutualExclusion`, checked early in expand() |
| FACT-04 | Fact queries with dimensions require all referenced objects from the same logical table path | Reuse RelationshipGraph + ancestors_to_root from fan_trap.rs for path validation |
</phase_requirements>

## Architecture Patterns

### Current Expansion Pipeline (pre-Phase 46)

```
bind() → extract params → lookup definition → expand(view, def, req) → SQL string
                                                    │
                                                    ├── validate (empty, unknown, duplicate, private)
                                                    ├── resolve dims/metrics
                                                    ├── inline facts into metric expressions
                                                    ├── check fan traps
                                                    ├── build SELECT (dims + aggregate metrics)
                                                    ├── build FROM + JOINs
                                                    └── build GROUP BY (ordinal positions)
```

### Phase 46 Modified Pipeline

```
bind() → extract params (dims, metrics, facts)
      → WILDCARD EXPANSION (new: resolve alias.* → concrete names)
      → lookup definition
      → expand(view, def, req)
              │
              ├── MUTUAL EXCLUSION CHECK (new: facts + metrics → error)
              ├── validate (empty request now includes facts)
              │
              ├─── [metrics mode] existing path: GROUP BY + aggregation
              │
              └─── [facts mode] NEW PATH:
                    ├── resolve requested facts + dims
                    ├── check PRIVATE access on facts
                    ├── validate table path constraint (FACT-04)
                    ├── inline fact dependencies (toposort)
                    ├── build SELECT (dims + raw fact exprs, NO aggregation)
                    ├── build FROM + JOINs
                    └── NO GROUP BY
```

### Recommended Project Structure Changes

```
src/
├── expand/
│   ├── sql_gen.rs      # Modified: new expand_facts() path or branch in expand()
│   ├── types.rs        # Modified: QueryRequest gains `facts` field, new ExpandError variants
│   ├── facts.rs        # Modified: may need fact resolution helpers
│   └── fan_trap.rs     # Reused: ancestors_to_root() for FACT-04 path validation
├── query/
│   ├── table_function.rs  # Modified: add `facts` named parameter, wildcard expansion
│   ├── explain.rs         # Modified: add `facts` named parameter, update header
│   └── error.rs           # Modified: update EmptyRequest message
└── model.rs            # No changes needed
```

### Pattern 1: Wildcard Expansion (Pre-Processing)

**What:** Resolve `table_alias.*` patterns in dimensions/metrics lists before constructing `QueryRequest`
**When to use:** In `bind()` of both `SemanticViewVTab` and `ExplainSemanticViewVTab`

The wildcard expansion happens BEFORE expand() is called. It transforms the user's input list:
```
["region", "o.*", "total_revenue"] → ["region", "order_date", "status", "total_revenue"]
```

Key rules from Snowflake and user assumptions review:
- Only `table_alias.*` is valid -- bare `*` is NOT supported [CITED: docs.snowflake.com/en/sql-reference/constructs/semantic_view]
- PRIVATE metrics/facts are excluded from wildcard expansion [VERIFIED: user assumptions review]
- Dimensions have no access field and are always included [VERIFIED: src/model.rs - Dimension has no `access` field]
- Wildcard must match a valid table alias in the definition [VERIFIED: src/model.rs - TableRef has `alias` field]

```rust
// Source: codebase analysis
fn expand_wildcards(
    items: &[String],
    def: &SemanticViewDefinition,
    item_type: WildcardItemType, // Dimension or Metric
) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for item in items {
        if item.ends_with(".*") {
            let alias = &item[..item.len() - 2];
            // Verify alias exists in tables
            let alias_exists = def.tables.iter()
                .any(|t| t.alias.eq_ignore_ascii_case(alias));
            if !alias_exists {
                return Err(format!("unknown table alias '{alias}' in wildcard '{item}'"));
            }
            // Collect matching items
            match item_type {
                WildcardItemType::Dimension => {
                    for dim in &def.dimensions {
                        if dim.source_table.as_deref()
                            .is_some_and(|st| st.eq_ignore_ascii_case(alias))
                        {
                            // Dimensions have no access modifier -- always included
                            result.push(dim.name.clone());
                        }
                    }
                }
                WildcardItemType::Metric => {
                    for met in &def.metrics {
                        if met.source_table.as_deref()
                            .is_some_and(|st| st.eq_ignore_ascii_case(alias))
                            && met.access != AccessModifier::Private
                        {
                            result.push(met.name.clone());
                        }
                    }
                }
            }
        } else {
            result.push(item.clone());
        }
    }
    Ok(result)
}
```

### Pattern 2: Fact Query SQL Generation

**What:** Generate unaggregated SQL for fact queries
**When to use:** When `QueryRequest.facts` is non-empty

```rust
// Source: codebase analysis of expand/sql_gen.rs patterns
// Fact queries produce:
//   SELECT
//       dim_expr AS "dim_name",
//       fact_expr AS "fact_name"
//   FROM "base_table" AS "alias"
//   LEFT JOIN ...
//
// No GROUP BY. No aggregation. Raw row-level expressions.
```

Key differences from metric queries:
1. SELECT items use fact expressions (with fact-to-fact inlining), NOT aggregate expressions
2. No GROUP BY clause
3. No fan trap check (facts are row-level, no aggregation to inflate)
4. FACT-04 path validation replaces fan trap check
5. When dimensions are present alongside facts: still no GROUP BY (Snowflake behavior)

### Pattern 3: FACT-04 Path Validation

**What:** All facts and dimensions in a fact query must follow the same logical table path
**When to use:** In the fact expansion path, before SQL generation

The Snowflake constraint is: "all facts and dimensions used in the query must be defined in the same logical table." [CITED: docs.snowflake.com/en/user-guide/views-semantic/querying]

For our multi-table model, this translates to: all referenced source_tables must be reachable through a single linear path in the relationship tree (no fan-out). This reuses the `ancestors_to_root()` infrastructure from `fan_trap.rs`.

Implementation approach:
1. Collect all source_table aliases from requested facts + dims
2. Build ancestor chains for each
3. Verify all are on the same root-to-leaf path (each is an ancestor or descendant of every other)

```rust
// Source: fan_trap.rs::ancestors_to_root pattern
fn validate_fact_table_path(
    view_name: &str,
    def: &SemanticViewDefinition,
    fact_tables: &[String],
    dim_tables: &[String],
) -> Result<(), ExpandError> {
    // Build parent map from RelationshipGraph
    let graph = RelationshipGraph::from_definition(def)?;
    let mut parent_map = HashMap::new();
    for (child, parents) in &graph.reverse {
        if let Some(parent) = parents.first() {
            parent_map.insert(child.clone(), parent.clone());
        }
    }
    
    // Collect all unique table aliases
    let all_tables: HashSet<String> = fact_tables.iter()
        .chain(dim_tables.iter())
        .cloned()
        .collect();
    
    // For each pair, verify one is an ancestor of the other
    let tables_vec: Vec<&String> = all_tables.iter().collect();
    for i in 0..tables_vec.len() {
        for j in (i+1)..tables_vec.len() {
            let a_ancestors = ancestors_to_root(tables_vec[i], &parent_map);
            let b_ancestors = ancestors_to_root(tables_vec[j], &parent_map);
            let a_is_ancestor_of_b = b_ancestors.contains(tables_vec[i]);
            let b_is_ancestor_of_a = a_ancestors.contains(tables_vec[j]);
            if !a_is_ancestor_of_b && !b_is_ancestor_of_a {
                return Err(ExpandError::FactPathViolation { ... });
            }
        }
    }
    Ok(())
}
```

### Pattern 4: Type Inference for Fact Query Output

**What:** Use stored `fact.output_type` for column type declarations at bind time
**When to use:** In `SemanticViewVTab::bind()` when processing fact queries

Facts already have DDL-time type inference via `typeof()` queries, stored in `fact.output_type: Option<String>`. [VERIFIED: src/ddl/define.rs lines 275-315, src/model.rs line 109]

For fact queries, the type resolution differs from metric queries:
- Metrics use `column_type_names`/`column_types_inferred` (LIMIT 0 inference at DDL time)
- Facts use per-fact `output_type` field (typeof() inference at DDL time)

The fact `output_type` is a SQL type name string (e.g., "INTEGER", "DECIMAL(10,2)"), not a `duckdb_type` enum. To use it at bind time, we need to either:
1. Parse the type name string to a LogicalTypeHandle, OR
2. Run a LIMIT 0 query on the fact expansion SQL at bind time

Option 2 is simpler and consistent with the existing fallback path in metric bind. For the primary path, we can attempt to map the `output_type` string to a DuckDB type, with LIMIT 0 as fallback.

### Anti-Patterns to Avoid

- **Modifying expand() signature to accept mode enum:** Instead, extend `QueryRequest` with an optional `facts` field. The expand function already dispatches on request contents (dims-only vs metrics-only vs both). Adding a facts branch follows the same pattern. [VERIFIED: src/expand/sql_gen.rs lines 29-34]
- **Running LIMIT 0 for every fact query:** Facts already have DDL-time type inference. Use stored types first, LIMIT 0 only as fallback. [VERIFIED: src/ddl/define.rs lines 275-315]
- **Allowing bare `*` wildcard:** Snowflake explicitly forbids unqualified wildcards. Only `table_alias.*` is valid. [CITED: docs.snowflake.com/en/sql-reference/constructs/semantic_view]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Relationship path validation | Custom tree walker | `ancestors_to_root()` from fan_trap.rs | Already handles parent map construction and LCA finding |
| Fact expression inlining | Custom inline logic | `inline_facts()` and `toposort_facts()` from expand/facts.rs | Already handles fact-to-fact DAG resolution with word boundary safety |
| Table alias validation | Custom alias lookup | `def.tables.iter().find(alias)` pattern used throughout codebase | Consistent with existing code |
| Named parameter extraction | Custom FFI parsing | `extract_list_strings()` from table_function.rs | Already handles LIST(VARCHAR) FFI correctly |

**Key insight:** The fact query expansion reuses most existing infrastructure. The primary new code is the SQL generation branch (no GROUP BY) and the table path validation (reusing graph primitives). Wildcard expansion is purely additive pre-processing.

## Common Pitfalls

### Pitfall 1: Wildcard + Explicit Name Duplicates
**What goes wrong:** User passes `["region", "o.*"]` where `region` is also scoped to table `o`, causing duplicate dimension errors.
**Why it happens:** Wildcard expansion adds names that are already explicitly listed.
**How to avoid:** After wildcard expansion, deduplicate the list before passing to expand(). Use a seen-set to skip already-added names (case-insensitive).
**Warning signs:** `DuplicateDimension` errors when wildcards are combined with explicit names.

### Pitfall 2: Fact Queries Attempting GROUP BY
**What goes wrong:** Fact queries with dimensions produce incorrect SQL if GROUP BY is accidentally included.
**Why it happens:** The existing expand() always adds GROUP BY when both dims and metrics are present. Fact queries need a different code path.
**How to avoid:** The fact expansion path must be a distinct branch that never adds GROUP BY.
**Warning signs:** Fact query results showing aggregated values instead of row-level data.

### Pitfall 3: EmptyRequest Validation Gap
**What goes wrong:** After Phase 46, `expand()` must accept requests with facts but no dims/metrics. The current EmptyRequest check (`dims.is_empty() && metrics.is_empty()`) would reject fact-only queries.
**Why it happens:** The validation predicate doesn't account for the new `facts` field.
**How to avoid:** Update the EmptyRequest check to: `dims.is_empty() && metrics.is_empty() && facts.is_empty()`.
**Warning signs:** Valid fact queries rejected with "specify at least dimensions or metrics" error.

### Pitfall 4: explain_semantic_view Missing facts Parameter
**What goes wrong:** Users cannot EXPLAIN fact queries.
**Why it happens:** The explain function's `named_parameters()` only declares `dimensions` and `metrics`.
**How to avoid:** Add `facts` as a named parameter to `ExplainSemanticViewVTab::named_parameters()` and update the bind logic to pass it through.
**Warning signs:** DuckDB error "Unknown named parameter: facts" on explain_semantic_view.

### Pitfall 5: PRIVATE Filtering Asymmetry in Wildcards
**What goes wrong:** Wildcard expansion for dimensions includes all items (correct -- no access field), but developer might accidentally try to filter dimensions by access.
**Why it happens:** Dimensions have no `access` field on the model struct, unlike metrics and facts.
**How to avoid:** Only filter by access for metrics and facts wildcards. Dimensions are always PUBLIC.
**Warning signs:** Compilation errors trying to access `dim.access`.

### Pitfall 6: Fact Output Type as String vs u32
**What goes wrong:** The metric type inference stores u32 enum values in `column_types_inferred`, but fact type inference stores SQL type name strings in `fact.output_type`. Mixing them up at bind time causes incorrect type declarations.
**Why it happens:** Two different type inference mechanisms were built at different times.
**How to avoid:** For fact queries, use a separate type resolution path that either: (a) maps the SQL type string to a LogicalTypeHandle, or (b) uses LIMIT 0 on the fact expansion SQL.
**Warning signs:** Type mismatches, VARCHAR fallback for all fact columns, or crashes.

### Pitfall 7: Wildcard on Base Table (No source_table)
**What goes wrong:** Some dimensions/metrics might have `source_table: None` (legacy or base table items). Wildcard `base_alias.*` wouldn't match them.
**Why it happens:** Items defined without explicit table qualification have `source_table: None` even if they come from the base table.
**How to avoid:** When wildcard alias matches the first table in the tables list (the base table), also include items where `source_table.is_none()`. Actually -- in the Phase 24+ DDL, ALL dimensions/metrics must be qualified (`alias.name AS expr`), so `source_table` is always `Some(alias)`. Legacy views without tables declarations have no table aliases, so wildcards wouldn't apply. This should not be an issue with current DDL. Add a defensive check just in case.
**Warning signs:** Wildcard on base table alias returns empty results even though dimensions exist.

## Code Examples

### Example 1: QueryRequest Extension

```rust
// Source: codebase analysis -- extend src/expand/types.rs
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
    pub facts: Vec<String>,  // NEW: fact names to query at row level
}
```

### Example 2: New ExpandError Variants

```rust
// Source: codebase analysis -- extend src/expand/types.rs
pub enum ExpandError {
    // ... existing variants ...
    
    /// Facts and metrics cannot be combined in the same query.
    FactsMetricsMutualExclusion { view_name: String },
    
    /// A requested fact name does not exist in the view definition.
    UnknownFact {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    
    /// A duplicate fact name was requested.
    DuplicateFact { view_name: String, name: String },
    
    /// Fact query references objects from incompatible table paths.
    FactPathViolation {
        view_name: String,
        table_a: String,
        table_b: String,
    },
}
```

### Example 3: Fact Query SQL Generation

```rust
// Source: codebase analysis -- SQL output pattern for fact queries
// Input: facts := ['net_price', 'tax_amount'], dimensions := ['region']
// Definition has facts: li.net_price AS li.extended_price * (1 - li.discount)
//                       li.tax_amount AS li.net_price * li.tax_rate

// Generated SQL (no GROUP BY, no aggregation):
// SELECT
//     o.region AS "region",
//     (li.extended_price * (1 - li.discount)) AS "net_price",
//     ((li.extended_price * (1 - li.discount)) * li.tax_rate) AS "tax_amount"
// FROM "orders" AS "o"
// LEFT JOIN "line_items" AS "li" ON "li"."order_id" = "o"."id"
```

### Example 4: Named Parameter Registration

```rust
// Source: codebase analysis -- table_function.rs pattern
fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
    Some(vec![
        (
            "dimensions".to_string(),
            LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar)),
        ),
        (
            "metrics".to_string(),
            LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar)),
        ),
        (
            "facts".to_string(),  // NEW
            LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar)),
        ),
    ])
}
```

### Example 5: Wildcard SQL Usage

```sql
-- User writes:
FROM semantic_view('analytics', dimensions := ['o.*'], metrics := ['li.*']);

-- After wildcard expansion, equivalent to:
FROM semantic_view('analytics', 
    dimensions := ['region', 'order_date', 'status'],
    metrics := ['total_net', 'total_tax']
);
```

### Example 6: Fact Query Usage

```sql
-- Row-level fact query with dimensions:
FROM semantic_view('analytics', 
    facts := ['net_price', 'tax_amount'],
    dimensions := ['region']
);
-- Returns unaggregated rows

-- Error: cannot mix facts and metrics:
FROM semantic_view('analytics', 
    facts := ['net_price'],
    metrics := ['total_net']
);
-- ERROR: semantic view 'analytics': cannot combine facts and metrics in the same query
```

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust unit/proptest) + sqllogictest-rs |
| Config file | test/sql/TEST_LIST (test manifest) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| WILD-01 | table_alias.* expands to all dims for that alias | unit + slt | `cargo test wildcard` / `just test-sql` | No -- Wave 0 |
| WILD-02 | table_alias.* expands to all metrics for that alias | unit + slt | `cargo test wildcard` / `just test-sql` | No -- Wave 0 |
| WILD-03 | Wildcard excludes PRIVATE metrics/facts | unit | `cargo test wildcard_private` | No -- Wave 0 |
| FACT-01 | Query with facts parameter returns results | unit + slt | `cargo test fact_query` / `just test-sql` | No -- Wave 0 |
| FACT-02 | Fact queries return unaggregated results | slt | `just test-sql` | No -- Wave 0 |
| FACT-03 | Facts + metrics produces blocking error | unit + slt | `cargo test facts_metrics_mutual` / `just test-sql` | No -- Wave 0 |
| FACT-04 | Fact path validation for cross-table queries | unit | `cargo test fact_path` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase46_wildcard.test` -- covers WILD-01, WILD-02, WILD-03
- [ ] `test/sql/phase46_fact_query.test` -- covers FACT-01, FACT-02, FACT-03, FACT-04
- [ ] Unit tests in `src/expand/sql_gen.rs` -- wildcard expansion, fact SQL generation
- [ ] Unit tests in `src/expand/types.rs` or new module -- fact path validation
- [ ] Add `test/sql/phase46_wildcard.test` and `test/sql/phase46_fact_query.test` to `test/sql/TEST_LIST`

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | yes (PRIVATE filtering) | AccessModifier enum check |
| V5 Input Validation | yes | Existing expand error handling + new wildcard alias validation |
| V6 Cryptography | no | N/A |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Wildcard bypassing PRIVATE access | Information Disclosure | Filter by AccessModifier::Private before returning expanded names |
| SQL injection via wildcard alias | Tampering | Alias validated against def.tables (allowlist), not interpolated raw |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Fact queries with dimensions produce no GROUP BY (Snowflake behavior) | Architecture Patterns | If Snowflake groups by dims in fact queries, our SQL output would be wrong. Medium risk -- Snowflake docs are slightly ambiguous on this but say "facts are row-level" |
| A2 | FACT-04 "same logical table path" means all tables on same root-to-leaf path | Pattern 3 | If Snowflake means strictly same single table, our path validation is too permissive. Low risk -- user assumptions review confirmed graph reuse |
| A3 | Fact output types can be resolved from stored output_type strings or LIMIT 0 fallback | Pattern 4 | If neither path works reliably, fact columns would fall back to VARCHAR. Low risk -- VARCHAR fallback is acceptable |

## Open Questions (RESOLVED)

1. **Fact queries with no dimensions: SELECT DISTINCT or plain SELECT?**
   - What we know: Metric-only queries use plain SELECT (global aggregate). Dimension-only queries use SELECT DISTINCT. Fact queries are unaggregated.
   - What's unclear: If user queries facts-only (no dimensions), should the output be SELECT DISTINCT or plain SELECT? Snowflake docs don't explicitly address this.
   - Recommendation: Use plain SELECT (no DISTINCT) for fact-only queries. Facts are row-level data; DISTINCT would lose duplicates that might be meaningful. If dimensions are present, still no DISTINCT -- the combination is for filtering/context, not deduplication.
   - RESOLVED: Use plain SELECT (no DISTINCT) for fact-only queries.

2. **Wildcard expansion for facts parameter**
   - What we know: User assumptions review says explain_semantic_view must support `facts` parameter. Wildcards on dimensions and metrics are confirmed.
   - What's unclear: Should `table_alias.*` in the `facts` parameter also expand to all facts for that alias?
   - Recommendation: Yes -- apply the same wildcard expansion to the `facts` parameter for consistency. This is a natural extension and Snowflake docs show `FACTS table.*` syntax.
   - RESOLVED: Yes — apply wildcard expansion to the facts parameter.

## Sources

### Primary (HIGH confidence)
- src/expand/sql_gen.rs -- expansion pipeline, SQL generation patterns
- src/expand/types.rs -- QueryRequest, ExpandError definitions
- src/expand/facts.rs -- fact inlining, toposort, derived metric resolution
- src/expand/fan_trap.rs -- ancestors_to_root(), path validation infrastructure
- src/query/table_function.rs -- VTab bind/func, named parameters, type inference
- src/query/explain.rs -- explain table function, parameter registration
- src/model.rs -- SemanticViewDefinition, Fact, Dimension, Metric, AccessModifier
- src/body_parser.rs -- PRIVATE/PUBLIC parsing, qualified entry parsing
- src/ddl/define.rs -- DDL-time fact type inference via typeof()

### Secondary (MEDIUM confidence)
- [Snowflake SEMANTIC_VIEW syntax](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- wildcard syntax, facts/metrics mutual exclusion
- [Snowflake querying semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/querying) -- fact query behavior, table path constraints

### Tertiary (LOW confidence)
- None -- all claims verified against codebase or official docs

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all changes within existing Rust codebase
- Architecture: HIGH -- expansion pipeline well-understood, clear extension points
- Pitfalls: HIGH -- based on codebase analysis and Snowflake docs cross-reference

**Research date:** 2026-04-11
**Valid until:** 2026-05-11 (stable internal codebase)
