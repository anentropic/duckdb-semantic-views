---
phase: 68-pre-tag-cleanup-phase-67-review-pr-35-code-review-follow-ups
fixed_at: 2026-05-27T15:55:00Z
review_path: .planning/phases/68-pre-tag-cleanup-phase-67-review-pr-35-code-review-follow-ups/68-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 4
skipped: 0
status: all_fixed
---

# Phase 68: Code Review Fix Report

**Fixed at:** 2026-05-27T15:55:00Z
**Source review:** `.planning/phases/68-pre-tag-cleanup-phase-67-review-pr-35-code-review-follow-ups/68-REVIEW.md`
**Iteration:** 1

**Summary:**
- Findings in scope (Critical + Warning): 4
- Fixed: 4
- Skipped: 0

Critical findings: 0 (none in review).
Info findings (5 total: IN-01..IN-05) deliberately out of scope per
`fix_scope: critical_warning`.

## Fixed Issues

### WR-01: `split_qualified_identifier` accepts empty `alias_part` on leading dot

**Files modified:** `src/body_parser.rs`
**Commit:** `52cdc9e`
**Applied fix:** Inside the depth-0-dot branch, bind both halves of the
split into local `alias` and `name` slices and short-circuit `return None`
when either side is empty. Today's NAB and window-ORDER-BY callers
tolerate the previous `Some(("", "foo"))` behaviour (every parsed
dimension carries a non-empty `source_table`, so the empty-vs-non-empty
case never matches), but the helper is a leaf utility — returning `None`
for `".foo"` / `"foo."` matches the docstring's stated intent and
protects future callers. `cargo check` and the targeted
`registration_error_surfaces` test pass.

### WR-02: `split_qualified_identifier` only splits at the first depth-0 dot

**Files modified:** `src/body_parser.rs`
**Commit:** `e1deb36`
**Applied fix:** Docstring-only change. The helper's name and original
docstring implied general N-segment support, but it only splits at the
FIRST depth-0 dot — `db.sch."tbl"` returns `Some(("db", "sch.\"tbl\""))`.
Tighten the docstring to spell out the first-dot contract, document the
new WR-01 empty-side rejection, and add a 3-segment example so future
callers do not assume recursive behaviour. No behavioural change.

### WR-03: C2 transmute-needle guard bypassed by alternative qualifying paths

**Files modified:** `tests/registration_error_surfaces.rs`
**Commit:** `9f61694`
**Applied fix:** Replace the single concatenated needle
`["std::", "mem::", "transmute"].concat()` with a `Vec<String>` of three
runtime-concatenated needles covering all three qualifying paths to the
FFI intrinsic:

  * `std::mem::transmute(...)` / `std::mem::transmute::<T,U>(...)`
  * `core::mem::transmute(...)` (Rust 2021+ re-exports `core::mem`)
  * unqualified `mem::transmute(...)` (after `use std::mem;`)

Filter scans non-comment lines for any needle match. Each needle is
still built via array concat so this file's source never contains an
adjacent `std::mem::transmute` token sequence in non-comment code,
preserving the plan-checker `grep -q "std" + "::mem::transmute"`
zero-hits invariant. `cargo test --test registration_error_surfaces`
passes (1/1).

### WR-04: ADBC ATTACH path escape only handles single quotes

**Files modified:** `test/integration/test_adbc_queries.py`
**Commit:** `64e7b0f`
**Applied fix:** Test-only doc change. Replace the misleading
"SQL-string escape parity with line 100" comment with an explicit
narrow-scope note: `tempfile.TemporaryDirectory()` paths on macOS/Linux
never contain single quotes, newlines, or NUL bytes, so the
`.replace("'", "''")` escape is sufficient for this fixture's input
domain. Comment warns against lifting the pattern into a production
code path that handles user-supplied paths. No behavioural change;
Python AST parse-check passes.

## Skipped Issues

None — all in-scope (Warning-severity) findings were fixed cleanly.

The following Info-severity findings were intentionally **not** addressed
this iteration because `fix_scope: critical_warning`:

- IN-01: Trailing-semicolon inconsistency in new phase68 fixtures
- IN-02: `is_quoting_balanced` byte-walk invariant comment
- IN-03: Duplicate ASC/DESC/NULLS parsing in NAB + OVER-ORDER-BY arms
- IN-04: `clippy::too_many_lines` allow may be unneeded after A3 simplification
- IN-05: `.gitignore` `p651_ok.yaml` comment block reads as scope creep

These are tracked in REVIEW.md and can be surfaced as TECH-DEBT or a
follow-up phase if desired.

---

_Fixed: 2026-05-27_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
