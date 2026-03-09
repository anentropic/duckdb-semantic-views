# Domain Pitfalls -- SQL DDL Syntax & PK/FK Relationship Model (v0.5.2)

**Domain:** Adding proper SQL DDL keyword syntax and Snowflake-style PK/FK relationship model to an existing DuckDB semantic views extension
**Researched:** 2026-03-09
**Context:** The extension already has a working DDL pipeline (parse -> detect -> rewrite -> execute via function calls), CTE-based expansion with ON-clause heuristic join resolution, dual DDL interface (function-based + native SQL), table aliases, join_columns FK pairs, and qualified column lookup. This research covers pitfalls specific to the v0.5.2 transition.

---

## Critical Pitfalls

Mistakes that cause rewrites, data corruption, or silent wrong results.

### C1: SQL DDL Keyword Parser Eats Function-Call DDL Body -- Backward Compatibility Break

**What goes wrong:**
The current `rewrite_ddl` function passes the CREATE body verbatim to the underlying function call: `CREATE SEMANTIC VIEW sales (dimensions := [...])` becomes `SELECT * FROM create_semantic_view('sales', dimensions := [...])`. The body uses DuckDB function-call syntax (`:=` named parameters, struct/list literals).

When you add SQL keyword syntax (`TABLES (...) DIMENSIONS (...) METRICS (...)`), the parser must distinguish between the OLD function-call body and the NEW keyword body. If the new parser consumes ALL `CREATE SEMANTIC VIEW` statements and applies keyword parsing, it will break existing definitions that use `:=` syntax.

The `TECH-DEBT.md` item #8 explicitly flags this: "The DDL body uses DuckDB function-call syntax because `rewrite_ddl` passes the body verbatim. A Snowflake-style SQL DDL grammar was the original intent but was never implemented."

**Why it happens:**
The temptation is to replace `parse_create_body` (which just extracts text between outer parens) with a keyword parser. But any stored definitions, documentation examples, and user scripts that use `:=` syntax will break immediately.

**Consequences:**
- All existing `CREATE SEMANTIC VIEW` statements stop working
- Stored catalog definitions cannot be re-created from their JSON (the function-based path still works, but users lose the native DDL path for existing syntax)
- Breaking change in a minor version, violating user trust

**Prevention:**
- Detect which syntax variant the body uses BEFORE parsing. Heuristic: if the body contains `:=` at the top level (outside strings/brackets), it is function-call syntax -- pass verbatim. If the body starts with a known keyword (`TABLES`, `DIMENSIONS`, `METRICS`, `RELATIONSHIPS`) followed by `(`, it is keyword syntax -- parse and translate.
- The detection must happen in `rewrite_ddl` or `validate_and_rewrite`, not deeper in the parser. The existing `scan_clause_keywords` function already scans for keyword presence -- extend it to also detect `:=` assignments.
- During a transition period (v0.5.2), support BOTH body formats. Deprecate the `:=` format in documentation. Remove support in a later version (v0.7.0+) when backward compatibility is no longer needed.
- **Test:** A sqllogictest that creates a view with OLD `:=` syntax, then queries it, MUST pass after the v0.5.2 parser changes.
- **Confidence:** HIGH. This is the #1 backward compatibility risk. The codebase already has the detection infrastructure (`scan_clause_keywords` + `parse_create_body`).

**Phase assignment:** Must be the FIRST parser change. Build the syntax discriminator before implementing keyword parsing.

---

### C2: PK/FK Join Path Ambiguity -- Diamond Joins Produce Wrong Results Silently

**What goes wrong:**
The current ON-clause heuristic (`resolve_joins` in `expand.rs`) is simple: collect `source_table` values from requested dimensions/metrics, then do a fixed-point transitive closure over ON-clause substring matching. There is at most ONE path between any two tables because joins are declared linearly and matched by table name.

With PK/FK relationships, you introduce a JOIN GRAPH where multiple paths can exist between two tables. Classic example (diamond):

```
       orders
      /      \
  customers  products
      \      /
       region
```

