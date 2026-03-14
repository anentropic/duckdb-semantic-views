# Domain Pitfalls -- Advanced Semantic Features (v0.5.3)

**Domain:** Adding FACTS clause, derived metrics, hierarchies, fan trap detection, role-playing dimensions, semi-additive metrics (NON ADDITIVE BY), and multiple join paths (USING RELATIONSHIPS) to an existing DuckDB semantic views extension
**Researched:** 2026-03-14
**Context:** The extension is a SQL preprocessor (expansion only, no execution engine). It has a PK/FK relationship model with graph validation that currently rejects diamonds and cycles. Kahn's algorithm provides topological sort join ordering. `body_parser.rs` state machine parses DDL clauses. `RelationshipGraph` in `src/graph.rs` manages join resolution. All SQL is expanded and handed to DuckDB for execution. The `Fact` model struct exists but is not wired into the body parser or expansion engine.

---

## Critical Pitfalls

Mistakes that cause rewrites, silent wrong results, or architectural dead ends.

### C1: Derived Metrics Create Circular Dependencies That Infinite-Loop the Expansion Engine

**What goes wrong:**
Derived metrics reference other metrics by name (e.g., `profit = revenue - cost`). If the dependency chain contains a cycle (`A = B + 1`, `B = A - 1`), the expansion engine tries to resolve `A` by expanding `B`, which tries to expand `A`, producing infinite recursion or a stack overflow.

The current `expand()` function in `expand.rs` resolves metrics by name lookup via `find_metric()`, which returns the `Metric.expr` string directly. There is no dependency resolution step. When a metric expression contains another metric name, the expansion engine must recognize the reference, look up the referenced metric, and substitute its expression. Without cycle detection, this substitution loops forever.

Snowflake's approach: derived metrics are scoped to the semantic view (not a logical table), and "you cannot use a derived metric in the expression for a regular metric, dimension, or fact. Only another derived metric can use a derived metric in its expression." This constraint prevents some cycles but does not eliminate derived-to-derived cycles.

MetricFlow's approach: metrics are nodes in a DAG; the manifest parser validates the dependency graph at definition time.

**Why it happens:**
The current metric model is flat -- each metric has `name`, `expr`, `source_table`, `output_type`. There is no concept of "this metric references that metric." Adding derived metrics means adding a dependency relationship between metrics, but without explicit dependency tracking, the system cannot detect cycles.

**Consequences:**
- Stack overflow or infinite loop during `expand()` at query time
- If protected by a recursion depth limit, the error message is confusing ("maximum recursion depth exceeded" instead of "circular metric dependency")
- DuckDB process crash in the stack overflow case (no graceful recovery from stack overflow in a loadable extension)

**Prevention:**
- At define time, build a metric dependency DAG. Parse each metric's expression for references to other metric names. Run topological sort (reuse Kahn's algorithm from `graph.rs`) on the metric DAG. Reject cycles with a clear error: "Circular metric dependency: revenue -> profit -> revenue."
- At expansion time, expand metrics in topological order (leaf metrics first, derived metrics last). Each derived metric's expression has its input metrics already resolved to SQL expressions.
- **Do NOT use recursive string substitution.** Instead, expand the dependency DAG into a layered SQL expression. For `profit = revenue - cost` where `revenue = SUM(o.amount)` and `cost = SUM(o.cost)`, the expanded SQL is `SUM(o.amount) - SUM(o.cost) AS "profit"`, not a recursive substitution.
- The metric name detection in expressions is a parsing problem. Two approaches:
  - (a) Require explicit syntax: `metric(revenue)` wrapper in expressions. This is unambiguous but verbose.
  - (b) Match metric names as identifiers in the expression. This risks false positives if a column name matches a metric name.
  - Recommendation: (a) explicit syntax, matching Snowflake's approach where derived metrics reference metrics by name without table qualification.
- **Confidence:** HIGH. Cycle detection in DAGs is a solved problem. The existing `toposort()` in `graph.rs` already returns `Err` on cycles. The risk is in failing to detect metric references in expressions.

**Detection:** Stack overflow during `expand()`. Query hangs indefinitely. Test: define `a AS metric(b) + 1`, `b AS metric(a) - 1`, expect define-time error.

**Phase assignment:** Must be addressed in the derived metrics phase. Cycle detection at define time, topological expansion at query time.

---

### C2: Fan Trap Produces Silently Inflated Metrics -- The Core Correctness Problem

**What goes wrong:**
When a query joins tables across a one-to-many relationship and aggregates metrics from the "one" side, the join duplicates rows before aggregation. Example:

```
orders (1) ---< line_items (many)
```

