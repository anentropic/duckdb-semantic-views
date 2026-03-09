# PITFALLS -- DDL Polish (v0.5.1)

**Domain:** Adding DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW native DDL and error location reporting to an existing DuckDB extension with parser hooks
**Researched:** 2026-03-08
**Context:** The extension already has a working `CREATE SEMANTIC VIEW` parser hook (fallback `parse_function` + `plan_function`), statement rewriting to function-based DDL, a dedicated DDL connection, and `strsim` for fuzzy matching. The catalog is a dual-store: `Arc<RwLock<HashMap>>` in-memory + `semantic_layer._definitions` DuckDB table via separate `persist_conn`. All DDL functions are registered as table functions with `extra_info` state injection. v0.5.0 shipped with 172 tests green.

---

## Critical Pitfalls

Mistakes that cause crashes, data loss, or require rearchitecting.

### P1: DROP/DESCRIBE/SHOW May Not Trigger Parser Fallback Hook

**What goes wrong:**
The current parser hook uses `parse_function` (fallback), which is only called when DuckDB's native parser FAILS on a statement. For `CREATE SEMANTIC VIEW`, this works because DuckDB's parser does not recognize `SEMANTIC` after `CREATE` and produces a parser error, triggering the fallback.

For `DROP SEMANTIC VIEW`, `DESCRIBE SEMANTIC VIEW`, and `SHOW SEMANTIC VIEWS`, the behavior depends on how DuckDB's native parser handles these prefixes:

- `DROP SEMANTIC VIEW x` -- DuckDB's parser recognizes `DROP` but not `SEMANTIC` as a valid object type (DuckDB supports TABLE, VIEW, FUNCTION, INDEX, SCHEMA, SEQUENCE, MACRO, TYPE). This likely produces a **parser error** at the `SEMANTIC` token, which WILL trigger the fallback hook.
- `DESCRIBE SEMANTIC VIEW x` -- DuckDB's `DESCRIBE` expects a table name, view name, or query. `SEMANTIC` is not a table/view, so DuckDB may attempt to resolve it as an identifier and produce a **catalog error** (not a parser error). Catalog errors happen AFTER parsing and do NOT trigger the parser fallback.
- `SHOW SEMANTIC VIEWS` -- `SHOW` in DuckDB is an alias for `DESCRIBE`. Same risk as DESCRIBE.

If DuckDB successfully parses `DESCRIBE SEMANTIC` (treating `SEMANTIC` as an identifier) and then fails at the binder/catalog level, the parser fallback is never called and the extension never gets a chance to intercept the statement.

**Consequences:** `DROP SEMANTIC VIEW` likely works via parser fallback. `DESCRIBE SEMANTIC VIEW` and `SHOW SEMANTIC VIEWS` may silently fail with unhelpful DuckDB errors ("Table 'SEMANTIC' does not exist") instead of being intercepted by the extension.

**Prevention:**
- Test each statement prefix in isolation FIRST, before implementing any logic. Run `DROP SEMANTIC VIEW x;`, `DESCRIBE SEMANTIC VIEW x;`, and `SHOW SEMANTIC VIEWS;` against a DuckDB instance with the extension loaded (but without the new parser detection) and observe the error type (Parser Error vs Catalog Error).
- If DESCRIBE/SHOW produce catalog errors (not parser errors), they CANNOT use the parser hook approach. Instead, implement them as table functions only: `FROM describe_semantic_view('name')` and `FROM list_semantic_views()` (which already exist). Document that `DESCRIBE SEMANTIC VIEW` is not supported as native syntax and users should use the function form.
- If they DO produce parser errors, add prefix detection for `DROP SEMANTIC VIEW`, `DESCRIBE SEMANTIC VIEW`, and `SHOW SEMANTIC VIEWS` alongside the existing `CREATE SEMANTIC VIEW` detection.
- **Confidence:** MEDIUM. DuckDB's parser behavior for unknown object types after DROP/DESCRIBE is not documented. The resolution requires empirical testing.

**Phase assignment:** Must be the FIRST thing validated. A 10-minute spike determines which DDL verbs can use the parser hook and which must remain function-only.

---

### P2: Catalog Inconsistency During CREATE OR REPLACE -- Persist Succeeds, In-Memory Fails

