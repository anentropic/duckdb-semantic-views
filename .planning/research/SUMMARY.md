# Project Research Summary

**Project:** DuckDB Semantic Views Extension
**Domain:** DuckDB Rust extension — DDL polish milestone (v0.5.1)
**Researched:** 2026-03-08
**Confidence:** HIGH (all features extend proven patterns; underlying functions already exist)

## Executive Summary

v0.5.1 is a targeted DDL polish milestone that completes the native SQL surface for semantic views. The v0.5.0 parser hook architecture established a proven rewrite-and-execute pattern for `CREATE SEMANTIC VIEW`; v0.5.1 extends that pattern to cover six additional DDL verbs (DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW, DROP IF EXISTS) plus improved error reporting. The work is almost entirely confined to `src/parse.rs` and `cpp/src/shim.cpp` — all underlying table function implementations already exist and are registered at init time.

The recommended approach is a three-wave delivery: (1) extend parser detection and rewriting to all seven statement types, (2) add error location reporting with clause-level hints and "did you mean" suggestions, (3) write README documentation. No new Cargo dependencies are needed. The only architectural complexity is DESCRIBE and SHOW, which return result sets rather than single-row side-effect confirmations — the cleanest solution is to generalize the C++ `sv_ddl_bind`/`sv_ddl_execute` functions to read output schema dynamically from the executed query result rather than hardcoding a single VARCHAR column.

The critical upfront risk is empirical: DuckDB's parser fallback only fires on parser errors, not catalog errors. `DROP SEMANTIC VIEW` will almost certainly trigger the fallback (DuckDB does not recognize SEMANTIC as a DROP object type), but `DESCRIBE SEMANTIC VIEW` and `SHOW SEMANTIC VIEWS` may produce catalog errors instead, bypassing the hook entirely. This must be validated with a 10-minute spike before any implementation begins. If DESCRIBE/SHOW cannot use the hook, they remain function-only in v0.5.1, which is acceptable — the function-based syntax already works.

## Key Findings

### Recommended Stack

The v0.5.0 Cargo.toml is unchanged for v0.5.1. The existing dependency set is fully sufficient: `duckdb =1.4.4` and `libduckdb-sys =1.4.4` for the VTab/FFI layer, `serde`/`serde_json` for catalog JSON, `strsim 0.11` for "did you mean" suggestions (already used in `expand.rs::suggest_closest()`), and `cc` for C++ shim compilation.

Three error formatting libraries were evaluated and rejected — `miette`, `ariadne`, and `codespan-reporting` are terminal diagnostic renderers that produce ANSI output incompatible with DuckDB's plain-text error channel. Hand-crafted `fmt::Display` following DuckDB's own `Hint:` / `Did you mean` conventions is the correct approach.

**Core technologies:**
- `duckdb =1.4.4` + `libduckdb-sys =1.4.4`: VTab, BindInfo, TableFunctionInfo, raw FFI — no version change
- `strsim 0.11`: Levenshtein-based "did you mean" — already used at query time; extend to DDL time
- `cc` (build-dep): C++ shim compilation — no change
- Hand-crafted `fmt::Display`: Error message formatting following DuckDB's plain-text conventions

### Expected Features

The seven DDL statements that complete the native syntax surface are all table stakes — any SQL dialect with CREATE also needs DROP, OR REPLACE, IF NOT EXISTS, DESCRIBE, and SHOW. All underlying function-based implementations exist; this milestone wires them to native syntax via the parser hook.

**Must have (table stakes):**
- `DROP SEMANTIC VIEW name` — every CREATE needs a DROP; `drop_semantic_view()` exists
- `DROP SEMANTIC VIEW IF EXISTS name` — standard DuckDB idempotent pattern; `drop_semantic_view_if_exists()` exists
- `CREATE OR REPLACE SEMANTIC VIEW name (...)` — standard update path; `create_or_replace_semantic_view()` exists
- `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` — migration safety; `create_semantic_view_if_not_exists()` exists
- `DESCRIBE SEMANTIC VIEW name` — inspection alongside creation; `describe_semantic_view()` exists
- `SHOW SEMANTIC VIEWS` — view discovery; `list_semantic_views()` exists
- README DDL syntax reference — users need to know the syntax

**Should have (differentiators):**
- Clause-level error hints ("Error in DIMENSIONS clause: ...") — high value, medium complexity
- "Did you mean" for DDL clause names — low complexity, reuses `suggest_closest()`
- "Did you mean" for view names in DROP/DESCRIBE — same pattern already at query time

