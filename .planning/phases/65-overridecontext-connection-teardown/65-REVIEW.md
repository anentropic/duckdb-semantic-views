---
phase: 65-overridecontext-connection-teardown
reviewed: 2026-05-25T00:00:00Z
depth: standard
files_reviewed: 30
files_reviewed_list:
  - cpp/src/shim.cpp
  - cpp/src/shim.hpp
  - src/ddl/alter_helpers_ffi.rs
  - src/ddl/define.rs
  - src/ddl/describe.rs
  - src/ddl/get_ddl.rs
  - src/ddl/list.rs
  - src/ddl/mod.rs
  - src/ddl/read_ffi.rs
  - src/ddl/read_yaml.rs
  - src/ddl/show_columns.rs
  - src/ddl/show_dims.rs
  - src/ddl/show_dims_for_metric.rs
  - src/ddl/show_facts.rs
  - src/ddl/show_materializations.rs
  - src/ddl/show_metrics.rs
  - src/lib.rs
  - src/parse.rs
  - src/query/explain.rs
  - src/query/table_function.rs
  - src/type_cache.rs
  - test/integration/test_concurrent_reads_per_call_conn.py
  - test/integration/test_create_from_yaml_v010.py
  - test/integration/test_readonly_load.py
  - test/sql/65_alter_comment_merge_patch.test
  - test/sql/65_alter_rename_via_sql.test
  - test/sql/65_json_merge_patch_smoke.test
  - test/sql/65_metadata_via_sql.test
  - test/sql/65_pk_error.test
  - test/sql/65_read_bridge_spike.test
  - tests/no_long_lived_conn.rs
findings:
  critical: 2
  warning: 9
  info: 6
  total: 17
status: issues_found
---

# Phase 65: Code Review Report

**Reviewed:** 2026-05-25
**Depth:** standard
**Files Reviewed:** 30
**Status:** issues_found

## Summary

Phase 65 is a wide-blast-radius surgical refactor: 17 read-side function
registrations migrated from duckdb-rs to a C++ Catalog API shim, and both
long-lived `duckdb_connection` handles (`H1 catalog_conn`, `H2 query_conn`)
retired from `init_extension`. The architectural shape — per-call
`Connection probe(*context.db)` opened inside C++ bind/exec callbacks and
borrowed across the FFI boundary via `reinterpret_cast<duckdb_connection>(Connection*)`
— is sound. The wire format, RAII guard (`SvOwnedBuffer`), and
`catch_unwind` discipline in the Rust dispatchers are consistent across
the 17 migrations and look correct under standard inspection.

That said, several real defects surface on close reading, two of which I
believe are correctness bugs that should block: a SQL-injection vector
through the YAML file path argument (which then degrades into a logically
incorrect double-emit pattern), and a subtle but provable double-emit /
state-leak in the helper TF and `list_semantic_views` exec callbacks when
the planner does not allocate local state. Several WARNINGS concern
unenforced invariants (no compile-time guarantee that the
`SvOwnedBuffer` (ptr,len) pair returned from Rust round-trips intact),
missing concurrency coverage of the parser_override write path, and a
schema-drift hazard in `sv_create_from_yaml_function` that the author
explicitly flagged in a comment but did not fix.

The structural guard test (`tests/no_long_lived_conn.rs`) is a good
addition but has a known-limitation hole around `use ... as` aliasing
that the author honestly documented — accepting as design.

## Critical Issues

### CR-01: SQL injection vector in `__sv_compute_create_from_yaml` bind via `read_text(...)` literal embedding

**File:** `cpp/src/shim.cpp:745-756`

**Issue:**
The bind callback for `__sv_compute_create_from_yaml` builds the
`read_text(...)` query by embedding the file-path string into the SQL
text after only doubling single quotes:

