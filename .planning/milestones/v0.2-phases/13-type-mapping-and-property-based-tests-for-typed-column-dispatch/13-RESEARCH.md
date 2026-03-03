# Phase 13: Type-mapping and property-based tests for typed column dispatch - Research

**Researched:** 2026-03-02
**Domain:** DuckDB C API binary chunk reading, typed vector output, proptest property-based testing in Rust
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Architecture: replace VARCHAR intermediary with binary reads**

Replace the current `build_varchar_cast_sql()` wrapper (which casts all columns to VARCHAR before reading) with direct typed binary reads from DuckDB result chunk vectors using `duckdb_vector_get_data()` + type-specific dispatch. This eliminates the string parse/reformat layer for all types that have flat binary representations.

Keep VARCHAR path only for types with no native flat representation (STRUCT, MAP, LIST of STRUCT, BLOB, BIT, VARINT).

**Bugs to fix (silent data corruption)**

- **TIMESTAMP**: currently declared as TIMESTAMP in schema but all values come back NULL (VARCHAR string "2024-01-15 10:30:00" fails `parse::<i64>()` → TypedValue::Null). Fix: read i64 microseconds directly from binary chunk.
- **BOOLEAN**: currently declared as BOOLEAN in schema but written via string path — type mismatch at FFI level, undefined behaviour. Fix: read u8 (0/1) directly, write to BOOLEAN slot.

**Type dispatch scope**

**Native binary dispatch (Phase 13):**
- All integer types: TINYINT, SMALLINT, INTEGER, BIGINT, UTINYINT, USMALLINT, UINTEGER, UBIGINT
- HUGEINT / UHUGEINT: read as 128-bit, output as BIGINT (truncated) — acceptable for real-world aggregate values
- Float types: FLOAT, DOUBLE
- BOOLEAN: u8 read, BOOLEAN output
- Temporal: DATE (i32 days), TIMESTAMP (i64 µs), TIMESTAMP_S/MS/NS, TIMESTAMP_TZ, TIME
- DECIMAL: read HUGEINT + scale from logical type metadata, declare DECIMAL(width, scale) output
- UUID: 16-byte copy, declare UUID output type
- ENUM: read ordinal, decode via `duckdb_enum_dictionary_value()`, output as VARCHAR (string values are correct semantic output)
- LIST with scalar element types (INTEGER, BIGINT, DOUBLE, BOOLEAN, DATE, TIMESTAMP, VARCHAR): read offset/length parent + child vector, declare LIST(element_type) output. Covers ARRAY_AGG use cases.

**VARCHAR fallback (intentional, deferred to future milestone):**
- STRUCT
- MAP
- LIST where element type is STRUCT or MAP
- INTERVAL (compound struct type)
- BLOB
- BIT, VARINT (exotic)

**Property-based test strategy**

**Two layers:**

1. **Unit PBTs** — test binary-read helper functions in isolation. Generate arbitrary in-bounds (chunk data, type_id) combinations. Property: never panics. Also test boundary values (i64::MAX, i32::MIN, -0.0, date epoch extremes, empty string → NULL).

2. **Integration PBTs** — full roundtrip via in-memory DuckDB. Strategy:
   - Proptest generates a Vec of typed values for each supported type
   - Create a DuckDB in-memory table with those values
   - Define a semantic_view over it with those columns as dimensions/metrics
   - Query via `semantic_view()`
   - Assert: output column type matches source type, output values match input values
   - NULL values: assert null rows in source → null rows in output

Existing `expand_proptest.rs` PBTs (SQL structure properties) are unchanged — they test the expansion engine, not the output layer.

### Claude's Discretion

- Exact Rust types used for DECIMAL intermediate (i128 vs two i64s for HUGEINT backing)
- Whether to use the `proptest` crate (already in use) or add `quickcheck`
- How to create mock DuckDB data chunks for unit PBTs (or whether to use DuckDB in-memory for both layers)
- Whether TIMESTAMP_TZ, TIMESTAMP_S/MS/NS get individual test cases or are grouped with TIMESTAMP
- Test file organisation (extend `expand_proptest.rs` or create new `output_proptest.rs`)

### Deferred Ideas (OUT OF SCOPE)

- STRUCT output — future milestone
- MAP output — future milestone
- LIST where element type is STRUCT or MAP — future milestone
- INTERVAL native output (compound struct type) — future milestone
- BLOB output — explicitly not needed
- HUGEINT output type (vs BIGINT truncation) — could revisit if users hit overflow issues
</user_constraints>

---

## Summary

Phase 13 is a correctness repair phase, not a feature phase. The current output pipeline in `func()` — `build_varchar_cast_sql()` → string chunk reads → `parse_typed_from_str()` → typed write — has three silent data-loss bugs: TIMESTAMP returns all-NULL, BOOLEAN produces undefined FFI behaviour, and DECIMAL/LIST columns come back as strings. The fix is to remove the VARCHAR cast wrapper and read binary values directly from `duckdb_result_get_chunk` vectors using `duckdb_vector_get_data()`.

