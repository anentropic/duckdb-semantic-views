# Epic: DuckDB 2.0 PEG grammar and full Snowflake parity

**Drafted:** 2026-10-12, against `main` at v0.13.0 (DuckDB v1.5.6).
**Evidence:** `_notes/peg-parser-research-2026-10-12.md` (DuckDB 2.0 grammar API,
the current Snowflake surface, and the inventory behind §3 of this plan).

## 1. Why

Most of the extension's remaining limitations come from how it has to talk to
DuckDB's parser, not from the semantic model. Today every statement goes through a
`parser_override` hook. Hand-written Rust scanners decide whether the text is ours,
parse it, and lower it into SQL text that DuckDB then parses again. That design is
the cause of:

- **the query surface**: `semantic_view('v', dimensions := [...])` instead of
  Snowflake's `SEMANTIC_VIEW(v DIMENSIONS ... METRICS ... WHERE ...)`, and the
  `where_clause :=` spelling, which exists only because `where` is reserved in a
  named-parameter slot;
- **query-time expressions and aliases** (#76), which are impossible without a grammar;
- **SHOW/DESCRIBE in subquery position** (#77). We only recognise statement *prefixes*;
- **a whole class of scanner bugs**: #24, #25, #38, #46, #59, #64/PARSE-9, #73 and
  PARSE-12, plus the double-parse error path (#22) and the `disable_peg_parser`
  interaction (#21);
- **~11k lines of Rust** (parse/, body_parser/ mechanics, statement-level util/ident
  helpers) and **~750 lines of C++** hook plumbing, plus roughly 8k lines of tests
  and three fuzz targets that only exist to guard those scanners.

DuckDB 2.0 moves its PEG parser into core and gives C++ extensions a supported way
to add grammar (`GrammarExtension` / `GrammarChange`). That lets our statements become
real productions: they parse wherever DuckDB's own statements parse, with DuckDB's
expression grammar inside them.

**Goal:** finish with full Snowflake semantic-view parity on DuckDB 2.0, with the
workaround layer deleted rather than wrapped. The tests, oracles, persistence model
and round-trip contracts that make the extension trustworthy all come through intact.

## 2. What DuckDB 2.0 gives us (summary; details and sources in the research note)

- **Timeline.** 2.0.0 is scheduled for **2026-10-21**. The 1.5.x line reaches end of
  life on **2026-11-01**. The PEG parser is the default in 2.0, and libpg_query is
  gone. 1.5.6 has **no** grammar-extension API: its PEG parser is opt-in and lives
  inside the autocomplete extension.
- **API.** You subclass `GrammarExtension`, return `vector<GrammarChange>`
  (`AddRule`, `AddChoice`, `PrependChoice`, `ReplaceChoice`, `ReplaceRule`,
  `SetTransformProcess`, ...), and register with
  `GrammarExtension::Register(db, ...)`. Each new rule gets a transform callback that
  turns the parse result into DuckDB AST nodes, and it can call the stock transformer
  for sub-rules such as expressions.
- **Hook points we need all exist:** `CreateStatementVariation`,
  `DescribeStatement`/`ShowOrDescribe`, `DropEntries`, `InnerTableRef`, `SelectAtom`.
  DuckDB's own demo (`test/extension/loadable_grammar_extension_demo.cpp`, pipe SQL)
  is the reference.
- **C++ only.** None of this is in the C API, so it lives in `cpp/src/shim.cpp`.
  That does not change our ABI posture. We already compile the amalgamation, use
  `C_STRUCT_UNSTABLE`, and pin DuckDB exactly (TECH-DEBT "Known Architectural
  Limitations" #2).
- **Activation is per connection.** `SET active_grammar_extensions = [...]` is
  local-scope, so we must turn it on for every connection, e.g. via an
  `OnConnectionOpened` callback. duckdb-psql (ported 2026-10-06) is a working example.
- **The parse is context-free.** The transformer has no `ClientContext`, so it cannot
  look up whether `FROM sales` is a semantic view. That matters for the
  standard-SQL query form (Phase 5).
- **The API is still moving.** The August blog snippet no longer matches `main`.
  Phase 1 is a spike for this reason.

## 3. Keep, rewire, delete

The full inventory is in the research note. In short:

**Keep unchanged (the domain, ~37k LOC):** `model.rs`, `graph/*`, `expand/*`,
`catalog/mod.rs`, `render_ddl.rs`, `render_yaml.rs`, `errors.rs`, `ddl/*` row
producers, `expr_tokens.rs`, `ffi_util.rs`, `body_parser/cross_refs.rs` (moves out of
`body_parser`), and the shim's function registration and bind/exec engine.

**Keep but rewire:**
- `parse/native_sql.rs` and `catalog/writes.rs`. Lowering DDL to DML on the caller's
  connection is what makes our DDL transactional, and it stays. Its *input* changes
  from a scanned `RewriteAction` to the grammar transform's output.
- `RewriteAction` / `CreateMode` survive as the IR between the C++ transform and Rust.
- `query/*`. Expansion and the type probe stay. The entry becomes the grammar's table ref.
- `ident.rs`. `ident_matches` and normalisation stay; name-slot slicing goes.
- `util.rs`. `suggest_closest` stays; statement-level quote and comment helpers go.
  `QuoteState` stays for as long as expressions are stored as text.
- `lib.rs` `REGISTRATIONS`. The parser-hook entry is replaced by grammar registration
  and the per-connection activation.

**Delete:**
- `parse/{ffi,detect,rewrite (scanning half),create_body,show_clauses,search_path}.rs`
- `body_parser/{lexer,cursor,scan,clause_bounds}.rs` and the recursive-descent
  mechanics of the clause parsers. The semantic checks inside them (duplicates,
  PRIVATE rules, verbatim-representability) move into the transform layer.
- shim `sv_parser_override`, `sv_parse_stub`, `sv_plan_unreachable`,
  `sv_register_parser_hooks`, and `cpp/include/parser_extension_compat.hpp`.
- The `search_path :=` text injection. The grammar transform adds the argument
  structurally.
- `tests/parser_hook_idempotent.rs` and `tests/registration_order_guard.rs`. Both are
  structural checks on the hook, and that hook will be gone.

**Port the cases, not the harness (coverage-forward, per CLAUDE.md):**
- ~3.5k lines of `rewrite.rs` unit tests, ~4.3k of `body_parser` tests,
  `parse_proptest`, `create_front_door_proptest`, `crlf_body_frontdoor`,
  `quoted_idents_regression`, the `error_caret_*` / `cr*_parser_*` / `phase2x`
  sqllogictests, and fuzz targets `fuzz_ddl_parse`, `fuzz_keyword_body` and
  `fuzz_parser_override_ffi`.
- These encode the accepted and rejected DDL language. Each case becomes a row in an
  accept/reject corpus run against the new grammar (sqllogictest plus a Rust-side
  corpus test).
- Error wording and caret positions will change, so cases assert on the new messages.
  Do not drop them.
- A case that no longer applies, because the grammar makes the bug unrepresentable,
  is listed in its phase's PR with the reason.

**Untouched:** all numeric oracles (differential, star_schema, multi_hop_join,
child_grain, semi_additive, window_metric, metamorphic, expand, output) and the
expansion, JSON, YAML, where-predicate and query-name fuzz targets. These are the
safety net the migration is measured against. `roundtrip_proptest`, `yaml_proptest`
and `fuzz_render_roundtrip` are re-pointed from `parse_keyword_body` to the new parse
entry, so the `GET_DDL` → parse contract keeps its guard.

**Mechanical churn to expect:** 83 of 112 `.test` files call
`semantic_view(...)`. Converting them is a scripted rewrite, landed as its own PR so
review can tell syntax churn from semantic change.

## 4. Snowflake parity map (beyond what we ship today)

Checked against docs.snowflake.com on 2026-10-12. "P#" is the phase in §5.

| Area | Snowflake | Ours today | Phase |
|---|---|---|---|
| `SEMANTIC_VIEW(v DIMENSIONS … METRICS … FACTS … WHERE …)` as a table expression (joins, CTEs, PIVOT) | GA | table function | P4 |
| Query items as expressions with `[AS] alias`, `tbl.*`, ad-hoc aggregates of dims/facts in METRICS | GA | names only (#76) | P4 |
| Standard SQL: `SELECT d, AGG(m) FROM sv WHERE … GROUP BY … HAVING …` | GA 2026-03 | none | P5 |
| SHOW/DESCRIBE usable as subqueries | (DuckDB idiom) | no (#77) | P3 |
| `data_type` in SHOW/DESCRIBE | populated | empty (#51) | P5 |
| BI discovery (view appears with typed columns) | n/a (Snowflake catalog) | none | P5 |
| `VARIABLES (...)` + query-time `VARIABLES v => val` | GA 2026-06 | none | P6 |
| Logical table as `alias AS (<query>)` | GA 2026-06 | none | P6 |
| `CREATE OR ALTER SEMANTIC VIEW` | preview | rejected | P6 |
| `SHOW … IN ACCOUNT`, `LIMIT n FROM '…'`, `GET_DDL('SEMANTIC VIEW', …)` spelling, `DERIVED_METRIC` kind in DESCRIBE, `required` column in `… FOR METRIC` | GA | partial | P6 |
| Metadata clauses: `AI_SQL_GENERATION`, `AI_QUESTION_CATEGORIZATION`, `AI_VERIFIED_QUERIES`, `TAG` / `SET TAG`, `COPY GRANTS`, `SAMPLE_VALUES`, `IS_ENUM`, `WITH CORTEX SEARCH SERVICE`, `MAX_STALENESS` | GA | rejected | P6 (accept + store + round-trip; decision D5) |
| Multi-path `USING` (several relationships) | preview 2026-03 | single | P6 |
| `ASOF` relationships | GA | not planned | P7 |
| Range relationships (`CONSTRAINT DISTINCT RANGE` + `BETWEEN … EXCLUSIVE`) | preview | none | P7 |
| Many-to-many via bridge tables | GA | none | P7 |
| Materializations (`ADD/DROP/REFRESH MATERIALIZATION`) | limited access | our own `MATERIALIZATIONS` clause | **out of scope for this epic** (per Paul, 2026-10-12) |
| Composable views (`IMPORTS`) | private preview | none | watch only |
| Privileges, PRIVATE visibility by role, Cortex Search | — | n/a in DuckDB | out of scope |

**Altertable features that go beyond Snowflake.** Paul asked for these to get a look
too (2026-10-12): metric `FILTER (WHERE …)`, `OFFSET (dim BY interval)`,
`CUMULATIVE`, and a built-in time spine (`TIME (col)` / `metric_time`). They are not
Snowflake syntax, and CLAUDE.md makes Snowflake the model for semantics, so they get
their own phase (P8) after parity. Each needs an explicit adopt / skip call.

Caveat on Altertable: our comparison is from the README Sylvain sent. The research
pass could not find their source repo publicly, so "they vendored the PEG parser on
1.5" is their claim, not something we have read.

## 5. Phases

One PR open at a time. Each waits for Paul's Copilot review and merges on his word.
Every PR passes `just test-all`. Any query-semantics change extends at least one
numeric oracle (CLAUDE.md).

### Phase 0: semantic foundations on v1.5.6 (now, before the engine moves)

Expansion-layer work that does not touch the parser. It should land while the parser
is still stable, so the differential harnesses judge it without migration noise.

- **TECH-DEBT #71 (HIGH)**, a metric's value changing with its co-queried metrics.
  See §6. It needs decision D1.
- #72 (disconnected base table accepted), #69/#70 (role-play × fact inlining /
  quoted relationship name). Small, and in the same emitters #71 touches.
- Optional: `feat/dim-metric-type-inference` (April) is checked for anything
  reusable for #51.

### Phase 1: spike on `v2.0-cyanoptera` (go / no-go)

Throwaway branch; the output is a `_notes/` write-up.

1. Register a `GrammarExtension` from our shim, where the amalgamation copy registers
   into the host's `DatabaseInstance`, and confirm it loads under the CLI and the
   Python client (the Phase 15 visibility trap).
2. Activate it per connection, including connections our own Rust code opens with
   `duckdb_connect`, and connections opened before `LOAD`.
3. One production each:
   - `CREATE SEMANTIC VIEW x AS TABLES (...) DIMENSIONS (...)` → transform → Rust IR
     → existing `native_sql` lowering;
   - `FROM SEMANTIC_VIEW(x DIMENSIONS a METRICS m)` → transform → our existing table
     function;
   - `SELECT * FROM (SHOW SEMANTIC VIEWS)`.
4. Measure what users see on a syntax error inside our rule, and on DuckDB's
   autocomplete.
5. Expression capture: can the transform get the **source text span** of an
   `Expression` child as well as its AST? That decides D4.
6. Check duckdb-rs / `libduckdb-sys` and `extension-ci-tools` status for 2.0, and how
   the community-extension CI builds against it.

Exit: a written go / no-go. On no-go, the fallback is the 2.0 token-based
`parse_function` hook. It is less capable, but it still removes the hand lexer.

### Phase 2: engine bump to DuckDB 2.0, behaviour unchanged

Bump `.duckdb-version` and the crate pins. Port the existing hook to 2.0's changed
`parse_function` signature (token vector in, `consumed_tokens` out). All tests stay
green with **no** user-visible change. This separates "new engine" from "new parser"
so a regression can be bisected to one or the other. Decision D2 decides whether
1.5.x keeps a maintenance branch.

### Phase 3: DDL and introspection as grammar productions

- CREATE (including `FROM YAML`), ALTER, DROP, DESCRIBE, every SHOW form, and
  `GET_DDL` become productions that transform into the Rust IR.
- Delete the §3 "delete" list in the same phase, across as many PRs as review needs.
  The grammar test corpus lands **before** the deletion PR, green against both
  implementations, so coverage provably carries over.
- Closes **#77** (SHOW/DESCRIBE as subqueries; restore the six removed doc examples),
  **#21**, **#22**, **#59**, **#60** (a DDL-text fuzz target against the grammar),
  and **#64/PARSE-9**.
- Updates MAINTAINER.md (src layout, fuzz list), `_notes/architecture.md` (currently
  describes v0.5.4; rewrite it), and TECH-DEBT accepted-decision #8 ("statement
  rewrite approach").

### Phase 4: `SEMANTIC_VIEW(...)` query construct and query-time expressions

- The grammar rule on `InnerTableRef` is
  `SEMANTIC_VIEW(<name> [METRICS|FACTS …] [DIMENSIONS …] [WHERE …] [VARIABLES …])`,
  with items `<expr> [[AS] alias]` and `tbl.*`. Clause order sets column order, as in
  Snowflake. `WHERE` takes uncorrelated subqueries.
- **#76** in full: expression items, aliases, Snowflake's same-logical-table rule for
  dimension/fact expressions, and ad-hoc aggregates of dims/facts as metrics.
- Emission gains expression GROUP BY keys on every path: base, per-grain,
  semi-additive snapshot, window, and materialization routing.
- Fences walked past (CLAUDE.md): the fan-trap checks, role-playing ambiguity and
  `USING`, the source-table reachability scanner, and the validation choke points.
  The PR enumerates each one.
- Oracles: `star_schema_proptest` and `multi_hop_join_proptest` generate expression
  items and mirror them.
- Decision D3 settles the fate of `semantic_view()` and `where_clause :=`.
- The scripted `.test` conversion PR lands here.

### Phase 5: standard-SQL querying, member types, BI discovery

- **#51 first.** Record each member's `data_type` at CREATE by binding a LIMIT 0
  probe in the caller's transaction. Then SHOW, DESCRIBE and placeholders have real
  types.
- **`SELECT … AGG(m) … FROM sv … GROUP BY …`.** The parse is context-free (§2), so
  it cannot know `sv` is semantic. A spike picks one of:
  - (a) a grammar rule that recognises a SELECT containing `AGG(` and defers it to a
    bind-time table function holding the statement, which resolves `sv` and either
    expands or re-plans the statement as plain SQL;
  - (b) a catalog placeholder view per semantic view, plus an optimizer or binder
    extension that rewrites the plan;
  - (c) a replacement-scan approach.
  Snowflake's restrictions (no joins, window calls, QUALIFY or correlated subqueries
  in this form) bound the work.
- **BI discovery.** Placeholder views in `information_schema` / `duckdb_views()` with
  one typed column per public member. Decision D6 settles the namespace sharing.

### Phase 6: remaining DDL and metadata parity

- `VARIABLES` (definition plus query-time binding), inline-query logical tables, and
  `CREATE OR ALTER`.
- Multi-path `USING`.
- The `SHOW` option set (`IN ACCOUNT`, `LIMIT … FROM`), `DESCRIBE` kinds and
  properties (`DERIVED_METRIC`, `CONSTRAINT`, `LABELS`, `required`), and the
  `GET_DDL('SEMANTIC VIEW', …)` spelling.
- The metadata-only clauses (D5).
- Each item is a small grammar plus model change. The round-trip proptest catches
  omissions in GET_DDL and YAML.

### Phase 7: relationship modelling parity

`ASOF` relationships, range relationships, and many-to-many bridges. Each is a new
join shape: new emitters, new fan-trap reasoning, and a new oracle in the
differential harnesses. #71's home-table design (§6) is the base they build on. The
bridge semantic (count once per group) is Altertable's, and it matches Snowflake's
documented intent.

### Phase 8: beyond Snowflake (adopt / skip per item)

Metric `FILTER`, `OFFSET`, `CUMULATIVE`, and time spine. Each item gets a one-page
proposal with its syntax and oracle story, and lands only if adopted.

## 6. TECH-DEBT #71: before, during, or after the epic?

**It does not depend on the parser.** The defect lives entirely in `src/expand`:
`per_grain` anchors each grain's CTE at its own table, while a single-grain request
is anchored at the base table (`expand::sql_gen` / `per_grain::is_eligible`). Nothing
in the grammar work changes it, and nothing in it needs the grammar. It has a
plausible implementation on v1.5.6 today.

**Recommendation: do it first, as Phase 0.** Three reasons:

- Phases 4, 5 and 7 (expression items, ad-hoc aggregates, standard-SQL queries,
  bridges, ASOF) all add emission shapes. Each would otherwise be built twice, once
  for each anchoring.
- It is a numbers change, so it should be judged by the differential and metamorphic
  harnesses while those are the only thing moving. Mixing it into a parser migration
  would blur which change moved a number.
- The `#[ignore]`d `mm3_metric_value_changes_when_co_queried_across_grains` already
  states the correct invariant, so it is a ready-made red test.

**Shape of the fix:** one anchoring rule everywhere, so a metric's number never
depends on its companions. The natural candidate is the per-grain rule we already
have for multi-grain queries, which is also Altertable's design: every metric is
aggregated from its **home table** with only many-to-one joins outward, then the
blocks are merged on the dimensions. That means single-grain child metrics are
re-anchored too. It is a deliberate change to some current single-grain numbers, so
it goes in the CHANGELOG under `Changed`.

It still needs a semantic call, D1, because it changes two edge cases:

- a child row whose FK is NULL is **counted**. Today it is dropped by the
  base-anchored join;
- a parent with no children reports `NULL` for a child `COUNT`, not `0`, unless we
  coalesce.

Snowflake's docs don't settle either case. The parity plan's maintainer-run
Snowflake check (`SNOWFLAKE-PARITY-PLAN.md` §3) is the cheapest way to be sure, and
the same fixture answers both.

## 7. Decisions only Paul can make

| # | Question | Recommendation |
|---|---|---|
| D1 | #71 semantics: count orphan child rows (FK NULL)? Childless parent's child-COUNT = `NULL` or `0`? | Home-table anchoring: orphans counted, `NULL` for childless, unless a Snowflake run says otherwise. Run the fixture on a Snowflake account if you can. |
| D2 | Drop DuckDB 1.x when Phase 2 lands? | Yes. 1.5 is end of life on 2026-11-01. Tag v0.13.x as the last 1.5 line (fixes only), and make 2.0 the only target from then on. |
| D3 | Keep `semantic_view('v', dimensions := …)` after `SEMANTIC_VIEW(...)` ships? | Keep it one release as a documented, deprecated alias that maps onto the same IR, then remove it. `explain_semantic_view` becomes a `semantic_view_sql`-style function over any statement. |
| D4 | Store member expressions as verbatim source text (today) or as DuckDB-normalised text? | Verbatim text, validated by parsing, if the Phase 1 spike can capture spans. That preserves `GET_DDL` fidelity. Moving to AST storage would also retire #73 and IDENT-2/3/4, but it is a separate decision. |
| D5 | Snowflake metadata clauses (`AI_*`, `TAG`, `COPY GRANTS`, `SAMPLE_VALUES`, `IS_ENUM`, Cortex search, `MAX_STALENESS`): reject (today), or accept and store? | Accept, store, show in DESCRIBE, and round-trip through GET_DDL/YAML, so Snowflake DDL pastes as is. `COPY GRANTS` and `MAX_STALENESS` are accepted as no-ops. |
| D6 | BI placeholder views share the table/view namespace (a semantic view can't share a name with a table). OK? | Yes, behind the Phase 5 spike. It is what lets BI tools see them at all. |
| D7 | Version line for the 2.0 cut? | Ship Phase 2 as v0.14.0. Consider v1.0.0 when Phase 4 lands with the Snowflake query construct. |

## 8. Done when

Every row in §4 is either shipped or recorded in `snowflake-comparison.rst` as a
deliberate, reasoned divergence. The §3 delete list is gone from `src/`. The only
parser-specific code left is the grammar and its transforms. No numeric oracle lost a
generator axis along the way.
