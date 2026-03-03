# Phase 11.1 Design: DDL and Query Syntax Alignment

## Section 1: Summary

Phase 11.1 reshapes the DDL and query interface to align with Snowflake semantic view concepts. No new semantic capabilities are introduced — this is an interface-only change. The JSON-string DDL argument is replaced with typed positional STRUCT/LIST arguments, and the query function is renamed from `semantic_query` to `semantic_view`. The internal data model is extended with `TableRef` and `JoinColumn` structs to hold the typed information previously encoded in raw JSON strings.

## Section 2: Before/After — DDL

### BEFORE (old 2-arg JSON interface)

```sql
SELECT define_semantic_view('sales_view', '{
    "base_table": "orders",
    "joins": [{"table": "customers", "on": "orders.customer_id = customers.id"}],
    "dimensions": [{"name": "region", "expr": "o.region", "source_table": "customers"}],
    "metrics": [{"name": "revenue", "expr": "sum(o.amount)"}]
}');
```

### AFTER (new 6-arg STRUCT/LIST interface)

```sql
SELECT define_semantic_view(
    'sales_view',
    [{'alias': 'o', 'table': 'orders'}, {'alias': 'c', 'table': 'customers'}],
    [{'from_table': 'o', 'to_table': 'c', 'join_columns': [{'from': 'customer_id', 'to': 'id'}]}],
    [{'name': 'region', 'expr': 'o.region', 'source_table': 'o'}],
    [{'name': 'order_date', 'expr': 'o.created_at', 'granularity': 'day'}],
    [{'name': 'revenue', 'expr': 'sum(o.amount)', 'source_table': 'o'}]
);
```

### Convenience pattern — simple single-table view (no joins, no time dimensions)

```sql
SELECT define_semantic_view(
    'orders_view',
    [{'alias': 'o', 'table': 'orders'}],
    [],                                         -- relationships: empty
    [{'name': 'region', 'expr': 'o.region', 'source_table': 'o'}],
    [],                                         -- time_dimensions: empty
    [{'name': 'revenue', 'expr': 'sum(o.amount)', 'source_table': 'o'}]
);
```

### define_or_replace variant

```sql
SELECT define_or_replace_semantic_view(
    'orders_view',
    [{'alias': 'o', 'table': 'orders'}],
    [],
    [{'name': 'region', 'expr': 'o.region', 'source_table': 'o'}],
    [],
    [{'name': 'revenue', 'expr': 'sum(o.amount)', 'source_table': 'o'}]
);
```

### Drop variants (UNCHANGED — still take name only)

```sql
SELECT drop_semantic_view('orders_view');
SELECT drop_semantic_view_if_exists('nonexistent_view');
```

## Section 3: Before/After — Query Function

### BEFORE

```sql
SELECT * FROM semantic_query('sales_view', dimensions := ['region'], metrics := ['revenue']);
```

### AFTER

```sql
SELECT * FROM semantic_view('sales_view', dimensions := ['region'], metrics := ['revenue']);
```

### Table-qualified dimension and metric names (new in 11.1)

```sql
SELECT * FROM semantic_view('sales_view', dimensions := ['o.region', 'c.tier'], metrics := ['revenue']);
```

The `alias.name` prefix (e.g., `o.region`) resolves the dimension or metric by matching both the bare name and `source_table == alias`. Falls back to bare-name lookup if no qualified match is found.

## Section 4: Parameter Types

The `define_semantic_view` VScalar function takes 6 positional arguments:

| Position | DuckDB Type | Description |
|----------|-------------|-------------|
| 0 | `VARCHAR` | View name |
| 1 | `LIST(STRUCT(alias VARCHAR, "table" VARCHAR))` | Table alias registry |
| 2 | `LIST(STRUCT(from_table VARCHAR, to_table VARCHAR, join_columns LIST(STRUCT("from" VARCHAR, "to" VARCHAR))))` | Join relationships |
| 3 | `LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))` | Dimensions |
| 4 | `LIST(STRUCT(name VARCHAR, expr VARCHAR, granularity VARCHAR))` | Time dimensions (granularity: `day`\|`week`\|`month`\|`year`) |
| 5 | `LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))` | Metrics |

**Note:** VScalar does not support named parameters — these are positional. Use `[]` (empty list) for unused list arguments.

## Section 5: Internal Model Changes

Two new structs are added to `src/model.rs`:

- **`TableRef`** — maps a short alias (e.g., `"o"`) to a physical table name (e.g., `"orders"`). Added to `SemanticViewDefinition.tables: Vec<TableRef>` with `#[serde(default)]`.
- **`JoinColumn`** — a column-pair relationship entry with `from` and `to` fields. Added to `Join.join_columns: Vec<JoinColumn>` with `#[serde(default)]`.

The existing `Join.on` (raw ON clause string) and `Join.from_cols` (Phase 11 array format) are retained for backward compatibility with stored JSON. Old definitions still deserialize correctly via serde defaults. New definitions write `join_columns` instead.

## Section 6: Snowflake Alignment

This interface aligns with Snowflake semantic view concepts and terminology. Snowflake uses `time_dimensions` as a separate list (distinct from regular dimensions), which is reflected in arg 4. The `join_columns` structure with `from`/`to` pair fields maps to Snowflake's `relationship_columns` which uses `left_column`/`right_column` — we use shorter field names for ergonomics. The `alias`/`table` pairing in the tables list corresponds to Snowflake's table-alias concept in semantic views. The `source_table` field in dimensions and metrics uses the alias (not the physical table name) to reference the owning table.

Reference: https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec
