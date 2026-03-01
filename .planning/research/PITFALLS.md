# PITFALLS — DuckDB Semantic Views Extension v0.2.0

**Research type:** Project Research — Pitfalls dimension
**Milestone context:** Subsequent milestone — adding C++ shim, time dimensions, EXPLAIN hook, and pragma_query_t catalog persistence to an existing Rust DuckDB extension.
**Date:** 2026-02-28

---

## Purpose

This document catalogs concrete mistakes, gotchas, and failure modes specific to the v0.2.0 additions:

1. Adding a C++ shim to an existing Rust DuckDB extension (build system integration, symbol visibility, ABI)
2. Implementing DuckDB parser hooks for native `CREATE SEMANTIC VIEW` DDL
3. Using `pragma_query_t`-pattern callbacks for catalog persistence
4. Implementing an EXPLAIN interception hook
5. Adding time dimension support with granularity coarsening in the SQL expansion engine
6. C++/Rust FFI at the DuckDB extension boundary (memory ownership, panics, exception safety)

**Known pitfalls already addressed in v0.1.0 research are not repeated here.** Specifically excluded: ABI instability across DuckDB minor versions, duckdb-rs not exposing parser hooks, SQL execution deadlock in scalar `invoke`, PRAGMA database_list Python naming, and duckdb/loadable-extension stub replacement.

Each pitfall includes: what goes wrong, warning signs, prevention strategy, and which v0.2.0 phase should address it.

---

## Part 1 — Adding a C++ Shim to an Existing Rust Extension

### P1.1 — Build system inversion: the project must switch from Cargo-primary to CMake-primary

**What goes wrong:**
The v0.1.0 extension uses `extension-template-rs`, a Cargo-primary build that delegates packaging to a Python script. When you add a C++ shim, the build must instead follow the C++ `extension-template` model: CMake drives the top-level build, invokes Cargo as a CMake custom target to produce a Rust static library (`libsemantic_views.a`), and the CMake linker then links the C++ shim object files and the Rust staticlib together into the final `.duckdb_extension` shared library.

The inversion is not cosmetic. The Cargo-primary model's `build.rs` and post-build Python script assume Cargo controls symbol export and footer injection. The CMake-primary model's footer-injection script assumes CMake controls the final link step. Running both in sequence produces a binary with two incompatible footer stamps, or one footer is silently overwritten.

**Warning signs:**
- The extension loads under `LOAD '/path/to/ext'` but fails under `INSTALL; LOAD` (the registry path enforces the official footer format).
- The `.duckdb_extension` file size doubles or triples — a sign the footer was appended twice.
- `nm -D` on the final shared library shows both the Rust entry symbol (`semantic_views_init_c_api`) and unexpected Rust std symbols with `pub` visibility — a sign the C++ link step did not override Rust symbol export defaults.

**Prevention strategy:**
- Commit to CMake-primary early. The C++ `extension-template` repository is the reference. Delete the Cargo-primary Python script and `cargo-duckdb-ext-tools` configuration from v0.1.0 before starting C++ shim work.
- In `CMakeLists.txt`, add a `ExternalProject_Add` or `add_custom_command` that runs `cargo build --release --lib --features extension` and produces `target/release/libsemantic_views.a`. Link this as a static library in the CMake target.
- Use only one footer-injection path. Let the CMake extension template's `duckdb_extension_load` macro handle the footer. Remove or disable any Rust-side post-build hooks.

**Phase assignment:** Resolve in the C++ shim scaffold phase (first v0.2.0 phase), before any parser hook implementation.

---

### P1.2 — Rust staticlib exports all std symbols into the shared library

**What goes wrong:**
When `libsemantic_views.a` (a Rust staticlib) is linked into the C++ shared library, the Rust compiler marks all `#[no_mangle]` and `pub extern "C"` symbols as globally visible ELF symbols. But it also exports the full Rust standard library symbol table into the shared object's PLT. The result: the `.duckdb_extension` exports hundreds of `_ZN3std...` symbols, bloating the shared library, confusing the dynamic linker, and potentially clashing with symbols in the DuckDB runtime itself if it also embeds Rust.

A subtle version: if DuckDB's own binary embeds Rust (future versions might), symbol collisions can cause the wrong function to be called, producing silent data corruption or crashes.

**Warning signs:**
- `nm -D libsemantic_views.duckdb_extension | wc -l` returns thousands of symbols where a C++ extension returns hundreds.
- The linker on Linux emits "multiple definition" warnings for standard library symbols during the CMake link step.
- The binary is 5-10x larger than a comparable C++ extension.

**Prevention strategy:**
- Add a version script (`-Wl,--version-script`) on Linux that restricts exported symbols to exactly the three DuckDB-required entry points: `<extname>_init_c_api`, `<extname>_version`, and `<extname>_storage_init` (if used). All other symbols are marked `local`.
- On macOS, use `-exported_symbols_list` with the same set of three symbols.
- Add a `build.rs` step that generates the version script automatically from the extension name, so it is not maintained manually.
- Verify after build: `nm -D *.duckdb_extension | grep -v 'semantic_views_' | grep ' T '` should return zero lines on Linux (all non-entry exported text symbols suppressed).
- Confidence: MEDIUM (Rust staticlib symbol bloat is a documented Rust issue; the DuckDB-specific version script pattern is inferred from C++ extension practice).

**Phase assignment:** C++ shim scaffold phase. The version script must be in place before the first CI build.

---

### P1.3 — The Rust and C++ runtimes both want to handle thread-local storage (TLS) and unwinding

**What goes wrong:**
On Linux, both the Rust runtime and the C++ runtime independently manage thread-local storage and stack unwinding. When the Rust staticlib and the C++ shim code are linked into the same shared library, the two runtimes may initialize conflicting TLS implementations. This is typically silent on macOS (where TLS and unwinding are handled by the OS) but manifests on Linux glibc as a crash in `__tls_get_addr` or an abort in `__cxa_throw` when a C++ exception propagates past Rust frames (or vice versa).

The specific DuckDB scenario: the C++ shim registers a parser callback. DuckDB's parser throws a C++ exception on a parse error. If the exception unwinds through a Rust stack frame in the shim (because the Rust `#[no_mangle]` function is in the call stack), the Rust runtime will see a "foreign exception" — behavior is currently defined as either aborting the process or returning an opaque error from `catch_unwind`.

**Warning signs:**
- Intermittent crashes on Linux that do not reproduce on macOS.
- Crash in `libstdc++` or `libunwind` rather than in extension code.
- The `backtrace` shows frames alternating between Rust and C++ in the shim layer.
- `SEGFAULT` or `SIGABRT` on the first `CREATE SEMANTIC VIEW` statement that triggers a parse error.

**Prevention strategy:**
- Treat every `extern "C"` function called from C++ as a hard boundary: wrap the Rust code body in `std::panic::catch_unwind`. Convert the caught panic to a DuckDB error string via `set_error`; never let a Rust panic cross into C++.
- Treat every C++ callback that calls into Rust as a C++ `try`/`catch` boundary: catch all C++ exceptions before they reach a Rust stack frame, convert them to a Rust-compatible error, and return.
- Do not use `extern "C-unwind"` ABI (RFC 2945) for DuckDB callback functions unless DuckDB's own extension shim examples explicitly use it. Default to `extern "C"` with explicit catch-at-boundary discipline.
- Confidence: HIGH (Rust reference, RFC 2945, Rustonomicon all confirm this; panic-unwind across FFI is undefined behavior under `extern "C"`).

**Phase assignment:** C++ shim scaffold phase. Establish the catch-at-boundary pattern before writing any parser logic.

---

## Part 2 — Parser Hook Implementation

### P2.1 — `parse_function` vs `parser_override`: wrong hook chosen for `CREATE` statements

**What goes wrong:**
DuckDB exposes two parser extension hook points:

- `parser_override`: Called before DuckDB's own parser attempts to parse the statement. If the override returns a result, DuckDB skips its own parser entirely.
- `parse_function` (fallback): Called only when DuckDB's parser fails to parse a statement.

