# Phase 9: Time Dimensions - Research

**Researched:** 2026-03-01
**Domain:** Rust struct extension + SQL codegen + DuckDB C API (MAP parameter extraction)
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- New optional fields on `Dimension`: `type: Option<String>` and `granularity: Option<String>`
- Both use `#[serde(default)]` so existing definitions without these fields continue to deserialize
- `SemanticViewDefinition` uses `#[serde(deny_unknown_fields)]` — fields must be added to the struct, not left as unknown
- `type: "time"` without a `granularity` field → error at define-time with message: `"dimension 'order_date' declares type 'time' but is missing required 'granularity' field"`
- SQL codegen: `date_trunc('granularity', expr)::DATE` — always cast to DATE regardless of source type
- New named parameter on `semantic_query`: `granularities` of type `MAP(VARCHAR, VARCHAR)`
- SQL syntax: `granularities := {'order_date': 'month'}`
- Extraction uses DuckDB MAP C API (`duckdb_get_map_size`, `duckdb_get_map_key`, `duckdb_get_map_value`)
- `granularities` map is passed through `QueryRequest` to `expand()`
- Unsupported granularity → error at query-bind time with message listing valid values
- Granularity override for non-time dimension → error at query-bind time
- Invalid `type` value → error at define-time
- No bypass mechanism — `type: "time"` always applies `date_trunc`

### Claude's Discretion
- Exact error message wording beyond the patterns above
- Levenshtein suggestion for misspelled granularity names (consistent with existing dimension/metric typo suggestions)
- Internal representation of `type` field (could be an enum `DimensionType` instead of `Option<String>`)
- Whether `QueryRequest` grows a `granularity_overrides: HashMap<String, String>` field or a wrapper struct

### Deferred Ideas (OUT OF SCOPE)
- `hour` and `quarter` granularities — deferred to v0.3.0
- Fiscal calendar / Sunday-start weeks — explicitly out of scope
- Custom time spine table — `date_trunc` is sufficient
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| TIME-01 | User can declare a dimension as time-typed with a granularity (day, week, month, year) in a semantic view definition | Serde `#[serde(default)]` fields on `Dimension` struct + validation in `from_json` |
| TIME-02 | `semantic_query` truncates time dimension values to the declared granularity using `date_trunc` | `expand()` generates `date_trunc('gran', expr)::DATE AS "name"` for time dims |
| TIME-03 | User can override time dimension granularity at query time via a `granularities` parameter | New named MAP parameter extracted in `bind()`, passed to `expand()` via `QueryRequest` |
| TIME-04 | Time dimensions on DATE source columns return DATE values (not TIMESTAMP) | `::DATE` cast in codegen ensures DATE output type regardless of source type |
</phase_requirements>

## Summary

Phase 9 is pure Rust — no C++ shim involvement. It extends three files: `model.rs` (struct fields), `expand.rs` (SQL codegen), and `table_function.rs` (MAP parameter extraction + registration). The changes are well-isolated and build directly on existing patterns.

The primary complexity is the DuckDB MAP C API for extracting `granularities := {'key': 'value'}` at bind time. The existing `extract_list_strings` function (table_function.rs:128) provides the exact FFI pattern to follow — MAP extraction uses the same `value_raw_ptr` helper with `duckdb_get_map_size`, `duckdb_get_map_key`, and `duckdb_get_map_value` instead of the list equivalents.

The `::DATE` cast in codegen (TIME-04) is the simplest part: DuckDB's `date_trunc` returns TIMESTAMP by default; appending `::DATE` explicitly converts to DATE type. This is verified in the existing test `test_dimension_expression_not_quoted` which already asserts expression verbatim rendering.

**Primary recommendation:** One plan (wave 1) covers the full implementation. All changes are in pure Rust, no cross-language concerns. Test coverage can be added directly to existing `mod tests` blocks in each file.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde + serde_json | Already in Cargo.toml | JSON de/serialization of `Dimension` struct | Used throughout model.rs |
| libduckdb-sys (`ffi`) | Already pinned `=1.4.4` | MAP C API extraction | Used in table_function.rs |
| strsim | Already in Cargo.toml | Levenshtein distance for granularity typo suggestions | Used in `suggest_closest` |