All the required C API primitives are confirmed present in `libduckdb-sys = "=1.4.4"`: `duckdb_vector_get_data`, `duckdb_vector_get_validity`, `duckdb_list_vector_get_child`, `duckdb_list_vector_get_size`, `duckdb_column_logical_type`, `duckdb_decimal_width`, `duckdb_decimal_scale`, `duckdb_enum_dictionary_value`, and `duckdb_create_decimal_type`. The duckdb-rs `LogicalTypeHandle` already has `.decimal(width, scale)` and `.list(child)` constructors for declaring output column types in `bind()`.

The test strategy is to stay with `proptest` (already in `dev-dependencies = "1.9"`, lock shows `1.10.0`). Integration PBTs using `Connection::open_in_memory()` are well-established in the codebase (catalog.rs tests). The two-layer approach (unit PBTs for the binary-read helpers, integration PBTs for end-to-end roundtrip) matches the existing test architecture and avoids the complexity of creating mock `duckdb_data_chunk` handles directly.

**Primary recommendation:** Remove `build_varchar_cast_sql()` and `parse_typed_from_str()`, introduce a `read_typed_from_vector()` dispatch function that reads raw binary per type, extend `try_infer_schema()` to also return logical type handles for DECIMAL/LIST/ENUM metadata, extend `type_from_duckdb_type_u32()` to return native types, and validate with a new `tests/output_proptest.rs` file using both unit and integration PBT layers.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `libduckdb-sys` | `=1.4.4` | Raw C API FFI — `duckdb_vector_get_data`, validity masks, logical types, list/decimal/enum helpers | Already in use; all needed functions confirmed present in bindings |
| `duckdb` | `=1.4.4` | High-level `LogicalTypeHandle::decimal()`, `LogicalTypeHandle::list()`, `DataChunkHandle::list_vector()`, `FlatVector::as_mut_slice_with_len` | Already in use; `decimal()` and `list()` constructors confirmed in `logical_type.rs` |
| `proptest` | `1.10.0` | Property-based testing — `proptest!` macro, `prop_oneof!`, range strategies for i64/f64/bool | Already in `dev-dependencies`; existing `expand_proptest.rs` establishes usage patterns |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `arbitrary` (Cargo feature) | `1` (optional dep) | Derive-based arbitrary value generation for model types | Consider if proptest strategy composition for unit PBTs becomes unwieldy; already declared as optional feature in Cargo.toml |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `proptest` for integration PBTs | `quickcheck` | quickcheck has simpler API but no shrinking — proptest's shrinking gives much better failure messages; project already uses proptest |
| DuckDB in-memory for both test layers | Mock `duckdb_data_chunk` handles for unit PBTs | Creating real DuckDB chunks requires bundled feature already present; mocking would be more code with less test value |
| `i128` for DECIMAL intermediate | Two-`i64` struct (HUGEINT layout) | i128 is natively supported in Rust and maps directly to the 128-bit HUGEINT binary; avoids manual upper/lower bit manipulation |

---

## Architecture Patterns

### Recommended Project Structure

```
src/query/
├── table_function.rs     # Surgery site: replace build_varchar_cast_sql + parse_typed_from_str
│                         # New: read_typed_from_vector(), try_infer_logical_types()
│                         # Extend: type_from_duckdb_type_u32() → type_from_logical_type()
├── error.rs              # Unchanged
├── explain.rs            # Unchanged
└── mod.rs                # Unchanged

tests/
├── expand_proptest.rs    # UNCHANGED — tests expansion engine SQL structure
└── output_proptest.rs    # NEW — unit + integration PBTs for typed output pipeline
```

### Pattern 1: Direct Binary Read from DuckDB Result Vector

**What:** Read raw typed values from `duckdb_vector_get_data()` pointer, casting to the correct Rust primitive matching DuckDB's in-memory layout.

**When to use:** For all flat scalar types (int, float, bool, date, timestamp, uuid).

**DuckDB binary layouts (confirmed from C API docs and libduckdb-sys structs):**

