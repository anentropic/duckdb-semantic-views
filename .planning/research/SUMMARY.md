# Project Research Summary

**Project:** DuckDB Semantic Views Extension
**Domain:** DuckDB extension (Rust + C++) — semantic layer / query expansion engine
**Researched:** 2026-02-28 / 2026-03-01
**Milestone:** v0.2.0 — Native DDL + Time Dimensions
**Confidence:** MEDIUM-HIGH (stack is well-confirmed; C++ parser hook internals require hands-on validation at implementation time)

---

## Executive Summary

v0.1.0 shipped a working semantic layer extension with a known architectural debt: all SQL-native features (native `CREATE SEMANTIC VIEW` DDL, `pragma_query_t` catalog persistence, EXPLAIN interception) were blocked by the DuckDB C API not exposing parser hooks to Rust. v0.2.0 resolves this by introducing a thin C++ shim — compiled via the `cc` build crate from a new `build.rs` — that registers parser extension hooks and PRAGMA callbacks with DuckDB's C++ SDK. All logic remains in Rust; the shim is strictly a DuckDB registration forwarding layer (~80 lines of C++).

The recommended build approach is Cargo-primary with `cc` crate (not CMake). This keeps existing CI, `just` tasks, and developer tooling unchanged while adding C++ compilation for the shim only. The only new Cargo dependency is `cc = "1.2"` in `[build-dependencies]`. DuckDB's C++ headers are vendored as `duckdb_capi/duckdb.hpp` (version-pinned to v1.4.4) to ensure reproducible extension builds. Time dimensions require no C++ work at all — they are pure Rust expansion engine changes using the existing `serde_json` dependency and DuckDB's built-in `date_trunc` SQL function.

The critical risk in v0.2.0 is the C++ shim integration: build system footprint (avoid CMake inversion), symbol visibility (Rust staticlib exports too many symbols without a linker version script), panic/exception safety across the FFI boundary, and `pragma_query_t` transaction semantics (in-memory catalog can diverge from persistent catalog after a rollback). These are all known pitfalls with clear mitigations, not open research questions. The implementation sequence must treat C++ infrastructure as a prerequisite, validate it independently, and layer features on top.

---

## Key Findings

### From STACK.md — Minimal, Well-Defined Additions

The v0.2.0 stack change is surgical. **Exactly one new Cargo dependency:** `cc = "1.2"` in `[build-dependencies]`. All existing dependencies (`duckdb = "=1.4.4"`, `libduckdb-sys = "=1.4.4"`, `serde`, `serde_json`, `proptest`, `arbitrary`) are sufficient for all other features. Key findings:

- **`cc` crate** (v1.2.45 current): Standard Rust ecosystem tool for C++ compilation via `build.rs`. Already used internally by `libduckdb-sys`. Build-time only — not in the compiled extension binary.
- **`duckdb.hpp` vendoring**: The C++ shim needs DuckDB's internal C++ headers. These are NOT available in extension mode builds (only in bundled/test mode). Vendor `duckdb.hpp` at `duckdb_capi/duckdb.hpp` from the pinned v1.4.4 release. One file, ~6MB, committed to the repo.
- **No `chrono`**: Time dimensions are SQL string codegen (`date_trunc('month', col)`) — Rust never does date arithmetic. `std::fmt` is sufficient.
- **No `cxx` crate**: The Rust/C++ boundary is a thin `extern "C"` surface (~5 functions). `cxx` is overkill for this; hand-written `extern "C"` is standard.
- **DuckDB `ParserExtension` API** is present in v1.4.4 (confirmed via GitHub issue #18485 and CIDR 2025 paper). Version pinning strategy is unchanged from v0.1.0.

### From FEATURES.md — Four New Features, Clear Build Order

v0.2.0 adds four features, all building on a common C++ shim prerequisite:

**Table Stakes (must have):**

| Feature | Complexity | Key Constraint |
|---------|-----------|----------------|
| `CREATE SEMANTIC VIEW` DDL | High | Requires C++ shim + `ParserExtension` registration |
| `DROP SEMANTIC VIEW` DDL | Low | Complement to CREATE; same parser hook |
| Time dimensions with granularity coarsening | Medium | Pure Rust; no C++ needed |
| `pragma_query_t` catalog persistence | High | Requires C++ shim; replaces sidecar file entirely |
| `EXPLAIN` showing expanded SQL | Medium | Use `parser_override` to rewrite to existing `explain_semantic_view`; true EXPLAIN interception is not feasible via stable API |

**Differentiators for v0.2.0:**
- Typed output columns (BIGINT/DOUBLE/DATE instead of all-VARCHAR) — Rust-only, not shim-dependent, but deferred to keep scope focused
- WEEK and QUARTER granularity — low-cost extension of time dimension enum

**Anti-features (explicitly deferred):**
- Community extension registry publication — deferred to v0.3.0 when DDL is stable and TPC-H demo exists
- YAML definition format — SQL DDL only; YAML adds no value
- Custom time spine table — `date_trunc` directly is sufficient
- Fiscal calendar / Sunday-start weeks — ISO 8601 only in v0.2.0 (document the convention)

**Critical path:** C++ shim → `pragma_query_t` → `CREATE SEMANTIC VIEW` parser hook → time dimension DDL syntax → granularity expansion → EXPLAIN rewrite

**Recommended build order:**
1. C++ shim infrastructure (build.rs + cc + duckdb.hpp + minimal shim.cpp compiles cleanly)
2. `pragma_query_t` catalog persistence (replace sidecar)
3. `CREATE SEMANTIC VIEW` parser hook (native DDL)
4. Time dimension DDL syntax + granularity expansion (pure Rust)
5. EXPLAIN `parser_override` rewrite
6. Sidecar removal + integration test update

### From ARCHITECTURE.md — Build Strategy and Component Map

The architecture decision is settled: **Cargo-primary with `cc` crate, not CMake**. Reasoning confirmed:

- Existing CI workflows, `just` task runner, and `cargo nextest` / `cargo-llvm-cov` tooling work unchanged
- The C++ shim is ~80 lines of registration + forwarding; the `cc` crate handles cross-compilation automatically
- CMake would require switching from `rust.Makefile` to `c_cpp.Makefile` and adding a `CMakeLists.txt` — disproportionate for a 50–100 line C++ file

**New and modified files:**

| Component | Status | Purpose |
|-----------|--------|---------|
| `src/shim/shim.cpp` | NEW | C++ registration: parser extension + pragma function |
| `src/shim/shim.h` | NEW | `extern "C"` boundary declarations |
| `src/shim/ffi.rs` | NEW | Rust implementations of `extern "C"` functions called by shim |
| `build.rs` | NEW | Compiles shim.cpp via `cc` crate (extension feature only) |
| `duckdb_capi/duckdb.hpp` | NEW | Vendored DuckDB C++ SDK header (version-pinned) |
| `Cargo.toml` | MODIFIED | Add `cc = "1.2"` to `[build-dependencies]` |
| `Justfile` | MODIFIED | Add `just update-headers` recipe |
| `src/model.rs` | MODIFIED | Add `TimeGrain` enum + `time_grain` field to `Dimension` |
| `src/expand.rs` | MODIFIED | `date_trunc` wrapping in SQL builder |
| `src/query/table_function.rs` | MODIFIED | Add `granularities` named parameter |
| `src/catalog.rs` | MODIFIED | Remove sidecar logic once `pragma_query_t` is active |
| `src/lib.rs` | MODIFIED | Call shim registration from `init_extension` |

**Key architectural constraints confirmed:**
- `plan_function_t` executes inside DuckDB's normal execution path — same lock constraints as v0.1.0's `invoke`. Use `pragma_query_t` (returns SQL string; DuckDB executes it post-lock) for all catalog writes.
- The C++ side holds only DuckDB C++ types and forwards all logic to Rust via `extern "C"`. No Rust→C++ catalog API calls.
- `build.rs` guards shim compilation behind `CARGO_FEATURE_EXTENSION` — `cargo test` does not compile C++.
- Backward compatibility: `#[serde(default)]` on `time_grain` field means existing definitions without it deserialize correctly.

### From PITFALLS.md — Six Risk Areas with Clear Mitigations

v0.2.0 introduces a qualitatively different risk profile from v0.1.0: the Rust+C++ FFI boundary is new territory. The pitfall research identified 15 concrete failure modes across 6 areas. Top pitfalls by impact:

**CRITICAL (blocks shipping if hit late):**

1. **P1.1 — Build system inversion**: Adding a C++ shim naively triggers a Cargo-primary → CMake-primary inversion that breaks footer injection. **Prevention:** Commit to `cc` crate / `build.rs` approach from the start; never introduce CMakeLists.txt.

2. **P1.3 — Panic/exception safety across FFI**: Rust panics crossing into C++ are undefined behavior. DuckDB C++ exceptions crossing into Rust frames can abort the process. **Prevention:** Every `extern "C"` function wraps the Rust body in `std::panic::catch_unwind`; every C++ callback wraps Rust calls in `try`/`catch`.

3. **P2.1 — Wrong parser hook chosen**: `CREATE` statements require `parser_override` (not the fallback `parse_function`), but `parser_override` fires for every query — the fast-exit path must be a case-insensitive prefix check returning "not handled" immediately for non-semantic-view statements.

4. **P3.2 — Transaction rollback divergence**: `pragma_query_t` SQL executes in DuckDB's normal transaction pipeline, so rollbacks undo the INSERT — but the in-memory Rust catalog may already have been updated. **Prevention:** Do not update in-memory catalog inside the PRAGMA callback; rebuild from persistent table on next read.

**MODERATE (correctness or platform-specific):**

5. **P1.2 — Rust staticlib symbol bloat**: Linking Rust staticlib into C++ shared library exports hundreds of `_ZN3std...` symbols. **Prevention:** Linker version script (Linux) / exported symbols list (macOS) restricting exports to exactly the three DuckDB entry points.

6. **P3.3 — SQL injection in pragma string**: JSON with single quotes breaks the `INSERT ... VALUES (...)` SQL string. **Prevention:** `sql_escape_string()` function doubling all single quotes before embedding in SQL literal.

7. **P5.1 — ISO week Monday boundary**: `date_trunc('week', ...)` is Monday-start (ISO 8601). US Sunday-start weeks are not supported in v0.2.0. **Prevention:** Document the convention; reject requests for FISCAL_WEEK.

8. **P5.2 — `date_trunc` on DATE returns TIMESTAMP**: Wrap with `CAST(... AS DATE)` when source column is `DATE` type to avoid `2024-01-01 00:00:00` vs `2024-01-01` string format mismatch in VARCHAR output.

9. **P4.1 — No stable EXPLAIN interception hook**: DuckDB's stable extension API has no EXPLAIN hook. **Prevention:** Use `parser_override` to detect `EXPLAIN ... semantic_query(...)` and rewrite to `explain_semantic_view()`. Document that this shows expansion, not DuckDB's physical plan.

**MINOR (one-time setup, low blast radius):**

10. **P6.1 — Memory ownership at FFI boundary**: Strings allocated by Rust must be freed by Rust; use `sv_free_str()` pattern. Never pass Rust-allocated `*mut c_char` to C++ for `free()`.

---

## Implications for Roadmap

### Suggested Phase Structure

v0.2.0 naturally decomposes into 5 phases ordered by dependency and risk:

**Phase 1: C++ Shim Infrastructure**
Rationale: All C++-dependent features are blocked until this is validated. Doing it first on its own (no parser logic yet) limits blast radius if build mechanics are wrong.
- Add `cc = "1.2"` to `[build-dependencies]`
- Download and vendor `duckdb.hpp` at `duckdb_capi/duckdb.hpp`
- Write `build.rs` guarded by `CARGO_FEATURE_EXTENSION`
- Write minimal `shim.cpp` that includes headers and compiles cleanly (no logic yet)
- Add linker version script for symbol visibility
- Establish `catch_unwind` / `try-catch` FFI boundary discipline in `src/shim/ffi.rs`
- Verify extension loads in DuckDB after C++ addition: `LOAD` smoke test passes
Pitfalls addressed: P1.1, P1.2, P1.3, P6.1
Research flag: **None** — patterns are confirmed; this is implementation work

**Phase 2: Time Dimensions (Pure Rust)**
Rationale: Independent of C++ shim; can proceed in parallel or immediately after Phase 1. Pure Rust, testable with `cargo test`. Delivers user-visible value before parser hook complexity.
- Add `TimeGrain` enum to `src/model.rs` with `#[serde(default)]` backward compat
- Add `date_trunc` wrapping in `build_sql()` for time dimension columns
- Add `granularities` named parameter to `semantic_query` VTab
- Validate: DATE source → CAST(date_trunc(...) AS DATE); TIMESTAMP source → TIMESTAMP output
- Explicitly reject TIMESTAMPTZ columns at definition time
- Document ISO week convention; add year-boundary edge case test
- Extend proptest coverage for time dimension expansion paths
Features delivered: TIME-1 through TIME-4 (day/week/month/quarter/year granularities)
Pitfalls addressed: P5.1, P5.2, P5.3, P5.4
Research flag: **None** — pure Rust SQL codegen; standard patterns

**Phase 3: `pragma_query_t` Catalog Persistence**
Rationale: Prerequisite for native DDL (Phase 4). Validates the persistence mechanism in isolation before adding parser complexity. Eliminates the sidecar file.
- Implement `sv_make_define_sql` and `sv_make_drop_sql` in `src/shim/ffi.rs` (builds SQL INSERT/DELETE strings with single-quote escaping)
- Implement `semantic_views_register_pragma` in `shim.cpp`
- Test: `PRAGMA define_semantic_view_internal(...)` → INSERT executed → definition survives restart
- Write rollback test: `BEGIN; PRAGMA ...; ROLLBACK;` → in-memory catalog reflects persistent table state
- Remove sidecar logic from `catalog.rs` once validated
Features delivered: `pragma_query_t` persistence replacing sidecar
Pitfalls addressed: P3.1, P3.2, P3.3
Research flag: **Moderate** — `pragma_query_t` + custom DDL (not just PRAGMA) integration path needs hands-on verification that `plan_function_t` can return SQL-for-execution rather than just a TableFunction. Validate against DuckDB 1.4.4 source during implementation.

**Phase 4: `CREATE SEMANTIC VIEW` Parser Hook**
Rationale: The flagship v0.2.0 feature; highest implementation complexity. Built on validated shim (Phase 1) and persistence (Phase 3).
- Implement `sv_parse_ddl` in Rust: parse DDL text into `SemanticViewDefinition` struct
- Implement `semantic_views_register_parser` in `shim.cpp`: register `parser_override` with fast-exit path (case-insensitive `CREATE SEMANTIC VIEW` prefix check)
- Implement `plan_function_t` to return table function that calls `sv_make_define_sql` and returns success message
- Implement `FALLBACK_OVERRIDE` semantics: pass through all non-semantic-view statements without error
- Support `CREATE OR REPLACE` and `IF NOT EXISTS` modifiers
- Update SQLLogicTest files for native DDL syntax
- Verify: function-based DDL (`define_semantic_view`) still works (backward compat)
Features delivered: `CREATE SEMANTIC VIEW`, `DROP SEMANTIC VIEW`
Pitfalls addressed: P2.1, P2.2, P2.3
Research flag: **High** — parser hook integration for a CREATE statement (not a PRAGMA) is the most under-documented pattern in DuckDB extension development. Validate `parse_function_t` + `plan_function_t` signature expectations against DuckDB 1.4.4 before writing implementation estimates.

**Phase 5: EXPLAIN Rewrite + Integration Hardening**
Rationale: Completes the v0.2.0 feature set. EXPLAIN interception via `parser_override` is a documentation-constrained feature (show expansion, not DuckDB plan). Integration hardening covers multi-platform CI and TPC-H demo.
- Add `parser_override` detection for `EXPLAIN ... semantic_query(...)` pattern
- Rewrite matching queries to `FROM explain_semantic_view(...)` — sugar over existing v0.1.0 VTab
- Add two-stage check: prefix `EXPLAIN` + contains `semantic_query(` substring
- Document clearly: output is expanded SQL, not DuckDB physical plan
- Add negative tests: `EXPLAIN SELECT 1` unchanged after extension load
- TPC-H demo notebook (real-world validation)
- Community extension registry research and CI prep (formal publication deferred to v0.3.0)
Features delivered: native-feeling EXPLAIN, TPC-H demo
Pitfalls addressed: P4.1, P4.2
Research flag: **Low** — EXPLAIN rewrite via parser_override is a documented pattern; no unknowns

### Deferred to v0.3.0+

- Typed output columns (BIGINT/DOUBLE/DATE instead of VARCHAR) — Rust-only but deferred to keep v0.2.0 scope focused
- Community extension registry publication — requires stable native DDL + TPC-H demo
- Fiscal calendar / Sunday-start week convention
- QUARTER granularity (trivial to add; deferred with WEEK for consistency)

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | One new dependency (`cc`), confirmed via crates.io + libduckdb-sys build output inspection. DuckDB ParserExtension API presence at v1.4.4 confirmed. |
| Features | HIGH | Four features explicitly listed in v0.1.0 TECH-DEBT.md with root-cause analysis complete. Behavior contracts are clear. Grammar design for DDL requires design work but is not a research gap. |
| Architecture | MEDIUM-HIGH | Build strategy (cc crate / build.rs) and Rust/C++ boundary design are confirmed. Specific C++ header include paths and `plan_function_t` return type constraints need hands-on validation at implementation time. |
| Pitfalls | HIGH | 15 concrete failure modes with prevention strategies and phase assignments. FFI boundary pitfalls (P1.3, P6.1) are HIGH-confidence based on Rust reference and RFC 2945. Parser hook pitfalls (P2.x) are MEDIUM-confidence; confirmed in DuckDB source/issues but exact signatures need verification. |

**Overall:** MEDIUM-HIGH. The v0.2.0 plan is well-researched and well-bounded. The main uncertainty is not "what to build" but "exact C++ API surface at implementation time." This is expected for DuckDB extension work and is mitigated by the phased approach (validate C++ infrastructure before adding parser logic).

### Gaps to Address During Planning

1. **`plan_function_t` return type for SQL-executing DDL**: Does returning a table function from `plan_function_t` correctly support the `pragma_query_t` SQL-return pattern, or is a PRAGMA the only route to executing SQL post-lock? Validate against DuckDB 1.4.4 `parser_extension.hpp` before writing Phase 4 implementation plan.

2. **DDL grammar design**: The exact `CREATE SEMANTIC VIEW` syntax (DIMENSIONS/METRICS/FILTERS keywords, JOIN declaration, TIME annotation) is not finalized. This is a design decision, not a research gap, but it must happen before Phase 4 implementation starts.

3. **`function-based DDL` deprecation path**: Should `define_semantic_view` / `drop_semantic_view` functions be deprecated in v0.2.0 or kept as compatibility aliases? Decision affects test migration scope.

4. **Symbol export verification CI step**: The linker version script approach (P1.2 mitigation) needs a CI step that runs `nm -D *.duckdb_extension` and asserts zero non-entry exported text symbols. Must be added before the first C++ shim build hits CI.

---

## Sources

### Primary (HIGH confidence)
- `cc` crate — crates.io v1.2.45 + docs.rs — standard Rust C++ compilation (confirmed current)
- Project `target/debug/build/libduckdb-sys-*/output` — confirmed `duckdb.hpp` in bundled mode, absent in extension mode (first-party)
- DuckDB GitHub issue #18485 — confirmed `DBConfig::GetConfig + config.parser_extensions.push_back()` at DuckDB 1.4.x
- `duckdb/pragma_function.hpp` (raw.githubusercontent.com) — confirmed `pragma_query_t` type signature
- `duckdb_extension.h` v1.4.4 — confirmed parser hooks NOT in `duckdb_extension_access` struct
- v0.1.0 TECH-DEBT.md — deferred requirements and root-cause analysis (first-party)

### Secondary (MEDIUM confidence)
- CIDR 2025 paper (Mühleisen & Raasveldt) — "Runtime-Extensible Parsers" — confirms `parse_function_t` / `plan_function_t` as production API
- DuckDB `parser_extension.hpp` (deepwiki summary) — `parse_function_t`, `plan_function_t`, `ParserExtensionPlanResult` structure
- DuckDB FTS extension source analysis — confirmed `pragma_query_t` SQL-return pattern
- Snowflake CREATE SEMANTIC VIEW docs — reference for DDL grammar design
- dbt MetricFlow time dimensions — reference for granularity coarsening design

### Tertiary (require implementation-time verification)
- Exact `plan_function_t` return type for SQL-executing DDL (vs. PRAGMA-only path)
- Specific `#include` paths within `duckdb.hpp` for `DBConfig` and `ParserExtension`
- DuckDB community extension registry current CI requirements (for v0.3.0)

---

*Research completed: 2026-03-01*
*Milestone: v0.2.0 — Native DDL + Time Dimensions*
*Ready for roadmap: yes*
