# Phase 68: Pre-Tag Cleanup — Phase 67 Review + PR #35 Code Review Follow-ups - Research

**Researched:** 2026-05-27
**Domain:** Tactical hygiene (body-parser surgical edits + test polish + one structural port)
**Confidence:** HIGH

## Summary

CONTEXT.md and SCOPE.md already inventory the 13 items and lock the decisions. This research verifies each code site matches its SCOPE description, resolves the one open planning landmine (B1/B2 call-site shape), and lays out the validation map. All findings are HIGH confidence — verified directly against the working tree on `milestone/v0.10.0`.

**Primary recommendation:** All 13 items are tractable with the (b)-class ports SCOPE.md describes. TECH-DEBT #25's classification of B1/B2 as "(c)-class structural-rewrite-required" was pessimistic — the call sites both receive raw `&str` content (already paren-extracted, comma-split), so porting `find_identifier_end` follows the same loop shape as `parse_single_table_entry`. The plan can confidently treat 68-03 as a (b)-class port.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** SCOPE.md is the authoritative item inventory; phase has minimal architectural gray area.
- **D-02:** A1 + A3 bundle in a single commit (collapse dead loop first, then add keyword guard at the clean site).
- **D-03:** A1 guard rejects bare-name `PRIMARY|UNIQUE|FOREIGN|REFERENCES|NOT` (case-insensitive), emits literal `"Missing physical table name after AS for alias '{alias}' in TABLES clause."` — pre-Phase-67 error message is the contract.
- **D-04:** A2 brings `test_adbc_queries.py:470` to parity with line 100's `.replace("'", "''")` treatment.
- **D-05:** A4 (unterminated quoted identifier) + A7 (`find_primary_key` boundary alignment) are surgical body-parser edits.
- **D-06:** A5 + A6 extend `test/sql/phase67_quoted_source_tables.test`.
- **D-07:** B1/B2 follow the `parse_single_table_entry` tokeniser shape — walk dot-separated segments via `find_identifier_end`, terminated by `,`, `)`, or end-of-clause. **Re-classified from (c)-class to (b)-class.**
- **D-08:** NON ADDITIVE BY and OVER ORDER BY accept dotted paths (`table.col`). Contract extension, not regression.
- **D-09:** B1/B2 require sqllogictest coverage — at least one fixture per site with a quoted identifier containing literal whitespace.
- **D-10:** If investigation reveals (c)-class scope, surface as SUMMARY finding and renegotiate.
- **D-11:** C1 — swap `&body[..body.len().min(400)]` to `body.get(..400).unwrap_or(body)`.
- **D-12:** C2 — drop trailing `(` from `["std::", "mem::", "transmute("]` so both bare and turbofish forms catch; one-line comment explaining why.
- **D-13:** C3 — **delete** `test/sql/p651_ok.yaml` (do NOT rewrite test to load it; rewriting would defeat the runtime `COPY TO ...` contract under test).

### Claude's Discretion

- Test file/fixture naming, helper function names (if introducing a shared tokeniser helper for B1/B2), commit message wording within the 68-01/68-02/68-03 plan structure.

### Deferred Ideas (OUT OF SCOPE)

- REL-01..04 (CHANGELOG, version bump, example file, DuckDB v1.5.3 bump) — milestone-close concerns; `/gsd-complete-milestone` owns them.

</user_constraints>

## Project Constraints (from CLAUDE.md)

- `just test-all` is the quality gate. Phase 68 verification must run the full suite (Rust + sqllogictest + DuckLake + ADBC), not just `cargo test`.
- `just ci` (adds clippy pedantic + fmt + cargo-deny + fuzz target compile) before any push to main.
- New sqllogictest files (B1/B2 per D-09) **must** be added to `test/sql/TEST_LIST` or the runner silently skips them.
- `statement error` assertions use **block form** (`---- separator` + substring), not inline regex — runner does not support inline form.
- Pre-commit hook runs `cargo fmt --check` + clippy; never `--no-verify`.
- Build/test commands: never bare `tail -N` on long-running output; redirect to `/tmp/claude/x.log` then tail the file (per repo rule 1).

## Item-by-Item Code-Site Verification

### A1 — Reserved-keyword guard at the `find_identifier_end` walk site

**Site:** `src/body_parser.rs:692-720`

