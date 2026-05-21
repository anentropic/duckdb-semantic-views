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
