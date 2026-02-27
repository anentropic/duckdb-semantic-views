---
phase: quick-1
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/expand.rs
autonomous: true
requirements: [QUICK-FIX-01]

must_haves:
  truths:
    - "Dot-qualified table names like 'jaffle.raw_orders' expand to '\"jaffle\".\"raw_orders\"' in generated SQL"
    - "Single-part table names like 'orders' still expand to '\"orders\"' (no regression)"
    - "Join table names with dots are also properly split and quoted"
    - "DuckLake integration test passes with dot-qualified base_table"
  artifacts:
    - path: "src/expand.rs"
      provides: "quote_table_ref function + updated expand() to use it for base_table and join tables"
      exports: ["quote_table_ref", "quote_ident", "expand"]
  key_links:
    - from: "src/expand.rs::expand()"
      to: "src/expand.rs::quote_table_ref()"
      via: "base_table and join.table quoting"
      pattern: "quote_table_ref.*base_table|quote_table_ref.*join\\.table"
---

<objective>
Fix dot-qualified table name handling in the expansion engine so that table references like `jaffle.raw_orders` are split into properly quoted parts (`"jaffle"."raw_orders"`) instead of being treated as a single monolithic identifier (`"jaffle.raw_orders"`).

Purpose: The DuckLake/Iceberg integration test defines `base_table: "jaffle.raw_orders"` where `jaffle` is an attached catalog and `raw_orders` is the table. The current `quote_ident()` wraps the entire string in one set of double quotes, which DuckDB interprets as a single identifier rather than a catalog-qualified reference.

Output: Updated `src/expand.rs` with a new `quote_table_ref` function and updated call sites, plus unit tests covering all cases.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src/expand.rs
@src/model.rs
@test/integration/test_ducklake.py

<interfaces>
From src/expand.rs:
```rust
/// Double-quote a SQL identifier (single part only).
pub fn quote_ident(ident: &str) -> String;

/// Expand a semantic view definition into SQL.
pub fn expand(view_name: &str, def: &SemanticViewDefinition, req: &QueryRequest) -> Result<String, ExpandError>;
```

From src/model.rs:
```rust
pub struct SemanticViewDefinition {
    pub base_table: String,      // Can be "orders" or "jaffle.raw_orders"
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
    pub filters: Vec<String>,
    pub joins: Vec<Join>,        // Join.table can also be dot-qualified
}
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add quote_table_ref function and update expand() call sites</name>
  <files>src/expand.rs</files>
  <action>
Add a new public function `quote_table_ref` that handles dot-qualified table references by splitting on `.` and quoting each part individually with `quote_ident`:

```rust
/// Quote a potentially dot-qualified table reference.
///
/// Splits on `.` and quotes each part individually. This handles:
/// - Simple names: `orders` -> `"orders"`
/// - Catalog-qualified: `jaffle.raw_orders` -> `"jaffle"."raw_orders"`
/// - Fully qualified: `catalog.schema.table` -> `"catalog"."schema"."table"`
///
/// Each part is quoted via `quote_ident`, so embedded double quotes are escaped.
#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    table
        .split('.')
        .map(|part| quote_ident(part))
        .collect::<Vec<_>>()
        .join(".")
}
```

Place it immediately after the existing `quote_ident` function (after line 139).

Then update two call sites in `expand()`:
1. Line 299: Change `quote_ident(&def.base_table)` to `quote_table_ref(&def.base_table)`
2. Line 304: Change `quote_ident(&join.table)` to `quote_table_ref(&join.table)`

Do NOT change the `quote_ident` calls for dimension/metric name aliases (lines 330, 333) -- those are column aliases which are always single identifiers.

Add unit tests in the existing `quote_ident_tests` module (or a new sibling `quote_table_ref_tests` module):

```rust
mod quote_table_ref_tests {
    use super::*;

    #[test]
    fn simple_table_name() {
        assert_eq!(quote_table_ref("orders"), "\"orders\"");
    }

    #[test]
    fn catalog_qualified() {
        assert_eq!(quote_table_ref("jaffle.raw_orders"), "\"jaffle\".\"raw_orders\"");
    }

