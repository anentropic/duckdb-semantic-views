# Code Review — 2026-07-18

**Scope:** Full codebase review at v0.10.4 (~43k lines of Rust in `src/`, ~40% production), on branch `claude/project-code-review-lxmki7`. Requested focus: architecture; code quality; correctness of SQL parsing; correctness of query expansion / query results; property-based-test quality; test coverage (that tests exist, are run by CI, and are not silently skipped).

**Method:** Five parallel review passes, one per dimension (architecture & code quality; SQL parsing correctness; query-expansion correctness; PBT/fuzz quality; CI & test-coverage wiring). Each pass read the actual code and traced concrete inputs rather than pattern-matching. The three headline findings were then independently spot-verified against source by the reviewer: the fan-trap check loops (`src/expand/fan_trap.rs:62-193`), the absence of a dollar-string token in the body-parser lexer (`src/body_parser/lexer.rs`), and the missing fuzz seed/corpus directories (against the latest `main` CI run logs). No fixes have been applied — this document is a review, not a remediation plan.

**Prior review:** `_notes/code-review-2026-07-16.md`. Several of that review's findings appear addressed in the current `## [Unreleased]` CHANGELOG section — notably F-1 (`NON ADDITIVE BY` polarity, now "corrected to match Snowflake (breaking)"), the parser-residue class (F-2/F-3/F-4, cf. `test/sql/cr20260716_parser_residue.test`), the window dotted/quoted reference fixes, and the F-5…F-9 Snowflake-portability items (trailing `COMMENT`, alias-less TABLES, `PUBLIC` on dims, `WITH SYNONYMS` without `=`, `DESC`). This review was conducted independently of that remediation and reports fresh findings; where a finding overlaps prior lineage it is noted inline.

---

## 1. Executive summary

This is an unusually disciplined codebase for its size. The architecture is a clean layered DAG, the FFI seam is exemplary (catch_unwind at every `extern "C"` boundary, RAII buffer ownership, a `#[repr(transparent)]` connection newtype backed by a `compile_fail` doctest *and* an AST-walking CI test), the parser is hardened against the classic quote/UTF-8/comment traps with no reachable panic found, the property-based tests assert real invariants over a sound differential oracle, and TECH-DEBT.md is accurate and actively curated. Zero TODO/FIXME/HACK markers in `src/`; the production `unwrap()`s are invariant-guarded.

The serious problems are concentrated in one place: **the fan-trap error fence in query expansion has three silent leak-throughs** that return wrong numbers instead of erroring. This matters more than anything else here because the project's whole safety stance is "error rather than silently inflate" — and the *unchecked* metric/dimension combinations are exactly the ones that silently inflate, while the *checked* ones correctly error. That is the worst possible distribution of outcomes. Secondary: the body parser doesn't recognize dollar-quoted strings structurally (one confirmed silent mis-parse, low frequency); 7 of 8 fuzz targets do zero fuzzing in CI due to a swallowed startup error; and the hardest expansion semantics have no randomized coverage.

| # | Cluster | Worst consequence | Where |
|---|---------|-------------------|-------|
| 1 | **Fan-trap fence has three silent leak-throughs** — metrics on a parent/ancestor table, a single multi-grain derived metric, and active semi-additive metrics | Silent inflated aggregates (double/N-counting) for query shapes that *look* safe; the checked neighbours correctly error | `src/expand/fan_trap.rs:62-193`, `src/expand/semi_additive.rs` (EXP-1/2/3) |
| 2 | **Role-playing ambiguity caught one hop deep only** | Dimensions on *descendants* of a role-played table, and facts sourced on one, silently bind to the first-declared relationship (declaration order changes query meaning) | `src/expand/role_playing.rs:75-84`, `join_resolver.rs:132-163`, `sql_gen.rs:216-224` (EXP-4/5) |
| 3 | **Dollar-quoted strings invisible to the body parser's structural scanners** — outer layers (`blank_sql_comments`, `expr_tokens`) handle them, the lexer/`QuoteState` do not | A `,` inside `$$…$$` splits one dimension entry into two garbage dimensions, silently stored; `)`/keywords inside one falsely reject. Low frequency but a real silent mis-parse | `src/body_parser/lexer.rs`, `scan.rs:19-61` (PARSE-1) |
| 4 | **7 of 8 fuzz targets do zero fuzzing in CI** — missing corpus dir → libFuzzer exits instantly → `\|\| true` swallows it → green | The Fuzz workflow is a compile check plus one fuzzed target; confirmed against latest `main` run (7 jobs concluded in 0-4s) | `.github/workflows/Fuzz.yml:43`, `fuzz/seeds/`, `.gitignore:40` (CI-1) |
| 5 | **Hardest expansion semantics have no randomized coverage** — no `proptest!` in `src/expand/`, `src/graph/`; the expand proptest forces `OneToOne` to route the fan-trap path *out* | The exact logic behind cluster 1 is pinned only by fixed examples; a differential star-schema property would catch EXP-1 directly | `tests/expand_proptest.rs:210-217`, `tests/differential_proptest.rs:18-22` (PBT-1) |

