# Code Review — 2026-08-03

**Scope:** Full codebase review at `f003605` (post-#188, ~53k lines of Rust in `src/`), on branch
`claude/codebase-review-duckdb-semantic-st7vts`. Review window **v0.10.4 → HEAD** — the last
in-depth review was `_notes/code-review-2026-07-18.md` at v0.10.4; v0.11.0 is the last pushed tag.
Requested focus: changes since the last release; correctness and Snowflake feature parity (under
the CLAUDE.md rule that Snowflake defines semantics and DuckDB defines dialect conventions); rough
edges and messy code; test coverage.

**Method:** Five parallel review passes: (1) commit-by-commit diff review of `v0.10.4..f38c3e5`
(27 commits — the per-grain aggregation engine, fan-trap fence leak fixes EXP-1/2/3, graph/identifier
hardening); (2) diff review of `f38c3e5..HEAD` (23 commits — `where_clause`, `LABELS = (FILTER)`,
multi-grain window/semi-additive/role-playing extensions, SHOW `IN` scope grammar, schema scoping +
search-path resolution, `GET_DDL` third argument); (3) Snowflake parity audit against docs and code;
(4) code-quality pass over all of `src/`; (5) static test-coverage audit (test surface, TEST_LIST
sync, proptest/fuzz invariants, CI wiring). Each pass read actual code and traced concrete inputs;
findings below were verified against source at HEAD, not just diff hunks. No build was run in the
review container; where that caveat matters it is noted. **No fixes have been applied — this
document is a review, not a remediation plan.**

**Prior review:** `_notes/code-review-2026-07-18.md`. Its headline findings are confirmed fixed in
this window: EXP-1/2/3 (fence leak-throughs) landed in b321eea; EXP-4/5 (role-playing beyond one
hop) in 5b3c999; EXP-6 (canonical identifier keys) in 9e0ed6d; PARSE-1 (dollar-quoted strings) in
675b027; CI-1 (fuzz targets doing zero fuzzing) in a409217; CI-2 (no `pull_request` triggers) in
d14d459; PBT-1 (no randomized expansion coverage) via the differential proptests in 4cb0111 /
535ea85 / 5c52305; and the root-anchored topology itself was replaced by per-grain aggregation
(TECH-DEBT #35/#36, f38c3e5). This review reports fresh findings; lineage is noted inline.

---

## 1. Executive summary

The overall trajectory since v0.10.4 is strongly positive: every headline finding of the prior
review was fixed, the root-cause topology divergence (root-anchored `FROM <base>`) was replaced
with Snowflake-matching per-grain aggregation, and the new work ships with independently-oracled
differential proptests. Discipline remains unusually high — near-zero panic surface, type-enforced
SQL escaping (`SqlLit`), exemplary FFI seam, zero TODO/FIXME in `src/`, an honest TECH-DEBT ledger,
and test files that document per-case red verification.

The serious problems are again concentrated in the "silent wrong number" class the project treats
as worst-in-class — and both HIGH findings sit on surfaces excluded from every numeric oracle
(PBT-6), which is why they survived:

| # | Cluster | Worst consequence | Where |
|---|---------|-------------------|-------|
| 1 | **EXP-9**: fan-trap fence never checks the `NON ADDITIVE BY` dimension's own table | Active semi-additive metric ranked across a fanning join silently double-counts | `src/expand/fan_trap.rs:72-116`, `semi_additive.rs:268-287` |
| 2 | **EXP-10**: `where_clause` member on a role-played table binds to the first-declared relationship | Filter applied through the wrong role — can group by one role while filtering by another; no error | `src/expand/where_clause.rs`, `sql_gen.rs:632-636`, `join_resolver.rs:132-163` |
| 3 | **CAT-1/2/3**: schema-scoping migration defects | Duplicate logical rows under `OR REPLACE` with reads resolving to the stale one; cross-schema version stamping; companion-import rows invisible to `SHOW … IN SCHEMA` | `src/catalog/mod.rs` |
| 4 | **PBT-6**: all five differential/numeric proptests hardcode `where_clause: None`; the multi-grain FULL OUTER combiner has no randomized coverage | The two newest correctness-critical surfaces are pinned only by fixed examples — EXP-9/EXP-10 went undetected | `tests/differential_proptest.rs:237` et al., `src/expand/per_grain.rs:1164` |
| 5 | **REL-1**: v0.12.0 declared in CHANGELOG/Cargo.toml but never tagged | Release process half-finished; `sort -V` tooling also confused by a stray legacy `v1.0` tag | repo tags |

**Direct answers to the questions asked:**

**Changes since last release.** Two spans, both unreviewed until now. v0.10.4→f38c3e5 delivered the
per-grain engine plus fence/graph/identifier/parser hardening — the engine's join semantics
(`IS NOT DISTINCT FROM`, COALESCE-chained keys, FULL OUTER) verified correct, the routing predicate
`needs_per_grain` exactly covers the shapes v0.11.0 errored, and the fence checks skipped in
per-grain mode are precisely the base-anchored invariants the new shapes replace. f38c3e5→HEAD
delivered `where_clause`, named filters, the multi-grain extensions, SHOW scope grammar, and schema
scoping — well-built overall, but this is where EXP-10 and the CAT- findings live.

**Correctness / Snowflake parity.** Unusually faithful; divergences are almost all deliberate,
reasoned, and documented, and the per-grain semantics were empirically probed against Snowflake
(TECH-DEBT #35/#36). Gaps: doc drift on the highest-traffic query reference page (PAR-1), empty
`data_type` in the SHOW entity listings since v0.10.0 (PAR-2), an undocumented no-cross-table-
column-references rule in member expressions (PAR-3), and `PRIVATE` rejected on dimensions without
a recorded rationale (PAR-4).

**Rough edges.** The crate has ≥6 independent quote-state walkers kept in sync only by comments,
and the two expansion-side copies have already drifted (EXP-16/17). `expand()` is a 336-line
strategy dispatcher whose flag interactions have already produced one ordering bug (ARCH-7). ~124
ad-hoc `to_ascii_lowercase()` alias comparisons bypass `ident_matches` while `tables.rs` stores
aliases with quotes preserved (ARCH-8). Test modules in `body_parser/mod.rs` and `parse/rewrite.rs`
were never split the way production was (ARCH-9).

**Test coverage.** Strong baseline: 94/94 TEST_LIST sync CI-enforced, nine proptest harnesses with
genuinely independent oracles, nine fuzz targets all wired, 80% line floor, deliberate
anti-vacuous-generator measures. The gaps are systematic rather than scattershot: PBT-6/PBT-7
(above), the read-only legacy-catalog migration refusal never exercised through a real persisted
database (TC-1), and 746-line `parse/native_sql.rs` with zero direct unit tests (TC-4).

---

## 2. Query expansion — correctness findings

### EXP-9 — HIGH: fence never checks the NON ADDITIVE BY dimension's own table — silent double-count through the snapshot join

`src/expand/fan_trap.rs:72-116` (metric × dimension loop iterates only *queried* `resolved_dims`);
`src/expand/semi_additive.rs:268-287` (NA-dim source tables joined into the snapshot CTE via
`collect_na_dim_source_tables` → `resolve_joins_pkfk`); CREATE-side validation checks only that the
NA dim *exists*, not its grain (`src/body_parser/mod.rs:215-248`).

EXP-3 (b321eea) made active semi-additive metrics subject to the metric×dim and metric×metric
checks — but the table joined *because of the un-queried NA dimension itself* is exempt from every
fence check, so a snapshot ranked across a fanning join silently inflates. This is the exact
mechanism EXP-3's own comment describes ("RANK ties across the fanned duplicates of one source row
are indistinguishable… silent double-count").

Repro (DDL-reachable, verified against validation code):

```
TABLES (o AS orders PRIMARY KEY (id), li AS line_items PRIMARY KEY (id))
RELATIONSHIPS (li_o AS li(order_id) REFERENCES o)      -- ManyToOne, li below o
DIMENSIONS (o.region AS o.region, li.ship_ts AS li.ship_ts)
METRICS (o.balance_at AS SUM(o.balance) NON ADDITIVE BY (ship_ts))

semantic_view(sv, metrics := ['balance_at'], dimensions := ['region'])
```

`ship_ts` is not queried → metric is *active* semi-additive → snapshot path. Fence: metric table
`o` vs queried dim table `o` → pass; `o` == root → pass. `resolve_joins_pkfk` joins `li` (the
NA-dim source), producing `FROM o LEFT JOIN li …` — each order duplicated per line item — then
`RANK() OVER (… ORDER BY li.ship_ts …)`. An order whose two line items tie at the snapshot value
has its `o.balance` summed **twice**. No error, wrong number. The asymmetry is stark: *querying*
`ship_ts` makes the metric regular and the fence then correctly raises `FanTrap` for the same join.

Still present at HEAD: the post-range `snapshot_cte_anchor` (`src/expand/per_grain.rs:634,652-655`)
checks only path *existence* to the NA-dim tables, not fanning, and returns `None` when the metric
is at root grain, leaving the base-anchored fanned join unguarded.

**Fix direction:** NA-dim source tables of active semi-additive metrics should participate in the
metric × dimension check — they are joined exactly like dimensions (the code's own comment at
`role_playing_affects_query` notes the same equivalence for `where_clause` members, which *did* get
their own check in `check_where_clause_fan_traps`; NA dims never did).

### EXP-10 — HIGH: `where_clause` member on a role-played table silently filters through the first-declared relationship

`src/expand/where_clause.rs:71-148` (no role resolution); `src/expand/sql_gen.rs:632-636` (joins
where-tables with no ambiguity check); `src/expand/join_resolver.rs:132-163` (`build_tree_parents`
binds a role-played table to whichever edge is declared first).

On the base-anchored metrics path (and the facts path), a `where_clause` naming a member on a
role-playing table is emitted against the bare table alias joined on the *first-declared*
relationship — silently, with no `AmbiguousPath`-style error, ignoring any co-queried metric's
`USING`. Role-playing ambiguity is checked only for *queried* dimensions (`find_using_context`) and,
on the facts path, queried dims and facts (`sql_gen.rs:242-252`); where-members are in neither set.
The member's table rides `resolve_joins_pkfk`'s `fact_source_tables` parameter
(`join_resolver.rs:310-317`), whose loop — unlike the dimension loop at :269-281 — never consults
`role_playing_bare_aliases`.

Repro: `flights` with `dep`/`arr` relationships to `airports a`, dimension `airport_city` on `a`:

```
semantic_view(sv, metrics := ['cnt'], where_clause := 'airport_city = ''NYC''')
```

filters via the `dep` edge regardless of intent — and if the metric declares `USING (arr)`, the
query *groups* by `a__arr.city` while *filtering* on `a.city` (dep). Wrong rows, no error. The
per-grain planner explicitly declines this shape (`per_grain.rs:517-524` puts where-tables in the
"strict" set) — but declining routes it to the base-anchored path, which emits it silently. Nothing
pins this: every `tests_role_playing.rs` case uses `where_clause: None` and
`test/sql/where_clause.test` has no role-playing case (see PBT-6). The queried-dimension precedent
(EXP-4/EXP-5 fail-loud) shows the intended posture.

**Fix direction:** run the same role-reachability test over where-member tables that queried
dimensions get; error absent a disambiguating `USING`, matching the per-grain path's posture.

### EXP-11 — MEDIUM: per-grain planner mis-anchors root-grain components of a derived metric (EXP-8 class) — RESOLVED 2026-08-05 (TECH-DEBT #50)

`src/expand/per_grain.rs:221-240` (single-grain fast path), `:339-341` (`decompose` skips
source-less metric as "Derived: inlined"), `:407-412` (`rebuild_expr` inlines the raw aggregate);
root cause `src/expand/facts.rs:564-608` (`collect_derived_metric_source_tables` contributes no
table for an aggregate metric with `source_table == None`).

A derived metric depending on both (a) a source-less aggregate metric — which per EXP-8 sits at
root grain, the substitution `check_fan_traps` and `plan()` both make for an *empty* grain set —
and (b) a base metric on a fanning parent table gets a grain set of only `{parent}`: the root
component vanishes. `plan()` takes the `grains.len() == 1` path and anchors the *entire* resolved
expression (both aggregates) at the parent table; the fence runs in per-grain mode, which skips the
EXP-1 root-grain and EXP-2 within-metric checks (`fan_trap.rs:121-124`). Emitted:
`SELECT (SUM(amount)) + … FROM orders AS o` — the base-table column binds against `o`: silently
wrong if `o` has a same-named column, confusing binder error otherwise. v0.11.0 raised
`RootGrainFanTrap` for the same query. In the multi-grain variant, `decompose` splices the raw
`SUM(amount)` into the **outer** SELECT over the grain CTEs.

**Reachability caveat (why MEDIUM):** current DDL cannot create a source-less *aggregate* metric —
`alias.name` qualification is mandatory and `validate_derived_metrics` rejects aggregates in
unqualified entries. The shape exists only in legacy stored catalog rows and constructed
definitions — but that is exactly the population EXP-8 was added to protect; planner and fence are
now inconsistent about it. **Fix direction:** apply the empty→root substitution *per component*
(make `collect_derived_metric_source_tables` treat a source-less aggregate dependency as
contributing the root, or have `decompose` decline when a dependency has `source_table == None`
and an aggregate in its expression).

### EXP-12 — MEDIUM: window inner-metric matching quote-sensitive in fence & CREATE, quote-insensitive in the emitter — RESOLVED 2026-08-04 (TECH-DEBT #43)

**Resolution note.** All four sites migrated to `ident_matches` in one change, as this entry
required. Worth recording what the fix surfaced: the hazard was described here as latent — real
only once the CREATE check moved — but it was already active for any definition carrying a quoted
reference. Measured directly: the quoted spelling anchored `__sv_agg` at the base table and
computed the inner aggregate over a fanned join (140 → 340) while the unquoted spelling anchored at
the aggregate's own grain. The CREATE check's strictness was the only thing keeping that
unreachable through DDL, which is exactly why the sites had to move together.

`src/expand/fan_trap.rs:402-406` (`metric_grain_tables` resolves `ws.inner_metric` via
`eq_ignore_ascii_case`); `src/body_parser/mod.rs:331-349` (CREATE inner-metric check, same) and
`:300-329` (window `ORDER BY` dim check, same); versus `src/expand/window.rs:141,163`, which
resolves the same reference via `normalize_ident_part`/`ident_matches`. The pattern was also copied
into the post-range `per_grain.rs:703,709` (`window_cte_anchor`).

Per CLAUDE.md, identifier matching must follow DuckDB's quote-insensitive rule via `ident_matches`
(the point of EXP-6/9e0ed6d) — these sites compare raw stored spellings, so
`METRICS (o.t AS SUM(o.x), b.w AS AVG("t") OVER (…))` is spuriously **rejected at CREATE** even
though `"t"` and `t` are the same identifier in DuckDB. Today the CREATE-side strictness *masks*
the fence-side bug; the moment the CREATE check migrates to `ident_matches` (the documented #25/#28
direction) without `fan_trap.rs:402-406` and `per_grain.rs:703,709` migrating with it, a quoted
inner reference loses its grain in `metric_grain_tables` → `RootGrainFanTrap` never fires → the
base-anchored `__sv_agg` CTE silently inflates while `window.rs` happily computes it. **These
sites must migrate together.**

### EXP-13 — LOW: `where_clause` bypasses access modifiers — RESOLVED 2026-08-04 (TECH-DEBT #47)

`src/expand/where_clause.rs:89-106`. `resolve_where_clause` builds its lookup from *all*
`def.dimensions`/`def.facts` with no `AccessModifier` check, so a PRIVATE member that
`resolve_names` refuses to let you query can still be referenced (and its values probed) via
`where_clause := 'private_member = …'`. The queried-member path enforces PRIVATE (Phase 43); the
predicate path does not.

### EXP-14 — LOW → **should have been HIGH**: dotted member references in `where_clause` fall through to raw columns, unlike every other member-reference site — RESOLVED 2026-08-04 (TECH-DEBT #47)

**Severity correction (2026-08-04).** This entry predicted "fails loud at bind". It also silently bypasses the
fan-trap fence: `source_tables` is populated only for references that *resolve*, so an unresolved
qualified reference removes its member from the fence's input and a predicate reaching a fanning
grain is accepted. Same class as EXP-10 (HIGH). See TECH-DEBT #47 for the counterexample.

`src/expand/where_clause.rs:116-138`. `scan_references` keys a dotted chain as `"o.order_date"`
(`expr_tokens.rs:38`), but the member lookup is keyed by bare name only, so
`where_clause := 'o.order_date > …'` is not substituted with the member's expression and is not
metric-checked (`'o.revenue > 5'` slips past `WhereClauseReferencesMetric`). The NA-dim and window
sites deliberately resolve dotted references through `resolution::dim_ref_key` (#30/#28); this new
site diverges. Where the dimension's expression differs from a same-named physical column, the
filter silently uses different semantics than the member; otherwise it fails loud at bind. Either
resolve dotted refs like #30 or document the divergence.

### EXP-15 — LOW: a stored cyclic definition passes the fence rather than erroring — RESOLVED 2026-08-05 (TECH-DEBT #48)

`src/expand/fan_trap.rs:499-515`. `fanning_edge_on_path` checks the forward key `(a,b)` first and
declares the hop safe even if a *reverse* ManyToOne edge `(b,a)` also exists. Unreachable through a
validated tree (toposort rejects cycles at CREATE), but expand-time rebuilds use
`RelationshipGraph::from_definition`, which does **not** run cycle detection — so a stored cyclic
definition (the parser-reachable class of #141, which d48abee proved reaches `expand`) passes the
fence instead of erroring. d48abee fixed the hang and left the Ok/Err outcome unspecified; the
safer bias for a safety check (per SG-7's own reasoning) is to run cycle detection in
`build_relationship_graph` and fail `UncheckableDefinition` loudly.

### EXP-16 — MEDIUM-LOW: `rewrite_count_star` is blind to double-quoted identifiers and dollar-quoted strings — RESOLVED 2026-08-04 (TECH-DEBT #46)

`src/expand/facts.rs:220-273`. The COUNT(\*) rewriter tracks only single-quote state (naive toggle
at :229-233) — no `"…"` identifiers, no `$tag$…$tag$` strings, unlike every parse-side scanner
(`body_parser::scan::QuoteState`, `expr_tokens`, `util::blank_sql_comments` all handle both since
PARSE-1). The text `count(*)` inside a double-quoted identifier (`"my count(*) col"`) or a
dollar-quoted literal is silently rewritten to `count(<pk>)`, corrupting an identifier or literal —
the E-3 class that TECH-DEBT #28 killed for reference scanning, surviving in this one production
scanner. TECH-DEBT #28 mentions only that `util::is_word_boundary_char` "survives for the
expand::facts COUNT/name matchers"; the quote-capability gap itself is unrecorded. Low likelihood,
silent failure, mechanical fix.

### EXP-17 — MEDIUM-LOW: `find_matching_paren` — fifth independent quote scanner, no dollar-quote support — RESOLVED 2026-08-04 (TECH-DEBT #46)

`src/expand/semi_additive.rs:883-918`. Hand-rolled `Mode::{Normal,SingleQuote,DoubleQuote}` paren
matcher used by `build_snapshot_block`'s SG-5 decomposition. A `)` inside a dollar-quoted string
terminates the match early, mis-slicing the aggregate's argument. Same unrecorded asymmetry as
EXP-16: parse-side scanners learned `$tag$` in PARSE-1; expansion-side scanners never did.

### EXP-18 — LOW: `get_rn_column_for_metric` silent fallback can mask a grouping bug — RESOLVED 2026-08-05 (TECH-DEBT #48)

`src/expand/semi_additive.rs:936-946`. If a metric index is in *no* NA group, the function returns
`"__sv_rn"` with a bare `// fallback` comment instead of erroring. Given the #129/#32 history (two
metrics sharing a rank column was a silent-wrong-answer bug), a metric silently borrowing group 1's
rank column is exactly the failure shape this project treats as worst-in-class. An
`unreachable!`/error return would fail loud.

### Verified-sound (adversarial checks that passed)

- **Per-grain join semantics** (f38c3e5, persists at HEAD): grain CTEs join on
  `IS NOT DISTINCT FROM` with COALESCE-chained keys and FULL OUTER JOIN — NULL dimension groups are
  preserved and match across grains; a group present at only one grain survives with NULL metrics;
  `CROSS JOIN` for the no-dims case is correct.
- **Routing predicate**: `needs_per_grain` (anchor-fans-root OR anchor-pair fanning) exactly covers
  the shapes v0.11.0 errored; the dimension-below-grain guard declines to the base path so the
  fence's `FanTrap` speaks; single-grain SQL is byte-identical to before.
- **Fence in per-grain mode**: the retained metric × dimension check is unconditional; the skipped
  root-grain (EXP-1) and metric×metric (EXP-2) checks are exactly the base-anchored-topology
  invariants the per-grain/anchored-CTE shapes replace (except as noted in EXP-11).
- **`window_cte_anchor`/`snapshot_cte_anchor` direction guards**: fire only when all metrics share
  one grain and only in the fan direction; the child-of-root direction is deliberately left
  base-anchored to preserve NULL-extended groups — the reasoning at `per_grain.rs:637-656,724-740`
  is correct.
- **COUNT(\*)/SG-8 skip on the per-grain path** is justified (anchor CTEs join outward only
  many-to-one, so bare `COUNT(*)` counts exactly anchor rows).
- **TECH-DEBT #37 relaxation** (`validate_fact_table_path` either-direction check) admits only
  fan-in chains that were wrongly rejected.
- **Cycle termination** (d48abee): visited-set guards in both `JoinTree` walks and BFS `find_path`;
  hang-guard test retained.
- **Role-playing beyond one hop** (5b3c999): ancestor walk sound; at f38c3e5 the per-grain path
  cannot bypass it (same-pair edge dedup declines every definition where `find_using_context` could
  error) — the residual is EXP-10's *where-member* seam, not the dimension seam.
- **Dollar-quoting** (675b027): all three parse-side lexical layers share `read_dollar_tag_len`;
  `''`/`""` escapes and unicode identifier bytes consistent across `expr_tokens`, the lexer, and
  the blanker; `$1`/lone `$` correctly excluded.
- **9e0ed6d / d22b754**: canonical-key migration and quote-stripped emission consistent at all
  consumer sites found; `semi_additive.rs:582`'s remaining `quote_ident(&nd.dimension)` is the
  intended fail-clean fallback.
- **Where-splice parenthesization**: each substitution is parenthesized, so member expressions keep
  their precedence (`(US OR EU) AND large`); the `us_or_eu AND is_large` rationale is correct.
- **Write/read search-path agreement**: `resolved_schema_expr` mirrors `resolve_in_search_path`
  branch-for-branch, both pinned by tests; `DROP/ALTER IF EXISTS` erroring on an off-path
  multi-candidate name is deliberate, argued, and pinned
  (`test/sql/schema_scoped_views.test:133-145`).
- **Fan-trap fence + `where_clause`**: `check_where_clause_fan_traps` runs unconditionally in
  `expand()` and is what protects the per-grain CTEs from a fanning where-member (its own doc
  comment is stale, though — see ARCH-12).

---

## 3. Catalog & schema-scoping findings

### CAT-1 — MEDIUM: migration keeps the legacy JSON's schema spelling verbatim — RESOLVED 2026-08-04 (TECH-DEBT #44) — `INSERT OR REPLACE`/`OR IGNORE` can create a duplicate logical row, and reads then resolve to the stale one

`src/catalog/mod.rs:214-230` (`migrated_row` uses the recorded spelling unnormalized);
`src/parse/native_sql.rs:343-354` (`INSERT OR REPLACE`/`OR IGNORE` conflict on the byte-equal
`(schema_name, name)` PK); `prepared_lookup` (`ORDER BY schema_name`).

All *guards* fold case (`row_predicate` uses `lower(schema_name) = lower(…)`), but the conflict
target of `INSERT OR REPLACE`/`OR IGNORE` is the raw PK. A legacy row whose JSON recorded a
non-canonical schema case — e.g. `"Analytics"`, stamped by the old CREATE from `current_schema()`'s
echo of `USE "ANALYTICS"`, exactly the phenomenon `create_target_schema_expr`'s own doc describes —
migrates with `schema_name = 'Analytics'`. A later `CREATE OR REPLACE SEMANTIC VIEW analytics.v`
inserts under canonical `'analytics'`; the PK conflict never fires (`'Analytics' ≠ 'analytics'`
byte-wise). Result: two rows for one logical view. Worse: `prepared_lookup` matches both via the
folded predicate and `resolve_in_search_path` takes `rows[0]` — `ORDER BY schema_name` puts
`'Analytics'` first — so **reads keep returning the pre-replace definition** while
`CREATE OR REPLACE` reported success. Plain CREATE is safe (folded `EXISTS` guard errors first);
OR REPLACE has no guard by design; OR IGNORE's absorb relies on the PK.

**Fix direction:** canonicalize the schema spelling during migration (the same `duckdb_schemas()`
lookup `create_target_schema_expr` performs), or fold `schema_name` on write.

### CAT-2 — MEDIUM-LOW: `upgrade_definitions_schema` UPDATE is schema-blind after schema scoping — RESOLVED 2026-08-04 (TECH-DEBT #45)

`src/catalog/mod.rs:349-359`. The AR-4 version stamp runs `UPDATE … WHERE name = ?`, so with the
new `(schema_name, name)` key it stamps *every* same-named row across schemas — including a row the
pass deliberately left unverified. Scenario: `main.v` is an un-upgradeable legacy row (kept at
version 0 "so reads hard-error rather than silently under-checking") and `analytics.v` is an
older-but-parseable row; processing `analytics.v` stamps `CURRENT_SCHEMA_VERSION` onto *both*.
Kept out of HIGH because the actual safety gate is content-based (`has_incomplete_relationships()`
in `fan_trap::build_relationship_graph`), not the version integer — this corrupts metadata and the
stated invariant, not query safety. Fix: `WHERE schema_name = ? AND name = ?`.

### CAT-3 — MEDIUM-LOW: v0.1.0 companion import doesn't backfill `schema_name` into the JSON, breaking the documented column/JSON lockstep — RESOLVED 2026-08-05 (TECH-DEBT #49)

`src/catalog/mod.rs:129-140` (import inserts `def` verbatim under `UNRECORDED_SCHEMA_FALLBACK`);
`src/ddl/list.rs:126-140` (SHOW listings read `d.schema_name` from the parsed JSON,
`unwrap_or_default`). `migrated_row` explicitly backfills the JSON "so the new `schema_name` column
and the `schema_name` inside the JSON … cannot disagree", and the reader doc claims lockstep is
maintained by "CREATE … the migration … a schema-moving ALTER" — the companion import is the fourth
writer and skips the backfill. A companion-imported row lives under column `schema_name = 'main'`
but its JSON has no `schema_name`, so `SHOW SEMANTIC VIEWS` lists it with an empty schema and
`SHOW SEMANTIC VIEWS IN SCHEMA main` misses it. Name-based lookup still works (schema comes from
the column). Low likelihood (v0.1.0 files), one-line fix.

### CAT-4 — LOW: read paths silently ignore a `database.` qualifier that write DDL rejects — RESOLVED 2026-08-05 (TECH-DEBT #49)

`src/query/table_function.rs` / `explain.rs` / `ddl/describe.rs` bind bodies (`parse_view_ref`
captures `database`, nothing checks it); `prepared_lookup` has no database predicate.
`semantic_view('otherdb.analytics.v')` resolves against the *current* catalog's `analytics.v` with
no error, while `DROP SEMANTIC VIEW otherdb.analytics.v` errors via `current_database_guard_select`
precisely because "that is a wrong-object write rather than an unsupported one". The same
wrong-object argument applies to reads: a three-part reference naming a foreign database returns
another database's data silently. The bind bodies should reject a mismatched `view.database`.

---

## 4. Parsing / SHOW-surface findings

### PARSE-3 — LOW: `SHOW … LIMIT 0` accepted despite "must be a positive integer" — RESOLVED 2026-08-05

`src/parse/show_clauses.rs:454-464`. The error message promises a positive integer; `0` parses and
is passed through. Cosmetic inconsistency — pick one.

**Resolved by keeping the value and fixing the message.** `LIMIT 0` is a zero-row listing in
DuckDB and the clause is emitted verbatim into the catalog query, so rejecting it would be a
gratuitous divergence from the host dialect (CLAUDE.md: dialect questions go to DuckDB); Snowflake
documents no lower bound either, only a 10000 upper one we do not impose. The parse accepts `u64`,
and the message now says `must be a non-negative integer`. The five reference pages that repeated
"Must be a positive integer" were corrected with it. Covered by
`show_clauses::tests::limit_zero_is_accepted` (pins the value) and
`limit_rejects_negative_with_non_negative_message` (pins the message; confirmed red first).

### PARSE-4 — LOW: `matching_close_paren` in search-path injection skips single-quoted and `$tag$` literals but not double-quoted identifiers — RESOLVED 2026-08-04 (TECH-DEBT #46)

`src/parse/search_path.rs`. A `)` inside a double-quoted identifier in the argument list
(`semantic_view('v', search_path := …)`-adjacent text) would mis-splice. Exotic — explicit
`search_path` arguments short-circuit the injection — but it is a sixth quote-scanner variant with
its own subset of the rules (see ARCH-6).

---

## 5. Snowflake parity

Snowflake defines semantics; DuckDB defines dialect (CLAUDE.md). The audit found the implementation
unusually faithful, with divergences almost all deliberate, reasoned, and recorded in
`docs/explanation/snowflake-comparison.rst` and TECH-DEBT.md. The per-grain semantics were
*empirically probed against Snowflake* (TECH-DEBT #35/#36) — stronger evidence than doc-reading.

**At or near full parity (verified in code):** the CREATE surface — TABLES (composite PRIMARY
KEY/UNIQUE, optional alias, SYNONYMS, COMMENT), RELATIONSHIPS (multi-column keys, UNIQUE targets,
role-playing), FACTS (chaining, cross-table named-fact inlining), DIMENSIONS, METRICS (derived
metric-on-metric with cycle detection, PRIVATE-in-derived, window metrics with frames,
`NON ADDITIVE BY` with ASC/DESC/NULLS and cross-table NA dims, `USING`), `LABELS = (FILTER)` with
non-FILTER labels rejected; ALTER (RENAME TO, SET/UNSET COMMENT); DROP IF EXISTS; DESCRIBE's
5-column property table; SHOW SEMANTIC VIEWS with the full clause grammar (TERSE, LIKE, IN
SCHEMA/DATABASE/ACCOUNT incl. qualified and bare forms, STARTS WITH, LIMIT); `SHOW … FOR METRIC`;
SHOW COLUMNS; GET_DDL incl. the 3-arg `use_fully_qualified_names` form; facts×metrics mutual
exclusion; dimensions-only DISTINCT / metrics-only grand-total query modes; wildcard `alias.*` with
PRIVATE excluded; fan-out protection erroring loudly with per-grain computation for multi-grain
shapes.

**Intentional, documented divergences (correctly recorded):** the query surface is a
`semantic_view()` table function with string-literal member lists rather than the
`SEMANTIC_VIEW(…)` clause with bare identifiers, and direct SQL over the view (`AGG()`) is
documented not-planned — a loadable extension has no binder hook; `where_clause :=` spelling
(`where` is reserved in DuckDB named-parameter position); identifier case-insensitivity and
search-path resolution follow DuckDB (#25/#28, CLAUDE.md); scalar `GET_DDL`/`READ_YAML` cannot
receive the search path (#19/#25); reads see committed state, single catalog (#19/#26); PK/FK are
logical assertions, no catalog inference (breaking change in v0.10.0, documented); the YAML schema
is the extension's own serde schema, not Snowflake's Cortex semantic-model spec (documented; the
Cortex-only concepts — `time_dimensions`, `custom_instructions`, `sample_values` — documented n/a);
`IN ACCOUNT` accepted as a no-op; `CREATE OR ALTER`, tags, `MAX_STALENESS`, AI/COPILOT clauses,
`COPY GRANTS`, column-level security documented out of scope. `MATERIALIZATIONS`,
`explain_semantic_view()`, `list_semantic_views()`, and DuckLake/Iceberg/Parquet sources are
clearly labelled extension-only additions.

### PAR-1 — MEDIUM (docs): the highest-traffic query reference page is stale on three counts — RESOLVED 2026-08-04

`docs/reference/semantic-view-function.rst`:
1. Does **not document `where_clause :=` at all** (nor `search_path :=`), despite the parameter
   being implemented (`src/query/table_function.rs:153-217`) and advertised as the
   Snowflake-`WHERE` equivalent in `snowflake-comparison.rst:448-449`. The syntax block at :20-25
   lists only `dimensions/metrics/facts`.
2. Line 42 still states an unqualified view name "is an error when several schemas hold one" — the
   pre-#187 interim rule; search-path resolution replaced it (5f06ce8), and
   `create-semantic-view.rst` has the new rule. The two pages now contradict each other.
3. Line 135 says "Column types are inferred at define time" — removed in v0.10.0; types are
   inferred at bind via a LIMIT-0 probe (`src/query/table_function.rs:241`; the
   `resolution.rs` line reference in the original finding was stale). The same sentence's
   "columns default to VARCHAR" was wrong too, and the finding did not catch it: a failed probe
   is a hard error (WR-08 / D-15 removed the placeholder fallback precisely because it masked
   broken `FACTS` expressions).

**Resolved 2026-08-04.** All three corrected in `docs/reference/semantic-view-function.rst`:
`where_clause` added to the syntax block, the parameter table and a new
`ref-sv-pre-agg-filtering` section under Filtering (which now contrasts the pre- and
post-aggregation filters explicitly) plus an example; `search_path` documented as
parser-injected rather than hand-written; the view-name rule replaced with the search-path
rule from `create-semantic-view.rst`; and the type-inference sentence rewritten to describe
the bind-time `LIMIT 0` probe. `sphinx-build -W` clean. No TECH-DEBT entry: the drift is fully
closed, with no deliberately-degraded remainder.

**Correction (same day, from review on PR #193).** The first pass at count 3 claimed
dimension-and-metric queries still prefer CREATE-time persisted types on pre-v0.10.0 rows. That
was wrong, and instructively so: it was taken from the *module doc comment* at
`src/query/table_function.rs:31-35`, which still described the persisted-types fast path — but
**AR-4 (PR-2) removed it**. `column_type_names` / `column_types_inferred` are gone from
`SemanticViewDefinition` (`src/model.rs:434-440`), legacy rows carrying them fall through to
read-side inference, and the bind body runs a single unconditional probe. So the module comment
is itself PAR-1-class drift, and reading it instead of the code reproduced the very failure this
finding is about. Both the module comment and the docs page are now fixed against the code.

### PAR-2 — MEDIUM: `SHOW SEMANTIC DIMENSIONS/METRICS/FACTS` returns an empty `data_type` for all views created ≥ v0.10.0 — a *pinned interim state whose follow-up never landed* — RECORDED 2026-08-05 (TECH-DEBT #51)

`src/ddl/show_entities.rs:11-15`. CREATE-time type inference (`typeof(expr)`) was removed in
v0.10.0 Phase 65 (D-16/D-17, milestone squash 1da5ab6); unless the user declared an explicit output
type the column is empty. Snowflake's SHOW output populates data types.

**Lineage (why this survived review):** this was not an untested accidental regression. The change
was deliberate, and the degraded behaviour is *explicitly pinned by a passing test* —
`test/sql/phase39_metadata_storage.test` Test 4 asserts `(empty)` with a comment stating the plan:
"The read-side bind callbacks under the C++ Catalog API shim (Plan 05) probe on demand at
SHOW / DESCRIBE bind time. **Until Plan 05 lands**, SHOW SEMANTIC FACTS returns '(empty)' …".
Plan 05's read-side probing never landed, and the only record of that outstanding promise is this
test comment — there is no TECH-DEBT entry, no ROADMAP item, and no CHANGELOG framing of the SHOW
column as a temporary regression. The failure mode here is **ledger discipline for
deliberately-shipped interim states**, not test coverage: a test exists, passes, and pins the
degraded output, so no amount of additional testing would have flagged it. Any change that ships a
knowingly-degraded interim behaviour needs a TECH-DEBT entry created in the same change, so the
promise outlives the test comment. *(Verify Snowflake's exact SHOW column list against current
docs; then either land the Plan 05 probe or record the divergence as accepted.)*

### PAR-3 — MEDIUM: cross-table column references in member expressions unsupported and undocumented — RECORDED 2026-08-05 (TECH-DEBT #52), premise partly refuted

`src/expand/join_resolver.rs:267-318`. Required joins are collected exclusively from each member's
declared `source_table` — never by scanning expression text for foreign aliases. Named *facts*
referenced across tables are inlined (`src/expand/facts.rs:168-195`), but a raw column of another
logical table in an expression (`o.margin AS o.amount - c.discount`) will not pull `c`'s join and
fails (or binds wrongly) at query time. Snowflake permits expressions to reference related logical
tables. The docs gesture at the rule ("each fact references columns from its own table",
`docs/how-to/facts.rst`) but there is no explicit statement of the limitation and no TECH-DEBT
entry. **Undocumented divergence — record it (or implement it).** *(Verify the precise Snowflake
cross-table-reference rule.)*

**Verification result (2026-08-05): the rule is at parity; only its enforcement point differs.**
Snowflake's validation rules say "Expressions cannot refer to base table columns from other tables
or expressions from unrelated logical tables" — so a raw foreign column is rejected there too, not
permitted as this finding assumed. Snowflake rejects it at CREATE; here the CREATE succeeds
(confirmed by running the CREATE funnel's validator set over a body-parsed definition) and the
member fails at query time as a DuckDB unknown-alias binder error, since `join_resolver` collects
joins from `source_table` alone. No wrong numbers — the alias is qualified, so nothing binds.
Recorded as TECH-DEBT #52 with the CREATE-time validator as the finish line, documented in
`how-to/facts.rst` and `snowflake-comparison.rst`, and pinned by
`par3_cross_table_column_reference_pulls_no_join`.

### PAR-4 — LOW: `PRIVATE` rejected on dimensions, presented as fact rather than as a divergence — NOT A DIVERGENCE, closed 2026-08-05

`snowflake-comparison.rst:63-65`; rejection in `body_parser/entries.rs`. Snowflake's grammar allows
`PRIVATE` on facts, dimensions, and metrics; here it is allowed only on facts/metrics. Documented,
but with no rationale and no TECH-DEBT entry. *(Verify current Snowflake docs.)*

**Verification result: the finding's premise is wrong.** `CREATE SEMANTIC VIEW` does list
`{ PRIVATE | PUBLIC }` in the `dimensionExpression` grammar, which is what the finding read, but
the prose immediately restricts it: "You cannot mark a dimension as private. Dimensions are always
public." Rejecting `PRIVATE` on a dimension is therefore **parity**, not a narrowing, and needs no
TECH-DEBT entry. The comparison-table row now states the Snowflake rule alongside ours instead of
listing our behaviour as a difference, and TECH-DEBT #47's "see PAR-4 for that divergence" pointer
is corrected. A lesson for the audit method rather than for the code: reading a grammar block
without its prose produced a divergence that does not exist — the mirror image of PAR-1, where
reading a stale doc comment instead of the code produced the same kind of error.

### PAR-6 — NEW (2026-08-05), MEDIUM: a metric referencing a named fact on another table inlines the fact but never joins its table

Found while verifying PAR-3, and **not** something this review caught the first time — PAR-3's own
text records cross-table named-fact references as working, citing `src/expand/facts.rs:168-195`.
That citation covers the inlining, which does work. The join does not: `join_resolver` collects
aliases from each member's declared `source_table` and, on the facts path, from *queried* facts —
never from a fact a metric merely references. So `o.mixed_margin AS SUM(o.amount - c.cust_discount)`
expands to `SUM(o.amount - (c.discount)) FROM "orders" AS "o"` with `customers` absent, and DuckDB
raises an unknown-alias error at query time.

This is the form Snowflake documents as *the* way to cross tables ("define facts on source tables,
and finally refer to these expressions from connected logical tables"), and the workaround PAR-3
would otherwise point users at. Fully DDL-reachable; loud rather than silent. Filed as TECH-DEBT
#53 and pinned by `par6_cross_table_fact_reference_inlines_but_pulls_no_join`, which asserts the
broken output so the defect shows in CI. Not fixed in the pass that found it: teaching
`join_resolver` about referenced facts turns a query-time error into a returned number, which needs
the fan-trap fence to see the referenced fact's grain and needs numeric-oracle coverage.

### PAR-5 — documented residual: window metrics whose inner aggregates sit at different grains still error

TECH-DEBT #36 "Still declined, deliberately"; CHANGELOG. Correct-or-error posture; listed here for
completeness as the largest remaining computable-in-Snowflake shape.

### Parity items flagged "verify against Snowflake docs"

Inline `AS` aliasing inside `SEMANTIC_VIEW(…)`; exact `SHOW SEMANTIC {DIMENSIONS,METRICS,FACTS}`
column lists; whether `SHOW … FOR METRIC` matches a real Snowflake command form exactly; the
precise cross-table-reference rule (PAR-3) and PRIVATE-dimension legality (PAR-4); Snowflake
additions newer than the audit's knowledge.

---

## 6. Rough edges / code quality

Baseline is high: 28 non-test `unwrap/expect/panic` hits, all doc-examples or messaged
invariant guards; SQL-literal escaping type-enforced (`SqlLit` — raw `&str` doesn't compile into
the emission helpers) with no bare splices found; `ffi_util.rs` exemplary (char-boundary
truncation, both-or-drop publish, documented `Box<[u8]>` rationale); 112 `extension` feature gates
all scoped with per-site rationale (ST-8 pattern); zero TODO/FIXME/HACK in `src/`; parse layering
(`detect` → `rewrite` → `body_parser` → `expr_tokens` → `ident`) intentional and documented.

### ARCH-6 — MEDIUM: ≥6 independent quote-state walkers, kept in sync only by comments

Confirmed implementations of "walk SQL text honoring `''`/`""` escapes":
`body_parser/scan.rs:26` `QuoteState` (the declared "ONE quote-tracking implementation" — but only
for that module's depth-0 scanners; `is_quoting_balanced` :449 and `split_qualified_identifier`
:220 re-roll it locally in the same file), `ident.rs:336` `find_identifier_end`, `util.rs:163`
`blank_sql_comments`, `expr_tokens.rs` tokenizer, `expand/semi_additive.rs:883`
`find_matching_paren` (EXP-17), `expand/facts.rs:220` `rewrite_count_star` (EXP-16), plus
`parse/search_path.rs::matching_close_paren` (PARSE-4). Comments literally say "Mirrors the escape
rule used by `src/ident.rs::find_identifier_end` so the two callers agree" (`scan.rs:446`) —
mirror-by-copy, agreement enforced by prose. The expansion-side copies have already drifted (no
dollar-quote support). A crate-level `QuoteState` (promote `body_parser::scan::QuoteState`) would
collapse the class. This pattern is what review §6.2 / TECH-DEBT #28 attacked for reference
scanning; the balance/paren/boundary scanners were out of that scope and are not in the ledger.

### ARCH-7 — MEDIUM: `expand()` is a 336-line strategy dispatcher with combinatorial flag threading

`src/expand/sql_gen.rs:323-658`. One function sequences facts dispatch → materialization routing →
per-grain plan → window anchor → snapshot anchor → COUNT(\*) guard → fence (mode =
`grain_plan.is_some() || window_anchor.is_some() || snapshot_anchor.is_some()`) → four emit paths.
Each v0.12 feature added another parallel `Option` anchor and interaction term; the guard at :477
shows the flags already interact pairwise and had a real ordering bug (PR #175 note at :474). No
planner abstraction — "which topology answers this query" is smeared across four locally-computed
values whose mutual consistency is by-hand. Small symptom: `where_tables` is computed **twice
identically** in the same function (:410-413 and :632-635). 15 `#[allow(clippy::too_many_lines)]`
across `src/` (+5 `too_many_arguments`) quantify the pressure; next-worst:
`parse_single_metric_entry` (335 lines), `parse_keyword_body` (333), `expand_window_metrics` (293).
This is where the next silent bug will come from.

### ARCH-8 — MEDIUM-LOW: ~124 ad-hoc `to_ascii_lowercase()` comparisons on aliases bypass `ident_matches`

Concentrated in `expand/per_grain.rs` (19), `expand/role_playing.rs` (15),
`expand/join_resolver.rs` (14), `graph/relationship.rs` (13), `graph/cardinality.rs`,
`graph/using.rs`. Bare ASCII fold — no quote stripping, no Unicode fold — relying on the implicit
invariant that aliases are stored in one spelling. But `body_parser/tables.rs` stores the alias
raw (quotes preserved), so a definition declaring `TABLES ("o" AS orders …)` with relationships
against `o` compares `"o"` vs `o` and misses. Whether these sites are inside TECH-DEBT #28's
recorded residual is genuinely ambiguous from the ledger (see ARCH-13). Related: EXP-11's root
cause `collect_derived_metric_source_tables` (`facts.rs:570-582`) still keys by raw
`to_ascii_lowercase()` while its sibling `collect_transitive_metric_names` was migrated to
`normalize_ident_part` in 9e0ed6d.

### ARCH-9 — LOW-MEDIUM: god test modules — production was split (AR-1), tests were not

`src/body_parser/mod.rs` is 4,344 lines, ~89% one inline `mod tests` (starts :477) testing
*submodules*; `src/parse/rewrite.rs` is 4,140 lines, ~3,575 of test module (starts :566). This
contradicts the convention the same codebase established in `expand/` (17 behaviour-named
`tests_*.rs` files, explicitly extracted per `expand/mod.rs:23`), and CLAUDE.md's "confirm the red
per case" discipline is harder to honor in a 3,900-line module.

### ARCH-10 — LOW-MEDIUM: stringly-typed errors persist in public APIs outside the boundary #31 declared

TECH-DEBT #31 covers the graph-module internals only. `Result<_, String>` is also the public
signature of `model.rs:506/553/563` (`from_json`/`from_yaml`), `ident.rs:205/288`,
`render_ddl.rs:442/466`, `render_yaml.rs:23`, and the catalog reader (`catalog/mod.rs:461/485/498`)
— crate entry points used by FFI dispatchers. Four error currencies (`ParseError`, `ExpandError`,
`QueryError`, `String`) with ad-hoc `format!` wrapping at each seam (e.g. `native_sql.rs:45`).
Every new caller re-decides how to wrap.

### ARCH-11 — LOW: `DdlKind` bakes modifiers into a flat 15-variant enum

`src/parse/mod.rs:66-83`. `Create`/`CreateOrReplace`/`CreateIfNotExists`, `Drop`/`DropIfExists`,
`Alter`/`AlterIfExists` force paired-variant handling at ~33 sites and two `unreachable!`s
(`rewrite.rs:506`). A `kind + modifier flags` struct deletes the pair-matching; the cost grows with
every new modifier.

### ARCH-12 — LOW: stale/misattached doc comments in the new expansion code

- `src/expand/fan_trap.rs` — `check_where_clause_fan_traps`'s doc says it is "Skipped on the
  per-grain path … though today the per-grain strategy rejects a `where_clause` outright, so this
  is belt and braces". Both claims are false as of #172: the check runs *unconditionally*
  (`sql_gen.rs:509-511`) and `expand_per_grain` *accepts* a `where_clause`. Behavior is correct;
  the comment describes an earlier increment and will mislead the next editor.
- `src/expand/per_grain.rs:422-459` and `:552-659` — refactor artifacts left rustdoc attached to
  the wrong items: the "Whether role-playing is relevant to **this query**" block renders on
  `scoped_roles` (its own doc concatenated below), leaving `role_playing_affects_query`
  undocumented; likewise the `__sv_agg`-anchor block renders on `snapshot_cte_anchor`, leaving
  `window_cte_anchor` (:659) with no doc.
- `src/parse/rewrite.rs:396` — bare `unreachable!()` with no message while its three siblings
  (:133, :331, :506) carry invariant messages.

### ARCH-13 — LOW: ledger and doc-sync drift

- **MAINTAINER.md omits `expand/where_clause.rs`** from the `src/expand/` module tree
  (`MAINTAINER.md:87-92`) — new in #171; project rules require same-change sync.
- **TECH-DEBT #28's status marker is stale**: header says `❌ … not yet applied`, the body's final
  bullet says "the reference-tokenizer arc (#28) is fully landed" — a reader cannot tell what
  remains open (directly relevant to ARCH-8).
- Entry #38 appears before #37; the "Last updated" trailer narrates #38 twice with different
  statuses. "Test Coverage Gaps" numbering jumps #4 → #19 unexplained in-file.
- **Stale "eight fuzz targets" comments** — there are nine: `Justfile:291`,
  `.github/workflows/Fuzz.yml:94`, `MAINTAINER.md:541-542,591` (while `MAINTAINER.md:872`
  correctly says nine).
- `src/lib.rs` — `expr_tokens`/`sql_lit` are `pub(crate)` while equally-internal `ffi_util`,
  `util`, `body_parser` are fully `pub`; the line is drawn by history, not policy.
- `src/graph/join_tree.rs` — dead in the default build; recorded in #37 "Left behind"; single
  consumer, worth folding in eventually.

---

## 7. Test coverage

### Test-surface map

94 sqllogictest files ↔ 94 TEST_LIST entries — exact match, **no orphans**, sync CI-enforced
(`CodeQuality.yml:63`, `just check-test-list`); 2 files deliberately parked in `test/sql/_excluded/`.
17 crates in `tests/` (9 proptest harnesses, all run by CI's `llvm-cov nextest` with an 80% line
floor). 9 fuzz targets, all wired into the Fuzz matrix and `just fuzz-all`. 22 Python integration
files, all reachable from `just test-integration` recipes. Unit-test weight is heavy where the risk
is (`parse/rewrite.rs` 276, `body_parser/mod.rs` 275, `expand/*` ≈290, `graph/` 84, `catalog/` 55).
No `#[ignore]`, no trivially-true assertions found; extension-gated unit tests do run in CI. The
roundtrip generator deliberately populates `is_filter` to defeat the vacuous-field hazard
(`tests/common/mod.rs:260-296`).

### PBT-6 — HIGH-VALUE GAP: all five differential/numeric proptests hardcode `where_clause: None` — PARTIALLY CLOSED 2026-08-03

`tests/differential_proptest.rs:237`, and one occurrence each in `star_schema_proptest.rs`,
`semi_additive_proptest.rs`, `window_metric_proptest.rs`, `multi_hop_join_proptest.rs`. The newest
correctness-critical feature is excluded from every numeric oracle — predicate *math* is checked
only by hand-picked sqllogictest rows. EXP-10 sits exactly on this blind surface. Randomizing
simple member predicates mirrored into each oracle's WHERE is the single highest-value test
investment available.

**Closed in two of the five** (2026-08-03), covering the two distinct emission topologies:

- `differential_proptest` — the base-anchored single-table path. A generated predicate AST is
  rendered twice: member names into `where_clause`, raw columns into the oracle's pre-aggregation
  `WHERE`. The definition now also declares one **filter member** (`LABELS = (FILTER)`) per
  dimension with a compound `d{i} = 0 OR d{i} = 2` expression, so substitution is not identity
  (no physical column is named `f{i}`) and precedence is load-bearing (a bare filter member inside
  a surrounding `AND` is only correct if the splice parenthesizes).
- `star_schema_proptest` — the per-grain CTE path. The predicate is mirrored into **both** grain
  halves of the oracle, which is what pins the CHANGELOG's "applied inside each grain CTE" claim.
  It also adds a second fence property: a predicate naming a CHILD-side member alongside the
  parent-grain metric must be rejected, since evaluating it in the parent CTE would join `t` and
  fan `u` — the `where_clause` analogue of the existing parent-metric/child-dimension rejection.

Both were **mutation-verified rather than merely observed green**, which is the point of the
exercise: removing the parenthesization at `where_clause.rs:93` fails `differential_proptest` with
a shrunk counterexample, and dropping the predicate from the grain CTE at `per_grain.rs:1044`
fails `star_schema_proptest`. Each harness also carries a generator-coverage guard
(`generator_varies_the_predicate_and_exercises_filter_members`,
`generator_reaches_both_where_clause_branches`) asserting that the predicate actually varies, that
filter members are actually referenced, and — in the star harness — that both fence branches are
actually reached, so an assertion inside an unreachable branch cannot pass for coverage.

**Then `semi_additive_proptest`** (2026-08-04), the highest-priority of the remaining three and the
path EXP-9 lived on. The predicate is applied before the `RANK` inside `__sv_snapshot`, so it moves
*which row wins the snapshot* rather than only shrinking what is summed; the oracle splices it into
every reference to the base table, including the snapshot-determining subquery. Verified by
mutation (dropping the injection at `semi_additive.rs:297` reds the property) and by a
deterministic `predicate_is_applied_before_the_snapshot_not_after` test that asserts the
before- and after-snapshot formulations *disagree* — so the harness fails if it ever stops being
sensitive to the distinction — as well as asserting the extension matches the before-form.

**Finally `window_metric_proptest` and `multi_hop_join_proptest`** (2026-08-04), closing the gap.
The window oracle filters the `agg` CTE only — the correlated subquery reads from it, so partition
membership follows the filter automatically — and its guard asserts predicates on a queried
dimension outside the effective partition are generated. The multi-hop oracle filters each grain's
half and gains a second fence property mirroring the dimension rule: a `where_clause` member
*below* a selected metric's grain must be rejected, since joining it in fans that metric. Both
mutation-verified (`window.rs:236`, `per_grain.rs:1044`).

**PBT-6 is now closed** — all five numeric harnesses generate and oracle-check a predicate, each
with a generator-coverage guard so the parameter cannot silently revert to an inert default. The
two remaining `where_clause: None` sites live in `expand_proptest.rs`, which checks structural
parse/expand invariants rather than numbers, and are deliberately out of scope.

### PBT-7 — ~~HIGH-VALUE GAP: the multi-grain FULL OUTER combiner has no randomized coverage~~ — WITHDRAWN 2026-08-04, the premise was false

**Original claim:** "Both grain-related proptests are single-table/single-grain. `coalesced_key`
(`src/expand/per_grain.rs:1164`) and NULL-dimension-key coalescing across grain groups — the
classic FULL-OUTER-COALESCE bug surface — are pinned only by fixed examples."

**That is wrong, and was wrong when written.** Checked directly while working through the PBT-6
items: `star_schema_proptest` spans two grains (child + parent) and `multi_hop_join_proptest` spans
three, and *both* oracles already combine them with `FULL OUTER JOIN` on `IS NOT DISTINCT FROM`
plus `COALESCE`d dimension keys — which is precisely the oracle shape this finding proposed
building. Their generators emit NULL group keys and NULL/dangling foreign keys, so the
NULL-coalescing surface was randomized too.

The finding appears to have been produced by pattern-matching the two *single-table* harnesses
(`differential_proptest`, `semi_additive_proptest`) and generalising to "both grain-related" ones
without reading the star and multi-hop oracles. Recorded here rather than deleted, because a
review's wrong findings are worth knowing about: this one would have cost a day building a harness
that already existed. PBT-6, filed alongside it by the same pass, was real and is now closed —
so the pass was not uniformly unreliable, which is exactly why individual claims need checking
before being scheduled.

### TC-1 — the read-only legacy-catalog migration refusal is unit-simulated only

`src/catalog/mod.rs:1219` (in-memory connection + hand-built legacy table). No integration test
opens a real persisted pre-scoping `.db` read-only through actual `LOAD` to prove the actionable
message surfaces through the FFI error path. Note `test/sql/readonly_load.test` does not test
read-only at all — it is a documented writable-bootstrap smoke; the name over-promises. Also
untested: legacy rows with *invalid JSON* during the schema-scoping migration (companion-file
corruption is tested; in-table corruption is not).

### TC-2 — per-recent-feature edge gaps

- **where_clause:** no end-to-end error-path tests (invalid syntax `'region ='`; what binder error
  an unresolvable name actually surfaces); no numeric semi-additive + where_clause sqllogictest
  (SQL-text unit assert only, `semi_additive.rs:2482`); no role-playing case (EXP-10); quoted
  member names in the predicate untested end-to-end; `where_clause.test`'s 9 cases carry no
  documented per-case red-walk (unlike `get_ddl_qualified_name.test:24-30`,
  `show_scope_all_commands.test:16-19`, `schema_scoped_views.test:17-21`).
- **LABELS = (FILTER):** LABELS on a *metric* neither accepted nor rejected by any test; a FILTER
  member used bare in a *multi-grain* predicate untested; quoted filter names used bare untested;
  the DESCRIBE/SHOW/YAML assertions live only in one serialized file (`named_filters.test`).
- **Multi-grain:** best-covered feature of the release (46 independently-reporting unit tests +
  8 `.test` files + 2 differential proptests) — remaining: a window metric and a semi-additive
  metric in the *same* query (mixed snapshot + window anchors in one plan); quoted identifiers on
  the per-grain path (phase68 quoted-ident tests predate per-grain).
- **SHOW IN scope:** exemplary red-walk discipline; remaining: `IN DATABASE <nonexistent>` (only
  nonexistent *schema* pinned); mixed quoting in the qualified form (`IN SCHEMA db."quoted
  schema"`); scope + LIKE in both orders.
- **Schema scoping:** `DROP SCHEMA` with semantic views inside — orphaned `_definitions` rows
  unpinned; ATTACHed second database interaction with scoping (single-catalog guard is name-level
  only).
- **GET_DDL:** qualify=true with quoted/space-bearing schema or view names (emitted-header quoting
  + replay unpinned); non-boolean third argument (string `'true'`, integer) — coercion vs error
  unpinned.

### TC-3 — surfaces with neither proptest nor fuzz

The multi-grain combiner (PBT-7); `where_clause` semantics (structural fuzz only —
`fuzz_where_predicate`'s quote/paren-balance oracle); SHOW/DESCRIBE read-side output tables;
catalog migration SQL; `native_sql.rs` emission; role-playing/USING paths (fixed-schema Python
differential only, per `differential_proptest.rs`'s own scope note).

### TC-4 — zero-direct-unit-test production files

`src/parse/native_sql.rs` (746 lines of SQL emission — largest), `src/parse/create_body.rs` (394),
`src/expand/sql_gen.rs` (658), `src/query/table_function.rs` (422), `src/query/explain.rs` (353),
most of `src/ddl/` (`define`, `get_ddl`, `list`, `show_columns`, `show_dims_for_metric`,
`show_entities`, `show_materializations`), `src/graph/toposort.rs`, `src/graph/cardinality.rs`.
All are exercised indirectly (sqllogictest / higher-level tests / the 80% floor), but a logic bug
there has no independently-reporting test to localize it.

### CI-6 — quality-gate items not in CI

`just test-ducklake` (real jaffle-shop data) is in **no** CI workflow and not in `test-all` — only
the synthetic `test-ducklake-ci` variant runs anywhere. Fuzz never runs on fork PRs (deliberate
CI-2 trust boundary; `just ci`'s compile-only `check-fuzz` is the PR-side guard). Multi-platform
sqllogictest is post-merge only (BuildQuick is linux_amd64). `paths-ignore` for docs is safe
(`.test`/`TEST_LIST` not ignored).

---

## 8. Release hygiene

### REL-1 — v0.12.0 declared but never tagged

`CHANGELOG.md` declares `[0.12.0] - 2026-07-28` and `Cargo.toml` is at `0.12.0`, but no `v0.12.0`
tag exists on the remote — the last tag is `v0.11.0` (bfaf54c). Per the project's own milestone
checklist the release is half-finished, and the large `## [Unreleased]` section now stacks on top
of an untagged version. Decide: tag f38c3e5 retroactively as v0.12.0, or fold the 0.12.0 section
back into Unreleased and cut v0.12.0 (or v0.13.0) from HEAD once the EXP-9/EXP-10 fixes land.

**Decided 2026-08-04 (maintainer):** the second option — v0.12.0 is still being worked towards and
will be tagged from HEAD once all its work is complete, not applied retroactively to f38c3e5.

**Partly actioned 2026-08-04.** The premature `## [0.12.0] - 2026-07-28` section has been folded
back into `## [Unreleased]` (subsections merged pairwise, 0.12.0's bullets first as the earlier
work; its `Known limitations` kept as the final subheading; the dangling `[0.12.0]:` compare link
removed — it pointed at a tag that does not exist). All 25 top-level bullets are preserved. This
restores the state CLAUDE.md's milestone checklist assumes: the version section is created *at
tag time*, and until then everything unreleased lives under `Unreleased`.

Two things remain for tag time, deliberately not done now while 0.12.0 work is still landing:
1. Rename `## [Unreleased]` to `## [0.12.0] - <date>`, add a fresh empty `Unreleased`, and add the
   `[0.12.0]:` compare link back.
2. **An in-version churn pass.** Now that the two sections are one version, CLAUDE.md's rule that
   in-version churn must not be listed applies across the merged content. At least one bullet is
   affected: the `Fixed` entry for the `where_clause` member on a role-played table (EXP-10)
   describes a defect in `where_clause`, which is itself an unreleased `Added` feature of this same
   version — so no released build ever had that bug and users have nothing to be told about.
   Flagged rather than deleted here, since removing a well-written entry is a maintainer call and
   more 0.12.0 work may yet change the picture. Re-check the whole merged set at tag time, not
   just this one.

REL-2 (the stray `v1.0` tag) is untouched and still open.

### REL-2 — stray legacy `v1.0` tag

A `v1.0` tag from 2026-02-28 (old milestone naming, commit 1837274) is still on the remote.
`git describe`-style tooling and `sort -V` consider it the highest version. Delete or rename it.

---

## 9. Suggested priority order

Status as of 2026-08-04. ✅ = landed on `main`; ⏳ = in review; ❌ = withdrawn.

1. ✅ **EXP-9 and EXP-10** — the two silent-wrong-number paths. PR #189 (TECH-DEBT #39/#40).
2. ✅ **PBT-6** — `where_clause` randomized in all five numeric oracles, each mutation-verified.
   PRs #189 + #191 (TECH-DEBT #41). ❌ **PBT-7** withdrawn: its premise was false, see above.
3. ✅ **EXP-12 paired migration** — all four window-inner-metric sites → `ident_matches`, together.
   PR #192 (TECH-DEBT #43). Found to be an *active* wrong-number bug, not the latent one filed.
   ✅ **CAT-1/CAT-2** — TECH-DEBT #44/#45. CAT-1's duplicate-row/stale-read symptom was reproduced
   end-to-end against a real pre-scoping database before fixing. All three merged in PR #192.
4. ✅ **PAR-1** — `semantic-view-function.rst` drift, all three counts corrected.
   **REL-1** decided and partly actioned — v0.12.0 will be cut from HEAD when its work is
   complete; the premature CHANGELOG section is folded back into `Unreleased`, with the rename and
   an in-version churn pass left as tag-time steps. **REL-2** (stray `v1.0` tag) still open. ARCH-13's
   sub-items are partly overtaken: MAINTAINER.md's `where_clause.rs` omission and the "eight fuzz
   targets" miscount were fixed on `main` independently; the TECH-DEBT #28 status marker is still
   ambiguous.
5. Remaining mediums, batched into reviewable PRs (2026-08-04):
   ✅ **EXP-16/EXP-17/PARSE-4** — the three divergent quote scanners, collapsed onto `QuoteState`
   (TECH-DEBT #46). Delivers ARCH-6's substance for those sites; ARCH-6 stays open for the rest.
   ✅ **EXP-13/14** (`where_clause` resolution, TECH-DEBT #47). ✅ **EXP-15/18** (silent fallbacks
   that should fail loud, TECH-DEBT #48). ✅ **CAT-3/CAT-4** (catalog + read-path scoping,
   TECH-DEBT #49). ✅ **EXP-11** (per-grain derived anchoring, TECH-DEBT #50) — the empty→root
   substitution is now per component, and `decompose` declines a source-less aggregate dependency.
   It stands alone as expected, but *not* for the anticipated reason: it needs no numeric-oracle
   work, because the shape is unreachable through DDL and the fix restores an error rather than
   changing a number. ✅ **PAR-2/3/4 + PARSE-3** — the parity pass (2026-08-05). PARSE-3 fixed by
   keeping `LIMIT 0` and correcting the message; PAR-2 and PAR-3 recorded as TECH-DEBT #51/#52 with
   the docs they had drifted from corrected; **PAR-4 closed as not-a-divergence** — Snowflake also
   forbids `PRIVATE` on dimensions. Verifying PAR-3 turned up **PAR-6** (TECH-DEBT #53), a real
   defect on the supported cross-table path, filed rather than fixed.
6. Structural debt as capacity allows: ARCH-6 (crate-level `QuoteState`), ARCH-7 (`expand()`
   planner), ARCH-8/9/10/11/12, TC-1, TC-2, TC-3, TC-4, CI-6.