```cpp
std::string path_escaped = bd->file_path;
{
    size_t pos = 0;
    while ((pos = path_escaped.find('\'', pos)) != std::string::npos) {
        path_escaped.replace(pos, 1, "''");
        pos += 2;
    }
}
Connection probe(*context.db);
std::string read_sql =
    "SELECT content FROM read_text('" + path_escaped + "')";
auto result = probe.Query(read_sql);
```

The accompanying comment claims this is safe because the path arrived
as a typed `Value` from the outer parser_override SELECT. That argument
holds *for the outer SQL embedding* but is **not** sufficient for the
inner `read_text(...)` literal: when the helper TF is invoked via SQL
(`SELECT * FROM __sv_compute_create_from_yaml('...', 'v', 0, '')`), a
user can pass any string they want as `input.inputs[0]`. Single-quote
doubling alone does not neutralise inputs that contain SQL fragments
without `'`: for instance, a NUL byte (`\x00`), or, much more practically,
a value like:

```
/tmp/x.yaml'); SELECT error('pwned' --
```

— this *would* be neutralised by the `'`-doubling. The broader concern is that
paths containing embedded control characters (NUL, BiDi overrides such as
U+202E, etc.) are not handled at all and may interact unsafely with the
downstream `read_text(...)` path-resolution layer.

Beyond the literal SQL surface, the more direct concern is the
comment's own admission: the code chose `Connection::Query` over the
Prepare/Execute path because the latter returns a non-materialized
result that triggers `InternalException` on downcast. The Prepare path
with `bind_varchar` would have eliminated the literal-embedding surface
entirely. The chosen workaround quietly trades a clean parameterised
path for an ad-hoc escape with weaker guarantees.

The helper TF is reachable as a public table function (it's registered
on the system catalog with no access modifier), so an attacker who can
issue SQL can invoke it directly with any path argument.

**Fix:**
Use the parameterised prepare/bind/execute path even if it requires
materialising the result manually:

```cpp
Connection probe(*context.db);
auto prep = probe.Prepare("SELECT content FROM read_text(?)");
if (prep->HasError()) {
    throw BinderException(
        "FROM YAML FILE failed: " + prep->GetError());
}
auto result = prep->Execute(bd->file_path);
// Materialize via MaterializedQueryResult::Create if needed,
// or read row-by-row from QueryResult::Fetch().
```

Or, alternatively, since the contract here really is "read a file as
bytes," skip SQL entirely and use DuckDB's `FileSystem::OpenFile` API
directly on the per-call `ClientContext` (`FileSystem::GetFileSystem(context)`).
That removes the SQL surface AND keeps the file-system gating
(`enable_external_access`) intact because `LocalFileSystem` honors the
same settings.

If neither is feasible, at minimum mark the helper TF non-public by
prefixing with `__` (already done) AND document the SQL-injection
property explicitly so future readers do not erode it further.

---

### CR-02: Double-emit / unbounded-loop hazard in `sv_create_from_yaml_function` (and equivalent in `sv_list_semantic_views_function`) when local state is absent

**File:** `cpp/src/shim.cpp:825-856`, `cpp/src/shim.cpp:1023-1059`

**Issue:**
Three exec callbacks (`sv_create_from_yaml_function`,
`sv_list_semantic_views_function`, and the
`SvVarcharBindData`/`SvVarcharBoolBindData` paths via
`sv_emit_varchar_rows` / `sv_emit_varchar_bool_rows`) include a
defensive branch for `data_p.local_state.get() == nullptr` that emits
rows unconditionally on every invocation:

```cpp
auto *state_p = data_p.local_state.get();
if (state_p == nullptr) {
    output.SetValue(0, 0, Value(bd.new_def));
    output.SetCardinality(1);
    return;
}
```

