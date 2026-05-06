# Phase 62: Caret restoration + LRU removal — Research

**Researched:** 2026-05-06
**Domain:** DuckDB parser-extension internals (parser_override + parse_function), Rust↔C++ FFI lifetime management, sqllogictest + Python integration test infrastructure
**Confidence:** HIGH (all four open questions resolved by direct code inspection of `cpp/include/duckdb.cpp` amalgamation and the project's own sources)

---

## 1. Executive summary

The ultraplan's mermaid architecture is sound — re-introducing `parse_function` purely as the error-reporting fallback keeps every v0.8.0/v0.8.1 transactional win and restores `ParserException::SyntaxError` caret rendering, because DuckDB's `Parser::ParseQuery` calls `parse_function` only after the default Postgres parser fails (`cpp/include/duckdb.cpp:347279-347304`) and throws via `ParserException::SyntaxError(query, result.error, result.error_location)` on `DISPLAY_EXTENSION_ERROR`. The `error_location` parameter is rendered as a byte offset into the **user's input string** — exactly what `ParseError::position` already carries throughout `validate_and_rewrite`.

There is one **showstopper** in the ultraplan as written: step 2 of "Files to modify → cpp/src/shim.cpp" proposes that `~SemanticViewsParserInfo()` call `sv_drop_override_context` → `duckdb_disconnect(catalog_conn)`. Tracing `~DatabaseInstance` (`duckdb.cpp:276813-276834`) shows `connection_manager.reset()` runs **before** `~DBConfig` fires (config is destroyed when the DatabaseInstance members destroy in reverse declaration order, after the destructor body completes). At the moment our destructor runs, `Connection::~Connection()` would call `ConnectionManager::Get(*context->db).RemoveConnection(*context)` (`duckdb.cpp:275798`), and `GetConnectionManager()` deref's a now-null `unique_ptr` (`duckdb.cpp:277129-277131`). **Result: use-after-free during DB shutdown.** The fix is to NOT disconnect the catalog connection in the destructor — drop the `Box<OverrideContext>` (free Rust-side allocation) but leak the `duckdb_connection`, matching v0.8.0's behaviour. The leak is bounded at one Connection object per DB ever created in the process; this was the original, working design before the LRU was introduced.

Everything else in the ultraplan holds up. The four open questions are resolved below with concrete code references.

---

## 2. Resolutions to the four open research questions

### Q1: Does `validate_and_rewrite` track positions in the user's input string or in the rewritten SQL?

**Quote:** *"Does `validate_and_rewrite` track positions in the user's input string or in the rewritten SQL? Caret rendering depends on `error_location` being a byte offset into the user's input. If positions are currently rewritten-SQL-side, the plan needs to add input-side position tracking."*

**Answer: positions are already tracked in the user's input string. No remapping work needed.**

Code references:
- `src/parse.rs:996-1063` — `validate_and_rewrite(query: &str)` — the parameter `query` is the user-provided input; `lead = skip_leading_whitespace_and_comments(query)` and `let trim_offset = lead;` define `trim_offset` as a byte offset into `query`. Every error path returns `position: Some(trim_offset + ...)` — i.e. an offset into the user's input.
- `src/parse.rs:1226-1237` — `validate_create_body` constructs `body_offset = trim_offset + body_offset_in_tns` and passes it to `parse_keyword_body(body_text, body_offset)`. The `body_offset` is again a user-input offset; `parse_keyword_body` propagates it into every `ParseError::position` it produces.
- `src/parse.rs:976-983` — `detect_near_miss` returns `position: Some(trim_offset)` where `trim_offset = lead` (user-input offset).
- `src/errors.rs:11-16` — the doc comment on `ParseError::position` explicitly states *"a 0-based byte offset into the original query string (before any trimming). DuckDB uses this to render a caret (^) under the error location."*

The rewritten SQL (`SELECT * FROM create_semantic_view_from_json(...)`) is built **only on the success path** (`Ok(Some(sql))`) and is never associated with positions. On the error path (`Err(ParseError)`) we never produce rewritten SQL at all.

**Implication for the plan:** the spike report's risk-3 ("`extract_caret_position` returns column index relative to the rewritten SQL") is a non-issue. `sv_parse_function_rust` can pass `ParseError::position` straight through to `ParserExtensionParseResult::error_location` and the caret will render in the right place. The Python helper at `test_caret_position.py:73-94` already strips DuckDB's `LINE 1: ` prefix correctly — it operates on DuckDB's rendered output, not on our internal SQL.

**Verification: HIGH** — confirmed by direct code reading; one test (`validate_and_rewrite_with_leading_comment_succeeds`, `src/parse.rs:4632`) already pins this contract for leading comments.

---

### Q2: Is the `duckdb_connection` still valid when `~SemanticViewsParserInfo()` fires during DB shutdown?

**Quote:** *"Destruction order: when `DBConfig` (and therefore `ParserExtensionInfo`) is destroyed, is the underlying `Database` (and its `duckdb_connection` handles) still valid? `~SemanticViewsParserInfo()` calls `sv_drop_override_context` → `duckdb_disconnect(catalog_conn)`. If the connection is already a dangling handle at that point, the destructor closes nothing or crashes."*

**Answer: NO. Calling `duckdb_disconnect` from `~SemanticViewsParserInfo()` is unsafe and will trigger use-after-free. The plan must drop this step.**

Destruction order, traced through `cpp/include/duckdb.cpp`:

1. `DatabaseInstance::~DatabaseInstance()` body runs (`duckdb.cpp:276813-276834`):
   - `connection_manager.reset()` (line 276819) — destroys the `unique_ptr<ConnectionManager>` and its internal map of `weak_ptr<ClientContext>`.
   - Subsequent resets of `db_manager`, `scheduler`, etc.
2. After the body returns, members are destroyed in reverse declaration order. `config` (a `DBConfig` value member of `DatabaseInstance`) is destroyed at this point.
3. `~DBConfig()` (line 276805) runs. Members destroyed include the `ExtensionCallbackManager` → `ExtensionCallbackRegistry` → vector of `ParserExtension` → each `parser_info` `shared_ptr<ParserExtensionInfo>` → `~SemanticViewsParserInfo()`.
4. If `~SemanticViewsParserInfo()` calls `duckdb_disconnect(catalog_conn)`, that does `delete reinterpret_cast<Connection*>(catalog_conn)` (`duckdb.cpp:266473-266479`).
5. `~Connection()` (line 275794) calls `ConnectionManager::Get(*context->db).RemoveConnection(*context)`.
6. `ConnectionManager::Get(DatabaseInstance &db)` returns `db.GetConnectionManager()` (line 276894-276896).
7. `DatabaseInstance::GetConnectionManager()` returns `*connection_manager` (line 277129-277131) — but `connection_manager` was reset at step 1, so this is a **null-pointer deref** (or at minimum a destroyed object access).

**This is a real bug, not a theoretical concern.** The v0.8.0 implementation (commit 680a967) handled this by simply not disconnecting the catalog connection at all — the `duckdb_connection` was leaked once per DB ever loaded. The v0.8.1 LRU was introduced to bound that leak in long-running multi-DB processes.

**Recommended fix for Phase 62:** keep the destructor, but only free the Rust-side `Box<OverrideContext>`. Do not call `duckdb_disconnect` on the contained `CatalogReader`. The `Connection` heap object leaks for the rest of the process, which is identical to v0.8.0 behaviour — and acceptable because:
- DuckDB itself does not expose a clean DB-shutdown hook (TECH-DEBT 20 — confirmed: `ExtensionCallback` only has `OnConnectionOpened/Closed/OnExtensionLoaded/OnExtensionLoadFail`; no `OnDatabaseShutdown` — `duckdb.cpp:276149-276177`).
- The leak is one `Connection` object (~few KB) per DB *ever* opened in the process — bounded by the actual workload, not by a 16-entry cap.
- The Rust-side allocation (`Box<OverrideContext>`, `Mutex`, etc.) IS reclaimed because `sv_drop_override_context(rust_state)` re-boxes and drops the `OverrideContext` — only the `Connection` in `CatalogReader::conn` leaks.
- The user-visible LRU silent-eviction-then-error class (TECH-DEBT 20) is gone either way — that was the goal of attaching `OverrideContext` directly to `parser_info`.

**Alternative considered:** register an `ExtensionCallback` (`duckdb.cpp:276149`) and disconnect in `OnConnectionClosed` for *some* connection. This does not work — `OnConnectionClosed` fires on the user's connections, not on our private `catalog_conn`, and DuckDB has no signal for "the last connection is closing, please tear down extension state."

**Action item for the planner:** the per-file change list at `cpp/src/shim.cpp` step 2 should read:
- `~SemanticViewsParserInfo()` calls `sv_drop_override_context(rust_state)`.
- `sv_drop_override_context` in Rust re-boxes the `OverrideContext`, lets it Drop normally, but does **not** invoke `duckdb_disconnect` on the inner `CatalogReader`. Add a comment in `sv_drop_override_context` documenting why the `duckdb_connection` is intentionally leaked (link back to `~DatabaseInstance` ordering and TECH-DEBT 20 resolution).

**Verification: HIGH** — destruction order traced through the amalgamation; behaviour validated against v0.8.0 commit 680a967 which used the same leak pattern successfully for one full milestone.

---

### Q3: Other ways the override setting can be disabled besides `disable_peg_parser` + missing FALLBACK re-set?

**Quote:** *"Other ways the override setting can be disabled besides `disable_peg_parser` + missing FALLBACK re-set? If yes, the rc=3 actionable error message ("`SET allow_parser_override_extension='FALLBACK'`") would be misleading in those other cases."*

**Answer: yes, four additional vectors. The actionable error message remains correct for all of them — re-issuing `SET allow_parser_override_extension='FALLBACK'` is the universal recovery in every case.**

Vectors discovered:

1. **User explicitly sets `DEFAULT` or `STRICT`** — `SET allow_parser_override_extension='DEFAULT'` (or `'STRICT'`) on any connection. The `OnSet` callback (`duckdb.cpp:301174-301176`) just validates the enum string — there is no scope restriction or warning. Under `DEFAULT_OVERRIDE`, `Parser::ParseQuery` (`duckdb.cpp:347190`) `continue`s past the `parser_override` block entirely, equivalent to disabled.
2. **`RESET allow_parser_override_extension`** — resets to `DefaultValue = "DEFAULT"` (`duckdb.cpp:4708-4717`), same effect as case 1. The setting's `Scope = SettingScopeTarget::GLOBAL_DEFAULT` means a session-level reset is permitted (verified: no DuckDB code rejects session resets of GLOBAL_DEFAULT settings).
3. **`CALL disable_peg_parser()`** — the documented case (TECH-DEBT 21). This is implemented in DuckDB's autocomplete extension (registered as a TABLE_FUNCTION_ENTRY, `duckdb.cpp:3412`) which is not in the amalgamation source and we therefore cannot inspect, but the observed behaviour is "resets `allow_parser_override_extension` to `DEFAULT`."
4. **STRICT_OVERRIDE with another extension's parser_override returning DISPLAY_ORIGINAL_ERROR** — under `STRICT_OVERRIDE` (`duckdb.cpp:347199-347205`), if any earlier-registered parser_override returns `DISPLAY_ORIGINAL_ERROR`, our hook is still iterated (each extension is independent), but if a later extension returns DISPLAY_EXTENSION_ERROR after ours returns DISPLAY_ORIGINAL_ERROR, the strict-error path takes precedence. This is an interaction effect; recovery is still "set FALLBACK." (Edge case — only fires when multiple parser_override extensions co-exist.)

In all four cases, the user-facing fix is identical: `SET allow_parser_override_extension='FALLBACK';` re-arms our hook. The ultraplan's actionable error message (rc=3 "parser_override disabled — SET allow_parser_override_extension='FALLBACK'") is therefore correct universally — it does not need case-specific branching.

**Refinement to the message wording (suggested for the plan):**
- Drop the implication that the user did something to "disable" it — they may simply have inherited a `DEFAULT` setting from session config or another extension. Suggested: `"semantic_views: parser_override is not active for this connection (allow_parser_override_extension='DEFAULT' or 'STRICT'). Re-enable with: SET allow_parser_override_extension='FALLBACK';"`.

**Verification: MEDIUM** — vectors 1, 2, 4 verified by reading `Parser::ParseQuery` and the setting struct. Vector 3 (`disable_peg_parser`) confirmed via TECH-DEBT 21 plus existing test `peg_compat.test:130-133` which works around it. The interaction effect with other parser_override extensions (vector 4) is theoretical — no other parser_override-using extension is currently part of this project's environment, but the behaviour is dictated by `Parser::ParseQuery`'s loop semantics.

---

### Q4: Is read-side table-function registration genuinely unaffected by the `OverrideContext` change?

**Quote:** *"Is read-side table-function registration (`describe_semantic_view`, `list_semantic_views`, `show_semantic_*`, `get_ddl`, `read_yaml_from_semantic_view`) genuinely unaffected by the `OverrideContext` change? The plan asserts yes — confirm by tracing each registration site."*

**Answer: yes, fully unaffected.**

All read-side registrations in `src/lib.rs:389-482` use the pattern `con.register_table_function_with_extra_info::<…VTab, _>(name, &catalog_reader)` or `con.register_scalar_function_with_state::<…>(name, &catalog_reader)`. The `&catalog_reader` is a `Copy` of `CatalogReader { conn: duckdb_connection }` (`src/catalog.rs:88-91`) — the table function gets its own copy of the raw connection pointer at registration time. Neither the LRU nor the `OverrideContext` is consulted on the read side.

A grep of the entire `src/` tree for `OverrideContext`, `parser_override_catalog`, `set_catalog_for_parser_override`, and `db_token` returns hits only in `src/parse.rs` (definition + write-side rewriters: `rewrite_create`, `rewrite_drop_or_alter`, `rewrite_yaml_file_create`, `sv_parser_override_rust`, `rewrite_to_native_sql`) and `src/lib.rs` (registration wiring at lines 328, 372-385). Read-side modules (`src/ddl/describe.rs`, `src/ddl/list.rs`, `src/ddl/show_*.rs`, `src/ddl/get_ddl.rs`, `src/ddl/read_yaml.rs`, `src/query/explain.rs`, `src/query/table_function.rs`) are NOT in the result set.

The `semantic_view` table function and `explain_semantic_view` use a separate `query_conn` (`src/lib.rs:462-476`) via `QueryState`, not the catalog connection — also unaffected.

**Specific implication for the plan:** when removing the LRU module and converting `set_catalog_for_parser_override` into the new direct attachment, no read-side `register_*` call needs to change. The `&catalog_reader` references can stay; the same `duckdb_connection` is shared (by value-copy) between the read-side table functions and the `OverrideContext` we hand to the C++ shim. All copies remain valid for the life of the database — same as today.

**Note on YAML FILE path (only edge case):** `rewrite_yaml_file_create` (`src/parse.rs:2062-2158`) calls `crate::query::table_function::execute_sql_raw(ctx.catalog.raw(), &read_sql)` to read the file via DuckDB's `read_text()`. After Phase 62 this becomes `execute_sql_raw(override_ctx.catalog.raw(), …)` — same `duckdb_connection`, same behaviour. Confirmed unaffected.

**Verification: HIGH** — full grep of `src/` performed; lib.rs registration block read in full.

---

## 3. Additional risks / edge cases (not in the ultraplan)

### Risk A — Extension-load ordering and `parser_info` shared_ptr indirection

`ExtensionCallbackManager::Register` (`duckdb.cpp:281093-281098`) uses an atomic-swapped `shared_ptr<ExtensionCallbackRegistry>`: each `Register` call **clones** the registry vector and atomically replaces the registry. Iteration via `ParserExtensions()` (line 281157) takes a `shared_ptr` snapshot, so concurrent registrations and concurrent parses are safe. **However**: if the extension is loaded twice into the same DB (e.g. user runs `LOAD semantic_views` after a previous `LOAD`), TWO `SemanticViewsParserInfo` objects exist, each holding a raw `duckdb_connection`. Both are iterated by `Parser::ParseQuery` (line 347186-347213). With FALLBACK_OVERRIDE, the first one to return success wins; under our new design both will return success on identical inputs, so behaviour is consistent — but `parser_info` no longer represents a 1:1 DB↔context mapping the way the LRU did.

**Mitigation in the plan:** none required; the ultraplan's design (one `OverrideContext` per `ParserExtensionInfo`, unique per `Register` call) already handles this correctly. Worth a note in the test plan: re-LOAD in the same DB should still work — likely already covered by `phase2_restart.test`. Worth verifying explicitly.

### Risk B — `duckdb_disconnect` ordering on extension unload (NOT extension reload)

DuckDB exposes no extension-unload hook in 1.10.502 (TECH-DEBT 20 acknowledges this). Phase 62's design accepts the connection leak in `~SemanticViewsParserInfo` (see Q2 resolution); there is no path for unload to free more than the user could before. This is a pure regression-from-current-state question: same leak shape as v0.8.0. Not a Phase 62 risk per se, just a property of the design.

### Risk C — The actionable error in rc=3 needs to round-trip caret-renderable text

`sv_parse_function_rust` returning rc=3 needs a `position` (byte offset). The natural choice: 0 (caret at the start of the offending statement, where `CREATE SEMANTIC VIEW` begins). Alternatively the position can be `UINT32_MAX` (no caret) — but if we do that, the user sees no `LINE 1: ... ^` rendering for this case, only a flat "Parser Error: ...". Recommend setting `position = 0` for this branch so the caret lands on the C of CREATE.

### Risk D — `disable_peg_parser` is a connection-level pragma

When the rc=3 actionable error fires, the user's connection has `parser_override_setting = DEFAULT`. The error message tells them to `SET allow_parser_override_extension='FALLBACK'`. Per `SettingScopeTarget::GLOBAL_DEFAULT` (line 4714) this SET will apply to all connections in the process — fine, but if the user is in a session where a different parser_override extension expected DEFAULT, they may have an interaction. This is unlikely in practice (we are the only parser_override consumer in the project's testing matrix). Not a Phase 62 problem; document as a limitation if it arises.

