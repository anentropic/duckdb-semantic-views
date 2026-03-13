# DuckDB Semantic Views

A DuckDB extension that lets you define dimensions and metrics once, then query them in any combination. The extension writes the GROUP BY and JOIN logic for you.

Inspired by [Snowflake Semantic Views](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view), adapted for DuckDB as a loadable extension.

v0.5.2 -- early-stage, not yet on the community registry.

## How it works

You define a semantic view over one or more tables, declaring:

- **Dimensions** -- columns or expressions to group by (region, category, `date_trunc('month', created_at)`, etc.)
- **Metrics** -- aggregates (`sum(amount)`, `count(*)`, etc.)
- **Relationships** -- PK/FK join paths between tables, included only when the query needs them

Then you query by picking which dimensions and metrics you want. The extension generates the SQL -- SELECT, FROM, JOIN, GROUP BY -- and DuckDB executes it.

## Quick start

```sql
CREATE TABLE orders (
    id INTEGER, region VARCHAR, category VARCHAR,
    amount DECIMAL(10,2)
);

CREATE SEMANTIC VIEW order_metrics AS
TABLES (
    o AS orders PRIMARY KEY (id)
)
DIMENSIONS (
    o.region AS o.region,
    o.category AS o.category
)
METRICS (
    o.revenue AS sum(o.amount),
    o.order_count AS count(*)
);

-- Pick any combination of dimensions and metrics
SELECT * FROM semantic_view('order_metrics',
    dimensions := ['region', 'category'],
    metrics := ['revenue', 'order_count']
);

-- Dimensions only (distinct values)
SELECT * FROM semantic_view('order_metrics',
    dimensions := ['region']
);

-- Metrics only (grand total)
SELECT * FROM semantic_view('order_metrics',
    metrics := ['revenue']
);

-- WHERE works on the result
SELECT * FROM semantic_view('order_metrics',
    dimensions := ['region'], metrics := ['revenue']
) WHERE region = 'East';
```

## Multi-table (PK/FK relationships)

Define relationships between tables with PRIMARY KEY and REFERENCES. Only the tables needed for your requested dimensions and metrics get joined.

```sql
CREATE TABLE customers (id INTEGER, name VARCHAR, tier VARCHAR);
CREATE TABLE products (id INTEGER, name VARCHAR, category VARCHAR);
CREATE TABLE orders (
    id INTEGER, customer_id INTEGER, product_id INTEGER,
    amount DECIMAL(10,2), region VARCHAR
);

CREATE SEMANTIC VIEW analytics AS
TABLES (
    o AS orders PRIMARY KEY (id),
    c AS customers PRIMARY KEY (id),
    p AS products PRIMARY KEY (id)
)
RELATIONSHIPS (
    order_customer AS o(customer_id) REFERENCES c,
    order_product AS o(product_id) REFERENCES p
)
DIMENSIONS (
    c.customer_name AS c.name,
    p.product_name AS p.name,
    o.region AS o.region
)
METRICS (
    o.revenue AS sum(o.amount),
    o.order_count AS count(*)
);

-- Only customers table is joined (products not needed)
SELECT * FROM semantic_view('analytics',
    dimensions := ['customer_name'],
    metrics := ['revenue']
);

-- Both customers and products tables are joined
SELECT * FROM semantic_view('analytics',
    dimensions := ['customer_name', 'product_name'],
    metrics := ['revenue']
);
```

See the generated SQL with `explain_semantic_view`:

```sql
SELECT * FROM explain_semantic_view('analytics',
    dimensions := ['customer_name'],
    metrics := ['revenue']
);
```

```
┌──────────────────────────────────────────────────────────────┐
│                        explain_output                        │
│                           varchar                            │
├──────────────────────────────────────────────────────────────┤
│ -- Semantic View: analytics                                  │
│ -- Dimensions: customer_name                                 │
│ -- Metrics: revenue                                          │
│                                                              │
│ -- Expanded SQL:                                             │
│ SELECT                                                       │
│     c.name AS "customer_name",                               │
│     sum(o.amount) AS "revenue"                               │
│ FROM "orders" AS "o"                                         │
│ LEFT JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id" │
│ GROUP BY                                                     │
│     1                                                        │
│                                                              │
│ -- DuckDB Plan:                                              │
│ ...                                                          │
├──────────────────────────────────────────────────────────────┤
│ 15+ rows                                                     │
└──────────────────────────────────────────────────────────────┘
```

## DDL reference

```sql
CREATE SEMANTIC VIEW name AS ...;
CREATE OR REPLACE SEMANTIC VIEW name AS ...;
CREATE SEMANTIC VIEW IF NOT EXISTS name AS ...;
DROP SEMANTIC VIEW name;
DROP SEMANTIC VIEW IF EXISTS name;
DESCRIBE SEMANTIC VIEW name;
SHOW SEMANTIC VIEWS;
```

## Building

Rust, built on the [DuckDB extension template for Rust](https://github.com/duckdb/extension-template-rs).

You need: Rust (stable), just, make, Python 3.

```bash
just setup     # one-time: installs dev tools, configures build
just build     # debug build
cargo test     # unit + property-based tests
just test-sql  # SQL logic tests (needs just build first)
just test-all  # everything
just lint       # fmt + clippy + cargo-deny
```

## License

MIT