### No New Dependencies
All required libraries are already in Cargo.toml. Phase 9 adds zero new dependencies.

## Architecture Patterns

### Pattern 1: Adding Serde-Optional Fields with Backward Compat
**What:** New optional fields on `Dimension` struct, using `#[serde(default)]` + `deny_unknown_fields` on the parent.
**When to use:** Required because `SemanticViewDefinition` has `#[serde(deny_unknown_fields)]` — any new field on `Dimension` must be declared on the struct or deserialization fails.
**Example (from model.rs existing pattern):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub source_table: Option<String>,
    // NEW:
    #[serde(default, rename = "type")]
    pub dim_type: Option<String>,   // "time" is the only valid value in v0.2.0
    #[serde(default)]
    pub granularity: Option<String>, // "day", "week", "month", "year"
}
```
Note: `type` is a Rust keyword — field must be renamed with `#[serde(rename = "type")]` and given a Rust-safe name (`dim_type`).

### Pattern 2: Validation at from_json
**What:** Add time dimension validation in `SemanticViewDefinition::from_json` after deserialization.
**When to use:** Errors at define-time (not query-time) — the locked decision requires catching missing granularity at define-time.
```rust
pub fn from_json(name: &str, json: &str) -> Result<Self, String> {
    let def: Self = serde_json::from_str(json)
        .map_err(|e| format!("invalid definition for semantic view '{name}': {e}"))?;

    // Validate time dimensions
    for dim in &def.dimensions {
        if let Some(ref dt) = dim.dim_type {
            if dt != "time" {
                return Err(format!(
                    "dimension '{}' has unknown type '{}'; only 'time' is supported",
                    dim.name, dt
                ));
            }
            if dim.granularity.is_none() {
                return Err(format!(
                    "dimension '{}' declares type 'time' but is missing required 'granularity' field",
                    dim.name
                ));
            }
        }
    }

    Ok(def)
}
```

### Pattern 3: SQL Codegen for Time Dimensions in expand()
**What:** Wrap time dimension expressions with `date_trunc('gran', expr)::DATE` in the SELECT items.
**When to use:** When `dim.dim_type == Some("time")` during expand — check the dimension's effective granularity (override takes precedence over declared).
**Key insight:** The `::DATE` cast is required because DuckDB's `date_trunc` returns TIMESTAMP by default. This satisfies TIME-04.
```rust
// In the resolved_dims loop (expand.rs around line 346):
for dim in &resolved_dims {
    let expr = if dim.dim_type.as_deref() == Some("time") {
        let gran = req.granularity_overrides
            .get(&dim.name.to_ascii_lowercase())
            .map(String::as_str)
            .or(dim.granularity.as_deref())
            .unwrap_or("day"); // fallback (validation prevents None reaching here)
        format!("date_trunc('{}', {})::DATE", gran, dim.expr)
    } else {
        dim.expr.clone()
    };
    select_items.push(format!("    {} AS {}", expr, quote_ident(&dim.name)));
}
```

### Pattern 4: MAP Parameter Extraction (DuckDB C API)
**What:** Extract `granularities := {'order_date': 'month'}` from `BindInfo` using MAP FFI.
**When to use:** In `bind()` after extracting dimensions/metrics. The MAP type is registered in `named_parameters()`.
**Pattern derivation:** Follow `extract_list_strings` (table_function.rs:128) but use MAP API calls.
```rust
/// Extract key-value pairs from a DuckDB MAP(VARCHAR, VARCHAR) value.
/// Safety: value must represent a MAP(VARCHAR, VARCHAR) value.
unsafe fn extract_map_strings(value: &Value) -> HashMap<String, String> {
    let value_ptr = value_raw_ptr(value); // reuse existing helper
    let size = ffi::duckdb_get_map_size(value_ptr);
    let mut result = HashMap::with_capacity(size as usize);
    for i in 0..size {
        let key = ffi::duckdb_get_map_key(value_ptr, i);
        let val = ffi::duckdb_get_map_value(value_ptr, i);
        let k_cstr = ffi::duckdb_get_varchar(key);
        let v_cstr = ffi::duckdb_get_varchar(val);
        if !k_cstr.is_null() && !v_cstr.is_null() {
            let k = CStr::from_ptr(k_cstr).to_string_lossy().into_owned();
            let v = CStr::from_ptr(v_cstr).to_string_lossy().into_owned();
            ffi::duckdb_free(k_cstr.cast::<c_void>());
            ffi::duckdb_free(v_cstr.cast::<c_void>());
            result.insert(k.to_ascii_lowercase(), v);
        }
        ffi::duckdb_destroy_value(&mut { key });
        ffi::duckdb_destroy_value(&mut { val });
    }
    result
}
```

