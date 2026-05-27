---
phase: 67-expansion-sql-coverage-and-tech-debt-cleanup
reviewed: 2026-05-27T00:00:00Z
depth: standard
files_reviewed: 9
files_reviewed_list:
  - src/body_parser.rs
  - test/integration/test_adbc_queries.py
  - test/sql/phase67_qualified_emission.test
  - test/sql/phase67_quoted_source_tables.test
  - test/sql/phase46_fact_query.test
  - test/sql/phase47_semi_additive.test
  - test/sql/phase48_window_metrics.test
  - test/sql/TEST_LIST
  - TECH-DEBT.md
findings:
  critical: 0
  warning: 3
  info: 4
  total: 7
status: issues_found
---

# Phase 67: Code Review Report

**Reviewed:** 2026-05-27
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

Phase 67 Plan 02 ports `find_identifier_end` from `src/ident.rs` into the
`TABLES (...)` clause source-table-name slot in `src/body_parser.rs`, plus a
sqllogictest fixture (`phase67_quoted_source_tables.test`) and 5 new Rust unit
tests. Plan 03/04 sites in this review are sqllogictest shape pins
(`phase67_qualified_emission.test`, plus tightenings to phase46/47/48 fixtures)
and a 7-scenario ADBC regression test (`test_adbc_queries.py`).

The functional fix is correct for the canonical TECH-DEBT #24 cases — quoted
names with embedded whitespace and the "PRIMARY KEY inside the name" pathological
case both parse correctly. Findings below are concentrated on three areas:

1. **Regression in error reporting for malformed `o AS PRIMARY KEY (id)`** — the
   new identifier-walk happily eats `PRIMARY` as a bare table name, downgrading
   what was previously a clear "Missing physical table name" error into a
   malformed `TableRef` that fails later with a worse message. This is a
   user-facing quality regression in the error path. (WR-01)
2. **Inconsistent SQL-literal escaping in `test_adbc_queries.py`** — the IN-02
   fix escapes `extension_path` but the sibling `other_db_path` interpolation
   on the same file (scenario 7, line 470) was left bare. Defense-in-depth
   parity broken. (WR-02)
3. **Dead-code branch in the new identifier loop** — the dot-consumption
   `continue` arm in `parse_single_table_entry` is unreachable in normal
   execution because `find_identifier_end` already walks across dots
   internally. Not a correctness bug, but the comment "Walk dot-separated
   segments" misrepresents what the helper actually does and risks future
   readers writing defensive code against a non-issue. (WR-03)

Test coverage in the new fixtures is adequate for the happy paths and the
canonical bug. Coverage gaps documented as Info items below.

## Warnings

### WR-01: Regression in error path for `TABLES (alias AS PRIMARY KEY (col))`

**File:** `src/body_parser.rs:692-720`
**Issue:** Pre-Phase-67, the parser computed `table_name = after_as[..pk_start].trim()`
where `pk_start` was the byte position of the `PRIMARY` keyword. For the input
`o AS PRIMARY KEY (id)`, `pk_start == 0`, so `table_name` evaluated to the empty
string and the explicit empty-check below the assignment surfaced a clear
"Missing physical table name after AS for alias 'o'" error.

Post-fix, the identifier walk runs first and `find_identifier_end` returns
the byte offset of the space after `PRIMARY` (length 7), so `name_end = 7`,
`table_name = "PRIMARY"`, and `after_name = " KEY (id)"`. The subsequent
`find_primary_key` scan over `" KEY (id)"` fails to match (no `PRIMARY`
keyword in the post-name slice), `find_unique` also fails, and the function
returns a `TableRef { alias: "o", table: "PRIMARY", pk_columns: vec![] }`.
The downstream resolver then fails with a less informative error
("table 'PRIMARY' does not exist") instead of the structured
"Missing physical table name" message.

Same regression class for `o AS UNIQUE (id)`, `o AS COMMENT 'x'`, and any
trailing-annotation keyword that the pre-fix code used as an end-of-name
sentinel.