DuckDB's table-function executor will call the exec callback **repeatedly
until it returns a chunk with `cardinality == 0`** (this is the streaming
contract). With no local-state-backed "already emitted" flag, the
nullptr-branch produces an infinite stream of identical rows, not a
single-emit. The accompanying comment correctly identifies the problem
("bind data is shared across executions, so for parallel safety we
register init_local... flip the registration to use the init_local
path"), but the registration WAS flipped — `sv_register_parser_hooks`
passes `sv_create_from_yaml_init_local` for the helper TF
(cpp/src/shim.cpp:2531) — yet the defensive fallback was left in place
and is, by design, broken.

The dispatch is currently saved because every registration in this
phase DOES pass a non-null `init_local`, so `data_p.local_state` is
never actually null at runtime. **But:** if a future migration uses
`sv_register_table_function` with `init_cb = nullptr` (the API permits
that — see `cpp/src/shim.hpp:73-75`), the resulting helper TF will hang
or OOM rather than emitting once. The "defensive" fallback paths are
incorrect in a way that can never be reached today but reads as a
working safety net.

The same hazard sits in the bind-side `sv_emit_varchar_rows` and
`sv_emit_varchar_bool_rows`: when `state_p == nullptr` they emit
`bd.rows` and return WITHOUT setting any "done" flag. They will be
called again with the same bind data and emit the same rows again,
ad infinitum.

In `sv_create_from_yaml_function` specifically, the nullptr branch
returns `output.SetCardinality(1)` with no termination signal — the
streaming executor will call again, see another single-row chunk, and
loop until the outer `INSERT ... SELECT ... FROM __sv_compute_*` query
runs out of memory accumulating rows.

**Fix:**
Remove the nullptr fallback entirely and assert that local state is
present. If `sv_register_table_function` is called with a null
`init_cb`, refuse to register or fail fast at the start of the exec
callback:

```cpp
static void sv_create_from_yaml_function(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<CreateFromYamlBindData>();
    auto *state_p = data_p.local_state.get();
    if (state_p == nullptr) {
        throw InternalException(
            "sv_create_from_yaml_function: local_state is null — "
            "registration must supply init_local callback");
    }
    auto &state = state_p->Cast<CreateFromYamlLocalState>();
    if (state.emitted) {
        output.SetCardinality(0);
        return;
    }
    output.SetValue(0, 0, Value(bd.new_def));
    output.SetCardinality(1);
    state.emitted = true;
}
```

Apply the same fix to `sv_list_semantic_views_function`,
`sv_emit_varchar_rows`, and `sv_emit_varchar_bool_rows`. Alternatively
(better), make `sv_register_table_function` require a non-null `init_cb`
at registration time and refuse a null pointer with a clear error.

The comment-block "(void)emitted; // single-shot semantics — even
without local state we emit once per bind" at cpp/src/shim.cpp:1148 is
also factually wrong about the contract and should be deleted.

---

## Warnings

### WR-01: `parse_string_list` accepts ambiguous null-buffer semantics, behaviour diverges between explain and table_function

**File:** `src/query/explain.rs:54-87` and `src/query/table_function.rs:61-92`

**Issue:**
Two near-identical parser functions for the LIST(VARCHAR) wire format
have different early-exit logic:

`src/query/explain.rs:54-62`:
```rust
unsafe fn parse_string_list(buf: *const u8, len: usize) -> Option<Vec<String>> {
    if buf.is_null() || len < 4 {
        if buf.is_null() && len == 0 {
            return Some(Vec::new());
        }
        if len < 4 {
            return None;
        }
    }
    ...
```

The first `if` condition is `buf.is_null() || len < 4`, but the inner
checks only handle `buf.is_null() && len == 0` and `len < 4`. The case
`buf.is_null() && len > 0 && len >= 4` falls through to
`std::slice::from_raw_parts(buf, len)` with a null pointer — **undefined
behaviour**. This is reachable in theory if the C++ side ever passes a
nonzero `len` with a null pointer.

In contrast, `src/query/table_function.rs:62-64` handles it correctly:
```rust
if buf.is_null() {
    return if len == 0 { Some(Vec::new()) } else { None };
}
```

The C++ side as written today always passes `(nullptr, 0)` for "not
provided" lists (cpp/src/shim.cpp:2016-2018), so the divergent path is
unreachable in current code. But the inconsistent handling is a
latent UB hazard.

**Fix:**
Replace `explain.rs::parse_string_list`'s early-exit block with the
`table_function.rs` shape:

```rust
if buf.is_null() {
    return if len == 0 { Some(Vec::new()) } else { None };
}
if len < 4 {
    return None;
}
```

Better: extract one shared helper into `src/ddl/read_ffi.rs` and call
it from both sites. The two functions are byte-for-byte identical
otherwise.

---

### WR-02: `sv_register_table_function`/`sv_register_scalar_function` swallow registration failures with stderr-only logging

**File:** `cpp/src/shim.cpp:570-626`, `cpp/src/shim.cpp:640-689`

**Issue:**
Both registration helpers catch all exceptions and return `false`, but
the only diagnostic is `fprintf(stderr, ...)`. Under DuckDB extension
load, stderr is often not visible to the user (especially through
ADBC/JDBC/Python where it may be redirected). The Rust caller in
`src/lib.rs::init_extension` does check the return value and converts
to `Err("Failed to register X via C++ Catalog API")`, but that error
loses the actual exception message — the user sees "Failed to register
list_semantic_views" with no hint of *why*.

If `Catalog::GetSystemCatalog(db)` or `CreateTableFunction` throws a
specific exception (e.g., "Function with name 'list_semantic_views' is
of a different type"), that information is dropped on the floor.

**Fix:**
Surface the C++ exception message back to the Rust caller. The cleanest
shape is to change the C ABI to accept an error-buffer:

```cpp
bool sv_register_table_function(
    duckdb_database db_handle, const char *name, ...,
    char *error_buf, size_t error_buf_len);
```

and write `e.what()` into the buffer on failure. The Rust caller then
includes it in the error message. Pattern matches what
`sv_parser_override_rust` and the 17 read-side dispatchers already do.

If reworking the ABI is too invasive, at minimum prefix the stderr
output with a clear marker so users grepping logs can find it
(`"[semantic_views fatal] ..."`).

---

### WR-03: `existence_guard_select` and metadata-via-SQL paths assume `semantic_layer._definitions` exists, but on RO-on-fresh-DB it may not

**File:** `src/parse.rs:2119-2126`, `src/parse.rs:1892-1901`

**Issue:**
The pure-SQL existence guard runs
`SELECT 1 FROM semantic_layer._definitions WHERE name = '...'`. If the
caller invokes DROP/ALTER on a read-only DB that was never bootstrapped
(no `_definitions` table), DuckDB raises a generic
`Catalog Error: Table _definitions does not exist` rather than the
intended `semantic view 'X' does not exist`. The original
`catalog.exists()` pre-check could distinguish missing-table from
missing-row, but the new pure-SQL guard cannot.

The integration test `test_fresh_readonly_empty_list` only exercises
READ paths (`list_semantic_views`, `describe_semantic_view`,
`semantic_view`), not DROP/ALTER on the fresh RO DB. The DROP/ALTER
case is gated behind `test_readonly_ddl_fails`, which only runs on a
bootstrapped DB — so the missing-table-on-RO case is untested.

This is a regression in error wording, not a correctness bug, but it
violates the "snapshot-consistent error wording" claim in the design
docs.

**Fix:**
Either (a) make `init_catalog` succeed silently on RO open even without
the table (it may already — but the guard still trips because the
guard runs AT DML time, not at init), or (b) wrap the guard SQL in a
`CASE WHEN NOT EXISTS (SELECT 1 FROM information_schema.tables WHERE
table_schema = 'semantic_layer' AND table_name = '_definitions') THEN
error('...') ELSE ... END` outer guard so the user sees the same
wording either way.

Add an integration test:
```python
def test_drop_on_fresh_readonly_clear_error():
    # Fresh RO DB, never bootstrapped -> DROP should say "does not exist"
    # not "Table _definitions does not exist".
```

---

### WR-04: Missing concurrency coverage for parser_override (CREATE/DROP/ALTER) under per-call connection model

**File:** `test/integration/test_concurrent_reads_per_call_conn.py`

**Issue:**
`test_concurrent_reads_per_call_conn.py` exercises 8 threads × 10 calls
of `show_semantic_dimensions`, which is a READ path. The Phase 65
migration also reshaped the WRITE path (parser_override emits pure SQL
INSERT/DELETE/UPDATE against `_definitions`). The write path is more
sensitive to the per-call connection model: same-transaction guards
embedded in the SQL rely on snapshot-consistency, and the
metadata-via-SQL `json_merge_patch` runs `now()` / `current_database()`
at execution time on the caller's connection.

There is no test that hammers concurrent CREATE/DROP/ALTER from
multiple threads to verify:
- No deadlock on the same `_definitions` table when many threads
  serialize their writes
- The race-guard wording ("already exists" / "does not exist") is
  returned to the correct caller even when their snapshots see
  different states
- PK constraint violations under concurrent CREATE of the same view
  surface a reasonable error rather than corrupting state

**Fix:**
Add `test_concurrent_writes_per_call_conn.py` with N threads each
running a mix of CREATE/DROP/ALTER targeting overlapping view names.
Assert that all operations complete (no hang), all "already exists"
errors look right, and the final `list_semantic_views()` state is
consistent with the operations that returned success.

This was a known concern at Phase 60 (TECH-DEBT 23) and the parser
guards were redesigned in Phase 65 to be transactional. There should
be a test that proves it.

---

### WR-05: BORROW contract for `reinterpret_cast<duckdb_connection>(Connection*)` is documented but unenforced at the type level

**File:** `cpp/src/shim.cpp:970, 1241, 1267, 1294, 1317, 1840, 1875, 2007, 2283, etc.`, all Rust dispatchers in `src/ddl/*.rs`

**Issue:**
The borrow contract — "Rust MUST NOT call `duckdb_disconnect` on the
borrowed handle, doing so deletes a stack Connection and is UB" —
is repeated in ~20 comments across the codebase. It is enforced
solely by code review.

Any future contributor adding a Rust dispatcher who reaches for
`ffi::duckdb_disconnect` (perhaps as a "cleanup" they think the C
API expects) would not see any compile-time, lint-time, or test-time
failure until production crashes. The structural guard test
(`tests/no_long_lived_conn.rs`) only catches `duckdb_connect` calls
in `init_extension`, not `duckdb_disconnect` anywhere.

The single 8-thread × 10-call integration test does exercise the
borrow but only proves that the *current* dispatchers do not
disconnect.

**Fix:**
Two options, in increasing strength:

1) **Static analysis guard:** Extend `tests/no_long_lived_conn.rs` (or
   add a sibling test) that AST-walks the entire `src/` tree looking
   for `duckdb_disconnect` calls. Whitelist only the test helper
   `RawDb::drop` site. Anything else fails CI.

2) **Type-level enforcement:** Wrap the borrowed handle in a Rust
   newtype:
   ```rust
   #[repr(transparent)]
   pub struct BorrowedConnection(ffi::duckdb_connection);
   // No Drop impl. The only constructor is unsafe and takes a
   // raw duckdb_connection. CatalogReader::new takes a
   // BorrowedConnection rather than a raw handle.
   ```
   Then `ffi::duckdb_disconnect` simply does not type-check against
   `BorrowedConnection`; you have to deliberately unwrap to call it.

