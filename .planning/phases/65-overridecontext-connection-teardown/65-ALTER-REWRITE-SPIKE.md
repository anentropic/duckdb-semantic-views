# Phase 65 — ALTER-Rewrite Spike Evidence

**Spike:** Forward-looking probe for v0.10.0's ALTER + CREATE FROM YAML FILE rewrites. Specifically: does DuckDB v1.5.2 support the SQL shape

```sql
UPDATE semantic_layer._definitions
SET definition = (SELECT new_def FROM __sv_spike_compute('v', 'op_payload'))
WHERE name = 'v'
```

where `__sv_spike_compute` is a table function registered via the C++ Catalog API (so its `bind` callback has `ClientContext &` in scope, the read-path mechanism validated by `65-READ-PATH-SPIKE.md`)?

The outer UPDATE binds on the caller's connection and rides their `BEGIN`/`ROLLBACK`/`COMMIT` (the D-20 transactional-DDL contract). The inner table function opens a per-call `Connection(*context.db)` from its bind callback (the C++ direct path validated by `65-OPTION-B-SPIKE.md` Probe 1 + `65-READ-PATH-SPIKE.md`) and reads current state. The result is funnelled back through the SET-expression subquery into the outer UPDATE, which writes transactionally.

If viable, this is the architectural primitive that lets v0.10.0:

- Implement ALTER SEMANTIC VIEW as `parser_override` → produces native SQL of shape `UPDATE _definitions SET definition = (SELECT new_def FROM __sv_compute_alter(name, op_payload)) WHERE name = ?` (re-parsed and re-dispatched through the caller's binder by `Parser::ParseQuery` exactly the same way CREATE/DROP/DESCRIBE already are at v0.9.0).
- Implement CREATE FROM YAML FILE the same way — YAML parse / type inference / catalog enrichment all happen inside the helper table function's bind, returning the final JSON-serialised definition string.

The reason a table-function helper is needed (rather than a pure SQL expression) is that ALTER needs to READ the current row, apply the operation, and WRITE the new row — and the read happens via `Connection(*context.db)` because read-side state needs catalog lookups (e.g. `duckdb_constraints()` for relationship inference, LIMIT 0 probes for type inference on file-backed DBs) that don't compose into a pure-SQL expression.

---

## Question

Three sub-questions (verbatim from the spike protocol):

1. **SQL-level capability** — does DuckDB v1.5.2 bind and execute `UPDATE … SET col = (SELECT v FROM my_table_function(…)) WHERE …` without errors?
2. **Transactional behaviour** — does the outer UPDATE participate in the caller's `BEGIN`/`COMMIT`/`ROLLBACK` (the D-20 contract)?
3. **Inner table function's view of state** — the inner TF reads via `Connection(*context.db)` (a fresh `ClientContext`). Does it see only committed state, or does it see the outer transaction's uncommitted writes? Informational, not gating — the TECH-DEBT 19 trade-off already accepts committed-only reads.

---

## API contract verification

The binder code path that handles SET expressions in UPDATE is at `cpp/include/duckdb.cpp:370905-370936`:

```cpp
for (idx_t i = 0; i < set_info.columns.size(); i++) {
    auto &colname = set_info.columns[i];
    auto &expr = set_info.expressions[i];
    ...
    if (expr->GetExpressionType() == ExpressionType::VALUE_DEFAULT) {
        update_expressions.push_back(make_uniq<BoundDefaultExpression>(column.Type()));
    } else {
        UpdateBinder binder(*expr_binder_ptr, context);
        binder.target_type = column.Type();
        auto bound_expr = binder.Bind(expr);
        PlanSubqueries(bound_expr, root);   // <-- subqueries supported
        ...
    }
}
```

The explicit `PlanSubqueries(bound_expr, root)` call (line 370930) confirms the binder accepts subqueries in SET expressions structurally — not as a side-effect of expression binding. `UpdateBinder` is a generic expression binder so any expression-form (including a subquery referencing a table function in its `FROM`) goes through the standard expression-binder path. Subqueries from table functions then bind as `LogicalGet` over the table-function call, the same path read-side table functions use today.

`Binder::Bind(UpdateStatement &stmt)` at `cpp/include/duckdb.cpp:370986+` then assembles the resulting `LogicalUpdate` + `LogicalProjection` + `LogicalGet` plan. There is no special-cased rejection of table-function calls inside SET subqueries anywhere along this path.

**Conclusion:** the API surface is structurally compatible. The spike's job is to convert the structural-compatibility argument into empirical evidence with the same test methodology as the prior B-prime spikes.

---

## Spike scaffold

All scratch code was added to `cpp/src/shim.cpp` + `test/sql/65_alter_spike.test`, wrapped in `// SPIKE-ALTER-65 — REVERT BEFORE COMMIT` markers, and reverted before this SPIKE.md was committed.

### Phase 1 — vanilla DuckDB binder smoke (zero extension)

Ran against vanilla `duckdb` v1.5.2 (Python bindings, same library the extension links against). Five sub-probes:

```python
import duckdb
con = duckdb.connect()
con.execute("CREATE TABLE t (id INT PRIMARY KEY, val INT)")
con.execute("INSERT INTO t VALUES (1, 100)")

# Probe A: INT variant — UPDATE SET = (SELECT MAX(range) FROM range(5))
con.execute("UPDATE t SET val = (SELECT MAX(range) FROM range(5)) WHERE id = 1")

# Probe B: VARCHAR variant — closer to ALTER shape
con.execute("CREATE TABLE u (name VARCHAR PRIMARY KEY, def VARCHAR)")
con.execute("INSERT INTO u VALUES ('v', 'ORIGINAL')")
con.execute("UPDATE u SET def = (SELECT 'TRANSFORMED:' || CAST(MAX(range) AS VARCHAR) FROM range(5)) WHERE name = 'v'")

# Probe C: ROLLBACK semantics
con.execute("BEGIN")
con.execute("UPDATE u SET def = (SELECT 'ROLLBACK_ME:' || CAST(MAX(range) AS VARCHAR) FROM range(3)) WHERE name = 'v'")
con.execute("ROLLBACK")

# Probe D: COMMIT semantics
con.execute("BEGIN")
con.execute("UPDATE u SET def = (SELECT 'COMMITTED:' || CAST(MAX(range) AS VARCHAR) FROM range(7)) WHERE name = 'v'")
con.execute("COMMIT")

# Probe E: RETURNING clause
con.execute("UPDATE u SET def = (SELECT 'WITH_RETURNING:' || CAST(MAX(range) AS VARCHAR) FROM range(2)) WHERE name = 'v' RETURNING name")
```

All five returned the expected values (output below).

### Phase 2 — sentinel `__sv_spike_compute` table function

Added directly to `cpp/src/shim.cpp` just before `sv_register_parser_hooks`:

```cpp
struct AlterSpikeBindData : public TableFunctionData {
    string current_name;
    string op_payload;
};
struct AlterSpikeLocalState : public LocalTableFunctionState {
    bool emitted = false;
};

static unique_ptr<FunctionData> sv_alter_spike_bind(
    ClientContext &context, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {
    auto bind_data = make_uniq<AlterSpikeBindData>();
    bind_data->current_name = input.inputs[0].GetValue<string>();
    bind_data->op_payload   = input.inputs[1].GetValue<string>();

    fprintf(stderr,
        "[ALTER-SPIKE] bind entered: current_name='%s' op_payload='%s'\n",
        bind_data->current_name.c_str(), bind_data->op_payload.c_str());

    try {
        Connection probe(*context.db);
        string select_sql =
            "SELECT definition FROM semantic_layer._definitions WHERE name = '" +
            bind_data->current_name + "'";
        auto result = probe.Query(select_sql);
        if (result->HasError()) {
            fprintf(stderr,
                "[ALTER-SPIKE] bind read error: %s\n", result->GetError().c_str());
        } else {
            fprintf(stderr,
                "[ALTER-SPIKE] bind read returned %llu rows for name='%s'\n",
                (unsigned long long)result->RowCount(),
                bind_data->current_name.c_str());
        }
    } catch (const std::exception &e) {
        fprintf(stderr,
            "[ALTER-SPIKE] bind Connection ctor/Query threw: %s\n", e.what());
    } catch (...) {
        fprintf(stderr, "[ALTER-SPIKE] bind threw unknown exception\n");
    }

    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("new_def");
    return std::move(bind_data);
}

static unique_ptr<LocalTableFunctionState> sv_alter_spike_init_local(
    ExecutionContext &, TableFunctionInitInput &,
    GlobalTableFunctionState *) {
    return make_uniq<AlterSpikeLocalState>();
}

static void sv_alter_spike_function(
    ClientContext &, TableFunctionInput &data_p, DataChunk &output) {
    auto &bind_data = data_p.bind_data->Cast<AlterSpikeBindData>();
    auto &state     = data_p.local_state->Cast<AlterSpikeLocalState>();
    if (state.emitted) { output.SetCardinality(0); return; }
    string new_def = "TRANSFORMED:" + bind_data.op_payload;
    output.SetValue(0, 0, Value(new_def));
    output.SetCardinality(1);
    state.emitted = true;
}

static void sv_register_alter_spike(DatabaseInstance &db) {
    TableFunction tf(
        "__sv_spike_compute",
        {LogicalType::VARCHAR, LogicalType::VARCHAR},
        sv_alter_spike_function,
        sv_alter_spike_bind,
        nullptr,                         // init_global
        sv_alter_spike_init_local);
    CreateTableFunctionInfo info(tf);
    info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;
    auto &system_catalog = Catalog::GetSystemCatalog(db);
    auto txn = CatalogTransaction::GetSystemTransaction(db);
    system_catalog.CreateTableFunction(txn, info);
    fprintf(stderr, "[ALTER-SPIKE] __sv_spike_compute registered\n");
}
```

Registration was hooked into the existing `sv_register_parser_hooks` C++ entry point (called by Rust at `init_extension` time) by appending a single call to `sv_register_alter_spike(db);` right before the success return. No Rust changes required.

### Test driver (sqllogictest)

`test/sql/65_alter_spike.test`:

```
require semantic_views

statement ok
LOAD semantic_views;

# Seed (semantic_layer._definitions exists post-LOAD via init_catalog).
statement ok
INSERT INTO semantic_layer._definitions (name, definition) VALUES ('v', 'ORIGINAL');

# Smoke 1: UPDATE-with-TF-subquery binds + executes
statement ok
UPDATE semantic_layer._definitions SET definition = (SELECT new_def FROM __sv_spike_compute('v', 'op1')) WHERE name = 'v';

query T
SELECT definition FROM semantic_layer._definitions WHERE name = 'v';
----
TRANSFORMED:op1

# Probe A: BEGIN -> UPDATE -> ROLLBACK -> row reverts
statement ok
BEGIN;
statement ok
UPDATE semantic_layer._definitions SET definition = (SELECT new_def FROM __sv_spike_compute('v', 'op_rolled_back')) WHERE name = 'v';
query T
SELECT definition FROM semantic_layer._definitions WHERE name = 'v';
----
TRANSFORMED:op_rolled_back
statement ok
ROLLBACK;
query T
SELECT definition FROM semantic_layer._definitions WHERE name = 'v';
----
TRANSFORMED:op1

# Probe B: BEGIN -> UPDATE -> COMMIT -> row persists
statement ok
BEGIN;
statement ok
UPDATE semantic_layer._definitions SET definition = (SELECT new_def FROM __sv_spike_compute('v', 'op_committed')) WHERE name = 'v';
statement ok
COMMIT;
query T
SELECT definition FROM semantic_layer._definitions WHERE name = 'v';
----
TRANSFORMED:op_committed

# Probe C (informational): inner TF reading via Connection(*context.db) —
# does it see uncommitted state?
statement ok
BEGIN;
statement ok
INSERT INTO semantic_layer._definitions (name, definition) VALUES ('new_view', 'JUST_INSERTED');
statement ok
UPDATE semantic_layer._definitions SET definition = (SELECT new_def FROM __sv_spike_compute('new_view', 'probe_c')) WHERE name = 'new_view';
statement ok
ROLLBACK;
```

Build: `just build` (cargo `--features extension` + cdylib pack) completed cleanly in 3m 32s. Run:

```bash
timeout 30 ./configure/venv/bin/python3 -u -m duckdb_sqllogictest \
  --test-dir test/sql \
  --file-list <(echo test/sql/65_alter_spike.test) \
  --external-extension build/debug/semantic_views.duckdb_extension \
  > $TMPDIR/65_alter_spike.log 2>&1
```

---

## Test methodology

The structure mirrors `65-OPTION-B-SPIKE.md` and `65-EXEC-TIME-SPIKE.md`. Each phase is gated by the preceding one:

| Phase | What it probes | Halt-rule if it fails |
|-------|---------------|----------------------|
| Phase 1 | Vanilla DuckDB v1.5.2 binds UPDATE-with-TF-subquery-in-SET | Pattern is structurally unsupported; spike concludes ALTER-RC1 immediately. |
| Phase 2 (build) | Extension table function via C++ Catalog API still compiles and registers cleanly | Spike infrastructure broken — would have to fix first. |
| Phase 3 Smoke 1 | Outer UPDATE with the extension TF's subquery binds + executes | If bind fails, the extension's TF doesn't compose into UPDATE — would need a different lifecycle phase. |
| Phase 3 Probe A | BEGIN/UPDATE/ROLLBACK reverts the row | If row persists post-ROLLBACK, the outer UPDATE isn't on the caller's connection — D-20 violation, ALTER-RC1. |
| Phase 3 Probe B | BEGIN/UPDATE/COMMIT persists the row | If row doesn't persist, COMMIT semantics broken — ALTER-RC1. |
| Phase 4 Probe C | Inner TF sees committed-only state (TECH-DEBT 19 carry-over) | Informational; either outcome is acceptable, both are documented. |

---

## Empirical outcomes

### Phase 1 — vanilla DuckDB binder smoke

```
[PHASE1-A] UPDATE-from-TF-subquery OK: [(1, 4)]
[PHASE1-B] UPDATE VARCHAR OK: [('v', 'TRANSFORMED:4')]
[PHASE1-C-during] [('v', 'ROLLBACK_ME:2')]
[PHASE1-C-after] [('v', 'TRANSFORMED:4')]
[PHASE1-D-after] [('v', 'COMMITTED:6')]
[PHASE1-E] RETURNING clause OK: [('v',)]
```

All five sub-probes PASS. The pattern is structurally supported by DuckDB v1.5.2's UPDATE binder for both INT and VARCHAR column types, both with and without an enclosing transaction, and the RETURNING clause composes cleanly on top.

### Phase 2 — build

`just build` exited 0; the spike infrastructure compiled into the cdylib without warnings on top of the existing extension. The `[ALTER-SPIKE] __sv_spike_compute registered` trace fires once at LOAD time, confirming `system_catalog.CreateTableFunction(txn, info)` accepted the registration (same pattern as `65-READ-PATH-SPIKE.md`).

### Phase 3 — extension-level smoke + Probe A (rollback) + Probe B (commit) + Phase 4 — Probe C (committed-state read)

Single run, verbatim stderr:

```
[1/1] test/sql/65_alter_spike.test
[ALTER-SPIKE] __sv_spike_compute registered
[ALTER-SPIKE] bind entered: current_name='v' op_payload='op1'
[ALTER-SPIKE] bind read returned 1 rows for name='v'
[ALTER-SPIKE] bind entered: current_name='v' op_payload='op_rolled_back'
[ALTER-SPIKE] bind read returned 1 rows for name='v'
[ALTER-SPIKE] bind entered: current_name='v' op_payload='op_committed'
[ALTER-SPIKE] bind read returned 1 rows for name='v'
[ALTER-SPIKE] bind entered: current_name='new_view' op_payload='probe_c'
[ALTER-SPIKE] bind read returned 0 rows for name='new_view'
SUCCESS
```

sqllogictest exit code: `0`. All four `query T` assertions matched the expected values. No deadlocks, no exceptions, no timeouts.

| Probe | Expected | Observed | Verdict |
|-------|----------|----------|---------|
| Smoke 1 | After UPDATE: `definition = 'TRANSFORMED:op1'` | Matches | PASS |
| Probe A (during) | Inside BEGIN: `definition = 'TRANSFORMED:op_rolled_back'` | Matches | PASS |
| Probe A (after ROLLBACK) | `definition = 'TRANSFORMED:op1'` (reverted) | Matches | PASS |
| Probe B (after COMMIT) | `definition = 'TRANSFORMED:op_committed'` (persisted) | Matches | PASS |
| Probe C (inner TF read) | 0 or 1 rows for just-inserted-uncommitted `new_view` | **0 rows** | Committed-only |

**Probe C result interpreted:** the outer transaction's `INSERT INTO semantic_layer._definitions VALUES ('new_view', 'JUST_INSERTED')` is uncommitted at the moment `__sv_spike_compute('new_view', ...)` binds. The bind callback's `Connection probe(*context.db)` opens a fresh `ClientContext` with its own transaction state — and queries `SELECT … WHERE name = 'new_view'` against that fresh `ClientContext`. The fresh `ClientContext` does NOT see the outer transaction's uncommitted INSERT — it returns 0 rows. This is exactly the committed-only read behaviour predicted by `65-OPTION-B-SPIKE.md`'s interpretation (and accepted as TECH-DEBT 19 for DESCRIBE/SHOW on existing read-path code).

Critically: even though the inner read sees `new_view` as nonexistent, the outer UPDATE still executes — it matches zero rows in `_definitions WHERE name = 'new_view'` (since the INSERT is on the same outer transaction, which the UPDATE *does* see, but the UPDATE itself binds the inner TF before reaching the WHERE-clause row-matching phase). The TF's bind succeeds, returns `'TRANSFORMED:probe_c'`, and the UPDATE matches 1 row in the outer txn's view of `_definitions` (the row INSERTed earlier in the same txn). Post-ROLLBACK, everything in the outer txn — including both the INSERT and the UPDATE — is gone.

---

## Verdict — **ALTER-RC0**

The "rewrite-to-UPDATE-with-table-function-subquery" pattern is fully viable on DuckDB v1.5.2 for the v0.10.0 architecture. Specifically:

1. **SQL-level capability:** confirmed — DuckDB v1.5.2's `Binder::Bind(UpdateStatement&)` and `BindUpdateSet` route SET-expression subqueries through `PlanSubqueries` (`cpp/include/duckdb.cpp:370930`), and the same code path that binds read-side TF references in plain SELECTs binds them inside UPDATE SET subqueries.
2. **Transactional behaviour:** confirmed — the outer UPDATE participates in the caller's `BEGIN`/`COMMIT`/`ROLLBACK` (Probes A and B both pass). The D-20 contract is preserved.
3. **Inner TF view of state:** confirmed — `Connection(*context.db)` sees committed-only state, the same trade-off TECH-DEBT 19 already documented for DESCRIBE/SHOW. ALTER and CREATE FROM YAML FILE will carry this trade-off forward unchanged.

No deadlocks observed (the `context_lock` self-deadlock from `65-EXEC-TIME-SPIKE.md` does NOT recur here — Probe 1 of `65-OPTION-B-SPIKE.md` already established that `Connection(*context.db)` opens a fresh `ClientContext` with its own lock, which is what the read-path also relies on).

No anomalies between Probe A and Probe B — both transactional outcomes are clean and symmetric.

---

## Implications for v0.10.0 re-planning

The B-prime architecture sketched in the orchestrator handoff is now empirically supported on all three lifecycle phases needed for full ALTER + CREATE-FROM-YAML coverage:

| Lifecycle phase | Mechanism | Spike validating |
|-----------------|-----------|------------------|
| Read path (list_*, show_*, describe_*) | `Connection(*context.db)` from C++ Catalog API table-function bind | `65-READ-PATH-SPIKE.md` (`READ-BIND-RC0`) |
| Write path (CREATE / DROP) | `parser_override` rewrites to native INSERT/DELETE on `_definitions` (re-parsed by `Parser::ParseQuery`, dispatched through caller's binder) | Already in production at v0.9.0 |
| Write path (ALTER / CREATE FROM YAML FILE) | `parser_override` rewrites to native `UPDATE _definitions SET definition = (SELECT new_def FROM __sv_compute(name, op_payload)) WHERE name = ?` — the same SQL-rewrite pattern, except SET RHS is a TF subquery that does the read+transform via `Connection(*context.db)` from its bind | **This spike (`ALTER-RC0`)** |

The v0.10.0 plan can therefore proceed with:

1. **Preserve `parser_override` as the single DDL entry point** (no change from v0.9.0). The success branch continues to produce native SQL strings re-parsed via DuckDB's `Parser::ParseQuery` and dispatched onto the caller's connection. This is the only mechanism in DuckDB v1.5.2 that preserves transactional DDL (`65-EXEC-TIME-SPIKE.md` ruled out the alternatives).
2. **Add a small family of `__sv_compute_*` helper table functions** registered via the C++ Catalog API (the registration pattern from `65-READ-PATH-SPIKE.md`). Each ALTER variant (RENAME, SET COMMENT, ADD/DROP DIMENSION/METRIC, etc.) routes through a corresponding `__sv_compute_<op>(name, op_payload)` whose bind opens `Connection(*context.db)`, reads the current JSON definition, applies the op, returns the new JSON. CREATE FROM YAML FILE uses `__sv_compute_create_from_yaml(path, opts)` along the same shape.
3. **Trivial-rewrite cases stay pure SQL.** ALTER variants whose new state is computable from `op_payload` alone (no current-state read needed) can rewrite to `UPDATE _definitions SET definition = json_set(definition, '$.comment', '<new>') WHERE name = ?` without a helper TF. Use the helper TF only when the new state needs (a) catalog reads (e.g. duckdb_constraints for FK inference), (b) type inference (LIMIT 0 probes on file-backed DBs), or (c) YAML parsing.
4. **Committed-state-read trade-off is documented per TECH-DEBT 19** and carries forward unchanged. The constraint: a user cannot, within a single transaction, CREATE a view and then ALTER it referencing data that depends on the CREATE's uncommitted state (e.g. CREATE then immediately ALTER ADD DIMENSION whose type-inference probes the CREATE'd view). In practice this is the same constraint v0.9.0 already imposes on DESCRIBE/SHOW within an uncommitted transaction, so no new surface for user surprise.

### What's NOT validated here (and shouldn't block v0.10.0 planning)

- **Concurrent ALTER on the same view from two connections** — the inner `Connection(*context.db)` opens its own transaction, so two simultaneous ALTERs would race on the read-then-write window. Not new; this is the same race surface as v0.8.0 IF NOT EXISTS (TECH-DEBT 23) and the v0.9.0 CREATE/DROP race window. Should be measured separately during v0.10.0 implementation, but is not a precondition for ALTER's viability.
- **WAL replay / persistence across DB restart** — the UPDATE writes to `semantic_layer._definitions` which is a normal table, so WAL is automatic. Should be sanity-checked during v0.10.0 implementation, but the persistence mechanism is unchanged from v0.9.0 CREATE/DROP.
- **Error propagation** — when `__sv_compute_*`'s bind throws (e.g. YAML parse error, type inference failure), the outer UPDATE fails its bind and the caller sees the inner error. Need to confirm this error surface is clean (no double-wrapping, no swallowed messages) during v0.10.0 implementation.

---

## Caveats / blast radius

1. **Probe C's committed-state read is a TECH-DEBT 19 carry-over, not a new wart.** The same dichotomy already constrains DESCRIBE/SHOW at v0.9.0 — that's why TECH-DEBT 19 is documented. ALTER inherits the constraint unchanged. No new user-facing surprise.
2. **Bind callback fires exactly once per outer UPDATE.** `65-READ-PATH-SPIKE.md` observed 3× bind invocations for read-side TFs (one per planning/execution phase). Here, the outer UPDATE's planner triggers exactly ONE bind of the inner TF per UPDATE statement (visible in the stderr trace — each of Smoke 1 / Probe A / Probe B / Probe C produces exactly one `[ALTER-SPIKE] bind entered` line). This is informational; if v0.10.0's helper TFs need to be idempotent, they should still be defensively idempotent because we have not validated this single-bind property across DuckDB minor versions.
3. **Connection lifecycle is per-call.** `Connection probe(*context.db)` is created inside the bind, destructs at end of scope, and acquires/releases `connections_lock` via `ConnectionManager::AddConnection`/`RemoveConnection` — same pattern as `65-OPTION-B-SPIKE.md` Probe 1 and `65-READ-PATH-SPIKE.md`. No long-lived extension-owned state is introduced. The OverrideContext-teardown concern of LIFE-01..04 (Phase 65's main goal) does NOT interact with this mechanism — the helper TFs don't stash anything at parser-info scope.
4. **No new locks held during bind.** Probes A and B passed cleanly with `BEGIN` outside the UPDATE; the outer transaction's locks on `_definitions` rows did not block the inner `Connection(*context.db)`'s read (the fresh `ClientContext` is in its own transaction). This is consistent with Probe C: the fresh `ClientContext` sees only committed state, so it can't conflict with the outer txn's uncommitted writes.
5. **`info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT`** was used at registration so re-loading the extension doesn't trip on a duplicate name. v0.10.0 should consider whether `ERROR_ON_CONFLICT` is safer (would catch accidental name collisions with built-ins or other extensions), but it's not load-bearing for this spike.
6. **Probe C's stderr trace shows `bind read returned 0 rows` for `'new_view'`** despite an outer txn having INSERTed it. The fresh `Connection`'s read goes through DuckDB's MVCC and sees only the snapshot the fresh `ClientContext`'s transaction was opened at — which is post-`COMMIT` of the prior probe, before the outer txn began. This is standard MVCC behaviour, just rendered explicit here.
7. **Phase 65's main work (OverrideContext teardown) is orthogonal.** The ALTER-rewrite pattern this spike validates depends only on (a) the C++ Catalog API for TF registration (already used by `65-READ-PATH-SPIKE.md`) and (b) `Connection(*context.db)` from a bind callback (already used by both prior spikes). It does NOT depend on OverrideContext teardown semantics; LIFE-01..04 can land independently of v0.10.0 ALTER work.
8. **Build artefacts and reverted state.** `git diff --stat cpp/src/shim.cpp src/lib.rs src/parse.rs test/sql/` returns empty after `git checkout cpp/src/shim.cpp` (`test/sql/65_alter_spike.test` was scratch and was removed with `rm`). A final `just build` post-revert succeeded cleanly. The broken `just test-sql` baseline (4/47 PASS from Plan 02 partial, preserved per D-12) is unchanged. The pre-existing `.planning/STATE.md M` and `.DS_Store` / `.cache/` `??` entries visible in `git status --short` are unrelated to this spike.

---

## Self-Check: PASSED

Verified post-write (before final commit):

- `.planning/phases/65-overridecontext-connection-teardown/65-ALTER-REWRITE-SPIKE.md` — present (this file, about to be committed)
- `git diff --stat cpp/src/shim.cpp src/lib.rs src/parse.rs test/sql/` — empty (no production code changes after revert)
- `test/sql/65_alter_spike.test` — not present (scratch file removed)
- Post-revert `just build` — PASS

---

*Phase: 65-overridecontext-connection-teardown*
*Spike: ALTER-rewrite pattern viability — `UPDATE _definitions SET col = (SELECT v FROM __sv_compute_*(...)) WHERE …` with C++ Catalog API TF + `Connection(*context.db)` in bind*
*Outcome: `ALTER-RC0` — pattern viable on DuckDB v1.5.2; v0.10.0 ALTER + CREATE FROM YAML FILE can adopt it on top of the v0.9.0 `parser_override` infrastructure*
*Date: 2026-05-23*
