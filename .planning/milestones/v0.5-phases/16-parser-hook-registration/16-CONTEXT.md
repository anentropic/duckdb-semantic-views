# Phase 16: Parser Hook Registration - Context

**Gathered:** 2026-03-07
**Status:** Ready for planning

<domain>
## Phase Boundary

Move parse logic from the C++ stub (sv_parse_stub) to Rust via FFI trampoline. DuckDB's parser calls the extension's parse_function for unrecognized statements, and the Rust side detects `CREATE SEMANTIC VIEW` prefix. Only `CREATE SEMANTIC VIEW` — no DROP/DESCRIBE/SHOW (those are future requirements DDL-F01 through F06, post-spike).

</domain>

<decisions>
## Implementation Decisions

### Claude's Discretion
All three gray areas were deferred to Claude's judgment:

- **Plan function behavior** — Whether sv_plan_stub passes parsed statement data through to Phase 17 or stays as a dummy stub. Claude decides based on what makes Phase 17 easiest to build on.
- **Test strategy** — What mix of sqllogictest and Rust unit tests to add in Phase 16 vs deferring to Phase 18. Claude decides based on success criteria coverage.
- **Parse result detail** — Whether Rust parse function just detects the prefix and passes raw text, or also extracts view name/body. Claude decides based on what Phase 17 needs.

</decisions>

<specifics>
## Specific Ideas

- "By end of spike I want to be able to see for myself that the basic setup works end to end — install the extension via Python client, see parser fallback do something"
- The user's verification path: Python DuckDB client → LOAD extension → type CREATE SEMANTIC VIEW → see visible result from parser hook chain
- This shapes all phases: whatever Phase 16 builds must work under Python DuckDB's -fvisibility=hidden (already validated in Phase 15)

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `cpp/src/shim.cpp`: sv_parse_stub (C++ detection logic to be replaced by Rust FFI call), sv_plan_stub (returns dummy TableFunction result), sv_register_parser_hooks (registration via DBConfig), SemanticViewParseData (carries query text from parse to plan)
- `src/lib.rs`: init_extension() already calls sv_register_parser_hooks(db_handle) — FFI bridge pattern established
- `build.rs`: cc crate compilation of shim.cpp + duckdb.cpp amalgamation, feature-gated on `extension`

### Established Patterns
- C++ helper pattern: Rust entry delegates to C++ via extern "C" FFI (Phase 15 Option A)
- Amalgamation compilation: duckdb.cpp provides all DuckDB C++ symbols — no manual stubs
- Feature gating: `#[cfg(feature = "extension")]` for extension-only code; `cargo test` stays pure Rust
- Panic safety: existing Rust FFI uses no catch_unwind yet — Phase 16 adds this (PARSE-05)

### Integration Points
- `cpp/src/shim.cpp`: sv_parse_stub needs to become a thin C++ trampoline that calls Rust extern "C" function
- `src/lib.rs` or new module: Rust parse function with case-insensitive detection, panic safety (catch_unwind), thread safety
- sv_plan_stub: may stay C++ for now (Phase 17 wires it to catalog) or also get a Rust trampoline

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 16-parser-hook-registration*
*Context gathered: 2026-03-07*