Query: `dimensions := ['product_name'], metrics := ['order_count']` where `order_count = COUNT(*)` is on `orders` and `product_name` is on `line_items`. The LEFT JOIN duplicates each order row for every line item. `COUNT(*)` returns `total_line_items`, not `total_orders`.

This is the most dangerous pitfall in semantic layer development because **the query succeeds with plausible-looking numbers**. There is no error, no warning -- just wrong results that may differ from the correct answer by 2-10x depending on data cardinality.

The current extension generates a flat `FROM base LEFT JOIN ... GROUP BY` query. It has no awareness of join cardinality and no fan-out prevention.

**Why it happens:**
The PK/FK relationship model declares structural connections but not cardinality direction (one-to-many vs many-to-one). Even if cardinality IS recorded, the expansion engine treats all joins equally -- it does not know that aggregating `orders.amount` after joining to `line_items` will inflate the sum.

Cube.dev solves this with primary key-based deduplication: "When detected, Cube generates a deduplication query that evaluates all distinct primary keys within the multiplied measure's cube and then joins distinct primary keys to produce correct results." This requires (a) PKs declared on every table, (b) relationship cardinality declared, (c) metric-aware expansion that pre-aggregates before joining when needed.

**Consequences:**
- SUM, COUNT, AVG metrics return inflated values
- Users trust the numbers because the query runs successfully
- The error is only caught when comparing against a known-correct baseline
- Retroactive fix requires changing the expansion strategy, potentially breaking existing query results (if users have already adjusted to the inflated values)

**Prevention (phased approach):**
1. **Phase 1 -- Detection and warning (v0.5.3):** At define time, record relationship cardinality (which side has the PK = "one" side). At query time, detect when a metric's source table is on the "one" side of a join triggered by a dimension on the "many" side. Emit a WARNING (not an error) in the query result or via DuckDB's warning mechanism.
2. **Phase 2 -- Pre-aggregation deduplication (future):** When fan-out is detected, generate a deduplication subquery:
   ```sql
   -- Instead of:
   SELECT li.product_name, COUNT(*) AS order_count
   FROM orders AS o LEFT JOIN line_items AS li ON ...
   GROUP BY 1

   -- Generate:
   SELECT li.product_name, sub.order_count
   FROM (SELECT o.id, COUNT(*) AS order_count FROM orders AS o GROUP BY o.id) AS sub
   LEFT JOIN line_items AS li ON sub.id = li.order_id
   GROUP BY 1
   ```
   This pre-aggregates the metric per PK before joining, eliminating fan-out.
3. **Phase 3 -- Automatic strategy selection (future):** Based on cardinality metadata and requested dimensions/metrics, automatically choose between flat expansion (safe), pre-aggregation (fan-out detected), or separate-query-then-join (chasm trap detected).

- **Confidence:** HIGH. Fan trap is the most-discussed pitfall in semantic layer literature. Every mature system (Cube.dev, Holistics, Sisense, MetricFlow) has a solution or explicit documentation.

**Detection:** Compare `COUNT(*)` on base table alone vs with a one-to-many join included. If the count changes, fan-out is occurring. Test: create orders + line_items tables, define metric on orders, dimension on line_items, assert warning is emitted.

**Phase assignment:** Fan trap detection phase. Warning is achievable in v0.5.3. Full deduplication is a future milestone.

---

### C3: Relaxing Diamond Rejection Breaks the Tree Invariant That Graph Validation Depends On

**What goes wrong:**
The current `RelationshipGraph.check_no_diamonds()` enforces that each non-root node has at most one parent. This tree invariant is assumed by:
- `resolve_joins_pkfk()` which walks `reverse` edges to find the single path from any node back to the root
- `synthesize_on_clause()` which finds THE join for an alias (not one of several)
- `toposort()` which returns a single linear order (trees have a unique topological sort)

The v0.5.3 feature "multiple join paths (USING RELATIONSHIPS)" explicitly relaxes the diamond rejection to allow the same table to be reachable via multiple paths when the user disambiguates with `USING RELATIONSHIPS`. But the graph infrastructure ASSUMES no diamonds. Simply removing `check_no_diamonds()` without updating the consumers causes:
- `resolve_joins_pkfk()` to follow one arbitrary path (whichever `reverse` edge it encounters first), ignoring the user's `USING RELATIONSHIPS` directive
- `synthesize_on_clause()` to find the wrong join when multiple joins target the same alias
- `toposort()` to produce a valid but potentially wrong ordering when multiple orderings are valid

**Why it happens:**
The tree invariant was a simplifying assumption that made the graph code correct and simple. Relaxing it requires upgrading every consumer of `RelationshipGraph` to handle DAGs (directed acyclic graphs with shared nodes), not just trees.

