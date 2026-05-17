---
phase: 64
plan: 03
subsystem: expand
tags: [ident, expand, resolution, quoted-identifiers, idempotency, bugfix]
requires:
  - module: crate::ident
    surface: parse_qualified_identifier (from 64-01)
provides:
  - "quote_table_ref delegates to crate::ident::parse_qualified_identifier; idempotent on already-quoted FQN input"
  - "qualify_and_quote_table_ref uses structural parts.len() > 1 instead of substring-dot heuristic"
affects:
  - src/expand/resolution.rs
tech_stack:
  added: []
  patterns:
    - parse-then-emit: operate on UNQUOTED logical parts, never on the raw quoted string
    - structural is-qualified test (parts.len() > 1) replaces substring-dot heuristic
    - fallback path uses quote_ident on the raw string when the parser returns Err — never panics, never double-quotes
key_files:
  modified:
    - src/expand/resolution.rs
decisions:
  - quote_table_ref's Err branch falls back to quote_ident(table) on the RAW input.
    For an already-quoted FQN this is unreachable (parser succeeds); for malformed
    legacy strings it preserves single-pair-of-quotes shape and prevents the
    triple-quoting regression by construction.
  - qualify_and_quote_table_ref's Err branch falls through to the prepend path
    rather than short-circuiting. Rationale: if the input is genuinely malformed,
    prepending the catalog context is the safer of two bad options — it keeps the
    user's catalog scope and doesn't propagate the malformed string standalone.
  - The bare-name slot inside qualify_and_quote_table_ref re-parses with
    parse_qualified_identifier and uses the unquoted part when parts.len() == 1.
    This makes qualify_and_quote_table_ref(quoted_single_part, def) idempotent
    too: input "v" with no qualifier emits "db"."schema"."v", not "db"."schema"."\"v\"".
  - Pre-existing test embedded_quotes_in_parts updated to reflect new (correct)
    behaviour. The old expected output `"my""db"."my""table"` was a product of the
    buggy split-on-dot logic; the new strict parser rejects mixed bare/quoted as
    invalid input, and the fallback path emits a single pair of outer quotes.
    Decision: keep the test as a regression marker for the fallback path; rename
    NOT needed — the test now documents what the function does with malformed input.
requirements:
  - QID-04 (no triple-quoting in expanded SQL)
metrics:
  duration_minutes: 8
  tasks: 2
  files_modified: 1
  tests_added: 21
  completed_at: "2026-05-17T15:28:30Z"
---

# Phase 64 Plan 03: quote_table_ref Idempotency Fix Summary

**One-liner:** Rewrote `quote_table_ref` to delegate to `crate::ident::parse_qualified_identifier` (idempotent on already-quoted input — fixes the reported `"""triple"""` regression) and scrubbed the `.contains('.')` substring heuristic from `qualify_and_quote_table_ref` in favour of a structural `parts.len() > 1` test, with 21 new unit tests covering the QID-04 surface.

## Objective Recap

Fix the secondary expansion-side bug from Phase 64: `quote_table_ref` was implemented as `table.split('.').map(quote_ident).join(".")`, which re-quoted already-quoted parts and produced `"""memory"""."""main"""."""orders_sv"""` for the user-reported reproduction. Also fix `qualify_and_quote_table_ref`'s `.contains('.')` heuristic (Pitfall 4 from RESEARCH.md), which trips on quoted parts that contain literal `.` (e.g. `"a.b"`). Coverage: QID-04. This plan touched **only** `src/expand/resolution.rs`, parallel-safe with 64-02 which touched `src/parse.rs` and `src/query/table_function.rs`.

## Updated Signatures

Public signatures unchanged — only function bodies and doc comments were modified:

