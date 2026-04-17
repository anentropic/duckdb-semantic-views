# Domain Pitfalls -- Snowflake SQL DDL Parity (v0.6.0)

**Domain:** Adding Snowflake SQL DDL parity features to an existing DuckDB semantic views extension
**Researched:** 2026-04-09
**Context:** Extension has 487 tests, 16,342 LOC across expand/ (7 submodules), graph/ (5 submodules), shared util.rs/errors.rs. Definitions stored as JSON in `semantic_layer._definitions` via parameterized prepared statements. C++ shim dynamically forwards VTab output as all-VARCHAR. Single-pass SQL expansion generates `SELECT dims, agg_metrics FROM base LEFT JOIN ... GROUP BY dims`.

---

## Critical Pitfalls

Mistakes that cause rewrites, incorrect query results, or breaking changes.

### C1: Semi-additive metrics require a fundamentally different expansion path

**What goes wrong:** The current `expand()` function in `src/expand/sql_gen.rs` generates exactly one query shape: `SELECT dims, agg_metrics FROM ... GROUP BY dims`. Semi-additive metrics (NON ADDITIVE BY) require a two-stage expansion: first select the "last snapshot" rows per non-additive dimension partition, then aggregate. Trying to bolt this onto the existing single-pass SQL generation produces either incorrect results (aggregating before snapshot selection) or a combinatorial explosion when mixing regular and semi-additive metrics in one query.

**Why it happens:** The expand function treats all metrics identically -- each becomes an aggregate expression in the SELECT list with a shared GROUP BY. NON ADDITIVE BY metrics need a subquery or CTE that filters to the latest snapshot rows before the outer aggregation. Snowflake's behavior: "rows are sorted by the non-additive dimensions, and the values from the last rows (the latest snapshots of values) are aggregated to compute the metric." If both regular and semi-additive metrics coexist in one query request, the expansion must produce a query structure where regular metrics aggregate over ALL rows but semi-additive metrics aggregate only over snapshot-selected rows.

**Consequences:**
- Incorrect aggregation results (inflated semi-additive values) if snapshot selection is skipped
- Overly complex generated SQL if both metric types coexist
- Potential correctness regression in existing tests if the expand function's control flow is restructured carelessly

**Prevention:**
- Design the semi-additive path as a distinct expansion mode using a CTE wrapper. The base CTE joins all tables and selects row-level facts/expressions. An intermediate CTE applies `ROW_NUMBER() OVER (PARTITION BY <non-excluded-dims> ORDER BY <non_additive_dims> DESC) = 1` to pick the latest snapshot row per partition. The outer query then aggregates.
- When only regular metrics are requested, the existing single-pass path must be untouched -- guard the semi-additive path behind a check for any NON_ADDITIVE_BY annotations in the resolved metrics.
- Build the NON ADDITIVE BY expansion as a composable wrapper around the existing `expand()` output, rather than adding branches inside the existing function.
- Test the mixed case explicitly: one query with both `SUM(amount)` (regular) and `SUM(balance) NON ADDITIVE BY (date_dim)` (semi-additive) must produce different aggregation scopes for each metric.

**Detection:** Expansion unit tests that compare regular metric values against semi-additive metric values on the same dataset. If they produce identical results on a dataset with multiple snapshot rows per partition, the semi-additive logic is not activating.

**Phase:** Should be an early feature phase -- it is the deepest structural change to the expansion pipeline and all subsequent features (window metrics, queryable facts) should build on the expanded pipeline.

---

### C2: DuckDB LAST_VALUE IGNORE NULLS crashes on all-NULL partitions (version-dependent)

**What goes wrong:** DuckDB versions prior to the fix for GitHub issue #20136 (merged December 2025 into main) crash with an internal error ("Attempted to access index 0 within vector of size 0") when `LAST_VALUE(expr ORDER BY ... IGNORE NULLS)` encounters a partition where ALL values are NULL. This is the exact pattern one might reach for when implementing semi-additive snapshot selection.

**Why it happens:** The natural DuckDB expansion for NON ADDITIVE BY uses `LAST_VALUE` or `ROW_NUMBER` with ORDER BY. If using `LAST_VALUE` with `IGNORE NULLS` and a partition has all NULLs for the metric column, DuckDB crashes. This is the class of bug the project has worked hard to prevent (see v0.5.0 Phase 17.1 Python crash investigation).