**Consequences:**
- Queries silently use the wrong join path, producing wrong results
- The wrong path may join through tables that filter or multiply rows differently
- No error is raised because the graph is still acyclic (diamonds are not cycles)

**Prevention:**
- Do NOT simply remove `check_no_diamonds()`. Instead:
  1. Keep `check_no_diamonds()` as the DEFAULT validation
  2. Add a new validation mode: `check_no_unresolved_diamonds()` which allows diamonds ONLY when every dimension/metric that traverses the diamond specifies `USING RELATIONSHIPS`
  3. At query time, `resolve_joins_pkfk()` must accept a `path_hint: Option<Vec<String>>` parameter that specifies which relationship names to follow. When a path hint is provided, follow only those edges. When no hint is provided and multiple paths exist, return an error: "Ambiguous path to table 'X'. Use USING RELATIONSHIPS to specify the join path."
- The `USING RELATIONSHIPS` clause must reference relationship NAMES (the `name` field on `Join`), not table aliases. Relationship names are already stored in `Join.name` (added in Phase 24).
- Modify `toposort()` to return all valid topological orderings when the graph is a DAG (or return the ordering consistent with the selected path hints).
- **Confidence:** HIGH. The code is small and well-structured. The changes are localized to `graph.rs` and `expand.rs`. But the surface area for bugs is large because every function that touches the graph must be reviewed.

**Detection:** Define a diamond graph, query with ambiguous path, expect error. Define same graph with `USING RELATIONSHIPS`, expect correct path. Test both paths and verify different results.

**Phase assignment:** Multiple join paths phase. Must be coordinated with graph validation changes.

---

### C4: Role-Playing Dimensions Require Duplicate Table Aliases, Breaking the Alias Uniqueness Assumption

**What goes wrong:**
Role-playing dimensions join the SAME physical table multiple times with different roles. Classic example: `orders` has `created_date`, `shipped_date`, `delivered_date`, all FK references to a `dates` table. The DDL needs:

```sql
TABLES (
    o AS orders PRIMARY KEY (id),
    d_created AS dates PRIMARY KEY (date_id),
    d_shipped AS dates PRIMARY KEY (date_id),
    d_delivered AS dates PRIMARY KEY (date_id)
)
```

The current `body_parser.rs` parses TABLES entries as `alias AS physical_table PRIMARY KEY (...)`. The `RelationshipGraph` uses `alias` as the node key. This part works -- different aliases for the same physical table are different nodes.

BUT: the current code in several places uses `table` (physical table name) as a lookup key, not `alias`:
- `resolve_joins_pkfk()` line 432: `j.table.to_ascii_lowercase() == *alias` -- this finds a join by matching the join's `table` field against the needed alias. For role-playing dims, the `table` field is `"dates"` for all three, but the needed alias is `"d_created"`. The lookup FAILS.
- `synthesize_on_clause()` line 227: `t.alias.to_ascii_lowercase() == to_alias_lower` -- this correctly uses alias. But it is called with `join.table` as the target, not the alias.
- `expand()` line 440: `t.alias.to_ascii_lowercase() == *alias` -- correct.

The confusion between `join.table` (which currently holds the TARGET ALIAS, not the physical table) and actual physical table names is a naming inconsistency in the model. For role-playing dims, `join.table` must hold the alias (e.g., `d_created`), and a SEPARATE field must hold the physical table (e.g., `dates`).

**Why it happens:**
In v0.5.2, `join.table` was overloaded to mean "the alias of the target table in the graph." This worked because there was a 1:1 mapping between aliases and physical tables. Role-playing dims break this assumption.

**Consequences:**
- Only one instance of the dates table is joined (the first one found)
- Dimensions from other date roles reference the wrong join or fail with "unknown source table"
- If by chance DuckDB finds the column in the wrong dates instance, results are silently wrong

**Prevention:**
- The `Join` struct already has `from_alias` (FK source alias) and `table` (FK target alias). For role-playing dims, this is sufficient IF `table` truly holds the alias and the physical table is resolved from `def.tables`.
- Review ALL code that reads `join.table` and verify it expects an alias, not a physical table name. The resolve + synthesize + expand pipeline must consistently use aliases.
- Add explicit role-playing dimension tests: three FKs from orders to dates via different aliases, query each dimension independently and together, verify correct results.
- Consider renaming `Join.table` to `Join.to_alias` for clarity. The physical table is always resolved via `def.tables.iter().find(|t| t.alias == join.to_alias).map(|t| t.table)`.
- **Confidence:** HIGH. The model supports this, but the naming confusion between `table` and `alias` across the codebase is the risk. A systematic audit of all `join.table` references is required.