Option 1 is mostly free. Option 2 catches it at compile time and is
a better long-term posture but does churn the API.

---

### WR-06: Layout assumption `reinterpret_cast<duckdb_connection>(Connection*)` is verified by reading the amalgamation but has no compile-time guard

**File:** `cpp/src/shim.cpp:958-970`

**Issue:**
The bridge mechanism — casting a stack `Connection *` to
`duckdb_connection` — works because
`duckdb_connect` is literally
`reinterpret_cast<duckdb_connection>(new Connection(...))`. The
comment cites duckdb.cpp:266432-266447 as evidence.

If a future DuckDB minor version changes that (e.g., introduces a
wrapper struct so `duckdb_connection->internal_ptr` is the
`Connection *`, which is exactly how `duckdb_database` works
today via `DatabaseWrapper`), the cast silently misinterprets the
pointer and reads garbage. The Rust dispatcher would then call
into a corrupt handle, likely crashing inside `ffi::duckdb_query`.

DuckDB's ABI is not stable across minor versions (per CLAUDE.md
"Critical Pitfalls"). The cited duckdb.cpp lines are pinned to a
specific commit but the test surface does not.

**Fix:**
Add a single C++ compile-time / load-time assertion that confirms
the layout assumption:

```cpp
// Verify the bridge contract: duckdb_connection is layout-compatible
// with Connection*. If a future DuckDB minor bump introduces a
// wrapper, this assertion fires and forces a re-architecture before
// any per-call Connection bridge can fire.
static_assert(sizeof(duckdb_connection) == sizeof(Connection*),
    "duckdb_connection must be pointer-sized — bridge contract broken");

// And at extension load (since static_assert can't probe runtime
// representation):
{
    Connection probe(*wrapper->database->instance);
    auto handle = reinterpret_cast<duckdb_connection>(&probe);
    // Round-trip through duckdb_query with a trivial statement;
    // if the handle is garbage, this will crash here at load time
    // rather than at first read-side bind.
    duckdb_result r;
    auto rc = duckdb_query(handle, "SELECT 1", &r);
    if (rc != DuckDBSuccess) {
        return false;  // refuse extension load
    }
    duckdb_destroy_result(&r);
}
```