    #[test]
    fn fully_qualified() {
        assert_eq!(quote_table_ref("catalog.schema.table"), "\"catalog\".\"schema\".\"table\"");
    }

    #[test]
    fn reserved_word_parts() {
        assert_eq!(quote_table_ref("select.from"), "\"select\".\"from\"");
    }

    #[test]
    fn embedded_quotes_in_parts() {
        assert_eq!(quote_table_ref("my\"db.my\"table"), "\"my\"\"db\".\"my\"\"table\"");
    }
}
```

Also add an expand test for dot-qualified base tables:

```rust
#[test]
fn test_dot_qualified_base_table() {
    let def = SemanticViewDefinition {
        base_table: "jaffle.raw_orders".to_string(),
        dimensions: vec![Dimension {
            name: "status".to_string(),
            expr: "status".to_string(),
            source_table: None,
        }],
        metrics: vec![Metric {
            name: "order_count".to_string(),
            expr: "count(*)".to_string(),
            source_table: None,
        }],
        filters: vec![],
        joins: vec![],
    };
    let req = QueryRequest {
        dimensions: vec!["status".to_string()],
        metrics: vec!["order_count".to_string()],
    };
    let sql = expand("jaffle_orders", &def, &req).unwrap();
    // Must produce "jaffle"."raw_orders" not "jaffle.raw_orders"
    assert!(
        sql.contains("FROM \"jaffle\".\"raw_orders\""),
        "dot-qualified base_table must be split and quoted: {sql}"
    );
}
```

And a test for dot-qualified join tables:

```rust
#[test]
fn test_dot_qualified_join_table() {
    let def = SemanticViewDefinition {
        base_table: "jaffle.raw_orders".to_string(),
        dimensions: vec![Dimension {
            name: "customer_name".to_string(),
            expr: "customers.name".to_string(),
            source_table: Some("jaffle.raw_customers".to_string()),
        }],
        metrics: vec![Metric {
            name: "order_count".to_string(),
            expr: "count(*)".to_string(),
            source_table: None,
        }],
        filters: vec![],
        joins: vec![Join {
            table: "jaffle.raw_customers".to_string(),
            on: "raw_orders.customer_id = raw_customers.id".to_string(),
        }],
    };
    let req = QueryRequest {
        dimensions: vec!["customer_name".to_string()],
        metrics: vec!["order_count".to_string()],
    };
    let sql = expand("jaffle_orders", &def, &req).unwrap();
    assert!(
        sql.contains("JOIN \"jaffle\".\"raw_customers\""),
        "dot-qualified join table must be split and quoted: {sql}"
    );
}
```
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && cargo test --lib expand -- --nocapture 2>&1 | tail -20</automated>
  </verify>
  <done>
    - `quote_table_ref("jaffle.raw_orders")` returns `"jaffle"."raw_orders"` (each part quoted separately)
    - `quote_table_ref("orders")` returns `"orders"` (single-part unchanged, no regression)
    - `expand()` uses `quote_table_ref` for `base_table` and `join.table` references
    - `expand()` still uses `quote_ident` for column aliases (dim/metric names)
    - All existing expand tests pass (no regression)
    - New tests for dot-qualified base_table and join table pass
  </done>
</task>

</tasks>

<verification>
1. `cargo test --lib expand` -- all expand module tests pass (existing + new)
2. `cargo test --lib` -- full lib test suite passes (no regression in model, catalog, etc.)
3. `cargo clippy --all-targets -- -D warnings` -- no new warnings
</verification>

<success_criteria>
- Dot-qualified table names are properly split and individually quoted in generated SQL
- All 20+ existing expand tests continue to pass
- New unit tests cover: simple names, catalog.table, catalog.schema.table, reserved words, embedded quotes
- DuckLake integration test (run separately via `just test-ducklake`) should now pass with the fix
</success_criteria>

<output>
After completion, create `.planning/quick/1-fix-dot-qualified-table-name-issue/1-SUMMARY.md`
</output>
