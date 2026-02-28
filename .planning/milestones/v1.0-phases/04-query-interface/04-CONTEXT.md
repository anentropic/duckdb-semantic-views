# Phase 4: Query Interface - Context

**Gathered:** 2026-02-25
**Status:** Ready for planning

<domain>
## Phase Boundary

Wire the expansion engine (Phase 3) to DuckDB's query pipeline so users can run `FROM view_name(dimensions := [...], metrics := [...])` and receive correct results. Covers replacement scan / table function registration, WHERE composition, EXPLAIN support, and integration tests (including Iceberg via DuckLake). Pre-aggregation is out of scope (future milestone).

</domain>

<decisions>
## Implementation Decisions

### Query ergonomics
- Follow Snowflake's model: allow dimensions-only (returns distinct values), metrics-only (returns global aggregate), and dimensions+metrics (grouped aggregation)
- Filters via standard SQL WHERE on the result — no `filters` parameter. WHERE clauses AND-compose with the view's row-level filters per QUERY-02
- Empty call `FROM view_name()` with no dimensions or metrics is an error with a helpful message directing users to specify at least one
- Error on empty should suggest: "Specify at least dimensions := [...] or metrics := [...]"

### Error experience
- Missing semantic view: fuzzy-match against registered views and suggest similar names (e.g., "Semantic view 'ordrs' not found. Did you mean 'orders'?")
- Invalid dimension/metric names: pass through expand() errors directly — Phase 3 already produces fuzzy-matched suggestions
- SQL execution failures: show both the expanded SQL that was generated AND the DuckDB error message, so users can see what went wrong and what SQL caused it
- All errors include actionable hints pointing to relevant DDL functions (e.g., "Run FROM describe_semantic_view('orders') to see available dimensions and metrics")

### EXPLAIN output
- Use standard DuckDB EXPLAIN syntax: `EXPLAIN FROM view_name(dimensions := [...], metrics := [...])`
- Output includes three parts: (1) metadata header with semantic view name, requested dimensions, and requested metrics, (2) pretty-printed expanded SQL with indentation, (3) DuckDB's standard EXPLAIN plan
- Expanded SQL should be formatted multi-line with SELECT/FROM/WHERE/GROUP BY on separate lines

### Claude's Discretion
- Table function vs replacement scan implementation approach
- SQL pretty-printing implementation (hand-rolled vs library)
- Exact error message formatting and wording
- How metadata header is formatted in EXPLAIN output

</decisions>

<specifics>
## Specific Ideas

- "I want to ensure that nothing we've built hinders the query planner vs equivalent hand-authored query" — integration tests should compare EXPLAIN plans between semantic view queries and hand-written equivalent SQL to verify the optimizer treats them identically
- Iceberg is the primary personal use case — DuckLake integration must be first-class, not an afterthought
- Follow Snowflake semantic view conventions where applicable (parameter patterns, error behavior)

### Integration test setup: DuckLake + jaffle-shop
- Set up a local DuckLake catalog with DuckDB as the catalog backend
- Use dbt-labs/jaffle-shop dataset (download from S3)
- Python script handles the full lifecycle: download jaffle-shop data, create DuckLake Iceberg catalog, load data into Iceberg tables
- Data files must be gitignored (only the script and catalog config are committed)
- Just recipe calls the Python script for convenience

### Integration test scenarios (all four required)
1. **Basic round-trip**: Define view over orders, query with dimensions + metrics, assert correct aggregates
2. **WHERE composition**: View has row filters + user adds WHERE, assert both apply correctly (QUERY-02)
3. **Iceberg source query**: Same semantic view pattern over DuckLake/Iceberg tables, prove extension works with iceberg_scan()
4. **Multi-table joins**: Semantic view with joins across orders+customers+products, assert join resolution works end-to-end
5. **EXPLAIN plan equivalence**: For key test cases, compare EXPLAIN output between semantic view query and equivalent hand-written SQL — assert plans are identical or equivalent

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 04-query-interface*
*Context gathered: 2026-02-25*
