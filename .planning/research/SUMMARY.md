# Project Research Summary

**Project:** DuckDB Semantic Views Extension -- Parser Extension Spike
**Domain:** DuckDB Rust extension -- C++ parser hook integration for native SQL DDL syntax
**Researched:** 2026-03-07
**Milestone:** v0.5.0 -- Native `CREATE SEMANTIC VIEW` syntax via parser extension hooks
**Confidence:** MEDIUM (entry point strategy unproven; parser hook API surface verified)

---

## Executive Summary

Adding native `CREATE SEMANTIC VIEW` DDL syntax to this Rust DuckDB extension requires integrating C++ parser extension hooks (`parse_function_t` / `plan_function_t`) into the existing pure-Rust architecture. The DuckDB parser hook API is well-documented and stable at v1.4.1+, with proven implementations in prql and duckpgq extensions. The recommended spike approach is **statement rewriting**: the `parse_function` intercepts `CREATE SEMANTIC VIEW ...` statements that DuckDB's parser rejects at the `SEMANTIC` keyword, rewrites them into the existing `FROM create_semantic_view(...)` table function syntax, and the `plan_function` returns a `TableFunction` directly (Path A -- no stash/OperatorExtension pattern needed for DDL). This validates parser hooks end-to-end without building a custom SQL parser.

The single largest risk is the **entry point strategy** (P2/P4 in PITFALLS.md). The current extension uses a Rust C API entry point (`semantic_views_init_c_api` with `C_STRUCT_UNSTABLE` footer). Parser hooks require C++ API access (`DBConfig::GetConfig`), which means either switching to a CPP entry point or calling C++ from the existing Rust entry. Switching to CPP breaks Rust's `ffi::duckdb_*` function pointer initialization -- the `AtomicPtr` stubs populated by `duckdb_rs_extension_api_init` are never called, causing null pointer dereference on the first `ffi::duckdb_query` call. Two viable options exist: **(A)** keep `C_STRUCT` footer and Rust entry, call a C++ helper from Rust that receives the `duckdb_database` handle and registers parser hooks; or **(B)** switch to CPP footer and C++ entry, bridge the C API initialization by extracting `duckdb_extension_info`/`duckdb_extension_access` from `ExtensionLoader`. Both need a POC before committing. This is a go/no-go blocker for the milestone.

The build system changes are minimal: `cc = "1.2"` is already a build dependency, `duckdb.hpp` is already vendored at `duckdb_capi/`. The C++ shim is approximately 40-50 lines. All existing Rust code (catalog, DDL functions, query execution, zero-copy output) is unchanged. Both DDL interfaces (function-based and native SQL) coexist permanently -- parser hooks only fire for statements DuckDB's parser cannot handle.

---

## Key Findings

### Recommended Stack

No new runtime dependencies are needed for v0.5.0. The `cc` crate and vendored `duckdb.hpp` are already in place from prior milestones.

**Core technologies:**
- **`cc` crate (1.2, build-dep):** Compiles `shim.cpp` against DuckDB amalgamation -- already present
- **`duckdb.hpp` (vendored, v1.4.4):** Provides `ParserExtension`, `DBConfig`, `ExtensionLoader` C++ types -- already at `duckdb_capi/`
- **`build.rs`:** Feature-gated C++ compilation under `CARGO_FEATURE_EXTENSION` -- already exists, needs symbol visibility update
- **No new `[dependencies]`:** All Rust logic reuses existing crates (`duckdb`, `serde_json`, `strsim`)

### Expected Features

**Must have (table stakes -- v0.5.0 spike):**
- `CREATE SEMANTIC VIEW name (...)` via parser hook -- the entire point of this milestone
- C++ shim entry point or helper -- architectural foundation for parser hook registration
- Entry point conflict resolution (P2/P4) -- must be solved before anything else
- Existing function-based DDL (`FROM create_semantic_view(...)`) continues working unchanged