**What goes wrong:**
The existing `define.rs` uses a write-first pattern: persist to DuckDB table via `persist_conn` FIRST, then update the in-memory `HashMap`. For CREATE OR REPLACE, the persist step uses `INSERT OR REPLACE` (always succeeds), but the in-memory step uses `catalog_upsert` (validates JSON first). If JSON validation passes for persist but fails for the in-memory update (e.g., a subtle difference in validation paths), the DuckDB table has the new definition but the HashMap has the old one.

Looking at the actual code, this specific scenario is unlikely because both paths validate JSON. But a more realistic variant exists: if `persist_define` succeeds but the Rust process panics or the connection is interrupted between the persist and the in-memory update, the DuckDB table is updated but the HashMap is not. On next extension load, `init_catalog` re-reads from the DuckDB table, so the next session picks up the change. But the CURRENT session has stale state.

For CREATE OR REPLACE specifically, this means the user thinks they replaced a view (the persist succeeded, no error), but queries against the view in the same session use the OLD definition from the HashMap.

**Consequences:** Silent use of stale definition in the current session after a successful CREATE OR REPLACE. The user observes "wrong results" rather than an error. This is worse than a crash because it is hard to diagnose.

**Prevention:**
- The current code already handles this correctly: `catalog_upsert` is called after `persist_define`, and both validate JSON. The risk is theoretical, not demonstrated.
- Add an integration test that does CREATE, then CREATE OR REPLACE with different metrics, then queries using the new metrics. Assert the new metric values appear. This catches any cache staleness.
- Consider reversing the order for CREATE OR REPLACE specifically: update the HashMap first (recoverable), then persist (if persist fails, the HashMap has the new definition for the current session, and the DuckDB table has the old one -- next session reverts, but at least the current session is consistent). However, this contradicts the write-first pattern used everywhere else and introduces a different inconsistency risk. **Keep the current write-first order but test the round-trip.**
- **Confidence:** LOW for the actual bug occurring (code paths are well-tested). HIGH for the importance of having a round-trip test.

**Phase assignment:** Testing phase. Add a round-trip sqllogictest for CREATE OR REPLACE.

---

### P3: DROP via Parser Hook Runs DDL on Dedicated Connection -- Catalog Bypass

**What goes wrong:**
The existing parser hook path (`sv_parse_stub` -> `sv_plan_function` -> `sv_ddl_bind`) executes rewritten DDL on the `sv_ddl_conn` (the dedicated DDL connection stored as a C++ file-scope static). For `CREATE SEMANTIC VIEW`, this works because `rewrite_ddl_to_function_call` rewrites to `SELECT * FROM create_semantic_view(...)`, which calls the Rust VTab `bind` function, which updates both the DuckDB table and the HashMap.

For `DROP SEMANTIC VIEW`, the rewrite must produce `SELECT * FROM drop_semantic_view('name')`. This calls `DropSemanticViewVTab::bind`, which:
1. Checks the HashMap for existence
2. Calls `persist_drop` (deletes from DuckDB table via persist_conn)
3. Calls `catalog_delete` (removes from HashMap)

