# Plan: Closing the Remaining Snowflake Parity Gaps

**Drafted:** 2026-07-28, after per-grain metric aggregation (#167) closed
TECH-DEBT #35.
**Revised:** 2026-07-28 — every item below ships **into v0.12.0**, which is
bumped in `Cargo.toml` but **not yet tagged**, as its own pull request. There is
no v0.12.1 / v0.13.0 / v0.14.0 staging: the milestone tag goes on once the
sequence below is done, and each PR adds its bullets to the existing
`## [0.12.0]` CHANGELOG section rather than opening a new one.

### Working agreement

- **One PR per item.** Each lands independently, green, on `main`.
- **Sequential, on the designated branch.** Follow-up work reuses
  `claude/snowflake-query-parity-ai9mbz`, restarted from `main` after each
  merge — so one PR is open at a time, in the order below.
- **The CHANGELOG accumulates.** `## [0.12.0] - 2026-07-28` stays open; its date
  is corrected at tag time.
- **Ordering is a dependency chain, not a preference.** PR 4 (`WHERE`) must
  precede PR 6 (multi-grain completeness): a predicate has to be injected into
  whatever CTEs that work produces, and the reverse order means touching the
  same emitters twice.

This plan covers what still diverges from Snowflake semantic views in **query
semantics**, plus the interface differences that are currently "not planned" and
worth an explicit decision. Ordered by (verified certainty × user impact) ÷ risk,
not by size.

## What the Snowflake docs actually say

Checked 2026-07-28 against `docs.snowflake.com`, because two v0.12.0 design
decisions were reasoned from the model rather than verified:

| Question | Snowflake's rule | Our status |
|---|---|---|
| Which dimensions may be queried with a metric? | "The logical table for the dimension must be related to the logical table for the metric" and must have "an equal or lower level of granularity than the logical table for the metric" ([querying](https://docs.snowflake.com/en/user-guide/views-semantic/querying)) | ✅ **Matches.** Our `FanTrap` rejection of a dimension below a metric's grain is their rule, not our invention. v0.12.0's docs inferred this — they can now cite it. |
| Can a predicate filter before aggregation? | Yes: `SEMANTIC_VIEW( v METRICS … DIMENSIONS … WHERE <predicate> )`. "In the condition, you can only refer to dimensions, facts, and expressions that use dimensions and facts." "This filter condition is applied before the metrics are computed." ([construct ref](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view)) | ❌ **Gap 1.** We have no pre-aggregation filter at all. |
| Named reusable filters? | `LABELS = (FILTER)` on a fact/dimension resolving to BOOLEAN, referenced bare in `WHERE` (GA May 2026) ([filters](https://docs.snowflake.com/en/user-guide/views-semantic/filters)) | ❌ **Gap 2.** Not modelled. |
| Facts + dimensions together? | "All facts and dimensions used in the query (**including those specified in the WHERE clause**) must be defined in the same logical table." | ✅ Implemented as `FactPathViolation`, relaxed to "reachable without fan-out". The fan-in over-strictness (TECH-DEBT #37) is fixed. |
| How are metrics at different grains combined? | **Not stated in the docs.** The construct reference documents the dimension-grain rule and the FACTS/METRICS exclusion, but not the join semantics between grains. | ⚠️ **Unverified.** We chose NULL-safe `FULL OUTER JOIN`; see PR 3. |

## The gaps, as a PR sequence

| PR | Item | Size | Blocked on |
|---|---|---|---|
| 1 | Docs accuracy pass (§0) | ~30 min | ✅ landed (#168) |
| 2 | TECH-DEBT #37 — fact-path fan-in fix (§1) | ~½ day | ✅ landed |
| 3 | Multi-grain join-semantics verification (§2) | ~½ day | **a Snowflake account — maintainer-run** |
| 4 | Pre-aggregation `WHERE` (§3) | 2 phases, may split into 2 PRs | — |
| 5 | `LABELS = (FILTER)` named filters (§4) | ~1 phase | PR 4 |
| 6 | Multi-grain completeness, TECH-DEBT #36 (§5) | 3 sub-items, one PR each | PR 4 |
| 7 | Query-syntax interface spike (§6) | spike first | decision after 4-6 |

PR 3 is the only one that cannot be done in this environment; it does not block
4-6, and if it turns up a divergence the correction is contained to
`per_grain::render_multi_grain`.

### 1. Docs accuracy pass (~30 min) — PR 1

v0.12.0 shipped two doc statements phrased as our own reasoning. Both are now
confirmed rules. Cite them.

- `docs/how-to/fan-traps.rst` and `docs/explanation/snowflake-comparison.rst`:
  state the equal-or-lower-granularity rule as Snowflake's, with a link.
- Sharpen the "Pre-aggregation `WHERE`" row of the not-supported table with the
  exact contract discovered above (it currently describes the gap vaguely).

No code. Do this first so the published docs stop hedging on a settled question.

---

### 2. TECH-DEBT #37 — fact-path false rejection under fan-in (~half a day) — PR 2

**Why this early:** smallest code change in the sequence, purely a bug, and it
is a *parity* bug — Snowflake's same-logical-table rule for facts does not
reject the shapes we reject.

`JoinTree::from_graph` takes each node's parent as `graph.reverse[node].first()`
— the first table that *references* it. Under fan-in (two children of one
parent) the base table itself is handed a "parent", and ancestry chains run
through the root and out the other side. Consequences: a fact on `shipments`
with a dimension on `customers` (`s → o → c`, every hop many-to-one) is rejected
with `FactPathViolation` though the path is safe; `SHOW SEMANTIC DIMENSIONS …
FOR METRIC` hides dimensions for the same reason.

**Fix:** derive the parent map by BFS from the base table over undirected edges —
the walk `expand::join_resolver::build_tree_parents` already performs for join
emission. Then `ancestors_to_root` is correct under fan-in and both consumers
are fixed at once.

**Tests (test-first):** a fact-query test for the `s → o → c` shape (currently
red), a `SHOW … FOR METRIC` sqllogictest for the sibling equivalent, and a
regression test that a genuinely unrelated pair still raises `FactPathViolation`.

**Risk:** low, but it *widens* what the facts path accepts — the guard against
over-widening is the third test above.

---

**✅ Landed. Two corrections to the plan above, both found while implementing:**

1. **Only fan-in onto the base table exists.** `RelationshipGraph::check_no_diamonds`
   exempts the root and rejects every other multi-parent node as an ambiguous
   join diamond, so the `s → o → c` shape is only reachable with `o` as the base
   table. The first fixtures written for this used a non-root fan-in and were
   rejected at `CREATE` time.
2. **The parent-map fix alone was not sufficient — it would have traded a false
   rejection for a false acceptance.** Rooting the tree correctly makes the base
   table an ancestor of *every* alias, including the fan-in siblings reachable
   only by walking a many-to-one edge backwards, so an ancestry test would then
   have *accepted* a fact on `line_items` with a dimension on `shipments` and
   silently returned row-multiplied output. `validate_fact_table_path` therefore
   moved off ancestry altogether onto the direction-aware `find_path` +
   `fanning_edge_on_path` walk. The parent-map fix still lands, and is what fixes
   `SHOW … FOR METRIC` (which already checked direction via its own card map).

The "guard against over-widening" test the plan asked for is what caught this:
it was written to pass before *and* after, and would have failed the naive fix.

---

### 3. Verification spike — multi-grain join semantics (~half a day, no code) — PR 3

The one place v0.12.0 may diverge silently. We combine per-grain results with a
NULL-safe `FULL OUTER JOIN`, so a dimension group present at one grain and
absent at another survives with a `NULL` metric. If Snowflake inner-joins, or
anchors on a primary grain and left-joins outward, our result *sets* differ on
exactly those groups (values agree wherever both grains have the group).

**Method:** run a fixed fixture on a real Snowflake account — customers /
orders / line_items with a childless parent and an order with no items — and
compare row sets for: parent metric + base metric by a parent dimension; two
sibling-child metrics by a parent dimension; a derived metric spanning grains.
Record the transcript in `_notes/`.

**Outcomes:** matches → delete the caveat from the docs and add the fixture as a
golden test. Differs → a correction PR before the tag; the join shape is one
function (`per_grain::render_multi_grain`), so the change is contained.

Same trip should settle two smaller unknowns, both currently listed as
behavioural differences on the comparison page without evidence: whether
Snowflake permits **window metrics co-queried with aggregate metrics**, and
whether it permits **`NON ADDITIVE BY` together with a window spec on one
metric**. If it does, each becomes a small scoped change; if not, the docs
should say "matches Snowflake" instead of "difference".

---

### 4. Gap 1 — Pre-aggregation `WHERE` — PR 4

The largest remaining semantic gap, and the one with a now-exact contract. A
filter on a member that is not in the output ("revenue for orders shipped after
X") cannot be expressed at all today.

**Interface decision (needs a call).** Snowflake puts the predicate inside the
construct: `SEMANTIC_VIEW( v METRICS … WHERE orders.order_date > '1995-01-01' )`.
Our query surface is a table function with named parameters, so the analogue is
a string-valued parameter. `where := '…'` reads closest to Snowflake but `WHERE`
is a SQL keyword in that position and may need quoting (`"where" := '…'`);
`filter := '…'` is lexically safe. **Recommendation:** try `where :=` first (a
30-minute spike against DuckDB's named-parameter parser), fall back to
`filter :=`, and document whichever it is as the `WHERE` equivalent.

**Work, in dependency order:**

1. **Plumbing** — one entry in `cpp/src/shim.cpp::sv_semantic_named_params()`
   (shared by `semantic_view` and `explain_semantic_view`, so they cannot
   drift), the matching pointer/len pair through the bind FFI, and a field on
   `QueryRequest`. Contained: the named-parameter list is already defined once.
2. **Resolution + validation** — the predicate references declared dimension and
   fact *names*; resolve them through the existing case/quote-insensitive
   matcher and substitute their expressions with `expr_tokens::inline_references`
   (the same quote/literal-aware splice the derived-metric path uses, so a name
   inside a string literal is untouched). Reject a reference to a **metric** —
   Snowflake's rule — with a clear error naming the metric.
3. **Grain rules** — predicate-referenced members participate in the same
   reachability checks as queried dimensions (Snowflake explicitly counts
   WHERE-clause members in its same-logical-table rule). Concretely: extend
   `check_fan_traps`'s metric × dimension loop and `per_grain::plan`'s
   sufficiency check to include them.
4. **Injection per emission path** — five sites, each with its own semantics:
   - base-anchored: `WHERE` before `GROUP BY`;
   - **per-grain**: inside *each* grain CTE, so it filters before each metric's
     own aggregation;
   - **semi-additive**: inside `__sv_snapshot`, *before* the `RANK` — filtering
     changes which row is the snapshot, which is what "before the metrics are
     computed" must mean. Needs its own test with a filter that excludes the
     otherwise-winning row;
   - **window**: inside `__sv_agg`, before the window function;
   - facts: plain `WHERE`.
5. **Materialization routing** — a pre-aggregated table cannot answer a filter on
   a member it does not carry. v1: skip routing whenever a predicate is present
   (correct, conservative). v2 (separate change): route when every referenced
   member is a materialized dimension.
6. **Hardening** — a `fuzz_where_predicate` target (the predicate is arbitrary
   user text spliced into generated SQL; the tokenizer is already
   quote/dollar-quote aware, and this is the sort of seam issue #145 came from),
   plus differential tests comparing filtered results against hand-written SQL.

**Size:** ~2 phases. Step 4 is where the real design work is; steps 1-2 are
mechanical. Split into two PRs (plumbing + base-anchored path, then the CTE
paths) if the first grows past a reviewable diff.

---

### 5. Gap 2 — `LABELS = (FILTER)` named filters — PR 5

Depends on Gap 1 — a named filter is a boolean dimension/fact usable bare in the
predicate. Once the predicate machinery exists this is mostly DDL surface:

- `body_parser`: accept `LABELS = (FILTER)` on a dimension/fact entry (the
  annotation slot already parses `COMMENT` / `WITH SYNONYMS`);
- model + JSON/YAML round-trip + `GET_DDL` rendering + `DESCRIBE` / `SHOW`
  output (the round-trip proptest will catch omissions);
- validation: a filter must resolve to BOOLEAN — we can only check this at query
  time via DuckDB's binder, so the honest v1 is to let the binder error;
- query: a bare filter name in the predicate resolves to its expression.

---

### 6. TECH-DEBT #36 — multi-grain completeness — PR 6 (three sub-PRs)

The residual v0.12.0 recorded: multi-grain queries whose metrics include a
window metric, an *active* semi-additive metric, or role-playing (`USING`)
resolution keep the fan-trap error. Snowflake answers them. Three independent
sub-items, each gated by one predicate (`per_grain::is_eligible`), so they can
land separately:

- **semi-additive at its own grain** — the snapshot CTE is base-anchored;
  `select_spec::push_from_anchor` already exists, so the change is to
  parameterize `expand_semi_additive`'s anchor and emit it as a grain group.
  Hardest of the three: the snapshot's `RANK` partitioning interacts with which
  dimensions the grain CTE carries.
- **window metrics at their own grain** — same shape for `__sv_agg`; simpler,
  because the window function runs over the CTE and only the inner aggregate is
  grain-sensitive.
- **role-playing in grain CTEs** — thread the `USING` scoped-alias context
  through `per_grain::anchor_joins` so a grain CTE joins the right role.

Do these *after* Gap 1: a `WHERE` predicate must be injected into whatever CTEs
these produce, and building them in the other order means touching the same
emitters twice.

---

### 7. Interface parity — an explicit decision, not a default — PR 7 (optional)

Two long-standing "not planned" entries deserve re-examination now that the
semantics are close:

- **`SEMANTIC_VIEW( v DIMENSIONS … METRICS … WHERE … )` as SQL syntax.** The
  extension already owns a `parser_override` hook that rewrites recognised
  statements before DuckDB plans them — the mechanism that makes our DDL
  Snowflake-shaped. The same hook could rewrite the `SEMANTIC_VIEW(…)` construct
  inside a query into the `semantic_view('v', dimensions := […])` table-function
  call, giving *syntactic* parity for the query side. **Risk is materially
  higher than the DDL case:** the construct can appear anywhere a table
  reference can (subqueries, CTEs, joins, set operations), so the rewrite is a
  scan of arbitrary SQL rather than a statement-prefix match, and a mis-detection
  breaks unrelated queries. Worth a spike; not worth committing to blind.
- **`AGG(metric)` in plain `SELECT … GROUP BY`.** Requires resolving view-defined
  aggregates in arbitrary SQL — well beyond the rewrite above. Recommend keeping
  this "not planned" and saying so plainly.

**`ASOF` / temporal relationships** stays out of scope unless a concrete use case
turns up; the docs' "standard equi-joins cover most use cases" is still true.

## Done when

After PR 6 the only *semantic* divergences left are `ASOF` relationships and
anything PR 3 turns up. Everything else is interface shape — a deliberate
choice rather than a gap. That is the point to tag v0.12.0: update the
`## [0.12.0]` date, run the milestone checklist in `CLAUDE.md` (example file,
version bump already done, `just clean-stale` after the tag), and decide PR 7
separately.
