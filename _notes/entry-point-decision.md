# Entry Point Decision: Phase 15

**Date:** 2026-03-07
**Decision:** GO
**Strategy:** Option A (C_STRUCT + C++ helper) with amalgamation source compilation

## What Was Tried

### Option B: CPP entry point (DUCKDB_CPP_EXTENSION_ENTRY)

Attempted first as the primary path. The C++ entry point (`semantic_views_duckdb_cpp_init`) would own the DuckDB handshake and delegate to Rust via FFI for catalog/function registration.

**Result: Failed.** The `DUCKDB_CPP_EXTENSION_ENTRY` macro and `ExtensionLoader` reference non-inlined C++ symbols that are not available under Python DuckDB's `-fvisibility=hidden`. The extension compiled but crashed at load time with unresolved symbols.

### Option A: C_STRUCT entry + C++ helper

Kept the existing Rust entry point (`semantic_views_init_c_api`, C_STRUCT ABI). After Rust-side initialization, calls `sv_register_parser_hooks()` in the C++ shim to register `ParserExtension` hooks on `DBConfig`.

**Initial attempt (header-only):** Compiled shim.cpp against `duckdb.hpp` only. Required manual symbol stubs for `Function`, `SimpleFunction`, `SimpleNamedParameterFunction`, `TableFunction` constructors/destructors, `DBConfig::GetConfig`, and more. Each stub revealed the next missing symbol ("whack-a-mole"). RTTI typeinfo for `GlobalTableFunctionState` was the final blocker under Python DuckDB.

**Final approach (amalgamation source):** Compiled `duckdb.cpp` (23MB amalgamation source) alongside `shim.cpp` via the `cc` crate. This provides ALL DuckDB C++ symbol definitions — constructors, destructors, RTTI, vtables — eliminating the need for any manual stubs. Symbol visibility on the cdylib restricts exports to `semantic_views_init_c_api` only, so the internal DuckDB definitions don't conflict with the host process.

**Result: Success.** Extension loads under Python DuckDB, all tests pass, parser hook chain works.

## Why It Works

1. **Amalgamation compilation** provides every symbol the C++ shim needs, regardless of host process visibility settings.
2. **Symbol visibility** (`-exported_symbols_list` on macOS, `--dynamic-list` on Linux) ensures the internal DuckDB symbols stay local to the extension binary — no ODR conflicts with the host.
3. **C_STRUCT ABI** means DuckDB initializes the Rust C API function pointer stubs (`duckdb_rs_extension_api_init`) automatically, so all duckdb-rs calls work normally.
4. **Parser hooks** are registered after Rust init by extracting `DatabaseInstance&` from the `duckdb_database` C API handle via `DatabaseWrapper`.

## Key Findings

- **C API stub initialization:** Works automatically under C_STRUCT ABI. Was the primary risk with Option B (CPP ABI skips this initialization).
- **Connection lifetime:** No issues — Rust creates connections via `duckdb_connect()` as before.
- **Parser hook registration:** Confirmed working. `DBConfig::GetConfig(db).parser_extensions.push_back(ext)` registers the hook on the live database.
- **Hook chain verification:** `CREATE SEMANTIC VIEW anything(...)` triggers `sv_parse_stub` -> `sv_plan_stub` -> returns `'CREATE SEMANTIC VIEW stub fired'`. Verified under Python DuckDB.
- **Build time:** First compilation of `duckdb.cpp` takes ~2.5 minutes. Cached by `cc` crate on subsequent builds (recompiles only when source changes).
- **Symbol stubs are fragile:** The header-only approach required manually reimplementing DuckDB internals and broke on RTTI. The amalgamation approach is robust and future-proof.

## Implications for Phase 16+

1. **C++ shim can freely use any DuckDB C++ type** — ParserExtension, TableFunction, Connection, ClientContext, etc. No symbol concerns.
2. **Entry point stays Rust-owned** (C_STRUCT ABI). C++ is a helper, not the entry point.
3. **`duckdb.cpp` must be version-pinned** to match `duckdb.hpp` and `TARGET_DUCKDB_VERSION`. The CI DuckDB Version Monitor workflow should re-fetch both files.
4. **Phase 16 (statement rewrite)** can implement full `sv_plan_stub` logic: parse the `CREATE SEMANTIC VIEW` text, construct appropriate parameters, and return a `TableFunction` that delegates to the existing `create_semantic_view` registration.
5. **Binary size** increases from amalgamation (~10-20MB debug). Release builds with LTO will strip unused symbols.
