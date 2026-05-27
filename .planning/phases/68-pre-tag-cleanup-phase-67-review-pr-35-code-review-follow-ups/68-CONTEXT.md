# Phase 68: Pre-Tag Cleanup — Phase 67 Review + PR #35 Code Review Follow-ups - Context

**Gathered:** 2026-05-27
**Status:** Ready for planning
**Source:** `/gsd-discuss-phase 68 --assumptions` (three judgment calls confirmed by user)

<domain>
## Phase Boundary

Final cleanup phase before v0.10.0 tag-and-merge. Clears three sources of small follow-up items, all non-blocking, all hygiene:

1. Phase 67 REVIEW.md findings (A1–A7) — 3 Warnings + 4 Info from the code review of Phase 67 changes
2. TECH-DEBT #25 (B1, B2) — sibling `split_whitespace` sites surfaced by Phase 67 Plan 02's audit-grep
3. PR #35 Copilot review comments (C1, C2, C3) — three inline comments on the v0.10.0 milestone PR

**In scope:** All 13 items inventoried in `SCOPE.md`.

**Out of scope (explicit):**
- REL-01..04 (CHANGELOG, version bump, example file, DuckDB v1.5.3 bump) — milestone-close tasks, not phase tasks. Per `feedback_defer_release_tasks`, do not fold these in even though the milestone is finishing. `/gsd-complete-milestone` handles them after Phase 68 verifies.

</domain>

<decisions>
## Implementation Decisions

### Architectural premise

- **D-01:** [informational] SCOPE.md is already detailed; this phase has minimal architectural gray area. Most decisions are tactical (ordering, test depth, identifier shape). Plan-phase should treat SCOPE.md as the authoritative item inventory.

### A. Phase 67 REVIEW.md mechanical fixes

- **D-02:** A1 (WR-01 reserved-keyword guard) and A3 (WR-03 dead loop collapse) **bundle in a single commit**. Reason: A3 deletes the loop site where A1's guard belongs; landing A3 first without A1 widens an error-message regression window. User-confirmed.
- **D-03:** A1's guard rejects bare table names matching `PRIMARY|UNIQUE|FOREIGN|REFERENCES|NOT` (case-insensitive) and emits the pre-Phase-67 error message `"Missing physical table name after AS for alias '{alias}' in TABLES clause."`. The pre-fix error message is the contract — add a fixture asserting the exact message.
- **D-04:** A2 (WR-02 SQL-string escape) brings `test_adbc_queries.py:470` to parity with line 100's `.replace("'", "''")` treatment of `extension_path`. Defense-in-depth, not a behaviour change.
- **D-05:** A4 (IN-01 unterminated quoted identifier) and A7 (IN-04 `find_primary_key` word-boundary alignment with `find_unique`) are surgical body-parser edits, no contract change beyond rejecting input that previously silently malformed.
- **D-06:** A5 (IN-02 mixed bare/quoted dot-qualified fixture row) and A6 (IN-03 default-schema cleanup) extend `test/sql/phase67_quoted_source_tables.test`.

### B. TECH-DEBT #25 structural rewrite

