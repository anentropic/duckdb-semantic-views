**Source:** https://github.com/anentropic/dbt-duckdb/blob/claude/fix-duckdb-extension-loading-hhWnp/notes/semantic_views_parser_comment_bug.md

**Title:** Parser hook fails to recognise DDL when query is preceded by a SQL comment (breaks dbt-duckdb / any caller that annotates queries)

## Summary

`sv_validate_ddl_rust` (and therefore `sv_parse_stub`) does case-insensitive **prefix** matching on the raw query string but does not strip leading SQL comments (`/* … */`, `-- …`). Any statement of the form

```sql
/* anything */ CREATE OR REPLACE SEMANTIC VIEW …
```

is classified as "not our statement" (rc=2 → `DISPLAY_ORIGINAL_ERROR`), so DuckDB falls back to the built-in PostgresParser error:

```
Parser Error: syntax error at or near "SEMANTIC"
```

Plain leading whitespace works fine — only comments trigger this.

This makes `semantic_views` effectively unusable through **dbt-duckdb**, because dbt-core unconditionally prepends a query annotation comment (`/* {"app": "dbt", …, "node_id": "model.X"} */`) to every statement it executes. It will also affect anything else that annotates queries (sqlfluff, BI tools that prepend session/user metadata, etc.).

## Reproducer (bare duckdb-python, no dbt)

DuckDB 1.5.2, semantic_views v0.7.1:

```python
import duckdb

p = duckdb.connect(":memory:", config={"allow_unsigned_extensions": "true"})
p.execute("INSTALL semantic_views FROM community")
p.load_extension("semantic_views")
p.execute("CREATE TABLE t(x INT, y INT)")

DDL = "CREATE OR REPLACE SEMANTIC VIEW sv AS TABLES (t AS t PRIMARY KEY (x)) DIMENSIONS (t.x AS xx) METRICS (t.y AS sum(y))"

p.execute(DDL)                       # OK
p.execute("\n\n   " + DDL)           # OK (whitespace only)
p.execute("/* hi */ " + DDL)         # FAIL: Parser Error: syntax error at or near "SEMANTIC"
p.execute("-- hi\n" + DDL)           # FAIL: same
```

## How it manifests in dbt-duckdb

`profiles.yml`:
```yaml
extensions:
  - { name: semantic_views, repo: community }
```

A `semantic_view` materialization that emits `CREATE OR REPLACE SEMANTIC VIEW …` directly fails with:
```
Parser Error: syntax error at or near "semantic"
LINE 2: create or replace semantic view "memory"."main"."sem_simple" as
                          ^
```

even though `INSTALL` and `LOAD` ran successfully on the database. The compiled SQL dbt sends includes its standard query annotation comment in front of the DDL, which is what trips the validator.

## Confirmed dbt workaround

```yaml
# dbt_project.yml
query-comment:
  comment: ''
  append: true
```

`append: true` moves the (now-empty) annotation to the trailing position instead of the leading position, which keeps the prefix match happy. Verified end-to-end: `OK created sql semantic_view model main.sem_simple`.

## Suggested fix

In `sv_validate_ddl_rust` (and `sv_parse_stub` if it does any pre-checking), before the keyword match, skip a leading run of:

- whitespace
- `-- … \n` line comments
- `/* … */` block comments (handle nesting if you want to be conservative; PostgresParser doesn't, so matching that is fine)

This is what the built-in DuckDB parser already does and what every other parser-extension extension I've seen does. The same change should apply at any other prefix-matching site (e.g. ALTER / DROP / SHOW SEMANTIC VIEW).

## Diagnosis context

I spent a while chasing a dbt-duckdb plumbing hypothesis (per-cursor `LOAD`, pool rebinding) before isolating this. For the record:

- DuckDB 1.5.2 stores parser extensions on `DBConfig::GetCallbackManager()` (database-level, one per `DatabaseInstance`); every `ClientContext` reads from the same registry. Sibling cursors (including ones created before `LOAD`) **do** see registered parser hooks.
- `sv_register_parser_hooks` correctly registers via `ParserExtension::Register(DBConfig::GetConfig(db), ext)`, so the hooks are visible everywhere on that database.
- The disconnect is purely at the prefix-match step inside the extension.

Bare-duckdb probes (parent / sibling / before-load cursor / multiple sibling cursors / replay of dbt-duckdb's `LocalEnvironment.handle()` flow) all pass. Add a single `/* … */` to the front of the DDL and they all fail identically.

Happy to PR the fix if useful.
