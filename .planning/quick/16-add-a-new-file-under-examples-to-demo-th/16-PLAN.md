---
phase: quick-16
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - examples/advanced_features.py
autonomous: true
requirements: []

must_haves:
  truths:
    - "Script runs end-to-end with `uv run examples/advanced_features.py` after `just build`"
    - "Script demonstrates FACTS clause with fact-referencing-fact chain"
    - "Script demonstrates HIERARCHIES clause with dimension drill-down"
    - "Script demonstrates derived metrics with stacking (profit, margin)"
    - "Script demonstrates cardinality annotations and fan trap detection error"
    - "Script demonstrates role-playing dimensions with USING RELATIONSHIPS"
    - "Each section prints labeled output showing correct computed results"
  artifacts:
    - path: "examples/advanced_features.py"
      provides: "Self-contained Python demo of all v0.5.3 features"
      min_lines: 150
  key_links:
    - from: "examples/advanced_features.py"
      to: "build/debug/semantic_views.duckdb_extension"
      via: "LOAD extension at script start"
      pattern: "LOAD.*semantic_views"
---

<objective>
Create `examples/advanced_features.py` -- a self-contained Python script that demonstrates all v0.5.3 advanced semantic features with realistic data and printed output.

Purpose: Give users a runnable demo of FACTS, HIERARCHIES, derived metrics, cardinality/fan trap detection, and role-playing dimensions with USING RELATIONSHIPS.
Output: A single Python file runnable via `uv run examples/advanced_features.py`.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@examples/basic_ddl_and_query.py (existing example -- follow this style exactly)
@test/sql/phase29_facts_hierarchies.test (DDL syntax for FACTS/HIERARCHIES)
@test/sql/phase30_derived_metrics.test (DDL syntax for derived metrics)
@test/sql/phase31_fan_trap.test (DDL syntax for cardinality + fan trap errors)
@test/sql/phase32_role_playing.test (DDL syntax for role-playing + USING)
</context>

<tasks>

<task type="auto">
  <name>Task 1: Create the advanced_features.py example script</name>
  <files>examples/advanced_features.py</files>
  <action>
Create `examples/advanced_features.py` following the exact style/conventions from `basic_ddl_and_query.py`:
- PEP 723 inline metadata header (`# /// script` / `# dependencies = ["duckdb==1.4.4"]` / `# requires-python = ">=3.9"` / `# ///`)
- Docstring with `uv run examples/advanced_features.py`
- Connect with `allow_unsigned_extensions: true`, LOAD from `build/debug/semantic_views.duckdb_extension`
- Print section headers with `=== Section Name ===` formatting
- Use `fetchall()` with `for row in ...` loops and `fetchone()` for single rows

The script should demonstrate these features in order, each with its own section header and printed output:

**Section 1: Setup -- Create physical tables**
Create an e-commerce schema with realistic data:
- `line_items` table: id, order_id, extended_price DECIMAL(10,2), discount DECIMAL(3,2), tax_rate DECIMAL(3,2), unit_cost DECIMAL(10,2)
  Data: (1, 1, 100.00, 0.10, 0.05, 50.00), (2, 1, 200.00, 0.20, 0.08, 80.00), (3, 2, 150.00, 0.00, 0.10, 60.00)
- `orders` table: id INTEGER, customer_id INTEGER, region VARCHAR
  Data: (1, 10, 'East'), (2, 20, 'West')
- `customers` table: id INTEGER, name VARCHAR, country VARCHAR, state VARCHAR, city VARCHAR
  Data: (10, 'Alice', 'US', 'NY', 'New York'), (20, 'Bob', 'US', 'CA', 'Los Angeles')

**Section 2: FACTS clause -- Reusable row-level expressions**
Create semantic view `sales` with:
- TABLES: li AS line_items PK(id), o AS orders PK(id), c AS customers PK(id)
- RELATIONSHIPS: li_to_order AS li(order_id) REFERENCES o MANY TO ONE, order_to_customer AS o(customer_id) REFERENCES c MANY TO ONE
- FACTS: li.net_price AS li.extended_price * (1 - li.discount), li.tax_amount AS li.net_price * li.tax_rate  (demonstrates fact-referencing-fact)
- HIERARCHIES: geo AS (country, state, city)
- DIMENSIONS: o.region AS o.region, c.country AS c.country, c.state AS c.state, c.city AS c.city
- METRICS: li.total_net AS SUM(li.net_price), li.total_tax AS SUM(li.tax_amount), li.total_cost AS SUM(li.unit_cost)

