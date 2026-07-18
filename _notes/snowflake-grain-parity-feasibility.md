# Feasibility: Snowflake metric-grain parity (vs. conservative fan-trap fixes)

**Date:** 2026-07-18
**Context:** `_notes/code-review-2026-07-18.md` §2 identified three silent
leak-throughs (EXP-1/2/3) in the fan-trap fence and proposed conservative
fixes (extend the fence, keep erroring). It also named the root cause: the
root-anchored `FROM <first table> LEFT JOIN …` topology is "the single biggest
semantic divergence from Snowflake (which joins only the tables a query needs
and computes each metric at its own grain)". This note assesses the
feasibility of reaching Snowflake parity instead of (or in addition to)
patching the fence.

**Verdict up front: feasible, and the two options are sequential, not
alternatives.** The conservative fixes are, almost exactly, Snowflake's
*validation rule* applied uniformly; the parity work is adding Snowflake's
*computation model* on top, which then converts several of those errors into
correct answers. Land the fence fixes first (small, closes silent-wrong-answer
holes now), then build the per-grain engine as a milestone. No DDL, catalog,
or on-disk model changes are required — parity is purely a query-time
(expansion-layer) restructuring, concentrated in `src/expand/` + `src/graph/`.

---

## 1. What Snowflake actually does (research summary)

Sources: Snowflake docs (SEMANTIC_VIEW construct, querying, validation-rules,
CREATE SEMANTIC VIEW), the Snowflake engineering blog "Why Do We Need Semantic
Views? Solving BI & SQL Traps" (the most explicit statement of the computation
model), the 2026-03-05 semi-additive release note, and several practitioner
write-ups. Firmness flags below; full citations in the research trail.