**Direct answers to the questions asked:**

**Architecture.** Clean DAG with exactly one cosmetic cycle (`catalog ↔ ddl`, caused by generic FFI-seam types living in `ddl/read_ffi.rs` when they belong in `ffi_util`). Pure logic is systematically separated from `extension`-gated FFI so `cargo test`/clippy/coverage exercise the core. The main quality risk is documentation drift: MAINTAINER.md's architecture section describes a v0.5-era tree (files that no longer exist, 3 fuzz targets vs 8, a phantom "HIERARCHIES" clause).

**Code quality.** High. Bug classes are retired structurally (the `SqlLit` newtype makes forgot-to-escape a compile error; the self-describing wire header turns C++/Rust drift into a loud assert). The one concentration of risk is `expand_semi_additive` (~339-line function, `semi_additive.rs:137-475`) — the crate's longest by 4×, and the file where the most recent behavioural bugs landed; the highest-churn logic is in the least-decomposed function.

**SQL parsing correctness.** Very strong — keywords inside strings/quoted idents are unmatchable by construction, comment blanking is a single length-preserving pre-pass so carets stay valid, UTF-8 handling is systematic, no reachable panic or hang found. One systemic gap (PARSE-1, dollar strings) plus a tail of "accepted-but-wrong, fails later without a caret" validation gaps.

**Query expansion / results.** The correctness surface. The fence catches most fan traps and errors loudly (`FanTrap`/`MetricFanTrap`), alias-shadowing defenses are systematic and correct (GROUP BY ordinals; RANK repeats expressions not aliases), identifier quoting is idempotent and injection-safe. But the three leak-throughs in cluster 1 are silent wrong-answer bugs, and the root-anchored `FROM <root>` topology is a deliberate divergence from Snowflake (which computes each metric at its own grain) that is the root cause of the leak-throughs' blast radius.

**PBT quality.** Structurally sound where it exists — real roundtrip identity over hostile shared generators, a sound `EXCEPT ALL`-inside-DuckDB differential oracle, a bind oracle (`PREPARE … LIMIT 0`) rather than substring checks, checked-in regression seeds tied to real bugs. Gaps: no randomized coverage of fan-trap/semi-additive/window/join-tree; the differential generator never emits NULLs, empty tables, or metrics-only/dims-only selections; one vacuous assertion (materializations asserted equal but never generated).

**Test coverage & CI.** The `just test-all` gate is fully enforced on every push, split across four workflows; TEST_LIST is verified in sync and drift is a hard CI error; no `#[ignore]`/skipif/mode-skip/commented-out tests anywhere. Real gaps: the fuzz workflow (CI-1); no `pull_request` triggers at all (fork PRs get zero CI); ruff configured but never run; the 80% coverage floor measures only default-feature code, so the FFI dispatch layer's coverage is overstated (it is genuinely exercised by sqllogictest + integration, which do run).

---

## 2. Query expansion — confirmed correctness bugs (fix first)

Root cause shared by EXP-1/2/3: generated SQL is always anchored `FROM <root table>` with LEFT JOINs outward, and the fan-trap checker only examines *some* metric/dimension pairings. Where the check runs you correctly get an error; where it doesn't you silently get inflated aggregates.

### EXP-1 — HIGH: metrics on a parent/ancestor table silently aggregate at root grain

`src/expand/fan_trap.rs:62-131`. The met×dim loop pairs a metric's grain tables only against *queried dimensions'* tables; dims with no source table are skipped, same-table pairs are skipped (`fan_trap.rs:89-92`), and the root is never an implicit participant (verified in source).