| DuckDB Type | Binary Layout | Rust Read Type |
|-------------|---------------|----------------|
| BOOLEAN | u8 (0=false, 1=true), 1 byte per slot | `*data_ptr.cast::<u8>().add(row)` |
| TINYINT | i8, 1 byte | `*data_ptr.cast::<i8>().add(row)` |
| SMALLINT | i16, 2 bytes | `*data_ptr.cast::<i16>().add(row)` |
| INTEGER | i32, 4 bytes | `*data_ptr.cast::<i32>().add(row)` |
| BIGINT | i64, 8 bytes | `*data_ptr.cast::<i64>().add(row)` |
| UTINYINT | u8, 1 byte | `*data_ptr.cast::<u8>().add(row)` |
| USMALLINT | u16, 2 bytes | `*data_ptr.cast::<u16>().add(row)` |
| UINTEGER | u32, 4 bytes | `*data_ptr.cast::<u32>().add(row)` |
| UBIGINT | u64, 8 bytes | `*data_ptr.cast::<u64>().add(row)` |
| FLOAT | f32, 4 bytes | `*data_ptr.cast::<f32>().add(row)` |
| DOUBLE | f64, 8 bytes | `*data_ptr.cast::<f64>().add(row)` |
| DATE | i32 (days since epoch), 4 bytes | `*data_ptr.cast::<i32>().add(row)` |
| TIMESTAMP | i64 (microseconds since epoch), 8 bytes | `*data_ptr.cast::<i64>().add(row)` |
| TIMESTAMP_S | i64 (seconds since epoch), 8 bytes | `*data_ptr.cast::<i64>().add(row)` |
| TIMESTAMP_MS | i64 (milliseconds since epoch), 8 bytes | `*data_ptr.cast::<i64>().add(row)` |
| TIMESTAMP_NS | i64 (nanoseconds since epoch), 8 bytes | `*data_ptr.cast::<i64>().add(row)` |
| TIMESTAMP_TZ | i64 (microseconds since epoch), 8 bytes | `*data_ptr.cast::<i64>().add(row)` |
| TIME | i64 (microseconds since midnight), 8 bytes | `*data_ptr.cast::<i64>().add(row)` |
| HUGEINT | `duckdb_hugeint { lower: u64, upper: i64 }`, 16 bytes | Cast to i128 or read as struct |
| UHUGEINT | `duckdb_uhugeint { lower: u64, upper: u64 }`, 16 bytes | Cast to u128 or read as struct |
| UUID | 16-byte blob (stored as `duckdb_uhugeint`), output as UUID | Write 16-byte raw to UUID output slot |
| DECIMAL | Backing type varies (i16/i32/i64/i128) per internal type from `duckdb_decimal_internal_type` | Read width/scale from logical type, read i128 backing |
| VARCHAR | `duckdb_string_t`, 16-byte union | Existing `read_varchar_from_vector()` — keep unchanged |
| LIST | `duckdb_list_entry { offset: u64, length: u64 }` parent + child vector | See Pattern 2 |
| ENUM | ordinal u8/u16/u32 depending on dictionary size | Read ordinal, call `duckdb_enum_dictionary_value()` |

**Example — boolean read:**

```rust
// Source: libduckdb-sys bindgen + DuckDB C API docs
unsafe fn read_bool_from_vector(
    chunk: ffi::duckdb_data_chunk,
    col_idx: usize,
    row_idx: usize,
) -> Option<bool> {
    let vector = ffi::duckdb_data_chunk_get_vector(chunk, col_idx as ffi::idx_t);
    // NULL check using validity mask (same pattern as read_varchar_from_vector)
    let validity = ffi::duckdb_vector_get_validity(vector);
    if !validity.is_null() {
        let entry = *validity.add(row_idx / 64);
        if entry & (1u64 << (row_idx % 64)) == 0 {
            return None; // NULL
        }
    }
    let data_ptr = ffi::duckdb_vector_get_data(vector);
    Some(*data_ptr.cast::<u8>().add(row_idx) != 0)
}
```

**Example — timestamp read (i64 microseconds):**

```rust
unsafe fn read_i64_from_vector(
    chunk: ffi::duckdb_data_chunk,
    col_idx: usize,
    row_idx: usize,
) -> Option<i64> {
    let vector = ffi::duckdb_data_chunk_get_vector(chunk, col_idx as ffi::idx_t);
    let validity = ffi::duckdb_vector_get_validity(vector);
    if !validity.is_null() {
        let entry = *validity.add(row_idx / 64);
        if entry & (1u64 << (row_idx % 64)) == 0 {
            return None;
        }
    }
    let data_ptr = ffi::duckdb_vector_get_data(vector);
    Some(*data_ptr.cast::<i64>().add(row_idx))
}
```

### Pattern 2: LIST Column Read and Write

**What:** LIST columns have a parent vector (holding `duckdb_list_entry { offset, length }` per row) and a child vector (holding the flat element values).

**When to use:** For LIST with scalar element types (INTEGER, BIGINT, DOUBLE, BOOLEAN, DATE, TIMESTAMP, VARCHAR) — covers the ARRAY_AGG use case.

**Reading from result chunk:**

```rust
// Source: libduckdb-sys bindgen — duckdb_list_entry, duckdb_list_vector_get_child
unsafe fn read_list_from_vector(
    chunk: ffi::duckdb_data_chunk,
    col_idx: usize,
    row_idx: usize,
    child_type_id: u32,
) -> Option<Vec<TypedScalar>> {
    let parent_vec = ffi::duckdb_data_chunk_get_vector(chunk, col_idx as ffi::idx_t);
    // Check NULL in parent validity mask
    let validity = ffi::duckdb_vector_get_validity(parent_vec);
    if !validity.is_null() {
        let entry = *validity.add(row_idx / 64);
        if entry & (1u64 << (row_idx % 64)) == 0 {
            return None;
        }
    }
    let entries = ffi::duckdb_vector_get_data(parent_vec);
    let entry = *entries.cast::<ffi::duckdb_list_entry>().add(row_idx);
    let offset = entry.offset as usize;
    let length = entry.length as usize;

    let child_vec = ffi::duckdb_list_vector_get_child(parent_vec);
    let mut result = Vec::with_capacity(length);
    for i in 0..length {
        result.push(read_scalar_from_vector(child_vec, offset + i, child_type_id));
    }
    Some(result)
}
```