The empty-check at line 711 (`if table_name.is_empty()`) is now effectively
unreachable: it fires only when `after_as` is itself empty (already
implicitly covered by the `segment_end == 0` branch above) or when
`after_as[..name_end].trim()` differs from `after_as[..name_end]`, which
cannot happen because `find_identifier_end` never advances past leading
whitespace (it returns 0 at the first whitespace byte).

**Fix:** Reject the case where the captured table name is a reserved DDL
keyword in this position. Minimal patch:

```rust
let table_name = after_as[..name_end].trim();
let upper_name = table_name.to_ascii_uppercase();
if matches!(
    upper_name.as_str(),
    "PRIMARY" | "UNIQUE" | "COMMENT" | "WITH"
) {
    return Err(ParseError {
        message: format!(
            "Missing physical table name after AS for alias '{alias}' in TABLES clause.",
        ),
        position: Some(after_as_offset),
    });
}
```

Alternatively, add a regression test that pins the new error shape (less
ideal — codifies the worse error) so the behaviour change is at least
explicit.

### WR-02: Inconsistent SQL-literal escaping in `test_adbc_queries.py`

**File:** `test/integration/test_adbc_queries.py:470`
**Issue:** The IN-02 fix escapes `extension_path` for SQL string-literal
interpolation at line 100-101 (`_bootstrap_extension`), but the sibling
`ATTACH '{other_db_path}'` interpolation at line 470 (scenario 7) was left
bare. Both paths come from `tmp_path` (a `pathlib.Path` from
`tempfile.TemporaryDirectory`), so in practice macOS/Linux temp dirs will
not contain single quotes — but the defense-in-depth justification cited
for the IN-02 fix ("path may contain '") applies symmetrically here. A
future refactor that lets a non-temp path flow into `other_db_path`
re-introduces the gap.

**Fix:** Apply the same escape:

```python
other_db_path_sql = other_db_path.replace("'", "''")
_execute(conn, f"ATTACH '{other_db_path_sql}' AS db2")
```

Or extract a `_quote_sql_literal(s)` helper and use it at both sites.

### WR-03: Dot-consumption branch in identifier walk is dead code

**File:** `src/body_parser.rs:692-709`
**Issue:** The loop comment says "Walk dot-separated segments via
`find_identifier_end` so quoted identifiers with internal whitespace ...
survive intact." In fact, `find_identifier_end` already traverses dots
that appear outside a quoted region (see
`src/ident.rs::find_identifier_end_tests::fqn_with_quoted_parts_runs_to_whitespace`
which asserts `find_identifier_end("\"db\".\"sch\".\"v\" AS x", true) == 14`,
spanning all three dot-separated parts in one call). The dot-detection
arm of the loop:

```rust
if name_end < after_as.len() && after_as.as_bytes()[name_end] == b'.' {
    name_end += 1; // consume dot, continue to next segment
    continue;
}
```

can only fire when `find_identifier_end` returns a position whose byte is
`.` — but `find_identifier_end`'s delimiter set is `{whitespace, ';',
'('}`, never `.`. So the arm is unreachable in practice.

This is not a correctness bug — the helper produces the right captured
slice either way — but the comment is misleading, and a future maintainer
who sees "Walk dot-separated segments" may add unnecessary defensive logic
in the helper or rewrite the loop on the assumption that it actually does
something.

**Fix:** Either delete the dot-detection arm and the surrounding loop
(replacing with a single `find_identifier_end` call) — clearer intent
preserved with zero behavioural change — OR revise the comment to say:

```rust
// `find_identifier_end` walks across dot-separated parts internally; the
// dot-rejoin loop below is defensive belt-and-braces for any future
// helper change that narrows the inner walk.
```

If the loop is kept, add a Rust unit test that proves the dot-arm
actually triggers (e.g. construct a contrived input where the inner
helper terminates at a dot) — otherwise it's untested dead code.

## Info

### IN-01: Unterminated quoted source-table name silently accepted

**File:** `src/body_parser.rs:692-720`
**Issue:** For input `o AS "unclosed` (no closing quote),
`find_identifier_end` saturates at `input.len()` per its documented
contract ("the caller's parser surfaces the structural error"). But this
caller does not surface the error — it accepts `table_name = "\"unclosed"`
and returns a `TableRef` with that malformed value. Downstream resolution
will fail with a generic error.