If a dimension comes from `region`, should the join path go through `customers` or `products`? The PK/FK model does not answer this -- it only says edges exist, not which path to take. Cube.dev calls this the "diamond subgraph problem" and requires explicit `join_path` declarations to resolve it. Snowflake prohibits circular relationships entirely (even through transitive paths) and does not support self-referencing tables.

**Why it happens:**
The current model has no graph -- it has a flat list of joins with table names. Moving to PK/FK creates an implicit directed graph (FK table -> PK table). When the graph has diamonds or multiple paths, the `resolve_joins` function's fixed-point loop will include ALL tables on ALL paths, producing a cartesian product or duplicate rows.

**Consequences:**
- Aggregation metrics (SUM, COUNT) return inflated values because rows are duplicated across multiple join paths
- The user sees no error -- the query executes successfully with wrong numbers
- This is the "fan trap" in BI terminology: "measures are very hard to de-duplicate because SUM DISTINCT is semantically invalid"

**Prevention:**
- **Option A (recommended for v0.5.2): Require explicit join path per dimension/metric.** The `source_table` field already exists on `Dimension` and `Metric`. Require it to be set when the view has more than one join. The join resolver follows the SINGLE path from base table to the source table, not all possible paths.
- **Option B: Disallow diamond graphs at define time.** At `CREATE SEMANTIC VIEW` validation, check that the FK graph is a tree (no cycles, no diamonds). Reject definitions where a table is reachable via multiple paths. This is what Snowflake does: "You cannot define circular relationships, even through transitive paths."
- **Option C: Shortest-path resolution.** If multiple paths exist, pick the shortest (fewest hops). This is what Holistics does with its tier-based ranking. But shortest path may not be the semantically correct path.
- **Recommendation:** Option B for v0.5.2 (simplest, matches Snowflake, prevents the problem entirely). Option A for a future version if multi-path support is needed.
- **Validation algorithm:** At define time, build a directed graph from FK -> PK. Run DFS from each table. If any table is visited twice, reject with error: "Ambiguous join path: table 'X' is reachable via multiple paths. Remove one relationship to create a tree structure."
- **Confidence:** HIGH. Diamond join problems are thoroughly documented in Cube.dev, dbt MetricFlow, Holistics, and Sisense. Every semantic layer has either solved or forbidden this.

**Phase assignment:** Must be part of the define-time validation phase, BEFORE the expansion engine is changed. Reject bad graphs early.

---

### C3: CTE Flat Namespace Breaks When Adding Qualified Columns

**What goes wrong:**
The current expansion strategy (`expand()` in `expand.rs`) creates a single `_base` CTE that joins all needed tables, then the outer SELECT references columns from `_base` using unqualified names. This is explicitly documented in `TECH-DEBT.md` item #7: "Dimension and metric expressions must use unqualified column names because the CTE-based expansion flattens all source tables into a single `_base` namespace."

When you add qualified column support (`orders.revenue`, `customers.name`), the expressions contain dot-qualified references. But after the CTE flattens everything into `_base`, there is no `orders` or `customers` alias in scope -- only `_base`. DuckDB will error: "Table 'orders' does not exist."

This is the core architectural tension: the CTE strategy REQUIRES unqualified names, but qualified columns REQUIRE table aliases to be in scope.

**Why it happens:**
The CTE was designed for a flat namespace model. Qualified columns fundamentally need either (a) table aliases inside the CTE, or (b) no CTE at all.

**Consequences:**
- Any expression using `alias.column` syntax fails at query time with DuckDB errors about missing tables
- The error comes from DuckDB's SQL engine, not from the extension, so the error message is confusing
- Partial fixes (string-replacing `alias.` with nothing) break for column names that contain dots or for expressions like `CASE WHEN orders.status = 'shipped' THEN orders.amount END`

**Prevention:**
Two viable approaches:

