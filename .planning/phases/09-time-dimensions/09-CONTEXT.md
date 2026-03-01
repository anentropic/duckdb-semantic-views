# Phase 9: Time Dimensions - Context

**Gathered:** 2026-03-01
**Status:** Ready for planning

<domain>
## Phase Boundary

Add time-typed dimensions to semantic view definitions. Users declare `type: "time"` + `granularity` on a dimension; `semantic_query` automatically wraps the expression in `date_trunc`. A `granularities` named parameter on `semantic_query` lets users override granularity at query time. Pure Rust — no C++ shim involvement.

Supported granularities in v0.2.0: `day`, `week`, `month`, `year` (ISO 8601). `quarter` and `hour` are explicitly deferred to v0.3.0.

</domain>

<decisions>
## Implementation Decisions

### Granularity declaration format
- New optional fields on `Dimension`: `type: Option<String>` and `granularity: Option<String>`
- Both use `#[serde(default)]` so existing definitions without these fields continue to deserialize
- The `SemanticViewDefinition` uses `#[serde(deny_unknown_fields)]` — the fields must be added to the struct, not left as unknown
- Example: `{"name": "order_date", "expr": "order_date", "type": "time", "granularity": "month"}`

### Missing-granularity fallback
- `type: "time"` without a `granularity` field → **error at define-time**
- Error message: `"dimension 'order_date' declares type 'time' but is missing required 'granularity' field"`
- Forces the user to be explicit; no silent defaulting to "day"

### SQL codegen for time dimensions
- `date_trunc('granularity', expr)::DATE` — always cast to DATE regardless of source type
- This satisfies TIME-04: DATE source columns return DATE, not TIMESTAMP strings like `2024-01-01 00:00:00`
- Applied in `expand()` in `expand.rs` when a dimension has `type == Some("time")`

### Query-time granularity override format
- New named parameter on `semantic_query`: `granularities` of type `MAP(VARCHAR, VARCHAR)`
- SQL syntax: `granularities := {'order_date': 'month'}`
- Extraction uses DuckDB MAP C API (`duckdb_get_map_size`, `duckdb_get_map_key`, `duckdb_get_map_value`)
- The `granularities` map is passed through `QueryRequest` to `expand()`

### Error behavior
- Unsupported granularity (e.g., `'quarter'`) → **error at query-bind time** with message listing valid values
- Granularity override for a non-time dimension → **error at query-bind time**: `"dimension 'region' is not a time dimension and cannot have a granularity override"`
- Invalid `type` value (not `"time"`) → **error at define-time**: `"dimension 'order_date' has unknown type 'date'; only 'time' is supported"`

### Raw access
- No bypass mechanism — `type: "time"` always applies `date_trunc`
- To get raw untruncated values, the user declares a regular dimension (no `type` field) alongside or instead of the time dimension
- This is the simplest design and avoids an `as_raw` escape hatch

### Claude's Discretion
- Exact error message wording beyond the patterns above
- Levenshtein suggestion for misspelled granularity names (consistent with existing dimension/metric typo suggestions)
- Internal representation of `type` field (could be an enum `DimensionType` instead of `Option<String>` — planner decides)
- Whether `QueryRequest` grows a `granularity_overrides: HashMap<String, String>` field or a wrapper struct

</decisions>

<specifics>
## Specific Ideas

- Requirements explicitly name `date_trunc` as the SQL function — use it directly, do not abstract
- TIME-04 is satisfied by `::DATE` cast in the codegen, not by inspecting source column types at runtime
- DuckDB's `date_trunc('week', d)` uses ISO 8601 (Monday start) — acceptable per requirements; no fiscal calendar needed

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `extract_list_strings` (table_function.rs:128) — pattern to follow for MAP extraction of `granularities`
- `suggest_closest` (expand.rs:12) — reuse for granularity typo suggestions
- `SemanticViewDefinition::from_json` (model.rs:58) — validation entry point; add time dimension validation here

### Established Patterns
- `#[serde(default)]` on new optional fields in `Dimension` — required for backward compat with existing definitions
- Error propagation via `Box<dyn std::error::Error>` in bind/invoke handlers
- Named parameters registered in `named_parameters()` (table_function.rs:330) — add `granularities` here with MAP type

### Integration Points
- `model.rs` — `Dimension` struct: add `dim_type: Option<String>` and `granularity: Option<String>`
- `expand.rs` — `expand()` function: wrap time dimension expressions with `date_trunc`
- `table_function.rs` — `bind()`: extract `granularities` MAP, pass to `QueryRequest`; `named_parameters()`: register MAP type
- `ddl/define.rs` — no changes needed; validate JSON → `SemanticViewDefinition::from_json` handles it

</code_context>

<deferred>
## Deferred Ideas

- `hour` and `quarter` granularities — deferred to v0.3.0 per REQUIREMENTS.md
- Fiscal calendar / Sunday-start weeks — explicitly out of scope
- Custom time spine table — `date_trunc` is sufficient per requirements

</deferred>

---

*Phase: 09-time-dimensions*
*Context gathered: 2026-03-01*