- **D-07:** B1 (`parse_non_additive_dims`) and B2 (`parse_window_spec` OVER ORDER BY) follow the **`parse_single_table_entry` tokeniser shape** — walk dot-separated segments via `find_identifier_end`, terminated by `,`, `)`, or end-of-clause. This re-classifies the sites from (c)-class (structural-rewrite-required) to (b)-class (port the same loop shape), because the user confirmed dotted paths are needed (see D-08), which aligns the contract with #24's TABLES clause fix.
- **D-08:** **NON ADDITIVE BY and OVER ORDER BY accept dotted paths** (`table.col`). Reason: SV defs can declare multiple tables; dimension references may need `table.col` qualification to disambiguate. Same logic applies to window function ORDER BY column refs. User-confirmed. Plan-phase should survey existing fixtures to confirm the dot-walk contract is consistent with what's emitted today.
- **D-09:** B1/B2 **require sqllogictest coverage** — at least one fixture per site with a quoted identifier containing literal whitespace, asserting the rewrite preserves the parsed shape end-to-end. Unit tests alone are insufficient per project quality-gate stance. User-confirmed.
- **D-10:** SCOPE.md flagged B1/B2 as (c)-class; D-07 re-classifies as (b)-class. If plan-phase investigation reveals the rewrite is significantly larger than the (b)-class port suggests, surface as a SUMMARY finding and the phase can renegotiate (per SCOPE.md's renegotiation clause).

### C. PR #35 Copilot review fixes

- **D-11:** C1 (`tests/registration_error_surfaces.rs:136` char-boundary panic) — swap `&body[..body.len().min(400)]` to `body.get(..400).unwrap_or(body)`. Avoids UTF-8 boundary panic in the error-formatting path.
- **D-12:** C2 (`tests/registration_error_surfaces.rs:171` transmute needle) — drop the trailing `(` from `["std::", "mem::", "transmute("]` so the needle catches both bare `std::mem::transmute(...)` and turbofish form `std::mem::transmute::<T, U>(...)`. Add a one-line comment explaining why the looser match is correct (turbofish form is the real target).
- **D-13:** C3 (`test/sql/p651_ok.yaml` unused fixture) — **delete the fixture**. Rewriting the test to load it would mean the test no longer exercises the runtime `COPY (...) TO` path, which is the actual filesystem-gating contract under test.

</decisions>

<sequencing>
## Implementation Order

- **Plan 68-01 (Wave 1):** A3 → A1 (bundled, single commit) → A2, A4, A5, A6, A7. Body-parser surgical edits + sqllogictest fixture polish.
- **Plan 68-02 (Wave 1, parallel with 68-01):** C1, C2, C3. Tests-only file edits + one fixture deletion. No overlap with body_parser, so genuinely parallel-safe.
- **Plan 68-03 (Wave 2):** B1 → B2. TECH-DEBT #25 structural rewrite. Depends on 68-01 because the WR-01 keyword-guard pattern from A1 should be the established reference for "reject reserved keywords as identifiers" in this parser before B1/B2 land.

</sequencing>

<risks>
## Risk Areas

- **B1/B2 ambiguity around `parse_non_additive_dims` call site.** If the function operates on already-tokenised input (rather than a raw slice), porting `find_identifier_end` requires lifting tokenisation up the call stack. Plan-phase should resolve before committing to a tokeniser shape. Surface as a SUMMARY finding if the rewrite balloons past ~150 LOC + tests per site.
- **A1 + A3 bundling discipline.** The order within the single commit matters: collapse A3's dead loop first, then add A1's guard at the clean site. A code review pass at commit time catches any drift.
- **C3 deletion vs. test-load divergence.** Deleting `test/sql/p651_ok.yaml` is the right call per D-13, but ensure no other test references it before deletion. Quick `grep -r "p651_ok.yaml"` is a sanity check.

</risks>

<dependencies>
## Dependencies

**From prior phases:**
- Phase 67 Plan 02's `find_identifier_end` helper + tests — B1/B2 reuse this.
- Phase 67 Plan 02's `test/sql/phase67_quoted_source_tables.test` — A5/A6 extend it.
- Phase 67's REVIEW.md and TECH-DEBT.md item #25 — primary inputs.

**External:** None. No new crates. No DuckDB version change in this phase (v1.5.3 bump deferred to milestone close per D-01).

**Feeds into:** `/gsd-complete-milestone` for v0.10.0 — Phase 68 must be Verified before tag-and-merge. REL-01..04 happen there.

</dependencies>

<quality_gates>
## Quality Gates

- `just test-all` (Rust unit + proptest + sqllogictest + DuckLake CI + ADBC) post-merge.
- `just ci` (adds clippy pedantic + fmt + cargo-deny + fuzz target compile) before push to main.
- New sqllogictest fixtures (D-09) registered in `test/sql/TEST_LIST` per CLAUDE.md rule.
- All commits pass pre-commit hook (`cargo fmt --check` + clippy). No `--no-verify`.

</quality_gates>
