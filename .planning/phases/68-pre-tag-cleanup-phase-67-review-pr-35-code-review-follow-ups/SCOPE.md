---
phase: 68
status: scope-draft
created: 2026-05-27
parent_artifacts:
  - .planning/phases/67-expansion-sql-coverage-and-tech-debt-cleanup/67-REVIEW.md
  - TECH-DEBT.md (item #25)
  - PR #35 Copilot review comments (3 inline)
---

# Phase 68 Scope Draft — Pre-Tag Cleanup

Final cleanup phase before v0.10.0 milestone close. Addresses three sources of small follow-up items:

1. **Phase 67 REVIEW findings** (`67-REVIEW.md` — code review of Phase 67 changes)
2. **TECH-DEBT #25** (sibling `split_whitespace` sites surfaced by Phase 67 Plan 02 audit-grep)
3. **PR #35 Copilot review comments** (3 inline comments on the v0.10.0 milestone PR)

All items are non-blocking; this phase is hygiene before tag-and-merge.

## Item Inventory

### A. Phase 67 REVIEW.md follow-ups

| ID | File | Severity | Description |
|----|------|----------|-------------|
| A1 | `src/body_parser.rs:692-720` | Warning | **WR-01** — identifier walk in `parse_single_table_entry` regresses error path for malformed `o AS PRIMARY KEY (id)` DDL. Currently consumes `PRIMARY` as a bare table name, producing "table 'PRIMARY' does not exist" instead of the pre-fix "Missing physical table name after AS". One-line reserved-keyword guard after the identifier walk. |
| A2 | `test/integration/test_adbc_queries.py:470` | Warning | **WR-02** — `ATTACH '{other_db_path}' AS db2` interpolates without the SQL-string escape (`.replace("'", "''")`) applied to `extension_path` at line 100 in IN-02. Defense-in-depth parity. Low practical risk (temp-dir path), but inconsistent. |
| A3 | `src/body_parser.rs:704-707` | Warning | **WR-03** — Dead dot-consumption loop arm. `find_identifier_end` already walks across dots internally (proven by `fqn_with_quoted_parts_runs_to_whitespace` test); the loop's comment misrepresents what the helper does. Remove the dead arm. |
| A4 | `src/body_parser.rs` | Info | **IN-01** — Unterminated quoted source-table name (`o AS "unclosed`) silently accepted as malformed name; should reject at parse time. |
| A5 | `test/sql/phase67_quoted_source_tables.test` | Info | **IN-02** — No test coverage for mixed bare/quoted dot-qualified names (e.g. `staging."my orders"`). Add fixture row. |
| A6 | `test/sql/phase67_quoted_source_tables.test` | Info | **IN-03** — Cleanup misses 3 default-schema base tables. Add `DROP TABLE` statements. |
| A7 | `src/body_parser.rs` | Info | **IN-04** — `find_primary_key` word-boundary check differs from `find_unique` (no `_` exclusion). Align. |

### B. TECH-DEBT #25 — sibling split_whitespace sites

| ID | File | Description |
|----|------|-------------|
| B1 | `src/body_parser.rs::parse_non_additive_dims` | Tokenises on whitespace — breaks for quoted identifiers containing literal whitespace in the `NON ADDITIVE BY (...)` clause. Phase 67 classified as **(c)-class structural-rewrite-required** (clause shape differs from #24's `TABLES (...)` so cannot port `find_identifier_end` the same way). |
| B2 | `src/body_parser.rs::parse_window_spec` (OVER ORDER BY) | Same bug class for the `OVER (... ORDER BY ...)` clause of window-metric DDL. Same (c)-class classification. |

User decision (gsd-phase scope confirmation 2026-05-27): include in this phase. The structural rewrite is in scope; if investigation reveals the rewrite is significantly larger than expected, surface as a SUMMARY finding and the phase plan can renegotiate.

### C. PR #35 Copilot Review Comments

| ID | File:Line | Comment |
|----|-----------|---------|
| C1 | `tests/registration_error_surfaces.rs:136` | `&body[..body.len().min(400)]` can panic on non-UTF-8 char boundary in the assertion's error-formatting path. Swap to `body.get(..400).unwrap_or(body)` or `chars().take(400)`. |
| C2 | `tests/registration_error_surfaces.rs:171` | Transmute invariant needle `["std::", "mem::", "transmute("]` misses turbofish form `std::mem::transmute::<T, U>(...)`. Drop the `(` so both forms catch. |
| C3 | `test/sql/p651_ok.yaml` | Checked-in fixture is unused — the sqllogictest `phase651_yaml_filesystem_access_gating.test` writes its own `__TEST_DIR__/p651_ok.yaml` at runtime via `COPY (...) TO ...` and never reads the checked-in file. Either delete the dead fixture, or rewrite the test to load it. |

## Suggested Wave Grouping (for plan-phase)

- **Wave 1 (mechanical, parallelisable):** A1, A2, A3, A4 (body_parser surgical fixes) + A7 (alignment) + C1, C2, C3 (test file fixes) + A5, A6 (sqllogictest fixture polish)
- **Wave 2 (depends on Wave 1 building):** B1 + B2 — TECH-DEBT #25 structural rewrite (needs the WR-01 keyword guard in place first since it touches the same parser)

## Out of Scope

- REL-01..04 (CHANGELOG, version bump, example file, DuckDB v1.5.3 dep bump) — these are milestone-close tasks, not phase tasks, per `feedback_defer_release_tasks.md`. Handled by `/gsd-complete-milestone` after Phase 68 verifies.

## Quality Gates

`just test-all` (Rust + sqllogictest + DuckLake CI + ADBC) post-merge. No new external dependencies expected.

## Provisional Plan Breakdown (for /gsd-plan-phase 68)

1. **68-01** — Phase 67 REVIEW mechanical fixes (A1–A7)
2. **68-02** — PR #35 Copilot review fixes (C1, C2, C3)
3. **68-03** — TECH-DEBT #25 structural rewrite (B1, B2) — depends on 68-01 for the WR-01 keyword guard to be in place

Three plans, two waves (68-01 + 68-02 parallel in Wave 1; 68-03 in Wave 2).