Repro:
```
TABLES (o AS orders PK(id), c AS customers PK(id))
RELATIONSHIPS (o_c AS o(customer_id) REFERENCES c)   -- ManyToOne
DIMENSIONS (c.segment AS c.segment)
METRICS (c.total_balance AS SUM(c.balance))

semantic_view(sv, metrics := [total_balance])                          -- silent
semantic_view(sv, dimensions := [segment], metrics := [total_balance]) -- silent
```
Traced SQL (both):
```sql
SELECT c.segment AS "segment", SUM(c.balance) AS "total_balance"
FROM "orders" AS "o"
LEFT JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
GROUP BY 1
```
Each customer row is repeated once per order → `SUM(c.balance)` inflated by each customer's order count, and customers with no orders vanish. Adding a dim on `orders` correctly errors `FanTrap` (path `c→o` reverses the M2O edge, `fan_trap.rs:112-115`) — so the checked combination errors while the unchecked one silently returns wrong numbers.

**Fix:** treat the root as an implicit fan-trap participant — for every metric, also run `fanning_edge_on_path` on the metric-table→root path (the reversed-edge test already in use; safe for FK-side metrics, catches parent-metric).

### EXP-2 — HIGH: a single multi-grain derived (or window) metric bypasses the metric×metric check

`src/expand/fan_trap.rs:155-193`. The met×met loop iterates ordered pairs `(i, j)` and skips `i == j` (verified in source), so a *single* metric whose `metric_grain_tables` returns two tables is never checked against itself.

Repro (all validators pass — derived metric has no aggregate of its own):
```
TABLES (o AS orders PK(id), li AS line_items PK(id))
RELATIONSHIPS (li_o AS li(order_id) REFERENCES o)    -- ManyToOne
METRICS (
  o.order_total AS SUM(o.amount),
  li.item_count AS COUNT(*),
  ratio AS order_total / item_count
)
semantic_view(sv, metrics := [ratio])
```
Traced SQL:
```sql
SELECT (SUM(o.amount)) / (COUNT("li"."id")) AS "ratio"
FROM "orders" AS "o"
LEFT JOIN "line_items" AS "li" ON "li"."order_id" = "o"."id"
```
`SUM(o.amount)` is computed over the fanned join — each order's amount counted once per line item — silently inflating the numerator. Querying `order_total, item_count` directly errors `MetricFanTrap` (pinned by `test_check_fan_traps_metric_metric_multi_grain_errors`); folding them into one derived metric erases the protection. Same hole for a window metric whose own/inner grain set spans a fan edge.

**Fix:** in `check_fan_traps`, additionally check all unordered pairs *within* each metric's own grain set, erroring with that metric's name.

### EXP-3 — HIGH (premise acknowledged "unproven" in code): active semi-additive metrics skip the fan check, but the RANK-CTE doesn't neutralize join fan-out

`src/expand/fan_trap.rs:64-79` (skip; the comment itself calls the neutralization assumption "unproven"), `src/expand/semi_additive.rs` (CTE).

Repro:
```
TABLES (o AS orders PK(id), li AS line_items PK(id))
RELATIONSHIPS (li_o AS li(order_id) REFERENCES o)
DIMENSIONS (li.item_name AS li.name, o.report_date AS o.report_date)
METRICS (o.total AS SUM(o.amount) NON ADDITIVE BY report_date)
semantic_view(sv, dimensions := [item_name], metrics := [total])
```
The metric is active semi-additive (NA dim not queried) → fan check skipped → the snapshot CTE runs over the `orders × line_items` fanned join. If one order at the snapshot date has two line items with the same `name`, both rows tie at rank 1 within that partition and `o.amount` is summed twice. RANK ties across fanned duplicates of one source row are indistinguishable from legitimate ties across distinct fact rows (the SG-4 rationale), so the CTE structurally cannot dedupe them. Silent double-counting.

**Fix:** don't skip — run the standard met×dim check for active semi-additive metrics too (same fanned row set), or pre-aggregate the metric's table at its own grain before joining. Flagged in code as SG-4/SG-5 rework; prioritize because it is silent.

### EXP-4 — MEDIUM: dimensions on *descendants* of a role-playing table silently bind to the first-declared relationship