1. **Per-metric own-grain aggregation** (firm). Each metric is aggregated
   from its own table's raw rows *before* any join; per-grain results are
   then combined on the requested dimensions. The blog shows the generated
   shape: one CTE per fact table aggregating at the linking-key grain, then
   dimension-anchored `LEFT JOIN`s of the aggregates ("when a model is
   aggregated before being joined … it becomes a one-to-one join"). Multiple
   metrics from tables at different grains in one query is the flagship use
   case, not an error.
2. **Granularity validation rule** (firm; error text empirically confirmed in
   production write-ups). A dimension paired with a metric must live on a
   table *related to* and at *equal-or-lower granularity* than the metric's
   table — i.e. the dimension's table must be the metric's table or an
   ancestor of it via many-to-one hops. A finer-grain or unrelated dimension
   is a compile-time error, not a NULL and not a cross join.
   `SHOW SEMANTIC DIMENSIONS … FOR METRIC` enumerates the legal set (we
   already mirror this).
3. **Parent-side metrics are never inflated by child fan-out** (firm on
   intent): `SUM(customers.balance)` is computed over customer rows only.
   The dangerous "aggregate parent over child join" query is *unrepresentable*
   — finer-grain dimensions are rejected by rule 2.
4. **Derived metrics across grains** (inferred, high confidence): each
   component metric is aggregated at its own grain; the derived expression is
   evaluated post-aggregation on the joined per-group values.
5. **Dimensions-only queries** are grouped/deduplicated (firm); whether
   fact-less dimension values appear is undocumented (inferred yes from the
   dimension-anchored join shape).
6. **Facts** come out at the row grain of their logical table, never grouped;
   FACTS and METRICS cannot mix (we already enforce this); facts from
   multiple tables join row-level via declared relationships.
7. **Semi-additive (`NON ADDITIVE BY`)**: sort by the NA dims, aggregate the
   last-row snapshots (firm) — our RANK-CTE approach matches, *modulo* the
   fanned input it currently runs over. **Window metrics** are evaluated over
   the aggregated result groups at the query's dimension grain (firm) — also
   matches our two-stage CTE design.
8. **`WHERE` inside `SEMANTIC_VIEW(...)`** is applied pre-aggregation and may
   reference dimensions/facts only (firm). We don't support it at all today.

**Undocumented gaps** (need empirical testing on a live Snowflake account, or
a documented local decision): the exact combining join type (dim-anchored
LEFT JOIN vs FULL OUTER of per-grain aggregates — observationally equivalent
under rule 2 in most shapes), and whether fact-less dimension values appear.

## 2. The key structural insight

Compare the three regimes for a metric `m` on table `T_m` queried with a
dimension on `T_d`:

| Shape | Today | Conservative fixes | Snowflake parity |
|---|---|---|---|
| `T_d` ancestor-or-self of `T_m` (M2O up) | ok, correct | ok, correct | ok, correct |
| `T_d` finer / sibling (fan edge on path) | **error** (`FanTrap`) | error | **error** (granularity rule) |
| `T_m` ancestor of root, dim on `T_m` (EXP-1) | **silent inflation** | error | **correct** (own-grain agg) |
| Two metrics at different grains | error (`MetricFanTrap`) | error | **correct** (per-grain + combine) |
| One derived metric spanning grains (EXP-2) | **silent inflation** | error | **correct** |
| Active semi-additive over fan (EXP-3) | **silent double-count** | error | **correct** |
| Dims-only / facts | NULL-extended + orphan-dropped rows | unchanged | **correct** (no root anchor) |

Two things fall out:

- **The conservative fixes ARE the Snowflake validation rule.** Extending the
  fence to cover the root as participant (EXP-1), within-metric grain pairs
  (EXP-2), semi-additive metrics (EXP-3), and empty grains (EXP-8) makes the
  met×dim check exactly "every grain table of every metric must reach every
  queried dimension's table without traversing a fan edge" — which is rule 2.
  Nothing in them is throwaway; the reachability predicate they need is the
  permanent validation half of parity.
- **Parity's delta is the compute engine, which *relaxes* the met×met rule.**
  Under parity, the met×met fan check is deleted entirely (multi-grain metric
  sets become legal and correct), and the met×dim check remains as the
  granularity rule. Errors currently guarding correctness become capabilities.

So the sequencing question answers itself: fence fixes first (they close the
silent-wrong-answer surface immediately and are small), engine second (it
upgrades error → correct answer). Tests written for the fence fixes convert
from `statement error` to value assertions when the engine lands — a cheap,
mechanical conversion, and the failing-first repros in the review are reusable
verbatim.

## 3. Target SQL shape

Grain groups are simpler than they first appear: every *base* metric has
exactly one source table (`source_table`, `None` ⇒ first declared table);
derived and window metrics decompose into base metrics. So **a grain group =
the set of requested-or-referenced base metrics sharing one source table**,
and `metric_grain_tables` (`fan_trap.rs:275`) already computes the transitive
grain sets.

For `metrics := [revenue@li, order_count@o], dimensions := [region@r]`:

```sql
WITH __sv_g0 AS (            -- grain {li}
    SELECT r.region AS __k0, SUM(li.price) AS __m0
    FROM line_items AS li
    LEFT JOIN orders    AS o ON li.order_id = o.id      -- M2O up only
    LEFT JOIN customers AS c ON o.customer_id = c.id
    LEFT JOIN regions   AS r ON c.region_id = r.id
    GROUP BY 1
), __sv_g1 AS (              -- grain {o}
    SELECT r.region AS __k0, COUNT(*) AS __m0
    FROM orders AS o
    LEFT JOIN customers AS c ON o.customer_id = c.id
    LEFT JOIN regions   AS r ON c.region_id = r.id
    GROUP BY 1
)
SELECT COALESCE(__sv_g0.__k0, __sv_g1.__k0) AS "region",
       __sv_g0.__m0 AS "revenue",
       __sv_g1.__m0 AS "order_count"
FROM __sv_g0
FULL OUTER JOIN __sv_g1
    ON __sv_g0.__k0 IS NOT DISTINCT FROM __sv_g1.__k0
```

Properties:

- Inside each CTE, joins go **many-to-one upward only** (guaranteed by the
  granularity rule), so no fan-out is possible — the fence assumption becomes
  a structural invariant instead of a checked property. `LEFT JOIN` upward
  preserves NULL-FK fact rows as NULL dimension groups (matches current
  behaviour for NULL FKs).
- `COUNT(*)` inside a grain CTE counts the metric table's own rows — correct
  by construction. The SG-8 `COUNT(*)`→`COUNT(pk)` rewrite (and its
  `CountStarRequiresPrimaryKey` error) becomes unnecessary on this path.
- A derived metric spanning grains (`ratio AS order_total / item_count`)
  becomes outer-select arithmetic over per-grain columns:
  `__sv_g1.__m0 / __sv_g0.__m1 AS "ratio"` — EXP-2 correct by construction.
- Metrics-only (no dims): each CTE is a one-row global aggregate; combine
  with `CROSS JOIN`.
- **Single-grain fast path:** when all requested metrics resolve to one grain
  table, emit today's flat single-SELECT shape — just anchored at the grain
  table instead of the root. Most existing sqllogictest expectations survive
  with only the FROM/JOIN lines changing.

**Combining-join decision** (the one place Snowflake is undocumented):
recommend `FULL OUTER JOIN … IS NOT DISTINCT FROM` on the dimension value
columns (group combos = union across metrics), rather than the blog's
dimension-anchored LEFT JOIN (combos = dimension-table contents, including
fact-less values; ill-defined when dims span multiple branches). Document the
choice explicitly; optionally settle empirically against a Snowflake trial
account later. DuckDB supports `IS NOT DISTINCT FROM` in join conditions; the
planner's handling of it as a hash-join key should be verified with an
`EXPLAIN` spot-check during the spike.

### Per-strategy mapping

- **Semi-additive:** the `__sv_snapshot` RANK CTE becomes the *inner* stage of
  the owning grain group's CTE — anchored at the metric's table, M2O joins up
  to dim + NA-dim tables. No fanned input ⇒ the "RANK ties across fanned
  duplicates" failure mode (EXP-3) is structurally gone, and the fan-check
  skip disappears. The decomposition/validation machinery
  (`parse_snapshot_aggregate`, `collect_na_groups`) is reused as-is.
- **Window:** today's `__sv_agg` CTE (aggregate at query grain, window over
  it) keeps its two-stage design; stage 1 simply becomes the combined
  per-grain result instead of a flat-join aggregate. This matches the
  documented Snowflake semantics (windows run over query-grain groups).
