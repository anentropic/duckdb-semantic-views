# Phase 12: EXPLAIN + Typed Output - Context

**Gathered:** 2026-03-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Wire `EXPLAIN FROM semantic_view(...)` to surface the expanded SQL via a native DuckDB explain callback, and replace all-VARCHAR column output with properly typed columns (BIGINT, DATE, DOUBLE, etc.).

Also included in this phase (explicit user decision):
- Rename `define_semantic_view*` → `create_semantic_view*` to mirror standard SQL DDL naming
- Add the missing `create_semantic_view_if_not_exists` variant

</domain>

<decisions>
## Implementation Decisions

### EXPLAIN — surface

- Wire native `EXPLAIN FROM semantic_view(...)` using the `explain_extra_info` C API callback on the `semantic_view` table function
- Retire `explain_semantic_view` as a separate function — native EXPLAIN replaces it
- The three-part format (metadata header + expanded SQL + DuckDB physical plan) stays; it is injected as Extra Info in the TABLE_FUNCTION node of DuckDB's EXPLAIN output

### EXPLAIN — output content

- Keep the current three-part format from `explain_semantic_view`:
  1. Metadata header (view name, dimensions, metrics)
  2. Expanded SQL
  3. DuckDB physical plan for the expanded SQL (via `EXPLAIN {expanded_sql}`)
- This appears inline as `Extra Info` in DuckDB's `EXPLAIN FROM semantic_view(...)` output

### Typed output — type resolution hierarchy

Types are resolved in this priority order:

1. **Explicit `output_type` in DDL** — user-declared type per column in `create_semantic_view`. Enforced via SQL `CAST(expr AS <type>)` in the generated query AND declared as that type in `bind()`. Takes precedence over everything.
2. **DDL-time inference** — at `create_semantic_view` call time, run `LIMIT 0` on the expanded SQL and store inferred types in the catalog JSON alongside the definition. At query bind time, read stored types directly (no inference overhead).
3. **Fallback: VARCHAR** — if neither explicit type nor successful DDL-time inference, column type is VARCHAR (current behaviour preserved).

### Typed output — staleness

- When inference is stored at DDL time (tier 2), types can go stale if upstream column types change.
- This is documented: users must re-run `create_semantic_view` (or `create_or_replace_semantic_view`) to refresh stored types if the upstream schema changes and no `output_type` was declared.

### DDL function rename + new variant

All `define_semantic_view*` functions renamed to `create_semantic_view*` to mirror standard SQL DDL:

| Old name | New name |
|---|---|
| `define_semantic_view` | `create_semantic_view` |
| `define_or_replace_semantic_view` | `create_or_replace_semantic_view` |
| *(did not exist)* | `create_semantic_view_if_not_exists` |
| `drop_semantic_view` | unchanged |
| `drop_semantic_view_if_exists` | unchanged |

- `create_semantic_view_if_not_exists`: succeeds silently (no-op) if the view already exists; errors only on other failures.
- This is a **breaking change** — intentional for v0.2.0 (pre-1.0). Test files and any REQUIREMENTS.md/ROADMAP.md references to `define_semantic_view` must be updated.

### Claude's Discretion

- Whether DDL-time LIMIT 0 inference can safely run from scalar `invoke()` context, or requires a different mechanism (the "no SQL from scalar invoke" deadlock risk is an open question — Claude should verify and find the right path)
- Exact `output_type` field name and storage format in catalog JSON (on each Metric/Dimension struct vs. a top-level `column_types` map)
- How to map DuckDB C API type enums (`duckdb_type`) to `LogicalTypeHandle` for declaring output columns
- Implementation approach for writing typed vectors in `func()` — whether to drop the VARCHAR-cast wrapper or use string-to-type coercion

</decisions>

<specifics>
## Specific Ideas

- `output_type` should act as both declaration (what DuckDB sees as the column type) AND enforcement (generates `CAST(expr AS <type>)` in the SQL). Not just a hint.
- DDL-time inference runs at the moment when the user has set up their environment — tables are attached, schema is known. This is the natural point to capture types.

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets

- `try_infer_schema` (`src/query/table_function.rs`): already runs `LIMIT 0` and captures both column names AND `Vec<ffi::duckdb_type>` — types are captured but currently discarded (`_types`). Phase 12 wires types through.
- `execute_sql_raw` (`src/query/table_function.rs`): used by both `semantic_view` and `explain_semantic_view` for FFI SQL execution — reusable for DDL-time inference.
- `collect_explain_lines` (`src/query/explain.rs`): already collects `EXPLAIN {sql}` output as lines — reusable for the EXPLAIN callback.
- `ExplainSemanticViewVTab` (`src/query/explain.rs`): existing three-part format logic can be adapted for the `explain_extra_info` callback.

### Established Patterns

- Separate `state.conn` connection: created at extension load time specifically to avoid re-entrancy issues; used for all SQL execution from bind/func contexts. DDL-time inference would need the same connection (or an equivalent one accessible from scalar invoke).
- `SemanticViewDefinition` in `model.rs`: already has `dim_type: Option<String>` on `Dimension` for time semantics — the new `output_type: Option<String>` field follows the same pattern.
- Scalar function invoke → catalog persistence uses sidecar file to avoid SQL deadlocks. DDL-time inference (read-only LIMIT 0) may be safe on `state.conn`, but this needs to be verified.

### Integration Points

- `src/ddl/define.rs` (`DefineSemanticView::invoke`): DDL-time inference runs here after `parse_define_args` and before catalog persistence. Store inferred types back into the definition structs before serializing to JSON.
- `src/query/table_function.rs` (`SemanticViewVTab::bind`): reads stored types from parsed `SemanticViewDefinition` instead of calling `try_infer_schema`.
- `src/lib.rs`: registration names change from `define_*` to `create_*`; `explain_semantic_view` registration removed; `explain_extra_info` callback wired onto `semantic_view`.
- `src/model.rs`: add `output_type: Option<String>` to `Metric` and `Dimension`.
- Test `.test` files: all `define_semantic_view` / `semantic_query` references updated to `create_semantic_view` / `semantic_view`.

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope (all items added were explicit user decisions, not out-of-scope capabilities).

</deferred>

---

*Phase: 12-explain-typed-output*
*Context gathered: 2026-03-02*