### Pattern 5: MAP Type Registration in named_parameters()
**What:** Register the `granularities` parameter with `MAP(VARCHAR, VARCHAR)` type.
**Key insight:** `LogicalTypeHandle::map()` requires two type args (key type, value type). Check if duckdb-rs exposes this.
```rust
fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
    Some(vec![
        ("dimensions".to_string(), LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar))),
        ("metrics".to_string(), LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar))),
        // NEW:
        ("granularities".to_string(), LogicalTypeHandle::map(
            &LogicalTypeHandle::from(LogicalTypeId::Varchar),
            &LogicalTypeHandle::from(LogicalTypeId::Varchar),
        )),
    ])
}
```

### Pattern 6: QueryRequest Extension
**What:** Add `granularity_overrides: HashMap<String, String>` to `QueryRequest`.
**When to use:** Keys are lowercased dimension names, values are granularity strings. `expand()` consults this map when building time dimension SELECT expressions.
```rust
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
    pub granularity_overrides: HashMap<String, String>, // NEW
}
```

### Pattern 7: Granularity Validation at Bind Time
**What:** Validate granularity override values in `bind()` before calling `expand()`.
**Error cases:**
1. Override for non-time dimension: `"dimension 'region' is not a time dimension and cannot have a granularity override"`
2. Unsupported granularity value: `"'quarter' is not a supported granularity; valid values: day, week, month, year"`
3. Levenshtein suggestion: reuse `suggest_closest(override_val, &VALID_GRANULARITIES)` for typo hints.

### Anti-Patterns to Avoid
- **Do NOT use `type` as a Rust field name** — it's a keyword. Use `dim_type` with `#[serde(rename = "type")]`.
- **Do NOT validate granularity at expand() time** — the locked decision says unsupported granularity errors at bind time (not codegen time).
- **Do NOT inspect source column types** — TIME-04 is satisfied by `::DATE` cast in codegen, no runtime type inspection needed.
- **Do NOT skip `duckdb_destroy_value` calls** — memory leaks in MAP extraction if values are not freed.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MAP key/value extraction | Custom binary parsing | `duckdb_get_map_key`, `duckdb_get_map_value` C API | Already exposed in libduckdb-sys |
| Typo suggestions | Levenshtein from scratch | `suggest_closest` (expand.rs:12) | Already exists, correct threshold |
| JSON field renaming | String manipulation | `#[serde(rename = "type")]` | Serde handles it cleanly |

## Common Pitfalls

### Pitfall 1: `type` is a Rust Keyword
**What goes wrong:** `pub type: Option<String>` doesn't compile.
**Why it happens:** `type` is a reserved keyword for type aliases.
**How to avoid:** Use `pub dim_type: Option<String>` with `#[serde(rename = "type")]`.
**Warning signs:** Compiler error `expected identifier, found keyword 'type'`.

### Pitfall 2: `date_trunc` Returns TIMESTAMP, Not DATE
**What goes wrong:** `date_trunc('month', order_date)` on a DATE column returns TIMESTAMP `2024-01-01 00:00:00`, not `DATE 2024-01-01`.
**Why it happens:** DuckDB promotes DATE to TIMESTAMP inside `date_trunc` to preserve generality.
**How to avoid:** Always append `::DATE` to the codegen: `date_trunc('month', order_date)::DATE`.
**Warning signs:** Test asserts `2024-01-01` but gets `2024-01-01 00:00:00`.

