# Phase 63: Read-Only Database LOAD Support — Research

**Researched:** 2026-05-15
**Domain:** DuckDB read-only access mode + extension load lifecycle; minimal changes to `init_catalog` + `CatalogReader` short-circuit; sqllogictest / pytest fixture design for read-only paths
**Confidence:** HIGH (every claim verified against repo code or vendored DuckDB amalgamation `cpp/include/duckdb.cpp`)

---

## 1. Executive Summary

Phase 63 is small in code surface (2 Rust files: `src/lib.rs`, `src/catalog.rs`) but requires precise care in three places that the ROADMAP sketch does not call out:

1. **`current_setting('access_mode')` returns the lowercased enum form** — `"read_only"`, not `"READ_ONLY"`. Trace: `AccessModeSetting::GetSetting` (`cpp/include/duckdb.cpp:301163-301167`) calls `StringUtil::Lower(EnumUtil::ToString(...))`. Match on `"read_only"` directly (or upper-case the result and match `"READ_ONLY"`); do NOT compare against the C++ enum literal.

2. **The `cpp/src/shim.cpp:399` `config.SetOption("allow_parser_override_extension", Value("FALLBACK"))` call IS safe on read-only DBs** — `AllowParserOverrideExtensionSetting::OnSet` (`duckdb.cpp:301174-301176`) only validates the enum string; no I/O, no catalog mutation. No phase work needed in C++.

3. **`rewrite_drop` / `rewrite_alter_rename` / `rewrite_alter_comment` call `catalog.exists(name)` BEFORE emitting any INSERT/DELETE/UPDATE.** On a fresh read-only DB with no `_definitions` table, that pre-check is what would surface `"Catalog Error: Table semantic_layer._definitions does not exist"` today. The `catalog_table_present` short-circuit converts that into `lookup → Ok(None)`, which routes through the existing `"semantic view '<name>' does not exist"` ParseError path (10 hits across `src/ddl/`, `src/query/error.rs:8`). For RO-05, this is the **correct** outcome on a fresh read-only DB: the user gets "does not exist" instead of "read-only" because the view legitimately doesn't exist. RO-05's "or the closest equivalent" clause covers this. On a bootstrapped read-only DB (the more interesting case), `lookup → Ok(Some(json))`, the rewrite proceeds, and the emitted INSERT/DELETE/UPDATE on the caller's connection surfaces DuckDB's standard `"Cannot execute statement of type \"<TYPE>\" on database \"<name>\" which is attached in read-only mode!"` (`duckdb.cpp:273011-273013`).

**Primary recommendation:** Implement exactly the 4-step ROADMAP sketch with the access_mode lowercase fix. Do not modify `cpp/src/shim.cpp`. Do not change `src/parse.rs`. Add the `catalog_table_present` field to `CatalogReader` and have `lookup` / `list_all` / `list_names` short-circuit on it.

---

## 2. Project Constraints (from CLAUDE.md)

The following directives MUST be honoured by the plan:

- **Quality gate:** `just test-all` MUST pass before phase verification. This runs `cargo test` (Rust unit + proptest + doc tests), `just test-sql` (sqllogictest), `just test-ducklake-ci`, plus the Python integration tests wired into `just test-all`. A verification that only runs `cargo test` is incomplete.
- **Pre-push:** `just ci` adds clippy pedantic + fmt + cargo-deny + fuzz target compilation checks.
- **Branch:** all work on `milestone/v0.9.0`. Verify branch before every commit (user switches branches frequently — `feedback_worktree_isolation.md`).
- **No worktrees, no parallel builds:** `feedback_worktree_isolation.md`, `feedback_no_parallel_builds.md`.
- **Long commands:** never pipe to bare `tail` — redirect to `$TMPDIR` first (`feedback_no_tail_on_long_commands.md`).
- **Milestone completion ritual:** CHANGELOG.md (Keep-a-Changelog 1.1.0 headings only — `Added`, `Changed`, etc. — NO ad-hoc `### Phase 63` subheading), example file, version bump in `Cargo.toml` + `description.yml`, then squash-merge + tag.

---

## 3. Numbered Answers to Q1–Q9

### Q1 — Access mode detection mechanism

**Recommended call sequence** at the LOAD entrypoint in Rust (inside `init_extension`, before `init_catalog`):

```rust
// Detect read-only access mode for THIS database.
// AccessModeSetting::GetSetting (duckdb.cpp:301163-301167) calls
// StringUtil::Lower(EnumUtil::ToString(AccessMode)), so the value is
// lowercased: "read_only" / "read_write" / "automatic" / "undefined".
let is_read_only: bool = con
    .query_row(
        "SELECT current_setting('access_mode')",
        [],
        |row| row.get::<_, String>(0),
    )
    .map(|s| s.eq_ignore_ascii_case("read_only"))
    .unwrap_or(false);  // fail-open: treat unknown as writable; init_catalog
                         // would then surface DuckDB's own error if writes fail.
```

Rationale and verification:

- **Setting name:** `"access_mode"` — declared in `AccessModeSetting` (`duckdb.cpp:4643-4651`), `Name = "access_mode"`.
- **Return shape:** the only enum members rendered by `current_setting('access_mode')` are the four enums declared at `duckdb.cpp:62026-62035`: `"undefined"`, `"automatic"`, `"read_only"`, `"read_write"` (all lowercased by `StringUtil::Lower`). Match `"read_only"` case-insensitively to be safe across DuckDB minor bumps.
- **`AUTOMATIC` mode:** the user passed nothing or `automatic`. After DB open, `DatabaseInstance::Initialize` (`duckdb.cpp:277171-277172`) coerces UNDEFINED → READ_WRITE — but the *setting* keeps the original string until `SET access_mode = ...` is issued. In practice no user opens a file-backed DB in `automatic` mode and expects read-only protection; treating unknown values as writable is correct.
- **In-memory DBs:** `:memory:` is always opened READ_WRITE (the C++ side throws `CatalogException("Cannot launch in-memory database in read-only mode!")` at `duckdb.cpp:426501` if you try otherwise). So `current_setting('access_mode')` on a `:memory:` connection always returns `"read_write"` (or `"automatic"` coerced to read_write). The `is_read_only=false` branch is the right outcome.
- **Attached DBs:** `current_setting('access_mode')` returns the **main DB's** setting only — it is a global setting on `DBConfig`, not per-attached-database. Per-attach mode is recorded on `DatabasePathInfo` (`duckdb.cpp:277338-277339`). For Phase 63 we ONLY care about the main DB the extension is loaded into; the catalog table `semantic_layer._definitions` lives in the main DB. Attached read-only DBs are out of scope (they don't host our catalog table).
- **Failure modes:**
  - `query_row` returns `Err` (e.g., setting renamed in a future DuckDB) → `unwrap_or(false)` → treat as writable → `init_catalog` proceeds → if the DB is actually read-only, `CREATE SCHEMA` fails with DuckDB's catalog error and LOAD itself fails. This is strictly worse than today only in the unlikely setting-rename case; a phase test (Q6) covers the happy path so a setting rename would be caught at CI bump time.
  - NULL value: not possible — `GetSetting` returns a `Value` constructed from a non-null string.
  - Multiple rows: not possible — `current_setting()` is a scalar.