**Writing to output (using duckdb-rs `ListVector`):**

```rust
// Source: duckdb-1.4.4/src/core/vector.rs — ListVector, set_entry, set_child
// output.list_vector(col_idx) returns a ListVector
// ListVector::set_entry(row, offset, length) writes the duckdb_list_entry
// ListVector::set_child([T]) writes the child flat data
```

### Pattern 3: DECIMAL Read with Logical Type Metadata

**What:** DECIMAL columns have a backing type (i16/i32/i64/i128) determined by precision. The scale (decimal places) and width come from the column's logical type, not the raw type ID.

**When to use:** All DECIMAL columns.

**Getting metadata:**

```rust
// Source: libduckdb-sys — duckdb_column_logical_type, duckdb_decimal_scale, duckdb_decimal_width
// duckdb_decimal_internal_type returns SMALLINT/INTEGER/BIGINT/HUGEINT based on precision
let logical_type = ffi::duckdb_column_logical_type(&mut result, col_idx as ffi::idx_t);
let width = ffi::duckdb_decimal_width(logical_type);
let scale = ffi::duckdb_decimal_scale(logical_type);
let internal = ffi::duckdb_decimal_internal_type(logical_type);
ffi::duckdb_destroy_logical_type(&mut logical_type);
```

**Declaring output column:**

```rust
// Source: duckdb-1.4.4/src/core/logical_type.rs — LogicalTypeHandle::decimal()
bind.add_result_column(name, LogicalTypeHandle::decimal(width, scale));
```

### Pattern 4: ENUM Read

**What:** ENUM columns store ordinal values (u8/u16/u32 depending on dictionary size). Decode to string using `duckdb_enum_dictionary_value()`.

**When to use:** ENUM source columns — output as VARCHAR (correct semantic output; users see string values).

```rust
// Source: libduckdb-sys — duckdb_enum_dictionary_value
let dict_val_ptr = ffi::duckdb_enum_dictionary_value(logical_type, ordinal as ffi::idx_t);
let s = CStr::from_ptr(dict_val_ptr).to_string_lossy().into_owned();
ffi::duckdb_free(dict_val_ptr.cast::<c_void>());
```

### Pattern 5: Integration PBT with In-Memory DuckDB Roundtrip

**What:** Generate typed Rust values → INSERT into DuckDB in-memory table → define semantic view → query via semantic_view() → assert output matches input.

**When to use:** For all Phase 13 type dispatch validation.

```rust
// Source: existing catalog.rs tests + expand_proptest.rs patterns
// Note: integration PBTs cannot use the loadable extension (no LOAD in bundled mode)
// Instead: test the binary-read + typed-write pipeline directly via table_function helpers
// OR: wire up the full VTab registration path in-process using register functions from lib.rs

proptest! {
    #[test]
    fn bigint_roundtrip(values in prop::collection::vec(i64::MIN..=i64::MAX, 1..20)) {
        let conn = Connection::open_in_memory().unwrap();
        // CREATE TABLE, INSERT values, run semantic_query, assert output matches
        // ...
    }
}
```

**Key constraint:** The VTab (`SemanticViewVTab`) can only be registered via `extension_entrypoint`. Integration tests must either:
1. Register the VTab directly using the `duckdb` crate's `Connection::register_table_function()` — this requires access to `QueryState` with a live catalog and conn handle, which is achievable in unit tests using `default` feature (bundled DuckDB)
2. OR test the `func()` internals directly by calling `execute_sql_raw` + the binary-read helpers without going through the full VTab path

