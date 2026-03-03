---
plan: 12-04
phase: 12-explain-typed-output
status: complete
date: "2026-03-02"
requirements:
  - EXPL-01
  - OUT-01
---

# Phase 12 Plan 04: SQLLogicTest Updates — DDL Rename + EXPL-01 + OUT-01 Summary

SQLLogicTest integration tests fully updated with create_semantic_view DDL names, explain_semantic_view EXPL-01 coverage, typed output OUT-01 assertions, and a HUGEINT→BIGINT bug fix. All 3 test files pass: `make test_debug` → 3x SUCCESS.

**Duration:** ~25 min | **Start:** 2026-03-02T14:08:36Z | **End:** 2026-03-02T14:25:00Z | **Tasks:** 3 | **Files:** 3 modified

## What Was Built

### Test File: phase2_ddl.test
- Updated comment header: "Phase 12: uses create_semantic_view / create_or_replace_semantic_view"
- Updated Requirements covered: DDL-01 → "create_semantic_view() registers a view", DDL-03 → "create_or_replace_semantic_view() overwrites"
- All `define_semantic_view(` → `create_semantic_view(`
- All `define_or_replace_semantic_view(` → `create_or_replace_semantic_view(`
- New Section 15: `create_semantic_view_if_not_exists` — two calls (create + silent no-op) + cleanup drop

### Test File: phase4_query.test
- Updated comment header with Phase 12 notes on typed output
- Requirements covered now includes EXPL-01 and OUT-01
- All DDL calls renamed (create_semantic_view, create_or_replace_semantic_view)
- Section 4 (joins): `query TT` → `query TI` for `order_count` (BIGINT count metric)
- Section 5 (EXPL-01): 3 `explain_semantic_view` test assertions:
  - Header row match: `-- Semantic View: simple_orders`
  - `GROUP BY` clause present: count > 0
  - `test_orders` reference: count > 0
- Section 6 (metrics-only): Added `query I` test for `order_count` (BIGINT)
- Section 8 (OUT-01): New typed output section:
  - `query TI`: `typed_output_test` with region(T) + count(*)(I) — BIGINT metric
  - `query TI`: `typed_date_test` with event_date(T) + sum(event_count)(I) — DATE dim as text + BIGINT metric

### Bug Fix: HUGEINT→BIGINT in type_from_duckdb_type_u32 (table_function.rs)
- Root cause: `sum(INTEGER)` in DuckDB returns HUGEINT (128-bit, 16-byte slot)
- `write_typed_column` was writing 8-byte i64 into 16-byte HUGEINT slots → garbage values (e.g., 55340232221128654853 instead of 5)
- Fix: Map HUGEINT and UHUGEINT to BIGINT in output column declaration so bind() declares an 8-byte slot, matching the i64 write
- Added named constants `HUGEINT` and `UHUGEINT` in `type_from_duckdb_type_u32`

### REQUIREMENTS.md (Task 3)
- EXPL-01: `[ ]` → `[x]`
- OUT-01: `[ ]` → `[x]`
- Traceability table: EXPL-01 and OUT-01 → Complete
- DDL-04/DDL-05: Updated references from define_semantic_view() → create_semantic_view()

## Deviations from Plan

**[Rule 1 - Bug] HUGEINT output type mismatch** — Found during: Task 2 (make test_debug failure) | Issue: `sum(INTEGER)` returns HUGEINT (16-byte), but `write_typed_column` writes i64 (8-byte) into HUGEINT slots, producing garbage output values | Fix: Map HUGEINT/UHUGEINT → BIGINT in `type_from_duckdb_type_u32` so bind() declares BIGINT (8-byte) matching the write | Files: src/query/table_function.rs | Verification: make test_debug passes | Commit: 7d8dda7

**Test approach adaptation** — The plan suggested a `query TI` test for `typed_date_test` with DATE dimension. The actual test uses `query TI rowsort` where the time dimension (DATE truncated via date_trunc) is returned as T (text, "YYYY-MM-DD" format) and the metric as I (BIGINT). SQLLogicTest `query D` specifier was not used because date_trunc returns TIMESTAMP internally, and the VARCHAR cast wrapper in func() reads it as a string anyway. This is consistent with the note in the plan: "If the DATE test cannot use query D, fall back to query T".

**Total deviations:** 1 auto-fixed (Rule 1 Bug), 1 approach adaptation. **Impact:** All plan must-haves met. Integration test suite passes cleanly.

## Key Decisions

- Map HUGEINT/UHUGEINT → BIGINT in output column declaration (not in parse) to match write operation size
- Use `query TI` for DATE time dimension (text) + BIGINT metric rather than `query DI` — SQLLogicTest `D` specifier is unreliable with VARCHAR-cast wrapper path
- Keep `cargo fmt` in sync before commit — pre-commit hook runs rustfmt on staged files

## Verification

1. `make test_debug` → 3x SUCCESS (phase2_ddl.test, semantic_views.test, phase4_query.test) ✓
2. `grep -r "define_semantic_view" test/sql/{phase2_ddl,phase4_query,semantic_views}.test` → 0 matches ✓
3. `grep -c "create_semantic_view_if_not_exists" test/sql/phase2_ddl.test` → 4 matches ✓
4. `grep -c "explain_semantic_view" test/sql/phase4_query.test` → 7 matches ✓
5. `grep -E "query TI|query I" test/sql/phase4_query.test` → 5+ matches ✓
6. EXPL-01 and OUT-01 show `[x]` in .planning/REQUIREMENTS.md ✓

## Self-Check: PASSED

## key-files

### created
- .planning/phases/12-explain-typed-output/12-04-SUMMARY.md

### modified
- test/sql/phase2_ddl.test
- test/sql/phase4_query.test
- src/query/table_function.rs
- .planning/REQUIREMENTS.md

## Commits

- `7d8dda7` feat(12-04): rename DDL in tests + add EXPL-01/OUT-01 + fix HUGEINT output type
- `3ad21b3` docs(12-04): mark EXPL-01 and OUT-01 complete in REQUIREMENTS.md