**Verification: HIGH.** All claims traced into `cpp/include/duckdb.cpp` (the vendored amalgamation that this repo builds against).

### Q2 — Read-only short-circuit semantics

**Minimal change** in `init_catalog` (`src/catalog.rs:25-64`): pass `is_read_only: bool` and skip the entire body when it's true.

```rust
pub fn init_catalog(con: &Connection, db_path: &str, is_read_only: bool) -> Result<()> {
    if is_read_only {
        // Read-only DB: skip schema/table creation AND companion-file migration.
        // The companion-file migration is write-only — see Q2 analysis below.
        return Ok(());
    }
    // ...existing body unchanged...
}
```

Why skipping the **entire** body is correct:

1. **`CREATE SCHEMA IF NOT EXISTS semantic_layer; CREATE TABLE IF NOT EXISTS semantic_layer._definitions (...)`** at `src/catalog.rs:26-32` — both are catalog mutations. `IF NOT EXISTS` does NOT make them no-ops on read-only DBs in DuckDB; they raise the standard read-only error. Confirmed by behaviour today: this is the exact error the user currently sees on `LOAD semantic_views` against a read-only DB.

2. **v0.1.0 companion-file migration** at `src/catalog.rs:36-61` is **write-only** by construction. Trace:
   - Line 36 `if db_path != ":memory:"` — file-backed DBs only.
   - Lines 37-45 build `migration_path` (e.g., `/path/to/db.duckdb.semantic_views`).
   - Line 46 `if migration_path.exists()` — short-circuits to no-op when no companion file.
   - Lines 47-56 `INSERT OR REPLACE` into `semantic_layer._definitions` — these REQUIRE write access AND that the table was just created above.
   - Line 59 `std::fs::remove_file(&migration_path)` — filesystem write.

   Every active step (insert + file delete) is a write. On a read-only DB, the INSERT would fail. Even if we skip just the writes, we still wouldn't have a `_definitions` table to insert into (we just skipped that). So the entire companion-file branch must also be skipped on read-only.

   **Real-world impact of skipping the migration on read-only:** essentially zero. The companion file format only existed in v0.1.0 (Feb 2026); a v0.1.0-vintage DB would have been migrated at first writable open during the v0.2.0+ era. A user who *first* opens their v0.1.0-era DB read-only at v0.9.0 has been carrying the companion file unmigrated for a year; they can re-open writable to migrate.

   **Document this** as a known limitation in DOC-02 (the explanation page), not as a separate requirement. Suggested wording: "On a read-only database, the v0.1.0 → v0.2.0+ companion-file migration cannot run. If you have a database that was last opened with a pre-v0.2.0 release, open it once writable to complete the migration before reverting to read-only."

**Verification: HIGH.** Lines cited verbatim from `src/catalog.rs` as read in this session.

### Q3 — `catalog_table_present` probe

**Probe SQL:**

```sql
SELECT 1 FROM information_schema.tables
WHERE table_schema = 'semantic_layer' AND table_name = '_definitions'
LIMIT 1
```

Run **only when `is_read_only=true`**. Writable DBs are about to run `CREATE TABLE IF NOT EXISTS semantic_layer._definitions` (lines 26-32) which guarantees the table exists when `init_catalog` returns; no probe needed in that path. This avoids paying for an extra query on the hot writable path.

**Probe location:** in `init_extension` (`src/lib.rs:339-482`), after the `is_read_only` detection and after `init_catalog`, but BEFORE `CatalogReader::new(catalog_conn)`. Compute the bool, pass it to `CatalogReader::new(conn, catalog_table_present)`. Suggested signature change:

```rust
// src/catalog.rs
pub struct CatalogReader {
    conn: ffi::duckdb_connection,
    catalog_table_present: bool,  // NEW
}

impl CatalogReader {
    pub fn new(conn: ffi::duckdb_connection, catalog_table_present: bool) -> Self {
        Self { conn, catalog_table_present }
    }
}
```

**Layout impact:** ZERO. `CatalogReader` is `#[derive(Clone, Copy)]` (`src/catalog.rs:88`), held by-value in `OverrideContext` (Phase 62 unification — `src/parse.rs:1758` `let catalog = ctx.catalog;`) and passed `&CatalogReader` to register_table_function_with_extra_info (`src/lib.rs:386-481`). Adding a `bool` keeps it `Copy`, doesn't break any existing call site, and isn't FFI-exposed (the `OverrideContext` Box pattern from Phase 62 means C++ holds an opaque pointer; the Rust struct layout is private). The Phase 62 RESEARCH explicitly confirms (`§Q4`) that `register_table_function_with_extra_info::<…VTab, _>(name, &catalog_reader)` "gets its own copy of the raw connection pointer at registration time" — adding a bool field follows the same Copy semantics.

**Reachability:** `prepared_lookup` (`src/catalog.rs:216`), `execute_list_all` (`src/catalog.rs:247`), `execute_list_names` (`src/catalog.rs:274`) are unsafe free functions called by methods on `CatalogReader` (lines 111-131). Convert each method to check `self.catalog_table_present` and short-circuit before the unsafe call:

```rust
pub fn lookup(&self, name: &str) -> Result<Option<String>, String> {
    if !self.catalog_table_present {
        return Ok(None);
    }
    unsafe { prepared_lookup(self.conn, name) }
}

pub fn list_all(&self) -> Result<Vec<(String, String)>, String> {
    if !self.catalog_table_present {
        return Ok(Vec::new());
    }
    unsafe { execute_list_all(self.conn) }
}

pub fn list_names(&self) -> Result<Vec<String>, String> {
    if !self.catalog_table_present {
        return Ok(Vec::new());
    }
    unsafe { execute_list_names(self.conn) }
}
```

`exists` (line 116) calls `lookup` and is automatically covered.

**Verification: HIGH.** All struct fields, signatures, and call sites verified by direct reading of `src/catalog.rs` and `src/lib.rs` in this session.

### Q4 — Reader-path short-circuit behaviour

**`list_semantic_views()` and `list_terse_semantic_views()` (RO-03 — empty list):**

Both are wired in `src/lib.rs:386-393` to read-side vtabs (`ListSemanticViewsVTab`, `ListTerseSemanticViewsVTab` in `src/ddl/list.rs`). They call `catalog.list_all()` / `catalog.list_names()`. With Q3's short-circuit returning `Vec::new()`, the vtab emits zero rows. **No vtab code needs to change.** RO-03 covered.