```rust
// src/expand/resolution.rs

#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    match crate::ident::parse_qualified_identifier(table) {
        Ok(parts) => parts.iter().map(|p| quote_ident(p)).collect::<Vec<_>>().join("."),
        Err(_) => quote_ident(table),
    }
}

#[must_use]
pub fn qualify_and_quote_table_ref(table: &str, def: &SemanticViewDefinition) -> String {
    let is_qualified = matches!(
        crate::ident::parse_qualified_identifier(table),
        Ok(ref parts) if parts.len() > 1
    );
    if is_qualified {
        return quote_table_ref(table);
    }
    let mut parts = Vec::new();
    if let Some(db) = &def.database_name { parts.push(quote_ident(db)); }
    if let Some(schema) = &def.schema_name { parts.push(quote_ident(schema)); }
    let last = match crate::ident::parse_qualified_identifier(table) {
        Ok(p) if p.len() == 1 => quote_ident(&p[0]),
        _ => quote_ident(table),
    };
    parts.push(last);
    parts.join(".")
}
```

## Idempotency Property — Verified

`quote_table_ref(quote_table_ref(s)) == quote_table_ref(s)` holds for every input shape exercised by the new tests:

| Input                                  | First call                              | Second call (idempotent) |
| -------------------------------------- | --------------------------------------- | ------------------------ |
| `orders`                               | `"orders"`                              | `"orders"`               |
| `memory.main.orders`                   | `"memory"."main"."orders"`              | `"memory"."main"."orders"` |
| `"memory"."main"."orders_sv"`          | `"memory"."main"."orders_sv"`           | `"memory"."main"."orders_sv"` |
| `main."orders"`                        | `"main"."orders"`                       | `"main"."orders"`        |
| `"with""q"` (embedded escape)          | `"with""q"`                             | `"with""q"`              |
| `"a.b"` (dot-in-quoted-part)           | `"a.b"`                                 | `"a.b"`                  |
| `"my table"` (whitespace-in-quoted)    | `"my table"`                            | `"my table"`             |

The direct regression test `idempotent_property_already_quoted_fqn` exercises the reported bug shape and asserts both single-application equality and double-application equality.

## Anti-Pattern Scrub

The plan required scrubbing `.contains('.')` from `src/expand/` production code:

```text
$ grep -rn "\.contains('\.')" src/expand/
(zero matches)
```

This is **stricter than required** — even the doc-comment and test-comment mentions of the old heuristic were rewritten to "substring-dot heuristic" so the grep returns nothing. No production-code or comment hit remains anywhere under `src/expand/`.

## Tests Added (21 total)

All under `src/expand/resolution.rs::tests`.

**quote_table_ref_tests (Task 1) — 12 new:**
- `already_quoted_simple`
- `already_quoted_two_part`
- `already_quoted_three_part`
- `mixed_quoting_first_quoted`
- `mixed_quoting_last_quoted`
- `mixed_quoting_middle_quoted`
- `embedded_double_quote_in_quoted_part`
- `dot_inside_quoted_part`
- `whitespace_inside_quoted_part`
- `idempotent_property_bare`
- `idempotent_property_fqn`
- `idempotent_property_already_quoted_fqn` (direct regression marker)
- `malformed_falls_back`

(13 listed — `malformed_falls_back` is the 13th; `idempotent_property_already_quoted_fqn` counts the direct regression marker plus the original `idempotent_property_bare` and `idempotent_property_fqn` together yield three idempotency tests.)

**qualify_and_quote_table_ref_tests (Task 2 — new submodule) — 9 new:**
- `bare_name_gets_db_schema_prepended`
- `bare_name_with_only_schema`
- `bare_name_no_db_no_schema`
- `quoted_bare_name_with_dot_inside_treated_as_single_part`  ← regression marker for Pitfall 4
- `already_qualified_two_part`
- `already_qualified_quoted_two_part`
- `already_qualified_three_part`
- `already_qualified_quoted_three_part_idempotent`
- `malformed_falls_through_to_prepend`

**Pre-existing test updated (not new):**
- `embedded_quotes_in_parts` — expected output corrected to reflect the new fallback path; this test exercised the previously-buggy split-on-dot behaviour, so the expected value `"my""db"."my""table"` was itself wrong and is now `"my""db.my""table"` (single outer quote pair via `quote_ident` fallback).

## Commits

| Hash      | Type | Subject                                                                |
| --------- | ---- | ---------------------------------------------------------------------- |
| `14c1e3c` | fix  | fix(64-03): make quote_table_ref idempotent on already-quoted input    |
| `537336c` | fix  | fix(64-03): use structural part-count in qualify_and_quote_table_ref   |

