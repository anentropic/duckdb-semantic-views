---
phase: 64
plan: 02
subsystem: parser
tags: [ident, parser, ddl, quoted-identifiers, capture-sites, runtime-arg]
requires:
  - module: crate::ident
    surface: parse_qualified_identifier, normalize_view_name, find_identifier_end (from 64-01)
provides:
  - sites: 5 DDL identifier-capture points in src/parse.rs delegate to crate::ident
  - site: emit_native_create_sql carries a MANDATORY defensive normalize_view_name shadow
  - site: src/query/table_function.rs bind() normalises the semantic_view() positional arg
affects:
  - src/parse.rs
  - src/query/table_function.rs
tech_stack:
  added: []
  patterns:
    - quote-aware delimiter scan replaces naive whitespace-find at every identifier-capture site
    - defensive shadowing at the catalog-write boundary (idempotent on already-normalised input)
key_files:
  modified:
    - src/parse.rs
    - src/query/table_function.rs
decisions:
  - Defensive shadow in emit_native_create_sql is UNCONDITIONAL — the trace through validate_create_body
    already normalises the happy-path input, but the shadow hardens the boundary against any future caller
    or refactor that bypasses validate_create_body. Cost is negligible (one strdup + state-machine pass).
  - Runtime semantic_view() arg also normalised so semantic_view('"v"', ...) resolves identically to
    semantic_view('v', ...). This keeps the user-facing contract uniform across DDL and query paths.
  - rewrite_alter normalises BOTH the source-name slot AND the RENAME TO target slot. Pre-fix, ALTER
    RENAME TO "memory"."main"."new_v" would have stored the new row under the literal 30-character
    string, regardless of whether the source was fixed.
  - Test coverage for the runtime-arg path is deferred to Plan 64-04 (sqllogictest). bind() requires a
    runtime BindInfo and cannot be unit-tested.
requirements:
  - QID-01 (fully-quoted FQN CREATE)
  - QID-02 (partial / mixed quoting)
  - QID-03 (DROP / ALTER / DESCRIBE / SHOW COLUMNS + runtime arg)
  - QID-06 (error messages reference bare name — implicit from capture-site fix)
metrics:
  duration_minutes: 7
  tasks: 2
  files_modified: 2
  tests_added: 23
  completed_at: "2026-05-17T15:22:00Z"
---

# Phase 64 Plan 02: DDL Capture-Site Wiring Summary

**One-liner:** Wired `crate::ident::{normalize_view_name, find_identifier_end}` into five DDL identifier-capture sites in `src/parse.rs` (including BOTH slots of ALTER RENAME), added a MANDATORY defensive normalise shadow at `emit_native_create_sql` entry, and normalised the runtime `semantic_view()` positional arg in `src/query/table_function.rs:482` — quoted FQNs in every DDL form and the table-function call now resolve to the bare unquoted last part as the catalog key.

## Objective Recap

Wire the 64-01 identifier-parsing helpers into every place that captures a view name from raw SQL text, so that `CREATE / DROP / ALTER / DESCRIBE / SHOW COLUMNS` accept quoted (`"v"`), fully-quoted FQN (`"db"."sch"."v"`), partially-quoted (`main."v"`), and inner-whitespace (`"my view"`) forms uniformly. Covers QID-01..03 capture-side and QID-06 implicitly. Out of scope here: `quote_table_ref` re-quoting bug (64-03) and the sqllogictest fixture / CHANGELOG (64-04).

## Capture Sites — Post-edit Line Numbers

| # | Site | File | Line(s) | normalize_view_name? | find_identifier_end? |
|---|------|------|---------|----------------------|----------------------|
| 1 | `extract_name_only` (DROP / DESCRIBE / SHOW COLUMNS / ALTER source) | src/parse.rs | 326 | ✅ | ✅ (allow_paren=false) |
| 2 | `rewrite_alter` source-name slot | src/parse.rs | 643 | ✅ | ✅ (allow_paren=false) |
| 3 | `rewrite_alter` RENAME TO target slot | src/parse.rs | 659 | ✅ | — (trims to whitespace, no quote-aware scan needed) |
| 4 | `extract_ddl_name` CREATE branch | src/parse.rs | 811 | ✅ | ✅ (allow_paren=true) |
| 5 | `validate_create_body` CREATE name segment | src/parse.rs | 1161 | ✅ | ✅ (allow_paren=true) |
| 6 | **`emit_native_create_sql` defensive shadow (MANDATORY)** | src/parse.rs | **1890** | ✅ | — |
| 7 | `bind()` runtime arg in semantic_view() | src/query/table_function.rs | 488 | ✅ | — |