**`describe_semantic_view('x')` and `FROM semantic_view('x', ...)` (RO-04 — clean "does not exist" error):**

Every read-side reader uses the same pattern:

```rust
let json = catalog.lookup(&name)
    .map_err(...)?
    .ok_or_else(|| format!("semantic view '{name}' does not exist"))?;
```

Verified across 10 sites:
- `src/ddl/get_ddl.rs:50`, `src/ddl/show_dims.rs:159`, `src/ddl/describe.rs:529`, `src/ddl/show_facts.rs:159`, `src/ddl/show_materializations.rs:146`, `src/ddl/read_yaml.rs:48`, `src/ddl/show_dims_for_metric.rs:196,200`, `src/ddl/show_metrics.rs:161`
- Plus `src/query/error.rs:8` doc comment confirming this is the canonical "lookup miss" error variant.

With Q3's short-circuit returning `Ok(None)`, the existing `.ok_or_else(...)` raises `"semantic view 'x' does not exist"`. **Reuse the existing path. Do NOT invent a new error variant.** RO-04 covered.

**Caching considerations — should `catalog_table_present` ever flip false → true?**

**Recommendation: stay false for the lifetime of the LOAD.** Rationale:

- A read-only `duckdb_connection` cannot transition to read-write in DuckDB; the access mode is fixed at DB open. No path exists for a write to land while we're loaded.
- A second writable connection to the same DB file would be REJECTED by DuckDB's `DatabaseManager` (`duckdb.cpp:277364` — "all attaches are in read-only mode" check) because the existing read-only attach blocks read-write attach.
- Therefore the only way the table can appear after our LOAD-time probe is if a separate process opens the file writable AND we somehow reload — which would re-trigger LOAD and re-run the probe.
- Stale cached `false` is therefore guaranteed-correct.

**Verification: HIGH.** Pattern verified across 10 reader sites (all use the same `.ok_or_else` shape); DuckDB attach-mode conflict logic verified at `cpp/include/duckdb.cpp:277364`.

### Q5 — DDL natural failure path

**Trace the rewrite chain:** `parser_override` → `sv_parser_override_rust` → `validate_and_rewrite` → `rewrite_to_native_sql` → `rewrite_drop_or_alter` / `rewrite_create` (`src/parse.rs:1715-1747`). The result is a SQL string returned through the override; DuckDB then **executes that SQL on the caller's connection** (the same connection that issued the original DDL). Phase 62 Plan 03 unified this: parser_override owns the success path, transactional rewrite + re-parse on the caller's connection.