**Consequences:** Extension users with sparse data (NULL metric values in some partitions) hit SIGABRT-level crashes that bypass all Rust safety guarantees.

**Prevention:**
- Use `ROW_NUMBER() OVER (PARTITION BY ... ORDER BY <non_additive_dims> DESC) = 1` for snapshot row selection instead of `LAST_VALUE`. ROW_NUMBER is safer because it selects a row position rather than a value, avoiding the NULL-handling edge case entirely.
- The project currently targets DuckDB 1.5.x, which should include the fix. However, the LTS branch (1.4.x) may not. Verify in CI with both targets.
- Add a proptest generating all-NULL metric columns within semi-additive partitions.
- Add an explicit sqllogictest case with all-NULL partitions and semi-additive metrics.

**Detection:** Proptest with `proptest::option::weighted(0.8, arb_value())` to generate high-NULL-rate partitions. Integration test with explicit all-NULL partition.

**Phase:** Same phase as semi-additive implementation. Must be tested as part of the core semi-additive expansion.

---

### C3: Window function metrics (PARTITION BY EXCLUDING) cannot coexist with GROUP BY

**What goes wrong:** The current expansion always generates GROUP BY when both dimensions and metrics are present. Window function metrics using `PARTITION BY EXCLUDING` produce a per-row result (no aggregation), which is fundamentally incompatible with GROUP BY in the same SELECT. If a query requests both an aggregate metric (`SUM(amount)`) and a window metric (`SUM(amount) OVER (PARTITION BY EXCLUDING date_dim)`), the expansion must handle the fact that SQL mandates window functions operate on the result set AFTER GROUP BY, but Snowflake's PARTITION BY EXCLUDING operates on pre-aggregation rows.

**Why it happens:** A metric defined as `SUM(x) OVER (PARTITION BY ...)` is not an aggregate -- it is a window function. Mixing it with aggregate metrics in the same SELECT clause requires the aggregate metrics to use GROUP BY while the window metric uses OVER. This produces valid SQL only if the window function operates on the grouped result, but Snowflake's PARTITION BY EXCLUDING semantics operate on raw rows, not post-GROUP BY results.

**Consequences:**
- DuckDB error: "window functions cannot appear in GROUP BY clause"
- Or incorrect results: window function computes over grouped rows instead of raw rows
- Users confused by which metrics can be combined in one query

**Prevention:**
- Forbid mixing aggregate metrics and window function metrics in the same query request. Return a clear ExpandError explaining they must be queried separately. Snowflake enforces strict dimension requirements on window metric queries, effectively preventing mixing.
- The simplest correct approach: if any requested metric has PARTITION BY EXCLUDING, ALL metrics in that query must be window functions or bare expressions, and the expansion omits GROUP BY entirely.
- Add a new ExpandError variant `IncompatibleMetricTypes` with a clear message.
- When only window metrics are requested, generate: `SELECT dims, window_metric_expr OVER (PARTITION BY <all-dims-except-excluded> ORDER BY ...) FROM base LEFT JOIN ...` with NO GROUP BY.

**Detection:** Unit test requesting one aggregate metric and one window metric in the same QueryRequest should return IncompatibleMetricTypes error.

**Phase:** Should follow semi-additive metrics. The expansion pipeline will already have CTE capability from semi-additive work.

---

### C4: JSON schema migration breaks existing stored views when adding new model fields

**What goes wrong:** Adding new fields to `SemanticViewDefinition` (comment, synonyms, private, non_additive_by, partition_by_excluding) and sub-structs (Metric, Dimension, Fact) requires careful serde annotation. If a new field is not `#[serde(default)]`, deserialization of ALL existing stored views fails on extension load, rendering the entire catalog inaccessible.

**Why it happens:** The project stores definitions as JSON in `semantic_layer._definitions`. Every `init_catalog` call deserializes all rows. A single missing `#[serde(default)]` on a new field causes `serde_json::from_str` to fail, which propagates as a catalog load error.

**Consequences:**
- Total data loss: existing semantic views become inaccessible
- Extension fails to load entirely (init_catalog returns Err)
- Users must manually edit the JSON in the DuckDB table to fix

**Prevention:**
- **Every new field** on `SemanticViewDefinition`, `Metric`, `Dimension`, `Fact`, `Join`, and `TableRef` MUST have `#[serde(default)]`.
- **Every `Option<T>` field** gets `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- **Every `Vec<T>` field** gets `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- **Every new enum field** gets `#[serde(default)]` with `#[derive(Default)]` on the enum.
- **Write a backward-compat test** that deserializes a JSON snapshot from v0.5.5 (the current version) with the new struct definition. This test must be written BEFORE adding any new fields and must pass after fields are added.
- **Do NOT use `#[serde(deny_unknown_fields)]`** -- the model comment explicitly notes this (model.rs line 160-161).
- v0.5.3 retrospective Key Lesson 1: "Adding a new field to a widely-used struct creates a large blast radius of required changes." Add ALL new annotation fields in a single batch phase.

**Detection:** Integration test loading a v0.5.5 JSON fixture and verifying all fields deserialize with correct defaults. This is the single most important test for this milestone.

**Phase:** Must be the FIRST phase -- model changes underpin every other feature.

---

### C5: GET_DDL round-trip loses expression quoting information

**What goes wrong:** GET_DDL must reconstruct valid DDL from the stored `SemanticViewDefinition` JSON. But the body parser discards certain DDL-level information:
1. **Expression quoting**: The parser strips outer quotes from identifiers. If a user wrote `"Order ID"` in a dimension expression, the stored `expr` field contains the expression without its DDL-level quoting context. Reconstructed DDL may need re-quoting.
2. **Whitespace and formatting**: All formatting is lost during parsing.
3. **Keyword casing**: Original DDL might use mixed case. Reconstruction normalizes to uppercase.
4. **NON ADDITIVE BY sort direction**: Must be stored (ASC/DESC, NULLS FIRST/LAST) and reconstructed faithfully.

**Why it happens:** The body parser (`body_parser.rs`) converts DDL text into struct fields, naturally discarding syntactic sugar. The model stores semantic content, not syntactic form.

**Consequences:**
- Reconstructed DDL, when re-executed, may fail if expression quoting is not restored
- Users expecting exact round-trip get syntactically different (but semantically equivalent) DDL

**Prevention:**
- Expressions are stored as opaque SQL strings -- they were valid when parsed and remain valid in reconstruction. The danger is only in the structural parts (names, aliases, PK columns).
- GET_DDL must re-quote all structural identifiers: table aliases, dimension/metric/fact names, PK column names, relationship names.
- For the entry format `alias.name AS expr`, always emit double-quoted names.
- For new annotations, emit canonical form: `NON ADDITIVE BY (dim1 DESC NULLS LAST)`, `COMMENT = 'text'`, `WITH SYNONYMS = ('s1', 's2')`, `PRIVATE`.
- Write a round-trip proptest: `parse(ddl) -> json -> parse(get_ddl(json))` must produce equal definitions.
- Accept that GET_DDL output is semantically equivalent but not syntactically identical. Document this.

**Detection:** Round-trip proptest is the canonical verification.

**Phase:** GET_DDL should be one of the LAST features -- it must reconstruct ALL new DDL syntax features (COMMENT, SYNONYMS, PRIVATE, NON ADDITIVE BY, PARTITION BY EXCLUDING).

---

## Moderate Pitfalls

### M1: COMMENT strings with single quotes break DDL parsing

**What goes wrong:** Comments like `COMMENT = 'Customer''s total balance'` contain escaped single quotes. The body parser's `split_at_depth0_commas` (body_parser.rs lines 56-59) correctly handles `''` inside strings, but the clause boundary scanner `find_clause_bounds` operates at a higher level and may not correctly handle single-quoted strings that appear in new annotation positions.

**Why it happens:** COMMENT is a new annotation syntax that doesn't follow the existing `alias.name AS expr` pattern. It is a key-value assignment (`COMMENT = 'text'`) that appears either at the view level (before TABLES) or inline on individual entries. The existing parser infrastructure handles entries separated by commas inside clause parentheses, but COMMENT adds a new syntactic form.

**Consequences:**
- Parse errors on valid DDL containing comments with special characters
- Truncated comments if the parser misidentifies the end of the string
- View-level COMMENT may confuse the clause boundary scanner if it appears between clauses

**Prevention:**
- View-level COMMENT should appear before the first clause keyword (TABLES). The clause boundary scanner should skip over `COMMENT = '...'` at the top level.
- Entry-level COMMENT should be parsed within the existing entry parser, after the `AS expr` portion.
- Use the existing single-quote escaping logic consistently.
- Test adversarial strings: empty comments, `''` (escaped quotes), `)` characters, clause keywords inside comments (`COMMENT = 'This has DIMENSIONS'`).
- Add `fuzz_ddl_parse` corpus entries with COMMENT containing special characters.

