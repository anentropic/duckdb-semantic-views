# Code Review — 2026-08-08

**Scope:** Full codebase review at `57e38fd` (post-#209), on branch
`claude/duckdb-semantic-views-review-ho1bm0`, working toward v0.12.0 (v0.10.4 released,
v0.11.0 unreleased). Requested focus: the recurring bug areas — parser, extension/connection
handling, generated SQL — and the test-gap / vacuous-test pattern behind them.

**Method:** Five parallel review passes: (1) parsing layer (`body_parser/`, `parse/`,
`expr_tokens.rs`, `sql_lit.rs`); (2) query expansion and SQL generation (`expand/`, `graph/`);
(3) catalog, connection, FFI (`catalog/`, `ddl/`, `query/`, `lib.rs`, `ffi_util.rs`,
`cpp/src/shim.cpp`); (4) identifiers, rendering, model (`ident.rs`, `render_ddl.rs`,
`render_yaml.rs`, `model.rs`, YAML import path); (5) test-suite honesty audit (harness pins,
new-test vacuity, TEST_LIST/fuzz/doc sync). Each pass audited the fix commits landed since the
last review (#203–#209) for completeness **before** fresh hunting. **Verification level is
stated per finding.** The expansion findings (EXP-25..29) were all *numerically confirmed*:
generated SQL executed against the bundled in-memory DuckDB and the wrong values observed. The
rendering/model findings (RT-7/8/9, MODEL-1) and parser findings (PARSE-10/11/12/13) were
executed via temporary tests (since removed; working tree clean). Two of the fix commits'
test sets (#204, #207) were confirm-the-red verified by actually reverting the fix hunks and
watching exactly the new tests fail. No fixes have been applied — this is a review document.

**Prior review:** `_notes/code-review-2026-08-06.md`. Its EXP-19..24, PARSE-5..8 and RT-5/6
findings are confirmed landed (#203–#209) and audited below. Its CAT-5/6, QRY-1/2, FF-12/13/14,
IDENT-1..5, PARSE-9, PBT-8..12 and TC-12/13 findings remain **open at this HEAD**; each was
re-confirmed still-open (see §8) and none has yet been given a TECH-DEBT entry.

---

## 1. Executive summary

The dominant lesson this round: **the last round's fixes need the same combinatorial scrutiny
as new features.** Of the four worst findings, three sit *inside or adjacent to* the #203/#207
fixes — the constant-argument fence's whitelist leaks four ways (EXP-25), the phantom-row
hazard it fences has a much larger unfenced surface (EXP-26), and the EXP-23 fix converted a
loud binder error into a silent 2× by adding a join without extending the fan-trap fence
(EXP-27, a regression new in #207). Meanwhile the blind-spot → bug correlation held for the
third consecutive round: EXP-28 sits exactly in the still-open PBT-8 cell (FACTS requests
pinned at `vec![]` in every harness), EXP-26's shape in the new child-grain harness's
constant-only metric list, and the brand-new harness itself re-introduced the canonical
`where_clause: None` pin (PBT-13) two commits after CLAUDE.md enshrined it as the known-fatal
shape.

The second theme is a **YAML-ingress class** the RT-5/6 fix (#209) did not reach: the fix
enumerated identifier *slots*, but fields that are not slots — member-expression comment
markers (RT-8), the entire `WindowSpec` payload and materialization/NAB cross-references
(MODEL-1), `output_type` (RT-7), keyword-colliding derived-metric names (RT-9) — still let a
YAML-validated definition render GET_DDL that replays to a *different* model or none at all.

| # | Cluster | Worst consequence | Where |
|---|---------|-------------------|-------|
| 1 | **EXP-25/26**: the SG-8 phantom-row fence is a leaking whitelist, and non-constant NULL-insensitive arguments have no fence at all | `COUNT(DISTINCT 1)`=1, `MIN(1)`=1, `SUM(COALESCE(li.qty,99))`=99 on a childless parent; a cross-table fact reference inflates a child-grain metric 25 vs 20 — all numerically confirmed | `src/expand/facts.rs:403,412,483,739` |
| 2 | **EXP-27**: WHERE-member fact chains join fanning tables unfenced — **regression opened by #207** | Silent 2× (`rev` = 200 vs 100), where pre-#207 the same query was a loud binder error — numerically confirmed through both the fact and dimension branches | `src/expand/where_clause.rs:231-241`, `fan_trap.rs:657,719` |
| 3 | **RT-8**: a `--` in a YAML member expr passes validation; GET_DDL replay silently merges two members into one | Dump/restore corrupts the model with no error anywhere — executed, 2 dims became 1 | `src/model.rs:747-775` vs `:595-611` |
| 4 | **PARSE-10**: window-metric expression text outside `FUNC(args) OVER (...)` is silently discarded | `AVG(w) OVER (...) + 100` parses; queries compute without the `+ 100` — definition and behaviour disagree, executed end-to-end | `src/body_parser/window.rs:24-106`, `expand/window.rs:277` |
| 5 | **MODEL-1**: YAML import bypasses every cross-reference validation living only in `parse_keyword_body` | Six YAML-valid shapes render GET_DDL its own parser rejects; a hostile `frame_clause` injects an extra metric into the replayed model | `body_parser/mod.rs:287-437` (checks with no YAML-path twin) |
| 6 | **EXP-28/29**: FACTS queries and dims-only DISTINCT on child-table members emit a phantom NULL row per childless parent | Row-level results contain rows that don't exist — sits exactly in the open PBT-8 blind spot | `src/expand/sql_gen.rs:195,316-327,690` |
| 7 | **PARSE-11/12/13**: nested resolving calls panic (→ silent no-injection); escape-string/typed-literal introducers scan as references; `--` comments don't terminate at bare `\r` | Search-path injection silently disabled; role-playing alias rewriter corrupts `e'...'` literals | `parse/search_path.rs:201-211`, `expr_tokens.rs:155-192`, `util.rs:388-395` |
| 8 | **PBT-13/14**: the new child-grain harness re-pins `where_clause: None`; window-over-dependent-metric never reaches a number | The next EXP-9/EXP-10-shaped bug has a ready-made hiding place | `tests/child_grain_proptest.rs:268`, `semi_additive_proptest.rs:800,829-831` |

Catalog/connection/FFI is the healthiest area this round: five LOW findings only, and the
transaction, concurrency, reload-idempotence and FFI panic-safety architecture all verified
clean (§5).

---

## 2. Audit of the #203–#209 fixes

- **EXP-19/20 (#203) — clean.** `collect_transitive_metric_names` (`facts.rs:828`) is
  genuinely transitive: window inners pushed unconditionally, recursion through derived
  metrics. Executed probes: window→derived→semi-additive errors correctly; a derived metric
  over **two** semi-additive bases with different NON ADDITIVE BY dims errors on the
  still-active base when only one NA dim is queried. Controls pass.
- **EXP-21 (#203) — holds for its stated scope, but the invariant is narrower than the
  hazard.** COUNT(1)/SUM(1)/COUNT('x')/nested parens/FILTER all guarded; guard propagates
  through derived-metric inlining (confirmed 102/100). The escapes are EXP-25/26 below, and
  TECH-DEBT #56's DISTINCT rationale is factually wrong (see EXP-25).
- **EXP-22 (#203) — clean.** All three re-anchor deciders take the qualification flag; the
  facts path never re-anchors; single-grain/multi-grain/window/snapshot topologies all route
  through them. Raw-column residual honestly recorded (TECH-DEBT #57).
- **EXP-23/24 (#207) — FIND and INLINE now agree at every traced site** (`metric_keys`
  mirrors `insert_fact_keys`; bare+qualified dual keying consistent). **But the EXP-23 join
  was added without extending the fan fence — that is EXP-27, a regression.**
- **PARSE-5 (#204) — complete.** All four scan sites in `inject_search_path` read the
  blanked copy; splice offsets index the original; length-preservation asserted. No remaining
  comment-blind injection/detection site found (`detect_ddl_kind`, `detect_near_miss`,
  `plan_rewrite`, `plan_ddl` all blank first). Nested-block-comment semantics verified against
  live DuckDB. Adjacent defects found in the same machinery: PARSE-11 (nested calls), PARSE-13
  (bare-`\r` line endings).
- **PARSE-6 (#204) — complete**, uniformly applied to COMMENT/LABELS/WITH, spaced dots
  handled. Confirm-the-red verified by reverting the `annotation_boundary_before` hunk: the 3
  new tests fail individually, the control passes.
- **PARSE-7 (#205) — complete at the scanner level.** All four scanners route through the
  shared `opens_escape_string`/`escaped_pair_end`; no fifth private scanner exists;
  `COMMENT = e'...'` is rejected *loudly* (acceptable divergence, worth a TECH-DEBT line). The
  gap the fix did not reach is upstream of the scanners: PARSE-12.
- **PARSE-8 (#206) — complete in the parsing layer.** Every remaining `eq_ignore_ascii_case`
  there is a keyword comparison. One production residue survives *outside* the parse layer:
  `src/expand/wildcard.rs:53,69,71` compares wildcard-qualifier aliases quote-blind (EXP-30).
- **RT-5/6 (#209) — real and well-built for identifier slots** (single YAML choke point,
  emission quote-protection, fuzz oracle's second escape genuinely gone,
  `roundtrip_proptest.rs` asserts strict per-clause equality). What the slot enumeration
  missed: non-slot fields — RT-7 (`output_type` dropped), RT-8 (expr comment markers), MODEL-1
  (WindowSpec payload + cross-references), RT-9 (keyword-colliding derived names).
- **Confirm-the-red discipline:** #204 and #207 test sets verified red by hunk-revert (each
  new test individually red, controls green). #203/#205/#206/#209 commit messages document
  per-case red; #203's anti-vacuity generator guards and #207's `references_chained_fact`
  guard are real. All six new `cr20260806_*.test` files are in TEST_LIST (list ↔ disk sync
  exact at 106/106).

---

## 3. Query expansion — correctness findings

All five findings numerically confirmed: model structs mirroring parser output, `expand()`
driven directly, generated SQL executed against in-memory DuckDB. Shared fixture for
25/26/28/29: base `o(id, region, rate)` rows (1,'E',10),(2,'N',5); child `li(id, order_id,
qty)` rows (1,1,2),(2,1,3); `li(order_id) REFERENCES o`, both PKs declared. Order 2 is
childless, so the base-anchored `FROM "o" LEFT JOIN "li"` NULL-extends it.

### EXP-25 — HIGH: three escapes of the EXP-21 constant-argument guard

`src/expand/facts.rs:403` (`MULTIPLICITY_SENSITIVE_AGGS`), `:412` (`is_constant_literal`),
`:483` (`constant_aggregate_arg`); TECH-DEBT #56.

- **`COUNT(DISTINCT 1)`** on `li`, dims `[region]` → 1, expected **0**. TECH-DEBT #56's claim
  that `COUNT(DISTINCT <constant>)` "needs nothing" because it is multiplicity-invariant is
  wrong: it is multiplicity-invariant but *existence*-sensitive. The phantom row is not a
  duplicate; it is a row that should not exist.
- **`COUNT(1+0)`** → 1, expected **0**. A constant *expression* is not a literal, so it fails
  `is_constant_literal` and escapes exactly as `COUNT(1)` did pre-#203.
- **`MIN(1)`** → 1, expected **NULL**. MIN/MAX/AVG were excluded on the same
  multiplicity-invariance rationale — correct for duplicates, wrong for empty groups. #203's
  own CHANGELOG argues empty-group semantics are part of the contract; MIN/MAX/AVG/ANY_VALUE
  violate it identically.

**Fix direction:** the per-spelling whitelist keeps leaking. The guard
`AGG(x) → AGG([DISTINCT ]CASE WHEN <pk> IS NOT NULL THEN x END)` is semantically neutral on
real rows for *any* argument (pk non-null there), so apply it to every aggregate argument of a
non-base metric instead of only recognized constants — which also fixes EXP-26.

### EXP-26 — HIGH: non-constant NULL-insensitive aggregate arguments count the phantom row — no fence exists at all

Emission at `facts.rs:739` runs only `guard_constant_arg_aggregates`.

- **`SUM(COALESCE(li.qty, 99))`** on `li` → 99, expected **NULL**. Any NULL-insensitive
  expression over the child's own columns (`COALESCE`, `CASE`, `x IS NULL`) resurrects the
  phantom row.
- **Cross-table fact reference:** fact `orate AS o.rate` on `o`, metric `m AS SUM(orate)` on
  `li` → emitted `SUM((o.rate))` over `FROM o LEFT JOIN li`: total **25 vs 20**, grouped
  **5 vs NULL** — the childless order contributes its parent-side value to a line-item-grain
  metric. This is the exact mirror of PAR-6's tested direction and sits in the child-grain
  harness's blind spot (constant args and `SUM(li.amount)` only).
  `check_referenced_fact_fan_traps` passes because `li→o` doesn't fan.

**Fix direction:** the generalized PK guard from EXP-25; extend `child_grain_proptest` with a
COALESCE metric and a parent-fact metric.

### EXP-27 — HIGH: a WHERE-member's fact chain reaching a fanning table joins it unfenced — silent 2×, regression opened by #207

`src/expand/where_clause.rs:63,80` (`member_fact_tables`), `:231-241` (added to
`source_tables`); missing fence: `fan_trap.rs:657` (`check_where_clause_fan_traps` checks only
the member's *own* table, `:673-676`) and `:719` (`check_referenced_fact_fan_traps` never sees
WHERE members).

- Model: base `o(id, region, amount)`=(1,'E',100); child `li`=(1,1,5),(2,1,6); fact
  `liq AS li.qty` on `li`; fact `of AS li.liq * 2` on `o`; metric `rev AS SUM(o.amount)` on
  `o`. Query dims `[region]`, metrics `[rev]`, `where_clause := 'of > 0'` → emits
  `FROM o LEFT JOIN li WHERE ((li.qty) * 2) > 0` → **`rev` = 200, expected 100**. Same
  through the *dimension* branch (`band AS CASE WHEN li.liq > 0 ...` on `o`). Both
  numerically confirmed.
- Before #207 both shapes were a loud binder error (`li` unjoined); #207 added the join
  without extending the fence — loud → silent-wrong.

**Fix direction:** fence each WHERE member's `member_fact_tables` (the exact set EXP-23
computes) through `fanning_edge_on_path`, mirroring how `check_referenced_fact_fan_traps`
treats queried members.

### EXP-28 — MEDIUM: FACTS query on a child-table fact returns a phantom row per childless parent

`src/expand/sql_gen.rs:195` (`expand_facts` always anchors at the base table, `:316-327`).
Fact `liqty AS li.qty` on `li`, `facts := ['liqty']` → `SELECT li.qty FROM o LEFT JOIN li` →
**3 rows `[2, 3, NULL]`, expected 2** — a spurious all-NULL fact row at line-item grain.
Numerically confirmed. Sits precisely in the PBT-8 blind spot. **Fix direction:** anchor the
fact query at the queried facts' common grain table (joining *up* for dimensions is fan-free),
or filter `WHERE <child pk> IS NOT NULL` when all queried facts live on one non-base table.

### EXP-29 — LOW: dims-only DISTINCT on a child dimension emits a join-manufactured NULL row

Same root, dims-only branch (`sql_gen.rs:690`, `distinct = true`). Dim `qty AS li.qty`,
`dimensions := ['qty']` → `[2, 3, NULL]`, expected `[2, 3]`; the NULL is indistinguishable
from a genuine data NULL. Numerically confirmed. Same fix as EXP-28.

### EXP-30 — LOW: wildcard qualifier alias comparison is quote-blind (PARSE-8's surviving sibling)

`src/expand/wildcard.rs:53,69,71` — `t.alias.eq_ignore_ascii_case(alias)` in production code,
so `'"O".*'` fails against alias `o` with "unknown table alias". Loud, not silent; same class
PARSE-8 closed elsewhere. Code-trace. **Fix:** `ident_matches`.

### Fence-ordering note (speculative, code-trace)

`try_route_materialization` (`sql_gen.rs:400-406`) runs before the EXP-19
`SemiAdditiveThroughDependency` fence, so a materialization naming a derived-over-semi-additive
metric routes without the error. Materialized contents are user-supplied, so no wrong number is
generated by us — flagged as fence-ordering awareness only.

### Examined and not flawed (expansion)

Two-child-fact-table fan trap (per-grain FULL OUTER, correct incl. NULL-group asymmetry);
derived ratio across two grains; WHERE member on base with parent-grain metric and
semi-additive + WHERE-on-child both loudly fenced; predicate placement pre-aggregation in
snapshot/window/per-grain CTEs; `fanning_edge_on_path` orientation; SG-16 role-playing
worst-case cardinality; where_clause metric-reference rejection incl. qualified; PRIVATE fact
fencing; wildcard PRIVATE/dedup; ordinal-GROUP-BY E-1 defense; `__sv_*` CTE names cannot
collide with user aliases.

---

## 4. Parsing layer — correctness findings

### PARSE-10 — HIGH: window-metric expression text outside `FUNC(args) OVER (...)` is silently discarded

`src/body_parser/window.rs:24-106`: after `cur.take_parens()` consumes the OVER body (line
45), remaining tokens are never checked; `func_part = expr[..over_tok.start]` accepts an
arbitrary prefix whose text before the first `(` becomes `window_function` verbatim. Emission
rebuilds solely from the spec (`src/expand/window.rs:277`).

Executed end-to-end: `t.m AS AVG(w) OVER (PARTITION BY d) + 100` parses cleanly; emitted SQL
is `AVG("w") OVER (PARTITION BY "d")` — the **`+ 100` vanishes**, so `m` returns a number 100
smaller than its (valid-DuckDB) definition text. Variants: `1 + AVG(w) OVER (...)` stores
`window_function == "1 + AVG"`; `AVG(w) OVER (...) - SUM(w) OVER (...)` silently drops the
second term. GET_DDL round-trips the stored text while queries compute from the spec —
definition and behaviour disagree.

**Fix direction:** after taking the OVER parens, error on any remaining token; require
`func_part` to be exactly one identifier chain immediately followed by the argument parens —
the same P-2/F-3 "reject, don't discard" rule already applied to USING/NAB residues.

### PARSE-11 — MEDIUM: `inject_search_path` panics on nested resolving calls; the caught panic silently disables all injection

`src/parse/search_path.rs:201-211` — `insert_at` is collected in *head order*, but a resolving
call nested inside another's parens closes **before** the outer close, so offsets descend and
`&sql[prev..close]` slices start > end → panic (observed at `:207`). Executed:
`SELECT * FROM semantic_view((SELECT view_name FROM show_semantic_dimensions('x') LIMIT 1))`
panics; in production both FFI hooks `catch_unwind` → rc=2 → the statement executes with **no
search-path injection for either call** — the PARSE-5 consequence resurfacing, plus a
swallowed panic on every execution. **Fix:** sort `insert_at` ascending (offsets index the
original text); pin idempotence for the nested shape.

### PARSE-12 — MEDIUM: escape-string / typed-literal introducers scan as identifier references

`src/expr_tokens.rs:155-192`: at `e'\''` the `e` is an ident byte, so `scan_chain` emits a
one-byte Reference chain before the string skipper sees the quote; same for `DATE'2020-01-01'`
→ reference `date`. Executed at the exact primitives expansion calls:
- with a declared member named `e`, `inline_references` corrupts `count_if(x = e'\'')` into
  `count_if(x = (o.something)'\'')` (query-time syntax error from a valid definition), and
  FIND reports a phantom dependency — which since PAR-6 changes join topology;
- `rewrite_qualifier("e.name || e'a'", "e", "e__dep")` → `e__dep.name || e__dep'a'` — the
  role-playing alias rewriter corrupts the literal when an alias is named `e`;
- a fact named `date` corrupts `o.ts > DATE'2020-01-01'` the same way.

Distinct mechanism from the known IDENT-3 family: the *prefix-abutting-a-quote* shape, newly
relevant because PARSE-7 made `e'...'` first-class. **Fix:** in `scan_chains`, when a chain is
immediately followed by `'` (the `opens_escape_string` adjacency rule), do not emit it as a
reference — treat it as the literal introducer. One rule fixes `e'...'`, `E'...'` and all
typed-literal prefixes.

### PARSE-13 — LOW: `blank_sql_comments` does not terminate `--` comments at bare `\r`; DuckDB does

`src/util.rs:388-395` blanks until `\n` only. Both sides executed: DuckDB ends the line
comment at `\r` (PG scanner rule); our blanker blanks the `\r` **and following code**.
Consequence: `-- note\rSELECT * FROM semantic_view('v')` blanks entirely → no search-path
injection while DuckDB executes the query; on the DDL side a clause after `-- c\r` silently
vanishes from the body. Bare-CR endings are rare — one-condition fix (terminate at `\r`,
keeping the byte).

### INFO (parsing, not defects)

`SHOW ... LIKE'x%'` (no space) rejected loudly — dialect nit (`show_clauses.rs:365-455`).
`COMMENT = e'...'` rejected loudly — consistent-but-narrower than host dialect; worth a
TECH-DEBT line if E-strings are in scope for annotation values.

---

## 5. Catalog, connection, FFI

The healthiest area this round — five LOW findings, none silent-wrong-number.

### CAT-7 — LOW: fixed `/tmp` filenames in file-backed unit tests collide across concurrent `cargo test` processes

`src/catalog/mod.rs:1829-1835, 1857-1861, 1898-1903, 1941-1944, 1986-1989`;
`src/lib.rs:832`. Observed live during this review: 4 tests failed on a conflicting-lock
error from a sibling test process, passed in isolation. Each test also `remove_file`s the
path up front, so one run can delete another's live database. **Fix:** the pid+nanos unique
naming `tc6_restart_persistence_survives_reopen` already uses (`catalog/mod.rs:2047-2062`).

### CAT-8 — LOW: read-only open of a v0.1.0 companion-file database silently loses all views

`src/catalog/mod.rs:61-75` — the RO branch checks only `definitions_is_legacy_shape`; the
companion-file detection (`:104-171`) is unreachable read-only. The analogous legacy-*table*
case refuses loudly with actionable wording; the companion-file case resolves every view as
nonexistent with no hint. Code-trace; narrow population. **Fix:** probe for the companion
file in the RO branch and refuse with the same wording.

### FF-15 — LOW: `__sv_compute_create_from_yaml` bind lacks the FF-4 NULL-argument guards every other TF received

`cpp/src/shim.cpp:948-950` (contrast the guards at `:1459-1462, :1494-1499, :2729-2732`).
`GetValue<string>()` on a NULL Value renders the string `"NULL"` (verified against bundled
DuckDB source), so NULL args become a file literally named `NULL` / a view named `NULL`.
**Fix:** the same up-front `IsNull()` → `BinderException` guard.

### FF-16 — LOW: a panic in the parser-hook Rust code reports as "not ours" — an internal bug surfaces as the user's syntax error

`src/parse/ffi.rs:193, :334` (`result.unwrap_or(2)`): a panic in `rewrite_to_native_sql`
yields rc=2 from both hooks → DuckDB's generic `syntax error at or near "SEMANTIC"` with zero
trace of the extension's failure — indistinguishable from "extension not loaded". Contrast
the 17 read dispatchers, whose panics surface as `internal error: panic inside <name>`.
**Fix direction:** panic arm writes a diagnostic into `error_out` and returns rc=1 from
`sv_parse_function_rust` (the `DISPLAY_EXTENSION_ERROR` channel exists); the override side
stays rc=2.

### LIFE-2 — LOW: a failed LOAD leaves the extension half-active, with no rollback

`src/lib.rs:518-533` — first registration failure returns `Err`, but the parser hooks +
`allow_parser_override_extension` setting (`shim.cpp:3063-3089`) and the catalog schema
(`lib.rs:499`) are already committed. DDL half-works while read TFs are absent, until a retry
LOAD (which does heal — WR-09 dedup + `ALTER_ON_CONFLICT`). Extraordinary failure, hence LOW;
cheap improvement: register the parser hook *last*.

### INFO (catalog/FFI)

`test/sql/extension_reload.test:1-12` header describes the retired pre-WR-09 architecture —
stale doc in exactly the file meant to pin reload semantics (test itself still valid).
`shim.cpp:2316,2331` unchecked `static_cast<uint32_t>` asymmetry vs the Rust `wire_len`
discipline (bounded; Rust decoder rejects desync). `src/ddl/list.rs:74-76` stale comment
(noted 2026-08-06, still present).

### Verified clean (catalog/connection/FFI)

Write-path SQL builders (every name/schema/path/comment slot SqlLit-escaped exactly once;
writes and reads share `resolve_in_search_path` branch-for-branch); transactional DDL incl.
rolled-back `CREATE OR REPLACE` byte-identical restore; concurrency posture (DuckDB PK/MVCC
serializes; outcome shapes pinned by the three `test_concurrent_*.py` suites); reload
idempotence (WR-09 dedup wraps both registration and allocation); FFI memory safety
(`catch_unwind` on all dispatchers, char-boundary error truncation, both-or-drop publish
contract, wire formats bounds/trailing/count-checked both sides); lifecycle (FF-3 primary-db
resolution, no long-lived connection, interrupt forwarding preserves the bare `Interrupted!`
contract); search_path ↔ catalog interplay (PARSE-5 fix verified in place; write-side
`resolved_schema_expr` evaluates the identical `SEARCH_PATH_SQL` on the caller's connection).
Executed: `cargo test` 1654 passed; the only 4 failures were the CAT-7 cross-process
collisions, green in isolation.

---

## 6. Identifiers, rendering, model

### RT-8 — HIGH: `member_expr` omits the `blank_sql_comments` check `slot_common` has — a `--` in a YAML expr silently merges/destroys members on replay

`src/model.rs:747-775` (`member_expr` checks trim-empty + `column_roundtrips_verbatim` only;
`slot_common` at `:595-611` additionally rejects comment markers; `column_roundtrips_verbatim`
has no comment awareness, as `model.rs:1013-1015` itself documents).

Executed: YAML `dimensions: [{name: d1, expr: 'o.a -- x', ...}, {name: d2, expr: o.b, ...}]`
passes the full YAML gate. GET_DDL renders `o.d1 AS o.a -- x,\n    o.d2 AS o.b`; replay
through the front door (which blanks comments before parsing) blanks from `--` through the
member-separating comma: **parse succeeds, 2 dimensions become 1**, with d2's definition
swallowed into d1's expression. Silent model corruption. (A `--` before COMMENT/SYNONYMS
annotations likewise eats them.) **Fix:** the same `blank_sql_comments(expr) != expr`
rejection in `member_expr` (and window `frame_clause`/`order_by.expr`/`extra_args` if MODEL-1
is fixed by validation).

### MODEL-1 — MEDIUM/HIGH: YAML import bypasses every semantic cross-reference validation that lives in `parse_keyword_body` — GET_DDL then emits DDL its own parser rejects

Checks live only in `src/body_parser/mod.rs` (window inner metric `:355-372`,
PARTITION/ORDER BY dims `:287-348`, materialization refs `:392-437`, materialization
duplicate names `:378-390`, at-least-one-of-DIMENSIONS/METRICS); the YAML path runs only
`validate_ddl_representable` + the graph validators, none of which touch `window_spec`,
`non_additive_by` existence, or `materializations` (grep-verified).

All executed — each passes the full YAML gate, then replay of its own GET_DDL **fails**:
`non_additive_by: [{dimension: ghost}]`; `window_spec: {window_function: AVG, inner_metric:
ghost}`; materialization referencing `ghost`; materializations named `m1`/`M1`; materialization
with no dimensions and no metrics. **Model-changing variant:** `frame_clause: 'ROWS BETWEEN 1
PRECEDING AND CURRENT ROW) , junk AS ('` → replay **parses to 3 metrics** — a metric injected
out of a frame-clause string. The window-spec *syntactic* slots are emitted raw by
`emit_window_expr` (`render_ddl.rs:319-371`) and never validated — squarely the RT-6 slot
class, not the "semantic cross-reference" class TECH-DEBT #60 consciously deferred. #60's
statement that the YAML-storable ⇒ parseable-DDL implication "is still enforced" is false for
all six shapes; the entry under-records what was given up.

**Fix direction:** syntactic slots → extend `validate_yaml_members`/`..._materializations`
with the existing slot predicates + comment/roundtrip checks for
`frame_clause`/`order_by.expr`/`extra_args`; semantic cross-refs → hoist the body-parser's
checks into shared functions called from both `parse_keyword_body` and
`enrich_definition_for_create`.

### RT-7 — MEDIUM: GET_DDL silently drops `output_type` — a query-semantics-bearing field only YAML can set

No emit site anywhere in `render_ddl.rs` (`emit_dimensions` `:260-276`, `emit_metrics`
`:374-407`, `emit_facts` `:238-257`); field defined `model.rs:62-67,195-200`; semantic effect
(CAST wrapping) at `expand/materialization.rs:124,133`, `expand/per_grain.rs:1002,1139`.
Executed: a YAML dimension with `output_type: VARCHAR(10)` passes the gate; rendered DDL
carries no trace; replay parses cleanly with `output_type: None` — the replayed view loses
the CAST, so queries return different types/values. Neither TECH-DEBT #51 nor #58 records
that GET_DDL (the documented dump/restore path) drops it, and `validate_ddl_representable`
doesn't reject it — so a definition is "YAML-storable" yet not DDL-representable,
contradicting #58's contract statement. **Fix:** reject at `validate_yaml_members` until DDL
grammar can carry it (matching the USING/NAB/OVER-on-derived precedent), or add DDL syntax +
renderer support; at minimum a TECH-DEBT entry per the degraded-state rule.

### RT-9 — LOW: a YAML derived metric named `private` renders bare and its GET_DDL cannot be replayed

`render_ddl.rs:128-134` quote-protects on lexing grounds only; the parser peels entry-initial
`PRIVATE` as the access modifier. Executed: YAML metric `{name: private, expr: total * 2}`
renders `private AS total * 2`; replay fails "Missing metric name". YAML-only ingress (DDL
can't create the bare shape); quoted `"private"` round-trips fine; only derived metrics
exposed. **Fix:** quote a bare entry-initial name matching `PRIVATE` on emission, or reject in
`validate_yaml_members`.

### IDENT-6 — LOW: `SHOW ... STARTS WITH` treats `_` / `%` in the prefix as wildcards

`src/parse/show_clauses.rs:112-115` — `name LIKE '{escaped}%'` with `SqlLit::escape` doubling
only `'`. Views `a_b` and `axb`: `STARTS WITH 'a_b'` returns both. The function's own doc
identifies exactly this hazard for IN SCHEMA and chose equality there. Snowflake's STARTS WITH
is a literal prefix. **Fix:** escape `%`/`_` + `ESCAPE` clause, or `starts_with(name, ...)`.

### Model invariants verified clean

Case-folded duplicate member names rejected on both paths (`graph/names.rs:36-56`; the
*materialization*-name gap is MODEL-1's); `validate_yaml_structure` covers empty member
lists except per-materialization; forward compat is non-destructive — upgrades and ALTER SET
COMMENT rewrite stored JSON only via `json_merge_patch` (unknown-field-preserving), RENAME
touches only the name column, no path rewrites stored JSON through the typed struct. Also
clean: `ident.rs` internals, `sql_lit.rs`, `render_yaml.rs` (pure serde, strips exactly the
four runtime fields), comment/synonyms/labels escaping, header quoting, IN SCHEMA/IN DATABASE
predicates, fuzz roundtrip target post-RT-5.

---

## 7. Test-suite audit

### PBT-13 — HIGH: the brand-new sixth numeric harness re-introduces `where_clause: None`

`tests/child_grain_proptest.rs:268`, landed in #203 — *after* PBT-6 closed exactly this pin
in all five prior harnesses and CLAUDE.md enshrined it as the canonical blind-spot shape. No
justifying comment; no filter members in its `build_def`. Concrete bug shape left open: a
pre-aggregation predicate interacting with the SG-8 `CASE WHEN <child pk> IS NOT NULL` guard —
a predicate that empties a child set turns a real parent into a childless one at filter time,
precisely where the phantom-row rewrite must hold (and where EXP-25/26 now show the guard
family is fragile). **Fix:** mirror the other harnesses — `Option<Pred>` over base dims +
filter members, WHERE in the correlated-subquery oracle, extend
`generator_produces_childless_base_rows` (`:329`) with predicate-branch counts.

### PBT-14 — MEDIUM: the window half of the dependent-metric property never reaches a number

`tests/semi_additive_proptest.rs:800` + `:829-831`: `if pick_window { return Ok(()); }` skips
the numeric comparison, deferring to `window_metric_proptest` — whose inner metric is always
the self-referential `"w"` (`window_metric_proptest.rs:340`). A window wrapping a *different*
(here semi-additive-classified) inner metric has numeric coverage nowhere randomized — the
surviving half of EXP-20's cell. Also `where_clause: None` in this property. **Fix:** oracle
`wbal` with the same correlated-subquery formulation window_metric uses (partition key is
fixed — mechanical), or generate a non-self-referential inner in window_metric_proptest.

### PBT-15 — LOW: no anti-vacuity count for `dsv` selection in star_schema_proptest

`tests/star_schema_proptest.rs:735-780`: the guard counts only predicate branches; nothing
asserts `sel_metrics` ever includes `dsv`/`svf` (contrast differential's
`references_chained_fact`). The file's own doctrine is to assert reach, not assume it.

### TC-14 — LOW: silent early-return escape in the CI-crash replay test

`tests/fuzz_render_roundtrip_regression.rs:63-65`: `let Ok(kb1) = parse_keyword_body(...)
else { return; }` — a future parser change flipping this fixed input to unparseable evaporates
the assertion with no signal (the RT-5 escape shape). **Fix:** pin the parse-ok branch
explicitly.

### TC-15 — LOW (doc-sync): TECH-DEBT.md "Test Coverage Gaps" is stale

Lines 127-163 contain four ancient entries; none of the live gaps with wrong-number history
(PBT-8 FACTS path, PBT-10 role-playing, window/semi pins) has a TECH-DEBT home — they live
only in `_notes/` review docs, the documented "record sits where nobody re-reads" expiry
shape. Suggest one entry per open PBT axis, referenced from harness comments.

### Status of prior test findings

- **PBT-8 — UNTOUCHED.** Every `QueryRequest` literal in every harness still pins
  `facts: vec![]` (8 sites, grep-verified); the row-level FACTS path is guarded only by
  substring unit tests. EXP-28 landed in exactly this cell this round.
- **PBT-9 — PARTIALLY CLOSED.** `dbal`/`dsv` now reach numeric oracles (#203/#207); still
  open: derived expressions fixed at `* 2`, no derived-over-derived, no derived metric in
  multi_hop/child_grain, window-over-derived numeric cell (PBT-14).
- **PBT-10 — UNTOUCHED.** No harness generates two edges to one table
  (`tests/common/mod.rs:242-259`); the feature with the worst regression history (EXP-4/5,
  EXP-10, F-18/T-15) still has zero randomized coverage. Highest-value open investment.
- **PBT-11 — MOSTLY UNTOUCHED.** Window pins (`order_by`/`frame_clause`/`extra_args`/
  self-referential inner/`["w"]`-only requests), semi pins (`nulls` default/1 NA dim/1 table),
  cardinality pins (M:1, 1-col keys), YAML `output_type: None` ×3 + `resolution_schema_name`
  reset — all unchanged. Closed sub-items: relationship names now generated (RT-5), `arb_expr`
  now emits escape strings (PARSE-7).
- **PBT-12 — hostile-identifiers × numeric untouched; topology partially improved**
  (child_grain adds the downward-LEFT-JOIN axis; sibling-fan/fan-in/diamond remain
  fixed-example-only).
- **TC-12 — all four sub-items untouched** (assertion-free `test_semicolon_in_name` arms;
  two empty-expectation `statement error` blocks in `quick_260430_vdz_leading_comments.test`;
  stale "not yet written" comment in expand_proptest; tests_fact_query substring assertions).
- **TC-13 — untouched.** `per_grain_role_playing.test:104` still pins degraded behaviour whose
  only record sits inside **resolved** TECH-DEBT #36; the sibling `phase39` site was correctly
  converted to reference open #51, so the rule is known — this one site was missed.

### Verified clean (test suite)

TEST_LIST ↔ disk exact (106/106) with `check-test-list`/`check-fuzz-list` wired into
`just ci`; all 9 fuzz targets carry real oracles or documented no-panic contracts (no
return-without-asserting remains post-RT-5); all 18 `src/expand/tests_*.rs` wired into
`mod.rs`; no `#[ignore]`; no tautological assertion shapes; `proptest-regressions` all match
live generators; `_excluded/` rationales recorded with named alternate coverage; MAINTAINER.md
fuzz-table/src-tree/oracle-paragraph current — doc-sync discipline held through #203–#209.

---

## 8. Still-open ledger (from 2026-08-06, re-confirmed at this HEAD)

CAT-5 (case-respelled schema defeats byte-equal PK; reads take `rows[0]`), CAT-6
(`DROP SCHEMA CASCADE` orphans definitions), QRY-1 (unbounded bind-time recursion via replaced
SQL view), QRY-2 (`EmptyRequest` before existence), FF-12 (unclamped `reserve()`), FF-13
(hardcoded `explain_semantic_view:` prefix in shared serializer), FF-14 (interior-NUL
truncation), IDENT-1 (exact-string `alias_to_table_map` — re-executed, still misses `O` vs
`o`), IDENT-2..5, PARSE-9. **None has a TECH-DEBT entry yet**, despite the 2026-08-06 review
calling for entries "in the next change that touches the area" — and #203–#209 touched
adjacent areas. Recording them is a one-commit task and should precede or accompany the next
fix round.

---

## 9. Suggested priority order

1. **EXP-25/26/27** — one generalized-PK-guard change plausibly clears 25+26; 27 is a
   regression from the current unreleased line and should not ship in v0.12.0.
2. **RT-8 + MODEL-1 + RT-7** — one validation-hoisting change at the YAML choke point covers
   all three (plus RT-9 as a rider); these corrupt models silently through the documented
   dump/restore path.
3. **PARSE-10** — silent wrong number from a valid definition; small parser fix.
4. **EXP-28/29 + PBT-13/PBT-8** — fix the facts/dims-only anchoring *with* the harness
   coverage that would have caught it (facts generation + where_clause in child_grain), per
   the numeric-oracle rule.
5. **PARSE-11/12** — silent injection-disable and literal corruption; both small.
6. **PBT-10** (role-playing randomization) — the largest remaining predicted blind spot.
7. The LOW/INFO tail (CAT-7/8, FF-15/16, LIFE-2, IDENT-6, PARSE-13, EXP-30, TC-14/15) and
   the TECH-DEBT ledger entries for §8.