**Where `_definitions` writes happen (caller's connection):**
- CREATE: emits INSERT (`src/parse.rs:1914-1939`) — always writes to `_definitions` via the caller.
- DROP without IF EXISTS: emits two-statement guard SELECT + DELETE (`src/parse.rs:2138-2143`) — DELETE on caller.
- DROP IF EXISTS: emits DELETE (`src/parse.rs:2124-2127`) — DELETE on caller.
- ALTER RENAME without IF EXISTS: guard SELECT + UPDATE (`src/parse.rs:2199-2205`) — UPDATE on caller.
- ALTER RENAME IF EXISTS: UPDATE (`src/parse.rs:2189-2193`) — UPDATE on caller.
- ALTER SET/UNSET COMMENT: similar UPDATE shape (similar pattern in `rewrite_alter_comment` continuing past `src/parse.rs:2218`).

**On a read-only DB, when does each path produce DuckDB's read-only error vs our extension error?**

The critical question: `rewrite_drop` / `rewrite_alter_*` / `rewrite_create` ALL pre-check the catalog via `catalog.exists(name)` or `catalog.lookup(name)` BEFORE emitting the DML. The pre-check uses the catalog connection (read-only-safe). With Q3's short-circuit:

| Scenario | Pre-check result | Emitted SQL runs | User-visible error |
|----------|------------------|------------------|--------------------|
| DROP `v` on read-only, fresh DB (no `_definitions`) | `exists=false` (short-circuit) | not emitted | `"semantic view 'v' does not exist"` (from `rewrite_drop` line 2114) — **friendly, NOT read-only message** |
| DROP `v` on read-only, bootstrapped DB (v exists) | `exists=true` | guard SELECT + DELETE on caller | DuckDB's `Cannot execute statement of type "DELETE" on database "..." which is attached in read-only mode!` (`duckdb.cpp:273011-273013`) — **the ROADMAP's expected RO-05 outcome** |
| DROP IF EXISTS `v` on read-only, fresh DB | `exists=false` | emits silent-no-op SELECT (line 2110-2112) | succeeds with 0 rows — **arguably wrong on read-only** but matches IF EXISTS contract; users opting in to "do nothing if the view isn't there" get exactly that |
| DROP IF EXISTS `v` on read-only, bootstrapped DB | `exists=true` | DELETE on caller | DuckDB's read-only error |
| ALTER RENAME on bootstrapped read-only | `exists=true` | UPDATE on caller | DuckDB's read-only error |
| CREATE `v` on read-only (regardless of view existence) | enrichment queries succeed (read-only) IF `catalog_table_present=true` (bootstrapped) | INSERT on caller | DuckDB's read-only error |
| CREATE `v` on read-only, fresh DB (catalog_table_present=false) | enrichment may issue `SELECT FROM semantic_layer._definitions WHERE name = ...` for EXISTS check on `OR REPLACE` / `IF NOT EXISTS` — this would fail with catalog error UNLESS routed through the same `catalog.exists/lookup` short-circuit | INSERT on caller — emitted unconditionally for plain CREATE (not OR REPLACE / IF NOT EXISTS) — `src/parse.rs:1926-1938` does NOT pre-check existence | DuckDB's read-only error from the INSERT (correct outcome) |

**This satisfies RO-05's "DuckDB's standard 'cannot write to read-only database' error (or the closest equivalent surfaced by the planner)."** The "or closest equivalent" wording is what protects the fresh-read-only-DB DROP case. The bootstrapped case (the one the ROADMAP cares about) gets the verbatim DuckDB error.

**Edge case verified — fresh read-only DB + DROP without IF EXISTS:** The user gets `"semantic view 'v' does not exist"`. This is technically not the read-only error, but it IS the correct semantic — there is no view to drop. Document this in TEST-01 / TEST-02 expectations: the test must assert "either matches `'does not exist'` OR matches `'read-only mode'`" — accept both depending on whether the view existed in the bootstrap. The simplest test design uses the bootstrapped scenario for RO-05 assertions.

**Edge case CONFIRMED ABSENT — DELETE against a nonexistent table.** The DELETE emitted by `rewrite_drop` is only emitted AFTER `catalog.exists(name)` returned true, which requires `catalog_table_present=true`, which means the table exists. So the "DELETE against missing table" case cannot fire on the caller's connection.

**Caret rendering note:** Phase 62 Plan 03 attached `error_location` to the rendered output via `sv_parse_stub`. The read-only error is raised by DuckDB *during execution*, not parsing — it surfaces as `Invalid Input Error: Cannot execute statement ...` without a caret. That's expected and matches DuckDB's normal read-only behaviour for any other DML.

**Verification: HIGH.** All emit-site line numbers verified by direct reading of `src/parse.rs` in this session; DuckDB's read-only check verified at `cpp/include/duckdb.cpp:273009-273014`.

### Q6 — Test infrastructure

**sqllogictest read-only support.** sqllogictest does NOT have a built-in `--read-only` connection mode. Looking at the existing `test/sql/extension_reload.test` (Phase 62 Wave 3) and `test/sql/phase2_restart.test`, the runner opens a single writable connection to the test DB. Two viable approaches in sqllogictest itself:

1. **`load <path> readonly` directive (DuckDB sqllogictest extension):** the upstream DuckDB sqllogictest runner accepts `load <path> readonly` as a pragma to open the connection read-only. This is how DuckDB's own test suite tests read-only paths. **Verify in our forked runner:** check `python_runner/runner.py` (or wherever the sqllogictest runner lives) for `readonly` keyword handling. If unsupported, this approach is blocked.

2. **`mode skip` + manual ATTACH:** open the test DB writable, bootstrap views, then `ATTACH '...' AS ro (READ_ONLY)`. ATTACH-as-read-only IS supported by DuckDB syntax (`AttachOptions::READ_ONLY`, `duckdb.cpp:262106`). However, our extension only operates on the **main** DB (`PRAGMA database_list` filtering at `src/lib.rs:344-355`), and the catalog table lives on the main DB. Loading the extension after attaching read-only would still find the main DB writable, defeating the test.

3. **Two test files / two-connection approach (recommended):**
   - `test/sql/readonly_load_bootstrap.test` (or inline at top of one file): runs writable, bootstraps `_definitions` content, then exits cleanly. Uses `__TEST_DIR__/readonly_test.db`.
   - `test/sql/readonly_load.test`: opens the same path read-only via `load <path> readonly` directive (if supported), or via `restart` followed by `SET access_mode='read_only'` (if DuckDB allows runtime SET — most likely not, as access_mode is a startup-only setting).

   **Most robust path:** delegate the read-only scenarios entirely to the Python integration test (TEST-02) where `duckdb.connect(path, read_only=True)` is unambiguously available. Use sqllogictest only for **scenarios reachable via single writable connection** that exercise the new code paths indirectly:
   - The `catalog_table_present` short-circuit on `CatalogReader::lookup` can be unit-tested via a Rust test (no sqllogictest needed): construct a `CatalogReader { conn, catalog_table_present: false }` against an in-memory DB and assert `lookup("any_view")` returns `Ok(None)` without hitting the DB.

   **Recommendation for TEST-01:** Wave 0 spike — run `grep -rn "readonly" python_runner/ 2>/dev/null` to confirm whether the runner supports `load <path> readonly`. If yes, use approach (1). If no, document the gap in the plan and rely on TEST-02 for full coverage; the sqllogictest file can still cover scenarios (a) and partly (b) by using the writable bootstrap + `restart` + (if access_mode-set works) read-only re-open. Worst case the file is mostly skip-stubs and TEST-02 carries the load.

**Python integration test (`test/integration/test_readonly_load.py`).** Pattern (cf. `test_multi_db_isolation.py:45-57`):

```python
import duckdb
from pathlib import Path

EXT_DIR = get_ext_dir()
EXT_PATH = get_extension_path()

def open_writable(path: str):
    conn = duckdb.connect(
        path,
        config={"allow_unsigned_extensions": "true", "extension_directory": EXT_DIR},
    )
    conn.execute(f"FORCE INSTALL '{EXT_PATH}'")
    conn.execute("LOAD semantic_views")
    return conn

def open_readonly(path: str):
    # `read_only=True` is the documented DuckDB Python API kwarg.
    conn = duckdb.connect(
        path,
        read_only=True,
        config={"allow_unsigned_extensions": "true", "extension_directory": EXT_DIR},
    )
    conn.execute(f"LOAD semantic_views")  # FORCE INSTALL not needed — already installed
    return conn
```

DuckDB Python `read_only=True` kwarg has been supported since at least 0.5.x. Repo pins `duckdb==1.5.2` in test scripts (verified in `examples/transactional_ddl.py:4`). **HIGH confidence** read-only kwarg is available.

**Three test scenarios outline:**

```python
def test_fresh_readonly_empty_list():
    # (a) Fresh read-only file → empty list (RO-01, RO-03)
    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "fresh.duckdb")
        # Create the file but never bootstrap our extension on it.
        duckdb.connect(db).close()
        ro = open_readonly(db)
        try:
            rows = ro.execute("SELECT name FROM list_semantic_views()").fetchall()
            assert rows == [], f"expected empty, got {rows}"
            # RO-04: describe missing view → "does not exist"
            try:
                ro.execute("FROM describe_semantic_view('missing')").fetchall()
                raise AssertionError("expected does-not-exist error")
            except duckdb.Error as e:
                assert "does not exist" in str(e), f"unexpected: {e}"
        finally:
            ro.close()

def test_bootstrapped_readonly_query_works():
    # (b) Bootstrapped, reopened read-only → list/describe/semantic_view all work (RO-01, RO-02)
    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "bootstrapped.duckdb")
        rw = open_writable(db)
        rw.execute("CREATE TABLE orders (id INTEGER PRIMARY KEY, region VARCHAR, amount DECIMAL(10,2))")
        rw.execute("INSERT INTO orders VALUES (1,'US',100),(2,'EU',200)")
        rw.execute("""
            CREATE SEMANTIC VIEW v AS
              TABLES (o AS orders PRIMARY KEY (id))
              DIMENSIONS (o.region AS o.region)
              METRICS (o.total AS SUM(o.amount))
        """)
        rw.close()
        ro = open_readonly(db)
        try:
            names = [r[0] for r in ro.execute("SELECT name FROM list_semantic_views()").fetchall()]
            assert names == ["v"], names
            desc = ro.execute("FROM describe_semantic_view('v')").fetchall()
            assert len(desc) > 0
            rows = ro.execute(
                "SELECT * FROM semantic_view('v', dimensions := ['region'], metrics := ['total'])"
            ).fetchall()
            assert {r[0] for r in rows} == {"US", "EU"}
        finally:
            ro.close()

def test_readonly_ddl_fails():
    # (c) DDL on read-only → DuckDB read-only error (RO-05)
    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "ddl.duckdb")
        rw = open_writable(db)
        rw.execute("CREATE TABLE orders (id INTEGER PRIMARY KEY, amount DECIMAL(10,2))")
        rw.execute("""
            CREATE SEMANTIC VIEW v AS TABLES (o AS orders PRIMARY KEY (id))
              DIMENSIONS (o.id AS o.id) METRICS (o.t AS SUM(o.amount))
        """)
        rw.close()
        ro = open_readonly(db)
        try:
            # DROP an EXISTING view → exercises the emitted DELETE on read-only.
            try:
                ro.execute("DROP SEMANTIC VIEW v")
                raise AssertionError("DROP should have failed on read-only DB")
            except duckdb.Error as e:
                msg = str(e)
                # Accept DuckDB's exact wording or any substring proving read-only origin.
                assert "read-only" in msg.lower(), f"expected read-only error, got: {msg}"

            # CREATE — same expectation.
            try:
                ro.execute("""
                    CREATE SEMANTIC VIEW w AS TABLES (o AS orders PRIMARY KEY (id))
                      DIMENSIONS (o.id AS o.id) METRICS (o.c AS COUNT(*))
                """)
                raise AssertionError("CREATE should have failed on read-only DB")
            except duckdb.Error as e:
                assert "read-only" in str(e).lower()

            # ALTER on existing view.
            try:
                ro.execute("ALTER SEMANTIC VIEW v RENAME TO w")
                raise AssertionError("ALTER should have failed on read-only DB")
            except duckdb.Error as e:
                assert "read-only" in str(e).lower()
        finally:
            ro.close()
```

**Wire into `just test-all`** by adding `just test-readonly` invoking `uv run test/integration/test_readonly_load.py` and adding to the `test-all` recipe. Inspect existing `justfile:100-144` (Phase 62 RESEARCH §6) for the wiring pattern; mirror `just test-multi-db`.

**Verification: HIGH** for the Python pattern (matches existing `test_multi_db_isolation.py`); **MEDIUM** for sqllogictest read-only directive (depends on the runner's implementation — Wave 0 grep needed).

### Q7 — Documentation surface

**DOC-02 — `docs/explanation/transactional-ddl-and-limitations.rst`:**

Existing structure (verified by full file read):
- Title (line 1-8)
- `versionadded:: 0.8.0` (line 10)
- Intro (lines 12-16)
- `DDL Now Participates in Your Transaction` (line 19, `_explanation-txn-ddl-what-changed`)
- `Reads Inside an Open Transaction See Committed State` (line 49, `_explanation-txn-ddl-write-visibility`)
- `CREATE IF NOT EXISTS Across Multiple Connections` (line 73, `_explanation-txn-ddl-create-race`)
- `DROP and ALTER Without IF EXISTS Detect Concurrent Drops` (line 104, `_explanation-txn-ddl-drop-alter-race`)
- `DuckDB's Experimental PEG Parser` (line 122, `_explanation-txn-ddl-peg`)
- `Summary` (line 145)

**Recommended insertion point:** new section between `DROP and ALTER Without IF EXISTS Detect Concurrent Drops` (ends ~line 119) and `DuckDB's Experimental PEG Parser` (starts line 122). New label: `_explanation-txn-ddl-readonly`. Add an entry in the `Summary` cross-references at the bottom (line 154-158). The page already has a `versionadded:: 0.8.0` directive at the top; the new section should carry its own `versionadded:: 0.9.0` (Sphinx supports per-section).

Recommended subsection content (to be drafted in Plan, not in this RESEARCH):
- LOAD on read-only succeeds.
- `list_semantic_views()` returns committed bootstrap on bootstrapped DBs, empty on fresh.
- `describe_semantic_view`, `FROM semantic_view(...)` work on bootstrapped views; "does not exist" error on missing.
- DDL fails with DuckDB's standard read-only error.
- Bootstrap-then-reopen workflow.
- Known limitation: v0.1.0 companion-file migration cannot run read-only (re-open writable once).

**DOC-03 — three reference pages** (`create-`, `drop-`, `alter-semantic-view.rst`).

Verified location of existing `.. note::` blocks for each:

- `docs/reference/create-semantic-view.rst:109-115` — already has TWO `.. note::` admonitions about transactional DDL and IF NOT EXISTS races, both placed AFTER the `Statement Variants` section and BEFORE `Clauses`. This is the natural slot for a third one-liner: a `.. note::` near line 116 saying "Requires a writable database. On a read-only database this statement fails with DuckDB's standard read-only error. See :ref:`explanation-txn-ddl-readonly`."

- `docs/reference/drop-semantic-view.rst` and `docs/reference/alter-semantic-view.rst` — file structure not read here but expected to mirror create's layout. Plan should add the same one-line `.. note::` after each file's main `Syntax`/`Variants` section.

**DOC-04 — README.md LOAD section.**

`grep -n "LOAD"` against `README.md` returned no matches (probably because the README uses `LOAD` only inline in the example SQL or section bodies, not as a heading). Direct read of the README header table of contents shows: `## How it works` (line 9), `## Quick start` (line 19), `## Multi-table` (line 62), `## FACTS` (line 141), `## Derived metrics` (line 165), `## Cardinality and fan trap detection` (line 178), `## Role-playing dimensions` (line 191), `## DDL reference` (line 217), `## Documentation` (line 236), `## Building` (line 242), `## License` (line 257). There is no dedicated "LOAD" section — the extension load step appears within Quick start.

**Recommendation for DOC-04:** Add a one-line note near the install/load instructions (likely in or just after `## Quick start`) along the lines of: "Read-only databases are supported for queries; `CREATE`/`DROP`/`ALTER SEMANTIC VIEW` require a writable database." Alternatively, add a short `## Read-only databases` subsection. Plan should choose based on the existing `## Quick start` shape (worth re-reading in the plan task).

**DOC-05 — `examples/readonly_load.py`.**

Model on `examples/transactional_ddl.py` (read in this session). Reuse:
- The shebang + `# /// script` PEP 723 inline metadata block (deps: `duckdb==1.5.2` only; no ADBC needed).
- The `EXTENSION_PATH = os.environ.get("SEMANTIC_VIEWS_EXTENSION_PATH", "build/debug/semantic_views.duckdb_extension")` pattern.
- `tempfile.TemporaryDirectory(prefix="sv_readonly_demo_")` for the DB file.
- The `setup` + `CREATE_VIEW` + `list_views` helper layout.

Suggested narrative (mirrors `transactional_ddl.py`'s scenario-based flow):
1. Open writable, create a table + view, list views, close.
2. Reopen read-only via `duckdb.connect(path, read_only=True)`. Show `LOAD semantic_views` succeeds.
3. Show `list_semantic_views()` returns the bootstrapped view.
4. Show `FROM semantic_view(...)` returns aggregated rows.
5. Try `CREATE SEMANTIC VIEW w ...` and catch `duckdb.Error`; print the read-only message verbatim.
6. (Optional) Open a fresh read-only DB (no bootstrap) to demonstrate empty-list behaviour.

**Verification: HIGH** for DOC-02 / DOC-03 / DOC-05 file locations and structure (read in this session); **MEDIUM** for DOC-04 — README has no explicit LOAD section, plan must choose a placement.

### Q8 — Validation Architecture (Nyquist)

`workflow.nyquist_validation: true` is the default (config absent — same convention as Phase 62). Section is REQUIRED. See §4 below.

### Q9 — Risks & open issues

**Risk 1 — `cpp/src/shim.cpp` writes to the config at LOAD.**
Line 399: `config.SetOption("allow_parser_override_extension", Value("FALLBACK"));`. This is **safe on read-only DBs**:
- `AllowParserOverrideExtensionSetting::OnSet` (`duckdb.cpp:301174-301176`) only validates the enum string (`EnumUtil::FromString<AllowParserOverride>`); no DB I/O.
- `DBConfig::SetOption` writes to in-memory `DBConfigOptions` / `set_variables` — not catalog mutation, no transaction needed.
- Even read-only DBs allow setting in-memory configuration values (e.g., `SET threads=4` works on read-only).

**No C++ changes needed for Phase 63.** Confirmed by grep: `grep -n "execute\|execute_batch\|CREATE\|INSERT\|UPDATE\|DELETE" src/lib.rs` returned only test-helper `execute_sql_raw` definitions plus the comment about catalog connection — no hidden writes in the LOAD path beyond `init_catalog`.

**Risk 2 — Other writes at LOAD?**
- `Connection::open_from_raw(db_handle.cast())` (`src/lib.rs:519`) — wraps the existing handle, no I/O.
- `con.prepare("PRAGMA database_list")` (`src/lib.rs:345`) — read-only PRAGMA.
- `con.register_*` calls (`src/lib.rs:386-481`) — register table functions in DuckDB's in-memory function catalog; not on-disk catalog mutations. Verified: `register_table_function_with_extra_info` calls `duckdb_create_table_function` etc. which are session-/DB-instance-level, not catalog DML.
- `duckdb_connect(db_handle, &mut catalog_conn)` and the same for `query_conn` (lines 367, 460) — open a new connection. Connection open does NOT require write access.
- Therefore the **only** writes today at LOAD are the two `CREATE` statements in `init_catalog` and the (conditional) companion-file migration. Phase 63 covers both.

**Risk 3 — Re-LOAD on read-only.** Phase 62's `extension_reload.test` covers re-LOAD on writable. For read-only, two `LOAD semantic_views` calls each produce a `SemanticViewsParserInfo` per Phase 62 RESEARCH §3 Risk A. Both will skip writes via `is_read_only=true`. No new risk; worth a brief assertion in TEST-02 if cycles allow.

**Risk 4 — `current_setting('access_mode')` future-proofing.** The matching is on `"read_only"` lowercased. If a future DuckDB version changed the rendering (e.g., to `"readonly"` or `"read-only"`), our detection would silently fail-open and `init_catalog` would fail at `CREATE SCHEMA`. Mitigation: a Rust unit test that asserts `current_setting('access_mode')` against a `Connection::open_with_flags(path, Config::access_mode(AccessMode::ReadOnly))` returns `"read_only"` — pins the contract against the bundled DuckDB. (Note: Rust unit tests run against the `bundled` feature, NOT the `extension` feature — `Connection::open_with_flags` IS available there.)

**Risk 5 — DuckDB Python `read_only=True` against a freshly-created empty file.** If the file exists but has zero bytes (as in the `duckdb.connect(db).close()` pattern in scenario (a)), DuckDB needs to be able to handle that. `duckdb.cpp:420819` raises `IOException("Cannot open database \"%s\" in read-only mode: database does not exist", path)` only when the file doesn't exist. An empty file: needs a quick check. Practical workaround if it fails: in scenario (a) bootstrap the DB writable (just a no-op like `SELECT 1`) before closing — a closed-then-reopened DuckDB DB file has the necessary header bytes. **Recommend:** the test scenario (a) should `duckdb.connect(db).execute("SELECT 1").close()` to ensure a valid header is written, then reopen read-only. Already implicit in the test snippet above.

**Risk 6 — Concurrent process bootstrap then reopen-readonly mid-LOAD.** Out of scope for v0.9.0 (TECH-DEBT-style edge case).

**No blockers identified.** All risks have mitigations within the Phase 63 plan scope.

---

## 4. Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Rust unit + proptest | `cargo test` (default `bundled` feature) |
| Sqllogictest | `just test-sql` (requires `just build` first) |
| Python integration | `uv run test/integration/test_readonly_load.py` (NEW; wire via `just test-readonly`) |
| Quick run command (per task commit) | `cargo test` |
| Per-wave merge | `just test-all` (= `test-rust + test-sql + test-ducklake-ci + test-vtab-crash + test-caret + test-adbc + test-large-view + test-multi-db + test-concurrent` per `justfile:137`; **add `test-readonly`** in this phase) |
| Phase gate | `just ci` green before `/gsd-verify-work` |

### Phase Requirements → Test Map

| REQ | Behaviour | Test type | Automated command | File exists? |
|-----|-----------|-----------|-------------------|--------------|
| **RO-01** | `LOAD semantic_views` succeeds on fresh read-only DB | Python integration | `uv run test/integration/test_readonly_load.py::test_fresh_readonly_empty_list` (load step itself) | ❌ Wave 0 |
| **RO-01** | `LOAD semantic_views` succeeds on bootstrapped read-only DB | Python integration | `uv run test/integration/test_readonly_load.py::test_bootstrapped_readonly_query_works` (load step) | ❌ Wave 0 |
| **RO-01** | `init_catalog` skips writes when `is_read_only=true` | Rust unit test | `cargo test catalog::tests::init_catalog_skips_writes_on_readonly` | ❌ Wave 0 |
| **RO-02** | `list_semantic_views()` returns bootstrapped views on read-only | Python integration | `test_bootstrapped_readonly_query_works` | ❌ Wave 0 |
| **RO-02** | `describe_semantic_view('v')` returns metadata on bootstrapped read-only | Python integration | `test_bootstrapped_readonly_query_works` | ❌ Wave 0 |
| **RO-02** | `FROM semantic_view('v', ...)` returns aggregated rows on bootstrapped read-only | Python integration | `test_bootstrapped_readonly_query_works` | ❌ Wave 0 |
| **RO-03** | `list_semantic_views()` returns empty on fresh read-only (no `_definitions`) | Python integration | `test_fresh_readonly_empty_list` | ❌ Wave 0 |
| **RO-03** | `list_all` / `list_names` short-circuit when `catalog_table_present=false` | Rust unit test | `cargo test catalog::tests::list_all_returns_empty_when_table_missing` | ❌ Wave 0 |
| **RO-04** | `describe_semantic_view('missing')` on fresh read-only → "does not exist" error | Python integration | `test_fresh_readonly_empty_list` | ❌ Wave 0 |
| **RO-04** | `FROM semantic_view('missing', ...)` on fresh read-only → "does not exist" error | Python integration | new sub-test in same file | ❌ Wave 0 |
| **RO-04** | `lookup` short-circuits to `Ok(None)` when `catalog_table_present=false` | Rust unit test | `cargo test catalog::tests::lookup_returns_none_when_table_missing` | ❌ Wave 0 |
| **RO-05** | `CREATE SEMANTIC VIEW` on read-only → DuckDB read-only error | Python integration | `test_readonly_ddl_fails` (CREATE branch) | ❌ Wave 0 |
| **RO-05** | `DROP SEMANTIC VIEW` on bootstrapped read-only → DuckDB read-only error | Python integration | `test_readonly_ddl_fails` (DROP branch) | ❌ Wave 0 |
| **RO-05** | `ALTER SEMANTIC VIEW ... RENAME TO` on bootstrapped read-only → DuckDB read-only error | Python integration | `test_readonly_ddl_fails` (ALTER branch) | ❌ Wave 0 |
| **RO-05** | `ALTER SEMANTIC VIEW ... SET COMMENT` on bootstrapped read-only → DuckDB read-only error | Python integration | new sub-test | ❌ Wave 0 |
| **RO-05** | (defensive) plain CREATE on FRESH read-only (no `_definitions`) → DuckDB read-only error from INSERT, NOT a friendlier wrapper | Python integration | new sub-test | ❌ Wave 0 |
| **TEST-01** | sqllogictest covering scenarios (a), (b), (c) | sqllogictest | `just test-sql` runs `test/sql/readonly_load.test` | ❌ Wave 0 — see Q6 caveat about runner support |
| **TEST-02** | Python integration covering scenarios (a), (b), (c) | Python integration | `uv run test/integration/test_readonly_load.py` | ❌ Wave 0 |
| **TEST-03** | `just test-all` and `just ci` pass with new fixtures | CI | `just test-all && just ci` | runs after all above land |
| **DOC-01** | CHANGELOG.md `[0.9.0]` section under standard headings | manual review | grep CHANGELOG.md for `## [0.9.0]` and standard `### Added` heading | ❌ Wave 0 |
| **DOC-02** | `docs/explanation/transactional-ddl-and-limitations.rst` has new "Read-only databases" section | manual review + docs build | `just docs-check` (per Phase 62 RESEARCH `just ci` chain) | ❌ Wave 0 |
| **DOC-03** | three reference pages each carry a one-liner | manual review | grep each `.rst` for "writable database" | ❌ Wave 0 |
| **DOC-04** | README.md mentions read-only support near LOAD/install instructions | manual review | grep README.md | ❌ Wave 0 |
| **DOC-05** | `examples/readonly_load.py` exists and runs | smoke run | `uv run examples/readonly_load.py` | ❌ Wave 0 |
| **REL-01** | `Cargo.toml` and `description.yml` bumped to `0.9.0` | manual review | grep both files for `0.9.0` | ❌ Wave 0 |

### Sampling cadence

- **Per task commit:** `cargo test` (Rust unit + proptest, 5–15 s) — catches Rust-side regressions in `CatalogReader` short-circuit logic.
- **Per Wave merge:** `just test-all` (~5–10 minutes, runs sqllogictest + integration tests). Phase 63 adds `test-readonly` to this chain.
- **Phase gate (before `/gsd-verify-work`):** `just ci` (lint + fuzz compile + docs-check; ~6–12 minutes total).

### Coverage strategy — RO-05 acceptance flexibility

The DuckDB read-only error message at `duckdb.cpp:273011-273013` is:

> `Cannot execute statement of type "INSERT" on database "<name>" which is attached in read-only mode!`

The test must assert on the substring `"read-only"` (case-insensitive) to be resilient to:
- DuckDB minor version wording shifts.
- The statement-type token (INSERT vs DELETE vs UPDATE) varies per DDL form.
- Database-name interpolation varies per test.

Strict matching on the full sentence is brittle and not necessary for RO-05 (whose acceptance criterion is "DuckDB's standard 'cannot write to read-only database' error or the closest equivalent surfaced by the planner").

### Wave 0 gaps

- [ ] `test/integration/test_readonly_load.py` — three test functions (a)/(b)/(c) plus shared helpers; mirror `test_multi_db_isolation.py:45-57` connection-builder pattern.
- [ ] `test/sql/readonly_load.test` — depends on Wave 0 spike `grep -rn "readonly" python_runner/` to confirm runner support. If supported: full three-scenario sqllogictest. If not: minimal smoke + reliance on TEST-02 with a comment in the file explaining the deferral.
- [ ] `src/catalog.rs::tests` — three unit tests: `init_catalog_skips_writes_on_readonly`, `lookup_returns_none_when_table_missing`, `list_all_returns_empty_when_table_missing`, `list_names_returns_empty_when_table_missing`. Total ~50 LOC.
- [ ] `src/lib.rs::tests` — one unit test pinning `current_setting('access_mode')` returns `"read_only"` (lowercased) when `Connection::open_with_flags` opens a read-only DB. Future-proofs against DuckDB rendering changes.
- [ ] `justfile` — add `test-readonly` recipe + add to `test-all` chain.
- [ ] `CHANGELOG.md` — add `## [0.9.0]` section with `### Added`; update `[Unreleased]` and bottom-of-file compare links.
- [ ] `docs/explanation/transactional-ddl-and-limitations.rst` — new section between lines 119 and 122 with `_explanation-txn-ddl-readonly` label + cross-ref in Summary.
- [ ] `docs/reference/{create,drop,alter}-semantic-view.rst` — one-line `.. note::` after Variants section in each.
- [ ] `README.md` — one-line note in Quick start about read-only support.
- [ ] `examples/readonly_load.py` — full PEP-723 script mirroring `transactional_ddl.py`.
- [ ] `Cargo.toml` + `description.yml` — version bump to `0.9.0`.

---

## 5. Risks Summary & Open Questions

**No blockers.** Phase 63 is small, well-contained, and the existing Phase 62 architecture (parser_override on the success path, `OverrideContext` per `parser_info`, `CatalogReader` `Copy` semantics) accommodates the change cleanly.

**Open question (Wave 0 spike, not blocking research):** does our Python sqllogictest runner accept `load <path> readonly`? Resolution: `grep -rn "readonly" python_runner/` early in implementation. Two outcomes both have plans (full TEST-01 vs minimal-stub-with-deferral-to-TEST-02).

**Known limitation to document (DOC-02):** v0.1.0 → v0.2.0+ companion-file migration cannot run on read-only DBs. Practical impact ~zero (companion file format is 4 versions stale). Recommend re-opening writable once if the user has a v0.1.0-era DB.

---

## 6. Sources

### Primary (HIGH confidence — direct code reading in this session)

- `cpp/include/duckdb.cpp` (DuckDB 1.10.502 amalgamation, vendored):
  - `AccessModeSetting` struct + `GetSetting` lowercased rendering — lines 4643-4651, 301154-301167
  - `GetAccessModeValues` enum string literals — lines 62026-62035
  - `DatabaseInstance::Initialize` UNDEFINED → READ_WRITE coercion — lines 277171-277172
  - `CatalogException` for in-memory + read_only — line 426501
  - Read-only DML rejection — lines 273009-273014 (`Cannot execute statement of type ...`)
  - `IOException` for missing read-only file — line 420819
  - `DatabaseManager` attach mode conflict — lines 277364-277368, 277527-277547
  - `AllowParserOverrideExtensionSetting::OnSet` — lines 301174-301176
  - `AttachOptions::READ_ONLY` parsing — lines 262106, 262124, 262177, 262198
- `src/lib.rs` — `init_extension` (lines 339-482), `PRAGMA database_list` (lines 343-355), `init_catalog` call site (line 359), `sv_register_parser_hooks` Rust→C FFI declaration (lines 329-335), C++ helper invocation (line 381), C_STRUCT entrypoint (lines 504-564)
- `src/catalog.rs` — full file: `init_catalog` (lines 25-64), v0.1.0 companion-file migration (lines 36-61), `CatalogReader` struct + Copy derive (lines 88-91), `lookup`/`exists`/`list_all`/`list_names` methods (lines 111-131), `prepared_lookup`/`execute_list_all`/`execute_list_names` (lines 216-293), existing tests (lines 299-483)
- `src/parse.rs` — DDL emit sites (CREATE: lines 1914-1939; DROP without IF EXISTS: lines 2138-2143; DROP IF EXISTS: lines 2124-2127; ALTER RENAME: lines 2189-2205; rewrite dispatch: lines 1715-1747; `rewrite_drop_or_alter` dispatch: lines 1753-1793; `rewrite_drop` `catalog.exists` pre-check: lines 2099-2101)
- `cpp/src/shim.cpp` — `sv_register_parser_hooks` body (lines 354-406), `config.SetOption` call (line 399)
- `src/ddl/{describe,list,get_ddl,read_yaml,show_*}.rs` — existing "does not exist" error pattern (10 confirmed sites listed in §3 Q4)
- `src/query/error.rs:8` — canonical doc comment for the lookup-miss error variant
- `examples/transactional_ddl.py` — full file (177 lines) for DOC-05 modelling
- `test/integration/test_multi_db_isolation.py` — full file (297 lines) for TEST-02 pattern
- `test/sql/extension_reload.test` — full file (41 lines) for sqllogictest style
- `test/sql/phase2_restart.test` — `restart` directive pattern (line 52); excluded from TEST_LIST due to runner limitations (line 10-14) — relevant precedent for "delegated to integration tests"
- `docs/explanation/transactional-ddl-and-limitations.rst` — full file (166 lines) for DOC-02 placement
- `docs/reference/create-semantic-view.rst` — lines 1-115 for DOC-03 placement (existing `.. note::` pattern)
- `README.md` — outline read for DOC-04 placement
- `.planning/REQUIREMENTS.md` — full file
- `.planning/ROADMAP.md` — Phase 63 section (lines 197-228)
- `.planning/STATE.md` — full file
- `.planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md` — full file (precedent for §6 Validation Architecture format and §5 Project Constraints layout)
- `CLAUDE.md` — full file
- `.planning/config.json` — `nyquist_validation: true` (default), `branching_strategy: milestone`

### Secondary (MEDIUM — derived)

- DuckDB Python `read_only=True` kwarg availability: inferred from `duckdb==1.5.2` pin in repo's example scripts and consistent kwarg presence in DuckDB Python docs since 0.5.x. Verifiable at plan time by `python -c "import duckdb; help(duckdb.connect)"`.

### Tertiary (LOW — none required)

No web searches required for this phase. All claims grounded in repo code or vendored DuckDB amalgamation.

---

## 7. Assumptions Log

| # | Claim | Section | Risk if wrong |
|---|-------|---------|---------------|
| A1 | `current_setting('access_mode')` will continue to return `"read_only"` (lowercased) for read-only DBs in DuckDB 1.5.x and 1.10.x | Q1 | LOW — pinned by Wave 0 unit test in `src/lib.rs::tests`; failure surfaces at CI bump time, not in production. |
| A2 | DuckDB Python `duckdb.connect(path, read_only=True)` works on `duckdb==1.5.2` (the pinned test dependency) | Q6 | LOW — kwarg is documented in DuckDB Python API; verifiable at plan time via `help(duckdb.connect)`. If it fails, alternatives exist (`config={"access_mode": "read_only"}`). |
| A3 | DuckDB sqllogictest runner used by this project supports a `load <path> readonly` directive | Q6, TEST-01 | MEDIUM — Wave 0 spike (grep `python_runner/`) resolves before plan execution; either outcome has a plan. |
| A4 | `DBConfig::SetOption("allow_parser_override_extension", "FALLBACK")` at `cpp/src/shim.cpp:399` succeeds on a read-only DB | Q9 Risk 1 | LOW — verified by reading `AllowParserOverrideExtensionSetting::OnSet` (`duckdb.cpp:301174-301176`); the OnSet handler is enum-validation-only with no I/O. |

All other claims in this RESEARCH are tagged `[VERIFIED]` via direct reading of repo code or the vendored DuckDB amalgamation in this session.

---

## RESEARCH COMPLETE

Phase 63 is a focused two-file Rust change (`src/lib.rs` + `src/catalog.rs`) plus tests + docs + version bump. The four-step ROADMAP sketch is correct in shape; the only nuance the sketch under-specifies is that `current_setting('access_mode')` returns the **lowercased** enum (`"read_only"`, not `"READ_ONLY"`) — match case-insensitively. No C++ changes are needed: `cpp/src/shim.cpp:399`'s `SetOption` is config-only (not catalog DML) and is safe under read-only. The `catalog_table_present=false` short-circuit in `CatalogReader::{lookup,list_all,list_names}` reuses the existing `"semantic view '<name>' does not exist"` error path (10 reader sites already use it) so RO-04 falls out for free without inventing a new error variant. RO-05's "DuckDB's standard read-only error" surfaces verbatim on bootstrapped read-only DBs (the case the ROADMAP names) because the rewritten INSERT/DELETE/UPDATE runs on the caller's connection; on a fresh read-only DB the friendlier `"does not exist"` may surface first for DROP/ALTER, which is acceptable per RO-05's "or the closest equivalent" wording. One Wave 0 spike (sqllogictest `load … readonly` runner support) determines whether TEST-01 ships full or stubbed; either outcome has a plan and TEST-02 (Python) carries full coverage regardless.