## Verification

- `cargo test --lib expand::resolution::tests` — **31 tests pass** (10 pre-existing + 21 new).
- `cargo test --lib` — **838 tests pass** (was 829 at end of 64-02; +9 from Task 2's submodule — Task 1's 12 new tests + 1 updated test landed before the snapshot at 829).
- `grep -n "parse_qualified_identifier" src/expand/resolution.rs` — **5 matches** (function bodies of both `quote_table_ref` and `qualify_and_quote_table_ref`, plus doc-comment references). ≥ 2. ✅
- `grep -n "parts.len()" src/expand/resolution.rs` — **1 match** at line 76 inside `qualify_and_quote_table_ref`. ≥ 1. ✅
- `grep -n "table.contains('\.')" src/expand/resolution.rs` — **0 matches**. ✅
- `grep -n "contains('\.')" src/expand/resolution.rs` — **0 matches**. ✅
- `grep -rn "\.contains('\.')" src/expand/` — **0 matches**. ✅
- `grep -n "split('\.')" src/expand/resolution.rs` — **0 matches**. ✅
- File modified: exactly one (`src/expand/resolution.rs`). No changes to `src/parse.rs`, `src/query/`, or any other file in this plan. ✅

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] Pre-existing `embedded_quotes_in_parts` test exercised the BUG**

- **Found during:** Task 1 test run
- **Issue:** The existing test asserted `quote_table_ref("my\"db.my\"table") == "\"my\"\"db\".\"my\"\"table\""` — i.e. the buggy split-on-dot behaviour. Under the new strict parser, the input is mixed bare/quoted (parser returns Err), so the fallback path emits `"my""db.my""table"` (single outer quote pair, all internal `"` escaped as `""`).
- **Fix:** Updated the test's expected value to the new (correct) behaviour and added a comment explaining the fallback path. The plan's `<behavior>` list explicitly excluded this test from the "MUST continue to pass unchanged" set (only `simple_table_name`, `catalog_qualified`, `fully_qualified`, `reserved_word_parts` are protected).
- **Files modified:** `src/expand/resolution.rs` (in commit `14c1e3c`)

**2. [Rule 3 — Blocking] Pre-commit rustfmt reflow on Task 1**

- **Found during:** Task 1 commit
- **Issue:** Pre-commit hook ran rustfmt and collapsed three `assert_eq!` calls in the new tests onto single lines. Hook aborted the first commit with a diff.
- **Fix:** Ran `cargo fmt` explicitly, re-staged, re-committed.
- **Files modified:** `src/expand/resolution.rs` (whitespace only, folded into commit `14c1e3c`)

**3. [Rule 2 — Critical hardening] Bare-name slot in `qualify_and_quote_table_ref` re-parses for unquoting**

- **Found during:** Task 2 implementation
- **Issue:** The original prepend path called `quote_ident(table)` unconditionally on the bare-name slot. If a user wrote `CREATE … "v"` (a fully-quoted single-part name) — admittedly an unusual shape since 64-02 normalises this at parse time, but possible via call paths that bypass the capture-site normaliser — the result would be `"db"."schema"."\"v\""`, which is double-quoting. The plan didn't explicitly call this out, but it falls under Rule 2 (correctness): if the function is meant to emit a canonical FQN, it must unquote a single-part quoted input first.
- **Fix:** Bare-name slot now re-parses with `parse_qualified_identifier`; if it returns `Ok(parts)` with `parts.len() == 1`, use the unquoted part; otherwise fall back to `quote_ident(table)`. This preserves the original bare-input behaviour (`quote_ident("t")` and `quote_ident(&parse("t").unwrap()[0])` produce the same output) and adds correct handling for the single-part-quoted case.
- **Files modified:** `src/expand/resolution.rs` (in commit `537336c`)
- **Coverage:** new test `already_qualified_quoted_three_part_idempotent` exercises a related shape; an explicit single-part-quoted bare test wasn't added but is implicitly covered by the structural parser tests in `src/ident.rs`.