Query and print:
- "Revenue by region" -- dimensions=['region'], metrics=['total_net', 'total_tax'] ORDER BY region
  Expected: East=(250.00, 17.30), West=(150.00, 15.00)
- Comment explaining the fact chain: net_price = price * (1 - discount), tax_amount = net_price * tax_rate

**Section 3: HIERARCHIES -- Drill-down paths (metadata)**
- DESCRIBE SEMANTIC VIEW sales to show hierarchies in output
- Print the hierarchies column (last column, index 7) showing the geo hierarchy definition
- Comment: "Hierarchies are metadata -- they document drill-down paths but don't affect query execution"

**Section 4: Derived metrics -- Metric-on-metric composition**
DROP and re-CREATE `sales` with same schema plus derived metrics:
- Same TABLES, RELATIONSHIPS, FACTS as section 2
- Same DIMENSIONS
- METRICS: li.revenue AS SUM(li.net_price), li.cost AS SUM(li.unit_cost), profit AS revenue - cost, margin AS profit / revenue * 100

Query and print:
- "Profitability by region" -- dimensions=['region'], metrics=['revenue', 'cost', 'profit'] ORDER BY region
  Expected: East=(250.00, 130.00, 120.00), West=(150.00, 60.00, 90.00)
- "Margin by region" -- wrap in subquery for ROUND: SELECT region, ROUND(margin, 1) FROM (semantic_view(..., metrics=['margin'])) ORDER BY region
  Expected: East=48.0, West=60.0
- "Grand total profit" -- metrics=['profit'] only, no dimensions
  Expected: 210.00

**Section 5: Fan trap detection -- Cardinality-aware safety**
Note: the semantic view already has `MANY TO ONE` on li_to_order. Demonstrate:
- Safe query: dimensions=['region'], metrics=['revenue'] -- succeeds (li->o is MANY TO ONE, safe direction)
  Print result.
- Unsafe query: dimensions=['region'], metrics=['order_count'] where order_count is defined on o -- but wait, we need order_count. DROP and re-CREATE to add `o.order_count AS COUNT(*)`.
  Actually, simpler approach: just show that querying an o-sourced metric with a dimension that requires traversing the fan-out direction is blocked.

Revised approach for fan trap section:
- DROP SEMANTIC VIEW sales
- CREATE SEMANTIC VIEW fan_trap_demo with:
  TABLES: o AS orders PK(id), li AS line_items PK(id)
  RELATIONSHIPS: li_to_order AS li(order_id) REFERENCES o MANY TO ONE
  DIMENSIONS: o.region AS o.region, li.status AS li.extended_price  (use a li-sourced dimension -- reuse extended_price or just use a CASE expr)

  Actually, let's keep it simpler and more realistic. Use a simplified schema:
  DIMENSIONS: o.region AS o.region
  METRICS: li.revenue AS SUM(li.extended_price), o.order_count AS COUNT(*)

  - Safe: dimensions=['region'], metrics=['revenue'] -- works (li->o MANY TO ONE, safe)
  - Fan trap: Attempt to query order_count (from o) grouped by a line_items dimension. But we don't have a li dimension defined...

  Better plan: Define a li-sourced dimension too:
  Add a `status` column to line_items, add dimension li.price_tier AS CASE WHEN li.extended_price > 100 THEN 'high' ELSE 'low' END

  Then: dimensions=['price_tier'], metrics=['order_count'] -- this requires traversing o->li (reverse of MANY TO ONE = ONE TO MANY fan-out), so it should be blocked.

Actually, simplest approach matching the test patterns: just add status to line_items directly. But we already created the table without it. So either ALTER TABLE or use a computed expression. Let's use a CASE expression as dimension.

Final fan trap approach:
- DROP SEMANTIC VIEW sales (from section 4)
- CREATE SEMANTIC VIEW fan_trap_demo:
  TABLES: o AS orders PK(id), li AS line_items PK(id)
  RELATIONSHIPS: li_to_order AS li(order_id) REFERENCES o MANY TO ONE
  DIMENSIONS: o.region AS o.region, li.price_tier AS CASE WHEN li.extended_price > 100 THEN 'high' ELSE 'low' END
  METRICS: li.revenue AS SUM(li.extended_price), o.order_count AS COUNT(*)

