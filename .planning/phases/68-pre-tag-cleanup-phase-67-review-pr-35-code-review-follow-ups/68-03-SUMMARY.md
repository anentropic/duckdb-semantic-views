---
phase: 68
plan: 03
subsystem: body_parser
tags: [hygiene, pre-tag-cleanup, parser, tech-debt-25, identifier-tokenisation, sqllogictest]
dependency_graph:
  requires:
    - "Phase 67 Plan 02 — find_identifier_end helper + parse_single_table_entry port (the TECH-DEBT #24 shape this plan mirrors)"
    - "Phase 68 Plan 01 — is_quoting_balanced helper + A1 reserved-keyword guard pattern"
  provides:
    - "Identifier-aware tokenisation of NAB dim_name slot (parse_non_additive_dims) — quoted identifiers with internal whitespace survive intact"
    - "Identifier-aware tokenisation of OVER ORDER BY column-reference slot (parse_over_content) — same contract"
    - "D-08 dotted-path acceptance for NAB and OVER ORDER BY at the parser AND the dimension-resolver layer"
    - "Structured ParseError for unterminated quoted identifiers in both clauses (mirrors A4 TABLES-clause contract)"
    - "New split_qualified_identifier private helper — quote-aware first-dot split"
  affects:
    - "src/body_parser.rs (parse_non_additive_dims, parse_over_content ORDER BY arm, NAB resolver, window ORDER BY resolver, new split_qualified_identifier helper)"
    - "test/sql/phase68_quoted_idents_non_additive.test (new fixture)"
    - "test/sql/phase68_quoted_idents_window.test (new fixture)"
    - "test/sql/TEST_LIST (two new entries)"
tech_stack:
  added: []
  patterns:
    - "find_identifier_end + is_quoting_balanced at three sibling parser sites (parse_single_table_entry, parse_non_additive_dims, parse_over_content ORDER BY arm)"
    - "split_qualified_identifier private helper for D-08 dotted-path resolution at NAB and window ORDER BY dim resolvers"
key_files:
  created:
    - "test/sql/phase68_quoted_idents_non_additive.test"
    - "test/sql/phase68_quoted_idents_window.test"
  modified:
    - "src/body_parser.rs"
    - "test/sql/TEST_LIST"
decisions:
  - "Apply Rule 2 (auto-add missing critical functionality): D-08 dotted-path acceptance requires resolver-layer support, not just parser-layer (the parser captures the dotted form, but the NAB→dim and window ORDER BY→dim resolvers compared only against the bare dim name). ~10 LOC delta — well within the (b)-class budget."
  - "Apply D-10 renegotiation for Scenario 2 (dotted-path e2e): expand-time semi-additive and window emission of dotted-path column refs produces the wrong quoting shape (renders `\"o.\"\"order date\"\"\"` instead of `\"o\".\"order date\"`). Narrowed Scenario 2 in both fixtures to DDL+round-trip validation only; expand-side dotted-path emission is out of scope for Plan 03. Documented in fixture comments."
  - "Resolver fix for D-08 reuses a small split_qualified_identifier helper (depth-0 dot detection respecting quoted regions). Applied at TWO resolver sites (NAB + window ORDER BY) so both clauses agree on the dotted-path contract."
metrics:
  duration: "~23 minutes"
  tasks_completed: 3
  files_modified: 4
  completed_date: 2026-05-27
---

# Phase 68 Plan 03: TECH-DEBT #25 Sibling split_whitespace Sites Summary

Land the two sibling `split_whitespace` sites in `src/body_parser.rs` that Phase 67 Plan 02's audit-grep flagged (TECH-DEBT #25). The port pattern mirrors the post-TECH-DEBT-#24 shape of `parse_single_table_entry`: capture the identifier via `find_identifier_end`, surface unterminated-quote errors via `is_quoting_balanced`, then tokenise only the modifier suffix. D-08 dotted-path acceptance lands at both the parser AND the dimension-resolver layers.

## What Shipped

**Commit 1 — `8f44794` (B1 — parse_non_additive_dims):**

- Replaced `entry_text.split_whitespace().collect()` with identifier-aware capture: `find_identifier_end(entry_text, false)` + `is_quoting_balanced` guard + modifier-suffix split.
- D-08 NAB resolver: accepts `alias.dim_name` form in addition to bare `dim_name`. Introduced `split_qualified_identifier` private helper (depth-0 dot detection respecting quoted regions).
- 4 new Rust unit tests (`test_parse_non_additive_dims_quoted_identifier_with_whitespace`, `_dotted_path`, `_unterminated_quote`, `_regression_bare_no_whitespace`) — all pass.
- New sqllogictest fixture `test/sql/phase68_quoted_idents_non_additive.test` registered in `test/sql/TEST_LIST`. Exercises 3 scenarios: quoted-whitespace e2e, dotted-path DDL+round-trip (narrowed per D-10), unterminated-quote error path.

