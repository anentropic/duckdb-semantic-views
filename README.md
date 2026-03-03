# DuckDB Semantic Views

DuckDB extension providing semantic views -- a declarative layer for dimensions, measures, and relationships.

**Version:** 0.3.0 | **Status:** Early-stage, pre-community-registry

Semantic views are defined once with named dimensions and metrics, then queried in any combination without writing GROUP BY or JOIN logic by hand. The extension handles SQL expansion; DuckDB handles execution.

## What are Semantic Views?

A semantic view is a reusable definition that maps business concepts (dimensions, metrics, time grains) onto physical tables. Instead of writing aggregation queries from scratch, you define the mapping once:

- **Dimensions** -- columns you group by (e.g., region, category, customer tier)
- **Time dimensions** -- date/timestamp columns with a granularity (e.g., monthly, yearly)
- **Metrics** -- aggregate expressions (e.g., `sum(amount)`, `count(*)`)
- **Relationships** -- join paths between tables, resolved automatically based on which dimensions/metrics you request

When you query a semantic view, the extension generates the appropriate SQL -- selecting only the tables, joins, and aggregations needed for your request.

This extension implements a concept similar to [Snowflake Semantic Views](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view), adapted for DuckDB as a loadable extension.

## Loading

For local builds, load the extension from the build output:

```sql
LOAD 'semantic_views';
```

Community registry installation (not yet published):

```sql
INSTALL semantic_views FROM community;
LOAD semantic_views;
```

## Creating a Semantic View

Use `create_semantic_view()` with 6 positional arguments:

```
create_semantic_view(
    name,             -- VARCHAR: view name
    tables,           -- LIST of STRUCT: {alias, table}
    relationships,    -- LIST of STRUCT: join definitions (empty [] for single-table)
    dimensions,       -- LIST of STRUCT: {name, expr, source_table}
    time_dimensions,  -- LIST of STRUCT: {name, expr, granularity}
    metrics           -- LIST of STRUCT: {name, expr, source_table}
)
```

### Single-table example

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

### Multi-table join example

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

Joins are resolved automatically -- only the tables needed for the requested dimensions and metrics are included in the generated SQL.

## Querying

Use the `semantic_view()` table function with named `dimensions` and `metrics` parameters:

```sql
-- Dimensions + metrics
SELECT * FROM semantic_view('orders',
    dimensions := ['region'],
    metrics := ['revenue']);

-- Multiple dimensions and metrics
SELECT * FROM semantic_view('orders',
    dimensions := ['region', 'category'],
    metrics := ['revenue', 'order_count']);

-- Dimensions only (returns DISTINCT values)
SELECT * FROM semantic_view('orders',
    dimensions := ['region']);

-- Metrics only (grand total)
SELECT * FROM semantic_view('orders',
    metrics := ['revenue']);

-- Time dimension (truncated to defined granularity)
SELECT * FROM semantic_view('orders',
    dimensions := ['order_date'],
    metrics := ['revenue']);

-- WHERE composition (filters applied to the expanded result)
SELECT * FROM semantic_view('orders',
    dimensions := ['region'],
    metrics := ['revenue'])
WHERE region = 'EMEA';
```

## Explain

Use `explain_semantic_view()` to see the expanded SQL the extension generates:

```sql
SELECT * FROM explain_semantic_view('orders',
    dimensions := ['region'],
    metrics := ['revenue']);
```

This returns the full SQL statement and DuckDB execution plan. Useful for debugging or understanding what the semantic view expands into.

## Other DDL Functions

- `create_or_replace_semantic_view(...)` -- overwrites an existing definition
- `create_semantic_view_if_not_exists(...)` -- no-op if the view already exists
- `drop_semantic_view('name')` -- removes a semantic view
- `drop_semantic_view_if_exists('name')` -- no-op if the view is not found
- `list_semantic_views()` -- returns a table of all registered views (columns: `name`, `base_table`)
- `describe_semantic_view('name')` -- returns view metadata (columns: `name`, `base_table`)

All DDL variants use the same 6-argument interface as `create_semantic_view()`.

## Tech Stack and Building

Built in Rust with a C++ shim, on top of the [DuckDB Extension Template for Rust](https://github.com/duckdb/extension-template-rs).

### Prerequisites

- Rust toolchain (stable)
- just (command runner)
- make
- Python 3 (for the sqllogictest runner and integration tests)

### Build commands

```bash
# Debug build (extension binary)
just build

# Rust unit + property-based tests
cargo test

# SQL logic tests (requires just build first)
just test-sql

# Full test suite (Rust + SQL logic + DuckLake integration)
just test-all

# Linting (format check + clippy + cargo-deny)
just lint
```

### First-time setup

```bash
just setup
```

This installs dev tools (cargo-nextest, cargo-deny, cargo-llvm-cov, cargo-fuzz), initializes git submodules, and configures the build environment.

## License

MIT