**Approach A: Keep CTE, emit aliases inside it (recommended).**
The `_base` CTE already uses `AS "alias"` for table references when `def.tables` is non-empty (lines 415-418, 425-433 of `expand.rs`). Inside the CTE, DuckDB DOES have the aliases in scope. The problem is only in the OUTER SELECT, which references `_base.column`. Fix: move dimension/metric expressions INTO the CTE's SELECT list (as named columns), then the outer SELECT just references `_base."dim_name"` and `_base."met_name"` by their semantic names. The expressions execute inside the CTE where aliases are available.

```sql
-- Current (broken for qualified exprs):
WITH "_base" AS (
    SELECT *
    FROM "orders" AS "o"
    JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
)
SELECT o.region AS "region"  -- ERROR: no "o" in scope
FROM "_base"

-- Fixed: expressions inside CTE
WITH "_base" AS (
    SELECT "o"."region" AS "region", SUM("o"."amount") AS "revenue"
    FROM "orders" AS "o"
    JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
    GROUP BY 1
)
SELECT "region", "revenue"
FROM "_base"
```

Wait -- moving aggregation INTO the CTE changes the semantics. Currently the CTE is `SELECT *` (all rows), and the outer query does `GROUP BY`. If aggregation moves into the CTE, the CTE must include the `GROUP BY`. This is a structural change to the expansion engine.

**Approach B: Drop CTE entirely, use direct FROM/JOIN.**
Generate a direct query without a CTE:

```sql
SELECT "o"."region" AS "region", SUM("o"."amount") AS "revenue"
FROM "orders" AS "o"
JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
WHERE (filter1) AND (filter2)
GROUP BY 1
```

This is simpler, keeps aliases in scope, and avoids the CTE abstraction layer. The downside: the current `LIMIT 0` type inference runs the expanded SQL -- changing the shape may affect type inference if any edge cases depend on the CTE structure.

**Recommendation:** Approach B (drop CTE). The CTE was an early design decision that served its purpose for the flat-namespace model. Qualified columns are a fundamentally different model. Dropping the CTE simplifies the code and aligns with how Snowflake and Cube generate SQL.

**Migration risk:** The `build_execution_sql` function wraps the expansion output in a subquery for type casting. This wrapping does not depend on CTE structure -- it wraps the entire SQL string. So Approach B should be compatible.

**Confidence:** HIGH. The CTE-vs-direct tradeoff is well understood. The codebase has clear separation between expansion (SQL generation) and execution (type inference + vector reference), so the expansion can be changed without affecting execution.

**Phase assignment:** Must be done BEFORE or ALONGSIDE qualified column support. Cannot add qualified columns to the current CTE structure without breakage.

---

### C4: Stored Definitions with ON-Clause Format Cannot Use PK/FK Join Inference

**What goes wrong:**
Existing catalog definitions stored in `semantic_layer._definitions` use the old `Join` format with a raw `on` string field. The model already supports backward compatibility via serde defaults: old JSON without `join_columns` deserializes with an empty `join_columns` vec (tested in `join_old_on_format_backwards_compat`). But the NEW expansion engine using PK/FK join inference will not understand old ON-clause joins.

If the expansion engine is changed to generate ON clauses from `join_columns` (FK/PK pairs) and falls back to the raw `on` string when `join_columns` is empty, this works. But if old definitions are NEVER migrated, the old heuristic transitive closure in `resolve_joins` (which uses `on_lower.contains(&other_lower)` for substring matching) must be kept forever.

**Why it happens:**
The old `resolve_joins` function has TWO responsibilities: (1) determine which joins are needed (from `source_table` fields), and (2) resolve transitive dependencies via ON-clause substring matching. For PK/FK joins, responsibility #2 changes (transitive deps are resolved via the FK graph), but responsibility #1 stays the same.

If you remove the substring-matching transitive closure and replace it with graph-based resolution, old definitions with only `on` strings and no `join_columns` will lose transitive dependency resolution entirely.

**Consequences:**
- Old definitions that relied on transitive join resolution silently stop including needed tables
- Queries return wrong results (missing joins = missing data or cartesian products) with no error