`grep -c normalize_view_name src/parse.rs` (production call sites only): **6** (lines 326, 643, 659, 811, 1161, 1890). The `use` declaration at line 14 and a test-module documentation comment near line 5028 are excluded from the production count.

`grep -c normalize_view_name src/query/table_function.rs`: **1** (line 488).

ALTER RENAME both slots verified: source at line 643, target at line 659 — `alter_rename_both_quoted` unit test exercises the full path.

## Defensive Shadow at `emit_native_create_sql` (Site E)

Located at `src/parse.rs:1879-1894` (the `Defensive normalisation` comment block, ending with the shadow `let name = normalize_view_name(name).map_err(...)?;`). The function parameter is `name: &str`; the shadow rebinds it to an owned `String`, which auto-derefs to `&str` at the three downstream call sites:

- `escape_sql_arg(&name)` at line 1895
- `ctx.catalog.exists(&name)` at line 1904
- `enrich_definition_for_create(&name, ...)` at line 1927 (required `&name` adjustment because the call previously moved the `&str` parameter; `String` auto-derefs but only behind `&`)

A second doc-comment block at lines 1896-1903 documents the bare-key invariant the shadow guarantees.

The shadow is **unconditional** — even though the happy-path through `validate_create_body` already normalises before reaching this function, the shadow hardens the catalog boundary against any future refactor or alternate caller. Cost: one `String` allocation + one state-machine pass on a typically-small identifier (~tens of nanoseconds), negligible relative to the catalog INSERT that follows.

## Tests Added (23 total)

All under `parse::tests::phase64_quoted_ident_tests` in `src/parse.rs`:

**DROP / DESCRIBE / SHOW COLUMNS (Site A via rewrite_ddl):**
- `drop_with_quoted_fqn` — `DROP SEMANTIC VIEW "db"."sch"."v"` → `drop_semantic_view('v')`
- `drop_with_quoted_bare` — `"orders_sv"` → `orders_sv`
- `drop_with_unquoted_fqn` — `db.sch.v` → `v`
- `drop_with_partial_quoting` — `main."orders_sv"` → `orders_sv`
- `drop_with_quoted_whitespace_name` — `"my view"` survives the inner space
- `drop_if_exists_with_quoted_fqn`
- `describe_with_quoted_fqn` — `"memory"."main"."v"` → `v`
- `show_columns_with_quoted_fqn`
- `drop_with_unterminated_quote_errors` — error path

**CREATE forms (Sites B + C via extract_ddl_name / validate_and_rewrite):**
- `create_with_quoted_fqn_extracts_bare_name`
- `create_or_replace_with_quoted_fqn_extracts_bare_name`
- `create_if_not_exists_with_quoted_fqn_extracts_bare_name`
- `create_with_partial_quoting_extracts_bare_name`
- `create_with_quoted_whitespace_name_extracts_intact`
- `create_with_unterminated_quote_errors`

**ALTER (Site D — source + target slots):**
- `alter_rename_source_quoted` — `ALTER ... "v" RENAME TO new_name`
- `alter_rename_target_quoted` — `ALTER ... v RENAME TO "memory"."main"."new_v"` → both stored as bare
- `alter_rename_both_quoted` — both slots fully-quoted FQN
- `alter_set_comment_with_quoted_source`
- `alter_unset_comment_with_quoted_source`
- `alter_rename_target_unterminated_quote_errors`

**extract_ddl_name CREATE branch (Site C explicit):**
- `extract_ddl_name_quoted_fqn_create`
- `extract_ddl_name_mixed_quoting_create`

All 23 pass. Full `cargo test --lib` suite: **816 tests, 0 failures**.

## Commits

| Hash      | Type | Subject                                                                              |
| --------- | ---- | ------------------------------------------------------------------------------------ |
| `b50f725` | feat | feat(64-02): normalise quoted identifiers at parse.rs DDL capture sites               |
| `5bb6c7a` | feat | feat(64-02): defensive identifier normalisation at emit + runtime arg                 |

## Verification

- `cargo test --lib parse::tests::phase64_quoted_ident_tests` — 23 tests pass.
- `cargo test --lib` — 816 tests pass (no regressions).
- `cargo build --lib --features extension` — extension feature build succeeds (Site E + emit_native_create_sql is feature-gated; Site F is in the extension code path; both compile).
- `grep -cE "normalize_view_name\(" src/parse.rs` → 6 production call sites (5 capture sites + 1 defensive shadow). Plus 1 import + 2 test-module documentation references = 9 raw matches. ✅ ≥ 6.
- `grep -cE "find_identifier_end\(" src/parse.rs` → 4. ✅ ≥ 4.
- `grep -n "Defensive normalisation" src/parse.rs` → 1 match at line 1879. ✅ ≥ 1.
- `grep -n "bare view identifier" src/parse.rs` → 2 matches (lines 1881, 1897 — the two doc-comment blocks around the shadow). ✅ ≥ 1.
- `grep -c "normalize_view_name" src/query/table_function.rs` → 1. ✅ == 1.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] `enrich_definition_for_create` parameter type after defensive shadow**