**Current code (verified at lines 692-718):** The identifier walk runs first. `find_identifier_end(&after_as[name_end..], /* allow_paren = */ true)` returns `7` for input `o AS PRIMARY KEY (id)` (the whitespace after `PRIMARY`), so `table_name = "PRIMARY"`. The subsequent `find_primary_key(&upper_after_name)` scan over `" KEY (id)"` does NOT find `PRIMARY` because the keyword was already eaten. The empty-check at line 711 fires only if `after_as[..name_end].trim() == ""`, which cannot happen after a non-zero `find_identifier_end` return.

**Fix scope (D-03):** Insert a guard between line 710 (`let table_name = after_as[..name_end].trim();`) and line 711 (`if table_name.is_empty()`). Reject `to_ascii_uppercase()` matches against `PRIMARY|UNIQUE|FOREIGN|REFERENCES|NOT`. The REVIEW.md draft used `"PRIMARY" | "UNIQUE" | "COMMENT" | "WITH"` — CONTEXT.md D-03 explicitly widens to `PRIMARY|UNIQUE|FOREIGN|REFERENCES|NOT` and drops `COMMENT|WITH`. Honor D-03; the REVIEW draft is informational only.

**Landmine:** The guard's word "PRIMARY" must run *before* the `is_empty()` check; otherwise the case `o AS PRIMARY` (no trailing `KEY`) would still surface a different error path. Place immediately after line 710.

**Confidence:** HIGH — directly verified.

### A3 — Dead dot-consumption loop arm

**Site:** `src/body_parser.rs:692-709`

**Current code:** Lines 692-709 contain the loop. `find_identifier_end` (verified at `src/ident.rs:186-217`) walks **across dots while outside quoted regions** (its delimiter set is `{whitespace, ';', '('}`, NEVER `.`). The dot-rejoin arm at lines 704-707 (`if after_as.as_bytes()[name_end] == b'.' { name_end += 1; continue; }`) is unreachable: `find_identifier_end` only returns at whitespace, `;`, or `(` — never at `.`.

**Fix scope (D-02 bundle):** Replace the entire loop (lines 692-709) with a single `find_identifier_end` call + the `segment_end == 0` empty-check. With the A1 guard from D-03 inserted immediately after, the resulting block is ~8 lines vs. the current ~18.

**Bundle ordering (D-02):** Collapse loop FIRST, then insert the A1 guard at the clean site. Doing A1 first would mean adding the guard inside the dead loop body and re-locating it after A3's collapse.

**Confidence:** HIGH — verified against `src/ident.rs` source + doctest `fqn_with_quoted_parts_runs_to_whitespace`.

### A4 — Unterminated quoted source-table name silently accepted

**Site:** `src/body_parser.rs:692-720` (same block as A1/A3)

**Current behavior:** `find_identifier_end` saturates at `input.len()` if it encounters an unterminated `"`. For input `o AS "unclosed`, the function captures `table_name = "\"unclosed"` (no closing quote), and the resulting `TableRef` carries the malformed value downstream.

