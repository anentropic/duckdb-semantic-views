# Project Research Summary

**Project:** DuckDB Semantic Views Extension -- v0.5.3 Advanced Semantic Features
**Domain:** DuckDB Rust extension -- advanced semantic modeling capabilities
**Researched:** 2026-03-14
**Confidence:** HIGH

## Executive Summary

v0.5.3 adds seven advanced semantic modeling features to the existing DuckDB semantic views extension: FACTS clause (named row-level sub-expressions), derived metrics (metric-on-metric composition), hierarchies (drill-down metadata), fan trap detection (structural correctness warnings), role-playing dimensions (same table joined via different relationships), semi-additive metrics (NON ADDITIVE BY for snapshot-style data), and multiple join paths (USING RELATIONSHIPS to disambiguate diamonds). All seven features operate within the extension's existing "expansion-only" preprocessor model -- the extension generates SQL, DuckDB executes it. No changes are needed to the FFI layer, catalog persistence, DDL pipeline, or query table function.

The features divide cleanly into three complexity tiers. **Low complexity:** FACTS clause (model struct already exists, parser follows existing patterns, expansion is text substitution), hierarchies (pure metadata, no SQL generation change), and role-playing dimensions (already works with the current alias-per-role architecture -- needs documentation and tests, not new code). **Medium complexity:** derived metrics (requires a new `metric_resolver.rs` module with DAG construction and cycle detection, but expansion uses expression inlining which avoids subqueries) and fan trap detection (new `fan_detection.rs` module analyzing graph topology against metric source tables, warning-only). **High complexity:** semi-additive metrics (changes the fundamental flat SELECT expansion to nested subquery with ROW_NUMBER window function) and multiple join paths/USING (relaxes the tree invariant in graph.rs, requires per-metric join path selection in expand.rs).

The recommended build order is driven by dependencies and increasing risk: (1) FACTS + Hierarchies, (2) Derived Metrics + Fan Detection, (3) Role-Playing verification, (4) USING RELATIONSHIPS (graph changes), (5) Semi-additive metrics. Semi-additive is recommended for deferral to v0.5.4 because it is the only feature that forces a structural change to the expansion pipeline (flat SELECT -> nested subquery). Every other feature works within the existing flat SELECT model.

The primary reference standard is Snowflake's `CREATE SEMANTIC VIEW` DDL, which was verified to have identical syntax for FACTS, USING, and NON ADDITIVE BY. Snowflake added semi-additive metrics on March 5, 2026 -- this is a very recent feature even in enterprise tools, supporting the deferral recommendation.

## Key Findings

**Stack:** Zero new Cargo dependencies. All features are implemented via extensions to `body_parser.rs`, `model.rs`, `graph.rs`, and `expand.rs`, plus two new modules (`metric_resolver.rs`, `fan_detection.rs`). The existing `serde`, `strsim`, and `proptest` crates handle all serialization, error suggestions, and testing needs.

**Architecture:** Three existing subsystems are modified (body_parser, graph, expand) and two new subsystems are added (metric_resolver, fan_detection). The expansion remains a single `pub fn expand()` entry point that returns a SQL string. No new connections, table functions, or FFI changes.

**Critical pitfall:** Derived metric expression substitution must use word-boundary matching to avoid substring collisions (e.g., `revenue` matching inside `total_revenue`). Facts must be parenthesized when inlined (`(price - discount)` not `price - discount`) to preserve operator precedence. The diamond rejection relaxation for USING must be implemented atomically with USING-aware expansion -- relaxing validation without updating expansion creates silently wrong results.

## Implications for Roadmap

Based on research, suggested phase structure:

1. **FACTS Clause + Hierarchies** -- Low risk, no graph changes
   - Addresses: FACTS parsing, fact expression substitution in expand, hierarchy metadata in DESCRIBE
   - Avoids: C4 (precedence -- parenthesize), M4 (clause ordering -- add to CLAUSE_ORDER)
   - Rationale: FACTS unblocks derived metrics. Hierarchies are independent metadata.