This pins the contract: a DuckDB bump that breaks the bridge fails at
LOAD time rather than first-query time.

---

### WR-07: `sv_resolve_output_logical_types` falls back silently on probe failure, hiding type errors

**File:** `cpp/src/shim.cpp:2203-2255`

**Issue:**
When the LIMIT 0 probe fails (either `HasError()` or `types.size() != cols.size()`),
the function silently falls back to declaring every column from the
`type_id` alone, returning a DECIMAL(18,3) placeholder for DECIMAL
columns and `LIST(VARCHAR)` for LIST columns. The user gets a column
typed as DECIMAL(18,3) when their actual data is DECIMAL(38,18),
producing incorrect numeric output without any error message.

The fallback is intentional ("matches the legacy fallback ladder"),
but the legacy ladder had similar problems and was never the right
behavior — it just was the behavior. Phase 65 was a good opportunity
to surface the error.

**Fix:**
On probe failure with a `needs_logical_probe` schema, throw
`BinderException("semantic_view: failed to infer column types via
LIMIT 0 probe: <error>")` rather than falling back. The user can
then fix their schema or report the bug. Silently returning the
wrong precision is the worst possible outcome.

If keeping the fallback is required for compatibility, at minimum
log a warning via DuckDB's `notification` channel so users have
some signal.