**Defer (v2+):**
- `ALTER SEMANTIC VIEW` — not in Snowflake; CREATE OR REPLACE is the update path
- Schema-qualified names (`myschema.myview`) — requires architectural catalog changes
- `SHOW SEMANTIC VIEWS LIKE '%pattern%'` — client-side filtering suffices for now
- Row-per-field DESCRIBE format (Snowflake style) — adds a new VTab, defer
- `DESC` alias for DESCRIBE — minor convenience; add if users request it

**Anti-features confirmed (do not build):**
- `miette`/`ariadne`/`codespan-reporting` — terminal renderers, incompatible with DuckDB error channel
- Full SQL parser (sqlparser, nom, pest) — grammar is 7 prefix patterns; overkill
- `regex` for DDL detection — existing `eq_ignore_ascii_case` byte comparison is faster and allocation-free

### Architecture Approach

The v0.5.1 architecture extends the two-layer DDL pattern established in v0.5.0: Rust handles all parsing intelligence (detection, classification, rewriting, error reporting) and C++ handles execution and result forwarding via the dedicated `sv_ddl_conn`. No VTab implementations in `src/ddl/` change; the function layer is complete. The primary changes are `src/parse.rs` (add `StatementKind` enum, extend detection to 7 patterns, add per-type rewrite functions, add `ParseError` with position tracking) and `cpp/src/shim.cpp` (generalize `sv_ddl_bind` to read output schema dynamically from executed query result).

**Major components:**
1. `src/parse.rs` — MODIFY (major): `StatementKind` enum, detection for all 7 DDL kinds, per-type rewrite functions, `ParseError` with position and suggestion fields, updated FFI boundary
2. `cpp/src/shim.cpp` — MODIFY (moderate): generalize `sv_ddl_bind`/`sv_ddl_execute` to be schema-flexible; update `sv_parse_stub` to handle ternary return (not-ours / ours / ours-but-error)
3. `src/ddl/*.rs`, `src/catalog.rs`, `src/expand.rs`, `src/query/*` — NO CHANGE: function layer and query pipeline are untouched

**Key patterns:**
- Longest-prefix-first detection order to avoid false matches (CREATE OR REPLACE before CREATE, DROP IF EXISTS before DROP)
- All statement types rewrite to `SELECT * FROM <existing_function>(...)` — no new VTab implementations
- C++ table function generalizes to execute any rewritten SQL on `sv_ddl_conn` and return the native result schema
- Error reporting: Rust catches DuckDB errors from rewritten SQL execution and re-maps positions to the original DDL context before returning to C++

### Critical Pitfalls

1. **DROP/DESCRIBE/SHOW may not trigger the parser fallback hook (P1, MEDIUM confidence)** — DuckDB's `parse_function` is a fallback called only on parser errors. DESCRIBE/SHOW may parse successfully (treating SEMANTIC as an identifier) and fail at the catalog layer instead, bypassing the hook. Validate empirically before implementing: run each statement prefix against DuckDB with the extension loaded and observe the error type. If DESCRIBE/SHOW produce catalog errors, keep them function-only for v0.5.1.

2. **Three-connection lock conflict during DROP (P3, MEDIUM confidence)** — DROP via the parser hook path involves three connections: main, `sv_ddl_conn` (parser DDL connection), and `persist_conn` (catalog persistence connection). DuckDB's single-writer model may cause a lock conflict when `sv_execute_ddl_rust` executes DROP on `sv_ddl_conn` which then tries to write via `persist_conn`. Test the native DDL DROP path early; if it deadlocks, consolidate writes onto `sv_ddl_conn`.

3. **Error positions meaningless after statement rewriting (P6, HIGH confidence)** — DuckDB error positions refer to the rewritten `SELECT * FROM create_semantic_view(...)` SQL, not the original DDL. The `SELECT * FROM create_semantic_view('name', ` prefix shifts all positions by a variable amount. Do not pass through DuckDB's raw character positions. Catch errors from rewritten SQL execution, extract the parameter name from the DuckDB error message, find it in the original DDL string, and report that position.

4. **Prefix detection ambiguity without longest-match ordering (P4, HIGH confidence)** — `CREATE OR REPLACE SEMANTIC VIEW` shares the `CREATE SEMANTIC VIEW` prefix. Checking shorter prefixes first extracts the view name from the wrong position. Detection must check in longest-first order: CREATE OR REPLACE (36 chars) -> CREATE IF NOT EXISTS (35 chars) -> CREATE (20 chars) -> DROP IF EXISTS (28 chars) -> DROP (19 chars) -> DESCRIBE (22 chars) -> SHOW (19 chars).