**Fix scope:** After capturing `table_name` (post-A1-guard, post-A3-collapse), count unescaped `"` bytes in the slice. If odd, return a `ParseError` with `position: Some(after_as_offset)` and message like `"Unterminated quoted identifier in source-table name for alias '{alias}' in TABLES clause."`. The count needs to treat doubled `""` as escaped (mirrors `find_identifier_end`'s own escape handling at `src/ident.rs:195-198`).

**Landmine:** Naive `table_name.matches('"').count() % 2` is incorrect because `"a""b"` is balanced. Iterate bytes and skip doubled quotes (or reuse the helper logic from `src/ident.rs::find_identifier_end` by reading its in_quotes terminal state — but the helper does not expose this). A minimal new private helper `is_quoting_balanced(s: &str) -> bool` in `src/body_parser.rs` is cleanest.

**Confidence:** HIGH.

### A5 — Mixed bare/quoted dot-qualified fixture row

**Site:** `test/sql/phase67_quoted_source_tables.test` (4 scenarios + cleanup; current at 157 lines)

**Current coverage:** Scenario 1 fully-bare quoted name (`"my orders"`), Scenario 2 fully-quoted 3-part FQN (`"stage 2"."my orders"`), Scenario 3 quoted with embedded `PRIMARY KEY`, Scenario 4 plain unquoted. No coverage of mixed bare/quoted like `staging."my orders"` or `"my db".sch.t`.

**Fix scope (D-06):** Add Scenario 5 — a new fixture block with a mixed-quoting source-table name. Pattern from REVIEW.md IN-02 suggests `o AS staging."my orders" PRIMARY KEY (id)`. Requires `CREATE SCHEMA staging;` + `CREATE TABLE staging."my orders" (...)` setup and a corresponding `DROP` in cleanup. A matching Rust unit test (`test_parse_single_table_entry_mixed_quoted_and_bare`) also wanted per REVIEW.md but not explicitly in D-06 — recommend including it to mirror Scenarios 1-4 which each have a sibling Rust test.

**Confidence:** HIGH.

### A6 — Default-schema cleanup

**Site:** `test/sql/phase67_quoted_source_tables.test:139-156`

**Current cleanup (verified):** Drops the 4 semantic views and `DROP SCHEMA "stage 2" CASCADE` — but `"my orders"` (in main), `"weird PRIMARY KEY name"` (in main), and `p67_plain_orders` (in main) are NOT dropped.

**Fix scope (D-06):** Add three `DROP TABLE IF EXISTS` at the bottom of the cleanup block. If A5 adds a `staging."my orders"` table, also drop that table or `DROP SCHEMA staging CASCADE`.

**Confidence:** HIGH.

### A7 — `find_primary_key` word-boundary alignment with `find_unique`

**Site:** `src/body_parser.rs:875-905` vs `815-834`

**Current divergence (verified):**

- `find_unique` (line 823): `before_ok = !bytes[i-1].is_ascii_alphanumeric() && bytes[i-1] != b'_'`
- `find_unique` (line 825): `after_ok = !bytes[i+kw_len].is_ascii_alphanumeric() && bytes[i+kw_len] != b'_'`
- `find_primary_key` PRIMARY before-check (line 881): `before_ok = i == 0 || !bytes[i-1].is_ascii_alphanumeric()` — **no `_` exclusion**
- `find_primary_key` PRIMARY after-check (line 884): `!bytes[after_primary].is_ascii_alphanumeric()` — **no `_` exclusion**
- `find_primary_key` KEY after-check (line 894-895): `!bytes[after_key].is_ascii_alphanumeric()` — **no `_` exclusion**

**Fix scope:** Three boundary checks to align. All three should match `find_unique`'s pattern: `c != b'_' && !c.is_ascii_alphanumeric()`. No behavioral change is expected on the existing fixture surface (per REVIEW.md IN-04 rationale: post-table-name slice rarely has alphanumeric/underscore prefixes), but the alignment removes a gratuitous divergence.

**Test:** No new test required to assert behavior (no observable change). A Rust unit test asserting `find_primary_key("my_PRIMARY KEY", ...)` returns `None` would pin the alignment, but is optional.

**Confidence:** HIGH.

### B1 — `parse_non_additive_dims` split_whitespace

**Site:** `src/body_parser.rs:1402-1490`

**Call site (verified at line 1946):** `parse_non_additive_dims(paren_content, entry_offset + na_start + 17)` — `paren_content` is the raw `&str` content extracted from the outermost `(...)` of `NON ADDITIVE BY (...)`. Already comma-split via `split_at_depth0_commas(content)` at line 1408.

**Current shape (verified at lines 1410-1488):** Each entry passes through `entry_text.split_whitespace().collect::<Vec<&str>>()`. `parts[0]` is the dim name; `parts[1..]` are ASC/DESC/NULLS FIRST|LAST modifiers walked by index in a `while i < upper_parts.len()` loop.

**Landmine — RESOLVED:** The call site shape supports a (b)-class port directly. `parse_non_additive_dims` receives raw `&str` entries; no call-stack lifting is needed. The port pattern is:

1. Walk the dim-name via `find_identifier_end(entry_text, /* allow_paren = */ false)` + dot-rejoin loop (same shape as `parse_single_table_entry`, post-A3-collapse). Set `allow_paren = false` because there are no parens inside NAB entries.
2. Capture `dim_name = entry_text[..name_end]`. Trim trailing whitespace.
3. Tokenise `entry_text[name_end..]` modifier suffix via `split_whitespace()` — the suffix has no quoted identifiers, only ASC/DESC/NULLS/FIRST/LAST keywords.
4. Feed modifier suffix tokens into the existing `while i < upper_parts.len()` loop (no change to that loop logic).

**Net change:** ~15-25 LOC swap in `parse_non_additive_dims`. Well within the "if balloons past ~150 LOC + tests" threshold from CONTEXT.md `<risks>`.

**D-08 verification (dotted-path support):** No existing sqllogictest fixture uses `table.col` qualification in `NON ADDITIVE BY` — all existing fixtures (`phase47_semi_additive.test`, `phase55_materialization_routing.test`, `phase67_qualified_emission.test`) use bare names like `report_date DESC` and `order_date DESC`. D-08 is a contract **extension**, not a regression preservation. No existing test will fail; new fixture (D-09) covers the dotted-path case.

**Confidence:** HIGH.

### B2 — `parse_window_spec` OVER ORDER BY split_whitespace

**Site:** `src/body_parser.rs:1705-1799` (focus 1731)

**Call site (verified at lines 1705-1799):** Inside the OVER `(... ORDER BY ...)` parser. `order_text` is derived from `after_order_by` (post-`ORDER BY` slice up to the frame clause), then run through `split_at_depth0_commas(order_text)`. Each entry passes through `entry_text.split_whitespace().collect::<Vec<&str>>()`. `parts[0]` is the expr; `parts[1..]` are ASC/DESC/NULLS modifiers.

**Identical bug class, identical fix shape.** Same loop refactor as B1.

**D-08 verification:** Same as B1 — existing fixtures all use bare column names (`date ASC NULLS LAST`, `sale_date ASC NULLS LAST`, `order_date ASC NULLS LAST`). Dotted-path support is a contract extension.

**Subtlety:** The OVER ORDER BY entry stores an `expr` (not strictly an identifier) in `WindowOrderBy::expr` — current code stores the bare `parts[0]` string as `dim_name`. Post-port, `find_identifier_end` captures an identifier-shaped slice. Window ORDER BY commonly takes column expressions (e.g., `date_trunc('day', order_date) ASC`); SCOPE/CONTEXT treats this as a column reference site per D-08's "OVER ORDER BY column refs" wording. **Confirm with the planner**: if expression-shaped ORDER BY (with function calls) was ever supported here, the port narrows that to identifier-shaped — verify against existing fixtures and `WindowOrderBy::expr` consumers. Spot-check shows all existing fixtures use bare identifiers, so this is theoretical, but flag it.

**Confidence:** HIGH for the bug fix; MEDIUM for the "expression vs identifier in ORDER BY" subtlety — addressable as one open question (see §Open Questions).

### C1 — UTF-8 char boundary panic in error formatter

**Site:** `tests/registration_error_surfaces.rs:135`

**Current code (verified):** `&body[..body.len().min(400)]` — slices by byte, can panic on a multi-byte UTF-8 codepoint boundary. The body is C++ source read from `cpp/src/shim.cpp`, which is ASCII in practice today but the assertion path runs only on failure (so the panic-on-format makes diagnosis worse, not better).

**Fix (D-11):** Swap to `body.get(..400).unwrap_or(body)`. This returns `Some(&str)` only if the boundary is valid, else falls back to the full body. One-line swap.

**Confidence:** HIGH.

### C2 — Transmute needle missing turbofish form

**Site:** `tests/registration_error_surfaces.rs:160-166`

**Current code (verified):** `let parts = ["std::", "mem::", "transmute("];` concatenated to `"std::mem::transmute("`. This catches the bare form `std::mem::transmute(...)` but misses turbofish `std::mem::transmute::<T, U>(...)` because the colon-colon comes between `transmute` and the opening paren.

**Fix (D-12):** Drop the `(`: `let parts = ["std::", "mem::", "transmute"];` → needle becomes `"std::mem::transmute"`. Both forms match. Add a one-line comment explaining: `// Needle matches both bare std::mem::transmute(...) and turbofish std::mem::transmute::<T, U>(...) forms — turbofish is the real footgun this guards against.`

**Subtle:** The needle is constructed at runtime specifically to keep the plan-checker's `grep -q "std" + "::mem::transmute"` finding the literal token sequence (see comment at lines 50-55 in the file). Verify the looser needle still satisfies the plan-checker invariant. The plan-checker needle has the form `std" + "::mem::transmute` (verified at line 51 of the file's docstring), which the new tokens `["std::", "mem::", "transmute"]` still concatenate into `"std::mem::transmute"` — substring match preserved.

