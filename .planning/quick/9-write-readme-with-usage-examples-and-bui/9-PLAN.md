---
phase: quick-9
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - README.md
autonomous: true
requirements: []
must_haves:
  truths:
    - "README explains what semantic views are and links to Snowflake prior art"
    - "README shows how to load, create, query, explain, and manage semantic views"
    - "README includes tech stack and build instructions"
  artifacts:
    - path: "README.md"
      provides: "Project README with usage examples and build instructions"
      min_lines: 150
  key_links: []
---

<objective>
Replace the stub README.md with a comprehensive project README covering introduction, usage examples (load, create, query, explain), other DDL functions, and tech stack / build instructions.

Purpose: Give visitors and potential users a clear understanding of what the extension does, how to use it, and how to build it.
Output: Complete README.md at repo root.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@README.md (current stub — to be replaced)
@Cargo.toml (version, description)
@justfile (build commands)
@test/sql/phase4_query.test (real usage examples for DDL + query syntax)
@test/sql/phase2_ddl.test (DDL function examples: create, drop, list, describe)
</context>

<tasks>

<task type="auto">
  <name>Task 1: Write complete README.md</name>
  <files>README.md</files>
  <action>
Replace the current stub README.md with the full project README. Use the following structure and content:

**Header:**
- Title: `# DuckDB Semantic Views`
- One-line description: DuckDB extension providing semantic views -- a declarative layer for dimensions, measures, and relationships.
- Mention current version: v0.3.0
- Note: early-stage / pre-community-registry status

**Section 1: What are Semantic Views?**
- Brief explanation: semantic views let you define dimensions and metrics once, then query any combination without writing GROUP BY or JOIN by hand.
- Link to [Snowflake Semantic Views](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) as inspiration/prior art.
- Clarify this is a DuckDB loadable extension implementing a similar concept.

**Section 2: Loading**
- Show `LOAD 'semantic_views';` for local builds
- Show future community registry pattern: `INSTALL semantic_views FROM community; LOAD semantic_views;` (note: not yet published)

**Section 3: Creating a Semantic View**
- Use `create_semantic_view()` with 6 positional arguments
- Show the function signature with comments explaining each arg:
  1. name (VARCHAR)
  2. tables (LIST of STRUCT with alias + table)
  3. relationships (LIST of STRUCT for joins -- empty [] for single-table)
  4. dimensions (LIST of STRUCT with name, expr, source_table)
  5. time_dimensions (LIST of STRUCT with name, expr, granularity)
  6. metrics (LIST of STRUCT with name, expr, source_table)

- **Single-table example** -- use a realistic orders scenario:
```sql
CREATE TABLE orders (
    id INTEGER, region VARCHAR, category VARCHAR,
    amount DECIMAL(10,2), created_at DATE
);

SELECT create_semantic_view(
    'orders',
    [{'alias': 'o', 'table': 'orders'}],
    [],
    [{'name': 'region', 'expr': 'region', 'source_table': 'o'},
     {'name': 'category', 'expr': 'category', 'source_table': 'o'}],
    [{'name': 'order_date', 'expr': 'created_at', 'granularity': 'month'}],
    [{'name': 'revenue', 'expr': 'sum(amount)', 'source_table': 'o'},
     {'name': 'order_count', 'expr': 'count(*)', 'source_table': 'o'}]
);
```

- **Multi-table join example** -- briefly show joining orders to customers:
```sql
SELECT create_semantic_view(
    'order_analytics',
    [{'alias': 'o', 'table': 'orders'},
     {'alias': 'c', 'table': 'customers'}],
    [{'from_table': 'o', 'to_table': 'c',
      'join_columns': [{'from': 'customer_id', 'to': 'id'}]}],
    [{'name': 'region', 'expr': 'region', 'source_table': 'o'},
     {'name': 'customer_tier', 'expr': 'tier', 'source_table': 'c'}],
    [],
    [{'name': 'revenue', 'expr': 'sum(amount)', 'source_table': 'o'}]
);
```
- Mention that joins are resolved automatically -- only the tables needed for the requested dimensions/metrics are joined.

**Section 4: Querying**
- Show `semantic_view()` table function with named parameters `dimensions` and `metrics`
- Show variations:
  - dims + metrics: `SELECT * FROM semantic_view('orders', dimensions := ['region'], metrics := ['revenue']);`
  - dims only (returns DISTINCT): `SELECT * FROM semantic_view('orders', dimensions := ['region']);`
  - metrics only (grand total): `SELECT * FROM semantic_view('orders', metrics := ['revenue']);`
  - WHERE composition: `SELECT * FROM semantic_view('orders', dimensions := ['region'], metrics := ['revenue']) WHERE region = 'EMEA';`
  - Time dimension: `SELECT * FROM semantic_view('orders', dimensions := ['order_date'], metrics := ['revenue']);`

**Section 5: Explain**
- Show `explain_semantic_view()` which returns the expanded SQL the extension generates:
```sql
SELECT * FROM explain_semantic_view('orders', dimensions := ['region'], metrics := ['revenue']);
```
- Note: useful for debugging or understanding what SQL the semantic view expands into.

**Section 6: Other DDL Functions**
- Brief bulleted list with one-line descriptions:
  - `create_or_replace_semantic_view(...)` -- overwrites an existing definition
  - `create_semantic_view_if_not_exists(...)` -- no-op if already exists
  - `drop_semantic_view('name')` -- removes a semantic view
  - `drop_semantic_view_if_exists('name')` -- no-op if not found
  - `list_semantic_views()` -- returns table of all registered views
  - `describe_semantic_view('name')` -- returns view metadata

**Section 7: Tech Stack and Building**
- Rust + C++ shim, built on the [DuckDB Extension Template for Rust](https://github.com/duckdb/extension-template-rs)
- Prerequisites: Rust toolchain, just, make, Python 3 (for sqllogictest runner)
- Build commands:
  - `just build` -- debug build (extension binary)
  - `cargo test` -- Rust unit + property-based tests
  - `just test-sql` -- SQL logic tests (requires `just build` first)
  - `just test-all` -- full test suite
  - `just lint` -- format check + clippy + cargo-deny

**Section 8: License**
- MIT (from Cargo.toml)

**Style guidelines:**
- Use fenced SQL code blocks with `sql` language tag
- Keep it concise -- this is a README, not a full manual
- No emojis
- Use `##` for sections, `###` for subsections
- Do NOT include a table of contents (the README is not long enough to need one)
  </action>
  <verify>
    <automated>test -f README.md && wc -l README.md | awk '{if ($1 >= 150) print "PASS: "$1" lines"; else print "FAIL: only "$1" lines"}'</automated>
  </verify>
  <done>README.md exists with all 8 sections, realistic SQL examples matching actual extension syntax, Snowflake link, build instructions, and is at least 150 lines.</done>
</task>

</tasks>

<verification>
- README.md contains all required sections (intro, loading, create, query, explain, other DDL, tech stack, license)
- SQL examples use correct syntax matching test/sql/ patterns
- Snowflake link is present
- Build commands match justfile
- Version matches Cargo.toml (0.3.0)
</verification>

<success_criteria>
- README.md is a complete, well-structured project README
- All SQL examples use correct, tested syntax (not pseudo-code)
- A new visitor can understand what the extension does and how to use it
</success_criteria>

<output>
After completion, create `.planning/quick/9-write-readme-with-usage-examples-and-bui/9-SUMMARY.md`
</output>