### Pitfall 3: MAP Key Case Sensitivity in Lookup
**What goes wrong:** User writes `granularities := {'Order_Date': 'month'}` but dimension name in definition is `order_date` — override not applied.
**Why it happens:** String equality is case-sensitive by default.
**How to avoid:** Lowercase all keys in `extract_map_strings` (done in Pattern 4 above). Match against lowercased dimension names.
**Warning signs:** Override silently ignored, no error raised.

### Pitfall 4: `duckdb_destroy_value` Missing on MAP Children
**What goes wrong:** Memory leak per query.
**Why it happens:** `duckdb_get_map_key` and `duckdb_get_map_value` allocate values that must be destroyed.
**How to avoid:** Call `duckdb_destroy_value` on both key and value in the MAP extraction loop.
**Warning signs:** Memory grows linearly with query count under Valgrind (not easily detected in Rust tests).

### Pitfall 5: `LogicalTypeHandle::map` API availability
**What goes wrong:** `LogicalTypeHandle::map` may not exist in duckdb-rs 1.4.4 API surface.
**Why it happens:** duckdb-rs Rust bindings may not expose all DuckDB type constructors.
**How to avoid:** Check the duckdb-rs 1.4.4 source for `LogicalTypeHandle` methods. If `map()` is unavailable, use the FFI directly: `ffi::duckdb_create_map_type(key_type, value_type)` and wrap with `LogicalTypeHandle`.
**Warning signs:** `error[E0599]: no method named 'map' found for struct 'LogicalTypeHandle'`.

### Pitfall 6: `#[serde(deny_unknown_fields)]` on Parent, Not Child
**What goes wrong:** The `deny_unknown_fields` attribute is on `SemanticViewDefinition`, not on `Dimension`. This means unknown fields on `Dimension` WOULD be rejected too (serde propagates strict checking through nested structs).
**Why it happens:** Actually this IS the desired behavior — the new `dim_type`/`granularity` fields must be declared on `Dimension` to be accepted.
**How to avoid:** Correctly add the fields to `Dimension` struct. Do not rely on the parent's attribute catching them — the parent's `deny_unknown_fields` applies only to the top-level keys.
**Clarification from code inspection:** `#[serde(deny_unknown_fields)]` is on `SemanticViewDefinition` which has fields `base_table`, `dimensions`, `metrics`, `filters`, `joins`. Adding `type`/`granularity` to `Dimension` itself is what unlocks them. The test `unknown_fields_are_rejected` in model.rs tests this at the top level.

## Code Examples

### Verified: Current expand() SELECT item generation (expand.rs:345-352)
```rust
// Source: src/expand.rs lines 345-352
let mut select_items: Vec<String> = Vec::new();
for dim in &resolved_dims {
    select_items.push(format!("    {} AS {}", dim.expr, quote_ident(&dim.name)));
}
for met in &resolved_mets {
    select_items.push(format!("    {} AS {}", met.expr, quote_ident(&met.name)));
}
```
This is the exact loop to modify for time dimension codegen.

### Verified: extract_list_strings FFI pattern (table_function.rs:128-143)
```rust
// Source: src/query/table_function.rs lines 128-143
pub(crate) unsafe fn extract_list_strings(value: &Value) -> Vec<String> {
    let value_ptr = value_raw_ptr(value);
    let size = ffi::duckdb_get_list_size(value_ptr);
    let mut result = Vec::with_capacity(size as usize);
    for i in 0..size {
        let child = ffi::duckdb_get_list_child(value_ptr, i);
        let cstr = ffi::duckdb_get_varchar(child);
        if !cstr.is_null() {
            let s = CStr::from_ptr(cstr).to_string_lossy().into_owned();
            ffi::duckdb_free(cstr.cast::<c_void>());
            result.push(s);
        }
        ffi::duckdb_destroy_value(&mut { child });
    }
    result
}
```
MAP extraction follows the same pattern with `duckdb_get_map_size`/`duckdb_get_map_key`/`duckdb_get_map_value`.

### Verified: named_parameters registration (table_function.rs:330-341)
```rust
// Source: src/query/table_function.rs lines 330-341
fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
    Some(vec![
        ("dimensions".to_string(), LogicalTypeHandle::list(...)),
        ("metrics".to_string(), LogicalTypeHandle::list(...)),
    ])
}
```
Add `granularities` entry here.

