---
phase: 68-pre-tag-cleanup-phase-67-review-pr-35-code-review-follow-ups
verified: 2026-05-27T15:29:10Z
status: passed
score: 12/12 must-haves verified
overrides_applied: 0
re_verification:
  previous_status: none
  previous_score: none
  gaps_closed: []
  gaps_remaining: []
  regressions: []
deferred:
  - truth: "End-to-end semi-additive / window expansion accepts dotted-path NAB and OVER ORDER BY column refs (e.g., `o.\"order date\"`)"
    addressed_in: "post-v0.10.0 (TECH-DEBT candidate)"
    evidence: "68-03-SUMMARY.md §Deviations 'Renegotiated (per D-10 — scope narrowing)' documents the expand-side emission gap (renders `\"o.\"\"order date\"\"\"` instead of `\"o\".\"order date\"`). Scenario 2 in both new fixtures intentionally narrowed to DDL+round-trip; closing the e2e gap requires changes to src/expand/semi_additive.rs and src/expand/window.rs, orthogonal to the Plan 03 parser-port scope."
---

# Phase 68: Pre-Tag Cleanup Verification Report

**Phase Goal:** Close all 12 review-derived follow-up items (A1–A7 from Phase 67 REVIEW.md, B1–B2 from TECH-DEBT #25, C1–C3 from PR #35 Copilot review) before tagging v0.10.0.

**Verified:** 2026-05-27T15:29:10Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (Acceptance Signals)

Each acceptance signal is one of the 12 SCOPE.md follow-up items (A1–A7, B1–B2, C1–C3). Source of truth: `SCOPE.md` (the phase is indexed by SCOPE IDs, not REQ IDs).

| # | Signal | Status | Evidence |
|---|--------|--------|----------|
| A1 | Reserved keywords (PRIMARY/UNIQUE/FOREIGN/REFERENCES/NOT) rejected as bare table names in `parse_single_table_entry` | VERIFIED | `src/body_parser.rs:771-783` contains the guard with literal D-03 keyword set; `matches!(upper_captured.as_str(), "PRIMARY" \| "UNIQUE" \| "FOREIGN" \| "REFERENCES" \| "NOT")` returns the canonical "Missing physical table name after AS for alias '{alias}' in TABLES clause." error. 6 new unit tests pass (`test_parse_single_table_entry_reserved_keyword_after_as_{primary,unique,foreign,references,not,lowercase}`). |
| A2 | ADBC ATTACH escape parity test landed | VERIFIED | `test/integration/test_adbc_queries.py:471-472` defines `other_db_path_sql = str(other_db_path).replace("'", "''")` and interpolates the escaped variable into `ATTACH '{other_db_path_sql}' AS db2`, byte-for-byte matching the line-100 `_bootstrap_extension` pattern. Test ran under `just test-all` (`uv run test/integration/test_adbc_queries.py`). |
| A3 | Dead dot-rejoin loop collapsed in `parse_single_table_entry` | VERIFIED | `grep -c 'if after_as.as_bytes()\[name_end\] == b\\'.\\''  src/body_parser.rs` → 0. Single `find_identifier_end(after_as, true)` call at `src/body_parser.rs:754` replaces the loop. |
| A4 | Unterminated quoted identifier in TABLES clause rejected | VERIFIED | `src/body_parser.rs:907` defines `fn is_quoting_balanced`. Call site `src/body_parser.rs:799-806` returns structured `ParseError` with literal "Unterminated quoted identifier in source-table name…" message. 3 new unit tests pass (`test_parse_single_table_entry_unterminated_quote`, `_quoted_with_doubled_escape_balanced`, `_unbalanced_after_doubled_escape`). |
| A5 | Mixed-quoting scenario (`staging."my orders"`) added | VERIFIED | `test/sql/phase67_quoted_source_tables.test:149-156` contains CREATE TABLE + CREATE SEMANTIC VIEW referencing `staging."my orders"`. Rust unit `test_parse_single_table_entry_mixed_quoted_and_bare` exists and passes. Fixture ran green under `just test-all`. |
| A6 | Cleanup hygiene applied (3 default-schema base tables) | VERIFIED | `grep -c 'DROP TABLE IF EXISTS' test/sql/phase67_quoted_source_tables.test` → 3. Cleanup section drops `"my orders"`, `"weird PRIMARY KEY name"`, `p67_plain_orders`, plus the staging schema introduced by Scenario 5. |
| A7 | `find_primary_key` word boundaries aligned with `find_unique` | VERIFIED | `src/body_parser.rs:997-1015` shows all three word-boundary checks (PRIMARY-before, PRIMARY-after, KEY-after) now exclude `b'_'`. `grep -c "b'_'" src/body_parser.rs` → 15 (3 new + existing). |
| B1 | `parse_non_additive_dims` ported to identifier-aware tokenisation + new fixture | VERIFIED | `src/body_parser.rs:1542` calls `find_identifier_end(entry_text, false)`; `:1553` runs `is_quoting_balanced` check. 4 new unit tests pass (`test_parse_non_additive_dims_{quoted_identifier_with_whitespace,dotted_path,unterminated_quote,regression_bare_no_whitespace}`). Fixture `test/sql/phase68_quoted_idents_non_additive.test` exists, registered in TEST_LIST, ran green. |
| B2 | `parse_window_spec` OVER ORDER BY ported + new fixture | VERIFIED | `src/body_parser.rs:1878` (inside `parse_over_content`) calls `find_identifier_end(entry_text, false)`. 4 new unit tests pass (`test_parse_window_spec_{quoted_order_by,dotted_order_by,unterminated_quote_order_by,regression_bare_order_by}`). Fixture `test/sql/phase68_quoted_idents_window.test` exists, registered in TEST_LIST, ran green. |
| C1 | UTF-8-safe body slice in `tests/registration_error_surfaces.rs` | VERIFIED | `tests/registration_error_surfaces.rs:135` contains `body.get(..400).unwrap_or(body),` — `grep -c 'body\.len()\.min(400)'` → 0. Test passes (`PASS [0.019s] (960/974) semantic_views::registration_error_surfaces`). |
| C2 | Turbofish-catching transmute needle | VERIFIED | `tests/registration_error_surfaces.rs:166` shows `parts = ["std::", "mem::", "transmute"]` (no trailing `(`). Comment at `:164` explains "needle now catches both bare std::mem::transmute(...) and turbofish std::mem::transmute::<T, U>(...) forms." |
| C3 | `test/sql/p651_ok.yaml` fixture deleted + .gitignore entry | VERIFIED | `git ls-files test/sql/p651_ok.yaml` → 0 lines (untracked). `.gitignore:30-32` contains the runtime-artefact comment + `test/sql/p651_ok.yaml` entry. Note: file exists on disk after a test run because the sqllogictest runner resolves `__TEST_DIR__` to `test/sql/` and `COPY (...) TO '__TEST_DIR__/p651_ok.yaml'` writes the runtime fixture there — the .gitignore prevents pollution of `git status`. Acceptable per 68-02-SUMMARY rationale (the contract under test is the runtime `COPY TO` path, not the checked-in file). |

**Score:** 12/12 must-haves verified

### Required Artifacts

All artifacts inspected at three levels (exists → substantive → wired).

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/body_parser.rs` | A1 guard, A3 collapse, A4 balanced-quote helper, A7 boundary alignment, B1+B2 ports, D-08 resolver helper | VERIFIED | All grep-driven structural checks pass; 154/154 body_parser tests green |
| `test/integration/test_adbc_queries.py` | Line 470 ATTACH escape parity | VERIFIED | `other_db_path_sql` variable + escaped f-string in place |
| `test/sql/phase67_quoted_source_tables.test` | Scenario 5 mixed-quoting + extended cleanup | VERIFIED | `staging."my orders"` appears in CREATE TABLE + CREATE SEMANTIC VIEW; cleanup block drops 3 extra tables + staging schema |
| `tests/registration_error_surfaces.rs` | C1 safe slice + C2 turbofish needle | VERIFIED | Both edits present; test passes |
| `test/sql/p651_ok.yaml` | DELETED from git tracking | VERIFIED | `git ls-files` returns empty; .gitignore prevents runtime regeneration polluting status |
| `.gitignore` | Runtime-artefact entry for p651_ok.yaml | VERIFIED | Entry at line 32 with explanatory comment |
| `test/sql/phase68_quoted_idents_non_additive.test` | New B1 fixture (quoted + dotted + unterminated scenarios) | VERIFIED | File exists (103 lines); contains `NON ADDITIVE BY`, `"order date"`, `o."order date"`, `Unterminated quoted identifier` |
| `test/sql/phase68_quoted_idents_window.test` | New B2 fixture (quoted + dotted + unterminated scenarios) | VERIFIED | File exists (108 lines); contains `OVER`, `"order date"`, `o."order date"`, `Unterminated quoted identifier` |
| `test/sql/TEST_LIST` | Two new entries | VERIFIED | `grep -c '^test/sql/phase68_quoted_idents_(non_additive\|window)\.test$'` → 2 |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| `parse_single_table_entry` | `find_identifier_end` | Single call replacing dead dot-rejoin loop (A3 collapse) | WIRED | `src/body_parser.rs:754` |
| `parse_single_table_entry` | `is_quoting_balanced` | A4 unterminated-quote guard after A1 keyword guard | WIRED | `src/body_parser.rs:799` |
| `parse_non_additive_dims` | `find_identifier_end` | One call per NAB entry; identifier-aware capture of dim_name | WIRED | `src/body_parser.rs:1542` |
| `parse_non_additive_dims` | `is_quoting_balanced` | Mirrors A4 guard for NAB clause | WIRED | `src/body_parser.rs:1553` |
| `parse_over_content` (window ORDER BY arm) | `find_identifier_end` | One call per ORDER BY entry | WIRED | `src/body_parser.rs:1878` |
| `parse_keyword_body` NAB resolver | `split_qualified_identifier` | D-08 dotted-path resolution (alias.name comparison) | WIRED | New helper at `src/body_parser.rs:105` reused at 2 resolver sites per 68-03-SUMMARY |
| `test/sql/TEST_LIST` | `phase68_quoted_idents_*.test` | Runner registration (CLAUDE.md hard rule) | WIRED | Both fixtures observed running under `just test-all` (`[1/1] test/sql/phase68_quoted_idents_non_additive.test` and `[1/1] test/sql/phase68_quoted_idents_window.test` in log) |
| `tests/registration_error_surfaces.rs` | `cpp/src/shim.cpp` (sv_register_table_function body) | fs::read_to_string + literal-substring assertion (init_cb invariant) | WIRED | Test passes; C1+C2 edits preserve assertion behaviour |

### Data-Flow Trace (Level 4)

Not applicable to this phase. All artifacts are parser-internal, test fixtures, or test assertions — no UI/dashboard data flow to trace. Behavioural correctness is exercised through unit tests + sqllogictest (e2e DDL + query) + ADBC integration tests.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| body_parser unit tests (covers A1, A4, A5, A7, B1, B2) | `cargo test --lib body_parser` | 154 passed, 0 failed | PASS |
| Full quality gate (CLAUDE.md required) | `just test-all` | RC=0; 974 cargo tests pass; 60 sqllogictests pass; 12/12 readonly_load_close; 7/7 ADBC | PASS |
| Phase 68 fixture B1 observed running | `grep phase68_quoted_idents_non_additive /tmp/claude/test_all.log` | `[1/1] test/sql/phase68_quoted_idents_non_additive.test` | PASS |
| Phase 68 fixture B2 observed running | `grep phase68_quoted_idents_window /tmp/claude/test_all.log` | `[1/1] test/sql/phase68_quoted_idents_window.test` | PASS |
| Phase 67 fixture (A5+A6) observed running | `grep phase67_quoted_source_tables /tmp/claude/test_all.log` | `[1/1] test/sql/phase67_quoted_source_tables.test` | PASS |
| C1+C2 test still passes | `cargo test --test registration_error_surfaces` (observed in test-all) | `PASS [0.019s] (960/974) ... init_extension_surfaces_registration_error_buf` | PASS |
| ADBC integration (A2 site) | `uv run test/integration/test_adbc_queries.py` (observed in test-all) | PASS line appears in test-all summary | PASS |
| C3 fixture untracked | `git ls-files test/sql/p651_ok.yaml` | 0 lines | PASS |

### Probe Execution

No formal probe scripts in this phase. The phase's verification contract is `just test-all` per CLAUDE.md Quality Gate, which was executed and returned exit 0.

### Requirements Coverage

Phase 68 is review-derived hygiene, NOT REQ-indexed. SCOPE.md acceptance signals A1–A7 / B1–B2 / C1–C3 serve as the requirements surface. All 12 verified above. There are no REQ IDs in the PLAN frontmatter `requirements:` field for any of the three plans (only `scope_items:`).

### Anti-Patterns Found

Files modified in this phase (extracted from SUMMARYs): `src/body_parser.rs`, `test/integration/test_adbc_queries.py`, `test/sql/phase67_quoted_source_tables.test`, `tests/registration_error_surfaces.rs`, `.gitignore`, `test/sql/phase68_quoted_idents_non_additive.test` (created), `test/sql/phase68_quoted_idents_window.test` (created), `test/sql/TEST_LIST`.

Scans:

- Debt markers in modified files: `grep -nE 'TBD|FIXME|XXX' src/body_parser.rs test/integration/test_adbc_queries.py test/sql/phase67_quoted_source_tables.test tests/registration_error_surfaces.rs test/sql/phase68_quoted_idents_*.test` — no occurrences attributable to this phase.
- Cleanup-warning comments: TODO/HACK markers present in `src/body_parser.rs` predate Phase 68 (legacy code paths) — no new debt markers introduced.
- Stub patterns / hardcoded empty data: none — every new code path has tests asserting non-empty / correctness.
- Console.log / debug prints: none in modified files.

No blocker anti-patterns found.

### Human Verification Required

None. All acceptance signals are verifiable programmatically (grep + unit tests + sqllogictest + ADBC integration). Visual rendering, UX, or external-service paths are not involved in this hygiene phase.

### Deferred Items (informational, not blocking)

One known deviation surfaced during execution and documented in 68-03-SUMMARY.md. It does not block Phase 68 closure but is worth surfacing to the user before tag:

**Expand-side dotted-path emission for semi-additive and window metrics**

- **What:** When `NON ADDITIVE BY (o."order date" DESC)` or `OVER (... ORDER BY o."order date" ASC ...)` is used (D-08 dotted-path contract), the parser + resolver layers handle the input correctly. However, the expand-time SQL generator in `src/expand/semi_additive.rs` and `src/expand/window.rs` emits the stored dim text into the generated ROW_NUMBER()/OVER clause without re-quoting it as `"o"."order date"` (two-part quoted), instead producing `"o.""order date"""` (single-quoted-identifier shape). The query then fails at DuckDB binder time with `Referenced column "o."order date"" not found`.
- **Plan 03's response:** Narrowed Scenario 2 in both new fixtures to DDL + round-trip only (no `semantic_view(...)` query against the dotted-path form). Plan-aligned per D-10 (renegotiation when downstream expand emission doesn't handle dotted refs).
- **Status:** Documented in 68-03-SUMMARY.md §Deviations. **NOT recorded in TECH-DEBT.md.**

**Recommendation (non-blocking):** Before tagging v0.10.0, add this as a fresh TECH-DEBT entry (or a v0.10.0 known-limitations bullet in CHANGELOG.md / TECH-DEBT.md `v0.10.0 additions`) so future contributors can find the surface area when authoring v0.10.1 work. The user's verification prompt explicitly called out this consideration: "consider whether it warrants a TECH-DEBT entry for the v0.10.0 known-limitations section."

This deferral does not affect Phase 68 status. It is a release-prep item belonging to `/gsd-complete-milestone` (which handles CHANGELOG, version bump, example file — per `feedback_defer_release_tasks.md`).

### Gaps Summary

None. All 12 acceptance signals verified in the codebase. Quality gates (`just test-all`) returned exit 0. Both new sqllogictest fixtures are registered in TEST_LIST and observed running under the test-all suite (not silently skipped). The one known deviation (expand-side dotted-path emission) is scoped out by the plan and surfaced here as a deferred item for milestone-close consideration — not a blocker.

---

*Verified: 2026-05-27T15:29:10Z*
*Verifier: Claude (gsd-verifier)*