### Risk E — sqllogictest Python wrapper may catch by exception type

Spike report risk #1 flagged this. Concrete check: `rg -n "InvalidInputException|InvalidInputError" test/integration/` — needs to run as part of Phase 62 implementation to confirm zero hits. Adding to validation strategy below.

### Risk F — Wave 0 compile guard for the `ParserExtensionParseResult` constructor with `error_location`

The compat header at `cpp/include/parser_extension_compat.hpp:67-85` declares `error_location` as a public field but currently no path constructs a `ParserExtensionParseResult` with `error_location` populated. After Phase 62 lands, `sv_parse_stub` writes to `result.error_location = position;` after `ParserExtensionParseResult(error_msg)` returns. This works because `error_location` is a public field, but the construct-then-assign is two statements. If a future DuckDB bump moves `error_location` private or changes its type from `optional_idx`, this breaks silently. Add a `static_assert(sizeof(optional_idx) <= 16, …)` or similar guard alongside the existing `static_assert(sizeof(ParserOptions) == 32, …)` (line 150) to catch drift early.

---

## 4. Implementation guidance — refinements to the ultraplan

The ultraplan's per-file change list is correct in shape. Three refinements:

### Refinement 1 — `cpp/src/shim.cpp` step 2: do NOT call `duckdb_disconnect` from the destructor

