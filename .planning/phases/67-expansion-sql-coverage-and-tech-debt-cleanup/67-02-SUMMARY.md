---
phase: 67-expansion-sql-coverage-and-tech-debt-cleanup
plan: 02
subsystem: body_parser
tags: [tech-debt, body-parser, quoted-identifiers, find_identifier_end]
requires:
  - src/ident.rs::find_identifier_end (Phase 64, shipped — unchanged)
provides:
  - identifier-aware tokenisation in src/body_parser.rs::parse_single_table_entry
  - regression fixture: test/sql/phase67_quoted_source_tables.test
  - TECH-DEBT #24 closed
affects:
  - src/body_parser.rs (1 file modified)
  - TECH-DEBT.md (#24 → ✅ RESOLVED; #25 added)
  - test/sql/TEST_LIST (1 entry added)
  - test/sql/phase67_quoted_source_tables.test (new)
tech-stack:
  added: []
  patterns:
    - dot-separated identifier walk via find_identifier_end + post-name keyword scan
key-files:
  created:
    - test/sql/phase67_quoted_source_tables.test
  modified:
    - src/body_parser.rs (parse_single_table_entry surgery + 5 new unit tests)
    - test/sql/TEST_LIST (registration)
    - TECH-DEBT.md (#24 resolved, #25 added)
decisions:
  - D-09 honored: src/ident.rs NOT modified; find_identifier_end reused as-is
  - D-10 honored: audit-grep over body_parser.rs ran; 2 sibling sites
    classified as (c)-class structural-rewrite-required, surfaced as
    TECH-DEBT #25 rather than fixed in-phase
  - D-11 honored: separate test/sql/phase67_quoted_source_tables.test
    (default per CONTEXT.md, not folded into phase67_qualified_emission.test)
metrics:
  duration: ~1.5h (wall clock, single executor wave)
  tasks: 4
  completed_date: 2026-05-27
---

# Phase 67 Plan 02: Body Parser Quoted-Identifier Fix (TECH-DEBT #24)

One-liner: Reused `src/ident.rs::find_identifier_end` (Phase 64) at the single remaining body-parser capture site (`parse_single_table_entry`) and shipped a 4-scenario sqllogictest fixture proving the canonical `"weird PRIMARY KEY name"` pathological case now parses correctly.

## What Changed

### 1. Surgery: `src/body_parser.rs::parse_single_table_entry`

**Commit:** `256ae65`

**Where:** `src/body_parser.rs` — added import at line 7 (`use crate::ident::find_identifier_end;`), restructured Step 3 of `parse_single_table_entry` (function starts at line 660). Pre-fix lines 683-740 (`upper = after_as.to_ascii_uppercase()` → `find_primary_key(&upper)` → `after_as[..pk_start].trim()`) replaced with:

- New lines 683-720: identifier-aware capture loop. Walks dot-separated segments via `find_identifier_end(&after_as[name_end..], /* allow_paren */ true)` until no further `.` separator follows; `table_name` is `after_as[..name_end].trim()`.
- New lines 722-768: `find_primary_key` / `find_unique` now run on the post-name slice (`after_name`, byte offset `after_as_offset + name_end`). Error positions are adjusted to `after_name_offset + pk_end`.

**Behaviour preserved byte-for-byte** for the four happy-path cases the existing 47-fixture sqllogictest suite + 130+ body_parser unit tests cover:
- `o AS orders PRIMARY KEY (id)` — unqualified + PK
- `o AS main.orders PRIMARY KEY (...)` — schema-qualified + PK
- `f AS orders UNIQUE (col)` — no PK, has UNIQUE
- `o AS orders` — bare table, no PK, no UNIQUE

**Bug-class now correctly handled:**
- `"my orders" PRIMARY KEY (id)` — internal whitespace in quoted name
- `"my db"."schema"."my table" PRIMARY KEY (id)` — 3-part quoted FQN with whitespace in 2 segments
- `"weird PRIMARY KEY name" PRIMARY KEY (id)` — **canonical TECH-DEBT #24 case**: quoted name CONTAINING the literal `PRIMARY KEY` substring. Pre-fix, the case-insensitive substring scan over the whole post-AS slice matched the inner-quoted keyword and split mid-identifier.

**New unit tests** (all under `src/body_parser.rs::tests`, all passing):
1. `test_parse_single_table_entry_quoted_with_internal_whitespace`
2. `test_parse_single_table_entry_quoted_containing_primary_key_substring`
3. `test_parse_single_table_entry_3part_quoted_fqn_with_whitespace`
4. `test_parse_single_table_entry_regression_no_whitespace`
5. `test_parse_single_table_entry_quoted_with_unique_no_pk`

### 2. Audit grep — Task 2 / D-10

**Commit:** (no source-code change committed; finding folded into the TECH-DEBT commit `56065ff`.)

**Grep:** `grep -n "split_whitespace\|split(' ')" src/body_parser.rs` (post-surgery line numbers — Task 1 added imports so the line numbers shifted by 1 from the plan's pre-edit reference of 1391/1707):

| File:Line | Site | Classification | Disposition |
|---|---|---|---|
| `src/body_parser.rs:1415` | `parse_non_additive_dims` — splits each `NON ADDITIVE BY` entry to peel off the dimension reference (token 0) then ASC/DESC/NULLS-FIRST/LAST modifier keywords (tokens 1..) | **(c)** real bug: a quoted-with-whitespace dimension reference would split mid-identifier; same identifier-vs-whitespace class as TECH-DEBT #24 | **Not fixed in-phase.** The modifier loop keys off `Vec<&str>` index positions to parse `ASC`/`DESC`/`NULLS FIRST`/`NULLS LAST` — a token-aware rewrite is **structural**, not a mechanical helper-reuse. Surfaced as **new TECH-DEBT #25**. |
| `src/body_parser.rs:1731` | `parse_window_spec` OVER `ORDER BY` parser — same shape as 1415 (dim_name + ASC/DESC/NULLS modifiers) | **(c)** real bug — sibling pattern | **Not fixed in-phase.** Same rationale. Captured under the same TECH-DEBT #25 entry. |

The plan's expected default (D-10: "Default expectation per CONTEXT.md: the one site SCOPE.md identifies is the only one") was for these to be (b)-class keyword-tokenisation. On audit they are actually (c)-class identifier slots with structural fix scope. Per the plan's framework for (c)-class findings ("if non-mechanical to fix: append a new entry to TECH-DEBT.md"), the deviation is the planned escape hatch, not scope creep.

### 3. sqllogictest fixture

**Commit:** `5fb2ed4`

**Created:** `test/sql/phase67_quoted_source_tables.test` (157 lines, 4 scenarios)
**Registered:** `test/sql/TEST_LIST` line 58

Scenario coverage maps onto TECH-DEBT #24 cases:

| Scenario | Input | TECH-DEBT #24 case exercised |
|---|---|---|
| 1 | `TABLES (o AS "my orders" PRIMARY KEY (id))` | Internal whitespace in single-part quoted name |
| 2 | `TABLES (o AS "stage 2"."my orders" PRIMARY KEY (id))` | Multi-part quoted FQN with whitespace in two segments |
| 3 | `TABLES (o AS "weird PRIMARY KEY name" PRIMARY KEY (id))` | **Canonical TECH-DEBT #24 case** — quoted name containing the literal `PRIMARY KEY` substring |
| 4 | `TABLES (o AS p67_plain_orders PRIMARY KEY (id))` | Regression baseline — unquoted, no whitespace — proves the fix did not regress the happy path |

Each scenario asserts:
- `semantic_view(...)` returns at least one row (smoke check that the rewritten SQL resolves end-to-end via DuckDB execution)
- `explain_semantic_view(...) WHERE explain_output LIKE '%FROM "..."%'` matches a verbatim quoted source-table reference, proving the source-table name survives CREATE → catalog persist → expansion emission with no whitespace truncation

### 4. TECH-DEBT.md updates

**Commit:** `56065ff`

- Entry #24: status flipped from ❓ to ✅; resolution annotation added inline citing commits `256ae65` (surgery) + `5fb2ed4` (fixture).
- Entry #25 added under a new `## v0.10.0 additions` section, documenting the sibling-slot finding from the audit-grep (NON ADDITIVE BY + OVER ORDER BY). Includes Origin (Phase 67 Plan 02 audit-grep), Decision (technical description), Why deferred (structural-rewrite + vanishingly-rare-case argument), and Action-if-a-user-hits-this (recommendation to define dimensions with bare names; outline of the structural fix if needed).

## Verification

| Gate | Result | Notes |
|---|---|---|
| `cargo test --lib body_parser` | **PASS** 135 tests | All 5 new tests pass; all prior body_parser tests preserved. |
| `cargo test --lib` | **PASS** 855 tests | Full lib test suite green. |
| `just test-rust` (`cargo nextest`) | **PASS** 955 tests | Full Rust unit + proptest suite. |
| `cargo clippy -- -D warnings` (Justfile `lint` recipe) | **PASS** | Pre-commit hook clean. |
| `cargo fmt --check` | **PASS** | Pre-commit hook clean. |
| `just build` | **PASS** | Debug extension binary builds. |
| `just test-sql` | **PASS** 58 tests run, 0 failed (60/60 incl. excluded) | New `phase67_quoted_source_tables.test` reported `SUCCESS` (index 46/60). |

### `just test-all` partial completion

`just test-rust` and `just test-sql` (the two gates that exercise our code changes) both passed. The downstream integration tests in `just test-all` (`test-ducklake-ci`, `test-vtab-crash`, `test-caret`, `test-adbc`, `test-adbc-queries`, `test-large-view`, `test-multi-db`, `test-readonly`, `test-concurrent`) panic at `uv` startup with:

```
thread 'main2' panicked at system-configuration-0.6.1/src/dynamic_store.rs:154:
Attempted to create a NULL object.
thread 'main' panicked at uv-0.9.18/crates/uv/src/lib.rs:2540:
Tokio executor failed, was there a panic?: Any { .. }
```

This is a `uv` / macOS `SCDynamicStore` interaction blocked by the sandbox in this executor environment — it occurs at process startup before any test Python is loaded, so it is **not** a defect in this plan's changes. The executor attempted `dangerouslyDisableSandbox: true` per CLAUDE.md Rule 2 (which lists `uv run test/integration/*.py` as pre-approved for the bypass) but the bypass was denied by the harness in this worktree.

**Mitigations / risk assessment:**

- All cargo-side gates pass on 955 tests including 5 new ones directly exercising the surgery.
- The full 60-test sqllogictest suite passes, including the new 4-scenario fixture that exercises CREATE → catalog persist → query → expansion roundtrip — the same end-to-end path the failing integration tests would exercise.
- The surgery is contained to a single function in `src/body_parser.rs`; it does not touch FFI, threading, transactions, multi-DB ATTACH, ADBC, or any of the surfaces the deferred integration tests target.
- The verifier or follow-up run on a host without the sandbox restriction can run `just test-all` + `just ci` to close the gap.

## Deviations from Plan

Per the plan's deviation rules:

### Auto-fixed Issues — None

The plan was executed as written. No Rule 1 (bug), Rule 2 (missing critical functionality), or Rule 3 (blocking issue) auto-fixes were applied to in-scope files.

### Auto-fixed environment setup (worktree)

Two environment-level fixes were required to make the worktree buildable; neither is a code change:
- **Missing git submodule:** `git submodule update --init --recursive` to populate `extension-ci-tools/` (referenced by `Makefile`).
- **Missing DuckDB amalgamation:** `cpp/include/duckdb.{hpp,cpp}` were absent (the gitignored amalgamation is normally downloaded by `make ensure_amalgamation`, but the network fetch was blocked by the sandbox). Restored by copying from the main repo's cache at `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/.amalgamation/v1.5.2/`. No code change.

### Audit-grep classification deviation

The plan's D-10 default expectation was that sibling `split_whitespace` sites would be (b)-class keyword-tokenisation. On audit they were actually (c)-class identifier-slot tokenisation with structural-rewrite scope. Per the plan's framework, the (c)-class disposition (new TECH-DEBT entry rather than in-phase fix) is the planned escape hatch for this finding — see Task 2 audit table above and TECH-DEBT.md #25.

## Self-Check

Verifying claims before completion:

**Created files:**
- `[FOUND]` `test/sql/phase67_quoted_source_tables.test` — 157 lines, 4 scenarios
- `[FOUND]` `.planning/phases/67-expansion-sql-coverage-and-tech-debt-cleanup/67-02-SUMMARY.md` (this file)

**Modified files:**
- `[FOUND]` `src/body_parser.rs` — `find_identifier_end` import + `parse_single_table_entry` surgery + 5 new tests
- `[FOUND]` `test/sql/TEST_LIST` — `test/sql/phase67_quoted_source_tables.test` registered
- `[FOUND]` `TECH-DEBT.md` — #24 marked ✅ RESOLVED; #25 added

**Commits (verify via `git log --oneline`):**
- `[FOUND]` `256ae65` — `fix(67-02): identifier-aware source-table tokenisation in TABLES clause`
- `[FOUND]` `5fb2ed4` — `test(67-02): add phase67_quoted_source_tables.test (TECH-DEBT #24 fixture)`
- `[FOUND]` `56065ff` — `docs(67-02): mark TECH-DEBT #24 RESOLVED; add #25 for sibling slot`

## Self-Check: PASSED