---

### WR-08: `try_infer_schema` returns `None` on error with no diagnostic; downstream constructs zero-typed columns silently

**File:** `src/query/table_function.rs:704-729`, callers at lines 294, 349

**Issue:**
`try_infer_schema` swallows errors from `execute_sql_raw` (`.ok()?`).
When it returns `None`, callers fall back to declaring all columns
with `type_ids = vec![0u32; names.len()]`. Type id 0 is
`DUCKDB_TYPE_INVALID`, which `sv_logical_type_from_c_type_id` maps
to `LogicalType::VARCHAR` (a fallback that the comment admits is
arbitrary). The user sees their integer or decimal column declared
as VARCHAR with no error.

This is more insidious than WR-07 because it only fires when the
expanded SQL itself is broken (table missing, permissions error,
etc.) — those are exactly the cases where the user MOST needs a
clear error message rather than a column-type mystery.

**Fix:**
Plumb the error message from `execute_sql_raw` back up to
`sv_semantic_view_bind_rust` and surface it as a BinderException
with the actual DuckDB error text. The expanded SQL is already
captured for the error message (`expanded_sql_for_error` on the
C++ side); plumbing the same surface through here keeps the
diagnostic path complete.

---

### WR-09: `init_extension` accumulated registration order is fragile

**File:** `src/lib.rs:450-575`