`src/expand/role_playing.rs:75-84` (`find_using_context` inspects only relationships targeting the dimension's *own* table), `src/expand/join_resolver.rs:132-163` (BFS picks first-declared edge for the bare alias).

Model: `f AS flights`; `a AS airports` role-played (`dep: f→a`, `arr: f→a`); `r AS regions` with `a_r AS a(region_id) REFERENCES r`; dim `region_name` on `r`; metric `arrival_count AS COUNT(*)` on `f` `USING (arr)`. Querying `[region_name], [arrival_count]`: `relationships_to_table(def, "r")` returns one relationship → `Ok(None)`, no ambiguity error; join resolution emits bare `a` via the first BFS edge (`dep`), and `r` joins off the departure instance. The grouping is the departure region even though the queried metric said `USING (arr)`. The direct-dimension case correctly errors `AmbiguousPath`; one hop down it is silent and declaration-order dependent.

**Fix:** in `find_using_context`, walk the dimension table's ancestor chain and treat any role-playing ancestor as requiring USING disambiguation (propagate the scoped alias down), or reject.

### EXP-5 — MEDIUM: facts sourced on a role-playing table silently bind to the first-declared relationship

`src/expand/sql_gen.rs:216-224` runs role-playing ambiguity detection for dimensions only; fact source tables go straight to the `fact_needed` path (`join_resolver.rs:389-412`) and join the bare alias via the first BFS edge. `FACTS (a.airport_city AS a.city)` in the flights model silently means "departure city"; declaring `arr` first flips it. A dimension on `a` in the same situation errors `AmbiguousPath`.

**Fix:** apply the same `is_role_playing_target` check to fact source tables in `expand_facts` and error.

### EXP-6 — MEDIUM: quoted stored metric names break resolved-expression keying in the window path

`src/expand/facts.rs:438` keys resolved expressions by `met.name.to_ascii_lowercase()` (quotes retained, since stored names keep their quotes); `src/expand/window.rs:129-144` looks up via `normalize_ident_part` (quotes stripped). A window metric over a quoted-name base metric misses the lookup and falls back to the raw expression — losing fact inlining and, worse, the SG-8 `COUNT(*)`→`COUNT(pk)` rewrite (silent overcount of NULL-extended LEFT-JOIN rows). `collect_transitive_metric_names` (`facts.rs:533`) has the same quoted-key mismatch, which can bypass the `CountStarRequiresPrimaryKey` guard.

**Fix:** key `ResolvedMetricExprs.exprs` / `count_star_no_pk` / transitive-name traversal on `normalize_ident_part` everywhere (one canonical key, as the rest of the codebase already migrated to).

### EXP-7 — LOW: quoted stored names leak literal quote characters into output columns and mat references

Select-item aliases are `quote_ident(&dim.name)` with the stored name retaining its quotes (`sql_gen.rs:453-457`, `semi_additive.rs:223`, `window.rs:163`; also `materialization.rs:122-137`). A dimension declared `"order date"` emits `... AS """order date"""` — the result column is literally named with quote characters, and a routed materialization would require a physical column literally named `"order date"` (with embedded quotes), so routed queries for quoted names bind-fail. **Fix:** strip stored-name quotes before `quote_ident` at emission sites.

### EXP-8 — LOW: met×dim fan check no-ops for empty-grain metrics, unlike met×met which substitutes the root

`fan_trap.rs:81` vs `143-153`. `metric_grain_tables` returns `[]` for a `source_table: None` metric with no base-metric references (legacy single-table `count(*)`); the met×dim loop then checks nothing while met×met maps `[]` to `[root]`. Reachable only via legacy catalog rows / library callers today, but it is the same asymmetry that produced EXP-1/2. **Fix:** map empty grain → root grain in the met×dim loop too (fold into EXP-1).

### Expansion — design decisions worth documenting (not bugs)

- **Materialization staleness:** routing trusts the user table verbatim (no freshness/content check) and runs *before* the fan-trap/ambiguity/SG-8 checks (`sql_gen.rs:321-328`), so a query that would error under live expansion can succeed against a mat table, and a stale table silently diverges. Document that routed results reflect the table as-is.
- **Cardinality inference is declaration-trusting** (`graph/cardinality.rs:87-117`): `OneToOne` from declared PK/UNIQUE, never verified against data — a wrong UNIQUE declaration turns a real fan trap "safe" with no runtime guard. Document.
- **HUGEINT→BIGINT normalization** (`query/wire.rs:195-221`): a genuinely >64-bit sum raises a runtime cast error rather than returning the wide value.
- **Root-anchored FROM** is the single biggest semantic divergence from Snowflake (which joins only the tables a query needs and computes each metric at its own grain); it is the root cause behind EXP-1/2/5's blast radius. Worth an explicit statement in the docs/architecture notes.

---

## 3. SQL parsing

### PARSE-1 — MEDIUM: dollar-quoted strings are invisible to the body parser's structural scanners

The comment blanker (`src/util.rs:174-220`) and the expression tokenizer (`src/expr_tokens.rs:154`) both understand `$tag$…$tag$` via the single-source `read_dollar_tag_len` (documented "matching PostgreSQL/DuckDB", `util.rs:104`), but the lexer (`src/body_parser/lexer.rs`) has no dollar-string token and `QuoteState` (`scan.rs:19-61`) tracks only `'`/`"`.

Dollar-quoting is a genuine DuckDB feature (verified against [DuckDB → Literal Types](https://duckdb.org/docs/current/sql/data_types/literal_types): both plain `$$…$$` and tagged `$tag$…$tag$`), and since dimension/metric expressions are DuckDB SQL fragments a user can legitimately write one. Consequences:

- **Silent mis-parse:** `DIMENSIONS (o.a AS $$p,q.r AS s$$)` — `split_at_depth0_commas` splits at the comma inside the literal; both halves parse as valid `alias.name AS expr` entries, so two garbage dimensions are stored instead of one. This is the P-1/P-2 silent-mis-parse class the codebase treats as its top bug family.
- **False rejections:** `)` inside `$$…$$` closes a clause early; `COMMENT`/`WITH SYNONYMS` inside one triggers the annotation scanner.

**Likelihood is low** — dollar-quoting inside a semantic-view expression is legal but uncommon (most expressions are column refs and aggregates; a string literal author would reach for `'...'` first), which is why this is Medium not High. **Fix:** add a `DollarString` token to the lexer via the existing `read_dollar_tag_len`, plus a dollar arm in `QuoteState::step`; add lexer/proptest cases with `$$)$$`, `$$,$$`, `$$COMMENT$$`. Both layers already share the tag grammar, so this closes the asymmetry without a second definition.

### PARSE-2…8 — LOW: "accepted-but-wrong, fails later without a caret" validation gaps

- `WITH SYNONYMS = ('a' banana)` silently drops `banana` (`annotations.rs:50-62` uses the ignore-trailing extractor meant for the COMMENT tail).
- `PRIMARY KEY (a b)` stores a column named `"a b"`; `UNIQUE (x y, z)` stores `"x y"`; materialization `TABLE t junk` stores `"t junk"` (`tables.rs:283-300`, `relationships.rs:183-198`, `materializations.rs:172-184`) — no identifier validation on captured columns / TABLE content.
- Relationship aliases accept string-literal tokens: `REFERENCES 'customers'` stores `'customers'` with quotes (`relationships.rs:103-114` via `peek_is_value`).
- `identifier_slot_error` accepts punctuation-laden runs: `DIMENSIONS (a+b.d AS o.x)` accepts source alias `a+b` (`scan.rs:245-259` + `ident.rs:127-145`).
- `SHOW … IN "My View"` splits a space-bearing quoted name mid-quote (`show_clauses.rs:112-115`, `226-228` — `find(char::is_whitespace)` instead of the quote-aware `find_identifier_end` the DESCRIBE path uses). Already tracked as TECH-DEBT #25.
- Two caret-precision nits: `parse_keyword_body` trailing-whitespace offset math (`mod.rs:111-112`) and OVER `ORDER BY` entry anchoring to the OVER keyword rather than the entry (`window.rs:92,259`).

**Fix pattern:** run `identifier_slot_error` (tightened to `is_ident_byte` runs) over captured columns / alias slots / TABLE content; require `Ident` token kinds for alias slots; migrate the SHOW name slots to `find_identifier_end`.

### Parsing — Snowflake fidelity notes

- Required `AS` before the clause list is a deliberate divergence (Snowflake has none — a statement pasted from Snowflake docs is rejected). Documented as this project's syntax; worth an explicit compatibility note given CLAUDE.md's "refer to Snowflake" rule.
- Quoted-identifier case-insensitivity deliberately follows DuckDB, not Snowflake (a documented decision, consistent crate-wide).
- "At least one of DIMENSIONS/METRICS" rejects a FACTS-only body — double-check against Snowflake's exact phrasing.
- No pre-aggregation `WHERE` inside `SEMANTIC_VIEW(...)` (filtering only on the outer query) — a real Snowflake feature with no equivalent; keep it on the documented not-supported list.

No reachable panic or infinite loop was found on malformed input; the `expect`s and fixed-width slices are all dominated by prior ASCII-prefix / peek guards.

---

## 4. Property-based test quality

### PBT-1 — HIGH: no randomized coverage of the hardest expansion semantics

No `proptest!` anywhere in `src/expand/`, `src/graph/`, or `src/query/`. `tests/differential_proptest.rs:18-22` scopes itself to base-table metrics / single grain / integer data; `tests/expand_proptest.rs:210-217` deliberately forces `Cardinality::OneToOne` "so the fan-trap safety check passes" — engineering the fan-trap path *out* of the property. Given §2's leak-throughs live exactly there, this is the highest-value test investment available.

### PBT-2 — MEDIUM: the differential generator excludes the classic aggregation-bug space

`tests/differential_proptest.rs:80-103`: every cell is a small non-negative integer (never NULL), tables are never empty (`rows` is `1..=25`), and selections always include ≥1 dim AND ≥1 metric — so NULL group keys, SUM-over-all-NULL, `COUNT(col)` vs `COUNT(*)` divergence, metrics-only (global aggregate) and dims-only (`SELECT DISTINCT`) paths are never differentially checked. `EXCEPT ALL` already treats NULLs as equal in DuckDB, so the comparator stays sound if cells become `Option<i64>`.

### PBT-3 — MEDIUM: vacuous assertion — `materializations` asserted equal but never generated

`tests/roundtrip_proptest.rs:72-77` asserts `reparsed.materializations == def.materializations`, but `arb_canonical_def` builds with `..Default::default()` (`tests/common/mod.rs:315-322`) so it is always `vec![] == vec![]`. The MATERIALIZATIONS clause has no roundtrip property at all. **Fix:** add a `Vec<Materialization>` arm to the generator, or delete the assertion so it can't masquerade as coverage. (This is precisely the "asserting equality on a field the generator never populates" case now called out in CLAUDE.md's refactoring-discipline rule.)

### PBT-4 — MEDIUM: YAML property misses YAML-hostile scalars and pins half the fields empty

`tests/yaml_proptest.rs`: `arb_table_ref`/`arb_dimension`/`arb_metric`/`arb_fact` hardcode `comment: None, synonyms: vec![], unique_constraints: vec![]`, and `arb_window_spec` hardcodes `extra_args/partition_dims/frame_clause` empty — those YAML fields never roundtrip. `arb_name` contains none of `null`, `~`, `no`, `on`, `yes`, `123`, `1.5`, `a: b`, ` padded `, `line\nbreak` — a serializer that fails to quote `no` would deserialize it as `false` and this suite could not catch it.

### PBT-5 — MEDIUM: five of eight fuzz targets check only "doesn't panic"

`fuzz_json_parse`, `fuzz_yaml_parse`, `fuzz_ddl_parse`, `fuzz_parser_override_ffi` are pure `let _ =`; `fuzz_query_names` uses the weak oracle `fuzz_sql_expand` itself calls out. Cheap upgrades: serde closure (`from_json(to_string(def)) == def`) + define-time validation for JSON/YAML; renderability assertion for `fuzz_ddl_parse`; reuse `is_balanced` for `fuzz_query_names`.

### Top 3 new properties to add

1. **Differential join/fan-trap.** Two-table star (`t(fk,d…,v…)` + `u(id PK,w)` ManyToOne), random rows incl. dangling/NULL fks, metrics on both sides. Oracle: pre-aggregate each side to its grain then join. Reuse the existing `EXCEPT ALL` comparator. Catches EXP-1 directly.
2. **Semi-additive differential.** Random `(entity, ts, balance)` with duplicate timestamps and NULLs; `SUM(balance) NON ADDITIVE BY ts DESC NULLS LAST`; oracle via `arg_max`/window written independently. Catches EXP-3's tie-break/NULL space.
3. **Validation ⇒ renderability ⇒ reparse closure.** For any definition accepted by define-time validation, `render_create_ddl` must succeed and its output must re-parse to an equivalent definition — closes the hole where `fuzz_render_roundtrip.rs:48-55` tolerates both render and re-parse failure.

---

## 5. Test coverage & CI wiring

### CI-1 — HIGH: 7 of 8 fuzz targets do zero fuzzing in CI, silently

`fuzz/corpus/` is gitignored (`.gitignore:40`; only `fuzz_json_parse`'s pre-ignore files remain tracked), `fuzz/seeds/fuzz_render_roundtrip` does not exist, libFuzzer exits immediately on a missing corpus directory, and `Fuzz.yml:43`'s `|| true` swallows the failure. Empirically confirmed against the latest `main` run: one target fuzzed 10m03s, the other seven concluded in 0-4s each with `ERROR: The required directory "fuzz/corpus/<target>" does not exist` — all green. **Fix:** `mkdir -p fuzz/corpus/<t> fuzz/seeds/<t>` before the run (or commit seed dirs — `fuzz_render_roundtrip` needs seeds regardless), replace `|| true` with exit-code handling that distinguishes "crash found" from "failed to start," and cache `fuzz/corpus/` so coverage accumulates.

### CI-2 — MEDIUM: no `pull_request` triggers anywhere

`grep pull_request .github/workflows/*.yml` → zero hits; everything keys off `push`. Same-repo branch pushes get CI, but a fork PR runs zero CI. **Fix:** add `pull_request` triggers to CodeQuality/IntegrationChecks, or explicitly document that fork PRs are unsupported.

### CI-3…6 — LOW

- sqllogictests never run on linux_arm64 (reusable-workflow limitation; binaries ship untested there).
- The real-data (jaffle-shop) DuckLake test is manual-only; CI uses the synthetic `test_ducklake_ci.py`. `Justfile:229` references a nonexistent `test-iceberg` recipe.
- `ruff.toml` exists but ruff is executed nowhere (workflows, Justfile, Makefile, pre-commit). Python style is unenforced.
- `just probe-isolation-workaround` (the canary for retiring the per-file sqllogictest process loop) is manual-only — the workaround can silently outlive its cause.

### Coverage-floor caveat

The 80%-lines floor (`cargo llvm-cov nextest`) measures only default-feature code, so the `#[cfg(feature = "extension")]` halves of ddl/query/parse/catalog aren't compiled under it — the floor overstates coverage of the FFI dispatch layer. That layer's executable coverage is sqllogictest + Python integration, which *do* run in CI, so it is covered, just not by the number.

### What is NOT silently skipped (verified)

TEST_LIST is in sync (77 entries both sides; `diff` clean; drift is a hard CI error mirroring `just check-test-list`). No `#[ignore]`, no `skipif`/`onlyif`/`mode skip` in active `.test` files, no commented-out `#[test]`s, no `pytest.mark.skip`, no feature-gated-off Rust test modules. The two `.test` files under `test/sql/_excluded/` are deliberately outside the runner with documented `cargo test` replacements. Python missing-binary guards fail loudly. The only *undocumented* silent skip found is the fuzz targets (CI-1).

---

## 6. Architecture & code quality

### ARCH-1 — MEDIUM: MAINTAINER.md architecture section is badly stale

`MAINTAINER.md:55-90` lists a source tree of files that do not exist (`catalog.rs`, `expand.rs`, `body_parser.rs`, `ddl_kind.rs`, `parser_trampoline.rs`, `ddl/create.rs|drop.rs|alter.rs|show.rs` — all absent), claims 3 fuzz targets (8 exist), references a "HIERARCHIES" clause with zero occurrences in `src/`, cites `src/parse.rs` (now a directory), and omits `graph/`, `parse/`, `render_ddl.rs`, `render_yaml.rs`, `ident.rs`, `expr_tokens.rs`, `ffi_util.rs`. Its CI-Workflows table is equally stale ("three fuzz targets", phantom PR triggers, `just test-iceberg`). The rest of MAINTAINER.md is accurate — the rot is concentrated in the source-tree/data-flow and CI sections. **Fix:** regenerate from the real tree; consider a CI check that listed paths exist.

### ARCH-2 — LOW: `catalog ↔ ddl` is the one module cycle

`catalog/mod.rs:208` imports `crate::ddl::read_ffi::BorrowedConnection` while `ddl/*` imports `catalog::CatalogReader`. `BorrowedConnection`, `run_dispatcher`, `read_str_arg`, and the wire serializers are seam infrastructure consumed by `query/`, `catalog/`, and `ddl/` alike — they belong in `ffi_util` (or an `ffi_util::conn` submodule). Moving them makes the layering a strict DAG with no behavioural change.

### ARCH-3 — LOW: `expand_semi_additive` is a ~339-line function

`semi_additive.rs:137-475` handles classification, SG-5 validation, CTE emission, snapshot-join collection, and outer-select emission in one body (crate's longest by ~4×). This is the file where the recent behavioural bugs (TECH-DEBT #30/#32) landed — highest-churn logic in the least-decomposed function. **Fix:** extract the CTE-emission and outer-SELECT stages as module helpers (`collect_na_groups` shows the pattern).

### ARCH-4 — LOW: oversized modules are mostly test mass; extraction convention applied inconsistently

`body_parser/mod.rs` is 4,036 lines but ~460 are production (first `#[cfg(test)]` at 462); `parse/rewrite.rs` 3,495/~535; `model.rs` 2,043/~525. There is no genuinely oversized production module — but `expand/` already uses the `tests_*.rs` extraction convention while these keep 1.5-3.5k-line in-file test modules. Applying it would cut those files 4-8×.

### ARCH-5 — LOW: `parse` is the top-level DDL orchestrator, contradicting the nominal pipeline

`parse/create_body.rs:211` → `graph::infer_cardinality`; `parse/native_sql.rs:22-26,206` → `catalog::writes` / `ddl::define`. That places `parse` *above* graph/catalog/ddl, not between body_parser and expand as the stated layering implies. It is acyclic and a coherent consequence of the parser_override design, but undocumented. The accurate picture — "two entry stacks, parse (DDL/write) and query (read), over a shared expand/graph/model core with catalog at the bottom" — deserves one paragraph in the rewritten MAINTAINER.md section (ARCH-1).

### Strengths (verified)

- **FFI seam discipline:** every `extern "C"` entry is `catch_unwind`-wrapped with panic-hardened panic arms; connection ownership is a `#[repr(transparent)]` newtype with a `compile_fail` doctest *and* an AST-walking CI test (`tests/no_long_lived_conn.rs`); buffer handoff via `Box<[u8]>` with a both-or-drop publish contract.
- **Bug classes retired structurally:** `SqlLit` newtype (forgot-to-escape = compile error); self-describing wire header (schema drift = loud assert); `wire_len` rejects rather than truncates u32 overflow.
- **TECH-DEBT.md is accurate and actively curated** — spot-verified entries #25/#19/#26/#27/#29 matched the code; #29 records a reachability analysis that *refuted* a proposed deletion.
- **Shared primitives have single owners** with "do not re-inline" provenance (`util::is_ident_byte`, `blank_sql_comments`, `expr_tokens`, `ident::find_identifier_end`); fail-closed decomposition (`parse_snapshot_aggregate` rejects undecomposable shapes).
- **Alias-shadowing defenses** (GROUP BY ordinals; RANK repeats expressions not CTE aliases) and **injection-safe idempotent identifier quoting** throughout the expansion layer.

---

## 7. Ranked recommendations

1. **Close the three fan-trap leak-throughs** (EXP-1/2/3) test-first — each has a concrete minimal repro above; the differential star-schema and semi-additive properties (PBT §Top-3) are the regression guards.
2. **Fix the Fuzz workflow** (CI-1): create corpus/seeds dirs, drop `|| true`, add the missing `fuzz_render_roundtrip` seeds, cache corpus.
3. **Add the two missing differential properties + NULLs/empty selections** (PBT-1/2).
4. **Teach the body-parser lexer dollar-quoted strings** (PARSE-1) — one shared tag grammar already exists; closes a confirmed silent mis-parse.
5. **Unify metric-name keying on `normalize_ident_part`** (EXP-6) and extend role-playing ambiguity detection to descendants and facts (EXP-4/5).
6. **Housekeeping:** regenerate MAINTAINER.md's architecture/CI sections (ARCH-1), add `pull_request` triggers (CI-2), decompose `expand_semi_additive` (ARCH-3), populate-or-delete the vacuous materializations assertion (PBT-3), wire up or remove ruff (CI-5).

One process observation: an unusual amount of what's *right* here — the error fences, tokenizer-based inlining, structural FFI tests, the honest debt ledger — shows a codebase that learns from its bugs and retires them as classes. The findings above sit almost entirely at the *edges* of that same machinery: places where a fence has a gap, not places with no fence. That is a much better starting position than most projects have.