**Should have (same milestone if time permits):**
- `CREATE OR REPLACE SEMANTIC VIEW` -- standard SQL DDL pattern, same parse hook
- `DROP SEMANTIC VIEW [IF EXISTS]` -- symmetric with CREATE, same mechanism
- `CREATE SEMANTIC VIEW IF NOT EXISTS` -- standard SQL guard
- Semicolon normalization -- known DuckDB inconsistency (issue #18485)

**Defer (post v0.5.0):**
- `DESCRIBE SEMANTIC VIEW` / `SHOW SEMANTIC VIEWS` -- natural SQL syntax, lower priority
- `parser_override` hook -- wrong hook type; `parse_function` (fallback) is correct
- OperatorExtension / stash pattern -- unnecessary for DDL
- Custom `ParserExtensionInfo` with Rust catalog pointer
- Error location reporting (`error_location` field)
- Native query syntax changes

### Architecture Approach

The architecture adds a thin C++ layer (shim or helper function) for parser hook registration and FFI trampolines. All parsing logic, DDL execution, and catalog management remain in Rust. The `plan_function` uses Path A (direct `TableFunction` return), mapping parsed DDL to the existing `create_semantic_view` table function. The statement rewrite approach for the spike converts `CREATE SEMANTIC VIEW sales (tables := [...], ...)` into `FROM create_semantic_view('sales', tables := [...], ...)`, reusing 100% of existing DDL code without building a new parser.

**Major components:**
1. **`shim.cpp` (NEW or MODIFY, ~40-50 lines)** -- C++ entry point or helper function; parser extension registration; FFI trampolines calling Rust `sv_parse`/`sv_plan`
2. **`src/parser.rs` (NEW, ~100-200 lines)** -- Rust-side `sv_parse` (prefix detection + statement rewrite to existing function call) and `sv_plan` (returns table function + parameters)
3. **`build.rs` (MODIFY)** -- Update exported symbol visibility for new/changed entry point
4. **`src/lib.rs` (MODIFY)** -- Add `sv_init_rust` FFI function (Option B) or add C++ helper call after existing init (Option A)
5. **All existing components (UNCHANGED)** -- `catalog.rs`, `expand.rs`, `ddl/*`, `query/*`, `model.rs`

### Critical Pitfalls

All 14 pitfalls from PITFALLS.md, organized by severity:

**CRITICAL (blocks shipping if hit):**

1. **P2: Dual entry point conflict** -- DuckDB calls exactly one entry based on footer ABI type. If footer says CPP but Rust stubs are not initialized, SEGFAULT on first `ffi::duckdb_*` call. **Avoid:** Resolve via Option A (keep C_STRUCT, call C++ helper) or Option B (CPP entry, bridge C API init). POC required.

2. **P4: Rust function pointer table not initialized under CPP entry** -- The specific mechanism behind P2. `duckdb_rs_extension_api_init` populates `AtomicPtr` stubs; never called under CPP ABI path. **Avoid:** Same resolution as P2 -- this is the go/no-go blocker.

3. **P1: ODR violations (two copies of DuckDB statics)** -- Safe under `RTLD_LOCAL` isolation for data access through host references, but exceptions and `std::string` must NOT cross the boundary. **Avoid:** Keep C++ shim minimal, use `const char*` at FFI boundary, wrap all C++ in `try/catch`.

4. **P3: C++ ABI compiler mismatch** -- CPP ABI requires same compiler as host DuckDB. **Avoid:** Pin compiler in CI via `extension-ci-tools`; keep C++ surface to ~50 lines with only C types at boundaries.

5. **P5: Footer ABI type must match entry strategy** -- Wrong footer = wrong entry point called = parser hooks silently not registered. **Avoid:** Update Makefile flag if using CPP; no change if using Option A.

**MODERATE (correctness or platform-specific):**

6. **P6: Symbol visibility macOS vs Linux** -- `build.rs` must export correct entry symbol name with correct underscore convention per platform. **Avoid:** Update export lists, verify with `nm -gU` post-build.

7. **P7: Thread safety of parse_function** -- Called from any DuckDB parser thread. **Avoid:** Keep `sv_parse` stateless and reentrant; no shared mutable state.

8. **P8: Memory ownership across FFI** -- `CString` lifetimes, heap allocation in correct allocator. **Avoid:** `CString::into_raw` + explicit free; C++ trampoline copies to `std::string` immediately.

9. **P9: Semicolon inconsistency** -- Trailing `;` presence varies by interface. **Avoid:** Strip trailing semicolons before prefix matching.

10. **P10: Double parser hook registration on reload** -- Guard with `std::once_flag`. **Avoid:** Cheap insurance even if DuckDB prevents double-load.

11. **P11: Panic across FFI boundary** -- Undefined behavior under `extern "C"`. **Avoid:** Wrap all Rust FFI in `catch_unwind`; never `unwrap()` on untrusted input.

**MINOR (one-time setup):**

12. **P12: Stale cc crate build artifacts** -- Previously hit in this project. **Avoid:** Explicit `rerun-if-changed` directives in `build.rs`.

13. **P13: Amalgamation header version mismatch** -- Must match pinned DuckDB v1.4.4 exactly. **Avoid:** Already vendored at correct version; CI version monitor covers updates.

14. **P14: parse_function vs parser_override** -- Use `parse_function` (fallback), NOT `parser_override`. Zero overhead for normal SQL. **Avoid:** Register `parse_function` only; confirmed by prql pattern.

---

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Entry Point POC (Go/No-Go)

**Rationale:** P2/P4 are the highest-risk unknowns with LOW confidence. Everything else depends on knowing which entry strategy works. This must be resolved before any other work begins.
**Delivers:** A loadable extension that enters via the chosen strategy, registers a stub parser hook returning `DISPLAY_ORIGINAL_ERROR` for all queries, and successfully runs existing `semantic_view()` queries proving Rust FFI still works.
**Addresses:** Entry point resolution, ABI footer (if needed)
**Avoids:** P2 (dual entry conflict), P4 (null function pointers), P5 (wrong footer)
**Approach:** Try Option A first (simpler -- keep C_STRUCT, call C++ helper from Rust). If blocked (e.g., `duckdb_database` to `DatabaseInstance*` cast is not feasible), try Option B (CPP entry + bridge C API init from `ExtensionLoader`).

### Phase 2: Build System Hardening

**Rationale:** Once the entry strategy is proven, formalize the build: symbol visibility for correct entry point, `rerun-if-changed` directives, CI verification of exported symbols and footer ABI type.
**Delivers:** Clean reproducible build that compiles `shim.cpp`, exports correct symbols, stamps correct footer, passes `just test-all`.
**Addresses:** Build infrastructure
**Avoids:** P3 (compiler mismatch), P5 (footer), P6 (symbol visibility), P12 (stale artifacts), P13 (header version)

### Phase 3: Parse Function (CREATE SEMANTIC VIEW)

**Rationale:** With the shim proven and build stable, implement the actual parse hook. The statement rewrite approach keeps this phase focused on FFI mechanics and prefix matching, not SQL grammar design.
**Delivers:** `CREATE SEMANTIC VIEW name (tables := [...], dimensions := [...], metrics := [...])` works end-to-end via rewrite to `FROM create_semantic_view(...)`.
**Addresses:** Core DDL syntax (the milestone's primary deliverable)
**Avoids:** P7 (thread safety -- stateless parse function), P8 (memory ownership -- CString pattern), P9 (semicolons -- normalize input), P11 (panic safety -- catch_unwind), P14 (use parse_function not parser_override)

### Phase 4: Extended DDL Surface + Tests

**Rationale:** Once CREATE works, the same prefix-match + rewrite pattern extends mechanically to DROP, OR REPLACE, IF NOT EXISTS. Add sqllogictest coverage for all variants.
**Delivers:** Full DDL surface via native SQL syntax. Both interfaces (function-based + native DDL) tested for interoperability. Guard against double registration (P10).
**Addresses:** Remaining table stakes and differentiators
**Avoids:** P10 (double registration -- add `std::once_flag` guard)

### Phase Ordering Rationale

- **Phase 1 before everything:** The entry point strategy is a go/no-go decision. If neither Option A nor Option B works, the milestone must be re-scoped.
- **Phase 2 before 3:** Build system must be stable before writing parser logic. Stale artifact issues (P12) from prior milestones cost hours of debugging.
- **Phase 3 before 4:** Statement rewrite for CREATE validates the entire pipeline (parse -> plan -> table function -> catalog). Extending to DROP/DESCRIBE is mechanical once this works.
- **Phases 3 and 4 could merge** if Phase 1 resolves cleanly and the spike is straightforward.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1:** NEEDS RESEARCH -- `ExtensionLoader` API surface for extracting `duckdb_extension_info`/`duckdb_extension_access` (Option B). The `duckdb_database` to `DatabaseInstance*` cast (Option A). No Rust+C++ mixed DuckDB extension with parser hooks exists as prior art.

Phases with standard patterns (skip research-phase):
- **Phase 2:** Standard `cc` crate configuration and symbol visibility -- well-documented, partially implemented in this project already.
- **Phase 3:** FFI trampoline pattern is standard Rustonomicon material. Statement rewrite is string manipulation. Parser hook registration follows prql pattern.
- **Phase 4:** Mechanical extension of Phase 3 patterns to additional statement types. sqllogictest patterns established in the project.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | No new dependencies. `cc` crate and `duckdb.hpp` already in place. Parser hook API verified from v1.4.1 source. |
| Features | HIGH | API surface verified verbatim from DuckDB source. Statement types, prefix patterns, and parse/plan function signatures well-defined. Path A (direct TableFunction return) confirmed from `bind_extension.cpp`. |
| Architecture | HIGH for data flow, MEDIUM for entry point | Parse -> plan -> TableFunction -> catalog flow verified from source. Entry point bridge (C++ <-> Rust stub init) is uncharted -- no Rust+C++ mixed DuckDB extension exists. |
| Pitfalls | HIGH for 12 of 14, LOW for P2/P4 | Entry point strategy (P2/P4) is the critical unknown requiring POC. All other pitfalls have well-documented, high-confidence mitigations. |

**Overall confidence:** MEDIUM -- architecture and API are well-understood, but the entry point strategy (the foundation) has LOW confidence and requires a POC spike.

### Gaps to Address

- **`ExtensionLoader` C API handle access:** Does `ExtensionLoader` expose `GetExtensionInfo()` / `GetExtensionAccess()`? If not, Option B is blocked. Verify against DuckDB v1.4.4 source.
- **`duckdb_database` to `DatabaseInstance*` cast:** For Option A, the C++ helper receives a C API handle and must extract the C++ reference. Internal layout of DuckDB's C API wrapper must be verified. Fragile across versions.
- **`plan_function` TableFunction return for DDL:** Verified from `ParserExtensionPlanResult` header definition and `bind_extension.cpp` flow, but no DDL extension source code has been fully traced through this path. The duckpgq extension (CREATE PROPERTY GRAPH) is the closest precedent but was not source-verified.
- **Windows build:** Not a primary target. `cc` crate handles MSVC but `duckdb.hpp` may need specific flags. Defer to post-spike.

---

## Sources

### Primary (HIGH confidence)
- [DuckDB `parser_extension.hpp` (v1.4.1)](https://github.com/duckdb/duckdb/blob/v1.4.1/src/include/duckdb/parser/parser_extension.hpp) -- API types, function pointer typedefs, result types
- [DuckDB `bind_extension.cpp` (v1.4.1)](https://github.com/duckdb/duckdb/blob/v1.4.1/src/planner/binder/statement/bind_extension.cpp) -- plan_function -> BindTableFunction flow
- [DuckDB `parser.cpp` (main)](https://github.com/duckdb/duckdb/blob/main/src/parser/parser.cpp) -- fallback hook trigger flow, statement splitting
- [prql DuckDB extension](https://github.com/ywelsch/duckdb-prql) -- parser extension registration pattern, stash pattern reference
- [duckpgq extension](https://github.com/cwida/duckpgq-extension) -- DDL parser extension precedent (CREATE PROPERTY GRAPH)
- [cc crate (crates.io)](https://crates.io/crates/cc) -- C++ compilation from build.rs
- Project `_notes/parser-extension-investigation.md` -- prior art analysis
- Project `build.rs`, `src/lib.rs` -- current entry point and symbol visibility

### Secondary (MEDIUM confidence)
- [DuckDB PR #3783](https://github.com/duckdb/duckdb/pull/3783) -- RTLD_LOCAL extension isolation, static linking per-extension
- [DuckDB PR #12682](https://github.com/duckdb/duckdb/pull/12682) -- C API extensions, ABI types, entry point dispatch
- [DuckDB issue #18485](https://github.com/duckdb/duckdb/issues/18485) -- semicolon inconsistency in parser extension input
- [DuckDB extension-ci-tools](https://github.com/duckdb/extension-ci-tools/) -- footer stamping script, `--abi-type CPP` support
- [RBAC Extension RFC](https://gist.github.com/dufferzafar/f12081d4f32e640966d984b33e7077e6) -- plan_function returning TableFunction for DDL
- [DuckDB CIDR 2025 paper](https://duckdb.org/pdf/CIDR2025-muehleisen-raasveldt-extensible-parsers.pdf) -- parse_function vs parser_override semantics
- [Rust FFI (Rustonomicon)](https://doc.rust-lang.org/nomicon/ffi.html) -- thread safety, memory ownership, panic safety

### Tertiary (LOW confidence -- needs POC validation)
- `ExtensionLoader` C API handle extraction -- architecturally sound but unverified against v1.4.4 source
- `duckdb_database` to `DatabaseInstance*` cast -- inferred from C API wrapper patterns, fragile across versions

---
*Research completed: 2026-03-07*
*Milestone: v0.5.0 -- Parser Extension Spike*
*Ready for roadmap: yes (after Phase 1 POC validates entry point strategy)*
