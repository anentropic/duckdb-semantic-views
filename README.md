# DuckDB Semantic Views

A DuckDB extension that lets you define dimensions and metrics once, then query them in any combination. The extension writes the GROUP BY and JOIN logic for you.

Inspired by [Snowflake Semantic Views](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view), adapted for DuckDB as a loadable extension.

v0.4.0 -- early-stage, not yet on the community registry.

## How it works

You define a semantic view over one or more tables, declaring:

- **Dimensions** -- columns or expressions to group by (region, category, `date_trunc('month', created_at)`, etc.)
- **Metrics** -- aggregates (`sum(amount)`, `count(*)`, etc.)
- **Relationships** -- join paths between tables, included only when needed

Then you query it by picking which dimensions and metrics you want. The extension figures out the SQL.

## Loading

```sql
LOAD 'semantic_views';
```

Once published to the community registry (not yet):

```sql
INSTALL semantic_views FROM community;
LOAD semantic_views;
```

## Creating a semantic view

`create_semantic_view()` takes a view name and keyword arguments:

```
create_semantic_view(
    name,             -- VARCHAR: view name
    tables,           -- LIST of STRUCT: {alias, table}
    relationships,    -- LIST of STRUCT: join definitions (empty [] for single-table)
    dimensions,       -- LIST of STRUCT: {name, expr, source_table}
    metrics           -- LIST of STRUCT: {name, expr, source_table}
)
```

### Single table

```sql
CREATE TABLE orders (
    id INTEGER, region VARCHAR, category VARCHAR,
    amount DECIMAL(10,2), created_at DATE
);

SELECT * FROM create_semantic_view(
    'orders',
    tables := [
        {'alias': 'o', 'table': 'orders'}
    ],
    dimensions := [
        {'name': 'region', 'expr': 'region', 'source_table': 'o'},
        {'name': 'category', 'expr': 'category', 'source_table': 'o'},
        {'name': 'order_month', 'expr': "date_trunc('month', created_at)", 'source_table': 'o'}
    ],
    metrics := [
        {'name': 'revenue', 'expr': 'sum(amount)', 'source_table': 'o'},
        {'name': 'order_count', 'expr': 'count(*)', 'source_table': 'o'}
    ]
);
```

### Multi-table joins

```sql
SELECT * FROM create_semantic_view(
    'order_analytics',
    tables := [
        {'alias': 'o', 'table': 'orders'},
        {'alias': 'c', 'table': 'customers'}
    ],
    relationships := [
        {'from_table': 'o',
         'to_table': 'c',
         'join_columns': [
            {'from': 'customer_id', 'to': 'id'}
         ]}],
    dimensions := [
        {'name': 'region', 'expr': 'region', 'source_table': 'o'},
        {'name': 'customer_tier', 'expr': 'tier', 'source_table': 'c'}
    ],
    metrics := [
        {'name': 'revenue', 'expr': 'sum(amount)', 'source_table': 'o'}
    ]
);
```

Only the tables needed for your requested dimensions/metrics get joined.

## Querying

```sql
-- Dimensions + metrics
SELECT * FROM semantic_view(
    'orders',
    dimensions := ['region'],
    metrics := ['revenue']
);

-- Multiple of each
SELECT * FROM semantic_view(
    'orders',
    dimensions := ['region', 'category'],
    metrics := ['revenue', 'order_count']
);

-- Dimensions only (returns distinct values)
SELECT * FROM semantic_view(
    'orders',
    dimensions := ['region']
);

-- Metrics only (grand total)
SELECT * FROM semantic_view(
    'orders',
    metrics := ['revenue']
);

-- Date truncation via dimension expr
SELECT * FROM semantic_view(
    'orders',
    dimensions := ['order_month'],
    metrics := ['revenue']
);

-- WHERE works on the result
SELECT * FROM semantic_view(
    'orders',
    dimensions := ['region'],
    metrics := ['revenue']
)
WHERE region = 'EMEA';
```

## Explain

See what SQL the extension generates:

```sql
SELECT * FROM explain_semantic_view(
    'orders',
    dimensions := ['region'],
    metrics := ['revenue']
);
```

Returns the expanded SQL and the DuckDB execution plan.

## Other DDL functions

All use the same argument signature as `create_semantic_view()`:

- `create_or_replace_semantic_view(...)` -- overwrites an existing view
- `create_semantic_view_if_not_exists(...)` -- no-op if already exists
- `drop_semantic_view('name')` -- removes a view
- `drop_semantic_view_if_exists('name')` -- no-op if not found
- `list_semantic_views()` -- table of all registered views
- `describe_semantic_view('name')` -- view metadata

## Building

Rust with a C++ shim, built on the [DuckDB extension template for Rust](https://github.com/duckdb/extension-template-rs).

You need: Rust (stable), just, make, Python 3.

```bash
just setup     # one-time: installs dev tools, configures build
just build     # debug build
cargo test     # unit + property-based tests
just test-sql  # SQL logic tests (needs just build first)
just test-all  # everything
just lint      # fmt + clippy + cargo-deny
```

## License

MIT