**Confidence:** HIGH.

### C3 — Unused checked-in fixture

**Site:** `test/sql/p651_ok.yaml` (15 lines, verified content)

**Verification of unused status:** `grep -rn "p651_ok"` finds 3 matches in `test/sql/phase651_yaml_filesystem_access_gating.test` — all reference the **runtime-generated** `'__TEST_DIR__/p651_ok.yaml'` path (written by `COPY (SELECT 'base_table: t1...' AS content) TO '__TEST_DIR__/p651_ok.yaml'` at line 41-55, then read at lines 64 and 126). The checked-in `test/sql/p651_ok.yaml` is never referenced. Confirmed via grep across `test/` and `src/`.

**Why D-13's "delete, not rewrite" is correct:** The test's contract is that the FROM YAML FILE path goes through `LocalFileSystem` at runtime. Rewriting the test to load the checked-in fixture would either (a) require pre-existing-on-disk files (defeats the `enable_external_access` gate test) or (b) substitute one filesystem read for another (functionally identical). Either way, the runtime `COPY TO ...` setup is the actual gate contract. Delete is correct.

**Fix:** `git rm test/sql/p651_ok.yaml`. No other changes needed.

**Confidence:** HIGH.

## B1/B2 Deep-Dive: Call-Site Shape Question