**Detection:** Define a view with two FKs to the same physical table (dates), query dimensions from each role, verify they return different values. Test: `d_created.month` should show order creation months, `d_shipped.month` should show shipment months.

**Phase assignment:** Role-playing dimensions phase. Must audit all `join.table` usages before implementation.

---

## Moderate Pitfalls

### M1: FACTS Clause Introduces a New Namespace That Collides With Dimensions and Metrics

**What goes wrong:**
The FACTS clause defines named row-level sub-expressions (e.g., `unit_price AS (o.amount / o.quantity)`). These are pre-aggregation expressions that metrics can reference. The `Fact` struct already exists in `model.rs` with `name`, `expr`, `source_table`.

The problem: fact names, dimension names, and metric names share the same user-facing namespace. If a user defines `revenue` as both a fact and a metric, which one wins? The current expansion engine resolves dimensions and metrics by name via `find_dimension()` and `find_metric()`. Facts need a THIRD lookup, and name collisions must be handled.

Snowflake handles this by requiring table-qualified names for facts and metrics: `orders.revenue` (fact) vs `revenue` (derived metric). But within a single table scope, names must still be unique.

**Why it happens:**
The `Fact` struct was added in Phase 11 but never wired into the body parser or expansion engine. It sits dormant in the model. When activated, it introduces a new name resolution layer.

**Consequences:**
- Ambiguous name resolution: `SUM(revenue)` in a metric expression -- is `revenue` a column, a fact, or another metric?
- If facts shadow column names, the user cannot reference the original column
- If facts and metrics can share names, substitution order determines behavior (fragile)

**Prevention:**
- Enforce namespace uniqueness at define time: no fact may share a name with a dimension or metric in the same view. Error: "Name 'revenue' is already defined as a metric. Fact names must be unique across facts, dimensions, and metrics."
- In metric expressions, facts are referenced by name and expanded BEFORE the metric expression is evaluated. Expansion order: (1) substitute fact references in metric expressions, (2) expand metric expressions into SQL.
- Facts do NOT appear in query results. They are internal computation helpers. The user never requests `facts := [...]` in the query function.
- **Confidence:** HIGH. Namespace collision is a standard parsing problem. The uniqueness check is simple.

**Phase assignment:** FACTS clause phase. Uniqueness validation at define time.

---

### M2: Semi-Additive Metrics (NON ADDITIVE BY) Require Fundamentally Different SQL Expansion

**What goes wrong:**
A semi-additive metric like `account_balance NON ADDITIVE BY (date_dim)` means: "SUM this metric across all dimensions EXCEPT date; for date, take only the latest value." The current expansion engine generates a single `SELECT ... GROUP BY` query. Semi-additive metrics require a two-stage query:

1. For each combination of additive dimensions, find the latest value of the non-additive dimension
2. Then aggregate the metric using those latest values