2. **Derived Metrics + Fan Trap Detection** -- Medium risk, new modules
   - Addresses: metric dependency DAG, expression inlining, fan-out warnings
   - Avoids: C1 (substring collision -- word-boundary matching), M1 (cycles -- Kahn's algorithm)
   - Rationale: Derived metrics are the highest-value feature. Fan detection is independent and can be built in parallel.

3. **Role-Playing Dimensions (verification)** -- Low risk, documentation
   - Addresses: Verify alias-per-role pattern works, add integration tests
   - Avoids: Premature USING implementation
   - Rationale: Likely already works. Needs tests, not code.

4. **USING RELATIONSHIPS** -- High risk, graph invariant change
   - Addresses: Relaxed diamond rejection, USING parsing, per-metric join path selection
   - Avoids: C2 (invariant gap -- atomic validation+expansion change)
   - Rationale: Most complex graph change. Build after simpler features are stable.

5. **Semi-Additive Metrics (defer to v0.5.4?)** -- High risk, expansion contract change
   - Addresses: NON ADDITIVE BY parsing, window function subquery generation
   - Avoids: C3 (expansion contract -- test build_execution_sql compatibility)
   - Rationale: Only feature that changes expansion from flat SELECT to nested subquery. Every other feature works within existing expansion structure.

**Phase ordering rationale:**
- FACTS before derived metrics because derived metrics may reference facts
- Fan detection does not depend on other features; can parallelize with Phase 2
- Role-playing verification before USING because role-playing validates the simpler case
- USING last among core features because it relaxes a fundamental invariant
- Semi-additive last (or deferred) because it changes the expansion pipeline structure

**Research flags for phases:**
- Phase 2 (Derived Metrics): Needs careful design of expression substitution to avoid substring collisions
- Phase 4 (USING RELATIONSHIPS): Needs design spike for dimension-USING scope inheritance (which join does a dimension use when metrics have different USING?)
- Phase 5 (Semi-Additive): Needs compatibility test with `build_execution_sql` wrapping before implementation

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Zero new deps; all features extend existing modules |
| Features | HIGH | Snowflake DDL grammar verified; Cube.dev and MetricFlow patterns cross-referenced; model struct (Fact) already exists |
| Architecture | HIGH | All integration points identified; expansion-only model preserved; no FFI changes needed |
| Pitfalls | HIGH | Expression substitution, invariant coupling, expansion contract risks identified from direct code analysis |

## Gaps to Address

- **Dimension-USING scope inheritance:** When metrics have USING, how do dimensions from the target table resolve? Research recommends inheriting from co-queried metrics, but Snowflake docs are not explicit on this. Needs design decision during Phase 4 planning.
- **Aggregate facts:** Snowflake allows COUNT() inside FACTS (e.g., `count_line_items AS COUNT(lineitem.line_item_id)`). This blurs the row-level boundary. Recommendation: restrict facts to row-level expressions initially; defer aggregate facts.
- **Cross-path derived metrics:** When a derived metric references metrics from different USING paths, expression inlining (Strategy A) is insufficient -- needs subquery wrapping (Strategy B). Defer to post-USING implementation; assess need based on real usage.
- **Fan trap detection accuracy:** Parsing aggregation type (SUM vs COUNT DISTINCT) from raw SQL expressions is heuristic. May produce false positives for nested expressions like `COALESCE(SUM(x), 0)`. Document limitation.
- **Multiple semi-additive metrics in one query:** Each needs a separate subquery with its own window function. The CTE composition pattern is complex. Consider limiting to one semi-additive metric per query initially.

## Sources

### Primary (HIGH confidence)
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- full DDL grammar with FACTS, USING, NON ADDITIVE BY
- [Snowflake Semantic View SQL Examples](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- role-playing, derived metrics, USING examples
- [Snowflake Validation Rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- graph constraints, diamond handling
- [Snowflake Semi-Additive Release (March 2026)](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY release timing
- [Kimball Semi-Additive Facts](https://www.kimballgroup.com/data-warehouse-business-intelligence-resources/kimball-techniques/dimensional-modeling-techniques/additive-semi-additive-non-additive-fact/) -- canonical additivity definitions
- Project source: `src/model.rs`, `src/body_parser.rs`, `src/expand.rs`, `src/graph.rs`, `src/ddl/define.rs` -- direct code inspection

### Secondary (MEDIUM confidence)
- [MetricFlow / DeepWiki](https://deepwiki.com/dbt-labs/metricflow) -- derived metric DAG architecture, fan trap handling
- [Cube.dev Measures](https://cube.dev/docs/reference/data-model/measures) -- measure composition patterns
- [Cube.dev Non-Additivity](https://cube.dev/docs/guides/recipes/query-acceleration/non-additivity) -- non-additive strategies
- [dbt MetricFlow](https://docs.getdbt.com/docs/build/about-metricflow) -- metric composition, semi-additive measures
- [Datacadamia Fan Trap](https://www.datacadamia.com/data/type/cube/semantic/fan_trap) -- fan trap definition and detection
- [Sisense Fan and Chasm Traps](https://docs.sisense.com/main/SisenseLinux/chasm-and-fan-traps.htm) -- detection patterns in BI tools

---
*Research completed: 2026-03-14*
*Ready for roadmap: yes*
