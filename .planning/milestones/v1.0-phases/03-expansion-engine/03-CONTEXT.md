# Phase 3: Expansion Engine - Context

**Gathered:** 2026-02-25
**Status:** Ready for planning

<domain>
## Phase Boundary

Pure Rust `expand()` function that takes a `SemanticViewDefinition` + selected dimensions/metrics and produces a SQL string. No DuckDB runtime needed. Covers MODEL-01..04, EXPAND-01..04, TEST-01..02. The query interface wiring (replacement scan, table function) is Phase 4.

</domain>

<decisions>
## Implementation Decisions

### Generated SQL shape
- CTE-wrapped structure: base query in `WITH "_base" AS (...)`, then `SELECT ... FROM "_base" GROUP BY ...`
- Human-readable formatting with indentation and newlines (user will see this via EXPLAIN in Phase 4)
- `SELECT *` in the base CTE — DuckDB's optimizer prunes unused columns
- Fixed CTE name `_base` — no view-name derivation needed for v0.1

### Join inclusion strategy
- Only include joins needed by the requested dimensions, metrics, or filters — not all declared joins
- Add optional `source_table` field to both `Dimension` and `Metric` structs — declares which join table the expression comes from
- Declaration-order chain resolution: user declares joins in dependency order, engine resolves transitive dependencies (if `regions` join references `customers` in its ON clause, include `customers` too)
- Filters that reference a joined table also trigger that join's inclusion

### Filter composition
- Multiple filter entries are AND-composed
- Each filter expression wrapped in parentheses for safety: `WHERE (filter1) AND (filter2)`
- Filters can reference any column from the base table or any declared join (all columns are in scope within the CTE)
- No filter-level OR composition — user writes OR logic within a single filter string

### Error messages
- Unknown dimension/metric: show the bad name, the view name, list available names, and suggest the closest match ("Did you mean 'region'?")
- Empty dimensions array is allowed — produces a global aggregate query (no GROUP BY)
- Empty metrics array is an error — at least one metric required
- Duplicate dimension/metric names in a single request produce an error
- No validation of metric expressions as aggregates — trust the definition author, let DuckDB catch issues at query time

### Claude's Discretion
- Fuzzy matching algorithm for "did you mean" suggestions (edit distance, etc.)
- Internal representation of the join dependency graph
- proptest strategy design for property-based tests
- Test dataset schemas and known-answer values

</decisions>

<specifics>
## Specific Ideas

- Follow Snowflake/dbt/Cube industry patterns for filter composition (AND-composed named conditions)
- Join chain support matches what regular DuckDB SQL can express — no artificial limitations vs raw SQL
- Model change needed: add optional `source_table: Option<String>` to `Dimension` and `Metric` structs (currently only have `name` and `expr`)

</specifics>

<deferred>
## Deferred Ideas

- Named, reusable filters (Snowflake-style with name/description/synonyms) — future milestone
- Pre-aggregation / materialized view matching — future milestone (PERF-FUT-01)
- Derived/ratio metrics referencing other metrics — future milestone (MODEL-FUT-01)

</deferred>

---

*Phase: 03-expansion-engine*
*Context gathered: 2026-02-25*