**Prevention:**
- Keep BOTH resolution strategies in `resolve_joins`, selected by the presence of `join_columns`:
  - If ALL joins have `join_columns` non-empty: use graph-based FK resolution
  - If ANY join has only `on` string: use the old substring-matching transitive closure
  - Do NOT mix strategies within a single definition
- Add a define-time validation: if the definition uses the new SQL keyword syntax (which produces `join_columns`), require ALL joins to have `join_columns`. If using the old function-call syntax, all joins use `on` strings.
- The `append_join_on_clause` function already handles both formats (lines 303-333 of `expand.rs`). The resolution strategy selection is the new piece.
- **Confidence:** HIGH. The serde backward compat is already tested. The resolution strategy split is the gap.

**Phase assignment:** Expansion engine phase. Must be decided before `resolve_joins` is modified.

---

## Moderate Pitfalls

### M1: SQL Keyword Parser Must Handle Nested Parentheses in Expressions

**What goes wrong:**
The new SQL DDL syntax uses keyword clauses:
```sql
CREATE SEMANTIC VIEW sales
TABLES (
    o AS orders PRIMARY KEY (order_id),
    c AS customers PRIMARY KEY (customer_id)
)
DIMENSIONS (
    region AS (c.region),
    order_year AS (date_trunc('year', o.order_date))
)
```

Parsing this requires finding the boundaries of each clause. The naive approach (find `DIMENSIONS (` and match its closing `)`) breaks because expressions inside contain nested parentheses: `date_trunc('year', o.order_date)` has its own parentheses.

The existing `scan_clause_keywords` and `validate_brackets` functions already handle bracket depth tracking and string literal escaping. But they scan for CLAUSE KEYWORDS, not for the BOUNDARIES of clause contents.

**Prevention:**
- Build the keyword parser on top of the existing bracket-tracking infrastructure in `parse.rs` (`validate_brackets`, `scan_clause_keywords`).
- Parse clause boundaries by: (1) find keyword, (2) find the opening paren after the keyword, (3) track bracket depth until the matching close paren at depth 0.
- Handle edge cases: string literals containing parentheses (`'hello (world)'`), nested function calls (`COALESCE(a, NULLIF(b, 0))`), array literals (`[1, 2, 3]`), struct literals (`{'key': 'value'}`).
- The bracket validator already handles `()`, `[]`, `{}` nesting and single-quoted strings. Reuse it.
- **Confidence:** HIGH. The infrastructure exists. The gap is using it for clause boundary detection rather than validation.

**Phase assignment:** Parser implementation phase.

---

### M2: PK/FK Validation Must Reject Self-References and Cycles

**What goes wrong:**
The PK/FK model creates a directed graph: each RELATIONSHIP declares `table_A (fk_col) REFERENCES table_B`. If a user accidentally creates a cycle (`A -> B -> C -> A`) or a self-reference (`A -> A`), the join resolution algorithm enters an infinite loop or produces nonsensical SQL.

Snowflake explicitly prohibits both: "You cannot define circular relationships, even through transitive paths" and "currently, a table cannot reference itself."

**Why it happens:**
The define-time validation in the current system does not inspect the join graph topology. Joins are stored as a flat list and resolved at query time. With PK/FK, the graph must be validated at define time.

**Consequences:**
- Cycles: infinite loop in transitive dependency resolution (the fixed-point loop in `resolve_joins` never converges)
- Self-references: the join tries to join a table to itself without distinguishing the two instances, producing nonsensical results or DuckDB errors

**Prevention:**
- At define time, build the FK directed graph and run cycle detection (DFS with a visited set).
- Reject self-referencing relationships with a clear error: "Table 'employees' cannot reference itself. Self-joins require explicit aliases."
- Reject cycles with: "Circular relationship detected: orders -> customers -> orders. Remove one relationship to break the cycle."
- This validation runs in the `create_semantic_view` bind function, before persisting the definition.
- **Confidence:** HIGH. Standard graph validation. DFS cycle detection is O(V+E) on a small graph.

**Phase assignment:** Define-time validation phase, alongside C2 (diamond detection).

---