```
// REVISED (was: ~SemanticViewsParserInfo() calls sv_drop_override_context → duckdb_disconnect)
~SemanticViewsParserInfo() override {
    if (rust_state) {
        sv_drop_override_context(rust_state);  // frees Box<OverrideContext> only
        rust_state = nullptr;
    }
    // The duckdb_connection inside CatalogReader is intentionally leaked.
    // ~DatabaseInstance resets connection_manager BEFORE ~DBConfig fires;
    // calling duckdb_disconnect here would invoke ~Connection() →
    // ConnectionManager::RemoveConnection() on a destroyed manager (UAF).
    // See Phase 62 RESEARCH.md §Q2 for the destruction-order trace.
}
```

The Rust side `Drop for OverrideContext` should NOT call `duckdb_disconnect` on `self.catalog.conn`. Document the leak in the impl with a comment back to TECH-DEBT 20's resolution.

### Refinement 2 — rc=3 actionable error: improved wording + position=0

```rust
// In sv_parse_function_rust, on the "valid DDL but parser_override didn't fire" branch:
write_error_to_buffer(error_out, error_out_len,
    "semantic_views: parser_override is not active for this connection \
     (allow_parser_override_extension is 'DEFAULT' or 'STRICT'). \
     Re-enable with: SET allow_parser_override_extension='FALLBACK';");
*position_out = 0_u32;  // caret on the 'C' of CREATE / 'D' of DROP / etc.
return 3;
```