**Commit 2 — `1dce720` (B2 — parse_over_content ORDER BY arm):**

- Same shape port at `parse_over_content` (the ORDER BY arm inside `parse_window_over_clause`).
- D-08 window ORDER BY resolver: reuses `split_qualified_identifier` for the same alias.name acceptance pattern.
- 4 new Rust unit tests (`test_parse_window_spec_quoted_order_by`, `_dotted_order_by`, `_unterminated_quote_order_by`, `_regression_bare_order_by`) — all pass.
- New sqllogictest fixture `test/sql/phase68_quoted_idents_window.test` registered. Exercises 3 scenarios mirroring B1.

**Task 3 — Full quality gate:**

- `just test-all`: exit 0. Both new Phase 68 fixtures (`phase68_quoted_idents_non_additive` + `phase68_quoted_idents_window`) appear in the test-all output — TEST_LIST registration verified by observation, not just file presence.
- `just ci`: exit 0. Clippy pedantic + fmt + cargo-deny + fuzz target compile + Sphinx docs all green.

## Verification

- `cargo test --lib body_parser` — **154 passed, 0 failed** (up from 150 pre-plan; 4 new B1 tests + 4 new B2 tests).
- `just test-sql` — Both new fixtures pass under the runner. Phase 47 (semi_additive) and Phase 48 (window_metrics) regression fixtures continue to pass.
- `just test-all` — All Rust unit + proptest + sqllogictest (60 fixtures total now) + DuckLake CI + ADBC + multi-DB + readonly + concurrent suites green.
- `just ci` — Above plus clippy pedantic + fmt + cargo-deny + fuzz target compile + Sphinx docs.
- No `--no-verify` used. Pre-commit hook (`cargo fmt --check` + clippy pedantic) passed on both source commits (one fmt + one clippy doc-markdown rejection on the first attempt; both fixed and re-staged per CLAUDE.md procedure, not bypassed).

## Deviations from Plan

### Auto-added (Rule 2 — missing critical functionality)

**1. [Rule 2 — Resolver-layer D-08 support] Extended NAB and window-ORDER-BY dim resolvers to accept dotted-path qualifiers**

- **Found during:** Task 1 (B1) sqllogictest fixture run
- **Issue:** The plan's RESEARCH §B1 + §B2 stated that D-08 dotted-path support was "free with the parser port" — but the downstream resolvers (`parse_keyword_body`'s NAB validator at body_parser.rs:446-470 and the window ORDER BY validator at body_parser.rs:567-589) only compared against bare `d.name`. Without resolver support, `o."order date"` in NAB/ORDER BY would parse correctly but fail at the validation step with "does not match any declared dimension".
- **Fix:** Added a small `split_qualified_identifier` private helper that splits a qualified identifier at the first depth-0 dot OUTSIDE a quoted region (so `"a.b"` stays atomic but `o."order date"` splits into `("o", "\"order date\"")`). Extended both resolvers to fall back to `(source_table, name)` comparison if the bare-name comparison fails.
- **Files modified:** `src/body_parser.rs` (helper + 2 resolver sites)
- **Commits:** `8f44794` (NAB resolver + helper), `1dce720` (window ORDER BY resolver)
- **Rationale:** D-08 is a CONTEXT.md locked decision. Without resolver support, D-08 is not delivered end-to-end. The fix is small (~10 LOC total) and stays within the (b)-class port budget per D-10. The helper is reusable across both clauses, so the same pattern lands at both sites consistently.

### Renegotiated (per D-10 — scope narrowing)

**2. [D-10 — narrow Scenario 2 e2e] Dotted-path scenarios in BOTH new fixtures landed as DDL+round-trip only, not full e2e query**