**Question raised in CONTEXT.md `<risks>`:** "If `parse_non_additive_dims` operates on already-tokenised input (rather than a raw slice), porting `find_identifier_end` requires lifting tokenisation up the call stack."

**Resolution:** Both functions receive **raw `&str` content** (post-paren-extraction but pre-tokenisation). The tokenisation is internal to each function. The port stays local to each function body.

- B1 call site (`src/body_parser.rs:1946`): `parse_non_additive_dims(paren_content, base_offset)` where `paren_content` is from `extract_paren_content(after_na)`.
- B2 site (`src/body_parser.rs:1725`): `let entries = split_at_depth0_commas(order_text);` — `order_text` is a raw `&str` slice of the post-`ORDER BY` content.

**`find_identifier_end` signature (verified at `src/ident.rs:186`):**

```rust
pub fn find_identifier_end(input: &str, allow_paren: bool) -> usize
```

- `allow_paren = true` for sites that terminate at `(` (e.g., function calls — `parse_single_table_entry` uses this for the `UNIQUE (` terminator).
- `allow_paren = false` for sites where `(` should not terminate — recommend `false` for B1/B2 since NAB entries and ORDER BY entries don't have parens inside the identifier slot.

Returns `input.len()` on unterminated quote (saturating). Callers must surface this as an error (same gap as A4 in the TABLES clause). **Plan-phase note:** B1/B2 should add the same balanced-quote check as A4, OR rely on `find_identifier_end` returning the full length and let the downstream "unknown modifier token" error surface — but the latter gives a worse error message. Recommend symmetric balanced-quote check.

**D-08 dot-walk contract:** `find_identifier_end` walks across dots that fall outside quoted regions — verified by `src/ident.rs::find_identifier_end_tests::fqn_with_quoted_parts_runs_to_whitespace`. So `"my db"."schema"."col"` is captured in one call. The dot-rejoin `loop { ... if name_end < after.len() && bytes[name_end] == b'.' { ... continue } break }` exists in `parse_single_table_entry` as defensive belt-and-braces — A3 marks it as unreachable. For B1/B2, the planner should NOT replicate the dead loop arm; one `find_identifier_end` call is sufficient.