### M3: Dual DDL Interface Desynchronization During Transition

**What goes wrong:**
The extension maintains two DDL interfaces: function-based (`create_semantic_view('name', ...)`) and native SQL (`CREATE SEMANTIC VIEW name (...)`). Both must produce identical JSON definitions in the catalog. During the v0.5.2 transition, the function-based path continues to accept `:=` syntax with raw struct/list literals, while the new SQL keyword path parses keyword clauses and translates them to JSON.

If the translation logic has subtle differences (e.g., different handling of whitespace in expressions, different quoting of identifiers, different default values for optional fields), the same semantic view defined via two paths will produce different JSON, leading to different query behavior.

**Why it happens:**
The two paths share the CATALOG and EXPANSION code but have different PARSING code. The function-based path receives pre-parsed DuckDB types (STRUCTs and LISTs from DuckDB's expression evaluator), while the SQL keyword path receives raw text that must be parsed by the extension.

**Consequences:**
- A view created via SQL keywords produces different results than the same view created via function calls
- Users report "the same definition gives different results" depending on how it was created
- Debugging is extremely difficult because the difference is in the JSON, not in the SQL

**Prevention:**
- Both paths MUST produce the same `SemanticViewDefinition` JSON. Add property-based tests:
  - Generate a random valid definition
  - Serialize it through the function-call path -> JSON
  - Serialize it through the keyword path -> JSON
  - Assert the JSON is identical (or semantically equivalent after normalization)
- The SQL keyword parser's output should be a `SemanticViewDefinition` struct, not raw JSON. This forces the same model through both paths.
- Add round-trip sqllogictests: create a view via keywords, describe it, create another view via functions with the same parameters, describe it, assert both descriptions match.
- **Confidence:** HIGH. The risk is real but the mitigation (shared model struct) is architecturally sound and already in place.

**Phase assignment:** Testing phase. Add cross-path equivalence tests after both parsers are implemented.

---

### M4: Fan Trap -- Metrics Silently Double-Count Across One-to-Many Joins

**What goes wrong:**
When a metric (e.g., `SUM(amount)`) is defined on a table that is joined to another table via a one-to-many relationship, the join duplicates rows from the "one" side. The SUM then counts each row multiple times.

Example: `orders` (1) -> `line_items` (many). A metric `total_orders = COUNT(*)` on `orders` joined with a dimension from `line_items` will count each order N times (once per line item), inflating the count.

Cube.dev solves this by requiring `primary_key` declarations and generating deduplication subqueries. MetricFlow restricts fan-out joins based on entity types.

**Why it happens:**
The current expansion engine does a simple `JOIN` and then `GROUP BY` in the outer query. It does not detect or prevent fan-out.

**Consequences:**
- Metrics return inflated values with no error or warning
- Users may not notice until comparing results against a known baseline
- This is a fundamental correctness issue that cannot be fixed with post-processing

**Prevention:**
- **For v0.5.2:** Document the constraint: "Metrics and dimensions in the same query should reference the same table or tables joined via many-to-one relationships. Joining a metric's table to a dimension's table via one-to-many will inflate metric values."
- **For v0.5.2:** At define time, if PK/FK is declared, record the relationship cardinality (one-to-many vs many-to-one based on which side has the PK). At query time, warn if a metric's source table is on the "one" side of a one-to-many join that was triggered by a dimension on the "many" side.
- **For future:** Implement Cube-style deduplication: pre-aggregate the metric in a subquery keyed by the PK, then join the pre-aggregated result to the dimension table. This eliminates fan-out but adds complexity.
- **Confidence:** MEDIUM. The problem is well-documented in the literature. The prevention for v0.5.2 is documentation + optional warning, not a full solution. A full solution requires metric-aware expansion.

**Phase assignment:** Documentation for v0.5.2. Full deduplication deferred to a future milestone.

---

### M5: Role-Playing Dimensions -- Same Table Joined Multiple Times With Different Meaning

**What goes wrong:**
A common data model pattern: an `orders` table has `created_date`, `shipped_date`, and `delivered_date`, all referencing a `dates` dimension table. The PK/FK model declares three relationships from `orders` to `dates`, each via a different FK column.

The current join model uses `join.table` as the key for join identity. Two joins to the same table (`dates`) are indistinguishable -- the second one overwrites the first in the `needed` set. Holistics calls this "role-playing dimensions" and flags it as a case where automatic path resolution fails.

**Why it happens:**
The `Join` struct uses `table: String` as its primary identifier. The `resolve_joins` function deduplicates by `table_lower` in a `HashSet`. Two joins to the same table are treated as one.

**Consequences:**
- Only one instance of the joined table is included in the SQL
- Dimensions from other instances silently reference the wrong join, producing wrong results
- No error is raised because the table name IS found in the join list

**Prevention:**
- Use the RELATIONSHIP NAME (or join alias) as the join identifier, not the table name. Snowflake's syntax requires naming each relationship: `orders_to_customers AS orders (o_custkey) REFERENCES customers`.
- The current `TableRef` struct has `alias` and `table` fields. Extend `Join` (or its replacement) to include a `relationship_name` or `alias` field that distinguishes multiple joins to the same physical table.
- At expansion time, use the relationship alias to emit `AS` clauses: `JOIN dates AS "created_dates" ON ...`, `JOIN dates AS "shipped_dates" ON ...`.
- This requires `source_table` on dimensions/metrics to reference the RELATIONSHIP alias, not the physical table name.
- **Confidence:** HIGH. Role-playing dimensions are a well-known pattern. The fix is aliasing, which the model already partially supports via `TableRef.alias`.

**Phase assignment:** Model design phase. Must be addressed in the PK/FK model before implementation.

---

### M6: LIMIT 0 Type Inference Breaks When Expansion Strategy Changes

**What goes wrong:**
The current define-time type inference runs `LIMIT 0` against the expanded SQL to discover column types. The expanded SQL uses the CTE structure (`WITH "_base" AS (...) SELECT ... FROM "_base" GROUP BY ...`). If the expansion strategy changes (C3: dropping CTE for direct FROM/JOIN), the SQL shape changes, and DuckDB may infer different types for the same expressions.

Specific risks:
- CTE wrapping can affect DuckDB's type coercion (e.g., integer promotion inside vs outside CTEs)
- The `build_execution_sql` function wraps the expansion in a subquery and adds casts -- changing the inner SQL may invalidate the wrapper
- Edge case: expressions that reference CTE aliases (e.g., `_base.column`) will fail after CTE removal

**Prevention:**
- Run type inference AFTER the expansion strategy is finalized. Do not mix old-style expansion with new-style type inference.
- The `build_execution_sql` wrapping is independent of CTE structure -- it wraps the entire SQL. Verify this is still true after expansion changes.
- Add sqllogictests that create views with all supported column types and verify query results after the expansion change.
- **Confidence:** MEDIUM. Type inference is robust (VARCHAR fallback on any failure), but behavioral differences between CTE and non-CTE SQL in DuckDB are not well-documented.

**Phase assignment:** Expansion engine phase, after C3 is resolved.

---

## Minor Pitfalls

### m1: SQL Keyword Parsing of Expressions Is Not Full SQL Parsing

**What goes wrong:**
The SQL keyword syntax includes expressions:
```sql
DIMENSIONS (
    region AS (c.region),
    year AS (date_trunc('year', o.order_date))
)
```

The extension must extract the expression text (e.g., `c.region`, `date_trunc('year', o.order_date)`) from within the `DIMENSIONS (...)` clause. This is NOT full SQL parsing -- it is bracket-matching to find expression boundaries. But expressions can contain:
- Commas inside function arguments: `COALESCE(a, b)` -- the comma is NOT an expression separator
- Nested parentheses: `CAST(x AS DATE)`
- String literals with special characters: `CASE WHEN status = 'it''s done' THEN 1 END`
- `AS` keyword inside expressions: `CAST(x AS VARCHAR)` -- the `AS` is NOT the alias separator

**Prevention:**
- Use the same bracket-depth + string-literal tracking from `validate_brackets` to find expression boundaries.
- Expression separators (commas) are only valid at depth 0 (outside all brackets).
- The `AS` keyword is only an alias separator at depth 0, outside string literals.
- Consider requiring expressions to be wrapped in parentheses: `region AS (c.region)`. This makes boundary detection unambiguous -- the expression is everything between the matched parens.
- **Confidence:** HIGH. Bracket-depth parsing is already implemented. The edge cases are known.

**Phase assignment:** Parser implementation phase.

---

### m2: `tables` Field Empty for Legacy Definitions Breaks Qualified Column Resolution

**What goes wrong:**
Legacy definitions (created before v0.5.2) have `tables: []` in their JSON (serde default). If the qualified column resolution code (`find_dimension`, `find_metric`) assumes `tables` is non-empty and uses it for alias lookup, legacy definitions will fail to resolve ANY qualified column lookups -- the alias map is empty.

Currently, `find_dimension` and `find_metric` already handle this: they try qualified lookup (alias match) first, then fall back to bare name. But if the qualified column support adds new code paths that bypass this fallback, legacy definitions break.

**Prevention:**
- All new code that accesses `def.tables` must handle the empty case.
- Add a test: create a legacy definition (no `tables` field), query it with an unqualified dimension name, assert it works.
- Consider: when `tables` is empty, populate it automatically from `base_table` and `joins[].table` with default aliases = table name. This normalization at load time eliminates the empty-tables edge case from all downstream code.
- **Confidence:** HIGH. The existing code handles this. The risk is in NEW code that assumes `tables` is populated.

**Phase assignment:** Model migration phase.

---

### m3: Semicolon Handling in Multi-Statement DDL

**What goes wrong:**
DuckDB's parser extension splits multi-statement input on `;` before passing individual statements to the fallback parser. The current `rewrite_ddl` trims trailing semicolons. But if a SQL DDL body contains semicolons inside string literals (e.g., a filter expression `status != 'a;b'`), DuckDB may split the statement at the wrong place, sending a truncated statement to the parser hook.

This is documented in the v0.5.0 PITFALLS as an existing known issue (citing DuckDB issue #18485). The new keyword syntax does not change this risk, but it does introduce more complex bodies where semicolons in string literals are more likely.

**Prevention:**
- The extension cannot control DuckDB's statement splitting. This is a DuckDB-level limitation.
- Document: "Avoid semicolons in string literals within DDL bodies. If needed, use the function-based DDL interface."
- This is a pre-existing issue, not new to v0.5.2.
- **Confidence:** HIGH. Well-known DuckDB parser extension limitation.

**Phase assignment:** Documentation only.

---

### m4: Keyword Name Collision Between TABLES Clause and DuckDB Keywords

**What goes wrong:**
The SQL keyword syntax introduces `TABLES`, `DIMENSIONS`, `METRICS`, `RELATIONSHIPS`, `FACTS` as clause keywords. If a user chooses a view name or table alias that matches these keywords (e.g., `CREATE SEMANTIC VIEW tables TABLES (...)`), the parser may misidentify the view name as a clause keyword.

**Prevention:**
- Parse view name FIRST (immediately after `CREATE SEMANTIC VIEW`), then parse clause keywords from the remainder.
- The view name position is fixed: always immediately after the DDL prefix, before any clause keyword.
- If the view name matches a clause keyword, it is still the view name -- clause keywords are only valid at the top level of the body, not in the name position.
- Add tests for view names that match clause keywords: `CREATE SEMANTIC VIEW tables TABLES (...)`.
- **Confidence:** HIGH. Positional parsing eliminates ambiguity.

**Phase assignment:** Parser implementation phase.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| SQL keyword parser | C1 (backward compat break), M1 (nested parens), m1 (expression parsing), m4 (keyword collisions) | Build syntax discriminator first; reuse bracket-depth tracking; positional parsing |
| PK/FK model and validation | C2 (diamond joins), M2 (cycles/self-ref), M5 (role-playing dims) | Tree validation at define time; relationship aliases; reject diamonds/cycles |
| Expansion engine changes | C3 (CTE breaks qualified cols), C4 (old ON-clause format), M6 (type inference shape) | Drop CTE for direct FROM/JOIN; keep dual resolution strategy; re-test type inference |
| Dual interface maintenance | M3 (desynchronization) | Shared model struct; cross-path equivalence tests; property-based tests |
| Fan-out prevention | M4 (silent double-counting) | Document constraint; optional warning; defer deduplication to future |
| Legacy compatibility | m2 (empty tables field), C4 (old join format) | Handle empty tables; keep substring-matching for old definitions |

---

## Research Notes

**Confidence Assessment:**

| Area | Confidence | Basis |
|------|------------|-------|
| Backward compat (C1) | HIGH | Direct code review of `rewrite_ddl`, `scan_clause_keywords`, TECH-DEBT.md item #8 |
| Diamond join problem (C2) | HIGH | Cube.dev docs, Snowflake validation rules, Holistics path ambiguity docs, dbt MetricFlow join logic |
| CTE vs qualified columns (C3) | HIGH | Direct analysis of `expand()` code, TECH-DEBT.md item #7 |
| Stored definition compat (C4) | HIGH | Code review of serde defaults in model.rs, existing backward compat tests |
| Fan trap / chasm trap (M4) | MEDIUM | Literature consensus (Cube, Sisense, datacadamia); v0.5.2 mitigation is documentation, not code |
| Role-playing dimensions (M5) | HIGH | Holistics docs, Snowflake relationship naming; existing model already has aliases |

**Sources consulted:**
- [Cube.dev: Working with Joins](https://cube.dev/docs/product/data-modeling/concepts/working-with-joins) -- diamond subgraph detection, primary key requirement for fan/chasm trap prevention
- [Cube.dev: Joins Reference](https://cube.dev/docs/reference/data-model/joins) -- relationship types, deduplication strategy
- [Snowflake: How Snowflake validates semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- circular relationship prohibition, self-reference prohibition
- [Snowflake: CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- PK/FK syntax, relationship naming
- [Snowflake: Semantic View Example](https://docs.snowflake.com/en/user-guide/views-semantic/example) -- TPC-H example with full PK/FK declarations
- [dbt: MetricFlow Join Logic](https://docs.getdbt.com/docs/build/join-logic) -- multi-hop join limits, fan-out prevention via entity types
- [Holistics: Path Ambiguity in Dataset](https://docs.holistics.io/docs/dataset-path-ambiguity) -- tiered path ranking, role-playing dimension handling
- [datacadamia: Fan Trap Issue](https://www.datacadamia.com/data/type/cube/semantic/fan_trap) -- fan trap definition, measure deduplication challenge
- [datacadamia: Chasm Trap Issue](https://datacadamia.com/data/type/cube/semantic/chasm_trap) -- chasm trap via multiple FK references
- [Sisense: Chasm and Fan Traps](https://docs.sisense.com/main/SisenseLinux/chasm-and-fan-traps.htm) -- detection and resolution
- [DuckDB: Runtime-Extensible SQL Parsers](https://duckdb.org/2024/11/22/runtime-extensible-parsers) -- parser hook fallback mechanism
- [boring-semantic-layer: Issue #32](https://github.com/boringdata/boring-semantic-layer/issues/32) -- multiple joins to same dimension table
- This project's `src/expand.rs` -- CTE construction, `resolve_joins`, `find_dimension`, `find_metric`, `append_join_on_clause`
- This project's `src/parse.rs` -- DDL detection, rewriting, validation, bracket tracking
- This project's `src/model.rs` -- `SemanticViewDefinition`, `Join`, `TableRef`, `JoinColumn`, serde defaults
- This project's `src/catalog.rs` -- dual-store catalog, JSON validation
- This project's `TECH-DEBT.md` -- items #6 (ON-clause heuristic), #7 (unqualified columns), #8 (statement rewrite syntax gap)