**Detection:** Proptest generating random strings as comment values (including ASCII printable, single quotes, parentheses, clause keywords).

**Phase:** Metadata system phase.

---

### M2: SYNONYMS parsing ambiguity with expression suffix

**What goes wrong:** The Snowflake syntax `WITH SYNONYMS = ('alias1', 'alias2')` introduces a new keyword sequence after a dimension/metric/fact entry. The parser may confuse `WITH` as part of a SQL expression or misparse the parenthesized synonym list as part of the expression.

**Why it happens:** The existing entry parser finds `AS` to split `name AS expr`. After extracting the expression, it must detect where the expression ends and annotations begin. If the expression ends with a parenthesized group (e.g., `COUNT(*)`), distinguishing between the expression's closing paren and a SYNONYMS paren requires keyword look-ahead.

**Consequences:**
- Silent misparse: synonym strings absorbed into the expression
- Parse error on valid DDL

**Prevention:**
- Parse annotations (COMMENT, SYNONYMS, PRIVATE/PUBLIC) AFTER the expression using keyword detection. After the `AS` keyword split, scan the expression portion for trailing annotation keywords at depth-0 (not inside parens/strings).
- The key insight: annotations use keywords (`COMMENT`, `WITH`, `PRIVATE`, `PUBLIC`) that are unlikely to appear as the LAST token of a valid SQL expression. A depth-0 scan from right-to-left for these keywords should work.
- Alternative: define a strict grammar where annotations appear in fixed order after the expression. Parse greedily for the expression (everything between AS and the first annotation keyword at depth-0).
- Test edge cases: `alias.name AS (CASE WHEN x > 0 THEN 1 END) WITH SYNONYMS = ('alt')`, `alias.name AS func(x) COMMENT = 'note'`.

**Detection:** Test cases with complex expressions followed by annotations.

**Phase:** Same metadata system phase as COMMENT.

---

### M3: PRIVATE metrics interacting with derived metrics

**What goes wrong:** A PRIVATE metric cannot be queried directly. But if a derived (non-private) metric references a private metric in its expression, the system must still inline the private metric's expression during expansion. If private metrics are excluded from the resolution pool, derived metrics that depend on them produce invalid SQL.

**Why it happens:** The `inline_derived_metrics` function in `expand/facts.rs` resolves all metric expressions by walking the dependency graph. If private metrics are filtered out before inlining, derived metrics referencing them have unresolved references.

**Consequences:**
- Derived metrics silently produce invalid SQL
- Or all derived metrics transitively depending on private metrics are blocked

**Prevention:**
- PRIVATE is a query-time filter, not a resolution-time filter. Private metrics must participate fully in the derived metric inlining pass. The PRIVATE check happens only when validating the user's explicit metric request.
- In `expand()`, resolve ALL metrics (including private) for the inlining pass. Then, when validating requested metric names, reject any marked PRIVATE.
- Add a new ExpandError variant: `PrivateMetric { view_name, name }`.
- Test: define `base_metric` (PRIVATE) and `derived_metric AS base_metric * 2` (PUBLIC). Querying `derived_metric` should work; querying `base_metric` should fail.

**Detection:** Unit test with private base metric and public derived metric.

**Phase:** Metadata system phase (PRIVATE/PUBLIC modifiers).

---

### M4: Queryable FACTS mixing row-level and aggregate output

**What goes wrong:** Facts are row-level expressions (no aggregation). If the table function accepts `facts := ['fact_name']` alongside `metrics := ['metric_name']`, the expansion must decide: do facts go in the SELECT without GROUP BY aggregation, or do facts become GROUP BY dimensions? Neither is correct if mixed naively.

**Why it happens:** The current `QueryRequest` has `dimensions` and `metrics`. Adding `facts` creates a third category:
- Dimensions-only: SELECT DISTINCT
- Metrics-only: global aggregate
- Both: GROUP BY dimensions, aggregate metrics
- Facts: row-level expressions, no aggregation

Facts + metrics in the same request is contradictory for a single flat query.

**Consequences:**
- Facts treated as dimensions produce incorrect GROUP BY (inflates cardinality)
- Facts treated as metrics produce errors (they are not aggregates)