- **Dims-only:** `SELECT DISTINCT` anchored at the dimension source table(s)'
  minimal connecting subtree — no root join ⇒ no NULL-extended root rows, no
  orphan-dropped child rows. When multiple dim tables connect only through a
  common child (chasm shape), the combos are those observed in the child;
  document this (today's behaviour via the root is the same modulo the root
  anchor).
- **Facts:** anchor at the finest requested fact table, join M2O upward
  row-level. Same fix for NULL-extension/orphans. The existing
  `FactPathViolation` single-path rule maps to (and is slightly stricter
  than) Snowflake's relationship requirement; keep it.
- **Materializations:** routing is an exact-match short-circuit and is
  unaffected mechanically. Note in docs that mat tables built under old
  semantics may diverge from the new live path (they already may, per the
  staleness note in the 07-18 review).
- **Role-playing:** scoped-join machinery (`scoped_join_alias`, scoped ON
  synthesis) is reused inside grain CTEs unchanged. EXP-4/5 (descendant /
  facts ambiguity) are orthogonal and needed in both worlds.

## 4. What changes, module by module

The read path is a linear pipeline with one dispatch point
(`expand()` at `sql_gen.rs:288`) feeding four emitters that share the
root-anchored topology via `resolve_joins_pkfk` (`join_resolver.rs:212`) and
`push_from_base` (`select_spec.rs:98`). That concentration is why this is
tractable.

| Module | Change | Est. churn |
|---|---|---|
| `join_resolver.rs` (648) | Add anchored variant: `resolve_joins_from(anchor, needed_aliases)` — same BFS/toposort machinery, parameterised root; M2O-only direction check | +200–400 |
| new `grain.rs` | Grain grouping, per-grain CTE emission, combining join, COALESCE'd dim output, derived-metric outer arithmetic | +400–600 |
| `sql_gen.rs` (493) | Orchestration rework: single-grain fast path vs multi-grain engine dispatch | ~200 |
| `select_spec.rs` (394) | Parametric FROM anchor (drop `def.base_table()` hardcode), combining-join render support | ~150 |
| `facts.rs` (1,341) | Inlining scoped per grain group; derived metrics resolve to CTE-column references instead of full textual inlining; SG-8 rewrite retired on the base path | ~300 |
| `fan_trap.rs` (1,012) | Replace met×dim + met×met loops with one uniform granularity check (ancestor-or-equal via M2O for every metric grain × queried dim); delete met×met entirely | net −400 |
| `semi_additive.rs` (2,208) | Re-anchor CTE at metric grain table; delete the fan-check skip; core CTE internals reused. Do the ARCH-3 decomposition first — refactoring a 339-line function while changing its topology is how regressions happen | ~300 |
| `window.rs` (917) | Stage 1 becomes combined result | ~150 |
| facts/dims-only paths | Re-anchor | ~150 |
| `model.rs`, catalog, DDL, parser | **No change** | 0 |

Total: roughly 2–3k lines of production churn concentrated in `src/expand/`,
plus large-but-mechanical sqllogictest expectation churn (fan-trap error
tests convert to value tests; FROM/JOIN lines change everywhere). For this
project's cadence that is a milestone of ~5–8 phases, not a rewrite: the
model, parser, catalog, FFI seam, wire protocol, and type-inference probe are
all untouched.

## 5. Behavioural changes (breaking-release inventory)

For queries that are legal today *and* fan-free, values are identical except:

1. Root NULL-extension rows disappear from dims-only / facts results
   (bug-fix-shaped, but observable).