The SQL pattern (Snowflake's approach):
```sql
-- Stage 1: Find latest date per group
WITH latest AS (
    SELECT customer_id, MAX(date_id) AS latest_date
    FROM balances
    GROUP BY customer_id
)
-- Stage 2: Aggregate using only latest values
SELECT b.customer_id, SUM(b.balance) AS total_balance
FROM balances b
JOIN latest l ON b.customer_id = l.customer_id AND b.date_id = l.latest_date
GROUP BY 1
```

The current `expand()` function generates a single flat query. Adding a CTE or subquery for the "find latest" step requires structural changes to the expansion engine. This is NOT a matter of changing the metric expression -- it changes the query SHAPE.

**Why it happens:**
The expansion engine treats all metrics uniformly: each metric is an aggregate expression in the SELECT list. Semi-additive metrics break this uniformity because they require row filtering (keep only latest per group) BEFORE aggregation.

**Consequences:**
- If implemented as a simple aggregate expression, SUM(balance) sums ALL rows across ALL dates, producing wrong totals
- The wrong result is typically much larger than the correct one (proportional to the number of time periods)
- Users of balance/inventory/snapshot metrics get grossly inflated numbers

**Prevention:**
- Detect semi-additive metrics at expansion time. If the query includes dimensions that are listed in `NON ADDITIVE BY`, the metric is being aggregated across those dimensions -- the non-additive behavior applies.
- If the query does NOT include the non-additive dimension, no special handling is needed (the metric is being aggregated across its non-additive dimension, which means "take latest" is exactly what the user wants).
- Generate a two-stage SQL expansion:
  - Inner: filter to latest value per group using `ROW_NUMBER() OVER (PARTITION BY [additive_dims] ORDER BY [non_additive_dim] DESC) = 1` or a MAX subquery
  - Outer: aggregate normally
- The `ROW_NUMBER` approach is simpler and handles ties deterministically (unlike MAX which may match multiple rows).
- **Confidence:** MEDIUM. The SQL pattern is well-understood, but integrating it into the existing expansion engine requires architectural changes. The expansion engine currently outputs a single SQL string; semi-additive metrics need conditional query shape changes.

**Detection:** Define a balance metric with `NON ADDITIVE BY (date_dim)`. Query with and without `date_dim` in dimensions. Compare SUM results against hand-computed correct values. Test: with date_dim, SUM should be total of all date's balances; without date_dim, SUM should be total of only the latest date's balances per group.

**Phase assignment:** Semi-additive metrics phase. Must modify the expansion engine to support conditional query shapes.

---

### M3: Hierarchies Are Metadata-Only Until a Drill-Down Query API Exists

**What goes wrong:**
Hierarchies define drill-down paths (e.g., `country -> region -> city`). They express a logical ordering of dimensions from coarse to fine. The current query API is `semantic_view('view', dimensions := [...], metrics := [...])` -- the user explicitly lists which dimensions to include. Hierarchies do not change query behavior; they only provide metadata about which dimensions relate to each other.

The pitfall is over-engineering: building hierarchy-aware expansion logic when the only consumer is metadata display (`DESCRIBE SEMANTIC VIEW`). If hierarchies influence query behavior (e.g., automatically rolling up to the next level, or validating that drill-down paths are followed in order), the expansion engine becomes significantly more complex with no user-visible benefit until BI tools consume the hierarchy metadata.

**Why it happens:**
Hierarchies are a table-stakes feature in BI tools (Power BI, Tableau, Looker). The temptation is to build rich hierarchy support (level-based rollup, automatic aggregation at each level, parent-child hierarchy detection). But this extension is a SQL preprocessor, not a BI tool. The query interface is explicit: the user lists dimensions.

**Consequences:**
- Over-engineering: weeks of work on hierarchy-aware expansion that no query consumer uses
- Complexity in the expansion engine that makes future changes harder
- The hierarchy metadata could have shipped in a day as a define-time validation + DESCRIBE output

**Prevention:**
- Implement hierarchies as **metadata only** in v0.5.3:
  - New `Hierarchy` struct: `name: String, levels: Vec<String>` (each level is a dimension name)
  - Define-time validation: all levels must be valid dimension names in the same view
  - DESCRIBE output: show hierarchy levels
  - No query-time behavior change
- Defer hierarchy-aware query behavior to a future milestone when BI tool integration provides a consumer.
- **Confidence:** HIGH. Every semantic layer (Snowflake, dbt, Cube) treats hierarchies as metadata. None of them change query SQL based on hierarchy declarations.

**Detection:** If the expansion engine is being modified for hierarchy support, the scope has crept. Test: hierarchies should not appear in expanded SQL.

**Phase assignment:** Hierarchies phase. Metadata-only implementation.

---

### M4: USING RELATIONSHIPS Requires a New Query API Parameter

**What goes wrong:**
The `USING RELATIONSHIPS` feature allows a metric or dimension to specify which join path to use when multiple paths exist. This disambiguation must flow from the metric/dimension definition through to the query expansion. But the current query function signature is:

```sql
semantic_view('view', dimensions := [...], metrics := [...])
```

Two scenarios need different handling:

1. **Definition-time USING RELATIONSHIPS:** The metric definition specifies the path:
   ```sql
   METRICS (
       customer_orders NON ADDITIVE BY (date_dim)
           USING RELATIONSHIPS (order_to_customer)
           AS COUNT(*)
   )
   ```
   This is stored in the metric's definition and used automatically at expansion time. No query API change needed.

2. **Query-time path override:** The user wants to specify a different path at query time (like Cube.dev's `joinHints`). This requires a new query parameter:
   ```sql
   semantic_view('view', dimensions := [...], metrics := [...],
                 using_relationships := ['path_a', 'path_b'])
   ```

If the feature is designed with only definition-time paths but users need query-time overrides, the API must be extended later -- which is a breaking change if the parameter name or semantics conflict.

**Prevention:**
- For v0.5.3, implement definition-time `USING RELATIONSHIPS` only. Store the relationship path on the `Metric` and `Dimension` structs.
- Design the struct field to be forward-compatible with query-time overrides: `using_relationships: Option<Vec<String>>` on `Metric`/`Dimension`.
- Document that query-time path overrides are deferred. Do NOT add a query-time parameter in v0.5.3.
- **Confidence:** HIGH. Snowflake's approach is definition-time only. Cube.dev's joinHints are a complexity that this extension should not need in v0.5.3.

**Phase assignment:** Multiple join paths phase.

---

### M5: body_parser.rs Clause Ordering Must Be Extended Without Breaking Existing DDL

**What goes wrong:**
The current `CLAUSE_ORDER` in `body_parser.rs` is:
```rust
const CLAUSE_ORDER: &[&str] = &["tables", "relationships", "dimensions", "metrics"];
```

Adding `FACTS` requires inserting it into this ordering. Snowflake places FACTS before DIMENSIONS. The natural order is:
```
TABLES -> RELATIONSHIPS -> FACTS -> DIMENSIONS -> METRICS
```

But existing DDL that omits FACTS (which is optional) must continue to parse. The `find_clause_bounds()` function validates order by checking that each keyword's index in `CLAUSE_ORDER` is greater than the previous keyword's index. If FACTS is inserted at position 2, existing DDL with `TABLES -> DIMENSIONS -> METRICS` still works (indices 0, 3, 4 after insertion -- still increasing).

The risk: if the ordering validation is implemented incorrectly (e.g., as a strict sequence rather than a subsequence check), adding FACTS breaks all existing DDL that skips it.

**Why it happens:**
The current validation checks that keywords appear in the correct relative order. This is a subsequence check, not a strict sequence check. But extending `CLAUSE_KEYWORDS` and `CLAUSE_ORDER` with new entries is error-prone if the validation logic is not clearly documented.

**Consequences:**
- All existing `CREATE SEMANTIC VIEW` statements fail with "unexpected clause keyword" errors
- Stored definitions cannot be recreated from their DDL
- Regression in basic functionality

**Prevention:**
- Add FACTS, HIERARCHIES, and any other new clause keywords to `CLAUSE_KEYWORDS` and `CLAUSE_ORDER` in the correct position.
- Verify that the ordering validation is a SUBSEQUENCE check (current keywords must appear in order, but gaps are allowed).
- Add backward compatibility tests: create a view with `TABLES -> DIMENSIONS -> METRICS` (no FACTS, no RELATIONSHIPS), assert it still parses correctly after adding new keywords.
- Add tests for every valid keyword ordering combination with optional clauses omitted.
- **Confidence:** HIGH. The current validation logic is a subsequence check (line 239 of body_parser.rs uses `CLAUSE_ORDER.iter().position()` comparison). Adding new entries is safe IF the position is correct.

**Detection:** Existing sqllogictest DDL tests fail after adding new keywords. Test: run `just test-sql` after modifying `CLAUSE_KEYWORDS`.

**Phase assignment:** First phase of any new clause addition. Must be the earliest parser change.

---

### M6: Derived Metrics Cannot Use the Same GROUP BY Strategy as Regular Metrics

**What goes wrong:**
Regular metrics are aggregate expressions (`SUM(amount)`, `COUNT(*)`). They appear directly in the SELECT list and the GROUP BY handles row grouping. Derived metrics combine other metrics: `profit = revenue - cost` where `revenue = SUM(amount)` and `cost = SUM(cost_amount)`.

If the derived metric expression is naively expanded to `SUM(amount) - SUM(cost_amount)`, this works for simple arithmetic. But if a derived metric uses non-aggregated references (e.g., `margin = revenue / total_revenue` where `total_revenue` is a window function), the expansion must handle:
- Mixing aggregate and window functions in the same SELECT
- Ensuring the GROUP BY applies to the base aggregates but not to the window function
- DuckDB's SQL semantics: you cannot reference an alias defined in the same SELECT list

The current expansion generates ordinal GROUP BY (`GROUP BY 1, 2, ...`). This works for flat aggregates. Derived metrics that reference aliases from the same SELECT list require a subquery or CTE wrapping.

**Why it happens:**
The expansion engine is designed for a flat SELECT/GROUP BY pattern. Derived metrics introduce expression dependencies within the SELECT list.

**Consequences:**
- DuckDB error: "column X must appear in the GROUP BY clause or be used in an aggregate function"
- Or: DuckDB error: "column alias X cannot be referenced in the same SELECT list"
- The error comes from DuckDB, not from the extension, so the message is confusing

**Prevention:**
- For v0.5.3, restrict derived metrics to arithmetic combinations of other metric expressions. The expansion substitutes the underlying aggregate expressions inline:
  - `profit = revenue - cost` expands to `SUM(amount) - SUM(cost_amount) AS "profit"`
  - This works because DuckDB evaluates `SUM(amount) - SUM(cost_amount)` as two aggregates with arithmetic
- Reject derived metrics that reference non-aggregate expressions or use window functions. These require a two-pass expansion (inner query with GROUP BY, outer query with derived calculations).
- Add define-time validation: a derived metric's resolved expression (after substituting all referenced metrics) must be a valid aggregate expression. If it contains non-aggregated column references, reject at define time.
- **Confidence:** MEDIUM. Simple derived metrics (arithmetic on aggregates) work with inline substitution. Complex derived metrics (window functions, conditional aggregation) require architectural changes.

**Detection:** Define `profit = revenue - cost`, expand, verify DuckDB accepts the SQL. Define `margin = revenue / SUM(revenue) OVER ()`, expect define-time rejection. Test both cases.

**Phase assignment:** Derived metrics phase. Restrict to arithmetic-on-aggregates for v0.5.3.

---

## Minor Pitfalls

### m1: FACTS Expressions Must Be Expanded BEFORE Metric Expressions

**What goes wrong:**
Facts are named row-level sub-expressions: `unit_price AS (o.amount / o.quantity)`. A metric references a fact: `avg_unit_price AS AVG(unit_price)`. The expansion must substitute `unit_price` with `o.amount / o.quantity` BEFORE generating the metric SQL.

If the expansion order is wrong (metrics expanded before facts), the generated SQL contains `AVG(unit_price)` where `unit_price` is not a column -- DuckDB errors with "column unit_price does not exist."

**Prevention:**
- Expansion order: (1) resolve facts by name in metric expressions, (2) expand metrics with substituted expressions, (3) generate SQL.
- Facts are purely syntactic substitutions -- they do not change the query structure. Treat them like macros.
- Use a simple string replacement or, better, an expression tree substitution that respects identifier boundaries (don't replace `unit_price` inside `unit_price_adjusted`).
- **Confidence:** HIGH. Textual substitution is straightforward. Boundary-aware substitution prevents false positives.

**Phase assignment:** FACTS clause phase.

---

### m2: Hierarchy Levels Must Reference Existing Dimensions

**What goes wrong:**
A hierarchy `geo_hierarchy` defines levels `[country, region, city]`. If `city` is not defined as a dimension in the view, the hierarchy is invalid. But if validation only checks at define time and the user later removes `city` (via `CREATE OR REPLACE`), the hierarchy becomes dangling.

**Prevention:**
- Validate hierarchy levels against dimension names at define time.
- Since the extension uses `CREATE OR REPLACE` (not ALTER), every definition is complete -- there is no "remove a dimension while keeping the hierarchy." The hierarchy levels are validated against the same definition's dimensions.
- **Confidence:** HIGH. Simple cross-reference validation.

**Phase assignment:** Hierarchies phase.

---

### m3: NON ADDITIVE BY Dimension Names Must Match Dimension Definitions

**What goes wrong:**
`NON ADDITIVE BY (date_dim)` references a dimension by name. If `date_dim` is misspelled or does not exist in the view, the semi-additive behavior silently does not apply -- the metric is treated as fully additive, producing wrong results.

**Prevention:**
- Validate that every dimension name in `NON ADDITIVE BY` exists as a defined dimension in the same view. Error at define time: "NON ADDITIVE BY references unknown dimension 'date_dim'. Available: [order_date, ship_date]."
- Use the existing `suggest_closest()` fuzzy matching for "did you mean?" suggestions.
- **Confidence:** HIGH. Name validation with fuzzy suggestions is an existing pattern in the codebase.

**Phase assignment:** Semi-additive metrics phase.

---

### m4: Relationship Names Must Be Unique and Explicitly Declared for USING RELATIONSHIPS

**What goes wrong:**
`USING RELATIONSHIPS (order_to_customer)` references a relationship by name. The current `Join.name` field is `Option<String>` -- it is optional. If some relationships are unnamed, `USING RELATIONSHIPS` cannot reference them, and the user gets a confusing error.

**Prevention:**
- If the view has diamonds (multiple paths), require ALL relationships to be named. Error at define time: "Relationship from 'o' to 'c' must have a name because the graph has multiple paths. Use: `rel_name AS o(fk_col) REFERENCES c`."
- If the view has no diamonds, relationship names remain optional (backward compatible).
- **Confidence:** HIGH. The `Join.name` field already exists. The validation is a conditional check.

**Phase assignment:** Multiple join paths phase.

---

### m5: Serde Backward Compatibility for New Model Fields

**What goes wrong:**
Adding new fields to `Metric` (e.g., `using_relationships: Option<Vec<String>>`, `non_additive_by: Option<Vec<String>>`) or to `SemanticViewDefinition` (e.g., `hierarchies: Vec<Hierarchy>`) changes the JSON schema stored in `semantic_layer._definitions`. Old stored definitions without these fields must still deserialize correctly.

**Prevention:**
- All new fields must have `#[serde(default)]` and `#[serde(skip_serializing_if = "...")]` attributes, matching the existing pattern used for `facts`, `tables`, `pk_columns`, etc.
- Add explicit backward compat tests: deserialize old JSON (without new fields) and verify defaults.
- The existing `unknown_fields_are_allowed` test confirms that `deny_unknown_fields` is NOT set -- this is critical for forward compatibility (old code loading new JSON).
- **Confidence:** HIGH. This is an established pattern in the codebase with existing tests.

**Phase assignment:** Every phase that modifies the model.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| FACTS clause | M1 (namespace collision), m1 (expansion order) | Enforce unique names across facts/dims/metrics; substitute facts before metrics |
| Derived metrics | C1 (circular deps), M6 (GROUP BY strategy) | Metric dependency DAG with cycle detection; restrict to arithmetic-on-aggregates |
| Hierarchies | M3 (over-engineering), m2 (dangling levels) | Metadata-only implementation; validate levels against dimensions |
| Fan trap detection | C2 (silent wrong results) | Record cardinality; detect and warn at query time |
| Role-playing dimensions | C4 (alias vs table confusion) | Audit all `join.table` usages; add role-playing tests |
| Semi-additive metrics | M2 (different SQL shape), m3 (dimension name validation) | Two-stage expansion with ROW_NUMBER; validate NON ADDITIVE BY names |
| Multiple join paths | C3 (tree invariant), M4 (query API), m4 (relationship names) | Conditional diamond validation; definition-time paths only; require names when ambiguous |
| All model changes | m5 (serde compat) | `#[serde(default)]` on all new fields; backward compat tests |

---

## Sources

**Industry sources consulted:**
- [Snowflake: CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- FACTS syntax, NON ADDITIVE BY, USING RELATIONSHIPS, derived metrics
- [Snowflake: How Snowflake validates semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- define-time vs query-time validation rules
- [Snowflake: Derived metrics release note (Sep 2025)](https://docs.snowflake.com/en/release-notes/2025/other/2025-09-30-semantic-view-derived-metrics) -- derived metric constraints
- [Snowflake: Semi-additive metrics release note (Mar 2026)](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY implementation
- [Snowflake: Best practices for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/best-practices-dev) -- column limits, security warnings
- [Cube.dev: Joins Reference](https://cube.dev/docs/reference/data-model/joins) -- relationship types, PK requirement, fan/chasm trap deduplication, Dijkstra path resolution
- [Cube.dev: Working with Joins](https://cube.dev/docs/product/data-modeling/concepts/working-with-joins) -- diamond subgraph handling, join hints, deduplication strategy
- [dbt MetricFlow: DeepWiki](https://deepwiki.com/dbt-labs/metricflow) -- derived metric nodes, DataflowPlanBuilder, CombineAggregatedOutputsNode
- [dbt: Build metrics intro](https://docs.getdbt.com/docs/build/build-metrics-intro) -- derived metric types, input_metrics pattern
- [dbt: Join logic](https://docs.getdbt.com/docs/build/join-logic) -- multi-hop joins, entity-based fan-out prevention
- [Kimball Group: Additive and Semi-Additive Facts](https://www.kimballgroup.com/data-warehouse-business-intelligence-resources/kimball-techniques/dimensional-modeling-techniques/additive-semi-additive-non-additive-fact/) -- canonical definitions
- [SQLBI: Semi-Additive Measures in DAX](https://www.sqlbi.com/articles/semi-additive-measures-in-dax/) -- implementation complexity
- [datacadamia: Fan Trap Issue](https://www.datacadamia.com/data/type/cube/semantic/fan_trap) -- fan trap definition, deduplication challenge
- [Holistics: Fan-out Issue](https://docs.holistics.io/docs/faqs/fan-out-issue) -- pre-aggregate-before-join deduplication strategy
- [boring-semantic-layer: Issue #32](https://github.com/boringdata/boring-semantic-layer/issues/32) -- multiple joins to same dimension table
- [LinkedIn: Hierarchies in star schemas](https://www.linkedin.com/advice/1/how-do-you-incorporate-hierarchies-drill-downs) -- hierarchy as metadata pattern

**Codebase sources:**
- `src/graph.rs` -- `RelationshipGraph`, `check_no_diamonds()`, `toposort()`, `validate_graph()`
- `src/expand.rs` -- `expand()`, `resolve_joins_pkfk()`, `synthesize_on_clause()`, `find_dimension()`, `find_metric()`
- `src/body_parser.rs` -- `CLAUSE_KEYWORDS`, `CLAUSE_ORDER`, `find_clause_bounds()`
- `src/model.rs` -- `SemanticViewDefinition`, `Fact`, `Metric`, `Dimension`, `Join`, serde annotations
- `TECH-DEBT.md` -- accepted decisions and deferred items
- `.planning/PROJECT.md` -- project context and constraints