**Issue:**
`init_extension` registers 17 read-side functions plus parser hooks
sequentially via 17 calls to `sv_register_*`. Each registration's
failure aborts further init via `?` — but the failures are not
idempotent: if registration #5 fails, registrations #1-#4 are still
live on the system catalog with `ALTER_ON_CONFLICT`, but the
extension is reported as not loaded. The user can `LOAD` again, get
registrations 1-4 re-applied (correctly, due to
`ALTER_ON_CONFLICT`), and registration #5 retry — but parser hooks
are also re-registered, accumulating in `DBConfig::parser_extensions`.

The pre-Phase-65 code had the same issue but a smaller surface (~3
registrations). Phase 65 multiplied the surface 5×. The blast
radius on partial-failure is now ~17 incompletely-registered
functions sitting in the catalog.

**Fix:**
Either:
1) Make `sv_register_parser_hooks` idempotent (currently
   `ParserExtension::Register` appends to a vector — second load
   adds a second copy of the hook).
2) Pre-flight all 17 registrations into a vector and only commit
   on full success (heavy refactor — probably not worth it).
3) Document the failure mode in the README ("If `LOAD semantic_views`
   fails partway, force-unload before retrying") — least invasive,
   adequate for the current failure rate.

The most pragmatic answer is to make parser-hook registration check
for an existing matching hook before appending. Stale catalog
entries are absorbed by `ALTER_ON_CONFLICT`.

---

## Info

### IN-01: Comment in `sv_create_from_yaml_function` is internally inconsistent

**File:** `cpp/src/shim.cpp:830-844`

**Issue:**
The comment block first describes the nullptr-fallback as defensive
("bind data is shared across executions, so for parallel safety we
register init_local"), then declares "Currently sv_register_parser_hooks
passes nullptr for init_cb on this helper" — but the actual
registration call at line 2531 passes `sv_create_from_yaml_init_local`,
not nullptr. The comment is stale.

**Fix:**
Update the comment to reflect that init_local is wired in, and remove
the misleading "flip the registration to use the init_local path"
recommendation since it is already done. (See CR-02 for the deeper
issue with this code block.)

---

### IN-02: Dead-code allowance on `type_id_to_display_name`

**File:** `src/query/table_function.rs:500-568`

**Issue:**
`type_id_to_display_name` is marked `#[allow(dead_code)]` because, per
its own doc-comment, "no in-tree caller after the CREATE-time type
inference paths were removed from `enrich_definition_for_create`
(D-16/D-17). Kept alive for Plan 05's read-side bind callbacks which
will re-probe at SHOW / DESCRIBE / `semantic_view()` bind time."

Plan 05 has now landed. The function is still unused in production
code (only the C++ side's `sv_logical_type_from_c_type_id` does the
mapping). If it is intended for use by `type_cache::InferredTypes`,
that wiring was deferred per the BATCH2-SUMMARY note. The
`#[allow(dead_code)]` masks the dead code, but it should either be
wired up or removed.

**Fix:**
Either:
- Wire `type_id_to_display_name` into the type_cache probe path so
  DDL output_type matches what users expect, or
- Delete the function and the `type_cache` module (which is also
  unused — see IN-03).

---

### IN-03: `type_cache` module is fully unused in production code

**File:** `src/type_cache.rs`

**Issue:**
The whole `type_cache` module is registered in lib.rs but per the
in-source comment at `src/query/table_function.rs:39-43`, the cache
is "intentionally NOT consumed here — the per-bind LIMIT 0 cost is
well under a millisecond and the cache adds complexity (fingerprint
over the JSON, lookup_or_probe wiring) without a measured win."

The module compiles, its tests run, but no production code calls
into it. ~225 lines of unused complexity tracked as TECH-DEBT.

**Fix:**
Either wire it in (low priority per the comment's own assessment)
or delete it now. Carrying unused infrastructure is a future-
contributor trap — someone will assume it is in the call graph and
spend time debugging "why isn't my cache populated?".

---

### IN-04: `CreateFromYamlBindData::kind` is captured but never used by Rust helper

**File:** `cpp/src/shim.cpp:712-715, 793`, `src/ddl/alter_helpers_ffi.rs:127`

**Issue:**
The `kind` int (0/1/2 = CREATE / OR REPLACE / IF NOT EXISTS) is
threaded all the way through bind, into the FFI, and the Rust helper
explicitly marks the parameter `_kind: u8` (intentionally unused).
The comment at cpp/src/shim.cpp:118-122 says the parameter is
"threaded for forward compat with future variants whose enrichment
differs."

If forward compat is the goal, fine — but the current code path is
strictly dead data being threaded across the FFI for no functional
reason. The kind is encoded redundantly in the outer
parser_override INSERT shape (`INSERT OR IGNORE` vs `INSERT OR REPLACE`
vs plain INSERT + CASE guard).

**Fix:**
Either remove the parameter from the FFI signature (slimmer, less
surface) or add a debug-only assertion in the Rust helper that the
caller's `kind` matches what the outer INSERT shape implies.

---

### IN-05: `read_yaml_from_semantic_view` has no test for null name vector input

**File:** `cpp/src/shim.cpp:1864-1895`, `src/ddl/read_yaml.rs`

**Issue:**
The C++ exec callback checks `name_validity.RowIsValid(i)` and sets
result validity to invalid for null inputs. Good. But there is no
sqllogictest or unit test that exercises
`SELECT read_yaml_from_semantic_view(NULL)` to confirm the result is
NULL rather than an error or crash. Same for `get_ddl(NULL, 'x')` /
`get_ddl('SEMANTIC_VIEW', NULL)`.

These are easy-to-write tests that catch a real regression
(e.g., someone deleting the null check while refactoring).

**Fix:**
Add to one of the existing sqllogictests:
```sql
query I
SELECT read_yaml_from_semantic_view(NULL) IS NULL;
----
true

query I
SELECT get_ddl(NULL, 'x') IS NULL;
----
true
```

---

### IN-06: Dual `probe_catalog_table_present` implementations (read_ffi.rs and list.rs)

**File:** `src/ddl/read_ffi.rs:57-74`, `src/ddl/list.rs:191-210`

**Issue:**
Both files define `probe_catalog_table_present` with byte-identical
SQL and identical safety contracts. The list.rs copy is module-private,
the read_ffi.rs copy is `pub`. The list.rs file uses its own local
copy in `sv_list_semantic_views_bind_rust` (line 99) but imports the
shared one elsewhere. Result: two functions doing exactly the same
job, with no compiler warning because both are reachable.

**Fix:**
Delete `src/ddl/list.rs::probe_catalog_table_present` and have
`sv_list_semantic_views_bind_rust` use
`crate::ddl::read_ffi::probe_catalog_table_present` like every other
dispatcher does. (Same comment applies to the duplicate `write_err`
in list.rs.)

---

_Reviewed: 2026-05-25_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