2. Child rows orphaned from the root now appear (ditto).
3. Global aggregates of non-root metrics are computed over the metric's own
   table, not the root-anchored join (correct-shaped; today these are only
   legal when fan-free, in which case values match — except via EXP-1 inputs,
   which were silently wrong).
4. `MetricFanTrap` errors for multi-grain metric sets become correct results.
5. `CountStarRequiresPrimaryKey` errors disappear on the base path.
6. EXP-1/2/3 silent-wrong answers become correct answers (via the interim
   fence fixes: first errors, then correct answers).

The project has already shipped a semantics-breaking correction this cycle
(NON ADDITIVE BY polarity), so the precedent and CHANGELOG discipline exist.
`docs/how-to/fan-traps.rst`, `snowflake-comparison.rst`, and
`explain-semantic-view-function.rst` need rewrites in the same milestone
(the fan-trap guide's "three approaches" section becomes largely obsolete —
approach 3, "pre-aggregate yourself", is what the engine now does).

## 6. Test strategy

The review's top PBT recommendation is the acceptance harness for this work:

- **Differential per-grain oracle proptest** (review §4 Top-3 #1): random
  two-table star with dangling/NULL FKs, metrics on both sides; oracle =
  hand-written pre-aggregate-then-join SQL; compare via the existing
  `EXCEPT ALL` comparator. Build it *before* the engine — it fails against
  today's EXP-1 behaviour (satisfying the fix-test-first rule), gates the
  fence fixes (as error-expectation), and then gates the engine (as
  value-equality). Extend with NULLs/empty tables/metrics-only per PBT-2.
- **Semi-additive differential** (Top-3 #2) gates the EXP-3 → per-grain
  conversion.
- Existing 77 sqllogictests carry forward with mechanical expectation
  updates; fan-trap `statement error` tests split into "still errors
  (granularity rule)" and "now returns correct values".

## 7. Risks and open questions

- **Combining-join semantics undocumented in Snowflake** — decide (FULL
  OUTER + `IS NOT DISTINCT FROM`), document as a deliberate interpretation,
  optionally verify empirically later. Low risk: observationally equivalent
  in the shapes the granularity rule permits.
- **Performance:** N grain groups = N scans of overlapping join paths vs one
  flat join today. Same-grain metrics share a CTE; single-grain queries keep
  the flat fast path; DuckDB's optimizer handles the rest. Not a correctness
  risk; benchmark during the spike (`IS NOT DISTINCT FROM` hash-join
  handling included).
- **Cardinality trust is unchanged:** grain rule still trusts declared
  PK/UNIQUE. But the blast radius shrinks — per-grain aggregation is correct
  regardless of the *metric side's* declared cardinality; only a falsely
  declared UNIQUE on a join *target* can still duplicate.
- **`expand_semi_additive` churn risk:** highest-churn logic in the
  least-decomposed function (ARCH-3). Mitigation: decompose first, re-anchor
  second, each under the differential oracle.
- **Facts+dims strictness:** Snowflake restricts FACTS+DIMENSIONS to one
  logical table; we are more permissive (path rule). Keep our behaviour,
  document the divergence — tightening would break users for no correctness
  gain.
- **Future unlock (not in scope):** a pre-aggregation `WHERE` argument
  (Snowflake's biggest remaining query-surface feature we lack) is only
  implementable *correctly* on the per-grain topology — the predicate must
  filter each grain's rows before aggregation. The flat design cannot express
  that for multi-grain queries. Parity work is a prerequisite investment for
  it.

## 8. Recommended sequencing

1. **Now (small, this cycle):** land the conservative fence fixes EXP-1, -2,
   -3, -8 test-first, plus the orthogonal EXP-4/5/6 role-playing/keying
   fixes. This closes every known silent-wrong-answer path immediately and
   builds the granularity-rule predicate parity needs anyway.
2. **Now (test infra):** the differential star-schema + semi-additive
   proptests (review Top-3 #1/#2), wired as the standing oracle.
3. **Milestone "grain parity":** spike the combining-join SQL shape against
   DuckDB (`IS NOT DISTINCT FROM` plan check) → anchored join resolver →
   single-grain re-anchor (fast path; retires root anchoring, SG-8, dims-only
   NULL-extension) → multi-grain engine (delete met×met check) →
   semi-additive re-anchor (after ARCH-3 decomposition) → window stage-1 swap
   → facts re-anchor → docs rewrite + CHANGELOG breaking-change section.

Each step keeps `just test-all` green and is independently shippable; the
engine steps convert specific fence errors to value-correct results with
their tests flipping from `statement error` to value assertions.
