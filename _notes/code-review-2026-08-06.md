# Code Review — 2026-08-06

**Scope:** Full codebase review at `ae03567` (post-#201, ~56k lines under `src/`), on branch
`claude/duckdb-semantic-views-review-zxrtpi`, working toward v0.12.0 (v0.10.4 is the last released
version; v0.11.0 is unreleased). Requested focus: another thorough pass over the areas that have
repeatedly produced correctness bugs — the parser, extension/connection handling, generated SQL —
and the test-gap / vacuous-test pattern that let those bugs through.

**Method:** Five parallel review passes: (1) the parsing layer (`body_parser/`, `parse/`,
`expr_tokens.rs`, `sql_lit.rs`); (2) query expansion and SQL generation (`expand/`, `graph/`,
`query/`); (3) catalog, connection and FFI (`catalog/`, `ddl/`, `query/wire.rs`, `lib.rs`,
`ffi_util.rs`, `cpp/src/shim.cpp`); (4) identifier handling, rendering and the model (`ident.rs`,
`render_ddl.rs`, `render_yaml.rs`, `model.rs`, `util.rs`); (5) a test-suite honesty audit
(TEST_LIST sync, vacuous assertions, proptest generator ranges, fuzz-target coverage,
degraded-output pins). Each pass traced findings to concrete inputs. **Verification level is
stated per finding**: the expansion findings were reproduced by driving `expand()` directly, three
of them confirmed end-to-end against the bundled DuckDB with wrong numbers observed; the two
parser criticals were reproduced by executing verbatim copies of the affected functions; the
catalog/render findings were verified by code trace (predicates and emission paths read directly),
not execution. No fixes have been applied — this document is a review, not a remediation plan. No
repo files were modified by the review itself.

**Prior review:** `_notes/code-review-2026-08-03.md`. Its findings are confirmed landed in this
window: EXP-9/10 (#189), PBT-6 (#191), EXP-12 + CAT-1/2 (#192), PAR-1 + REL-1 (#193),
EXP-16/17 + PARSE-4 (#194), EXP-13/14 (#195), EXP-15/18 (#196), CAT-3/4 (#197), EXP-11 (#198),
PAR-2/3/4 + PARSE-3 (#199), PAR-6 (#200), and the member-expression scoping work (#201,
TECH-DEBT #52/#54). This review reports fresh findings; lineage to prior IDs is noted inline.

---

## 1. Executive summary

The pattern of the last three rounds repeats, with a sharper edge: the serious problems are again
silent wrong numbers, and again they sit almost exactly in the cells the test suite does not
randomize. This round the test audit and the correctness passes ran independently, and the audit's
top three predicted blind spots — derived metrics (no numeric oracle at all), semi-additive
combinations (harness pins `nulls`/single-dim/single-table), and the facts-in-WHERE path — are
precisely where the expansion pass found the four confirmed wrong-number bugs. The blind-spot →
bug correlation is now strong enough to treat harness coverage as the leading indicator it has
proven to be.

| # | Cluster | Worst consequence | Where |
|---|---------|-------------------|-------|
| 1 | **EXP-19/20**: semi-additive routing never walks the dependency graph | A derived metric over a semi-additive base (or a window metric with a semi-additive inner) silently discards `NON ADDITIVE BY` — `double_balance ≠ 2 × balance`, numerically confirmed | `src/expand/semi_additive.rs:134-144`, `sql_gen.rs:461-475`, `window.rs:147-197` |
| 2 | **EXP-21**: `COUNT(1)` escapes the SG-8 count-star fence | Constant-argument aggregates on a joined table count the NULL-extended LEFT JOIN row — `COUNT(1)=2` next to `COUNT(*)=1`, numerically confirmed | `src/expand/facts.rs:316-374` |
| 3 | **EXP-22**: unqualified `where_clause` member spliced into re-anchored CTEs | The same predicate text binds against different tables in different CTEs; silently filters the wrong table when column names coincide — numerically confirmed | `src/expand/per_grain.rs:851,755-758,681-683` |
| 4 | **PARSE-5**: search-path injection is comment-blind | An ordinary SQL comment silently disables or corrupts `search_path :=` injection — a live hazard for dbt-duckdb, which prepends a comment to every statement | `src/parse/native_sql.rs:146-156`, `parse/search_path.rs` |
| 5 | **PARSE-6**: `o.comment = 'x'` mis-parses as a COMMENT annotation | User's predicate silently discarded at CREATE; corruption surfaces only as a query-time syntax error | `src/body_parser/annotations.rs:154-196` |
| 6 | **CAT-5/6**: schema case-respelling defeats the byte-equal PK; `DROP SCHEMA` orphans definitions | `CREATE OR REPLACE` inserts a duplicate row and reads resolve the stale one | `src/parse/native_sql.rs:343-354`, `catalog/mod.rs:921-927` |
| 7 | **RT-5/6**: GET_DDL emits unparseable or model-changing DDL for YAML-imported definitions, and the fuzz oracle is structured so it can never notice | The GET_DDL roundtrip contract fails exactly where the fuzz target returns-without-asserting | `render_ddl.rs:146-150,245-251`, `render_ddl.rs:1352-1364` |
| 8 | **PBT-8/9/10**: FACTS queries, derived metrics, and role-playing have zero randomized numeric coverage | Three whole features carry the `where_clause: None` blind-spot shape that let EXP-9/10 through; two of them were hiding this round's Tier-1 bugs | `tests/*` (details §6) |

---

## 2. Query expansion — correctness findings

All six findings in this section were reproduced by driving `expand()` directly; EXP-19, EXP-21
and EXP-22 were additionally executed end-to-end against the bundled DuckDB and the wrong numbers
observed.

### EXP-19 — HIGH: derived metric over a semi-additive base silently discards NON ADDITIVE BY

`src/expand/semi_additive.rs:134-144` (`is_active_semi_additive` inspects only the metric's own
`non_additive_by`); dispatch at `src/expand/sql_gen.rs:461-475`.

The snapshot-CTE routing predicate is evaluated per *requested* metric and never walks the
dependency graph. A derived metric (`source_table == None`, empty `non_additive_by`) referencing a
semi-additive base metric classifies as regular; `inline_derived_metrics` splices the base
metric's raw aggregate in, and the base-anchored path aggregates over **all** rows instead of the
RANK-1 snapshot. Nothing errors — contrast the SG-5 co-query guard, which only fires when the
semi-additive metric itself is in the request.

Repro: `accounts(id, customer_id, report_date, balance)` with rows (cust 1: 100@Jan, 150@Feb;
cust 2: 50@Jan, 70@Feb); metrics `balance AS SUM(balance) NON ADDITIVE BY (report_date)` and
`double_balance AS balance * 2`. Querying `balance` by `customer_id` → 150 / 70 (snapshot,
correct). Querying `double_balance` emits `SELECT customer_id, (SUM(balance)) * 2 … GROUP BY 1`
and returns **500 / 240** instead of 300 / 140 — verified end-to-end. So
`double_balance ≠ 2 × balance`, a self-contradiction independent of Snowflake parity.

Minimum correct behaviour: error (like SG-5). Snowflake-parity behaviour: snapshot, then compose.

Coverage: none. `semi_additive_proptest.rs` generates no derived metrics;
`tests_derived_metric.rs` and `test/sql/*.test` never combine a derived metric with a
semi-additive base. `test_co_query_derived_metric_errors` covers only the co-query direction.

### EXP-20 — HIGH: window metric with a semi-additive inner metric — same root cause

`src/expand/window.rs:147-197`; routing root cause as EXP-19 (`sql_gen.rs:461-463`).

`expand_window_metrics` builds `__sv_agg` as `SELECT dims, <inner metric expr> … GROUP BY dims`.
When the inner metric carries `NON ADDITIVE BY`, its resolved expression is still the plain
aggregate, so the window function runs over a non-snapshot aggregate.
`body_parser/metrics.rs:296` rejects `OVER` + `NON ADDITIVE BY` on the *same* metric, but nothing
guards the inner reference.

Repro: same `accounts` fixture; `rolling AS AVG(balance) OVER (PARTITION BY customer_id)` with
`balance` semi-additive. The emitted CTE contains `SUM(balance) AS "balance"` — all report dates
summed (250/120 feeding the window) instead of the 150/70 snapshots. Emitted SQL confirmed; the
numbers follow from EXP-19's verified data.

Coverage: none. `window_metric_proptest.rs`'s inner metric is always self-referential (§6).

### EXP-21 — HIGH: `COUNT(1)` (and any constant-argument aggregate) escapes the SG-8 fence

`src/expand/facts.rs:316-374` (`rewrite_count_star` matches only the literal `*`), applied at
`facts.rs:524-541`.

SG-8 rewrites `COUNT(*)` on a non-base source table to `COUNT("<alias>"."<pk>")` (or errors when
no PK exists) precisely because every synthesized join is a LEFT JOIN and childless base rows
produce one NULL-extended child row. `COUNT(1)` — and any other NULL-insensitive
constant-argument aggregate, e.g. `SUM(1)` — has the identical failure mode but is neither
rewritten nor recorded in `count_star_no_pk`, so it passes every guard and over-counts by one per
childless parent. The per-grain path is unaffected; only the base-anchored path is wrong.

Repro: `orders(1,'open'),(2,'open')`; `line_items(10, order_id=1, …)` (order 2 childless);
metrics `n_one AS COUNT(1)` and `n_star AS COUNT(*)`, both on `li`; dims `[status]`. The emitted
SQL keeps `COUNT(1)` verbatim next to `COUNT("li"."id")`; executed result: **`n_one = 2`,
`n_star = 1`** — verified end-to-end.

Coverage: `tests_count_star_rewrite.rs` and `test/sql/count_star_left_join.test` cover only the
`COUNT(*)` spelling. None of the five numeric harnesses generate constant-argument aggregates.

### EXP-22 — HIGH: unqualified `where_clause` member binds against the anchor table in re-anchored CTEs

`src/expand/per_grain.rs:851` (`is_eligible` declines unqualified *dims* only), `per_grain.rs:755-758`
and `681-683` (window / snapshot anchors, same asymmetry); predicate injection at
`per_grain.rs:1005-1008` / `1136` and `semi_additive.rs:297-300`; member skip at
`fan_trap.rs:673-675` (`member_table == None` ⇒ unchecked).

All three re-anchoring deciders decline a queried dimension with `source_table == None` because
"its binding would move with the anchor" — but apply no such check to `where_clause` members. A
member declared without a source table contributes nothing to `where_tables`, skips
`check_where_clause_fan_traps` and the role-playing check, and its expression is spliced verbatim
into every CTE, including one whose `FROM` is not the base table. The same predicate text then
resolves against different tables in different CTEs.

Repro: `orders2(id, status, customer_id)` base, parent `customers2(id, region, balance, status)`,
dim `status AS status` (no source table), metric `total_balance AS SUM(c.balance)` on `c`. Query
dims `[region]`, metrics `[total_balance]`, `where_clause := 'status = ''active'''`. Single-grain
emission: `FROM "customers2" AS "c" WHERE (status) = 'active'` — binds `customers2.status`.
Executed: returns **50** (the 'active' *customer*) where base-table semantics require the empty
set; with `status = 'open'` it returns nothing where base semantics require 150. Verified
end-to-end. The multi-grain variant emits the same predicate into both grain CTEs — two different
bindings in one statement.

Coverage: every `where_clause` test and every harness WHERE member is table-qualified. The
unqualified-member × re-anchor cell is untested.

### EXP-23 — MEDIUM: `where_clause` fact references skip the inlining pass

`src/expand/where_clause.rs:142-149` — the fact branch of `resolve_where_clause` does
`dim_exprs.entry(key).or_insert_with(|| format!("({})", fact.expr))` with no `inline_facts` /
topological pass, unlike the dimension branch two lines above (which calls
`inline_dimension_facts`, extended by TECH-DEBT #54). Consequentially `source_tables`
(lines 158-173) carries only the member's own table, never tables reached through fact references.

A fact chaining to another fact leaves the inner fact's name in the emitted WHERE as a bare column
(binder error on a valid query); a fact referencing a fact on another table additionally leaves
that table unjoined; and if the base table happens to have a physical column with the inner
fact's name, the filter silently binds to it — a wrong-answer corner.

Repro: facts `base_price AS o.price * (1 - o.discount)`, `total AS base_price * o.quantity`;
metric `revenue AS SUM(total)`. Query `metrics := ['revenue'], where_clause := 'total > 100'`
emits `WHERE (base_price * o.quantity) > 100` while the SELECT list correctly emits
`SUM(((o.price * (1 - o.discount)) * o.quantity))`. Emitted SQL confirmed.

Coverage: `test/sql/where_clause.test` filters on a single-level fact only; no chained-fact or
cross-table-fact WHERE coverage anywhere.

### EXP-24 — MEDIUM: own-qualified derived-metric references — FIND and INLINE disagree

`src/expand/facts.rs:586-597` — the derived-metric replacement map is keyed by **bare** canonical
names only; contrast `facts.rs:290-300` (`insert_fact_keys` inserts bare + own-qualified keys for
facts) and `per_grain.rs:428-436` (`decompose` inserts both keys). Detection sites all use
`references_ref(expr, name, source_table)`, which *does* match the qualified spelling, and
`graph/member_refs.rs:34-37,108-110` documents `t1.metric_a + t2.metric_b` as "the legal
cross-table forms, which must keep working".

So the qualified reference contributes the base metric's table to grains/joins/USING resolution,
but `inline_derived_metrics` leaves the text verbatim and the emitted SQL contains `li.item_rev`
as a raw column. Consequence: binder / "must appear in GROUP BY" errors on a form the validator
explicitly blesses; for a metrics-only query with a same-named physical column, unaggregated
row-per-row output.

Repro: base `o`, child `li` (`li.order_id → o.id`); metrics `item_rev AS SUM(li.price)` on `li`,
derived `double_rev AS li.item_rev * 2`. Query dims `[status]`, metrics `[double_rev]` emits
`SELECT o.status …, li.item_rev * 2 AS "double_rev" … GROUP BY 1` — no aggregation, unresolvable
column. The bare-reference control emits `(SUM(li.price)) * 2` correctly. The multi-grain path
handles the same spelling correctly via `decompose`, so behaviour differs by emission path.

Coverage: none for the qualified spelling on the base-anchored/single-grain paths.
`member_refs` tests assert only CREATE-time acceptance.

### Examined and not flawed

For the cross-check ledger: the EXP-9 sibling hypothesis — a co-queried regular metric fanned by
an active semi-additive metric's NA-dim join — is closed by tree geometry: any NA table that fans
metric *g* while being reachable fan-free from the semi metric's grain *s* forces the fan edge
onto the `g→s` path, which the ordered metric×metric check catches (traced over
`fanning_edge_on_path` + `build_card_map` key orientation). `fanning_edge_on_path`'s
forward-key-first bias is safe because both fence entry points reject cycles (EXP-15).
`where_clause` splice parenthesization, string-literal/dollar-quote safety in the splicer,
GROUP-BY-ordinal alias shadowing (E-1), FULL OUTER JOIN NULL-safe keys, `quote_table_ref`
idempotence, and the wire-format encode/decode all check out with existing coverage.

---

## 3. Parsing layer — correctness findings

Both criticals in this section were verified by executing verbatim copies of the affected
functions; the repro outputs are quoted from those runs.

### PARSE-5 — HIGH: search-path injection is comment-blind — an ordinary comment silently disables or corrupts it

`src/parse/native_sql.rs:146-156` (the `Ok(None)` not-our-DDL branch hands the **raw**, un-blanked
query to the injector); `src/parse/search_path.rs:154-197` (`inject_search_path` uses
`expr_tokens::scan_function_heads`), `:96-121` (`matching_close_paren` uses `QuoteState`, which
has no comment handling), `:166-171` (only *whitespace* is skipped between head and `(`);
`src/expr_tokens.rs:44-46` states the violated precondition: "Expressions reaching this layer are
already comment-blanked… so the tokenizer does not handle SQL comments."

`plan_rewrite` blanks comments for its own analysis, but the read-side injection path does not.
Verified consequences:

- `-- don't touch\nSELECT * FROM semantic_view('v')` → **no injection** (the apostrophe in the
  comment opens a phantom string literal that swallows the function head). Same for
  `/* it's fine */ SELECT …`.
- `SELECT * FROM semantic_view('v' /* x) */)` → the `)` inside the comment is taken as the call's
  close; output is `semantic_view('v' /* x, search_path := list_concat(...)) */)` — the argument
  landed **inside the comment**; the query executes with no path.
- A comment between `semantic_view` and `(` defeats the whitespace-only skip — no injection.
- Control `SELECT * FROM semantic_view('v')` injects correctly.

Downstream (per `src/catalog/mod.rs:916-955`): with no path supplied, resolution falls back to
"unique match, else ambiguity error". With a view name present in ≥2 schemas, adding a comment
above a working query flips it into an ambiguity error; with a single off-path match, the query
resolves a view DuckDB's search-path rule would report as nonexistent. dbt-duckdb — explicitly
supported via `quick_260430_vdz_leading_comments.test` — prepends a comment to *every* statement
and survives today only because its JSON annotation happens to contain balanced apostrophes and no
`)`-relevant content.

Fix shape: scan the length-preserving `blank_sql_comments` output and splice at those offsets into
the original text (offsets are preserved by the blanking contract).

Coverage: the read-side comment path is entirely untested — no injection test contains a comment.

### PARSE-6 — HIGH: `o.comment = 'x'` silently mis-parses into expression `o.` plus a COMMENT annotation

`src/body_parser/annotations.rs:154-196` — the detection loop's boundary check
(`before_ok = i == 0 || !is_ident_continuation(bytes[i-1])` at lines 167/175/183) treats `.` as a
word boundary, so a **qualified column reference** `o.comment` starts the "annotation region" at
the `comment` token, leaving the dangling `o.` as the stored expression. Consumed by every entry
parser (`entries.rs:163`, `metrics.rs:160`, `tables.rs:199`).

Verified behaviours:

- `parse_trailing_annotations("o.comment = 'x'", 0)` → `Ok(expr="o.", comment=Some("x"))`.
  End-to-end: `… DIMENSIONS (o.d AS o.comment = 'x')` stores dimension expr `o.`;
  `graph/member_refs.rs:97-99` skips bare chains, so CREATE **succeeds**; the user's predicate is
  silently discarded and the corruption surfaces only as a query-time syntax error. GET_DDL
  round-trips the corrupted definition.
- `parse_trailing_annotations("o.comment = 'abc' LABELS = (FILTER)", 0)` → a named filter on a
  `comment` column, fully silent.
- `o.comment` alone → `Err("Expected '=' after COMMENT keyword.")`; `o.labels IS NOT NULL` →
  `Err("Expected parenthesized list after LABELS.")` — DuckDB-legal qualified references to
  unquoted columns named `comment`/`labels` are unusable.
- Control: `o."comment"` works (pinned by `test_quoted_comment_column_usable_in_expression`).

DuckDB accepts unquoted `comment`/`labels` as column names, so under the project's dialect rule
this is ours to fix. Minimal fix: `before_ok` must also require `bytes[i-1] != b'.'`.

Coverage: only the quoted workaround is pinned; nothing tests the unquoted qualified form. Not in
TECH-DEBT.

### PARSE-7 — MEDIUM: no scanner in the crate understands DuckDB escape-strings (`e'\''`)

`src/util.rs:177-228` (`QuoteState::step`), `src/body_parser/lexer.rs:127-152`,
`src/expr_tokens.rs:224-238` (`skip_single_quoted`), `src/util.rs:286-295` (`blank_sql_comments`
string state). Found independently by two review passes.

DuckDB accepts Postgres-style `E'...'` strings where `\'` is an escaped quote. All four scanners
treat `\` as an ordinary byte, so in `e'\''` the middle `'` + closing `'` are consumed as a `''`
escape pair and the literal is read as unterminated — or, if another `'` appears later in the
entry, the scanner re-syncs off-by-one, moving commas/keywords in or out of "string" state (the
historical PA-3/P-1 class, with silent mis-split as the worst case).

Repro: `METRICS (o.m AS count_if(x = e'\''))` → "Unterminated string literal in metric entry"
(expected: parses; the expression is valid DuckDB SQL).

Coverage: none; no TECH-DEBT entry mentions escape strings.

### PARSE-8 — LOW: residual `eq_ignore_ascii_case` at CREATE-time validation sites (EXP-12's siblings)

`src/body_parser/mod.rs:257` (EXCLUDING), `:278` (PARTITION BY), `:380-382` / `:403-405`
(materialization dimension/metric checks). Contrast `:305-316` and `:341-343`, which use
`ident_matches` (the EXP-12 fix). (`:217-229`, the NON ADDITIVE BY site, is already recorded in
TECH-DEBT #28 and not re-counted.)

`PARTITION BY EXCLUDING "Region"` compares the quote characters as data and fails, so CREATE
errors "EXCLUDING dimension '"Region"' not found" even though `ORDER BY "Region"` in the same
clause resolves. The CREATE-time validator now rejects input the expansion layer (post-#28
Slice 3) was fixed to accept. TECH-DEBT #28's closing line ("the reference-tokenizer arc fully
landed") reads as if this were done; these sites are not enumerated there.

### PARSE-9 — LOW: name slots accept identifier garbage DuckDB would reject

`src/ident.rs:127-145` (bare part grammar `[^."]+` accepts `,`, `'`, `)`),
`src/ident.rs:346-386` (`find_identifier_end` stops only at whitespace/`;`/optionally `(`), used by
`src/parse/rewrite.rs:225-239` (RENAME TO), `:69` (DROP/DESCRIBE), `src/parse/create_body.rs:96`.

`ALTER SEMANTIC VIEW v RENAME TO x,y` captures `x,y` as one token and **succeeds**, storing the
literal name `x,y` (reachable afterwards only as `"x,y"`). DuckDB's own parser rejects the
identifier. No injection risk (`SqlLit` escapes at emission); the loose grammar is documented in
`ident.rs`, but silently *executing* with such names rather than erroring like the host dialect is
an accepts-garbage path. No test pins either acceptance or rejection.

---

## 4. Catalog, connection, FFI

### CAT-5 — HIGH: schema drop/recreate under a different case spelling defeats the byte-equal conflict key

`src/parse/native_sql.rs:343-354` (and the YAML sibling at `:452-465`) — `INSERT OR REPLACE`
conflicts on the **byte-equal** `(schema_name, name)` PK; every read/guard predicate folds case
(`lower(schema_name) = lower(...)`). `src/catalog/mod.rs:690-744` (`prepared_lookup`),
`:921-927` (`resolve_in_search_path` takes `rows[0]` on the assumption "the PK rules out
duplicates within one schema"), `:250-259` (the CAT-1 ghost-schema migration path carries unknown
spellings verbatim).

`create_target_schema_expr` canonicalizes to the catalog's *current* spelling at CREATE time, so a
stored spelling can diverge from the current canonical one when (i) a schema is dropped and later
recreated with different case, or (ii) a CAT-1-migrated ghost spelling's schema is later created
in a different case. After that, `CREATE OR REPLACE <schema>.v` inserts a **second** row instead
of replacing; both rows match the folded qualified lookup; `ORDER BY schema_name` is binary, so
`'Analytics' < 'analytics'` puts the **stale** row first.

Repro: `CREATE SCHEMA "Analytics"; CREATE SEMANTIC VIEW "Analytics".v AS …;
DROP SCHEMA "Analytics" CASCADE;` (row survives — CAT-6) `CREATE SCHEMA analytics;
CREATE OR REPLACE SEMANTIC VIEW analytics.v AS <new def>;` → `_definitions` holds
`('Analytics','v')` and `('analytics','v')`; reads resolve the old definition. (Plain `DROP`
heals it — the folded DELETE removes both.)

Verified by code trace (all predicates read directly); not executed end-to-end in the review
container. Coverage: CAT-1 tests cover migration-time canonicalization only; no test exercises
DROP SCHEMA + recreate.

### CAT-6 — MEDIUM: `DROP SCHEMA … CASCADE` orphans semantic-view definitions

Design gap: rows live in `semantic_layer._definitions` and the parser override intercepts only
semantic-view DDL, so the extension never sees `DROP SCHEMA`. Rows persist; `SHOW SEMANTIC VIEWS`
keeps listing views in a schema that no longer exists; `semantic_view('s.v')` resolves the
definition then fails with a raw binder error on the dropped base tables. Snowflake (the semantics
reference) drops schema-scoped objects with the schema. `DROP SEMANTIC VIEW s.v` still works, so
cleanup is possible but manual — and this is the enabler for CAT-5.

Not recorded in TECH-DEBT; per CLAUDE.md's own rule, this degraded state needs an entry in the
same change that acknowledges it, whatever the fix timeline.

### QRY-1 — MEDIUM: constructible unbounded bind-time recursion through a replaced SQL view

`src/query/table_function.rs:242-256` (the LIMIT-0 probe executes the expanded SQL at bind);
`cpp/src/shim.cpp:2724-2849` (`sv_semantic_view_bind` / `init_global`, each opening a fresh
`Connection` and running a full query).

`semantic_view()`'s bind runs `{expanded_sql} LIMIT 0` as a real query on a fresh connection,
re-entering the full parse→bind pipeline. If a base "table" of the semantic view is a SQL view
that itself selects from `semantic_view()` of the same view, each bind recursively binds the next
level; DuckDB's recursive-view depth check cannot see across the boundary because every level is a
new query on a new connection/binder. Nothing bounds the depth.

Repro: `CREATE VIEW w AS SELECT 1 AS x;` → `CREATE SEMANTIC VIEW sv AS TABLES (w AS w PRIMARY KEY
(x)) …;` → `CREATE OR REPLACE VIEW w AS SELECT x FROM semantic_view('sv', dimensions := ['x']);`
(binds against the *old* `w`) → `SELECT * FROM w;` → unbounded recursion, two Connections per
level, until stack/memory exhaustion. EXP-15's cycle fence covers only intra-definition cycles,
not cycles through the DuckDB catalog. No test.

### QRY-2 — LOW: `EmptyRequest` checked before view existence

`src/query/table_function.rs:170-172`; same ordering in `src/query/explain.rs:167-171`.
`SELECT * FROM semantic_view('tpyo_view')` reports "empty request" instead of "does not exist" and
skips the did-you-mean machinery. No test pins this combination.

### FF-12 — LOW (defence-in-depth): C++ wire parsers `reserve()` unclamped counts

`cpp/src/shim.cpp:1251`, `:1334`, `:2785`. The Rust decoder clamps `with_capacity` to what the
buffer could physically hold (FF-6, `src/query/wire.rs:86-92`); the mirrored C++ parsers
`reserve()` counts read straight from the buffer before bounds-checking, so a truncated/corrupt
buffer (only producible by an extension-internal bug) surfaces as `bad_alloc` rather than the
intended "FFI buffer truncated" diagnostic. Same fix shape as FF-6.

### FF-13 — LOW: NULL-list-element errors always attributed to `explain_semantic_view`

`cpp/src/shim.cpp:2322-2327` — `sv_serialise_string_list` is shared by `semantic_view`,
`explain_semantic_view`, and (via `sv_search_path_payload`) every SHOW/DESCRIBE TF, but hardcodes
the `explain_semantic_view:` prefix. `semantic_view('v', dimensions := [NULL])` blames the wrong
function. No test covers a NULL list element on `semantic_view()`.

### FF-14 — INFO: interior NUL in a hand-tampered catalog row silently truncates

`src/catalog/mod.rs:656-668` — `read_column_string` goes through `duckdb_value_varchar` /
`CStr::from_ptr`, stopping at the first NUL. Unreachable via extension-written rows (serde_json
escapes NUL); needs a manual `INSERT` into `_definitions`. Worth a comment at most.

### Verified clean

FFI panic safety (all 17 read dispatchers via `run_dispatcher`, the parser-override trio,
`sv_free_buffer`, the yaml helper and the entrypoint are wrapped in `catch_unwind`; error-buffer
writes truncate on char boundaries). Wire format: checked `wire_len` on the Rust side, bounds-checked
reads and trailing-byte rejection on the C++ side, AR-3 header asserted against the declared
column set, empty-result encoding enforced both sides, endianness explicit. The
`BorrowedConnection` contract (no `duckdb_disconnect` on borrowed handles, `tests/no_long_lived_conn.rs`
structural guard). Parser-hook re-registration dedup (WR-09) pinned by `extension_reload.test`.

Also noted: `src/ddl/list.rs:72-76` has a stale comment claiming the presence probe "shares the
caller's catalog/search-path view" — the per-call `Connection(*context.db)` explicitly does not,
which is the entire premise of TECH-DEBT #19. Doc-only, but it asserts the opposite of the design.

---

## 5. Identifiers, rendering, model

### RT-5 — HIGH: GET_DDL emits structurally invalid DDL for YAML-imported definitions — and the fuzz oracle can never notice

`src/render_ddl.rs:146-150` (relationships: `" AS "` pushed unconditionally, name only when
`Some`), `:245-251` (dimensions) and `:222-228` (facts: `src.` qualifier only when
`source_table` is `Some`). Grammar counterparts: `body_parser/relationships.rs:53-67`
("Relationship name is required") and `body_parser/entries.rs:111-118` ("Expected 'alias.name'
qualified identifier"). Validation gap: `parse/create_body.rs:360-394` + `ddl/define.rs` — nothing
in `rewrite_ddl_yaml_body` → `enrich_definition_for_create` requires either field, and
`model.rs:58-61` documents `source_table: None` as a legitimate state.

So `CREATE SEMANTIC VIEW v FROM YAML $$…$$` with `joins: [{table: c, from_alias: o, fk_columns:
[customer_id]}]` (no `name:`) succeeds, and GET_DDL then renders `     AS o(customer_id)
REFERENCES c` — DDL its own parser rejects. Same for a dimension without `source_table:`.

The reason no fuzzing has caught this: the `fuzz_render_roundtrip` oracle (mirrored at
`render_ddl.rs:1352-1364`) **returns without asserting** when `parse(render(def))` fails — exactly
this case — so the target generates `name: None` via `Arbitrary` but classifies every such break
as "unreachable input" and stays green. And `tests/common/mod.rs` always sets
`name: Some("rel{i}")` (`:237`) and `source_table: Some(...)` (`:259,:285,:310`), so the
roundtrip proptest structurally cannot reach the failing shapes.

### RT-6 — MEDIUM: hostile YAML-supplied values in slots without RT-4 quote protection

`src/render_ddl.rs:147-149` (relationship name), `:223-228` / `:246-250` / `:366-371` (member
`src` and `name`), `:372-381` (USING / NON ADDITIVE BY refs), `:400-434` (materialization slots).

RT-4 added round-trip predicates only for alias/column/source-table slots
(`emit_alias`/`emit_column`/`emit_table`); every other identifier slot is emitted verbatim on the
assumption it is parser-shaped — true for DDL-created views (each slot passed
`identifier_slot_error`), false for YAML imports, which perform no identifier-syntax validation.
Consequences: `name: "my dim"` renders `o.my dim AS …` (parse error); `source_table: "a.b"`
renders `a.b.region AS …`, which **silently re-parses to a different model** (alias `a`, name
`b.region`); `pk_columns: ["a--b"]` passes `column_roundtrips_verbatim` (QuoteState has no comment
awareness) and renders `PRIMARY KEY (a--b)`, which the front door's `blank_sql_comments` pre-pass
then blanks to end-of-line.

RT-5 + RT-6 point at one fix: validate YAML imports with the same
`identifier_slot_error`/name-required rules as the DDL path (or extend the render predicates), and
remove the fuzz oracle's parse-fail escape so the contract is actually enforced. A TECH-DEBT entry
for the YAML→GET_DDL contract is currently missing.

### IDENT-1 — MEDIUM: `alias_to_table_map` is exact-string keyed while alias matching is case-insensitive everywhere else

`src/model.rs:492-497`; consumers `src/ddl/show_entities.rs:94`, `ddl/describe.rs:274,352,433`,
`ddl/show_dims_for_metric.rs:161,199`.

CREATE validation matches aliases case-insensitively (`check_source_tables_reachable` lowercases
both sides) and query-time resolution uses `ident_matches`, but the map is keyed on
`t.alias.clone()` exactly as stored and all six lookups use the member's stored qualifier
spelling. A definition where the member qualifier's case differs from the TABLES alias is fully
valid and queryable, but every SHOW/DESCRIBE surface resolves its `table_name` to
`.unwrap_or_default()` = **empty string**.

Repro: `TABLES (O AS orders PRIMARY KEY (id)) DIMENSIONS (o.region AS o.region) …` → CREATE
succeeds, queries work, `SHOW SEMANTIC DIMENSIONS` shows `table_name = ''`. This is the literal
"HashMap keyed by exact string while matching uses ident_matches" divergence. No test uses a
case-mismatched alias/qualifier pair. (Distinct from TECH-DEBT #28, which is quote-stripping.)

### IDENT-2 — MEDIUM: tokenizer splits a qualified reference at a spaced (or comment-blanked) dot

`src/expr_tokens.rs:276-284` (`scan_chain` continues only across a dot *immediately* followed by a
part; pinned as desired at `:495-499`).

SQL allows whitespace around the qualifier dot, and the comment pre-pass **manufactures** it:
`o./*c*/np` is blanked (length-preservingly) to `o.     np` before storage, which the tokenizer
then reads as two chains (`o`, `np`). The bare tail `np` matches a declared fact/metric and
`inline_references` splices over just the tail: `FACTS (o.np AS o.price)`,
`METRICS (o.m AS SUM(o./*unit*/np))` → stored `SUM(o.     np)` → inlining yields
`SUM(o . (o.price))` — a query-time syntax error from generated SQL (the E-3 corruption class;
the E-3 guarantee holds only for contiguous dots). The current behaviour is pinned by
`dotted_chain_is_not_split_by_whitespace` *as desired*; no end-to-end test covers a comment or
whitespace inside a qualified reference.

### IDENT-3 — MEDIUM: cast/EXTRACT positions scan as references — a member named `date`/`year` corrupts sibling expressions

`src/expr_tokens.rs:155-192` — no `::` / `CAST(… AS t)` / `EXTRACT(f FROM …)` context awareness.
The type name after `::`, the target of `CAST(... AS t)`, and the field in `EXTRACT(year FROM ts)`
are bare identifier chains not followed by `(`, so they are emitted as *references*. If a fact or
metric with that normalized key exists, FIND reports a dependency — which since PAR-6 also
**joins the fact's table** — and INLINE substitutes into the type position.

Repro: `FACTS (o.date AS o.order_date)`, `METRICS (o.m AS COUNT(*) FILTER (WHERE o.ts::date =
DATE '2026-01-01'))` → fact `date` inlined → `o.ts::(o.order_date)` → binder/parser error at
query time. Weaker variant: the phantom dependency alone changes join topology. No test exercises
`::`, `CAST`, or `EXTRACT` in `expr_tokens`; the proptest expression atoms have no cast arm.

### IDENT-4 — LOW: a quoted all-digit name (`"0"`) collides with numeric literals — silent wrong number

`src/expr_tokens.rs:455-458` — a comment asserts the false invariant that a numeric-literal chain
"can never be a declared name". Bare names can't start with a digit, but quoted ones can:
`METRICS (o."0" AS SUM(o.x), o.m AS revenue + 0)` — `normalize_ident_part("\"0\"")` = `"0"`, the
same key the literal `0` produces, so `m` classifies as derived referencing metric `"0"` and
inlines it: `revenue + (SUM(o.x))`. Silent wrong number; low likelihood. Both halves of the
mechanism are pinned by existing unit tests.

### IDENT-5 — LOW: quoted-dot single identifier ≡ qualified two-part reference in every match key

`src/ident.rs:407-417` (parts re-joined with `.`), consumed by `ident_matches` (`:430-439`) and
`IdentRef::key` (`expr_tokens.rs:76-78`). `normalize_ident_part("\"a.b\"")` and
`normalize_ident_part("a.b")` both yield `a.b`, so a dimension literally named `"o.region"` is
matched by the qualified reference `o.region` and vice versa — DuckDB treats these as different
objects. `last_part_key` exists precisely because the joined key can't make this distinction for
function heads; references/FIND/INLINE still conflate them. Requires dot-bearing quoted names, so
low practical impact — but the flattened-key design should either encode the distinction or
document the collision.

Also noted (informational): `render_ddl.rs:180-213` (`logical_ident` /
`ref_columns_match_target_pk`) still implements the pre-2026-07-12 Snowflake case rule in its
comments; both failure directions only emit a redundant `(ref_columns)` list, so behaviour is
correct, but the comment documents a superseded convention. `ident_matches`'s ASCII fast path does
not trim while the quoted slow path does (`ident_matches("region", " region ")` false,
`ident_matches("\"region\"", " region ")` true) — no current caller passes untrimmed input; latent
trap.

---

## 6. Test-suite audit

The structural headline: **three whole features carry the exact `where_clause: None` blind-spot
shape that let EXP-9/EXP-10 through** — the field present in every struct literal, never varied —
and two of them were hiding this round's Tier-1 bugs.

### PBT-8 — HIGH: `QueryRequest.facts` pinned at `vec![]` in every harness — the FACTS query path has zero randomized coverage

`tests/differential_proptest.rs:431`, `star_schema_proptest.rs:614`,
`multi_hop_join_proptest.rs:629`, `semi_additive_proptest.rs:513`, `window_metric_proptest.rs:469`,
`expand_proptest.rs:324,538`. No `FactName` import exists anywhere under `tests/`. The row-level
fact path (`src/expand/facts.rs` — unaggregated passthrough, fact-in-fact inlining, the
`FactPathViolation` fence, fan-in direction checks) is exercised only by fixed `.test` rows and
substring unit tests (`tests_fact_query.rs` asserts `sql.contains("LEFT JOIN")` — never executed
against data). A wrong fact inlining or join path returning wrong *rows* would be silent.

### PBT-9 — HIGH: derived metrics never reach a numeric oracle

`tests/parse_proptest.rs:1005-1051` covers derived metrics with *no-panic* properties only. All
numeric coverage is fixed examples (`phase30_derived_metrics.test`, `tests_derived_metric.rs`
substrings, one fixed row-set in `test_integration/test_differential.py`). No harness generates a
metric referencing another metric — which is exactly where EXP-19 and EXP-24 sat.

### PBT-10 — HIGH: role-playing has no randomized coverage in any harness

All five numeric harnesses build at most one relationship per table pair;
`tests/common/mod.rs:236-243` generates at most one join per non-base table, all targeting `t0`,
so no generated definition ever has two edges to the same table. Coverage is fixed-only. Given the
recorded history (EXP-4/5, EXP-10, F-18/T-15 — all silent wrong-role bugs), this is the
highest-risk fully-absent axis: every new member-bearing surface has re-opened the role-playing
seam, and each time only a hand-written test caught (or missed) it.

### PBT-11 — MEDIUM: pinned generator fields across the remaining harnesses

- `window_metric_proptest.rs:337-347`: `order_by: vec![]`, `frame_clause: None`,
  `extra_args: vec![]`, inner metric always self-referential (`"w"`), request always exactly the
  single window metric (`:468`). Ordered/framed windows, `LAG(x, n)`, window-wrapping-a-different-
  metric (EXP-20's cell) and window-mixed-with-plain-metric queries are fixed-example-only. The
  header documents the partition-only scope honestly, but per CLAUDE.md that tier is insufficient
  for number-changing features.
- `semi_additive_proptest.rs:326-333`: `nulls` pinned to the per-direction parser default;
  single NA dimension always; single table always — semi-additive × joins, × role-playing, and
  × derived metrics (EXP-19's cell) are fixed-test-only, in "the file where the most recent
  behavioural bugs landed" (the harness's own words).
- `star_schema_proptest.rs:360` / `multi_hop_join_proptest.rs:359,367` pin
  `Cardinality::ManyToOne`; composite-key joins are numeric-tested only by fixed
  `phase33_cardinality_inference.test` (parse-level composite keys *are* varied).
- `yaml_proptest.rs:111,191,218`: `output_type: None` in all three entry generators — YAML is the
  *only* surface that can set `output_type` (TECH-DEBT #51), so the one field that would populate
  the degraded `data_type` column has zero randomized round-trip coverage. `:192`
  `using_relationships: vec![]`. And `arb_definition:286` pins `resolution_schema_name: None`
  while `yaml_export_roundtrip:330-336` resets only the other three stripped fields — that
  assertion is vacuously green and would fail spuriously (not catch a regression) if the field
  were ever varied.
- `tests/common/mod.rs`: aliases pinned `t{i}` (`:188`), join names pinned `Some("rel{i}")`
  (`:237`), FK/ref columns bare-ASCII only (`:226,:232`), `source_table` always `Some` — the RT-5
  shapes are structurally unreachable; `arb_payload` contains no newline, so multi-line comments
  never round-trip.

### PBT-12 — MEDIUM: hostile identifiers never reach a numeric oracle; fan-trap topology is chains-only

All five numeric harnesses use fixed ASCII member names. Quoted/unicode/keyword identifiers are
varied only for parse/render fidelity — a quoting bug in *expansion* that changes numbers (the
EXP-16/17/PARSE-4 family) has no randomized net. And the only randomized fence coverage is one M:1
hop (`star_schema`) plus a fixed 3-table linear chain (`multi_hop_join`): sibling-fan (the classic
fan trap), fan-in, and diamond topologies are fixed-test-only — no harness randomizes the topology
itself.

### TC-12 — MEDIUM: vacuous / weak individual tests

- `tests/parse_proptest.rs:831-848` `test_semicolon_in_name`: all three match arms are empty — the
  test passes on every non-panicking outcome, while its comment states an invariant ("the
  rewritten SQL must start with `SELECT * FROM` … no raw ';' injected") that nothing asserts.
- `test/sql/quick_260430_vdz_leading_comments.test:75-83`: the only two `statement error` blocks
  (of 217) with an empty expected message — any error from any cause passes.
- `tests/expand_proptest.rs:326-331`: `where_clause: None` with a stale comment claiming the
  where_clause property coverage is "not yet written" — it has since been written (PBT-6).
  Misleading to a future reader deciding whether coverage exists.
- `src/expand/tests_fact_query.rs:43` and siblings: substring assertions (`contains("LEFT JOIN")`)
  as the only guard on the facts path (see PBT-8).

### TC-13 — LOW (process): a degraded-behaviour pin whose only record sits inside a *resolved* TECH-DEBT entry

`test/sql/per_grain_role_playing.test:104` pins an error "until USING context is threaded into the
grain CTEs", cross-referenced to TECH-DEBT #36 — but #36 is marked ✅ RESOLVED, and the residual
("still declined, deliberately: a `where_clause` member on a role-played table, a metric's own
grain table on one, descendants; the window path stays strict") lives inside the resolved entry.
This is the same expiry shape as the Plan 05 / `data_type` incident CLAUDE.md documents: a passing
assertion whose open-item record sits where nobody re-reads. Promote the residue to its own OPEN
entry.

### Verified clean

TEST_LIST ↔ directory sync is exact (100/100) and CI-enforced (`just check-test-list`); the two
`_excluded/` files have documented runner limitations and verified alternate coverage. All 217
`statement error` blocks use block form. No `mode skip`. All 12 empty `query` outputs are
intentional (with positive controls where it matters). All five numeric oracles are structurally
independent of the implementation, and `semi_additive_proptest.rs:658-726` *proves* its oracle
structure is load-bearing. Every PBT-6 harness carries an anti-vacuity generator guard asserting
each property branch is reached — this is the model pattern the PBT-8/9/10 gaps should adopt.
Fuzz: 9 targets, three-way registration guarded by `just check-fuzz-list`; all major parse entry
points covered (the ALTER `SET COMMENT` JSON merge-patch has no dedicated target — rides
serde_json; acceptable).

---

## 7. Suggested priority

1. **EXP-19/20/21/22** — the confirmed wrong-number bugs, test-first per the discipline. Each fix
   must un-pin its harness cell in the same change: derived-over-semi-additive and window-inner
   need derived-metric generation in the semi-additive/window harnesses (PBT-9/11);
   constant-argument aggregates need generator arms in the join harnesses; unqualified WHERE
   members need `source_table: None` members varied where re-anchoring can trigger.
2. **PARSE-5/6** — the parser criticals. PARSE-5 is a live dbt-duckdb hazard; the fix shape
   (scan blanked text, splice into original) is mechanical. PARSE-6 is a one-byte boundary fix
   plus tests for unquoted `comment`/`labels` columns.
3. **EXP-23/24, RT-5/6** — wrong-error on valid input and the GET_DDL roundtrip contract; RT-5's
   fix must also remove the `fuzz_render_roundtrip` parse-fail escape and un-pin the
   `common/mod.rs` `Some(...)` fields so the contract is enforced going forward.
4. **CAT-5/6, QRY-1** — TECH-DEBT entries at minimum in the next change that touches the area
   (CAT-6 and QRY-1 may reasonably be deferred as documented limitations; CAT-5 has a concrete
   fix: make the write path's conflict handling case-fold like the read path, e.g. delete-by-folded-
   key before insert, plus a startup dedup for already-diverged rows).
5. **PBT-10 (role-playing) and PBT-8 (FACTS)** — the two highest-value harness investments;
   role-playing has an empty coverage row and the worst regression history.
6. Tier-3 consistency items (IDENT-1..5, PARSE-7/8/9, FF-12/13, stale comments) as opportunistic
   fixes with tests, or explicit TECH-DEBT entries where deferred.

## 8. Feature × harness coverage matrix

Legend: **V** = varied by generator, **C** = present but constant/pinned, **–** = absent.
Columns: diff = differential, star = star_schema, hop = multi_hop_join, semi = semi_additive,
win = window_metric, exp = expand, rt = roundtrip (+common), cfd = create_front_door (+common),
yam = yaml, par = parse, out = output.

| Feature | diff | star | hop | semi | win | exp | rt | cfd | yam | par | out |
|---|---|---|---|---|---|---|---|---|---|---|---|
| where_clause | V | V | V | V | V | C (None) | – | – | – | – | – |
| FACTS request | C (empty) | C | C (empty) | C | C | C | V (parse) | V (parse) | V (parse) | C (no-panic) | – |
| derived metrics | – | – | – | – | – | – | – | – | – | C (no-panic) | – |
| window metrics | – | – | – | – | V (partition only) | – | – (pinned None) | – (pinned None) | V (parse) | C (fixed) | – |
| semi-additive | – | – | – | V (order; nulls/dims/tables C) | – | – | V (parse) | V (parse) | V (parse) | – | – |
| role-playing | – | – | – | – | – | – | – | – | – | – | – |
| multi-hop joins | – | – (1 hop) | V (fixed 3-chain) | – | – | C | C (all target t0) | C | V (parse) | – | – |
| composite keys | – | C (1-col) | C (1-col) | – | – | – | V (parse) | V (parse) | V (parse) | – | – |
| NULL data | V | V (+dangling FK) | V (+dangling FK) | V | V | – | n/a | n/a | n/a | n/a | C (unit) |
| hostile identifiers | – | – | – | – | – | – | V | V | V | V | – |
| comments/annotations | – | – | – | – | – | – | V (no newline) | V | V | C (fixed) | n/a |
| per-grain | – | V (2 grains) | V (3 grains) | – | – | – | – | – | – | – | – |
| fan-trap topologies | – | V (1-hop) | V (2-hop chain) | – | – | – | – | – | – | – | – |

Reading: the **role-playing row is empty**, the **derived-metrics row has no numeric cell**, and
the **FACTS row is all-C/– on the numeric side**. Those three are the `where_clause`-shaped blind
spots still open; the topology axis (sibling-fan/diamond) and the hostile-identifier × numeric
cell are the next tier.