5. **FFI return code insufficient for DESCRIBE/SHOW output schemas (P10, MEDIUM confidence)** — `sv_ddl_bind` currently hardcodes a single VARCHAR output column. DESCRIBE returns 6 columns; SHOW returns 2 columns with N rows. Either generalize the C++ table function to read output schema from the executed result dynamically, or keep DESCRIBE/SHOW as function-only for v0.5.1 to avoid this complexity.

## Implications for Roadmap

Based on research, suggested four-phase structure:

### Phase 1: Parser Hook Validation Spike
**Rationale:** P1 is a binary scope question — if DESCRIBE/SHOW cannot use the parser hook, the implementation scope changes before any code is written. This requires no code, takes 10-30 minutes, and must come first.
**Delivers:** Confirmed list of which DDL statements can use native syntax in v0.5.1
**Addresses:** P1 (DESCRIBE/SHOW fallback behavior)
**Avoids:** Implementing DESCRIBE/SHOW native syntax only to discover it does not work at integration time

### Phase 2: Extended DDL Detection and Routing
**Rationale:** All function-based backends exist; the work is entirely in the parser detection and rewrite layer. Splitting into a pure-Rust sub-wave (1a) before C++ integration (1b) gives fast feedback via `cargo test` before requiring `just build` + `just test-sql`.
**Delivers:** Native `DROP`, `CREATE OR REPLACE`, `CREATE IF NOT EXISTS` syntax; DESCRIBE/SHOW native syntax if Phase 1 confirmed they work
**Uses:** `strsim 0.11` (existing), `StatementKind` enum in `src/parse.rs`, longest-prefix-first detection
**Implements:** Extended `detect_statement()` + per-type `rewrite_to_function_call()` in `parse.rs`; generalized C++ `sv_ddl_bind`
**Avoids:** P4 (prefix ambiguity — longest-match order), P3 (three-connection lock — test DROP early), P10 (output schema — generalize bind or scope-cut DESCRIBE/SHOW)
**Sub-waves:** 1a: Pure Rust detection + rewrite (`cargo test`); 1b: FFI + C++ integration (`just test-sql`)

### Phase 3: Error Location Reporting
**Rationale:** Error reporting is independent of the DDL verbs once the parse layer is in place. Depends on Phase 2's `ParseError` struct and position-tracking infrastructure being complete.
**Delivers:** Clause-level error hints, "did you mean" for DDL clause names and view names, character-position error reporting
**Uses:** `strsim 0.11` (existing `suggest_closest()`), `ParseError` struct from Phase 2
**Avoids:** P6 (error position mapping — re-map to original DDL), P7 (buffer truncation — increase to 4096 or dynamic allocation), P11 (wrong suggestion context — parameterize by vocabulary)

### Phase 4: Documentation
**Rationale:** Must come last; depends on all DDL verbs and error reporting being confirmed correct.
**Delivers:** README DDL syntax reference, worked examples covering create/query/describe/drop lifecycle
**Addresses:** P8 (document DROP concurrency behavior), P5 (document canonical syntax forms, mutual exclusion of OR REPLACE and IF NOT EXISTS)

### Phase Ordering Rationale

- Phase 1 precedes all implementation because it determines scope (P1 is a binary blocker for DESCRIBE/SHOW native syntax)
- Phase 2 split into pure-Rust before C++ integration maximizes feedback speed; `cargo test` is fast; `just build` + `just test-sql` is slower
- Phase 3 depends on Phase 2's `ParseError` infrastructure but is otherwise independent
- Phase 4 is documentation-only and follows confirmed behavior from Phases 1-3

### Research Flags

Phases likely needing validation during planning:
- **Phase 1 (parser hook validation):** Pure empirical test — run each statement prefix against DuckDB with extension loaded; observe whether error is Parser Error or Catalog Error
- **Phase 2, sub-wave 1b (C++ generalization):** The generalized `sv_ddl_bind` reading dynamic schema from `duckdb_result` uses C API column introspection not yet exercised in the codebase; verify `duckdb_column_type` -> `LogicalType` mapping and that these C API functions work through loadable-extension stubs

