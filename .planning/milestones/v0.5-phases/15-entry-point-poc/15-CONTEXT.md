# Phase 15: Entry Point POC - Context

**Gathered:** 2026-03-07
**Status:** Ready for planning

<domain>
## Phase Boundary

Determine which entry point strategy allows parser hook registration while preserving existing Rust C API functionality. This is a spike — the output is a working extension binary with a stub parser hook, not a releasable feature. Phase 16+ builds real parsing on top of whatever strategy succeeds here.

</domain>

<decisions>
## Implementation Decisions

### POC execution order
- Try Option B first (CPP entry via `DUCKDB_CPP_EXTENSION_ENTRY`, delegates to Rust via FFI) — this is the proven pattern used by prql/duckpgq
- If Option B works and existing tests pass, record the decision and move on — no need to also prove Option A
- If Option B fails, pivot to Option A (keep C_STRUCT, call C++ helper from Rust init) before declaring no-go
- Only declare no-go if both options fail

### Go/no-go recording
- Record decision in both phase verification report AND a dedicated `_notes/entry-point-decision.md` doc with full rationale
- STATE.md accumulated decisions updated as usual

### duckdb.hpp sourcing
- Vendor `duckdb.hpp` (amalgamation header only, ~800KB) in `cpp/include/duckdb.hpp`
- Header only — no `duckdb.cpp` source file needed for the shim
- Keep header up to date via the existing DuckDB Version Monitor CI action (add a step to re-fetch the header when bumping versions)
- Shim source lives at `cpp/src/shim.cpp`

### Verification approach
- This is a spike, not a release — no new CI tests needed for Phase 15
- Verification: stub `parse_function` returns a no-op statement (e.g., `SELECT 'CREATE SEMANTIC VIEW stub fired'`) proving the full hook chain works: parse → plan → execute
- Existing functionality verified by running `just test-all` (full suite: Rust unit, sqllogictest, DuckLake CI)
- Phase 16 adds proper test coverage

### Cargo test isolation
- C++ shim compilation feature-gated: build.rs only compiles `shim.cpp` when `CARGO_FEATURE_EXTENSION` is set
- `cc` crate is an optional build-dependency, gated on the `extension` feature — `cargo test` (bundled mode) never downloads or uses it
- Zero impact on existing developer workflow: `cargo test` remains pure Rust

### Entry point design
- Clean break: rewrite init to the from-scratch design where C++ entry is the only DuckDB entry point
- C++ entry owns: DuckDB handshake, parser hook registration, calling Rust init
- Rust init owns: catalog setup, DDL function registration, query function registration (all existing logic)
- No legacy naming artifacts — the Rust function was never an entry point, just an internal init called by C++
- Work on a feature branch, not main

### Claude's Discretion
- Exact FFI function signatures between C++ and Rust
- Connection lifetime management (how the C++ entry creates and passes the duckdb_connection to Rust)
- Symbol visibility list updates in build.rs (which symbols to export for CPP vs C_STRUCT)
- Error handling across the C++/Rust FFI boundary
- Whether to use `ExtensionLoader&` (modern API) or `DatabaseInstance&` (older API) in the C++ entry

</decisions>

<specifics>
## Specific Ideas

- prql extension is the reference implementation for CPP entry + parse_function fallback — follow its patterns where applicable
- Investigation doc (`_notes/parser-extension-investigation.md`) has the proposed architecture diagram and detailed analysis

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/lib.rs:init_extension()` — all the catalog/function registration logic, reusable as the Rust init function called by C++
- `build.rs` — symbol visibility infrastructure for Linux (--dynamic-list) and macOS (-exported_symbols_list), needs updating for new entry point name
- `src/lib.rs:semantic_views_init_c_api_internal()` — manual FFI entrypoint with db_handle extraction, pattern to follow for the new Rust init
- `.github/workflows/DuckDBVersionMonitor.yml` — needs a step added to update vendored duckdb.hpp

### Established Patterns
- Feature-gating: `#[cfg(feature = "extension")]` for extension-only code, `#[cfg(not(feature = "extension"))]` for test helpers
- Manual FFI entrypoint: hand-written instead of macro to capture raw duckdb_database handle
- Separate connections: persist_conn for DDL writes, query_conn for semantic_view execution — both created from db_handle

### Integration Points
- `build.rs` — add cc crate compilation of shim.cpp (feature-gated)
- `Cargo.toml` — add cc as optional build-dependency
- `src/lib.rs` extension module — rewrite entry point, expose Rust init as extern "C"
- `cpp/src/shim.cpp` — new file: C++ entry point + parser hook registration
- `cpp/include/duckdb.hpp` — new vendored file

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 15-entry-point-poc*
*Context gathered: 2026-03-07*
