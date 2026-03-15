---
phase: quick
plan: 17
type: execute
wave: 1
depends_on: []
files_modified: [README.md]
autonomous: true
requirements: [FACT-01, FACT-05, HIER-01, HIER-03, DRV-01, FAN-01, FAN-02, JOIN-02, ROLE-01]
user_setup: []

must_haves:
  truths:
    - "README.md version updated from v0.5.2 to v0.5.3"
    - "FACTS clause feature briefly documented with SQL example"
    - "Derived metrics feature briefly documented with SQL example"
    - "HIERARCHIES clause feature briefly documented with SQL example"
    - "Cardinality annotations documented in RELATIONSHIPS context"
    - "Fan trap detection briefly documented"
    - "Role-playing dimensions with USING RELATIONSHIPS documented with SQL example"
  artifacts:
    - path: "README.md"
      provides: "Updated project documentation"
      min_lines: 220
  key_links:
    - from: "README.md version line"
      to: "FACTS/HIERARCHIES/cardinality/fan-trap/USING sections"
      via: "sequential flow from feature intro to DDL reference"
      pattern: "v0.5.3 .*FACTS.*HIERARCHIES.*cardinality.*USING"
---

<objective>
Update README.md with brief documentation of v0.5.3 features (FACTS, derived metrics, hierarchies, cardinality, fan trap detection, role-playing dimensions with USING).

Purpose: Communicate new capabilities to users; show feature scope without exhaustive reference docs.
Output: Updated README.md with 5 new feature sections and refreshed version line.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
</execution_context>

<context>
Current README.md (lines 1-171) covers:
- Quick start (single-table semantic view)
- Multi-table (PK/FK relationships)
- explain_semantic_view function
- DDL reference (CREATE/DROP/DESCRIBE/SHOW variants)
- Building instructions

Additions needed after "Multi-table (PK/FK relationships)" section (after line 105) and before "DDL reference" (before line 141).

Features to document (from REQUIREMENTS.md):
- FACTS (FACT-01, FACT-05): Named row-level expressions, inline in metrics
- Derived metrics (DRV-01): Metrics referencing other metrics
- HIERARCHIES (HIER-01, HIER-03): Drill-down paths (pure metadata)
- Cardinality (FAN-01): one_to_one, one_to_many, many_to_one on relationships
- Fan trap detection (FAN-02): Warning when crossing one-to-many boundary
- Role-playing dimensions with USING (ROLE-01, JOIN-02): Multiple join paths with relationship-scoped aliases
</context>

<tasks>

<task type="auto">
  <name>Task 1: Update README.md with v0.5.3 feature sections</name>
  <files>README.md</files>
  <action>
1. Change version line 7 from "v0.5.2" to "v0.5.3"