Phases with standard patterns (can proceed without additional research):
- **Phase 2, sub-wave 1a (Rust parse layer):** Pure prefix matching and string rewriting — well-understood, no external unknowns, full unit test coverage possible
- **Phase 3 (error reporting):** All components exist (`strsim`, `suggest_closest`, error type patterns from `ExpandError`); extension of established patterns
- **Phase 4 (documentation):** Markdown only

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Zero new dependencies confirmed; all existing crates sufficient; miette/ariadne explicitly evaluated and rejected |
| Features | HIGH | All 7 function-based backends exist and registered; DuckDB and Snowflake DDL semantics verified from official docs |
| Architecture | HIGH | Two-layer parser hook pattern proven in v0.5.0; all changes extend existing patterns; C++ generalization approach is concrete and well-specified in ARCHITECTURE.md |
| Pitfalls | MEDIUM | P1 (DESCRIBE/SHOW hook behavior) and P3 (three-connection locking) are empirically unconfirmed; all other pitfalls are HIGH confidence with clear mitigations |

**Overall confidence:** HIGH

### Gaps to Address

- **P1 (DESCRIBE/SHOW parser hook behavior):** Must be validated empirically before Phase 2 begins. If catalog errors bypass the hook, scope narrows to CREATE/DROP variants only; DESCRIBE/SHOW remain function-only for v0.5.1.
- **P3 (three-connection lock during DROP):** Test `DROP SEMANTIC VIEW` via native DDL path early in Phase 2 integration. Lock conflict is MEDIUM confidence (not confirmed); if it occurs, consolidate writes onto `sv_ddl_conn` and bypass `persist_conn`.
- **C++ `duckdb_column_type` -> `LogicalType` mapping:** The C API provides `duckdb_type` enum values; mapping to `LogicalType` for the generalized bind function needs verification against the DuckDB C API reference before Phase 2b.
- **`sv_rewrite_rust` buffer sizing:** The rewritten SQL string may be longer than the original DDL. The output buffer for `sv_rewrite_rust` needs to be sized appropriately; consider dynamic allocation or a generous fixed size (e.g., 8192 bytes) to avoid truncating long view bodies.

## Sources

### Primary (HIGH confidence)
- DuckDB official documentation: CREATE VIEW, DROP, DESCRIBE, SHOW TABLES — DDL behavioral semantics
- Snowflake semantic view documentation: CREATE/DROP/DESCRIBE/SHOW SEMANTIC VIEWS — reference model
- Project source code (`src/parse.rs`, `cpp/src/shim.cpp`, `src/ddl/*.rs`, `src/lib.rs`, `src/expand.rs`, `src/query/error.rs`) — direct code inspection
- Project `Cargo.toml` — dependency versions confirmed
- [strsim 0.11.1 — crates.io](https://crates.io/crates/strsim) — latest version confirmed
- [DuckDB Runtime-Extensible Parsers (blog)](https://duckdb.org/2024/11/22/runtime-extensible-parsers) — parser hook fallback mechanism
- [DuckDB parser_extension.hpp](https://raw.githubusercontent.com/duckdb/duckdb/main/src/include/duckdb/parser/parser_extension.hpp) — `error_location` field, `DISPLAY_EXTENSION_ERROR` result type
- [DuckDB structured errors — GitHub issue #13782](https://github.com/duckdb/duckdb/issues/13782) — confirms DuckDB errors are plain-text strings
- [DuckDB "Did you mean" format — GitHub issue #16829](https://github.com/duckdb/duckdb/issues/16829) — confirms `Did you mean` and `LINE 1:` / caret format

### Secondary (MEDIUM confidence)
- [DuckPGQ extension](https://github.com/cwida/duckpgq-extension) — existence proof for multi-DDL-type parser extensions (CREATE/DROP PROPERTY GRAPH)
- [DuckDB issue #18485](https://github.com/duckdb/duckdb/issues/18485) — semicolon inconsistency in parser extensions (already handled in v0.5.0)
- [DuckDB Runtime-Extensible Parsers (CIDR 2025 paper)](https://duckdb.org/pdf/CIDR2025-muehleisen-raasveldt-extensible-parsers.pdf) — extension parsers replace full grammar, not extend it

### Tertiary (LOW confidence — needs empirical validation)
- DuckDB parser fallback behavior for DROP/DESCRIBE/SHOW: documented as fallback-on-parse-failure; actual behavior for these specific prefixes unconfirmed and requires empirical test

---
*Research completed: 2026-03-08*
*Ready for roadmap: yes*