**Shared helper opportunity (Claude's Discretion):** B1 + B2 + the post-A3 `parse_single_table_entry` all share the pattern `find_identifier_end(s, false) + balanced-quote check`. A 5-10 LOC private helper `fn capture_identifier(s: &str) -> Result<usize, String>` would deduplicate three sites. Plan-phase: include as a sub-task in 68-03, or defer to a future refactor. Either is defensible.

## Test Coverage Map

| Item | Test Type | Test Location | New / Extend |
|------|-----------|---------------|--------------|
| A1 | Rust unit | `src/body_parser.rs::tests::test_parse_single_table_entry_reserved_keyword_after_as` (5 variants: PRIMARY, UNIQUE, FOREIGN, REFERENCES, NOT) | New |
| A1 | sqllogictest | `test/sql/phase67_quoted_source_tables.test` — add `statement error` block asserting the literal pre-fix message | New row in existing fixture |
| A3 | (None) | Bundled with A1; behavioral coverage via A1's tests | — |
| A4 | Rust unit | `src/body_parser.rs::tests::test_parse_single_table_entry_unterminated_quote` | New |
| A5 | Rust unit | `src/body_parser.rs::tests::test_parse_single_table_entry_mixed_quoted_and_bare` (recommended per REVIEW.md) | New |
| A5 | sqllogictest | `test/sql/phase67_quoted_source_tables.test` — Scenario 5 mixed quoting | Extend |
| A6 | (None) | Cleanup hygiene — no new test, just `DROP TABLE IF EXISTS` rows | — |
| A7 | Rust unit (optional) | `src/body_parser.rs::tests::test_find_primary_key_word_boundary_underscore` | Optional |
| B1 | Rust unit | `src/body_parser.rs::tests::test_parse_non_additive_dims_quoted_identifier_with_whitespace` + `test_parse_non_additive_dims_dotted_path` | New |
| B1 | sqllogictest | New file `test/sql/phase68_quoted_idents_non_additive.test` (D-09), wired in `TEST_LIST` | **New file** |
| B2 | Rust unit | `src/body_parser.rs::tests::test_parse_window_spec_quoted_order_by` + `test_parse_window_spec_dotted_order_by` | New |
| B2 | sqllogictest | New file `test/sql/phase68_quoted_idents_window.test` (D-09), wired in `TEST_LIST` | **New file** |
| C1 | (None) | Behavioral test infeasible (panic path only fires on assertion failure); fix is a one-line API swap, code-review covers | — |
| C2 | (None) | Same — needle change is verified by inspection; the test continues to pass | — |
| C3 | (None) | Deletion only; `just test-sql` confirms no fixture references the deleted file | — |

**Quality gate:** All new sqllogictest files MUST be registered in `test/sql/TEST_LIST` per CLAUDE.md. The plan must include this as an explicit task — Phase 63 Plan 02 documented this exact rule as a hard gate.

## Validation Architecture

**Test Framework:**

| Property | Value |
|----------|-------|
| Rust unit | cargo test (workspace default) |
| Property tests | proptest crate (existing) — not exercised by Phase 68 fixes |
| sqllogictest | sqllogictest-bin runner via `just test-sql` (requires `just build` first to materialize the loadable extension) |
| Integration | pytest (existing) — `test_adbc_queries.py` for A2 |
| Quick run command | `cargo test --lib` for hot loop on body_parser changes; `cargo test --test registration_error_surfaces` for C1/C2 |
| Full suite | `just test-all` |

**Item → Test Map:**

| Item | Behavior | Test Type | Automated Command | File Exists? |
|------|----------|-----------|-------------------|-------------|
| A1 | Bare reserved keyword after AS surfaces structured error | unit | `cargo test --lib test_parse_single_table_entry_reserved_keyword_after_as` | ❌ Wave 0 |
| A1 | Same, end-to-end via DDL | sqllogictest | `just test-sql phase67_quoted_source_tables` | Extend existing |
| A3 | Dead loop arm removed — happy path unchanged | (covered by existing) | `cargo test --lib body_parser::tests` (existing TECH-DEBT #24 tests) | ✅ |
| A4 | Unterminated quoted identifier rejected at parse time | unit | `cargo test --lib test_parse_single_table_entry_unterminated_quote` | ❌ Wave 0 |
| A5 | Mixed bare/quoted dot-qualified name parses correctly | unit + sqllogictest | `cargo test --lib test_parse_single_table_entry_mixed_quoted_and_bare` + `just test-sql phase67_quoted_source_tables` | ❌ Wave 0 (extend existing file) |
| A6 | Fixture cleanup is complete | (none — hygiene) | `just test-sql phase67_quoted_source_tables` (must still pass) | ✅ |
| A7 | `find_primary_key` word-boundary matches `find_unique` | unit (optional) | `cargo test --lib find_primary_key_word_boundary` | ❌ Optional |
| B1 | NAB clause accepts quoted identifier with internal whitespace | unit + sqllogictest | `cargo test --lib test_parse_non_additive_dims_quoted_identifier_with_whitespace` + `just test-sql phase68_quoted_idents_non_additive` | ❌ Wave 0 (new file) |
| B1 | NAB clause accepts dotted path | unit + sqllogictest | (same) | ❌ Wave 0 |
| B2 | OVER ORDER BY accepts quoted identifier with internal whitespace | unit + sqllogictest | `cargo test --lib test_parse_window_spec_quoted_order_by` + `just test-sql phase68_quoted_idents_window` | ❌ Wave 0 (new file) |
| B2 | OVER ORDER BY accepts dotted path | unit + sqllogictest | (same) | ❌ Wave 0 |
| C1 | UTF-8 char boundary safe in error formatter | (none — code-review verified) | `cargo test --test registration_error_surfaces` (existing — must still pass) | ✅ |
| C2 | Transmute needle catches both bare + turbofish | (none — needle assertion stays green on absence of any transmute) | `cargo test --test registration_error_surfaces` | ✅ |
| C3 | Dead fixture deleted, test still passes | (deletion only) | `just test-sql phase651_yaml_filesystem_access_gating` | ✅ |

**Sampling Rate:**

- **Per task commit:** `cargo test --lib` (~30s on warm cache); for B1/B2 add `just test-sql <new_fixture>` once the new fixture lands.
- **Per wave merge:** `just test-all` (full suite — ~3-5 min on warm cache).
- **Phase gate:** `just ci` green (adds clippy + fmt + cargo-deny + fuzz target compile) before push to main.

**Wave 0 Gaps:**

- [ ] `test/sql/phase68_quoted_idents_non_additive.test` — new fixture (B1, D-09)
- [ ] `test/sql/phase68_quoted_idents_window.test` — new fixture (B2, D-09)
- [ ] `test/sql/TEST_LIST` — register the two new fixtures (CLAUDE.md hard rule; runner skips files not in TEST_LIST)
- [ ] New Rust unit tests in `src/body_parser.rs::tests` — 5+ new tests covering A1, A4, A5, B1, B2
- [ ] Optional new sibling Rust test for A7 alignment

## Planning Landmines

1. **D-02 commit ordering for A1+A3.** A3 deletes the loop where A1's guard belongs. Land A3's collapse first (in-place edit replacing lines 692-709 with a single `find_identifier_end` call), then A1's guard at the clean site immediately after. A code reviewer should see the diff as one atomic change. The risk if reversed: A1's guard sits inside the dead loop body, then has to be relocated when A3 collapses the loop — extra churn for no benefit.

2. **D-03 keyword list ≠ REVIEW.md draft.** CONTEXT.md D-03 specifies `PRIMARY|UNIQUE|FOREIGN|REFERENCES|NOT`. REVIEW.md WR-01 fix sketch uses `PRIMARY|UNIQUE|COMMENT|WITH`. The CONTEXT.md list is authoritative. The plan-checker should grep the implementation to confirm the literal CONTEXT.md keyword set is what landed.

3. **TEST_LIST registration is a hard gate.** Two new sqllogictest files (B1 + B2 fixtures) require explicit `test/sql/TEST_LIST` entries. The runner silently skips files not listed — a green test run with no `phase68_quoted_idents_*` lines in output is a false-positive. Plan must include this as an explicit task, ideally co-located with the fixture creation step.

4. **A4 balanced-quote check shape.** A naive `count('"') % 2` is wrong because `find_identifier_end` already handles doubled-quote escapes (`""` inside `"..."`). The balanced-quote helper must mirror that escape rule. Recommend a private helper in `src/body_parser.rs` (5-10 LOC) rather than copy-paste.

5. **B1/B2 share an internal pattern with A1/A3/A4.** The post-A3 capture loop in `parse_single_table_entry`, plus B1's and B2's identical patterns, total three sites with the same `find_identifier_end + balanced-quote + dot-rejoin` shape. A small private helper `capture_identifier(s: &str) -> Result<(&str, &str), ParseError>` (slice + remainder) would dedupe. CONTEXT.md does not require this; Claude's discretion. **If the planner introduces a shared helper, do it in 68-03 (after 68-01 has landed the in-place A1/A3/A4 shape that informs the helper signature).**

6. **C3 deletion verification.** Per CONTEXT.md `<risks>` — quick `grep -rn "p651_ok.yaml"` before deletion confirms no other test references the static file. **Already verified in this research:** 3 matches, all in `phase651_yaml_filesystem_access_gating.test`, all referencing the runtime `__TEST_DIR__/...` path. Safe to delete.

7. **A2 is a one-line escape.** The plan should not over-engineer this. CONTEXT.md D-04 explicitly cites the line 100 pattern (`extension_path.replace("'", "''")`); replicate it at line 470 for `other_db_path`. Extracting a `_quote_sql_literal(s)` helper is REVIEW.md's optional suggestion — defensible if the planner wants the parity to be visible, but not required by D-04.

8. **Plan 68-03 dependency on Plan 68-01 is real but soft.** CONTEXT.md sequences 68-03 after 68-01 because B1/B2 "should reference" the A1 keyword-guard pattern. In practice, B1/B2 do not need a reserved-keyword guard (the modifier loop's "unexpected token" error path already catches `PRIMARY|UNIQUE` etc. landing as the first token). The dependency exists to **maintain consistency** in the parser, not because of a hard code coupling. If timeline pressure surfaces, 68-03 could land independently — but per CONTEXT.md's sequencing the planner should keep the 68-01 → 68-03 dependency.

## Open Questions for Planner

1. **B2 expression-shaped ORDER BY entries.** SCOPE/CONTEXT describe OVER ORDER BY as a column-reference slot per D-08. Spot-check of existing fixtures shows all entries use bare column names. If the planner finds a test or REQUIREMENTS.md entry permitting expression-shaped ORDER BY (e.g., `date_trunc('day', order_date) ASC`), the B2 port restricts that — surface as a finding, narrowing the contract is a behaviour change. Investigation budget: 2 minutes (grep for `OVER (` in tests + scan `WindowOrderBy::expr` consumers in `src/`). **Likely a non-issue** but worth a sanity check.

2. **A7 unit test inclusion.** REVIEW.md IN-04 does not require a regression test (no observable behaviour change today). CONTEXT.md D-05 calls A7 a "surgical body-parser edit." Planner's call: include a `test_find_primary_key_word_boundary_underscore` unit test for documentation value, or skip it. Either is defensible.

3. **Shared `capture_identifier` helper.** Optional refactor opportunity discussed in landmine 5 above. Planner can fold into 68-03 or defer to a future hygiene phase.

## Sources

### Primary (HIGH confidence)

- `src/body_parser.rs` (working tree, milestone/v0.10.0) — direct code verification at lines 670-720, 815-905, 1402-1490, 1700-1800, 1930-1950, 2580-2620.
- `src/ident.rs:180-220` — `find_identifier_end` signature, delimiter set, escape handling.
- `tests/registration_error_surfaces.rs:1-178` — full C1/C2 context.
- `test/sql/phase67_quoted_source_tables.test` — full fixture content.
- `test/sql/phase651_yaml_filesystem_access_gating.test:40-130` — C3 verification (runtime COPY TO __TEST_DIR__ path).
- `test/integration/test_adbc_queries.py:90-110, 460-485` — A2 site.
- `test/sql/p651_ok.yaml` — confirmed 15-line static fixture, never loaded.
- `.planning/phases/68-pre-tag-cleanup-phase-67-review-pr-35-code-review-follow-ups/68-CONTEXT.md` — locked decisions D-01..D-13.
- `.planning/phases/68-pre-tag-cleanup-phase-67-review-pr-35-code-review-follow-ups/SCOPE.md` — inventory.
- `.planning/phases/67-expansion-sql-coverage-and-tech-debt-cleanup/67-REVIEW.md` — A1..A7 origin.
- `TECH-DEBT.md` item #25 — B1/B2 origin and (c)-class classification (now downgraded per D-07).
- `CLAUDE.md` — test infrastructure rules (TEST_LIST, statement error block form, build/test command rules).

## Metadata

**Confidence breakdown:**

- Code-site verification: HIGH — all 11 items verified directly against working tree.
- B1/B2 (c)-class → (b)-class re-classification: HIGH — call-site shape inspection confirms raw `&str` content; no call-stack lifting needed.
- D-08 dotted-path contract extension: HIGH — no existing fixture uses dotted paths in NAB or OVER ORDER BY, so the contract is purely additive.
- C3 deletion safety: HIGH — grep confirms no static-file reference.

**Research date:** 2026-05-27
**Valid until:** Phase 68 completion (no external dependencies; code locations may drift if work begins on `milestone/v0.10.0` before phase starts — re-verify line numbers at task start).

## RESEARCH COMPLETE