`CREATE SEMANTIC VIEW` starts with `CREATE`, which DuckDB's parser parses successfully — it just doesn't know about `SEMANTIC VIEW` and produces an error at the second token. The `parse_function` fallback is triggered. But the `parse_function` receives the raw query string for that statement, not a pre-split individual statement. The query string may include a trailing semicolon on some interfaces (CLI) but not others (DuckDB UI, Python), and this is a documented inconsistency (GitHub issue #18485, labeled "under review" as of August 2025).

**What goes wrong concretely:** The extension pattern-matches the query string for `CREATE SEMANTIC VIEW` using a regex or prefix check. The match fails if the string has a trailing semicolon on one interface but not another, producing a confusing "parse error" that is interface-dependent.

**Warning signs:**
- `CREATE SEMANTIC VIEW my_view (...)` works from Python but fails from the DuckDB CLI.
- The parser hook fires in one test environment but not another.
- The hook fires with different query strings for the same SQL depending on whether the query ends with `;`.

**Prevention strategy:**
- In the parse function, normalize the query string: trim leading/trailing whitespace and trailing semicolons before pattern matching.
- Use `parser_override` rather than the fallback `parse_function` for `CREATE SEMANTIC VIEW`. `parser_override` is called before DuckDB's parser, giving the extension full control. The tradeoff: `parser_override` is called for every query, not just failed ones — write the fast-exit path (check if the trimmed query starts with `CREATE SEMANTIC VIEW` case-insensitively) as the first operation.
- Write integration tests that exercise the hook from: Python `duckdb.connect().execute(...)`, CLI `duckdb -c "..."`, and the DuckDB test runner (`sqllogictest`). All three send queries differently.
- Confidence: MEDIUM (semicolon inconsistency confirmed in GitHub issue #18485; parser_override vs parse_function choice is based on DuckDB parser source code analysis and CIDR 2025 paper).

**Phase assignment:** Parser hook implementation phase.

---

### P2.2 — `plan_function` must return a `TableFunction` result — not a plan node

**What goes wrong:**
Once the `parse_function` produces a custom `ExtensionStatement` for `CREATE SEMANTIC VIEW`, DuckDB calls `plan_function` to convert that statement into an executable plan. The `plan_function` API does not accept arbitrary logical operators — it must return a `TableFunction` result (similar to how `PRAGMA` statements return table-valued results). Many extension authors expect to return a `CreateStatement`-style catalog plan and encounter undocumented type errors when DuckDB's planner rejects the result.

The canonical pattern (used by all documented community extension custom DDL): `plan_function` returns a table function that, when executed, performs the DDL side effects (inserting into the catalog) and returns a success/message result set (or an empty result). The DDL effect happens during query execution, not during planning — this means the `CREATE SEMANTIC VIEW` is not transactional in the same way as a DuckDB `CREATE TABLE`.

**Warning signs:**
- The `plan_function` compiles but DuckDB panics or returns an internal error at planning time.
- `CREATE SEMANTIC VIEW` appears to succeed (no error) but the view is not visible in `list_semantic_views()`.
- Attempting to return a logical plan node from `plan_function` produces a static assertion failure in the C++ shim at compile time.

**Prevention strategy:**
- Model `plan_function` exactly after the FTS extension's PRAGMA pattern: return a table function call that performs the DDL mutation when executed. The table function can call back into the Rust `define_semantic_view` logic (the same logic already used in v0.1.0), now triggered from the native DDL path rather than the scalar function path.
- The table function result should return a single VARCHAR column `"message"` with a success message (e.g., `"Semantic view 'name' created"`). This is the simplest valid result type.
- Write a test that checks: `CREATE SEMANTIC VIEW` succeeds, `list_semantic_views()` shows the new view, `DROP SEMANTIC VIEW` removes it, `list_semantic_views()` no longer shows it — all in the same test session.
- Confidence: MEDIUM (based on FTS extension pattern and DuckDB parser source; plan_function API behavior inferred from parser.cpp analysis).

**Phase assignment:** Parser hook implementation phase.

---

### P2.3 — Parser hook registered globally — affects all connections and all databases

**What goes wrong:**
DuckDB parser extensions are registered at the database level, not per-connection. When the extension is loaded, the parser hook is registered for all queries on all connections to that database. If another extension also registers a `parser_override`, the override functions are called sequentially in registration order. If the semantic views extension's parser override throws a C++ exception (or invokes `set_error`) for a statement it doesn't recognize, it can incorrectly reject queries that the next override or DuckDB's own parser would have handled.

Additionally, DuckDB's extension system uses `STRICT_OVERRIDE` and `FALLBACK_OVERRIDE` modes. Using `STRICT_OVERRIDE` means the extension's parser failure is final — DuckDB will not try its own parser as a fallback. Using `FALLBACK_OVERRIDE` (the correct choice) means DuckDB falls through to the next parser extension or its own parser if the extension doesn't handle the statement.

**Warning signs:**
- Standard DuckDB SQL statements fail to execute after loading the semantic views extension.
- `SELECT 1` or `PRAGMA database_list` throws a "parse error" that did not occur before loading the extension.
- The extension's `parser_override` returns an error for unknown statements instead of returning a "not handled" signal.

**Prevention strategy:**
- Use `FALLBACK_OVERRIDE` mode. Return "not handled" for any statement that does not start with `CREATE SEMANTIC VIEW` (after normalization). Do not return an error — return the correct "pass through" enum value.
- The fast-exit path must be unconditional: if the trimmed statement does not start with `CREATE SEMANTIC VIEW` (case-insensitive), return immediately without inspecting the rest.
- Do not call `set_error` from within the `parser_override` for unrecognized statements. Only call it for statements that are recognized as `CREATE SEMANTIC VIEW` but are syntactically invalid.
- Test by loading both the semantic views extension and another community extension in the same session and verifying both function correctly.
- Confidence: MEDIUM (based on DuckDB parser source code analysis showing sequential extension evaluation and early-exit on success).

**Phase assignment:** Parser hook implementation phase.

---

## Part 3 — pragma_query_t Catalog Persistence

### P3.1 — `pragma_query_t` returns SQL that DuckDB executes — not a result set

**What goes wrong:**
The `pragma_query_t` pattern (used by the FTS extension) is a PRAGMA function type that returns a `string` — a SQL query string that DuckDB then executes. This is how FTS's `create_fts_index` triggers the creation of a schema full of tables: the PRAGMA callback constructs a multi-statement SQL string (creating tables, inserting data, defining macros) and returns it; DuckDB executes it.

Extension authors unfamiliar with this pattern attempt to implement a standard `pragma_function_t` (which returns a result table directly) and cannot figure out why the catalog changes they make inside the callback don't persist — because `pragma_function_t` runs inside the execution context where SQL is blocked (the v0.1.0 deadlock problem).

`pragma_query_t` works because the SQL is returned and executed after the callback returns, before execution locks are held. But this means the SQL must be completely constructed in the callback — it cannot depend on query results from within the callback itself.

**Warning signs:**
- The PRAGMA callback appears to succeed but the catalog table (`semantic_layer._definitions`) is not updated.
- Attempting to run DuckDB SQL from within the `pragma_query_t` callback causes the same deadlock as v0.1.0's scalar invoke.
- The returned SQL string is not valid at the time of construction (e.g., it references tables that don't exist yet).

**Prevention strategy:**
- Return a complete, self-contained SQL string from the `pragma_query_t` callback. For `CREATE SEMANTIC VIEW`, this is an `INSERT INTO semantic_layer._definitions VALUES (...)` statement where the definition JSON is embedded as a string literal in the SQL.
- The JSON must be serialized to a SQL string literal with all single quotes escaped (or use `$$`-style dollar quoting if DuckDB supports it — verify; DuckDB does not support PostgreSQL-style dollar quoting as of 1.4.x).
- Test the persistence round-trip explicitly: call the PRAGMA, close the connection, reopen the database, verify the definition is present in `semantic_layer._definitions`.
- Confidence: MEDIUM (FTS extension uses this pattern; the execution-after-return mechanism is inferred from the FTS implementation analysis and the v0.1.0 deadlock root cause).

**Phase assignment:** pragma_query_t implementation phase.

---

### P3.2 — Transaction rollback does not undo `pragma_query_t` side effects

**What goes wrong:**
When DuckDB executes the SQL returned by a `pragma_query_t` callback, that SQL runs as a DuckDB query. If the surrounding user transaction is rolled back, the `INSERT INTO semantic_layer._definitions` is rolled back too (correct). However, the PRAGMA callback itself — which may have already updated the in-memory Rust catalog — is not rolled back. The result: after a rollback, the in-memory catalog says the semantic view exists but the persistent catalog table says it does not.

On the next extension load, `init_catalog` reads the persistent table and the in-memory catalog is rebuilt correctly — but within the current session, the discrepancy causes `semantic_query` to find the view in memory while `list_semantic_views` (which reads the persistent table) shows it as absent.

**Warning signs:**
- `BEGIN; CREATE SEMANTIC VIEW x ...; ROLLBACK; SELECT * FROM list_semantic_views()` shows view `x` when it should not.
- `BEGIN; CREATE SEMANTIC VIEW x ...; ROLLBACK; FROM semantic_query('x', ...)` succeeds when it should fail with "view not found".
- Tests that test rollback behavior pass on fresh sessions but fail after prior aborted transactions.

**Prevention strategy:**
- Update the in-memory Rust catalog only after the `pragma_query_t` SQL has been committed, not during the callback. This means: do not add to the in-memory catalog inside the PRAGMA callback. Instead, add a post-commit hook, or accept eventual consistency: the in-memory catalog is rebuilt from the persistent table on the next read if a discrepancy is detected.
- The simplest approach: the in-memory catalog is a cache, always authoritative from the persistent table. Before any operation that reads the catalog, check if the persistent table version matches the in-memory version (a sequence counter or row count). If not, rebuild.
- Write an explicit rollback test in the test suite.
- Confidence: MEDIUM (DuckDB transaction semantics for table operations are well-documented; the in-memory/persistent discrepancy is inferred from the architectural pattern).

**Phase assignment:** pragma_query_t implementation phase.

---

### P3.3 — The pragma SQL string must embed definition JSON as a SQL literal — injection risk

**What goes wrong:**
The `pragma_query_t` callback constructs SQL like:
```sql
INSERT INTO semantic_layer._definitions (name, definition_json)
VALUES ('my_view', '<json here>');
```
The `<json here>` placeholder is replaced by the serialized definition. If the JSON contains a single quote (any SQL string field with an apostrophe, e.g., a filter expression `WHERE status = 'active'`), the generated SQL will be syntactically broken, producing a parse error or, worse, SQL injection if the JSON is attacker-controlled.

**Warning signs:**
- `CREATE SEMANTIC VIEW` with a filter expression containing a single quote silently fails or truncates the definition.
- Integration tests do not cover definitions with SQL expressions containing apostrophes.

**Prevention strategy:**
- Escape all single quotes in the JSON string by doubling them (`'` → `''`) before embedding in the SQL literal. This is the standard SQL string literal escaping rule.
- Alternatively, serialize the JSON with Rust's `serde_json::to_string` and then apply the single-quote doubling pass. A dedicated function `fn sql_escape_string(s: &str) -> String` in the shim should be tested independently.
- Add a specific test: create a semantic view with a filter expression `WHERE description = 'it''s here'` and verify the definition round-trips correctly through save and reload.
- Confidence: HIGH (standard SQL string escaping requirement; no DuckDB-specific uncertainty).

**Phase assignment:** pragma_query_t implementation phase.

---

## Part 4 — EXPLAIN Hook

### P4.1 — There is no stable, documented `EXPLAIN` interception hook in DuckDB's extension API

**What goes wrong:**
DuckDB's official extension API (the C API used by `extension-template-rs` and the stable `duckdb-cpp-api`) does not expose an `EXPLAIN` statement interception hook. The EXPLAIN statement is handled internally by DuckDB's planner and produces a `PhysicalExplain` operator. Extensions can intercept the logical plan via an optimizer extension (`OptimizerExtension`) but cannot intercept `EXPLAIN` before DuckDB begins planning.

The workaround used in v0.1.0 (`explain_semantic_view()` table function) is effectively the only stable approach. Implementing true `EXPLAIN FROM semantic_query(...)` that shows expanded SQL instead of DuckDB's physical plan would require either:
- A C++ internal API hook (not in the stable extension API), or
- A `parser_override` that detects `EXPLAIN ... semantic_query(...)`, executes the expansion to get the SQL, and returns that as the result of a custom statement — effectively reimplementing `explain_semantic_view()` as a native DDL statement.

**Warning signs:**
- No DuckDB community extension in the registry implements EXPLAIN interception (the absence is evidence that the hook does not exist or is too fragile).
- Searching the DuckDB source for "explain" in extension API headers returns zero results.
- The `duckdb-cpp-api` stable API (work in progress as of 2025) does not include EXPLAIN hooks in its surface area.

**Prevention strategy:**
- Do not attempt to intercept `EXPLAIN` at the physical plan level. It requires internal C++ DuckDB API access that is not stable and will break with every DuckDB release.
- The `parser_override` approach: detect `EXPLAIN FROM semantic_query(...)` pattern, strip the `EXPLAIN` keyword, run `explain_semantic_view()` logic, and return the expanded SQL as a result table. This is syntactic sugar on top of the existing v0.1.0 `explain_semantic_view` function, not true EXPLAIN interception.
- For the v0.2.0 EXPLAIN goal, scope the feature as: `EXPLAIN semantic_query('view', dimensions := [...])` is rewritten by the parser override to `FROM explain_semantic_view('view', dimensions := [...])`. This is honest with users: the result is the expanded SQL string, not DuckDB's physical plan.
- Document clearly what `EXPLAIN` shows (the expansion, not the DuckDB physical plan) to avoid user confusion.
- Confidence: MEDIUM (absence of EXPLAIN hooks in community extension radar and stable C API; inferred from DuckDB architecture).

**Phase assignment:** EXPLAIN hook phase. Scope correctly before building.

---

### P4.2 — `parser_override` for EXPLAIN detection fires for all `EXPLAIN` statements, not just semantic query ones

**What goes wrong:**
If the EXPLAIN interception uses `parser_override`, the override fires for `EXPLAIN SELECT 1`, `EXPLAIN FROM parquet_scan(...)`, and every other `EXPLAIN` statement. If the fast-exit path is wrong, every `EXPLAIN` in the database is affected.

The pattern match must be both fast (it runs before every parse) and precise (it must not match `EXPLAIN SELECT` but must match `EXPLAIN FROM semantic_query(...)` and `EXPLAIN semantic_query(...)`).

**Warning signs:**
- Standard `EXPLAIN SELECT 1` returns an empty result or an error after loading the extension.
- `EXPLAIN SELECT * FROM some_table` is broken.
- The override is called for every query, noticeably slowing query execution (measurable with benchmarking).

**Prevention strategy:**
- Use a two-stage check: first check if the normalized query starts with `EXPLAIN` (fast string prefix check), then check if it contains `semantic_query(` as a substring. Only trigger the custom path if both conditions are true.
- Benchmark the `parser_override` overhead: for a 1000-query loop of `SELECT 1`, the overhead of the prefix check should be below 1 microsecond per query. If it is higher, the check is doing too much work.
- Write negative tests: verify that `EXPLAIN SELECT 1` still works correctly after loading the extension.
- Confidence: HIGH (general parser override design principle; specific DuckDB behavior confirmed by parser source analysis).

**Phase assignment:** EXPLAIN hook phase.

---

## Part 5 — Time Dimension Granularity

### P5.1 — `date_trunc('week', ...)` returns ISO Monday boundaries — wrong for US-convention users

**What goes wrong:**
DuckDB's `date_trunc('week', ts)` follows ISO 8601 — weeks start on Monday. If a user's semantic view time dimension uses `'week'` granularity, all weekly aggregations will use Monday-aligned bins. For US-standard analytics (Sunday-start weeks) the results will be misaligned by 0–6 days depending on the day of the week.

The ISO week year boundary is a deeper edge case: ISO week 1 of year N may begin in late December of year N-1. For example, 2015-01-01 falls in ISO week 1 of 2015, but the week begins on 2014-12-29. Aggregating by `date_trunc('week', ...)` for dates in the last days of December will group them into a bin that starts in the following year, which is counterintuitive when users expect year-aligned weekly data.

**Warning signs:**
- Weekly aggregations do not match the user's BI tool (which may use Sunday-start weeks).
- Year-end weekly reports show data attributed to the following year's first week.
- The documented granularity options do not specify ISO vs. US week conventions.

**Prevention strategy:**
- Document the week convention explicitly: "Week granularity uses ISO 8601 (Monday-start). Sunday-start weeks are not supported in v0.2.0."
- Do not offer a `FISCAL_WEEK` or `US_WEEK` granularity in v0.2.0. Add it only after implementing a clear convention specification mechanism in the DDL (e.g., `GRANULARITY week CONVENTION iso`).
- For time dimension definitions, add a validation check: if granularity is `'week'`, emit a warning (not an error) in the definition output that documents the ISO convention.
- Include a test: verify that a date of `2014-12-30` (which is in ISO week 1 of 2015) aggregates to the week starting `2014-12-29`, and that this behavior is asserted and documented.
- Confidence: HIGH (ISO 8601 week behavior in DuckDB is confirmed by DuckDB documentation and PostgreSQL date_trunc gotcha documentation; the ISO convention is standard SQL).

**Phase assignment:** Time dimension implementation phase.

---

### P5.2 — `date_trunc` on `TIMESTAMP` returns `TIMESTAMP`, not `DATE` — type mismatch downstream

**What goes wrong:**
`date_trunc('month', my_date_column)` where `my_date_column` is a `DATE` type returns a `TIMESTAMP` (specifically `TIMESTAMP '2024-01-01 00:00:00'`), not a `DATE` (`DATE '2024-01-01'`). When the generated SQL uses the result in a `JOIN` or comparison against a `DATE` column, DuckDB will implicitly cast — which usually works but can cause performance degradation (preventing index use) or subtle type errors if the consuming query applies strict type checks.

More concretely: if the user's semantic view groups by a `DATE` time dimension and then the user compares the output (VARCHAR, due to v0.1.0 architecture) against another date, the string representation `2024-01-01 00:00:00` does not sort or compare the same as `2024-01-01`.

**Warning signs:**
- Time dimension output values look like `2024-01-01 00:00:00` instead of `2024-01-01` in the VARCHAR output.
- String comparison on time dimension output gives wrong sort order (e.g., `'2024-01-01 00:00:00' > '2024-01-02'` evaluates as string comparison).
- Joining the `semantic_query` output to a table with a `DATE` column requires an explicit cast.

**Prevention strategy:**
- In the SQL expansion, wrap `date_trunc(...)` in a `CAST(... AS DATE)` when the source time dimension column is a `DATE` type. The model must record whether the source column is a `DATE` or `TIMESTAMP` (this metadata must be captured at definition time via `DESCRIBE` on the base table).
- For `TIMESTAMP` source columns, keep the `date_trunc` result as `TIMESTAMP` — the VARCHAR output will serialize as a consistent format either way.
- Add explicit tests for each combination: (`DATE` source, `month` granularity) → output is `2024-01-01`; (`TIMESTAMP` source, `month` granularity) → output is `2024-01-01 00:00:00`.
- Confidence: HIGH (DuckDB date_trunc behavior documented in DuckDB function reference; TIMESTAMP vs DATE return confirmed by issue #9223 analysis).

**Phase assignment:** Time dimension implementation phase.

---

### P5.3 — `TIMESTAMP WITH TIME ZONE` date truncation shifts by timezone — produces wrong day boundaries

**What goes wrong:**
DuckDB stores `TIMESTAMPTZ` internally as UTC. When `date_trunc('day', ts_with_tz)` is called, DuckDB converts the UTC timestamp to the session's configured timezone before truncating. This means a UTC timestamp of `2024-01-01 23:00:00+00` will truncate to `2024-01-01` in UTC but to `2024-01-02` in UTC+8 (East Asia) or `2023-12-31` in UTC-5 (US East). The session timezone is a DuckDB configuration setting — if users configure different timezones, the same query returns different results.

This is not a DuckDB bug — it is correct timezone-aware behavior — but it is a semantic layer pitfall because:
1. The semantic view is defined once and shared across users with different timezones.
2. The expansion engine does not have access to the session timezone at definition time.
3. The expanded SQL produces different results depending on who runs it.

**Warning signs:**
- The same semantic view query returns different daily totals in different timezones.
- Tests using `TIMESTAMPTZ` pass in CI (UTC timezone) but fail for US-timezone users.
- The `DuckDB` configuration setting `TimeZone` is not accounted for in time dimension expansion.

**Prevention strategy:**
- In v0.2.0, explicitly support only `TIMESTAMP` (naive/UTC) and `DATE` column types for time dimensions. Reject `TIMESTAMPTZ` columns at definition time with a clear error: "Time dimension columns must be TIMESTAMP (without timezone) or DATE. TIMESTAMPTZ is not supported in v0.2.0 due to timezone-dependent truncation behavior."
- Document this limitation explicitly. TIMESTAMPTZ support requires either a per-view timezone setting or an always-UTC normalization step, both of which are deferred.
- In CI, always verify: `SELECT current_setting('TimeZone')` returns `UTC` before running time dimension tests. Add this as a test setup assertion.
- Confidence: HIGH (DuckDB timezone behavior for TIMESTAMPTZ confirmed in DuckDB issue #9223 and DuckDB timestamp documentation).

**Phase assignment:** Time dimension implementation phase. Validation check at definition time.

---

### P5.4 — NULL timestamps in time dimension columns silently drop rows

**What goes wrong:**
`date_trunc('month', NULL)` returns `NULL`. If the base table has rows with NULL values in the time dimension column, those rows are silently excluded from groupings that use that time dimension — they have no bucket to fall into. This can cause metric totals to change depending on whether the time dimension is included in the query.

This is correct SQL behavior, but it violates the semantic layer contract: "the metric total should be stable across different dimension combinations." A `SUM(revenue)` without time dimension should equal `SUM(revenue)` with time dimension when all non-NULL dates are included — but if some rows have NULL dates, they are excluded from the time-dimensioned query, making the totals differ.

**Warning signs:**
- `semantic_query('view', metrics := ['total_revenue'])` returns a different total than `semantic_query('view', dimensions := ['order_month'], metrics := ['total_revenue'])` summed across all buckets.
- The difference equals the revenue for rows with NULL order dates.
- No error is emitted — the results are silently wrong.

**Prevention strategy:**
- When a time dimension is included in the query, document that NULL values in the time column are excluded from the result. Add this as a visible note in the `describe_semantic_view()` output for time dimension columns.
- Optionally: add a `COALESCE` wrapper in the expansion — `date_trunc('month', COALESCE(order_date, DATE '1970-01-01'))` — to bucket NULLs into a sentinel value. But this is misleading: the sentinel bucket will appear in results with a fake date. The cleaner approach is to leave NULL handling to the user and document it.
- Write a test that includes NULL dates in the test dataset and verifies the documented behavior (NULL rows excluded from time-dimensioned queries).
- Confidence: HIGH (standard SQL NULL behavior; not DuckDB-specific).

**Phase assignment:** Time dimension implementation phase.

---

## Part 6 — C++/Rust FFI at the Extension Boundary

### P6.1 — Memory ownership mismatch: C++ allocates, Rust frees (or vice versa)

**What goes wrong:**
DuckDB's C++ API passes strings and data structures as raw pointers. When the C++ shim passes a string (e.g., the query string from `parse_function`) to Rust code, and Rust stores or copies it, there are two hazards:

1. Rust converts a `*const c_char` to a `&str` — this is a borrow with a lifetime tied to when DuckDB frees the buffer. If DuckDB frees the buffer before the Rust code finishes using it (e.g., the Rust code queues work asynchronously), the `&str` becomes dangling.

2. Rust allocates a `String` to hold the definition JSON and passes a `*const c_char` back to C++ (via `set_error` or similar). If C++ frees this pointer with `free()` but Rust allocated it with the Rust allocator (which may differ on Windows), the free is undefined behavior.

**Warning signs:**
- Intermittent crashes (use-after-free) that only occur under high query load or when multiple queries are in flight.
- Valgrind or AddressSanitizer reports "invalid read" in the shim after extension load.
- The extension crashes on Windows but not on macOS/Linux (different default allocators).

**Prevention strategy:**
- For strings passed from C++ to Rust: always copy into a Rust-owned `String` immediately. Never store a `&str` that points into C++ memory across an await point or function call boundary.
- For strings passed from Rust to C++ (e.g., error messages via `set_error`): use a `CString` that is kept alive for the duration of the C++ call, then drop it after. Do not rely on C++ to free Rust-allocated memory.
- If DuckDB's API requires passing a heap-allocated string that DuckDB will free: use `libc::malloc` to allocate it so `free()` is the correct deallocation, not Rust's allocator. Or arrange for DuckDB to copy the string immediately (verify with DuckDB API documentation whether `set_error` copies its argument or stores a pointer).
- Confidence: HIGH (standard C/Rust FFI memory ownership requirements; Rustonomicon FFI chapter).

**Phase assignment:** C++ shim scaffold phase. Establish ownership protocol before writing any logic.

---

### P6.2 — `unsafe` Rust in FFI callbacks is not covered by existing fuzz targets

**What goes wrong:**
The v0.1.0 extension already has a known gap: the FFI execution layer (`execute_sql_raw`, `read_varchar_from_vector`) is not fuzz-covered because the loadable-extension function pointers are only initialized at DuckDB runtime. The C++ shim introduces new `unsafe` Rust code in:

- Parser callback functions (converting C pointers to Rust types)
- The `plan_function` result construction (writing DuckDB result types from Rust)
- The `pragma_query_t` string construction (reading parameters from C API)

These functions will be called from DuckDB's parse/plan path, which is triggered by user-supplied SQL. A malformed `CREATE SEMANTIC VIEW` statement could reach this path.

**Warning signs:**
- The new C++ shim code has more `unsafe {}` blocks than the v0.1.0 codebase.
- No fuzz target covers the `parse_function` or `plan_function` call paths.
- The CI test suite does not include malformed `CREATE SEMANTIC VIEW` statements as negative test cases.

**Prevention strategy:**
- Add a fuzz target `fuzz_create_ddl` that generates random SQL strings starting with `CREATE SEMANTIC VIEW` and feeds them through the Rust-side parse logic. The fuzz target cannot test the full C++ callback chain (requires DuckDB runtime) but can test the definition JSON parsing and SQL string construction logic.
- For the full integration path: add SQLLogicTest negative test cases covering: empty view name, invalid granularity, missing base table, SQL injection attempts in field names.
- Each `unsafe` block in the C++ shim FFI code should have a comment explaining the safety invariant being upheld and the DuckDB API guarantee that ensures the invariant.
- Confidence: HIGH (v0.1.0 TECH-DEBT.md explicitly documents this gap; C++ shim amplifies it).

**Phase assignment:** Hardening phase (after C++ shim is functional). Do not defer past the first CI run of the shim.

---

### P6.3 — Double initialization if extension is loaded twice in the same session

**What goes wrong:**
DuckDB's extension loader calls the entrypoint function once per database session. However, if a user calls `LOAD 'semantic_views'` twice (or if the extension is both autoloaded and manually loaded), the Rust initialization code runs a second time. The v0.1.0 code calls `init_catalog`, which creates the `semantic_layer` schema and `_definitions` table — this is idempotent (`CREATE SCHEMA IF NOT EXISTS`, `CREATE TABLE IF NOT EXISTS`). But the C++ shim adds `AddParserExtension` to register the parser hook. Calling `AddParserExtension` twice registers two copies of the hook. Both fire for every query, the second one processes what the first already handled (a statement that was already parsed), and the behavior is undefined.

**Warning signs:**
- After `LOAD 'semantic_views'` is called twice, `CREATE SEMANTIC VIEW` is processed twice (or errors on the second processing attempt).
- The `CREATE SEMANTIC VIEW` result set contains two rows.
- Fuzz or stress tests that reload the extension between tests see inconsistent behavior.

**Prevention strategy:**
- In the C++ `LoadInternal` function (called by DuckDB's extension loader): check if the parser extension is already registered before calling `AddParserExtension`. Maintain a static boolean `semantic_views_parser_registered` (with thread-safe initialization using `std::once_flag`) that prevents double registration.
- Alternatively, rely on DuckDB's extension loader guarantee: the entrypoint is called exactly once per database handle lifetime. Document this assumption. If violated, the static guard is the safety net.
- Add a test: explicitly call `LOAD 'semantic_views'` twice in a test script and verify that `CREATE SEMANTIC VIEW` works exactly once and `list_semantic_views()` returns one row.
- Confidence: MEDIUM (DuckDB extension loader is expected to call entrypoint once; the double-registration hazard is inferred from how AddParserExtension works in DuckDB's parser source).

**Phase assignment:** C++ shim scaffold phase. Establish the guard pattern before the first integration test.

---

## Summary: Pitfall Priority by Phase

| Phase | Critical Pitfalls | Why |
|-------|-------------------|-----|
| C++ Shim Scaffold | P1.1 (build inversion), P1.2 (symbol bloat), P1.3 (unwind/TLS conflict), P6.1 (memory ownership), P6.3 (double init) | Foundational; wrong choices require complete rework of the build and FFI layer |
| Parser Hook | P2.1 (parse_function vs parser_override), P2.2 (plan_function returns TableFunction), P2.3 (global registration, fallback mode) | Parser bugs affect all users and all queries; hard to debug after the fact |
| pragma_query_t | P3.1 (SQL-not-result semantics), P3.2 (rollback discrepancy), P3.3 (SQL literal escaping) | Persistence bugs cause silent data loss or corruption |
| EXPLAIN Hook | P4.1 (no stable hook exists), P4.2 (fires for all EXPLAIN) | Scope correctness prevents wasted implementation effort |
| Time Dimensions | P5.1 (ISO week convention), P5.2 (DATE vs TIMESTAMP return type), P5.3 (TIMESTAMPTZ timezone dependence), P5.4 (NULL silent exclusion) | All are silent correctness failures — no error, wrong answer |
| Hardening | P6.2 (fuzz gap amplified by C++ shim) | Security and correctness; should not be deferred to v0.3.0 |

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| C++ shim, first CMake build | P1.1 — footer injected twice | Delete Cargo-primary packaging before adding CMake |
| Linking Rust staticlib | P1.2 — symbol bloat | Add version script before first CI build |
| Parser callback error path | P1.3 — panic unwinds through C++ | `catch_unwind` wrapper on every Rust function called from C++ |
| `CREATE SEMANTIC VIEW` from CLI vs Python | P2.1 — trailing semicolon mismatch | Normalize query string before pattern match |
| Parser hook registration | P2.3 — breaks all SQL if wrong mode | Use FALLBACK_OVERRIDE, fast-exit on non-matching statements |
| pragma_query_t returns SQL | P3.1 — SQL not result | Model on FTS extension's `CreateFTSIndexQuery` pattern |
| Rollback of CREATE SEMANTIC VIEW | P3.2 — in-memory/persistent discrepancy | Defer in-memory update until after commit |
| View name with SQL chars | P3.3 — single-quote injection | Escape with doubled quotes, dedicated test |
| `EXPLAIN semantic_query(...)` | P4.1 — no stable hook | Use parser_override rewrite to explain_semantic_view() |
| EXPLAIN override | P4.2 — captures all EXPLAIN | Two-stage check: starts with EXPLAIN + contains semantic_query( |
| Week granularity | P5.1 — ISO week convention | Document ISO convention, test year-end boundary |
| date_trunc on DATE column | P5.2 — returns TIMESTAMP | Add CAST(... AS DATE) in expansion for DATE source columns |
| TIMESTAMPTZ time dimensions | P5.3 — timezone-dependent results | Reject TIMESTAMPTZ at definition time in v0.2.0 |
| NULL in time column | P5.4 — silent row exclusion | Document, add test with NULL dates in dataset |
| C++ shim unsafe code | P6.2 — not fuzz covered | Add fuzz_create_ddl target in hardening phase |
| Extension loaded twice | P6.3 — double parser registration | `std::once_flag` guard in LoadInternal |

---

## Research Notes

**Confidence assessment:**

| Area | Confidence | Basis |
|------|------------|-------|
| Build system inversion (P1.1) | MEDIUM | Inferred from extension-template-rs vs extension-template architecture; no direct "how to convert" documentation found |
| Symbol bloat (P1.2) | MEDIUM | Documented Rust language issue (#33221, #73295); DuckDB-specific version script pattern is inferred |
| Unwind/TLS (P1.3) | HIGH | Rust RFC 2945, Rustonomicon, catch_unwind documentation |
| parser_override vs parse_function (P2.1) | MEDIUM | DuckDB parser.cpp source analysis; semicolon inconsistency confirmed in issue #18485 |
| plan_function returns TableFunction (P2.2) | MEDIUM | FTS extension pattern analysis; DuckDB parser.cpp source |
| Global parser registration (P2.3) | MEDIUM | DuckDB parser.cpp override mode enum analysis |
| pragma_query_t SQL semantics (P3.1) | MEDIUM | FTS extension implementation (CreateFTSIndexQuery pattern confirmed); `pragma_query_t` type confirmed in DuckDB source |
| Rollback discrepancy (P3.2) | MEDIUM | DuckDB transaction model documentation; architectural inference |
| SQL literal escaping (P3.3) | HIGH | Standard SQL escaping; no DuckDB specifics needed |
| No stable EXPLAIN hook (P4.1) | MEDIUM | Absence of evidence in extension API, community extensions, duckdb-cpp-api; cannot confirm definitively without testing |
| Time granularity pitfalls (P5.1–P5.4) | HIGH | DuckDB function documentation, ISO 8601 standard, confirmed DuckDB issue #9223 |
| Memory ownership (P6.1) | HIGH | Standard C/Rust FFI; Rustonomicon |
| Fuzz gap (P6.2) | HIGH | Directly stated in v0.1.0 TECH-DEBT.md |
| Double init (P6.3) | MEDIUM | DuckDB extension loader behavior inferred from architecture; std::once_flag pattern is standard |

**Sources consulted:**
- DuckDB parser source: `duckdb/src/parser/parser.cpp` (GitHub)
- DuckDB extension issues: #18485 (semicolon handling), #9223 (date_trunc timezone)
- DuckDB community extensions issue #54 (Rust extension development guidance)
- DuckDB duckdb-rs issue #370 (C API extension for Rust)
- Rust RFC 2945 (c-unwind-abi), RFC 2797 (ffi-unwind project)
- Rust language issues: #33221 (staticlib symbol bloat), #73295 (symbol export from staticlib)
- DuckDB FTS extension (duckdb/duckdb-fts): CreateFTSIndexQuery pattern
- CIDR 2025 paper: "Runtime-Extensible Parsers" (Mühleisen and Raasveldt)
- DuckDB timestamp documentation and timezone guide
- v0.1.0 TECH-DEBT.md (this project)
- v0.1.0 src/lib.rs (this project) — manual FFI entrypoint reference