**Prevention:**
- **Facts-only mode**: When facts (and optionally dimensions) are requested with NO metrics, generate `SELECT DISTINCT fact_exprs, dim_exprs FROM ...` (no GROUP BY).
- **Disallow facts + metrics in the same request.** Return a clear error: "Cannot mix facts (row-level) and metrics (aggregated) in the same query."
- Add a `facts` field to `QueryRequest`. Check: if `facts` is non-empty AND `metrics` is non-empty, return an error.

**Detection:** Unit test verifying `facts + metrics` returns an error. Unit test verifying `facts + dimensions` produces correct row-level SQL.

**Phase:** Should follow semi-additive and window metric phases.

---

### M5: Fan trap detection not updated for semi-additive and window metrics

**What goes wrong:** The existing `check_fan_traps` function checks every (metric, dimension) pair for one-to-many boundary crossings. Semi-additive metrics change the aggregation semantics (snapshot selection before aggregation), potentially neutralizing certain fan traps. Window metrics bypass GROUP BY entirely, making fan trap detection irrelevant for them.

**Why it happens:** Fan trap detection assumes all metrics are simple aggregates.

**Consequences:**
- False positive fan trap errors on semi-additive metrics that would produce correct results after snapshot selection
- False positive fan trap errors on window metrics that do not aggregate

**Prevention:**
- Add a metric kind annotation: `Regular`, `SemiAdditive`, `Window`.
- In `check_fan_traps`, skip window metrics entirely.
- For semi-additive metrics: still flag fan traps but with a softer message, or skip them if the non-additive dimensions adequately partition the data. The simplest approach: skip fan trap checking for semi-additive metrics since snapshot selection is specifically designed to handle the "latest value" case.

**Detection:** Test semi-additive metric across one-to-many boundary -- should NOT produce fan trap error.

**Phase:** Same phase as semi-additive metrics.

---

### M6: Wildcard expansion (customer.*) with name collisions across tables

**What goes wrong:** `dimensions := ['customer.*']` should expand to all dimensions from the customer logical table. But if two tables define dimensions with the same name (e.g., both `customer` and `order` have a `status` dimension), the wildcard expansion produces duplicates that the existing `DuplicateDimension` check rejects.

**Why it happens:** Wildcard expansion resolves at `bind()` time to the list of dimension names whose `source_table` matches. The resolved names pass to `expand()`, which performs the duplicate check. If another explicitly named dimension collides with a wildcard-expanded name, the error is confusing.

**Consequences:**
- Confusing error messages: "duplicate dimension 'status'" when the user only wrote `customer.*`
- Ambiguity about which dimension "wins" if wildcards from two tables overlap

**Prevention:**
- Resolve wildcards BEFORE passing to `expand()`, in the `bind()` function.
- If explicit names collide with wildcard-expanded names, silently deduplicate (they refer to the same dimension).
- If wildcards from two different tables collide, error with: "Ambiguous dimension 'status' matched by both 'customer.*' and 'order.*'."
- Implement wildcard resolution as a preprocessing step in `bind()`.

**Detection:** Test: `customer.*` alone, `customer.* + explicit customer.name` (dedup), `customer.* + order.*` with collision (error).

**Phase:** Late in the milestone -- after all features have added their fields to the model.

---

### M7: SHOW ... IN SCHEMA/DATABASE scope model mismatch with DuckDB

**What goes wrong:** Snowflake has ACCOUNT > DATABASE > SCHEMA hierarchy with `IN SCHEMA my_db.my_schema` scoping. DuckDB has a similar hierarchy but the extension stores all semantic views in a single catalog table. v0.5.5 added `database_name` and `schema_name` to `SemanticViewDefinition`, but these are stored inside the JSON definition, not as indexed catalog columns. Filtering by schema requires deserializing every definition to check the field.

**Why it happens:** The metadata fields were added for SHOW output display, not for efficient filtering. The current design reads all definitions into `CatalogState` (a `HashMap<String, String>`) at load time -- filtering requires JSON deserialization of each value.

**Consequences:**
- `IN SCHEMA` filtering is O(n) with JSON deserialization per entry
- `IN DATABASE` (no name) must resolve `current_database()`, which requires SQL execution not available in VTab bind

