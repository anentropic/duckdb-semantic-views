# Code Review — 2026-07-16

**Scope:** Full codebase review at v0.10.4 (commit `a9c3d59`, ~40.7k lines of Rust in `src/` plus the C++ shim), on branch `claude/codebase-review-coverage-8k07s0`. Focus per request: (a) test coverage of edge cases, particularly around the SQL parser; (b) correctness of behaviour w.r.t. Snowflake semantic views SQL functionality, adapted to DuckDB conventions; (c) whether the code — after many iterations and refactors — meets expert Rust/C++ staff-engineer quality.

**Method:** Four focused review passes (parser internals; parser/output test coverage; Snowflake conformance; engineering quality across expansion/graph/catalog/DDL/FFI/C++), each instructed to read TECH-DEBT.md and `_notes/code-review-2026-07-11.md` first and to report the fix status of every prior finding rather than re-reporting it. Findings marked **[verified]** were confirmed empirically: parser findings via a temporary probe test harness run against the actual `parse_keyword_body`/`plan_rewrite` entry points (deleted after the run); Snowflake claims against live docs.snowflake.com fetches (the headline finding was independently re-fetched and confirmed twice); DuckDB-convention claims against Python duckdb 1.5.4. The full `cargo test` suite was run on this snapshot: **1,293 tests, 0 failures**.

