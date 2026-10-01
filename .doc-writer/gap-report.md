# Gap Detection Report

**Source root:** src/
**Language:** rust
**Generated:** 2026-09-30, against `main` @ `67af10f` (v0.13.0)
**Total exported symbols:** 165 Rust `pub` items (scan-exports.sh) + 19 registered SQL functions
**Documented symbols:** 18 of 19 SQL functions (all user-facing ones); 0 of 165 Rust items
**Undocumented symbols:** 1 SQL function (internal); 165 Rust items (internal: see Notes)

## Why the audit counts two surfaces

`scan-exports.sh` finds Rust `pub` items (`KeywordBody`, `CiName`, `QueryRequest`,
`FanTrapError`, `expand()` and so on). This project ships a DuckDB **extension**, not a Rust
crate. Users never link against these items; they are `pub` only for crate-internal module
boundaries, tests and fuzz targets. Leaving all 165 out of user docs is correct and none is
listed as a gap.

What users *do* use is the SQL surface. The registered functions were taken from
`duckdb_functions()` by diffing it before and after `LOAD`ing `build/debug/semantic_views.duckdb_extension`
on DuckDB 1.5.6. The DDL statements were checked against the `reference/` pages.

## Undocumented Exports

| Symbol | File | Type |
|--------|------|------|
| `__sv_compute_create_from_yaml(file_path, view_name, comment, search_path)` | src/ (DDL rewrite target) | table function — **internal**, `__sv_` prefix; the rewrite target for `CREATE SEMANTIC VIEW … FROM YAML FILE`. Correctly undocumented. |

## SQL surface coverage

| Function | Documented as | Pages | Notes |
|----------|---------------|------:|-------|
| `semantic_view(view_name, dimensions, metrics, facts, where_clause)` | itself | 30 | All named parameters documented |
| `explain_semantic_view(…)` | itself | 15 | All named parameters documented |
| `get_ddl(object_type, object_name[, use_fully_qualified_names])` | `GET_DDL(...)` | 14 | **Name mismatch:** `reference/get-ddl.rst` calls the 2nd parameter `<name>`; `duckdb_functions()` reports `object_name` (as of #236) |
| `read_yaml_from_semantic_view(view_name)` | itself | 9 | ok |
| `list_semantic_views` / `list_terse_semantic_views` | `SHOW [TERSE] SEMANTIC VIEWS` | 9 / 2 | `list_semantic_views()` still named as a `FROM` source (intended) |
| `describe_semantic_view` | `DESCRIBE SEMANTIC VIEW` | 14 | Statement-first by design (#235) |
| `show_columns_in_semantic_view` | `SHOW COLUMNS IN SEMANTIC VIEW` | 4 | ok |
| `show_semantic_{dimensions,metrics,facts,materializations}[_all]` | `SHOW SEMANTIC …` | 2–9 | ok |
| `show_semantic_dimensions_for_metric` | `SHOW SEMANTIC DIMENSIONS … FOR METRIC` | 6 | ok |

The `search_path` parameter on every table function is reserved for the extension (the
parser rewrite supplies it), and the function descriptions say so. It is correctly left out of
user docs; `search_path` appears only as the DuckDB session setting.

## Notes

- **v0.13.0's headline feature is barely documented.** "Every function the extension
  registers is documented in `duckdb_functions()`" is in the CHANGELOG, and
  `examples/function_discovery.py` demos it. But the only doc page that mentions
  `duckdb_functions()` is `changelog.md`. No how-to or reference page shows how a tool or agent
  discovers the functions, their parameter names and their examples over a SQL connection.
- **The `get_ddl` parameter-name drift** is minor, but it's the kind #236 set out to fix:
  someone reading `duckdb_functions()` sees `object_name`, while the docs say `<name>`.
- The DDL reference covers all verbs: `CREATE` (incl. `FROM YAML`), `ALTER`, `DROP`, `DESCRIBE`,
  `SHOW` × 7 variants, `GET_DDL`, and `READ_YAML_FROM_SEMANTIC_VIEW`.