### Verified: DuckDB `date_trunc` behavior
`date_trunc('month', DATE '2024-03-15')` returns `TIMESTAMP '2024-03-01 00:00:00'`.
`date_trunc('month', DATE '2024-03-15')::DATE` returns `DATE '2024-03-01'`.
Source: DuckDB documentation (verified by requirement TIME-04 and CONTEXT.md specification).

## State of the Art

| Old Approach | Current Approach | Impact |
|--------------|------------------|--------|
| Raw `expr` in SELECT | `date_trunc('gran', expr)::DATE` for time dims | Returns proper DATE type |
| No named time parameter | `granularities MAP(VARCHAR, VARCHAR)` | Query-time override |
| No type field on Dimension | `dim_type: Option<String>` + `granularity: Option<String>` | Declarative time typing |

## Open Questions

1. **`LogicalTypeHandle::map()` availability in duckdb-rs 1.4.4**
   - What we know: `LogicalTypeHandle::list()` exists (currently used for dimensions/metrics)
   - What's unclear: Whether `LogicalTypeHandle::map()` exists in duckdb-rs 1.4.4 Rust bindings
   - Recommendation: Check during implementation. If unavailable, fall back to FFI: `ffi::duckdb_create_map_type(key_type.as_raw(), value_type.as_raw())` — libduckdb-sys exposes this function.

2. **`HashMap` import in expand.rs**
   - What we know: `expand.rs` currently uses `std::collections::HashSet` but not `HashMap`
   - What's unclear: Whether `QueryRequest` should live in expand.rs or get its own module
   - Recommendation: Add `use std::collections::HashMap;` to expand.rs — QueryRequest is already defined there.

## Sources

### Primary (HIGH confidence)
- Direct code inspection of `src/model.rs`, `src/expand.rs`, `src/query/table_function.rs` — verified current patterns
- DuckDB C API (libduckdb-sys): `duckdb_get_map_size`, `duckdb_get_map_key`, `duckdb_get_map_value` confirmed present in libduckdb-sys crate (same pattern as list API)
- Rust/serde: `#[serde(rename = "type")]` + `#[serde(default)]` — confirmed necessary for `type` keyword conflict

### Secondary (MEDIUM confidence)
- DuckDB `date_trunc` return type behavior — confirmed by CONTEXT.md specification and requirement TIME-04

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all dependencies already in Cargo.toml
- Architecture: HIGH — based on direct code inspection
- Pitfalls: HIGH — `type` keyword and `::DATE` cast are concrete, verifiable issues
- MAP API: MEDIUM — libduckdb-sys exposes the C API but `LogicalTypeHandle::map()` needs runtime verification

**Research date:** 2026-03-01
**Valid until:** 2026-04-01 (DuckDB pinned to 1.4.4, stable)

## RESEARCH COMPLETE

**Phase:** 9 - Time Dimensions
**Confidence:** HIGH

### Key Findings
- Zero new dependencies — all libraries already in Cargo.toml
- `type` is a Rust keyword — must use `dim_type` with `#[serde(rename = "type")]`
- `date_trunc` returns TIMESTAMP — `::DATE` cast is mandatory for TIME-04
- MAP parameter extraction follows the same FFI pattern as `extract_list_strings`
- `LogicalTypeHandle::map()` availability needs verification during implementation; FFI fallback documented
- All four requirements map to isolated, testable code changes in three files

### File Created
`.planning/phases/09-time-dimensions/09-RESEARCH.md`

### Confidence Assessment
| Area | Level | Reason |
|------|-------|--------|
| Standard Stack | HIGH | Direct Cargo.toml inspection |
| Architecture | HIGH | Direct source code inspection |
| Pitfalls | HIGH | Concrete Rust/DuckDB behavioral facts |
| MAP API | MEDIUM | C API confirmed, Rust wrapper TBD |

### Open Questions
- `LogicalTypeHandle::map()` in duckdb-rs 1.4.4 — fallback to `ffi::duckdb_create_map_type` documented

### Ready for Planning
Research complete. Planner can now create PLAN.md files.