**Prior review:** `_notes/code-review-2026-07-11.md`. Since then, 46 remediation commits landed (PRs #71–#114): the §6.1 lexer/cursor migration (phases 1–8), the §6.2 expansion-layer consolidation (`SelectSpec`, `JoinTree`, one reference-scan engine, one materialization matcher), structured parse errors, `SqlLit`, the dispatcher/registration consolidations, and the T-1…T-9 test gaps. This review's first job was to audit what actually stuck.

This document is a review, not a remediation plan — no fixes have been applied.

---

## 1. Executive summary

**The 2026-07-11 remediation genuinely stuck.** Of the ~35 findings in the prior review, all but a handful are FIXED to the standard that review's own meta-lesson demanded ("all sites migrated, old form deleted") — verified at current file:line, not from commit messages. E-1 (the silent-wrong-results semi-additive alias bug) is fixed with regression tests at three layers, including a differential-harness section with engineered ties; the parser has a real lexer/token cursor and the eight pre-migration keyword scanners are deleted, not bypassed; `SqlLit` makes missed-escaping a compile error; the half-migration table from §3.4 is fully closed (17/17 dispatchers, C++ registration consolidated, bespoke list parser folded). The abstractions built are structural, not conventional — the bug classes they target are now mostly unrepresentable.

Against that backdrop, this review found **one HIGH conformance bug (silent wrong numbers for ported Snowflake DDL)**, **one recurrence of the parser's historical silent-discard disease in the exact spots the lexer migration didn't reach**, and a short tail of coverage/doc/hygiene residue.

| # | Cluster | Worst consequence | Where |
|---|---------|-------------------|-------|
| 1 | **Semi-additive `NON ADDITIVE BY` polarity is inverted relative to Snowflake** — [verified against live Snowflake docs, twice] | Any Snowflake DDL ported unchanged silently returns the opposite-end snapshot (earliest instead of latest); Snowflake's own account-balance example returns 210 there, 100 here | `src/expand/semi_additive.rs:291-296`, `docs/how-to/semi-additive-metrics.rst:73-74` (F-1) |
| 2 | **Search-anywhere-then-slice-backwards sub-parsers still silently discard text** — the P-1/P-3 disease class, alive one keyword to the left of where it was fixed | `OVER (ORDER BY d PARTITION BY d)` accepted with the ORDER BY destroyed → window metric silently computes an unordered aggregate; junk swallowed around `USING (...)`/`NON ADDITIVE BY (...)`; MATERIALIZATIONS duplicate `TABLE` last-wins — all [verified by probe] | `src/body_parser/window.rs:186-213`, `metrics.rs:184-256`, `materializations.rs:121-201` (F-2, F-3, F-4) |
| 3 | **Snowflake-portability edge**: several valid Snowflake DDL forms are rejected (trailing `COMMENT`, alias-less TABLES entry, `PUBLIC` on dims, `WITH SYNONYMS` without `=`, `DESC` abbreviation), with errors that don't say why | Copy-paste porting friction, misleading diagnostics; README documents removed cardinality syntax that the parser now rejects | `src/parse/create_body.rs`, `src/body_parser/tables.rs`, README.md:189-198 (F-5…F-9) |
| 4 | **Coverage residue**: the differential oracle still has no role-playing/USING section; doctests (including the load-bearing `compile_fail` FFI guard) run in no CI job; interior empty entries (`a,,b`) are silently dropped against the splitter's own doc | The remaining features pinned only by small fixtures are the alias-binding-under-joins family that produced E-1 | `test/integration/test_differential.py`, Justfile:37, `src/body_parser/scan.rs:105-134` (T-10…T-13) |

**Direct answers to the three questions asked:**

**(a) Parser edge-case test coverage.** Strong, and dramatically improved since 2026-07-11: every prior T-gap closed (duplicate clauses with caret asserts, empty bodies, in-body comments, hostile-identifier proptests through the `plan_rewrite` front door, exhaustive 2-clause order inversions, 500-entry/64-deep stress tests, a randomized Rust-side differential proptest). The classics are all covered: doubled-quote escapes, keywords-as-identifiers, nested block comments, tagged/untagged dollar quotes, unicode identifiers with exact caret columns, CRLF/tabs generatively. TEST_LIST is clean (72/72, guarded twice). The residual holes are narrow and enumerated in §4 — the notable ones are the missing differential section for role-playing `USING`, doctests absent from CI, and the fact that every fresh parser bug found by this review (F-2…F-4) lives in a slot no test constrains: the *interiors* of OVER/USING/NAB/MATERIALIZATIONS sub-clauses, where "residue between captured regions" is never asserted empty.

**(b) Snowflake conformance under DuckDB conventions.** The DDL grammar, clause ordering, entry annotations, FACTS/METRICS mutual exclusion, qualified-only wildcards, window-metric required-dims rule, DESCRIBE's 5-column property model, and SHOW's 8-column entity listings all track Snowflake closely; deviations forced by DuckDB (table-function query surface, no owner columns, explicit PRIMARY KEY, uniform case-insensitivity) are deliberate, well-chosen, and mostly well-documented — the identifier-case adaptation in particular is thorough and empirically matches DuckDB's rule. Two things genuinely violate the project's own "Snowflake is the reference" rule: the semi-additive polarity inversion (F-1, silent wrong numbers) and a set of doc errors that misrepresent Snowflake's own syntax on the comparison page (§5). The pre-aggregation in-clause `WHERE` — a real Snowflake feature with no equivalent here — is missing from the documented not-supported list.

**(c) Staff-engineer quality.** **Yes — grade A, up from the prior review's A–.** The evidence is not just the code but the trajectory: a 240-line adversarial review was closed out finding-by-finding, each fix carrying a file:line-citing comment back to its finding ID and a regression test at the right layer. The unsafe/FFI seam would pass a hostile staff review (RAII buffer ownership, catch_unwind at every `extern "C"` boundary via one dispatcher, compile-time + runtime ABI drift guards, an AST-walking CI test enforcing the connection borrow model, zero statics); zero TODO/FIXME/HACK in `src/` + `cpp/`; ~12 guarded non-test unwraps in 40k lines; CI hygiene is exemplary (pinned actions with dated rationales, TEST_LIST sync gate, 80% coverage floor, fuzz targets actually run 10 min/push with auto-filed crash issues). What separates it from unqualified staff level is a short list: the typed-error rollout stops at the graph/validation layer (~15 `Result<_, String>` signatures with no positions), TECH-DEBT.md has drifted measurably behind three weeks of remediation velocity, the reference-scanning engine is consolidated but still not quote-aware (TECH-DEBT #28), and a small sediment of residue (`extract_ddl_name`'s dead duplicate grammar, one `''`-escape copy outside `SqlLit`, the 13 unreachable `fk_columns.is_empty()` guards).