- Safe query: dimensions=['region'], metrics=['revenue'] -- print result
- Fan trap query: try/except around dimensions=['price_tier'], metrics=['order_count']
  Print the error message showing "fan trap detected"
- Comment: "Fan trap: order_count is sourced from orders, but price_tier requires joining line_items. Traversing orders->line_items is ONE TO MANY (fan-out), which would inflate the count."

**Section 6: Role-playing dimensions with USING RELATIONSHIPS**
- DROP fan_trap_demo
- Create airports table: airport_code VARCHAR, city VARCHAR, country VARCHAR
  Data: ('SFO', 'San Francisco', 'US'), ('JFK', 'New York', 'US'), ('LHR', 'London', 'UK')
- Create flights table: flight_id INTEGER, departure_code VARCHAR, arrival_code VARCHAR, carrier VARCHAR
  Data: (1, 'SFO', 'JFK', 'AA'), (2, 'JFK', 'LHR', 'BA'), (3, 'LHR', 'SFO', 'AA')

CREATE SEMANTIC VIEW flight_analytics AS
  TABLES: f AS flights PK(flight_id), a AS airports PK(airport_code)
  RELATIONSHIPS: dep_airport AS f(departure_code) REFERENCES a, arr_airport AS f(arrival_code) REFERENCES a
  DIMENSIONS: a.city AS a.city, a.country AS a.country, f.carrier AS f.carrier
  METRICS: f.departure_count USING (dep_airport) AS COUNT(*), f.arrival_count USING (arr_airport) AS COUNT(*), total_flights AS departure_count + arrival_count

Query and print:
- "Departures by city" -- dimensions=['city'], metrics=['departure_count'] ORDER BY city
  Expected: London=1, New York=1, San Francisco=1
- "Arrivals by city" -- dimensions=['city'], metrics=['arrival_count'] ORDER BY city
  Expected: London=1, New York=1, San Francisco=1
- "Flights by carrier (non-ambiguous dimension)" -- dimensions=['carrier'], metrics=['departure_count', 'arrival_count'] ORDER BY carrier
  Expected: AA=(2, 2), BA=(1, 1)
- "Ambiguous query" -- try/except: dimensions=['city'], metrics=['total_flights']
  Print error showing ambiguity (total_flights references both USING paths, city is from airports)
- "Total flights by carrier (derived, non-ambiguous)" -- dimensions=['carrier'], metrics=['total_flights'] ORDER BY carrier
  Expected: AA=4, BA=2

**Section 7: EXPLAIN -- See the generated SQL for role-playing**
- explain_semantic_view('flight_analytics', dimensions=['city'], metrics=['departure_count'])
- Print the SQL to show scoped aliases in the generated JOIN

**Section 8: DESCRIBE -- Full metadata view**
- DESCRIBE SEMANTIC VIEW flight_analytics
- Print selected columns showing the relationships, metrics (with USING), etc.

Use try/except for error demonstration (fan trap and ambiguity). Print the error message content.
Ensure all SQL matches the verified DDL syntax from the sqllogictest files exactly.
Do NOT include any cleanup (DROP statements) -- this is a demo script, not a test.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && just build && uv run examples/advanced_features.py 2>&1 | tail -50</automated>
  </verify>
  <done>
    - Script runs successfully end-to-end with `uv run examples/advanced_features.py`
    - Each section prints labeled output with correct computed results
    - Fan trap detection error is caught and printed
    - Ambiguous role-playing error is caught and printed
    - EXPLAIN shows generated SQL with scoped aliases
    - Script follows exact same style as basic_ddl_and_query.py (PEP 723, extension loading, print formatting)
  </done>
</task>

</tasks>

<verification>
```bash
cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && just build && uv run examples/advanced_features.py
```
Script should run without errors and print output for all sections with correct values.
</verification>

<success_criteria>
- `examples/advanced_features.py` exists and is a valid PEP 723 script
- `uv run examples/advanced_features.py` runs end-to-end after `just build`
- All six v0.5.3 features are demonstrated: FACTS, HIERARCHIES, derived metrics, cardinality/fan trap, role-playing/USING, EXPLAIN/DESCRIBE
- Output values match expected calculations
- Error cases (fan trap, ambiguity) are caught and their messages displayed
</success_criteria>

<output>
After completion, create `.planning/quick/16-add-a-new-file-under-examples-to-demo-th/16-SUMMARY.md`
</output>