**Recommendation (Claude's discretion):** Hybrid approach — unit PBTs test the binary-read helper functions in isolation (no VTab needed), integration PBTs test full end-to-end by registering the VTab using the high-level duckdb-rs API in bundled mode. This avoids needing the loadable extension path.

### Pattern 6: Writing BOOLEAN to Output

**What:** BOOLEAN output column uses `FlatVector::as_mut_slice::<u8>()` (1 byte per slot, 0=false, 1=true). The `type_from_duckdb_type_u32()` must return `LogicalTypeId::Boolean` for BOOLEAN type ID.

**Critical:** Current code falls through to the VARCHAR path for BOOLEAN — undefined FFI behaviour because DuckDB expects a 1-byte slot but the string inserter writes a `duckdb_string_t` (16 bytes). This is the UB bug.

```rust
// Fix: in write_typed_column, add BOOLEAN arm
BOOLEAN => {
    let null_positions: Vec<usize> = /* ... */;
    {
        let dst = out_vec.as_mut_slice_with_len::<u8>(n_rows);
        for (i, val) in values.iter().enumerate() {
            dst[i] = if let TypedValue::Bool(b) = val { *b as u8 } else { 0 };
        }
    }
    for i in null_positions { out_vec.set_null(i); }
}
```

### Anti-Patterns to Avoid

- **Keeping `build_varchar_cast_sql()` for some types:** Partial removal creates two code paths that must be tested separately and can diverge. Remove entirely — binary reads are the unified path.
- **Forgetting to call `duckdb_destroy_logical_type()`:** Logical type handles returned by `duckdb_column_logical_type()` are heap-allocated and must be freed. Memory leak if skipped.
- **Assuming DECIMAL internal type is always HUGEINT:** DuckDB uses SMALLINT (i16) for DECIMAL(1-4,s), INTEGER (i32) for DECIMAL(5-9,s), BIGINT (i64) for DECIMAL(10-18,s), HUGEINT (i128) for DECIMAL(19-38,s). Must dispatch on `duckdb_decimal_internal_type`.
- **Writing HUGEINT as i64 into a 16-byte slot:** Current code declares output as BIGINT for HUGEINT (correct — avoids writing 8 bytes into a 16-byte slot), but the read must also read only 8 bytes (the lower 64 bits), not the full 16.
- **Collecting all rows before writing (current architecture):** The current code collects all rows across all chunks into `col_strings`, then writes. This is correct architecture to keep — the binary-read version should do the same (collect all rows as `TypedValue`s, then write).
- **Using `proptest!` macro in integration tests that do slow I/O without lowering case count:** Default proptest runs 256 cases. Integration tests involving DuckDB query execution should use `ProptestConfig { cases: 20, .. ProptestConfig::default() }` to keep test time under 30 seconds.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DECIMAL output column type declaration | Custom raw FFI `duckdb_create_decimal_type` call | `LogicalTypeHandle::decimal(width, scale)` | Already implemented in duckdb-rs; handles the FFI call and Drop |
| LIST output column type declaration | Custom raw `duckdb_create_list_type` call | `LogicalTypeHandle::list(child)` | Already implemented in duckdb-rs |
| LIST data writing | Manual `duckdb_list_entry` + child vector management | `DataChunkHandle::list_vector(col_idx)` → `ListVector::set_entry()` + `ListVector::set_child()` | duckdb-rs `ListVector` encapsulates the two-pointer (parent entries + child) layout |
| Proptest value generation for numeric types | Custom `Strategy` for i64, i32, f64 ranges | Proptest's built-in range strategies: `i64::MIN..=i64::MAX`, `any::<bool>()`, `-1e15_f64..1e15_f64` | Proptest handles edge cases, NaN, infinity, shrinking |
| NULL detection in binary chunk | Manual bit arithmetic | Same pattern as existing `read_varchar_from_vector()` validity mask check | Already proven correct in production; copy the validity mask pattern |

**Key insight:** The duckdb-rs layer at `duckdb = "=1.4.4"` provides `LogicalTypeHandle::decimal()`, `LogicalTypeHandle::list()`, `DataChunkHandle::list_vector()`, and `FlatVector::as_mut_slice_with_len()` — these are the correct abstractions. Only drop to raw `libduckdb-sys` FFI for reading from result chunks (which are in the C API result format, not a duckdb-rs VTab output chunk).

---

## Common Pitfalls

### Pitfall 1: DECIMAL Internal Type Mismatch

**What goes wrong:** Reading all DECIMAL columns as HUGEINT (i128) when the internal type is SMALLINT (i16) — reads garbage bytes for low-precision DECIMALs.

**Why it happens:** DECIMAL is a logical type with a physical backing type that varies by precision. `duckdb_column_type()` returns `DUCKDB_TYPE_DECIMAL (15)` for all DECIMALs; the physical type requires `duckdb_decimal_internal_type()` on the logical type handle.

**How to avoid:** Always call `duckdb_column_logical_type()` for DECIMAL columns, then `duckdb_decimal_internal_type()` to get the backing type (SMALLINT/INTEGER/BIGINT/HUGEINT), and dispatch the binary read on that backing type.

**Warning signs:** Test with DECIMAL(3,2) (maps to SMALLINT backing) — if output is garbage, the internal type dispatch is wrong.

### Pitfall 2: Logical Type Handle Memory Leak

**What goes wrong:** `duckdb_column_logical_type()` returns a heap-allocated `duckdb_logical_type` handle that must be freed with `duckdb_destroy_logical_type()`. Forgetting this leaks memory.

**Why it happens:** The C API requires explicit destroy calls for all handles; Rust's ownership model doesn't apply to opaque C pointers.

**How to avoid:** Use RAII wrapper or ensure `duckdb_destroy_logical_type(&mut logical_type)` is called in all code paths (including error paths). Consider a small `struct LogicalTypeOwned(ffi::duckdb_logical_type)` with `impl Drop`.

**Warning signs:** Valgrind / leak sanitizer reports under the DECIMAL/LIST/ENUM inference path.

### Pitfall 3: ENUM Ordinal Width Depends on Dictionary Size

**What goes wrong:** Reading ENUM ordinal as u8 when the dictionary has > 255 entries (stored as u16) or > 65535 entries (stored as u32).

**Why it happens:** DuckDB chooses the smallest integer type that fits the dictionary: u8 for ≤ 255 values, u16 for ≤ 65535, u32 otherwise.

**How to avoid:** Call `duckdb_enum_dictionary_size()` and dispatch the ordinal read width accordingly.

**Warning signs:** Large ENUM columns (e.g., 300 distinct values) return wrong string values — ordinal bytes are misaligned.

### Pitfall 4: LIST Child Vector Is Flat (Not Per-Row)

**What goes wrong:** Assuming the child vector of a LIST column has one entry per row — it is actually a flat array of all element values across all rows. Each row's elements are located at `child[entry.offset .. entry.offset + entry.length]`.

**Why it happens:** The `duckdb_list_entry { offset: u64, length: u64 }` layout is unintuitive — offset is into the shared flat child vector, not into a per-row subarray.

**How to avoid:** Always use `entry.offset` + `entry.length` to slice the child, not `row_idx * max_length`.

**Warning signs:** Second list row always returns elements from the beginning of the child vector (offset=0 assumption).

### Pitfall 5: ProptestConfig Case Count in Integration Tests

**What goes wrong:** Integration tests with DuckDB query execution at default 256 cases can take 30-60 seconds, slowing the test suite enough that developers skip running it.

**Why it happens:** Each proptest case spawns a DuckDB in-memory connection + table creation + query execution.

**How to avoid:** Annotate integration PBT tests with `#[test]` and configure within the `proptest!` block: use `ProptestConfig { cases: 20, .. ProptestConfig::default() }` or the `#[proptest(cases = 20)]` attribute form.

**Warning signs:** `cargo test` takes > 30 seconds for the new test file.

### Pitfall 6: TIMESTAMP NULL Bug — Root Cause

**What goes wrong:** TIMESTAMP values come back all-NULL today because `parse_typed_from_str()` tries to parse the human-readable string `"2024-01-15 10:30:00"` as `i64` (microseconds), which fails and falls through to `TypedValue::Null`.

**Why it happens:** The VARCHAR cast wrapper converts TIMESTAMP to its display string (e.g., `"2024-01-15 10:30:00.000000"`), but `parse_typed_from_str` expects a raw microsecond integer string.

**How to avoid:** The binary read approach reads i64 microseconds directly — no string parsing, no format mismatch.

**Warning signs:** Any test with a TIMESTAMP column returning non-NULL source values but NULL output confirms the bug is present.

---

## Code Examples

Verified patterns from official sources:

### Validity Mask Check (confirmed in existing production code)

```rust
// Source: existing read_varchar_from_vector() in src/query/table_function.rs
unsafe fn is_null(vector: ffi::duckdb_vector, row_idx: usize) -> bool {
    let validity = ffi::duckdb_vector_get_validity(vector);
    if !validity.is_null() {
        let entry_idx = row_idx / 64;
        let bit_idx = row_idx % 64;
        let entry = *validity.add(entry_idx);
        if entry & (1u64 << bit_idx) == 0 {
            return true; // NULL
        }
    }
    false
}
```

### Declare DECIMAL Output Column (confirmed from duckdb-1.4.4/src/core/logical_type.rs)

```rust
// Source: duckdb-1.4.4/src/core/logical_type.rs line 234-240
// LogicalTypeHandle::decimal(width, scale) wraps duckdb_create_decimal_type
bind.add_result_column(col_name, LogicalTypeHandle::decimal(width, scale));
```

### Declare LIST Output Column (confirmed from duckdb-1.4.4/src/core/logical_type.rs)

```rust
// Source: duckdb-1.4.4/src/core/logical_type.rs line 216-222
// LogicalTypeHandle::list(child_type) wraps duckdb_create_list_type
let child = LogicalTypeHandle::from(LogicalTypeId::Bigint);
bind.add_result_column(col_name, LogicalTypeHandle::list(&child));
```

### Write LIST to Output (confirmed from duckdb-1.4.4/src/core/vector.rs)

```rust
// Source: duckdb-1.4.4/src/core/vector.rs — ListVector
let mut list_vec = output.list_vector(col_idx);
// For each row: set_entry(row_idx, offset_in_child, length)
list_vec.set_entry(row_idx, child_offset, list_len);
// Write child elements (for i64 children):
list_vec.set_child(&child_data_flat);  // &[i64]
```

### Get Column Logical Type (confirmed from libduckdb-sys-1.4.4 bindgen)

```rust
// Source: libduckdb-sys-1.4.4/src/bindgen_bundled_version_loadable.rs line 2242
let logical_type = ffi::duckdb_column_logical_type(&mut result, col_idx as ffi::idx_t);
let width = ffi::duckdb_decimal_width(logical_type);
let scale = ffi::duckdb_decimal_scale(logical_type);
let internal_type = ffi::duckdb_decimal_internal_type(logical_type);
ffi::duckdb_destroy_logical_type(&mut { logical_type });
```

### Proptest Integration Test Structure

```rust
// Source: adapted from existing tests/expand_proptest.rs pattern
use proptest::prelude::*;
use duckdb::Connection;

proptest! {
    #![proptest_config(ProptestConfig { cases: 20, .. ProptestConfig::default() })]

    #[test]
    fn bigint_column_roundtrip(
        values in prop::collection::vec(prop::num::i64::ANY, 1..10)
    ) {
        let conn = Connection::open_in_memory().unwrap();
        // 1. CREATE TABLE t (v BIGINT)
        // 2. INSERT values
        // 3. Register semantic view + query via semantic_view()
        // 4. Assert output column type is BIGINT, values match
    }
}
```

### Extended `TypedValue` Enum

```rust
// Phase 13 extends the current TypedValue enum:
enum TypedValue {
    Null,
    Bool(bool),     // NEW: BOOLEAN binary read
    I8(i8),         // NEW: TINYINT
    I16(i16),       // NEW: SMALLINT
    I32(i32),       // INTEGER, DATE (kept)
    I64(i64),       // BIGINT, TIMESTAMP, etc. (kept)
    U8(u8),         // NEW: UTINYINT
    U16(u16),       // NEW: USMALLINT
    U32(u32),       // NEW: UINTEGER
    U64(u64),       // NEW: UBIGINT
    F32(f32),       // NEW: FLOAT
    F64(f64),       // DOUBLE (kept)
    I128(i128),     // NEW: HUGEINT backing for DECIMAL
    Str(String),    // VARCHAR, ENUM decoded, fallback (kept)
    List(Vec<TypedValue>),  // NEW: LIST with scalar elements
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `duckdb_value_varchar` C API | Binary chunk reads via `duckdb_vector_get_data` | Phase 12 partially migrated; Phase 13 completes | Old API unreliable with chunked results; binary reads are the canonical method |
| VARCHAR cast SQL wrapper | Direct binary reads per type | Phase 13 removes the wrapper | Eliminates silent NULL/UB bugs; no string parse/reformat overhead |
| `date_str_to_epoch_days()` | Binary read i32 directly | Phase 13 deletes this function | DATE values are stored as i32 days-since-epoch in binary; no string parsing needed |

**Deprecated/outdated:**

- `build_varchar_cast_sql()`: delete in Phase 13 — replaced by binary reads
- `parse_typed_from_str()`: delete in Phase 13 — replaced by binary reads
- `date_str_to_epoch_days()`: delete in Phase 13 — DATE binary read returns i32 directly

---

## Open Questions

1. **How to wire up VTab registration in integration PBTs**
   - What we know: `SemanticViewVTab` requires `QueryState` (catalog + raw conn). The bundled feature enables `Connection::open_in_memory()` in tests. The duckdb-rs crate provides `Connection::register_table_function()`.
   - What's unclear: Whether `Connection::register_table_function()` with a bundled-mode `QueryState` (using `ffi::duckdb_connect` on the same database) will work without the loadable extension entrypoint path. The raw `ffi::duckdb_database` handle is needed to create a second connection.
   - Recommendation: Use `duckdb::Connection::open_in_memory()` and access the raw handle via `Connection::path()` or via transmute/unsafe access (the existing lib.rs pattern uses `Connection::open_from_raw`). Alternatively, test the `execute_sql_raw` + binary-read helpers directly without the full VTab, accepting that the VTab registration path is tested via the existing sqllogictest CI.

2. **DECIMAL binary output: should Phase 13 write DECIMAL or DOUBLE?**
   - What we know: DuckDB's output DECIMAL type requires `LogicalTypeHandle::decimal(width, scale)` for `bind()` and writing via `as_mut_slice::<i128>()` (or the appropriate backing type) for `func()`. The current code emits VARCHAR for DECIMAL.
   - What's unclear: Writing DECIMAL to a VTab output slot requires the output vector to be declared as DECIMAL and the backing slot size must match `duckdb_decimal_internal_type`. This is doable but requires storing (width, scale, internal_type) at bind time.
   - Recommendation: Implement full DECIMAL support (read i128, write as DECIMAL(width,scale)). Store width+scale in `SemanticViewBindData` alongside the type_id, or re-query logical type at func() time via the result metadata. The CONTEXT.md explicitly calls for native DECIMAL output.

3. **`SemanticViewDefinition` model changes for DECIMAL/LIST metadata**
   - What we know: CONTEXT.md notes that DECIMAL scale and LIST child type need to survive DDL-time inference to be available at bind time. Phase 12 added `column_type_names` + `column_types_inferred` (Vec<u32> of raw type IDs) — raw type ID is insufficient for DECIMAL (need width+scale) and LIST (need child type ID).
   - What's unclear: Whether to extend the model (add `column_logical_type_meta: Vec<Option<LogicalTypeMeta>>`) or re-run LIMIT 0 at bind time to re-derive logical type metadata.
   - Recommendation: Re-run `duckdb_column_logical_type` at bind time from the existing bind-time LIMIT 0 result (when DDL-time type_map is empty, the fallback path already runs LIMIT 0). For the primary path (DDL-time inference stored), either extend the model with a metadata field or re-run LIMIT 0 at bind time only for DECIMAL/LIST columns. Re-running at bind time is simpler and avoids model schema changes.

---

## Validation Architecture

> `workflow.nyquist_validation` is not present in `.planning/config.json` — this section is included for completeness per standard template, but Nyquist validation is not configured.

### Test Framework

| Property | Value |
|----------|-------|
| Framework | proptest 1.10.0 (dev-dependency) + duckdb bundled (default features) |
| Config file | none — configured via `ProptestConfig` in test code |
| Quick run command | `cargo test output_proptest -- --test-threads=1` |
| Full suite command | `cargo test` |

### Phase Requirements → Test Map

Phase 13 has no formal requirement IDs assigned in REQUIREMENTS.md. The following maps to the bug fixes and type coverage from CONTEXT.md.

| Bug/Feature | Behavior | Test Type | Automated Command | File Exists? |
|-------------|----------|-----------|-------------------|-------------|
| TIMESTAMP NULL bug | TIMESTAMP columns return values, not NULL | integration PBT | `cargo test output_proptest::timestamp` | No — Wave 0 |
| BOOLEAN UB bug | BOOLEAN columns write correct 0/1 bytes | unit PBT | `cargo test output_proptest::boolean_write` | No — Wave 0 |
| DECIMAL as VARCHAR bug | DECIMAL output is DECIMAL type, not VARCHAR | integration PBT | `cargo test output_proptest::decimal` | No — Wave 0 |
| LIST as string bug | LIST(BIGINT) output is LIST type, not VARCHAR | integration PBT | `cargo test output_proptest::list_bigint` | No — Wave 0 |
| Integer type dispatch | All int types (TINYINT..UBIGINT) roundtrip | unit PBT | `cargo test output_proptest::integer_types` | No — Wave 0 |
| Float type dispatch | FLOAT, DOUBLE roundtrip | unit PBT | `cargo test output_proptest::float_types` | No — Wave 0 |
| DATE binary read | DATE values read as i32, not parsed from string | unit PBT | `cargo test output_proptest::date_binary` | No — Wave 0 |
| NULL propagation | NULL source values → NULL output values | integration PBT | `cargo test output_proptest::null_propagation` | No — Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test output_proptest -- --test-threads=1`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- `tests/output_proptest.rs` — new file covering all Phase 13 type dispatch

---

## Sources

### Primary (HIGH confidence)

- `libduckdb-sys-1.4.4/src/bindgen_bundled_version_loadable.rs` — confirmed presence of all needed FFI functions: `duckdb_vector_get_data`, `duckdb_vector_get_validity`, `duckdb_list_vector_get_child`, `duckdb_list_vector_get_size`, `duckdb_list_vector_set_size`, `duckdb_column_logical_type`, `duckdb_decimal_width`, `duckdb_decimal_scale`, `duckdb_decimal_internal_type`, `duckdb_enum_dictionary_value`, `duckdb_enum_dictionary_size`, `duckdb_destroy_logical_type`, `duckdb_create_decimal_type`, `duckdb_create_list_type`
- `duckdb-1.4.4/src/core/logical_type.rs` — confirmed `LogicalTypeHandle::decimal(width, scale)`, `LogicalTypeHandle::list(child)`, `LogicalTypeHandle::decimal_width()`, `LogicalTypeHandle::decimal_scale()`
- `duckdb-1.4.4/src/core/data_chunk.rs` — confirmed `DataChunkHandle::list_vector(idx)`, `DataChunkHandle::flat_vector(idx)`
- `duckdb-1.4.4/src/core/vector.rs` — confirmed `ListVector::set_entry()`, `ListVector::set_child()`, `ListVector::set_len()`, `FlatVector::as_mut_slice_with_len()`
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/src/query/table_function.rs` — existing patterns for `read_varchar_from_vector` (validity mask), `write_typed_column`, `parse_typed_from_str`, `build_varchar_cast_sql`
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/tests/expand_proptest.rs` — existing proptest infrastructure: `proptest!` macro, `arb_query_request`, fixture definitions
- `Cargo.toml` / `Cargo.lock` — `proptest = "1.10.0"` confirmed in dev-dependencies; `duckdb/bundled` feature confirmed for test builds

### Secondary (MEDIUM confidence)

- proptest 1.9 docs (docs.rs/proptest) — confirmed `prop_oneof!`, `prop_compose!`, `proptest!`, `prop_assert*`, `ProptestConfig`; range strategies for numeric types; shrinking behavior
- DuckDB C API documentation (inferred from bindgen output) — binary layouts for BOOLEAN (u8), TIMESTAMP (i64 µs), DATE (i32 days), LIST (duckdb_list_entry struct with u64 offset+length), HUGEINT (16-byte struct lower/upper)

### Tertiary (LOW confidence)

- DuckDB DECIMAL internal type dispatch (i16 for precision ≤4, i32 for ≤9, i64 for ≤18, i128 for ≤38) — inferred from DuckDB source conventions, not verified against 1.4.4 source code directly; confirmed `duckdb_decimal_internal_type()` exists and returns a `duckdb_type` which will contain SMALLINT/INTEGER/BIGINT/HUGEINT

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all libraries confirmed present in the pinned versions (libduckdb-sys=1.4.4, duckdb=1.4.4, proptest=1.10.0)
- Architecture: HIGH — all FFI functions verified in bindgen output; all duckdb-rs abstractions verified in source
- Pitfalls: MEDIUM — DECIMAL internal type dispatch conventions inferred, not verified from DuckDB 1.4.4 source; other pitfalls verified from C API docs and existing code patterns

**Research date:** 2026-03-02
**Valid until:** 2026-04-02 (pinned library versions; binary layouts are stable for 1.4.4)