---

## 2. Confirmed bugs (fix first)

### F-1 — HIGH [verified]: `NON ADDITIVE BY` snapshot polarity is inverted relative to Snowflake

Snowflake (user-guide/views-semantic/sql, fetched live and re-confirmed): *"the rows are sorted by the non-additive dimensions, and the values from the **last** rows (the latest snapshots of values) are aggregated"* — default ASC, and its account-balance example (`NON ADDITIVE BY (year_dim, month_dim, day_dim)`, no DESC) returns the **latest** snapshot (210 for cust-001/2024); DESC is documented as selecting the earliest.

This project emits `RANK() OVER (... ORDER BY <NA dims in declared order>)` and aggregates rows at rank 1 (`src/expand/semi_additive.rs:291-296`) — **first**-row-wins. Its own docs state the inversion plainly: *"ASC (default) — selects the earliest snapshot row; DESC — selects the latest"* (`docs/how-to/semi-additive-metrics.rst:73-74`). The docs' own examples reach "latest" only by writing `report_date DESC` — exactly the opposite annotation from what the same intent needs in Snowflake.

Concrete: `accounts(date, balance)` rows `('2024-01-01', 100), ('2024-03-30', 210)`; DDL `a.bal NON ADDITIVE BY (date_dim) AS SUM(a.balance)`. Snowflake: 210. Here: 100. No error — every ported DDL silently returns opposite-end snapshots, and `snowflake-comparison.rst:322-344` claims the syntax "is aligned" without flagging the semantic inversion. Under CLAUDE.md's "refer to what Snowflake does" rule this is a correctness bug, not a documented deviation.

