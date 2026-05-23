# Phase 65: OverrideContext Connection Teardown - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-23
**Phase:** 65-overridecontext-connection-teardown
**Areas discussed:** Treatment of Plan 02 partial commits; PK auto-inference removal — user migration; ALTER helper TF granularity + DDL-time semantics; Plan/wave structure for re-plan

This is the third re-plan for Phase 65. Architecture was already locked by ROADMAP's Phase 65 entry (preserve `parser_override`, eliminate catalog reads inside it, ALTER via UPDATE-with-TF-subquery, read-path via C++ Catalog API shim). Discussion focused on implementation choices on top of that locked architecture.

---

## Treatment of Plan 02 partial commits

### Q1 — baseline

| Option | Description | Selected |
|---|---|---|
| Hard-revert commits 0d2c0b7 + f9caafe | Roll src/parse.rs OverrideContext + cpp shim signature back to v0.9.0 (Phase 62 shape with INTENTIONAL LEAK). New plan retires H1 entirely. | ✓ |
| Keep db_handle plumbing, surgically remove broken ConnGuard::open sites | Preserve FFI signature change as 'might be useful later'; remove 4× ConnGuard::open + CatalogReader calls. | |
| Keep db_handle, repurpose for different use | Find an actual need for db_handle inside parser_override under new architecture. Speculative. | |

**User's choice:** Hard-revert commits 0d2c0b7 + f9caafe.
**Notes:** Drops D-12 (PRE-BPRIME) "reusable foundation" framing — under read-elimination, db_handle is genuinely unused by parser_override.

### Q2 — ConnGuard module fate

| Option | Description | Selected |
|---|---|---|
| Keep ConnGuard module + watchdog tests; let ConnGuard go unused | ~200 LOC kept; watchdog tests stay (LIFE-01 evidence). | |
| Delete ConnGuard module too (full revert of Plan 01 code, keep tests) | Cleanest; watchdog tests stay. | |
| Find a Rust consumer for ConnGuard | Speculative under read-elimination. | |

**User's choice:** Remove ConnGuard; timing flexible but no "we tried this" archaeology marker in source code.
**Notes:** User explicitly rejected leaving ConnGuard as a marker — that goes in `.planning/` notes only. Captured as D-02 (delete in Plan 03's slimming wave). Watchdog tests stay per D-03.

---

## PK auto-inference removal — user migration

### Q1 — new error surface

| Option | Description | Selected |
|---|---|---|
| Hard error at CREATE/ALTER with actionable message | Fail fast; clear migration path message. | ✓ |
| Hard error only when query needs the join | Defers pain; produces misleading 'CREATE succeeded then query fails'. | |
| WARN at CREATE, hard error at query time | DuckDB warning surface is thin; users might miss. | |

**User's choice:** Hard error at CREATE/ALTER with actionable message.
**Notes:** Captured as D-06 with specific message text.

### Q2 — existing persisted definitions

| Option | Description | Selected |
|---|---|---|
| Leave existing persisted definitions untouched; error only on mutation | Zero-friction upgrade for unmodified views. | ✓ |
| Validate on LOAD, error if any view's definition was built via auto-inference | Heavy-handed; requires marker we don't have. | |
| Validate on first SELECT through semantic_view() | Adds runtime cost; ambiguous surface. | |

**User's choice:** Leave existing persisted definitions untouched; error only on mutation.
**Notes:** Captured as D-07.

---

## ALTER helper TF granularity + DDL-time semantics

### Q1 — helper TF strategy

| Option | Description | Selected |
|---|---|---|
| Pure-SQL json_set for trivial + helper TFs only for catalog/YAML reads | Trivial variants via UPDATE _definitions SET definition = json_set(...); helpers only when ClientContext reads needed. | ✓ |
| One mega __sv_compute_alter(name, op_json) dispatcher | Single TF dispatches internally. | |
| One helper TF per ALTER variant | Maximal type-safety; ~10 TFs of boilerplate. | |

**User's choice:** Pure-SQL json_set for trivial + helper TFs only for catalog/YAML reads.
**Notes:** Captured as D-09 / D-10 / D-11 / D-12. Variants categorized in CONTEXT.md `<specifics>` table.

### Q2 — SHOW/DESCRIBE data_type column under deferred inference

| Option | Description | Selected |
|---|---|---|
| SHOW/DESCRIBE itself runs the LIMIT 0 probe on first call | Same v0.9.0 behavior; session-cached process-local. | ✓ |
| Return NULL/empty until a SELECT runs, then populate | User-visible change; breaks v0.7.1 behavior. | |
| Persist inferred types in _definitions at CREATE time after all | Defeats read-elimination goal. | |

**User's choice:** SHOW/DESCRIBE runs the LIMIT 0 probe on demand at bind time.
**Notes:** Captured as D-16 / D-17. Cache shape is Claude's discretion.

---

## Plan/wave structure for re-plan

### Q1 — plan grouping

| Option | Description | Selected |
|---|---|---|
| 5 plans, grouped by architectural concern | Slimming / ALTER / read-path / close-out / release-prep. | ✓ (4 plans — release-prep moves to Phase 66) |
| 7-8 atomic plans, one per concern | Maximal traceability; more checkpoint overhead. | |
| 3 mega-plans grouped by lifecycle | Coarser; harder to roll back partial wave. | |

**User's choice:** 5 plans, but Q2 below confirmed release-prep belongs to Phase 66, so Phase 65 = 4 plans (03-06).
**Notes:** Captured as D-18 / D-19.

### Q2 — Phase 65 / 66 boundary

| Option | Description | Selected |
|---|---|---|
| Phase 65 owns lifecycle fix only; Phase 66 owns expansion qualification + ADBC tests + CHANGELOG + version bump | Matches current ROADMAP. | ✓ |
| Phase 65 absorbs release prep too | If ADBC tests reveal surviving qualification bugs, milestone re-opens. | |

**User's choice:** Phase 65 owns lifecycle fix only.
**Notes:** Captured as D-20.

---

## Claude's Discretion

- Process-local type-inference cache shape (D-16 / Claude's Discretion in CONTEXT.md).
- Exact JSON path strings for D-09's json_set calls.
- Whether Plan 04's helper TFs live in cpp/src/alter_helpers.cpp or extend shim.cpp.
- Structural test mechanism for "no long-lived duckdb_connection in init_extension" (Plan 06).
- Test layout for ALTER coverage (sqllogictest vs Python integration vs both).

## Deferred Ideas

See CONTEXT.md `<deferred>` section. Highlights: EXPAND-CTX-01..03, ADBC query test harness, CHANGELOG, version bump, _notes/ cleanup — all Phase 66. TECH-DEBT #19/#21/#23/#24 unchanged. Long-lived native-handle audit findings get surfaced as TECH-DEBT/Phase 66 follow-ups rather than absorbed.

---

*Phase: 65-overridecontext-connection-teardown*
*Logged: 2026-05-23*