2. After the "Multi-table (PK/FK relationships)" section (after the closing ``` on line 105), add five new feature sections in this order:

**Section A: FACTS (row-level expressions)**
   - Brief intro: "Reuse common row-level expressions across metrics"
   - Show FACTS clause syntax: `FACTS (alias.fact_name AS expression)`
   - Small example: Add a fact like `o.discount_flag AS o.amount < 100` to the existing analytics example
   - Metric referencing the fact: `o.low_value_orders AS count(*) WHERE o.discount_flag`
   - Mention: "Facts are inlined into metric expressions at expansion time"

**Section B: Derived Metrics (metric composition)**
   - Brief intro: "Combine metrics without table prefixes"
   - Show syntax: `metric_name AS base_metric_a + base_metric_b`
   - Example: Add derived metric to analytics: `o.revenue_per_order AS o.revenue / o.order_count`
   - Mention: "Derived metrics expand by substituting base metric expressions"

**Section C: Hierarchies (drill-down metadata)**
   - Brief intro: "Define drill-down paths for dimension hierarchies"
   - Show syntax: `HIERARCHIES (hierarchy_name AS (dim1, dim2, dim3))`
   - Example: `product_hierarchy AS (product_category, product_name)` or `date_hierarchy AS (year, month, day)`
   - Mention: "Pure metadata for discovery; not used in query expansion"

**Section D: Cardinality & Fan Trap Detection**
   - Brief intro: "Declare relationship cardinality; detect queries that cross one-to-many boundaries"
   - Show cardinality syntax in RELATIONSHIPS: `order_customer AS o(customer_id) REFERENCES c [MANY TO ONE]`
   - Explain: one_to_one, one_to_many, many_to_one annotations (optional, default is many_to_one)
   - Explain fan trap: "If a one-to-many relationship exists between metric and dimension sources, expansion emits a warning (query still runs)"
   - Example: querying customer-level metric with order-level dimension in same query; extension warns "may inflate results"

**Section E: Role-Playing Dimensions with USING**
   - Brief intro: "Same table joined via different relationships produces distinct role instances"
   - Show USING syntax in METRICS: `metric_name AS agg_expr USING (relationship_name)`
   - Example: flights table with departure_airport and arrival_airport relationships
     ```sql
     RELATIONSHIPS (
       flight_departure AS f(departure_code) REFERENCES a,
       flight_arrival AS f(arrival_code) REFERENCES a
     )
     DIMENSIONS (
       a.departure_city AS a.city,  -- uses flight_departure
       a.arrival_city AS a.city     -- uses flight_arrival
     )
     METRICS (
       f.flight_count AS count(*) USING (flight_departure)
     )
     ```
   - Mention: "USING selects which join path the metric should use; without it, ambiguous queries error"

3. Update the DDL reference section (lines 141-151) to mention FACTS and HIERARCHIES clauses:
   - Change "CREATE SEMANTIC VIEW name AS ...;" to include: "TABLES (...) RELATIONSHIPS (...) FACTS (...) DIMENSIONS (...) HIERARCHIES (...) METRICS (...)"
   - Add note: "FACTS, HIERARCHIES optional; DIMENSIONS, METRICS required"

Keep the tone consistent with existing README (brief, example-driven, not exhaustive). Each section should be 5-8 lines plus a short SQL block.
  </action>
  <verify>
    <automated>grep -q "v0.5.3" README.md && grep -q "FACTS" README.md && grep -q "HIERARCHIES" README.md && grep -q "Cardinality" README.md && grep -q "USING" README.md && wc -l README.md | awk '{if ($1 > 220) print "PASS"; else print "FAIL: " $1 " lines"}'</automated>
  </verify>
  <done>
    - README.md version line updated to v0.5.3
    - FACTS section added with brief intro, syntax, and metric usage example
    - Derived metrics section added with intro, syntax, and example
    - HIERARCHIES section added with intro, syntax, and example
    - Cardinality and fan trap detection section added with explanation and warning behavior
    - Role-playing dimensions with USING section added with flights example
    - DDL reference updated to show FACTS and HIERARCHIES clauses in syntax
    - Total line count > 220 (expanded from 171)
  </done>
</task>

</tasks>

<verification>
All sections present and readable: `grep -E "^## " README.md | head -20`
Version correct: `head -10 README.md | grep v0.5.3`
Feature keywords present: `grep -E "FACTS|HIERARCHIES|Cardinality|USING|Role-playing|Fan trap" README.md`
No broken syntax or example blocks: Manual verification in context
</verification>

<success_criteria>
- README.md updated with version v0.5.3
- Five new feature sections added between multi-table and DDL reference
- Each section has intro, syntax block, and brief example or explanation
- Tone matches existing README (concise, example-driven)
- DDL reference shows FACTS and HIERARCHIES clauses
- File is valid markdown with no syntax errors
- Total expanded to 220+ lines (from current 171)
</success_criteria>

<output>
After completion, commit:
```bash
git add README.md
git commit -m "docs: update README.md with v0.5.3 features (FACTS, derived metrics, hierarchies, cardinality, fan traps, role-playing USING)"
```
</output>