**Fix:** select the *last*-ranked row per declared order (equivalently: invert each sort key's direction when emitting the RANK `ORDER BY`), update the how-to, the comparison page, and every fixture that compensated with DESC. This is a behavioural break for existing users — worth a loud CHANGELOG entry and possibly a transition note.

### F-2 — MED-HIGH [verified by probe]: OVER-clause content before `PARTITION BY` is silently discarded — including a misplaced `ORDER BY`

`src/body_parser/window.rs:186-213`: `find_kw_seq(&["PARTITION","BY"])` searches anywhere in the OVER body and nothing validates the region before its match. Probe through `parse_keyword_body` (and the `plan_rewrite` front door): `o.w AS AVG(m1) OVER (ORDER BY d PARTITION BY d)` → **accepted**, `partition_dims=["d"]`, `order_by=[]` — the ORDER BY is destroyed, so the window metric silently computes an unordered aggregate where DuckDB and Snowflake would reject the clause order. `OVER (banana PARTITION BY d)` is likewise accepted.

The bitter detail (same shape as E-1's history): the P-3 fix added exactly this validation *after* PARTITION BY — `window.rs:217-226` errors on stray text between the dims and ORDER — but the region *before* PARTITION BY never got the same check. **Fix:** when `PARTITION BY` is present, require its match to start at offset 0 of the OVER content (≈3 lines + tests).

### F-3 — MED [verified by probe]: junk between `USING (...)` / `NON ADDITIVE BY (...)` and `AS` in a METRICS entry is silently discarded

`src/body_parser/metrics.rs:184-213` (NAB) and `:229-256` (USING) both peel their region with `before_as[..kw_tok.start]` after `take_parens()` and never inspect the cursor's residue. Probes: `o.m USING (r) junk AS SUM(o.v)` and `o.m NON ADDITIVE BY (d) junk AS SUM(o.v)` → both accepted, junk gone, through the full front door. The migration's own doc comment ("text between the name and the constraint is a visible unexpected token") is violated one region to the right. **Fix:** after each `take_parens()`, error if the remaining region is non-empty/unexpected.

### F-4 — MED [verified by probe]: MATERIALIZATIONS sub-body tolerates junk and duplicate sub-clauses (last-wins)

`src/body_parser/materializations.rs:121-153` + `extract_paren_list` (`:181-201`). Probes: `mm AS (junk TABLE t, DIMENSIONS (d))` → accepted; `mm AS (TABLE t, TABLE u, DIMENSIONS (d))` → accepted with `table="u"` (silent overwrite — the P-2 duplicate class); `mm AS (TABLE t, DIMENSIONS (d) junk)` → accepted, junk dropped. **Fix:** reject non-empty region before the first sub-keyword; error on repeated sub-keywords; assert no residue after a consumed `(...)` group.

*(F-2/F-3/F-4 are one disease: the lexer/cursor migration made search-then-slice quote-safe but not discard-safe. A single "residue must be empty" sweep over every `find_kw*` + backwards-slice site closes the class — see §3.)*

### F-5 — MED (user-facing docs): README documents relationship cardinality annotations the parser rejects

README.md:189-198 still documents `ONE TO ONE` / `ONE TO MANY` / `MANY TO ONE` annotations with a `REFERENCES o MANY TO ONE` example — removed in v0.6.0 (CHANGELOG "breaking"); `src/body_parser/relationships.rs:116-126` rejects them with "Cardinality is now inferred from PK/UNIQUE constraints". Copy-pasting the README fails. README is the only file left with this content; `docs/` is correct. Same file: "DIMENSIONS, METRICS required" (README.md:229-235) contradicts the implemented (and Snowflake-correct) at-least-one rule.

### F-6 — MED: Snowflake's view-level `COMMENT` position is rejected with an unhelpful error

Snowflake puts `COMMENT = '...'` **after** the last clause; this project only accepts it between the name and `AS` (`src/parse/create_body.rs:29-74`). The Snowflake form dies as "Unknown clause keyword 'COMMENT'" (`clause_bounds.rs:127-141`) with no hint about the supported spelling. Accept the trailing position (Snowflake-compatible) or special-case the message.

### F-7 — MED: mandatory table alias breaks valid Snowflake TABLES entries with a misleading message

Snowflake: `[ <table_alias> AS ] <table_name>` — alias optional. Here `TABLES (orders PRIMARY KEY (id))` → "Expected 'AS' after table alias 'orders'" (`src/body_parser/tables.rs:68-79`). Either default the alias to the table name or detect-and-explain.

### F-8 — MED: no pre-aggregation `WHERE`; absent from the not-supported list

Snowflake's in-construct `WHERE <predicate>` filters over dims/facts **before** metric computation. The only filter here is the outer SQL WHERE — post-aggregation (`docs/reference/semantic-view-function.rst:143-152`), equivalent only when filtering on a queried dimension. "Revenue for orders shipped after X" (filter on a member not in the output) is inexpressible. Not listed in `snowflake-comparison.rst:374-392` "Features Not Yet Supported" — it should be, or a `where :=` named parameter added (a natural fit for the table-function surface).

### F-9 — LOW-MED [verified by probe]: name slots accept arbitrary multi-token runs as a single name

`src/body_parser/entries.rs:138-140`, `metrics.rs:264-265`, `relationships.rs:61`: `o.d junk AS o.x` → dimension literally named `"d junk"`; same for metric and relationship names. An unquoted multi-word name is not a legal SQL identifier; the stored component is unqueryable. Require a single (optionally dotted-contiguous) value token, as `take_source_table_name` already does.

### Smaller confirmed items

- **F-10** — `DESC SEMANTIC VIEW` not recognized: Snowflake documents `{DESCRIBE | DESC}` and DuckDB itself accepts `DESC t` (verified on 1.5.4), so both conventions support it; `src/parse/detect.rs:137-139` matches only `describe`. One-line prefix addition.
- **F-11** [verified by probe] — empty quoted identifier `""` accepted in body name/alias slots (`TABLES ("" AS orders ...)` parses); DuckDB rejects zero-length quoted identifiers and `ident.rs:123-125` rejects them for view names — body slots should match.
- **F-12** — `PUBLIC` rejected on dimensions (`src/body_parser/entries.rs:85-95`) though Snowflake's grammar allows it; `WITH SYNONYMS` hard-requires `=` (`annotations.rs:201-206`) though Snowflake makes it optional. Both break DDL porting.
- **F-13** — the Snowflake-comparison page misrepresents Snowflake's own syntax: its "Snowflake" tab shows `CREATE SEMANTIC VIEW analytics AS ...` (Snowflake has no `AS` — masking this project's own deviation), and its "equivalent SEMANTIC_VIEW clause" example (`snowflake-comparison.rst:266-271`) is not valid Snowflake syntax. Misleads porting in both directions. Also `docs/reference/create-semantic-view.rst:56-58` shows `NON ADDITIVE BY` on derived metrics, which the parser rejects (`metrics.rs:303-312`).
- **F-14** — misleading diagnostic for the most common porting mistake: `DIMENSIONS (region AS upper(o.region))` (unqualified entry name) errors with "Expected 'AS' keyword in dimension/metric entry ..." because `entries.rs:111-137` finds the first `.` *anywhere in the entry*, including inside the expression. Bound the dot search to before the first depth-0 `AS`.
- **F-15** — all 11 annotation-path errors carry `position: None` (`annotations.rs`) — carets lost in that subtree; 45 `position: None` sites remain overall (P-4 residual; several are legitimately position-free).
- **F-16** — `parse_keyword_body`'s leading-`AS` strip has no word-boundary check (`body_parser/mod.rs:86-90`; `ASTABLES(...)` would parse). Unreachable through the front door (guarded at `create_body.rs:121-125`) but the fn is `pub` and fuzz-facing; one-line `is_ident_byte` check.
- **F-17** — dollar-quoted strings inside AS-body *expressions* sit outside the lexer/QuoteState grammar (`$` is `Symbol`); a `,` or `)` inside `$$...$$` mis-splits, though all traced cases end in a confusing error rather than silent corruption (the balance pre-checks catch the residue). Documented lexer scope decision — worth a TECH-DEBT line.
- **F-18** — defensive booby trap in the fixed E-1 path: `semi_additive.rs:228` falls back to `quote_ident(&nd.dimension)` — the alias, the exact shape E-1 fixed — when an NA dim resolves to no dimension. Unreachable while CREATE-time validation holds; add a comment or make it an internal error. Relatedly, an unqueried NA dim's ORDER BY uses the raw definition expression with no role-playing scoped-alias rewrite — the semi-additive × role-playing cell is untested (SPECULATIVE).

---

## 3. Parser layer: post-migration verdict (question a, part 1)

**The §6.1 lexer/cursor migration is sound, idiomatic, and complete for clause *structure*; incomplete for clause *interiors*.** The lexer (`lexer.rs`, ~170 production lines) and cursor (`cursor.rs`, ~290) are well-designed: infallible lexing with `Unterminated` as a token kind, half-open byte spans that provably tile the input (generative proptests over hostile alphabets and arbitrary Unicode), quoted-ident-vs-keyword distinction carried in the type, carets recovered from token offsets instead of arithmetic. All eight pre-migration keyword scanners and both non-quote-aware `.find('(')` sites are **deleted**, not bypassed — the prior review's done-criterion was honored. Prior parser findings P-1, P-2, P-3, P-5, P-6, P-7, P-8, P-9, P-13, P-14 are all FIXED and probe-verified; P-4 is partial (45 `position: None` residue), P-11 is fixed except the four SHOW name slots (`show_clauses.rs:113,152,226` — catalogued in TECH-DEBT #25, whose list is now stale on three of five entries), P-12 (cosmetic boundary inconsistencies) remains open.

The residual weakness is a *pattern*, not a component: sub-parsers that use `find_kw*`-anywhere + backwards slicing (`before_as[..tok.start]`) instead of forward consumption — and every fresh silent-discard finding (F-2, F-3, F-4) lives in exactly those spots. The cursor made search-then-slice quote-safe but not discard-safe. One follow-up PR — "after every `take_parens`/keyword-region capture, assert the residue is empty or expected" — closes the class; each fix is ~3 lines plus a test.

Residual hand-rolled scanner inventory: ~10 (vs ~23 pre-migration), each either justified upstream of the lexer by design (`match_keyword_prefix` for prefix detection, `blank_sql_comments` as the phase-8 pre-pass — a documented, well-reasoned decision), confined to identifier-free text (two ASC/DESC/NULLS modifier loops), a leaf module shared with `parse/` (`ident.rs`), or catalogued debt (SHOW name slots). One is genuine sediment: `extract_ddl_name` (`rewrite.rs:437-530`) — `pub`, zero production callers, carrying its own drifted LIKE/IN grammar including a bare whitespace split. Delete it and re-point its proptests at `plan_rewrite`/`parse_show_filter_clauses`.

---

## 4. Test coverage (question a, part 2)

### 4.1 Prior gaps: all closed

T-1 through T-9 from the prior review are FIXED (verified at test-name level): the differential oracle gained semi-additive (with engineered ties at the snapshot date and an expr≠column dimension — the exact E-1 poison), window, and wildcard sections; duplicate-clause errors assert message **and** second-occurrence caret; empty clause bodies pinned; in-body comments pinned end-to-end with caret honesty; hostile identifier generators now drive the full `plan_rewrite` front door (`tests/common/mod.rs` + `tests/create_front_door_proptest.rs`); all 15 two-clause order inversions asserted with carets; 500-entry METRICS and 64-deep parens stress tests; a Rust-side randomized-schema differential proptest (`tests/differential_proptest.rs`). Every §2 bug from the prior review has a regression fixture with the exact poison (`cr20260711_correctness.test`, `e4_cross_source_diamond.test`).

Infrastructure is clean: TEST_LIST 72/72 with a `comm`-verified sync gate run both locally (`just check-test-list`) and in CI; the two `_excluded` tests self-document a real runner limitation and are covered elsewhere; `Fuzz.yml` actually runs all 8 targets 10 min/push with auto-filed crash issues; CodeQuality adds an 80% line-coverage floor.

### 4.2 Remaining gaps (prioritized)

| ID | Value | Effort | Gap |
|---|---|---|---|
| T-10 | HIGH | S | **The F-2/F-3/F-4 interiors**: no test constrains residue inside OVER (pre-PARTITION region), around USING/NAB parens, or in MATERIALIZATIONS sub-bodies (junk, duplicates). Land these with the fixes — they are the regression tests for §2. |
| T-11 | MED | M | **Differential oracle still has no role-playing `USING` section** (the T-1 residual). Two role-played edges to one dimension table vs a hand-written double-join — the same alias-binding-under-joins family that produced E-1. The other three sections' helpers make this cheap. |
| T-12 | MED | S | **Doctests run in no CI job**: `test-rust` is `cargo nextest run` (Justfile:37), which skips doctests, and no workflow runs `cargo test --doc`. 14 doc blocks including the **`compile_fail` FFI-safety guard at `src/ddl/read_ffi.rs:88`** currently compile-or-not unobserved. One line in the Justfile. |
| T-13 | MED | S | **Interior/leading empty entries silently dropped**: `split_at_depth0_commas` (`scan.rs:105-134`) discards *every* empty entry though its doc promises only trailing-empty discard — `DIMENSIONS (a AS x,, b AS y)` parses silently. Pin or reject (rejecting matches the P-2 philosophy); fix the doc either way. |
| T-14 | LOW | S | `""` in body entry slots (F-11); annotation error carets (`err.position` unasserted in the P-2 unit tests — a caret regression would pass); a directed CRLF end-to-end body test (currently safe by construction only). |
| T-15 | LOW | S | Semi-additive × role-playing interaction cell (F-18); a 20-line `fuzz_lexer` target asserting the tiling invariant directly (currently exercised transitively). |

---

## 5. Snowflake conformance under DuckDB conventions (question b)

Full matrix in the conformance pass; summary of verdicts (Snowflake behaviour verified against live docs unless noted):

**Conforms:** clause set and ordering (TABLES → RELATIONSHIPS → FACTS → DIMENSIONS → METRICS, at-least-one-of-DIMENSIONS/METRICS); entry direction `name AS expr`; PRIVATE/PUBLIC on facts/metrics; `USING`/`NON ADDITIVE BY` grammar positions; window-metric grammar incl. `PARTITION BY EXCLUDING` and the required-dims query rule; derived metrics; FACTS+METRICS mutual exclusion in queries; qualified-only wildcards with PRIVATE exclusion; dims-only/metrics-only query semantics; fan-trap rejection; DESCRIBE's exact 5-column kind/property model; SHOW SEMANTIC DIMENSIONS/METRICS/FACTS' exact 8 columns; GET_DDL; SHOW filter set (LIKE→ILIKE, STARTS WITH, IN, LIMIT).

**Deliberate, documented adaptations (well-chosen):** table-function query surface instead of the `SEMANTIC_VIEW(...)` FROM construct (honest given DuckDB's parser-extension limits, prominently documented); uniform DuckDB-style case-insensitivity for all identifiers including quoted (explicitly documents the departure from Snowflake, empirically matches DuckDB's rule — rare thoroughness); no owner columns in SHOW (TERSE stays byte-compatible); explicit `PRIMARY KEY` requirement with `duckdb_constraints()` auto-resolution; `MATERIALIZATIONS` and `SHOW ... FOR METRIC` clearly flagged as extensions beyond Snowflake; stricter multi-grain-metric and window-mixing rejections, documented.

**Violations of the reference:** F-1 (semi-additive polarity — the one silent-wrong-numbers conformance bug); the porting-friction set F-6/F-7/F-10/F-12 (each rejects valid Snowflake DDL, mostly with unhelpful messages); F-8 (pre-aggregation WHERE missing and unlisted); and the documentation set F-5/F-13 (README documents removed syntax; the comparison page misquotes Snowflake in both directions and understates the gap list — `CREATE OR ALTER`, TAG, `LABELS`, `MAX_STALENESS`, AI_* clauses, query-side member aliases, and F-8 are all absent from "not yet supported", though several are legitimately N/A for DuckDB).

**DuckDB-convention execution:** error format matches DuckDB (`ParseError{message, position}` byte offsets feeding native `Parser Error: ... LINE n: ^` rendering, with dedicated caret tests incl. unicode); one gap is F-10 (`DESC`), which is a DuckDB convention as much as a Snowflake one. Minor nit: DESCRIBE emits `''` where Snowflake/DuckDB would use NULL for non-applicable cells.

One uncertain item (MEMORY, untested): Snowflake permits dimension expressions referencing same-table FACTS; here fact inlining runs only for metric/fact expressions (`src/expand/facts.rs:350-463`), so a dimension referencing a fact name would leak raw into SQL. Worth a test + doc note either way.

---

## 6. Engineering quality (question c)

**Grade: A** (prior review: A– for idiomaticity; this covers the full engineering surface).

**What earns it** (all verified in current code): the remediation was closed to the "all sites migrated, old form deleted" standard — the §3.4 half-migration table is empty (17/17 dispatchers on `run_dispatcher`, C++ registration behind one `SvTableFunctionSpec` core with the init_local/init_global XOR enforced at registration, the bespoke list parser folded onto the strict generic scaffold); the duplication counts collapsed (6 reference scanners → 1 engine + 2 deliberately-separate quote-aware graph scanners with written rationale; 4 SELECT emitters → 1 `SelectSpec` in which the E-1 alias-shadowing defense is structural — `GroupBy::Ordinals` only; 3 graph traversals → 1 `JoinTree` for the directed walks); the class-killing abstractions are compile-time where possible (`SqlLit` sole-constructor escaping, `Resolvable` making the closure transposition `unreachable!`, `CiName<K>` kind markers); the FFI/C++ seam is exemplary (RAII `SvOwnedBuffer`, both-or-drop publish contract with proptest-grade tests, every C-API boundary in `try/catch(...)`, no C++ exception reaches a Rust frame, `static_assert` + runtime ABI probes, the `no_long_lived_conn.rs` AST guard, zero statics, ~12 guarded non-test unwraps, zero TODO/FIXME/HACK); every user-string SQL splice goes through `SqlLit` or `quote_ident`; E-1's fix was empirically re-verified on DuckDB 1.5.4 (old shape reproduces the wrong `('US', 3)`, current shape yields `('US', 2)`) and is pinned at three layers including EXPLAIN-output assertions. `cargo test`: 1,293/1,293 green on this snapshot.

**The four things separating it from unqualified staff level:**

1. **Finish or fence the error architecture.** Parse/expand/query are typed with positions; the graph/validation layer still feeds ~15 `Result<_, String>` signatures upward positionless. Extend the treatment or write the paragraph declaring the boundary deliberate — currently it reads as an unfinished rollout, the exact pattern the prior review warned about.
2. **TECH-DEBT.md accuracy sweep.** The project's canonical trade-off record is measurably stale against the remediation velocity: entry #25 cites three sites the §6.1 migration already fixed (only the SHOW slots remain); #28 references a duplicated materialization matcher that E-6 collapsed. The file is load-bearing for future agents/reviewers; it needs the same sweep the code got.
3. **The reference-scanning engine is consolidated but still not quote-aware** (TECH-DEBT #28): string literals inside expressions can still be substituted into; quoted names in NAB/derived expressions don't match. E-2/E-3 is narrowed, not killed; the one-quote-aware-tokenizer endgame from prior-§6.2 remains the right next structural move.
4. **Sweep the sediment**: `extract_ddl_name` (dead duplicate grammar), `render_ddl.rs:14-15`'s `escape_single_quote` copy outside `SqlLit` (context differs — GET_DDL text vs executable SQL — but the "exactly one place" rationale is one site short), the 13 unchanged `fk_columns.is_empty()` legacy guards (E-8, untouched), the F-18 alias fallback, and the missing one-line `MaxThreads() == 1` comment on `SemanticViewGlobalState`'s unsynchronized cursor. Half a day; leaves no known residue.

---

## 7. Recommended sequence

1. **Correctness PR:** F-1 (polarity — with CHANGELOG break notice + differential-section update so the oracle enforces Snowflake polarity), F-2, F-3, F-4, each with the probe inputs as regression tests (T-10).
2. **Porting/diagnostics PR:** F-6, F-7, F-9, F-10, F-11, F-12, F-14; README fix (F-5) and comparison-page corrections (F-13, F-8 listing) can ride along.
3. **Cheap test/infra holes:** T-11 (USING differential section), T-12 (doctests in CI — one line), T-13, T-14.
4. **Hygiene:** TECH-DEBT.md sweep, `extract_ddl_name` deletion, escape-copy unification, F-15 annotation positions, F-16, F-18 hardening, E-8 guard removal.
5. **Structural (opt-in):** quote-aware reference tokenizer (kills the E-2/E-3 class); typed errors through the graph layer.
