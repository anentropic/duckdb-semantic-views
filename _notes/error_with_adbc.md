## Resolution (v0.10.0)

Fixed by Phase 66 (EXPAND-CTX-01..03). Semantic view expansion now calls
`qualify_and_quote_table_ref` (see `src/expand/resolution.rs`) at every emission site —
the main expand path, FACTS, semi-additive metrics, window metrics, and materialization
routing — emitting fully-qualified `"database"."schema"."table"` references that resolve
regardless of the per-call Connection's catalog/schema defaults. The regression is
guarded by `test/integration/test_adbc_queries.py`, runnable via `just test-adbc-queries`.

See commits: b55936f, b116553, 9fe1ae5

---

[original content below]

The actual error when the xfail mark is removed:

> adbc_driver_manager.ProgrammingError: Catalog Error: Table with name sales_data does not exist!
> Did you mean "memory.sales_data"?
>
> FROM "sales_data" AS "s"

Here's what's happening:

When you call `Sales.query().metrics(...).execute()` via a DuckDB pool, Semolina generates
```sql
SELECT ... FROM semantic_view('sales_view', dimensions=[...], metrics=[...])
```

DuckDB's `semantic_view()` table function then **internally expands** that to plain SQL:
```sql
SELECT s.country, SUM(s.revenue) FROM "sales_data" AS "s" GROUP BY 1
```

That internal expansion fails — DuckDB knows the table exists as `memory.main.sales_data` but when `semantic_view()` generates and re-runs the inner SQL, it can't resolve the unqualified `sales_data` name.

**The core issue:** DuckDB's `semantic_view()` table function doesn't maintain the right catalog/schema context when executed through the ADBC driver. The table was created as memory.main.sales_data, but the inner SQL expansion references it as just `sales_data`, which fails in the ADBC execution context.

The **passing** tests (raw SQL like `SELECT country FROM sales_data`) work fine because that SQL runs directly on the connection without going through semantic_view()'s internal expansion step.

So the 20 xfails mark the exact boundary: **pool lifecycle, extension loading, raw SQL, and cursor operations** all work correctly through ADBC. Only `semantic_view()` **queries** via the ADBC pool are broken, which is the core thing the whole milestone is building toward.

This is upstream — either DuckDB's `semantic_view()` extension or the ADBC driver needs to fix the catalog resolution inside the function's expansion step.