- **Found during:** Task 2 first extension-feature build
- **Issue:** Plan §<behavior> step 2 anticipated that the shadowed `name` (now `String`) would still pass `enrich_definition_for_create(name, ...)`. Compile error E0308: `expected &str, found String` — auto-deref does not apply when the call moves the value.
- **Fix:** Pass `&name` explicitly at line 1927.
- **Files modified:** `src/parse.rs` (one-character edit, folded into commit `5bb6c7a`)
- **Commit:** `5bb6c7a`

**2. [Rule 3 — Blocking] Pre-commit rustfmt reflow**

- **Found during:** Task 1 commit
- **Issue:** Pre-commit hook ran rustfmt and reflowed a handful of `assert_eq!` calls and `let name = normalize_view_name(...)` statements that had been written across two lines. Hook aborted the first commit with a diff.
- **Fix:** Ran `cargo fmt` explicitly, re-staged the canonical output, re-committed.
- **Files modified:** `src/parse.rs` (whitespace only)
- **Commit:** folded into `b50f725` on re-commit.

**3. [Plan-deviation note — not a deviation per se] Site D source-slot `find_identifier_end` guard**

- **Found during:** Task 1 implementation of `rewrite_alter`
- **Issue:** Plan §<behavior> Site D said "replace the `name_end` scan + capture with find_identifier_end". The original code used `.ok_or("Missing view name after ALTER SEMANTIC VIEW")?` which required a non-`None` result (i.e. there must be something AFTER the name). `find_identifier_end` returns `input.len()` if it scans to end-of-input, so I needed to surface the same "missing operation" error explicitly: I check `name_end == 0 || name_end == after_prefix.len()` (no name OR no rest-of-clause). This preserves the original semantics for `ALTER SEMANTIC VIEW v` (no RENAME / SET / UNSET) — without it, `rewrite_alter` would have unwrapped the post-name empty rest and silently fallen into the "unsupported operation" branch with a confusing error.
- **Outcome:** Existing test `test_validate_rewrite_alter_missing_operation` continues to pass.

No feature-gating tweaks were needed for `src/query/table_function.rs` — the file already lives in the extension code path, and `crate::ident` has no feature gate.

## Known Stubs

None. Every code change wires a concrete normaliser; no placeholders, no empty values flowing to UI.

## Threat Flags

None. Phase 64 is parser hardening on a path with no new external surface; the defensive shadow is purely defensive (correctness, not auth/PII).

## Downstream Plan Inputs

- **64-03 (`quote_table_ref` fix):** Will use the same `crate::ident::parse_qualified_identifier` helper to split source-table refs in the TABLES clause before re-quoting. This plan (64-02) has NOT touched `src/expand/` — 64-03's exclusive territory.
- **64-04 (sqllogictest fixture + CHANGELOG + REQUIREMENTS):** Will cover the end-to-end CREATE → store-as-bare → `semantic_view('orders_sv', ...)` → resolve round-trip with the extension loaded. The runtime-arg normalisation path (Site F) is NOT unit-tested in this plan (bind() requires a runtime BindInfo); the sqllogictest fixture is the validation gate.

## Final Counts

- `grep -cE "normalize_view_name\(" src/parse.rs`: **6** (production call sites)
- `grep -c "normalize_view_name" src/query/table_function.rs`: **1**
- `grep -c "find_identifier_end" src/parse.rs`: **4**
- `grep -c "Defensive normalisation" src/parse.rs`: **1**

## Self-Check: PASSED

- `b50f725` exists — VERIFIED via `git log --oneline -3`.
- `5bb6c7a` exists — VERIFIED via `git log --oneline -3`.
- `src/parse.rs` modified, `src/query/table_function.rs` modified — VERIFIED via `git status` post-commits (clean working tree).
- No edits to `src/expand/resolution.rs` (64-03's territory) — VERIFIED.
- No edits to `test/sql/` (64-04's territory) — VERIFIED.
- `cargo test --lib` exits 0 with 816 tests — VERIFIED.
- `cargo build --lib --features extension` clean — VERIFIED.
- `grep -cE "normalize_view_name\(" src/parse.rs` ≥ 6 — VERIFIED (6).
- `grep -c "normalize_view_name" src/query/table_function.rs` == 1 — VERIFIED.
- `grep -n "Defensive normalisation" src/parse.rs` ≥ 1 — VERIFIED.
- ALTER RENAME both slots normalised (lines 643 + 659) — VERIFIED.