### Refinement 3 — Verification step: detect "load twice in same DB"

Add a sqllogictest case (or augment `phase2_restart.test`) that:
1. `LOAD semantic_views;`
2. `CREATE SEMANTIC VIEW v1 AS …;`
3. `LOAD semantic_views;` (no-op or re-register — depends on DuckDB's idempotency)
4. `DROP SEMANTIC VIEW v1;` — must succeed without ambiguity even though two `parser_info` instances may now exist.

If DuckDB de-duplicates `LOAD`, this is a no-op test; if it doesn't, this confirms our hook composes correctly. Either result is acceptable; the test pins the contract.

---

## 5. Project Constraints (from CLAUDE.md)

The following directives from `./CLAUDE.md` MUST be honoured by the plan:

- **Quality gate:** `just test-all` MUST pass before phase verification. This runs `test-rust + test-sql + test-ducklake-ci + test-vtab-crash + test-caret + test-adbc + test-large-view + test-multi-db + test-concurrent` (`justfile:137`). A verification that only runs `cargo test` is incomplete.
- **Pre-push:** `just ci` adds lint (clippy pedantic + fmt + cargo-deny) and fuzz target compilation checks (`justfile:144`).
- **Branch:** all work on `milestone/v0.8.0`. Currently on `milestone/v0.8.1` per the consolidation note in the ultraplan (the branch will be renamed before merge per the v0.8.0 surgery step). Verify branch before every commit.
- **Parallel builds forbidden:** never run `cargo` or `make` in parallel (feedback `feedback_no_parallel_builds.md`).
- **No worktrees:** `feedback_worktree_isolation.md` — do not use worktree isolation.
- **No long-running commands piped to bare `tail`:** redirect to `$TMPDIR` first.

These constraints carry the same authority as locked decisions and are not negotiable in the plan.

---

## 6. Validation Architecture

This section maps Phase 62's behavioural requirements to test infrastructure. Phase 62 has no formal REQ-IDs (it is interior architecture work), so the table is keyed by behavioural property.

### Test Framework

| Property | Value |
|----------|-------|
| Rust unit + proptest | `cargo test` (default `bundled` feature; `extension`-feature-gated tests run via `just test-sql` / `just test-caret` indirectly through the loaded extension) |
| sqllogictest | `just test-sql` (requires `just build` first) |
| Python integration | `uv run test/integration/<name>.py`, individually wired to `just test-caret`, `just test-adbc`, `just test-multi-db`, `just test-concurrent`, `just test-large-view`, `just test-vtab-crash` |
| Quick run command (per task commit) | `cargo test` |
| Per-wave merge | `just test-all` |
| Phase gate | `just ci` (= `lint test-all check-fuzz docs-check`) green before `/gsd-verify-work` |

### Phase 62 Behavioural Requirements → Test Map

| Behaviour | Test type | Automated command | File exists? |
|-----------|-----------|-------------------|-------------|
| **B1: Caret renders for malformed CREATE (missing `(`)** | Python integration | `uv run test/integration/test_caret_position.py::test_caret_missing_paren` | ✅ exists; tighten assertions to call `extract_caret_position` and assert column equals start of offending token (currently asserts message text only) |
| **B2: Caret renders for clause typo (`TBLES` → suggest `TABLES`)** | Python integration | `uv run test/integration/test_caret_position.py::test_caret_clause_typo` | ✅ exists; tighten as B1 |
| **B3: Caret renders for prefix near-miss (`CRETAE`)** | Python integration | `uv run test/integration/test_caret_position.py::test_caret_near_miss` | ✅ exists; tighten as B1 |
| **B4: Caret column is correct for multi-line CREATE** | Python integration | new test in `test_caret_position.py` (e.g. `test_caret_multiline_typo`) | ❌ Wave 0 — test query spans 3+ lines with malformed clause on line 2; assert caret reports the column on the offending line, not absolute byte offset |
| **B5: Caret column is correct in presence of multibyte UTF-8 chars before error** | Python integration | new test (`test_caret_unicode_prefix`) | ❌ Wave 0 — query like `CREATE SEMANTIC VIEW vüé AS TBLES …`; assert caret column reflects character position, NOT byte offset (DuckDB's caret is rendered against the displayed query text). Document if DuckDB itself rounds to byte vs char — pin whatever it does today as the contract. |
| **B6: Caret renders for ALTER errors** | Python integration or sqllogictest | new test, e.g. add to `test_caret_position.py` | ❌ Wave 0 — `ALTER SEMANTIC VIEW v RENAM TO w;` should surface caret at `RENAM` |
| **B7: Caret renders for DROP errors** | Python integration | new test | ❌ Wave 0 — `DROP SEMANTIC VIEW;` (missing name) should surface caret after the prefix |
| **B8: rc=3 actionable error fires when override setting is `DEFAULT`** | sqllogictest | append to `test/sql/peg_compat.test` near line 130 | ❌ Wave 0 — `CALL disable_peg_parser(); CREATE SEMANTIC VIEW v AS …;` — assert error contains the actionable hint substring `SET allow_parser_override_extension='FALLBACK'`, then re-issue the SET and verify recovery. Modify the existing peg_compat.test §4 to remove the workaround SET on the first attempt and test the new error path. |
| **B9: rc=2 deferral for non-DDL queries** | Rust unit test | `cargo test parse::tests::sv_parse_function_rust_returns_2_for_select` (new) | ❌ Wave 0 — also covers UTF-8 invalidity case |
| **B10: rc=1 with position for malformed CREATE** | Rust unit test | `cargo test parse::tests::sv_parse_function_rust_returns_1_with_position` (new) | ❌ Wave 0 |
| **B11: rc=1 for near-miss with suggestion text** | Rust unit test | `cargo test parse::tests::sv_parse_function_rust_returns_1_for_cretae` (new) | ❌ Wave 0 |
| **B12: Transactional CREATE/DROP/ALTER under `BEGIN…ROLLBACK` still works (unchanged from v0.8.x)** | sqllogictest | `just test-sql` runs `test/sql/v080_transactional_ddl.test` | ✅ exists — must remain green byte-identical |
| **B13: ADBC autocommit=false transactional DDL still works** | Python integration | `just test-adbc` | ✅ exists — must remain green |
| **B14: PK constraint race for concurrent CREATE IF NOT EXISTS unchanged** | Python integration | `just test-concurrent` | ✅ exists — pinned by TECH-DEBT 23 |
| **B15: Multi-DB isolation preserved (no LRU eviction error)** | Python integration | `just test-multi-db` | ✅ exists; extend to load extension into >16 distinct DBs sequentially in one process and assert no eviction error from any. The current test loads into 2 DBs only. |
| **B16: Multi-DB long-process memory bounded** | Python integration | extend `test_multi_db_isolation.py` or new test | ❌ Wave 0 — sequentially open + close 50 in-memory DBs each running CREATE; assert no panic, RSS does not grow unboundedly (loose check — log RSS and assert delta < 50 MB or similar). Catalog connection leak is one Connection per DB; 50 DBs leaks ≤ 50 Connection objects = small. |
| **B17: LRU module is gone** | Code review / build | `rg "parser_override_catalog\|set_catalog_for_parser_override\|MAX_CATALOG_ENTRIES\|LRU_CAPACITY" src/` returns nothing | manual check (also: the existing `src/parse.rs:2761-2786` test must be deleted, not just `#[ignore]`d) |
| **B18: Extension re-LOAD in same DB does not crash** | sqllogictest | new test or extension to existing | ❌ Wave 0 — `LOAD semantic_views; LOAD semantic_views; CREATE SEMANTIC VIEW v …;` should work; pins idempotency contract |
| **B19: FALLBACK_OVERRIDE error path produces `Parser Error:` not `Invalid Input Error:`** | Python integration probe (defensive) | new test | ❌ Wave 0 — pin the wrapper class transition; if any sqllogictest happens to depend on the old prefix, `rg "Invalid Input Error.*semantic\|Invalid Input Error.*does not exist" test/sql/` should return zero hits |

### Sampling cadence

- **Per task commit:** `cargo test` (5–15 s; unit + proptest, excludes extension feature). Catches Rust-level rewriter / parser regressions early.
- **Per Wave merge:** `just test-all` (build + Rust + sqllogictest + DuckLake CI + 6 Python integration tests; ~5–10 minutes).
- **Phase gate (before `/gsd-verify-work`):** `just ci` (adds lint + fuzz compile + docs-check; ~6–12 minutes total).

### Coverage strategy — caret position correctness

- **LINE 1 / single-line:** B1, B2, B3 (existing tests, tightened in Phase 62 task).
- **Multi-line:** B4 (new) — DuckDB renders caret on `LINE N` not `LINE 1` for multi-line input; verify our position propagates correctly.
- **Unicode column counting:** B5 (new) — pin whatever DuckDB does today as the contract. Note: DuckDB's `ParserException::SyntaxError` uses byte-based positioning internally; what the user sees in `LINE 1: …^` is how DuckDB renders that. We should NOT remap.
- **Per DDL form:** ensure caret tests exist for CREATE (B1-B3, B4, B5), ALTER (B6), DROP (B7). DESCRIBE / SHOW are read-side and rarely produce ParseError-from-validation (they pass through to read-side table functions); a single sqllogictest assertion that DESCRIBE non-existent view still produces a sane error suffices.
- **Per error class:** structural (missing token), clause typo (TBLES), near-miss prefix (CRETAE), name-only-form errors (B7). Each class hits a distinct code path in `validate_and_rewrite`.

### Coverage strategy — LRU removal

- **B17:** the LRU module is gone — grep-based smoke check + delete the `parser_override_catalog_lru_evicts_oldest` test (`src/parse.rs:2763-2786`).
- **B15, B16:** behavioural verification that the LRU's two failure modes are gone:
  - Silent eviction (no test ever covered this directly because eviction was silent — replaced by the inability to evict).
  - "Catalog context evicted (process opened more than 16 databases)" friendly error (formerly emitted by `rewrite_create` / `rewrite_drop_or_alter`). Confirm by opening 17+ DBs in a single process and verifying every one supports CREATE.
- **Multi-DB isolation:** B15 — `test_multi_db_isolation.py` already pins per-DB routing correctness; ensure the new design preserves it. The path from `parser_info` to `OverrideContext` to `CatalogReader` is direct and per-DB, so isolation is structurally preserved (no global map to confuse).

### Wave 0 gaps

- [ ] `test/integration/test_caret_position.py` — tighten 3 existing test assertions to call `extract_caret_position(error_text)` and assert column position; add B4 (multiline), B5 (unicode), B6 (ALTER), B7 (DROP).
- [ ] `test/sql/peg_compat.test` — add B8 case: `disable_peg_parser` followed by CREATE without re-arming FALLBACK should surface the actionable rc=3 error.
- [ ] `src/parse.rs::tests` — add B9 (rc=2 for SELECT 1), B10 (rc=1 with position for malformed CREATE), B11 (rc=1 with suggestion text for CRETAE). Estimated ~50 LOC.
- [ ] `src/parse.rs::tests` — DELETE existing test `parser_override_catalog_lru_evicts_oldest` (lines 2761-2786). Cannot survive Phase 62 — the module it exercises is removed.
- [ ] `test/integration/test_multi_db_isolation.py` — extend to 17+ sequential DB opens (B15 expanded coverage); add B16 (RSS bounded over 50 iterations).
- [ ] New sqllogictest case for B18 (extension re-LOAD).
- [ ] Probe for sqllogictest matchers anchored on `Invalid Input Error:` prefix (B19): `rg "error.*Invalid Input Error" test/sql/` — spike report says zero hits, re-confirm during Wave 0.
- [ ] Probe for Python tests catching by exception type: `rg "InvalidInputException\|ParserException" test/integration/` — if any hit, those tests will need updating in Phase 62 because the wrapper class will change.

---

## 7. Sources

### Primary (HIGH confidence — direct code reading)

- `cpp/include/duckdb.cpp` (DuckDB 1.10.502 amalgamation, vendored in repo):
  - `Parser::ParseQuery` — lines 347169-347320 (parser_override iteration; parse_function fallback; ParserException::SyntaxError throw with error_location)
  - `~DatabaseInstance` — lines 276813-276834 (destruction order: `connection_manager.reset()` then config destruction)
  - `~Connection` — lines 275794-275799 (uses ConnectionManager via context->db)
  - `DatabaseInstance::GetConnectionManager` — lines 277129-277131 (deref of unique_ptr)
  - `ExtensionCallbackManager::Register` + `ParserExtensions()` — lines 281093-281161 (atomic shared_ptr swap; parser_info shared_ptr lifetime)
  - `ExtensionCallback` — lines 276149-276177 (no shutdown hook)
  - `AllowParserOverrideExtensionSetting` — lines 4708-4717 (DefaultValue=DEFAULT, scope=GLOBAL_DEFAULT)
  - `AllowParserOverrideExtensionSetting::OnSet` — lines 301174-301176
- `cpp/include/parser_extension_compat.hpp` — re-declarations matching the amalgamation; ParserExtensionParseResult with error_location field at line 84
- `cpp/src/shim.cpp` — current parser_override hook (lines 112-157), registration (lines 165-210)
- `src/parse.rs` — validate_and_rewrite (line 996), detect_near_miss (line 946), rewrite_to_native_sql (line 1750), sv_parser_override_rust (line 2547), parser_override_catalog LRU module (lines 40-129), sql_throwing (line 2609), write_error_to_buffer (line 1631)
- `src/lib.rs` — init_extension (lines 334-485), parser hook registration (lines 372-386), read-side table function registrations (lines 389-482)
- `src/catalog.rs` — CatalogReader Copy semantics (lines 88-91), RAII guards PreparedStmt + QueryResult
- `src/errors.rs` — ParseError::position contract (lines 11-16)
- `TECH-DEBT.md` lines 162-198 — items 19, 20, 21, 22, 23 all relevant
- `test/integration/test_caret_position.py` — full file (240 lines)
- `test/sql/peg_compat.test` — lines 115-145 (PEG re-enable + workaround SET)
- `justfile` — test target wiring (lines 100-144)
- `_notes/v0.8.0_phase_62_ultraplan.md` — design source
- `_notes/v0.8.0_phase_62_sqllogictest_spike.md` — test scope

### Secondary (MEDIUM — derived from code observations)

- `disable_peg_parser` resetting `allow_parser_override_extension` — reported in TECH-DEBT 21; the implementation is in DuckDB's autocomplete extension (registered as TABLE_FUNCTION_ENTRY at `duckdb.cpp:3412`) which is not in the amalgamation source. Behaviour confirmed by `peg_compat.test:120-133` which works around it.

### Tertiary (LOW — none required)

No web searches needed for this phase. All claims grounded in repo code or vendored DuckDB amalgamation.

---

## 8. Assumptions Log

| # | Claim | Section | Risk if wrong |
|---|-------|---------|---------------|
| A1 | `disable_peg_parser` resets `allow_parser_override_extension` to DEFAULT (not FALLBACK or any other state). | Q3 vector 3 | Low — test `peg_compat.test:130` would catch a divergence. The actionable error text would still be correct (re-set FALLBACK works regardless of the prior state). |
| A2 | DuckDB 1.10.502 has no extension-unload hook. | Q2 mitigation | Low — TECH-DEBT 20 (written 2026-04) confirms this; if a future bump adds one, the leak becomes recoverable but isn't broken. |
| A3 | The Risk-A scenario (loading extension twice into same DB) is not currently exercised by the test suite explicitly. | Risk A | Low — even if it is exercised, the new design composes correctly because each `parser_info` carries its own `OverrideContext`. |

All other claims in this research are tagged VERIFIED via direct reading of code in the repo or the vendored amalgamation.

---

## 9. Open questions remaining

None blocking. All four ultraplan questions are resolved with HIGH or MEDIUM confidence; the one showstopper (Q2 destruction order) has a concrete mitigation that the planner can adopt directly.

A residual research-grade question worth tracking but not blocking Phase 62:
- Does DuckDB's `ParserException::SyntaxError` render Unicode in the offending query as character columns or byte columns? (B5 will pin whatever it does as the contract.)

---

## RESEARCH COMPLETE