**Fix:** After the identifier walk, check whether the captured slice has
balanced quoting. Simplest: count unescaped `"` bytes in `table_name`; if
odd, return a structured "unterminated quoted identifier" error.
Alternatively, call `crate::ident::parse_qualified_identifier(table_name)`
in a debug-asserts-only branch and surface that error.

### IN-02: Test coverage gap — no mixed bare/quoted dot-qualified source name

**File:** `test/sql/phase67_quoted_source_tables.test`, `src/body_parser.rs:2596-2609`
**Issue:** Scenario 2 of the sqllogictest fixture and
`test_parse_single_table_entry_3part_quoted_fqn_with_whitespace` both use
fully-quoted FQNs (`"my db"."schema"."my table"` and similar). There is
no test for mixed quoting like `staging."my orders"` or `"my db".sch.t`
or `db."my schema".tbl`. While `find_identifier_end`'s own tests cover
mixed cases, the body-parser wrapper is not exercised against them. If a
future refactor narrows the inner helper's dot-walking, mixed-quoting
regressions would slip through.

**Fix:** Add one Rust unit test:

```rust
#[test]
fn test_parse_single_table_entry_mixed_quoted_and_bare() {
    let result =
        parse_tables_clause("o AS staging.\"my orders\" PRIMARY KEY (id)", 0).unwrap();
    assert_eq!(result[0].table, "staging.\"my orders\"");
    assert_eq!(result[0].pk_columns, vec!["id"]);
}
```

And one sqllogictest scenario in `phase67_quoted_source_tables.test`.

### IN-03: Test fixture cleanup misses default-schema tables

**File:** `test/sql/phase67_quoted_source_tables.test:140-156`
**Issue:** The cleanup section drops the four semantic views and the
`"stage 2"` schema CASCADE, but `"my orders"`, `"weird PRIMARY KEY name"`,
and `p67_plain_orders` (all in `main`) are not dropped. sqllogictest
typically gives each file a fresh in-memory DB so cross-file leakage is
not a hazard today, but if the runner ever shares DBs this becomes a
silent dependency. Sibling fixtures (e.g. `phase47_semi_additive.test`
lines 215-221) explicitly drop their base tables.

**Fix:** Add three `DROP TABLE IF EXISTS` statements at the bottom of the
cleanup block:

```
statement ok
DROP TABLE IF EXISTS "my orders";

statement ok
DROP TABLE IF EXISTS "weird PRIMARY KEY name";

statement ok
DROP TABLE IF EXISTS p67_plain_orders;
```

### IN-04: `find_primary_key` boundary check inconsistent with `find_unique`

**File:** `src/body_parser.rs:875-905` vs `815-834`
**Issue:** `find_primary_key` uses `!bytes[i - 1].is_ascii_alphanumeric()`
as the word-boundary check before `PRIMARY` (no `_` exclusion), while
`find_unique` uses `!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1]
!= b'_'`. This means a hypothetical input like `my_PRIMARY KEY (id)` would
match `PRIMARY` because the underscore counts as a word boundary in
`find_primary_key` but not in `find_unique`. Not exploitable today (the
Phase 67 surgery now ensures `find_primary_key` runs only on the
post-table-name slice, where alphanumeric/underscore prefixes are
unlikely), but the divergence is gratuitous and could surprise future
maintainers.

**Fix:** Align `find_primary_key` boundary check with `find_unique`:

```rust
let before_ok = i == 0 || {
    let c = bytes[i - 1];
    !c.is_ascii_alphanumeric() && c != b'_'
};
```

Apply the same change to the `KEY` boundary check at line 894-895.

---

_Reviewed: 2026-05-27_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