**Prevention:**
- Accept the O(n) cost -- semantic view catalogs are small (typically <100 entries). The VTab already iterates the full catalog for SHOW.
- For "current database/schema" resolution: the extension already captures `database_name` and `schema_name` at define time. At query time, resolve the current database/schema in the DDL rewrite step (in `parse.rs`), not in the VTab. The rewrite can inject the current context as a parameter to the VTab function call.
- Alternatively, resolve in the parse_show_filter_clauses and pass as a string parameter to the VTab.
- Handle edge cases: `IN DATABASE` (no name) = filter by current database, `IN SCHEMA` (no name) = filter by current schema, `IN SCHEMA db.schema` = filter by both.

**Detection:** Test with views created in different schema contexts, then `SHOW IN SCHEMA x` filtering.

**Phase:** SHOW enhancements phase.

---

## Minor Pitfalls

### N1: NON ADDITIVE BY dimension references must be validated at define time

**What goes wrong:** `NON ADDITIVE BY (dim1, dim2)` references dimension names. If those names don't exist in the view's dimension list, the error should be caught at define time, not query time.

**Prevention:** After parsing the metric entry with NON ADDITIVE BY, validate each referenced dimension name against the parsed dimensions list. Emit a ParseError with position if not found, using the existing "did you mean" suggestion pattern.

**Phase:** Semi-additive metrics phase.

---

### N2: PARTITION BY EXCLUDING dimension cross-entity validation

**What goes wrong:** Snowflake requires excluded dimensions be "accessible from the same entity that defines the window function metric." If a PARTITION BY EXCLUDING references a dimension from an unreachable table, the generated SQL references an unavailable column.

**Prevention:** At define time, verify excluded dimension names are from the same table or a table reachable via one-to-one relationships from the metric's source table.

**Phase:** Window function metrics phase.

---

### N3: TERSE mode column subsetting requires VTab bind-time awareness

**What goes wrong:** Snowflake TERSE mode returns 5 columns instead of 8 for SHOW SEMANTIC VIEWS. If the VTab always declares 8 columns, the TERSE flag must either change the bind-time schema or leave columns empty.

**Prevention:** Pass TERSE as a named parameter to the VTab. In bind(), declare only the TERSE column set when the flag is true. This matches the existing pattern where different VTab variants declare different column schemas.

**Phase:** SHOW enhancements phase.

---

### N4: Struct blast radius when adding annotation fields

**What goes wrong:** v0.5.3 retrospective: "Adding a new field to a widely-used struct creates a large blast radius (23+ struct literals)." COMMENT, SYNONYMS, PRIVATE, NON_ADDITIVE_BY, PARTITION_BY_EXCLUDING add 5+ fields across Metric, Dimension, Fact, and SemanticViewDefinition.

**Prevention:**
- Add ALL new annotation fields in a single batch phase.
- Verify all test fixtures use `..Default::default()` pattern. Grep for struct literals that manually specify all fields.
- Consider an `Annotations` sub-struct grouping COMMENT + SYNONYMS + PRIVATE to reduce per-struct blast radius.

**Phase:** First phase (model changes).

---

### N5: GET_DDL ordering must be deterministic for version control

**What goes wrong:** If reconstruction iterates HashMap fields or unordered collections, output order varies between runs.

**Prevention:** Reconstruct in Vec index order (tables, joins, facts, dimensions, metrics). All are stored as Vecs preserving insertion order. Do NOT sort alphabetically -- preserve the user's original declaration order.

**Phase:** GET_DDL phase.

---

### N6: C++ shim column count must be updated for new SHOW columns

**What goes wrong:** Adding columns (comment, synonyms) to SHOW output changes the column count. The C++ shim `sv_ddl_bind` dynamically reads column count from the result, so the schema forwarding is transparent. BUT sqllogictest assertions are column-count-sensitive (`query TTTTT` specifies exact column types and count). Every `.test` file asserting SHOW output must be updated atomically.

**Prevention:** Change VTab output columns and ALL corresponding test expectations in the same commit. Run `just test-all` after every schema change.

**Phase:** Every phase that changes output columns.

---

### N7: View-level vs entry-level COMMENT parsing ambiguity

**What goes wrong:** Snowflake supports COMMENT at the view level (`CREATE SEMANTIC VIEW ... COMMENT = '...' AS TABLES (...)`) and at the entry level (on individual dimensions, metrics, facts). The parser must distinguish between a view-level COMMENT (before TABLES keyword) and the start of the TABLES clause.