**The problem:** The `DropState` stored as `extra_info` on the `drop_semantic_view` function was registered with `persist_conn` pointing to the persistence connection created during `init_extension`. But when `DROP SEMANTIC VIEW` arrives via the parser hook path, the DDL is executed on `sv_ddl_conn` (the parser's dedicated DDL connection), NOT on the main connection. The `drop_semantic_view` function was registered on the main connection, and its `extra_info` (including `persist_conn`) was set at registration time.

**Question:** Does executing `SELECT * FROM drop_semantic_view('name')` on `sv_ddl_conn` still find the registered table function? YES -- table functions are registered at the database level (not per-connection), so `sv_ddl_conn` can invoke them. The `extra_info` pointer was set during registration and is accessible from any connection. So this should work.

**Where it actually breaks:** If there is a connection-specific state assumption. The `persist_conn` is a separate connection handle. When `drop_semantic_view` runs on `sv_ddl_conn` and then calls `persist_drop` on `persist_conn`, we have THREE connections involved: main, sv_ddl_conn (parser's DDL connection), and persist_conn. All three share the same database. DuckDB's single-writer model means only one can write at a time. If `sv_ddl_conn` is in a transaction (from the plan_function binding), and `persist_drop` tries to write via `persist_conn`, there may be a lock conflict.

**Consequences:** Potential deadlock or write-lock timeout during DROP via native DDL syntax.

**Prevention:**
- Test `DROP SEMANTIC VIEW x;` via the native DDL path (not just the function path) as an early integration test. If it deadlocks, the three-connection pattern needs restructuring.
- If a lock conflict occurs: rewrite DROP to also use `sv_ddl_conn` for persistence (skip `persist_conn` entirely when coming from the parser hook path). This may require modifying the FFI contract.
- The simplest fix if this is a problem: have `sv_execute_ddl_rust` call `drop_semantic_view` on the SAME connection it was given (the `exec_conn` parameter), which avoids the cross-connection lock conflict.
- **Confidence:** MEDIUM. The three-connection interaction is not tested in v0.5.0 (which only has CREATE via parser hook, not DROP). The lock behavior depends on DuckDB's transaction isolation for internal connections.

**Phase assignment:** Validate early. The first DROP implementation MUST be tested via the native DDL path.

---

## Moderate Pitfalls

### P4: Parser Detection Must Handle Multiple Prefixes Without Ambiguity

**What goes wrong:**
The current `detect_create_semantic_view` function checks a single prefix: `"create semantic view"`. Adding DROP, DESCRIBE, SHOW, CREATE OR REPLACE, and CREATE IF NOT EXISTS requires matching MULTIPLE prefixes. The detection function is called for EVERY failed parse in DuckDB, so it must be fast and unambiguous.

Prefix ambiguity risks:
- `CREATE OR REPLACE SEMANTIC VIEW` starts with `CREATE` -- must match the longer prefix, not just `CREATE SEMANTIC VIEW`.
- `CREATE SEMANTIC VIEW IF NOT EXISTS` starts with `CREATE SEMANTIC VIEW` but has additional tokens before the view name.
- `DROP SEMANTIC VIEW IF EXISTS` vs `DROP SEMANTIC VIEW` -- the IF EXISTS variant must be detected as a DROP, not misidentified.

If the detection function matches `CREATE SEMANTIC VIEW` for a `CREATE OR REPLACE SEMANTIC VIEW` statement, the rewrite will be wrong (the view name extraction will start at the wrong position).

**Consequences:** Wrong view name extracted. Malformed rewritten SQL. Confusing error messages.

**Prevention:**
- Use ordered prefix matching: check LONGER prefixes first.
  1. `CREATE OR REPLACE SEMANTIC VIEW` (longest CREATE variant)
  2. `CREATE SEMANTIC VIEW IF NOT EXISTS` (but note: IF NOT EXISTS comes AFTER the view name in standard SQL -- need to decide on syntax)
  3. `CREATE SEMANTIC VIEW` (base case)
  4. `DROP SEMANTIC VIEW IF EXISTS`
  5. `DROP SEMANTIC VIEW`
  6. `DESCRIBE SEMANTIC VIEW`
  7. `SHOW SEMANTIC VIEWS`
- Return a discriminant (enum or integer code) indicating WHICH statement type was detected, not just a boolean. The current `PARSE_NOT_OURS` / `PARSE_DETECTED` binary result is insufficient.
- DuckDB SQL convention: `IF NOT EXISTS` follows the object name in `CREATE TABLE IF NOT EXISTS t (...)` but follows the object type in `CREATE SCHEMA IF NOT EXISTS s`. For semantic views, use the `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)` syntax (IF NOT EXISTS after the type, before the name) -- this matches `CREATE TABLE IF NOT EXISTS`.
- Wait: DuckDB's standard `CREATE VIEW IF NOT EXISTS v AS ...` puts IF NOT EXISTS after VIEW, before the name. Follow this convention: `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)`.
- **Confidence:** HIGH. This is a pure string-matching problem. The mitigation is straightforward prefix ordering.

**Phase assignment:** Parser detection phase. Extend `detect_create_semantic_view` to return an enum.

---

### P5: IF NOT EXISTS Position Ambiguity in DDL Syntax

**What goes wrong:**
SQL standards and DuckDB place `IF NOT EXISTS` differently depending on the statement:
- `CREATE TABLE IF NOT EXISTS t (...)` -- after TABLE, before name
- `CREATE VIEW IF NOT EXISTS v AS ...` -- after VIEW, before name
- `CREATE SCHEMA IF NOT EXISTS s` -- after SCHEMA, before name
- `CREATE OR REPLACE TABLE t (...)` -- OR REPLACE after CREATE, before TABLE

For semantic views, the natural syntax would be:
- `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)`
- `CREATE OR REPLACE SEMANTIC VIEW name (...)`
- `DROP SEMANTIC VIEW IF EXISTS name`

But there is a subtlety: `CREATE OR REPLACE SEMANTIC VIEW IF NOT EXISTS name (...)` -- can OR REPLACE and IF NOT EXISTS coexist? In DuckDB, `CREATE OR REPLACE TABLE IF NOT EXISTS` is a parser error. The same should apply here.

**Consequences:** If the parser accepts conflicting modifiers, the behavior is undefined (replace OR skip?). If the parser rejects them, the error message must be clear.

**Prevention:**
- Reject `CREATE OR REPLACE ... IF NOT EXISTS` explicitly with a clear error: "Cannot combine OR REPLACE with IF NOT EXISTS."
- The `DefineState` struct already has `or_replace: bool` and `if_not_exists: bool` fields. The existing code comment says "Mutually exclusive with or_replace (or_replace takes precedence if both set)." Change this to an explicit error instead of silent precedence.
- Define the canonical syntax:
  - `CREATE SEMANTIC VIEW name (...)`
  - `CREATE OR REPLACE SEMANTIC VIEW name (...)`
  - `CREATE SEMANTIC VIEW IF NOT EXISTS name (...)`
  - `DROP SEMANTIC VIEW name`
  - `DROP SEMANTIC VIEW IF EXISTS name`
- **Confidence:** HIGH. Syntax design decision with clear precedent from DuckDB's own behavior.

**Phase assignment:** Parser detection phase. Document the syntax before implementing.

---

### P6: Error Location Reporting -- Character Positions Meaningless After Rewriting

**What goes wrong:**
The user writes: `CREATE SEMANTIC VIEW sales (tbles := [...], dimensions := [...])` (typo: `tbles` instead of `tables`). The extension rewrites this to: `SELECT * FROM create_semantic_view('sales', tbles := [...], dimensions := [...])`. DuckDB executes the rewritten SQL and returns an error: "Unknown named parameter 'tbles'... at position 47." Position 47 refers to the REWRITTEN SQL, not the original DDL. The user sees a position that does not correspond to their input.

This is even worse when the rewrite changes the string layout significantly. The `SELECT * FROM create_semantic_view('name', ` prefix shifts all positions by a variable amount depending on the view name length.

**Consequences:** Error positions are misleading. Users cannot find the error in their original SQL. This undermines the "quality error reporting" goal of v0.5.1.

**Prevention:**
- Do NOT pass through DuckDB's character positions from rewritten SQL errors. Instead, catch errors from the rewritten SQL execution and re-report them with context from the ORIGINAL DDL.
- For parameter name errors (e.g., unknown named parameter `tbles`): extract the parameter name from the DuckDB error, find its position in the ORIGINAL DDL string, and report that position.
- For structural errors (missing parens, malformed STRUCT literals): parse the original DDL enough to identify which clause contains the error, and report the clause name rather than a character position.
- The `sv_execute_ddl_rust` function already catches errors from `ffi::duckdb_query` and returns them as strings. Enhance the error path to:
  1. Catch the raw DuckDB error from the rewritten SQL
  2. Map it back to the original DDL context
  3. Add clause-level hints ("in the dimensions clause") and "did you mean" suggestions
- For "did you mean" on parameter names: the known parameter names are fixed (`tables`, `relationships`, `dimensions`, `metrics`). Use `strsim` to suggest corrections.
- **Confidence:** HIGH. This is a known problem with statement rewriting. The mitigation is error message post-processing, not architectural change.

**Phase assignment:** Error reporting phase. This is the core of v0.5.1's error UX improvement.

---

### P7: Error Messages from C++ BinderException Truncated at Buffer Boundary

**What goes wrong:**
The `sv_ddl_bind` function in `shim.cpp` calls `sv_execute_ddl_rust` which writes errors into a fixed-size buffer (`char error_buf[1024]`). If the error message exceeds 1024 bytes (plausible for errors that include the full expanded SQL, available column lists, or "did you mean" suggestions), the message is truncated at the buffer boundary.

The C++ side then throws `BinderException("CREATE SEMANTIC VIEW failed: %s", error_buf)`, which DuckDB formats and shows to the user. A truncated error message ending mid-word or mid-suggestion is confusing and unhelpful.

**Consequences:** Long error messages are silently truncated. Users see incomplete suggestions or partial SQL fragments.

**Prevention:**
- Increase the error buffer size. 1024 bytes is tight for error messages that include "did you mean" suggestions with available names listed. Use 4096 bytes.
- Alternatively, allocate the error string dynamically: have `sv_execute_ddl_rust` return a `malloc`'d C string (via `CString::into_raw`) and have the C++ side free it after use. This eliminates the size limit entirely. The pattern is: Rust allocates, C++ copies into the BinderException string, then calls a Rust-side free function (`sv_free_string`).
- At minimum, if using a fixed buffer, ensure the error message is constructed to fit: put the most important information (error type, parameter name, suggestion) first, and the expanded SQL (if any) last, so truncation cuts the least important part.
- **Confidence:** HIGH. Buffer overflow is a well-understood problem. The fix is mechanical.

**Phase assignment:** Error reporting phase. Increase buffer or switch to dynamic allocation.

---

### P8: DROP Semantic View Leaves Orphaned References in Active Queries

**What goes wrong:**
If a user executes `DROP SEMANTIC VIEW sales` while another connection has an active `FROM semantic_view('sales', ...)` query in flight, the DROP removes the definition from the HashMap. The in-flight query may be in the middle of execution (it read the definition during bind, expanded the SQL, and is streaming results). The DROP does not affect the in-flight query because the expanded SQL is already being executed by DuckDB on its own connection.

But if the in-flight query fails and retries (or if DuckDB re-binds for any reason), it will find the definition missing from the HashMap and produce a confusing error: "Semantic view 'sales' not found" -- even though it was just working moments ago.

**This is standard database behavior** -- DuckDB's own `DROP TABLE` has the same semantics. But for semantic views, the "not found" error is more confusing because the view is not visible in DuckDB's catalog (it is in our custom HashMap), so `SHOW TABLES` never showed it in the first place.

**Consequences:** Confusing error during concurrent DROP + query. Not a data corruption risk.

**Prevention:**
- This is acceptable behavior. DuckDB's own DROP has the same semantics.
- Document in README: "DROP SEMANTIC VIEW takes effect immediately. In-flight queries that reference the dropped view may continue until completion but cannot be re-executed."
- The error message for "view not found" already includes "Did you mean?" suggestions and "Run FROM list_semantic_views()" guidance. This is sufficient.
- **Confidence:** HIGH. Standard database behavior. No code change needed, just documentation.

**Phase assignment:** Documentation phase.

---

### P9: DESCRIBE/SHOW Rewriting Harder Than DROP -- No Clean Function Target

**What goes wrong:**
For DROP, the rewrite is clean: `DROP SEMANTIC VIEW x` becomes `SELECT * FROM drop_semantic_view('x')`. The drop function already exists and returns a result.

For DESCRIBE and SHOW, the rewrite targets also already exist:
- `DESCRIBE SEMANTIC VIEW x` -> `SELECT * FROM describe_semantic_view('x')`
- `SHOW SEMANTIC VIEWS` -> `SELECT * FROM list_semantic_views()`

But the rewrite is more complex because:
1. `DESCRIBE SEMANTIC VIEW x` must extract the view name `x` from a different position than CREATE (no parenthesized body).
2. `SHOW SEMANTIC VIEWS` has no view name at all -- it is a parameterless command.
3. `DROP SEMANTIC VIEW IF EXISTS x` must handle the IF EXISTS modifier.

Each statement type requires its own parse-and-rewrite function. The current `parse_ddl_text` and `rewrite_ddl_to_function_call` are specific to CREATE and assume a parenthesized body.

**Consequences:** If you try to reuse the CREATE parser for DROP/DESCRIBE/SHOW, view names are extracted from the wrong position or the parser fails on missing parentheses.

**Prevention:**
- Write a separate rewrite function for each statement type. Each function extracts only the parameters it needs:
  - `rewrite_drop(query) -> "SELECT * FROM drop_semantic_view('name')"` or `"SELECT * FROM drop_semantic_view_if_exists('name')"`
  - `rewrite_describe(query) -> "SELECT * FROM describe_semantic_view('name')"`
  - `rewrite_show(query) -> "SELECT * FROM list_semantic_views()"`
- The detection function returns a statement type discriminant. The `sv_execute_ddl_rust` function dispatches to the correct rewriter based on the type.
- **Alternatively:** Build a single `parse_ddl` function that returns a structured result: `{ stmt_type: Create/Drop/Describe/Show, name: Option<&str>, body: Option<&str>, or_replace: bool, if_exists: bool, if_not_exists: bool }`. Each rewriter is a match arm on `stmt_type`.
- **Confidence:** HIGH. This is straightforward parsing work. The risk is only in trying to be too clever with shared code.

**Phase assignment:** Parser extension phase. Build the dispatch table before implementing individual DDL verbs.

---

### P10: FFI Return Code Insufficient for Multiple Statement Types

**What goes wrong:**
The current FFI contract between Rust and C++ is:
- `sv_parse_rust(query, len) -> u8`: returns 0 (not ours) or 1 (detected CREATE SEMANTIC VIEW)
- `sv_execute_ddl_rust(query, len, conn, name_out, ...) -> u8`: returns 0 (success) or 1 (error)

For v0.5.1, the parse function needs to detect MULTIPLE statement types and communicate WHICH type was detected back to C++. A single u8 return of 0/1 is insufficient.

The execute function also needs different behavior per type: CREATE returns a view name, DROP returns a view name, DESCRIBE returns multiple columns of metadata, SHOW returns a list of views. These have different output schemas.

**Consequences:** If the FFI contract is not extended, all new DDL types must produce the same output schema (single VARCHAR "view_name"), which is wrong for DESCRIBE and SHOW.

**Prevention:**
- Extend `sv_parse_rust` to return a discriminant: 0 = not ours, 1 = CREATE, 2 = CREATE OR REPLACE, 3 = CREATE IF NOT EXISTS, 4 = DROP, 5 = DROP IF EXISTS, 6 = DESCRIBE, 7 = SHOW. These fit in a u8.
- For `sv_execute_ddl_rust`: keep the current contract for CREATE/DROP (which both return a view name). For DESCRIBE and SHOW, use the rewrite-to-function approach: the Rust side rewrites the DDL to the corresponding `SELECT * FROM describe_semantic_view(...)` or `SELECT * FROM list_semantic_views()`, executes it on the DDL connection, and returns a success signal. The C++ `sv_ddl_bind` function declares a minimal output schema (single VARCHAR) for CREATE/DROP, but for DESCRIBE/SHOW, the actual results are produced by the table function executing on the DDL connection, and the plan function should return a different TableFunction with the correct schema.
- **Simpler approach:** Have the C++ `sv_plan_function` check the discriminant and select different TableFunction implementations for each statement type. Only CREATE/DROP use `sv_ddl_bind`. DESCRIBE/SHOW get their own bind functions that declare the correct output schemas.
- **Simplest approach:** All statement types are rewritten to `SELECT * FROM [function](...)` by Rust, executed on the DDL connection by `sv_execute_ddl_rust`, and the C++ plan function always returns a minimal "success message" TableFunction. The actual query results come from the rewritten function call. This works for CREATE and DROP (which return a view name), but DESCRIBE and SHOW should return their full result sets. If the rewritten SQL is executed on `sv_ddl_conn` and the results are discarded (only the side effect matters for CREATE/DROP), then DESCRIBE/SHOW need a different approach because their VALUE is the result set, not a side effect.
- **Recommended approach:** Only use the parser hook rewriting for DDL STATEMENTS (CREATE, DROP) which have side effects. For QUERIES (DESCRIBE, SHOW) that return result sets, either:
  (a) Rewrite to the function call and have the C++ plan_function return a table function that executes the rewrite and returns results, or
  (b) Accept that DESCRIBE/SHOW remain function-only (no native syntax) for v0.5.1.
  Option (b) is simpler and still delivers value. Document the function syntax in README.
- **Confidence:** MEDIUM. The FFI extension is mechanical but the output schema problem for DESCRIBE/SHOW requires an architectural decision.

**Phase assignment:** Architecture decision needed before implementation. Decide whether DESCRIBE/SHOW get native syntax in v0.5.1 or remain function-only.

---

## Minor Pitfalls

### P11: Fuzzy Matching Suggestions in DDL Errors May Be Wrong Context

**What goes wrong:**
The existing `suggest_closest` function (using `strsim::levenshtein`) is used at query time to suggest corrections for unknown dimension/metric names. For DDL errors, the same function could suggest corrections for unknown parameter names (e.g., `tbles` -> `tables`).

But the available names differ by context:
- In a query: available names are dimensions and metrics of the specific view
- In DDL: available names are the fixed set of parameter names (`tables`, `relationships`, `dimensions`, `metrics`)
- In DROP: available names are existing view names

If `suggest_closest` is called with the wrong candidate list, the suggestions will be nonsensical.

**Prevention:**
- Parameterize suggestions by context. When constructing error messages for DDL, pass the correct candidate list.
- For unknown parameter names in CREATE DDL: candidates = `["tables", "relationships", "dimensions", "metrics"]`
- For "view not found" in DROP: candidates = list of existing view names from the HashMap
- **Confidence:** HIGH. The function already takes `available: &[String]` as a parameter. Just pass the right list.

**Phase assignment:** Error reporting phase.

---

### P12: SHOW SEMANTIC VIEWS Name Conflicts with DuckDB SHOW

**What goes wrong:**
DuckDB's `SHOW` command is an alias for `DESCRIBE`. The full set of DuckDB SHOW commands includes `SHOW TABLES`, `SHOW ALL TABLES`, `SHOW DATABASES`. If the extension intercepts `SHOW SEMANTIC VIEWS`, it must ensure that `SHOW TABLES` and other DuckDB SHOW commands are NOT intercepted.

The prefix `SHOW SEMANTIC` is unique enough (DuckDB has no built-in `SHOW SEMANTIC` command), but careless prefix matching could intercept `SHOW SETTINGS` if only checking `SHOW S...`.

**Prevention:**
- Match the exact prefix `SHOW SEMANTIC VIEWS` (three words, case-insensitive), not just `SHOW S`.
- The existing `detect_create_semantic_view` pattern of comparing a full prefix string handles this correctly.
- Test that `SHOW TABLES`, `SHOW ALL TABLES`, and `SHOW DATABASES` still work after the extension is loaded.
- **Confidence:** HIGH. String prefix matching. Straightforward.

**Phase assignment:** Parser detection phase. Add negative tests for DuckDB's own SHOW commands.

---

### P13: CREATE OR REPLACE Does Not Re-Infer Column Types

**What goes wrong:**
The existing `DefineSemanticViewVTab::bind` performs DDL-time type inference via `LIMIT 0` on the expanded SQL (lines 123-145 of `define.rs`). This populates `column_type_names` and `column_types_inferred` in the definition JSON. For CREATE OR REPLACE, if the new definition has different source tables or expressions, the column types may have changed. If the type inference step is skipped (e.g., because `persist_conn` is None for in-memory databases), the definition retains the old type information from the previous CREATE.

In practice, type inference runs in the same `bind` call for CREATE OR REPLACE as for CREATE, so the types ARE re-inferred. But if the source tables do not exist yet when CREATE OR REPLACE runs (e.g., the user is redefining a view before recreating the underlying tables), the `LIMIT 0` query fails, type inference is silently skipped, and the definition stores empty type vectors.

**Consequences:** Subsequent queries against the replaced view may use stale type information, leading to type mismatches handled by `build_execution_sql` cast wrappers. This works but is inefficient and may produce unexpected cast behavior.

**Prevention:**
- This is already handled correctly by the existing code: type inference failure is non-fatal, and `build_execution_sql` handles mismatches at query time.
- Add a test: CREATE a view, DROP the source table, CREATE OR REPLACE the view with a different source, re-create the source table, then query. Assert types are correct at query time.
- **Confidence:** HIGH. The existing defensive type system handles this. Test coverage is the gap.

**Phase assignment:** Testing phase.

---

### P14: Error Buffer UTF-8 Safety at FFI Boundary

**What goes wrong:**
The `write_to_buffer` function in `parse.rs` copies Rust string bytes into a raw C buffer. If the error message contains multi-byte UTF-8 characters (e.g., from table names or column names with accented characters), truncation at the buffer boundary may cut a multi-byte sequence in half. The C++ side reads the buffer as a `char*` and constructs a `std::string`. A truncated UTF-8 sequence is invalid, but `std::string` does not validate encoding, so it becomes a `BinderException` with mojibake or partial characters.

**Consequences:** Garbled error messages when non-ASCII identifiers are involved and the error message is near the buffer size limit.

**Prevention:**
- In `write_to_buffer`: after determining `copy_len`, walk backward to find a valid UTF-8 boundary. Truncate to the last complete UTF-8 code point.
- Alternatively, switch to dynamic allocation (see P7) to eliminate truncation entirely.
- This is a minor issue because DuckDB identifiers are typically ASCII. But it is a correctness gap.
- **Confidence:** HIGH. UTF-8 truncation is a well-known problem.

**Phase assignment:** Error reporting phase. Fix if switching to dynamic allocation; otherwise, add boundary-safe truncation.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Parser hook validation spike | P1 (DROP/DESCRIBE/SHOW may not trigger fallback) | Empirical test of each prefix against DuckDB's parser |
| Parser detection extension | P4 (multiple prefix matching), P5 (IF NOT EXISTS position), P12 (SHOW conflicts) | Ordered prefix matching, define syntax, negative tests |
| FFI contract extension | P10 (return code insufficient), P7 (buffer truncation), P14 (UTF-8 safety) | Extended discriminant, larger/dynamic buffers |
| DROP implementation | P3 (three-connection lock conflict), P8 (concurrent DROP + query) | Test native DDL path early, document behavior |
| CREATE OR REPLACE | P2 (persist vs memory inconsistency), P5 (mutual exclusion with IF NOT EXISTS), P13 (type re-inference) | Round-trip tests, explicit rejection of conflicting modifiers |
| Error reporting | P6 (position meaningless after rewrite), P7 (buffer truncation), P11 (wrong suggestion context) | Error post-processing, context-aware suggestions |
| DESCRIBE/SHOW | P1 (may not trigger parser hook), P9 (different rewrite structure), P10 (output schema mismatch) | Decide function-only vs native syntax before implementing |

---

## Research Notes

**Confidence Assessment:**

| Area | Confidence | Basis |
|------|------------|-------|
| Parser fallback behavior for DROP (P1) | MEDIUM | DuckDB docs confirm DROP supports specific object types; `SEMANTIC` is not one; likely parser error but needs empirical test |
| Parser fallback behavior for DESCRIBE/SHOW (P1) | LOW | DESCRIBE may treat next token as identifier, causing catalog error not parser error; needs empirical test |
| Catalog consistency (P2) | HIGH | Code review of existing write-first pattern; risk is theoretical |
| Three-connection locking (P3) | MEDIUM | DuckDB single-writer documented; but three-connection interaction during parser hook bind is novel territory |
| Prefix matching (P4) | HIGH | Pure string matching; ordered prefixes solve this |
| IF NOT EXISTS syntax (P5) | HIGH | DuckDB precedent clear: IF NOT EXISTS after object type, before name |
| Error position mapping (P6) | HIGH | Well-known statement rewriting problem; error post-processing is standard approach |
| Buffer truncation (P7) | HIGH | Fixed buffer arithmetic; obvious fix |
| FFI contract extension (P10) | MEDIUM | Mechanical change but DESCRIBE/SHOW output schema needs architectural decision |

**Sources consulted:**
- [DuckDB DROP statement documentation](https://duckdb.org/docs/stable/sql/statements/drop) -- supported object types
- [DuckDB DESCRIBE statement documentation](https://duckdb.org/docs/stable/sql/statements/describe) -- DESCRIBE syntax and behavior
- [DuckDB issue #18485: Inconsistent semicolon handling](https://github.com/duckdb/duckdb/issues/18485) -- parser extension input normalization (already handled in v0.5.0)
- [DuckDB Runtime-Extensible Parsers (blog)](https://duckdb.org/2024/11/22/runtime-extensible-parsers) -- parse_function is fallback, not override
- [DuckDB Runtime-Extensible Parsers (CIDR 2025 paper)](https://duckdb.org/pdf/CIDR2025-muehleisen-raasveldt-extensible-parsers.pdf) -- extension parsers replace full grammar, not extend it
- [Effective Rust: Control what crosses FFI boundaries](https://effective-rust.com/ffi.html) -- FFI error handling best practices
- [Rust FFI error reporting (users.rust-lang.org)](https://users.rust-lang.org/t/best-practices-for-error-reporting-from-rust-to-c/18345) -- Rust-to-C error patterns
- [DuckPGQ extension (GitHub)](https://github.com/cwida/duckpgq-extension) -- existence proof for DROP PROPERTY GRAPH via parser hooks
- This project's `src/parse.rs` -- current parser detection and rewriting
- This project's `cpp/src/shim.cpp` -- current C++ shim with plan_function and DDL connection
- This project's `src/catalog.rs` -- dual-store catalog (HashMap + DuckDB table)
- This project's `src/ddl/define.rs` -- CREATE with or_replace and if_not_exists flags
- This project's `src/ddl/drop.rs` -- DROP with persist_conn and if_exists patterns
- This project's `src/ddl/describe.rs` -- DESCRIBE as table function (existing)
- This project's `src/ddl/list.rs` -- LIST as table function (existing)
- This project's `src/query/error.rs` -- existing error types and display formatting
- This project's `src/expand.rs` -- fuzzy matching via strsim, ExpandError types
- This project's `src/lib.rs` -- extension init, connection creation, function registration
- This project's `TECH-DEBT.md` -- accepted decisions and deferred items