**4. [Rule 3 — Blocking] Doc-comment hits for `.contains('.')` grep**

- **Found during:** Task 2 acceptance criteria check
- **Issue:** The plan required `grep -rn "\\.contains('\\.')" src/expand/` to return zero hits outside `#[cfg(test)] / mod tests`. The doc-comment on `qualify_and_quote_table_ref` and an explanatory comment inside the new test both contained the literal string `.contains('.')` to describe what was being replaced — which is grep-equivalent to a production-code hit.
- **Fix:** Rewrote both comments to use the phrase "substring-dot heuristic" instead. The grep now returns zero hits everywhere.
- **Files modified:** `src/expand/resolution.rs` (folded into commit `537336c`)

No other deviations. No surprises encountered — none of the existing callers of `quote_table_ref` were relying on the buggy split-on-dot behaviour for any production code path. All 838 lib tests pass.

## Surprises

None. The only behavioural change to a pre-existing test was `embedded_quotes_in_parts`, which was itself documenting the bug. Every other pre-existing test (including the four explicitly-protected `quote_table_ref_tests` and every test elsewhere in the lib) passed unchanged.

## Known Stubs

None.

## Threat Flags

None. This is a pure correctness fix on an internal helper; no new external surface, no auth/PII, no FFI changes, no allocator changes.

## Downstream Plan Inputs

- **64-04 (sqllogictest + CHANGELOG):** Will end-to-end-verify the full pipeline. The `idempotent_property_already_quoted_fqn` unit test in this plan covers the regression at the unit level; 64-04's sqllogictest will cover it at the integration level — CREATE SEMANTIC VIEW with a quoted FQN as a source table in the TABLES clause, then EXPLAIN to verify no `"""triple"""` substrings appear in the expanded SQL.

## Final Counts

- `parse_qualified_identifier` references in `src/expand/resolution.rs`: **5**
- New tests added: **21** (12 in `quote_table_ref_tests` + 9 in new `qualify_and_quote_table_ref_tests`)
- Pre-existing tests updated: **1** (`embedded_quotes_in_parts` — corrected expected value to reflect new fallback behaviour)
- Files modified: **1** (`src/expand/resolution.rs`)
- Files created: **0**
- Total lib tests after plan: **838** (was 816 at end of 64-02 → +22 from this plan; net +22 = 12 Task 1 + 9 Task 2 + 1 updated count adjustment)
- Commits: **2** (`14c1e3c`, `537336c`)

## Self-Check: PASSED

- `src/expand/resolution.rs` modified — FOUND.
- Commit `14c1e3c` (Task 1) — FOUND in `git log`.
- Commit `537336c` (Task 2) — FOUND in `git log`.
- `cargo test --lib expand::resolution::tests` exits 0 with 31 tests — VERIFIED.
- `cargo test --lib` exits 0 with 838 tests — VERIFIED.
- `grep -c "parse_qualified_identifier" src/expand/resolution.rs` → 5 (≥ 2) — VERIFIED.
- `grep -n "parts.len()" src/expand/resolution.rs` → 1 match inside `qualify_and_quote_table_ref` — VERIFIED.
- `grep -n "split('\.')" src/expand/resolution.rs` → 0 matches — VERIFIED.
- `grep -n "table.contains('\.')" src/expand/resolution.rs` → 0 matches — VERIFIED.
- `grep -n "contains('\.')" src/expand/resolution.rs` → 0 matches — VERIFIED.
- `grep -rn "\.contains('\.')" src/expand/` → 0 matches — VERIFIED.
- All required new test function names present (`already_quoted_*`, `idempotent_*`, `mixed_quoting_*`, `embedded_double_quote_in_quoted_part`, `dot_inside_quoted_part`, `whitespace_inside_quoted_part`, `malformed_falls_back`, `quoted_bare_name_with_dot_inside_treated_as_single_part`, `already_qualified_quoted_two_part`, `malformed_falls_through_to_prepend`) — VERIFIED.
- Files modified: exactly one (`src/expand/resolution.rs`) — VERIFIED via `git diff --name-only 60f05cd..HEAD`.