- **Found during:** Task 1 (B1) Scenario 2 query expansion + Task 2 (B2) Scenario 2 query expansion
- **Issue:** After resolver-layer D-08 support, parse + name-resolution succeed for `o."order date"` in both NAB and OVER ORDER BY. But the expand-time SQL generation for semi-additive (Plan 47) and window (Plan 48) metrics emits the raw stored dim text directly into the generated `ROW_NUMBER() OVER (ORDER BY ...)` and `OVER (... ORDER BY ...)` clauses, producing `"o.""order date"""` (single quoted identifier shape) instead of the correct `"o"."order date"` (two-part qualified shape). The query then fails at DuckDB binder time with `Referenced column "o."order date"" not found`.
- **Fix:** Per the plan's B1 Step C "narrowing" guidance (sqllogictest fixture authors authorise narrowing the dotted-path scenario when downstream expand emission doesn't handle dotted refs), narrowed Scenario 2 in both fixtures to assert DDL round-trip + DESCRIBE-style observation that the dotted text was captured verbatim. The actual `semantic_view(...)` query for dotted-path NAB/ORDER BY is left for a future phase (it's an expand-side fix to `src/expand/semi_additive.rs` and `src/expand/window.rs` — orthogonal to the parser port that Plan 03 is scoped to).
- **Files:** `test/sql/phase68_quoted_idents_non_additive.test`, `test/sql/phase68_quoted_idents_window.test`
- **Rationale:** D-10 explicitly authorises renegotiation if the rewrite balloons. The narrowed scope still closes the SCOPE.md B1/B2 items at the parser layer (the actual TECH-DEBT #25 surface) and at the dim-resolver layer (D-08 contract). End-to-end semi-additive/window emission for dotted refs is a separate concern, fairly classified as the originally-postponed v0.10.1 territory.

### Risk renegotiation per D-10 — body delta size

Total LOC delta on `src/body_parser.rs`:
- B1 parse_non_additive_dims port: ~28 LOC (the function body itself + new doctext)
- NAB resolver D-08 extension: ~14 LOC
- B2 parse_over_content ORDER BY arm: ~21 LOC
- Window ORDER BY resolver D-08 extension: ~13 LOC
- New `split_qualified_identifier` helper: ~32 LOC (function + doctest)
- 8 new Rust unit tests: ~95 LOC

Per-site body delta is ~30 LOC + resolver, well under the D-10 ~50-LOC threshold for the parser port itself. The shared helper and resolver tweaks are clearly within the (b)-class port budget.

## Acceptance Criteria Status

All SCOPE items B1 + B2 closed:

- [x] B1 — `parse_non_additive_dims` calls `find_identifier_end` (verifiable via grep).
- [x] B1 — 4 new unit tests in `body_parser::tests` pass.
- [x] B1 — `test/sql/phase68_quoted_idents_non_additive.test` exists, contains `NON ADDITIVE BY` + quoted-whitespace `"order date"` + dotted path `o."order date"`.
- [x] B1 — `test/sql/TEST_LIST` registers it exactly once.
- [x] B2 — `parse_over_content` ORDER BY arm calls `find_identifier_end`.
- [x] B2 — 4 new unit tests in `body_parser::tests` pass.
- [x] B2 — `test/sql/phase68_quoted_idents_window.test` exists, contains `OVER` + quoted-whitespace + dotted path.
- [x] B2 — `test/sql/TEST_LIST` registers it exactly once.
- [x] Both fixtures pass under `just test-sql`.
- [x] Pre-existing phase47_semi_additive + phase48_window_metrics still pass (no regression on bare-identifier surface).
- [x] D-08 contract extension lands at parser AND resolver for both NAB and OVER ORDER BY.
- [x] Pre-commit hook passes without `--no-verify` on both source commits.
- [x] `just test-all` exit 0.
- [x] `just ci` exit 0.

## Threat Mitigations Applied

- **T-68-07 (Tampering — `parse_non_additive_dims` and `parse_window_spec` ORDER BY arm shredding quoted identifiers via `split_whitespace`):** mitigated by the B1 + B2 ports to `find_identifier_end` + `is_quoting_balanced`. Malformed identifiers (unterminated quotes, shredded quoted-whitespace names) now surface structured `ParseError` instead of corrupting downstream SQL generation. D-08 dotted-path acceptance is purely additive (no existing fixture used the shredding behaviour, verified by RESEARCH §B1/§B2).

## Self-Check: PASSED

- `src/body_parser.rs::parse_non_additive_dims` contains `find_identifier_end` (verified `grep -A 30 'fn parse_non_additive_dims' src/body_parser.rs | grep -c 'find_identifier_end'` → 1).
- `src/body_parser.rs::parse_over_content` ORDER BY arm contains `find_identifier_end` (verified around line ~1860 post-edit).
- `src/body_parser.rs` defines `fn split_qualified_identifier`.
- `src/body_parser.rs::parse_keyword_body` NAB validator AND window ORDER BY validator both call `split_qualified_identifier` (D-08 resolver extension).
- `test/sql/phase68_quoted_idents_non_additive.test` exists with `NON ADDITIVE BY`, `"order date"`, `o."order date"`, `Unterminated quoted identifier`.
- `test/sql/phase68_quoted_idents_window.test` exists with `OVER`, `"order date"`, `o."order date"`, `Unterminated quoted identifier`.
- `test/sql/TEST_LIST`: `grep -c '^test/sql/phase68_quoted_idents_non_additive\.test$' test/sql/TEST_LIST` → 1; `grep -c '^test/sql/phase68_quoted_idents_window\.test$' test/sql/TEST_LIST` → 1.
- Commits `8f44794` (B1) and `1dce720` (B2) both exist on `milestone/v0.10.0` (`git log --oneline -3` confirmed).
- `just test-all` log at `/tmp/claude/test_all.log`: both `phase68_quoted_idents_non_additive` and `phase68_quoted_idents_window` appear exactly once in output (TEST_LIST registration verified by observation).
- `just ci` log at `/tmp/claude/ci.log`: clippy pedantic + fmt + cargo-deny + fuzz check + Sphinx docs all complete successfully.
