# Vector Copy Research

## Current Architecture

The `semantic_view()` table function executes expanded SQL on a separate DuckDB connection,
then manually reads each value per-type from the inner result and writes it to the output chunk.

**Key files**: `src/query/table_function.rs` (lines 529-628 for `func()`, 817-1026 for
`read_typed_from_vector`, 1037-1378 for `write_typed_column`)

**Problems**:
1. ~560 lines of type-dispatch code (read + write) that duplicates for every DuckDB type
2. STRUCT/MAP types fall back to VARCHAR — no structured output
3. **Buffer overflow bug**: `func()` accumulates ALL rows from ALL chunks into a single
   `Vec<TypedValue>` per column, then writes them to the output chunk. DuckDB output chunks
   are limited to `STANDARD_VECTOR_SIZE` (2048 rows). If the inner query returns >2048 rows,
   `output.set_len(total_rows)` and the slice writes overflow the chunk buffer.
4. `func()` is called once, sets `done = true`, and returns. DuckDB's table function protocol
   expects streaming: `func()` is called repeatedly, returning one chunk at a time, until
   `output.set_len(0)` signals completion.

## Discovery: `duckdb_vector_reference_vector` and `duckdb_vector_copy_sel`

DuckDB's C API exposes two functions that can replace all type-dispatch code:

### `duckdb_vector_reference_vector(to_vector, from_vector)`
- Makes `to_vector` reference `from_vector` with **shared ownership**
- After the call, both vectors share the underlying buffer
- Zero-copy: no data movement at all
- Type-generic: works for ANY DuckDB type including STRUCT, MAP, LIST, nested types

### `duckdb_vector_copy_sel(src, dst, sel, src_count, src_offset, dst_offset)`
- Copies data from `src` to `dst` using a selection vector
- Type-generic: handles all types including nested
- With an identity selection vector (0, 1, 2, ..., n-1), acts as a memcpy
- Small overhead vs reference_vector but still eliminates all manual type dispatch

Both are available in the `libduckdb-sys` FFI bindings (confirmed in bindgen.rs).

## Lifetime Safety Question

The critical question for `reference_vector` is: does the reference increment a refcount
(shared ownership) or is it a shallow pointer alias?

The DuckDB header comment says "the vectors share ownership of the data", suggesting refcounting.
But this needs empirical verification — if the source chunk is destroyed and the reference
is shallow, reading from the output chunk would cause use-after-free.

**Test plan** (implemented in `tests/vector_reference_test.rs`):
1. Execute a query returning multiple chunks
2. For each source chunk: create output chunk, reference vectors, destroy source, read from output
3. If values are correct after source destruction → safe (shared ownership)
4. If crash/corruption → unsafe (shallow alias), fall back to `vector_copy_sel`

## Snowflake AGG Syntax Analysis

Snowflake semantic views support `AGG:` prefix syntax (e.g., `AGG:SUM(amount)`) for aggregate
measures. This is parsed by the Snowflake SQL parser itself.

In DuckDB extensions, this is **not feasible**:
- Python DuckDB compiles all C++ with `-fvisibility=hidden`
- Parser hooks are not accessible from loadable extensions
- The `duckdb_entrypoint` function has no parser registration mechanism

The current approach (table function with named parameters) is the correct strategy.

## DuckDB Extension Point Inventory

What IS available from loadable extensions:
- Table functions (VTab) — used for `semantic_view()`
- Scalar functions (VScalar) — used for DDL functions
- Pragma functions — available but limited
- Aggregate functions — available
- Copy functions — available
- Type definitions — available
- All C API data manipulation functions (vector ops, chunk ops, etc.)

What is NOT available:
- Parser hooks (syntax extension)
- Catalog entry types (custom catalog objects)
- Optimizer rules
- Internal table/view creation during extension load (must use SQL)

## The >2048 Row Bug

Current `func()` collects ALL rows into `Vec<TypedValue>` then writes to a single output chunk.
DuckDB chunks are limited to `STANDARD_VECTOR_SIZE` (2048). With >2048 rows:

- `as_mut_slice_with_len(n_rows)` on a 2048-slot chunk with n_rows > 2048 → buffer overwrite
- `output.set_len(total_rows)` sets chunk size beyond its capacity

This is a latent memory safety bug that would manifest with any query returning >2048 rows.

## Recommended Approach

### Streaming refactor with vector_reference_vector (primary) or vector_copy_sel (fallback)

1. **Move result state to `InitData`**: Store `duckdb_result`, `chunk_count`, `current_chunk_idx`
2. **Streaming `func()`**: Each call fetches one source chunk, copies/references vectors to
   output chunk, sets output size, advances index. When all chunks consumed, sets size to 0.
3. **Delete ~560 lines**: `read_typed_from_vector`, `write_typed_column`, `TypedValue` enum,
   `read_varchar_from_vector`, `read_varchar_from_raw_vector`
4. **STRUCT/MAP support**: Vector reference/copy is type-generic — complex types just work
5. **Remove type normalization hacks**: No need for HUGEINT→BIGINT normalization since we're
   not reading/writing individual values

### Why this works
- DuckDB's table function protocol already expects streaming (repeated `func()` calls)
- Source chunks from `duckdb_result_get_chunk` are already sized at STANDARD_VECTOR_SIZE
- Vector reference/copy handles ALL types, including nested STRUCT/MAP/LIST
- The output chunk provided by DuckDB to `func()` is already the right size

### What stays the same
- `bind()` — still needs type inference for output column declaration
- `expand.rs` — SQL generation unchanged
- `ddl/` — all DDL functions unchanged
- `lib.rs` — connection setup unchanged
- `explain.rs` — explain function unchanged