**Prevention:** The view-level COMMENT must appear between the view name and `AS` keyword, or between `AS` and the first clause keyword. Since the parser currently expects `AS` followed immediately by clause keywords, insert COMMENT parsing between `AS` and the first clause keyword scan.

**Phase:** Metadata system phase.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Model + backward compat | JSON deserialization failure (C4), struct blast radius (N4) | Every new field: `#[serde(default)]` + backward compat test. Batch all model changes. |
| Semi-additive metrics | Wrong aggregation scope (C1), DuckDB NULL crash (C2), fan trap interaction (M5) | Two-stage CTE expansion, ROW_NUMBER over LAST_VALUE, fan trap bypass for semi-additive |
| Window function metrics | GROUP BY incompatibility (C3), fan trap false positives (M5) | Forbid mixing with aggregate metrics, skip fan trap check for window metrics |
| Metadata (COMMENT/SYNONYMS/PRIVATE) | Parser ambiguity (M1, M2), private metric inlining (M3), view-level COMMENT (N7) | Depth-0 keyword scan for annotations, PRIVATE as query-time-only filter |
| Queryable FACTS | Facts + metrics mixing (M4) | Separate facts-only expansion mode, error on facts + metrics |
| Wildcard selection | Name collision (M6) | Pre-resolve in bind(), deduplicate, error on cross-table ambiguity |
| GET_DDL | Round-trip information loss (C5), ordering fidelity (N5) | Round-trip proptest, canonical ordering, re-quote identifiers |
| SHOW enhancements | Scope model mismatch (M7), TERSE columns (N3), shim column counts (N6) | Filter on stored metadata, TERSE as VTab param, atomic test updates |

## Sources

- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- NON ADDITIVE BY, PARTITION BY EXCLUDING, COMMENT, SYNONYMS, PRIVATE syntax (HIGH confidence)
- [Snowflake querying semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/querying) -- query-time behavior, wildcard selection, fact querying (HIGH confidence)
- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) -- IN SCHEMA/DATABASE, TERSE mode, output columns (HIGH confidence)
- [Snowflake semi-additive release note](https://docs.snowflake.com/en/release-notes/2026/other/2026-03-05-semantic-views-semi-additive-metrics) -- NON ADDITIVE BY behavior: "rows are sorted by the non-additive dimensions, and the values from the last rows are aggregated" (HIGH confidence)
- [Snowflake YAML specification](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- non_additive_dimensions fields, synonyms, access_modifier (HIGH confidence)
- [Snowflake GET_DDL](https://docs.snowflake.com/en/sql-reference/functions/get_ddl) -- semantic view DDL reconstruction (MEDIUM confidence -- limited detail for semantic views specifically)
- [DuckDB issue #20136](https://github.com/duckdb/duckdb/issues/20136) -- LAST_VALUE/FIRST_VALUE IGNORE NULLS crash on all-NULL partitions, fixed Dec 2025 in PR #20153 (HIGH confidence)
- [DuckDB window functions](https://duckdb.org/docs/current/sql/functions/window_functions) -- LAST_VALUE, IGNORE NULLS, frame specifications (HIGH confidence)
- [Serde field attributes](https://serde.rs/field-attrs.html) -- skip_serializing_if, default, backward compatibility patterns (HIGH confidence)
- [DuckDB SHOW FROM/IN discussion](https://github.com/duckdb/duckdb/discussions/16083) -- IN SCHEMA/DATABASE not natively supported in DuckDB SHOW (MEDIUM confidence)
- Codebase: `src/expand/sql_gen.rs` -- single-pass expansion with GROUP BY (HIGH confidence, direct inspection)
- Codebase: `src/expand/fan_trap.rs` -- cardinality-based fan trap detection (HIGH confidence, direct inspection)
- Codebase: `src/model.rs` -- SemanticViewDefinition serde annotations (HIGH confidence, direct inspection)
- Codebase: `src/body_parser.rs` -- clause boundary scanning, entry parsing (HIGH confidence, direct inspection)
- Codebase: `src/parse.rs` -- DDL detection, SHOW filter parsing (HIGH confidence, direct inspection)
- Project RETROSPECTIVE.md v0.5.3 -- "Adding a new field to widely-used struct creates large blast radius" (HIGH confidence)
- Project TECH-DEBT.md item 12 -- DDL pipeline all-VARCHAR result forwarding (HIGH confidence)
