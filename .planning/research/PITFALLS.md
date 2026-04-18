# Domain Pitfalls -- YAML Definitions & Materialization Routing (v0.7.0)

**Domain:** Adding YAML as a second definition format and materialization routing to an existing DuckDB semantic views extension
**Researched:** 2026-04-17
**Context:** Extension has 705 tests, 25,983 LOC across expand/ (7 submodules), graph/ (5 submodules), shared util.rs/errors.rs. Definitions stored as JSON in `semantic_layer._definitions` via parameterized prepared statements. C++ shim dynamically forwards VTab output as all-VARCHAR. Expansion generates complex SQL with CTE-based semi-additive/window metric pipelines, fan trap detection, role-playing dimensions, derived metrics with DAG validation, and PRIVATE/PUBLIC access control. Current definition path: SQL DDL -> body_parser.rs -> SemanticViewDefinition -> JSON -> catalog. Adding a second input path (YAML) and a query-time interception layer (materialization routing) touches nearly every module.

---

## Critical Pitfalls

Mistakes that cause rewrites, data correctness bugs, or security vulnerabilities.

### Pitfall 1: Re-aggregation of Non-Additive Metrics Produces Silently Wrong Results

**What goes wrong:** A materialization table stores pre-aggregated data at dimension granularity (region, month, product). A query requests a subset (region, month). The routing engine matches the materialization and wraps it with `GROUP BY region, month`. For SUM and COUNT, this is correct -- they are additive. For COUNT(DISTINCT customer_id), AVG(price), or PERCENTILE_CONT(0.5), re-aggregation produces wrong numbers. Summing daily unique users to get monthly uniques overcounts. Averaging averages is not the same as averaging raw values.

**Why it happens:** The additivity check is easy to forget when the routing code path works perfectly for SUM/COUNT. Every metric expression in this codebase is a raw SQL string (e.g., `"SUM(amount)"`, `"COUNT(DISTINCT customer_id)"`), and there is no structured representation of the aggregate function type. Detecting additivity requires parsing the SQL expression to extract the outermost aggregate function.

**Consequences:** Users get wrong query results with no error or warning. This is the worst possible failure mode for a semantic layer -- the entire value proposition is correct results.

**Prevention:**
1. Classify metrics as additive/non-additive at define time. Parse the outermost function from the metric expression and store an `Additivity` enum (Additive, NonAdditive, DerivedUnknown) in the `Metric` model.
2. During materialization matching, reject matches where any requested metric is non-additive AND the query dimensions are a proper subset of the materialization dimensions (i.e., re-aggregation would be needed).
3. Non-additive metrics in a materialization can only be used when the query dimensions EXACTLY match the materialization dimensions (no re-aggregation needed).
4. Additive functions: SUM, COUNT, MIN, MAX. Non-additive: COUNT(DISTINCT ...), AVG, MEDIAN, PERCENTILE_CONT/DISC, any user-defined expression that cannot be determined. AVG can be decomposed into SUM/COUNT for re-aggregation if both components are stored.
5. When unsure about additivity (complex expressions, nested functions), default to non-additive -- correctness over performance.

**Detection:** Unit tests comparing materialization-routed query results against raw-table expansion for identical inputs. Property-based tests with random dimension subsets for each aggregate function type. Mandatory test case: `COUNT(DISTINCT x)` with subset dimensions must NOT match a materialization that requires re-aggregation.

**Phase:** Materialization routing engine phase. Must be foundational to the matching algorithm.

---

### Pitfall 2: Semi-Additive and Window Metrics Bypass Materialization Correctness

**What goes wrong:** The existing codebase has semi-additive metrics (NON ADDITIVE BY with CTE-based ROW_NUMBER snapshot selection) and window function metrics (PARTITION BY EXCLUDING with CTE-based inner aggregation + outer window SELECT). These metrics have complex expansion pipelines that cannot be replicated by simple `SELECT ... FROM materialization GROUP BY ...` re-aggregation. A materialization router that only checks metric names and dimensions without understanding these special metric types will route to a materialization and produce wrong results.

**Why it happens:** Semi-additive metrics depend on having raw-granularity rows to pick the latest snapshot via ROW_NUMBER. Window metrics depend on inner aggregation + outer window application. Both require the full CTE pipeline. A pre-aggregated materialization has already collapsed the rows, destroying the information needed for these operations.

**Consequences:** Snapshot metrics return wrong values (not the latest snapshot). Window metrics compute windows over the wrong partition boundaries. Both are silent correctness failures.

**Prevention:**
1. During materialization matching, automatically exclude any query that includes semi-additive metrics (those with `non_additive_by` populated) or window metrics (those with `window_spec` populated).
2. These metrics should NEVER be served from a materialization unless the materialization was specifically built to store the output of that exact CTE pipeline (i.e., the materialization dimensions exactly match the query dimensions AND the materialization was built for that specific metric). Even then, this is fragile.
3. Simpler rule: if any requested metric is semi-additive or windowed, skip materialization routing entirely and fall back to raw expansion.

**Detection:** Test that semi-additive and window metrics are never routed to materializations. Negative test cases for the matching algorithm.

**Phase:** Materialization routing engine phase, implemented alongside the additivity check.

---

### Pitfall 3: serde_yaml is Deprecated and Alternatives Have Soundness Issues

**What goes wrong:** The obvious dependency choice `serde_yaml` is archived and unmaintained. The fork `serde_yml` had soundness issues (segfaults in the serializer) and its GitHub project was also archived. Choosing the wrong YAML crate leads to either unmaintained dependencies triggering `cargo-deny` advisories, or soundness bugs causing crashes in the extension (which runs inside DuckDB's process).

**Why it happens:** The Rust YAML ecosystem is fragmented after dtolnay deprecated serde_yaml. Multiple forks exist with varying quality, and the situation is still evolving.

**Consequences:** cargo-deny CI failures. Potential segfaults in DuckDB process from unsound YAML parsing. Advisory warnings on the community extension registry.

**Prevention:**
1. Use `serde_yaml_ng` (maintained fork of serde_yaml using unsafe-libyaml) as the primary choice -- it has the most traction and closest API compatibility with the original serde_yaml.
2. Alternative: `serde-saphyr` which uses a pure-Rust YAML parser (saphyr-rs), eliminating unsafe code entirely. Better for security but newer and less battle-tested.
3. Pin the chosen crate version exactly (matching the project pattern of `= version` pins for duckdb).
4. Test: fuzz the YAML parser with adversarial inputs including the billion laughs payloads (anchor bombs). Add a `fuzz_yaml_parse` target alongside the existing `fuzz_ddl_parse` and `fuzz_json_parse`.
5. Document the choice rationale in TECH-DEBT.md.

**Detection:** `cargo-deny` CI step will catch unmaintained/unsound advisories. Fuzz testing catches crashes.

**Phase:** First phase -- YAML parsing infrastructure must be established before YAML definitions can be implemented.

---

### Pitfall 4: YAML Anchor/Alias Bombs (Billion Laughs Attack)

**What goes wrong:** YAML's anchor (&) and alias (*) features allow recursive references that cause exponential memory expansion during deserialization. A 1KB YAML file can expand to gigabytes of memory, crashing the DuckDB process. Since this extension accepts user-provided YAML (both inline and from files), this is a real attack surface.

**Why it happens:** YAML's expressive power (anchors, merge keys, alias expansion) is a security liability when processing untrusted input. Most YAML libraries expand aliases eagerly during parsing.

**Consequences:** DuckDB process crash from OOM. Denial of service. Since extensions run with full process privileges, there is no sandboxing to contain this.

**Prevention:**
1. Set a maximum input size limit for YAML definitions (e.g., 1MB -- no legitimate semantic view definition would be larger).
2. After deserialization, validate that the resulting `SemanticViewDefinition` struct has reasonable cardinality (e.g., max 10,000 dimensions, max 10,000 metrics). This catches bomb payloads that expand into huge structures.
3. If using `serde-saphyr`, check if it has built-in anchor depth/expansion limits. If using `serde_yaml_ng`, the underlying unsafe-libyaml may not have limits -- the input size cap is the primary defense.
4. Consider disabling anchor/alias processing entirely if the YAML library supports it. Semantic view definitions should not need anchors.
5. The fuzz target (`fuzz_yaml_parse`) must include anchor bomb patterns.

**Detection:** Fuzz target with crafted anchor bomb payloads. Unit test verifying that oversized YAML inputs are rejected before parsing. Memory-limited test environment.

**Phase:** YAML parsing infrastructure phase, as part of input validation.

---

### Pitfall 5: Dual Definition Format Creates Model Drift

**What goes wrong:** SQL DDL and YAML produce the same `SemanticViewDefinition` struct via different parsing paths. Over time, new features are added to the SQL DDL body_parser but not to the YAML parser (or vice versa). GET_DDL round-trip works for SQL-created views but breaks for YAML-created views. SHOW/DESCRIBE commands display different information depending on how the view was created.

**Why it happens:** Two input parsers targeting the same model is inherently fragile. The existing codebase has extensive backward-compat tests for the JSON serialization format, but those test the storage layer, not the input parsing layer. Each new model field (like `comment`, `synonyms`, `access`, `non_additive_by`, `window_spec`) added in future milestones needs to be implemented in BOTH parsers.

**Consequences:** Feature asymmetry between SQL and YAML definitions. User confusion. Bugs where YAML definitions silently drop metadata that SQL definitions preserve. GET_DDL produces incorrect output for YAML-created views.

**Prevention:**
1. **Shared validation layer.** Both SQL DDL and YAML parsing must produce a `SemanticViewDefinition` and then pass through the same validation function. This function checks all invariants (graph validation, PK/FK resolution, cardinality inference, UNIQUE constraint checking). Currently this validation is scattered across `body_parser.rs` and `ddl/define.rs`.
2. **Feature parity tests.** For every SQL DDL test case, create a corresponding YAML test case that produces the same `SemanticViewDefinition`. Assert `serde_json::to_string(from_sql) == serde_json::to_string(from_yaml)`.
3. **YAML schema derives from Rust structs.** The YAML format should map 1:1 to the serde-serialized form of `SemanticViewDefinition` (which is already the JSON format). Do not invent a new schema -- deserialize YAML directly into `SemanticViewDefinition` using serde. This makes the two formats structurally identical.
4. **Reject YAML-only features.** Do not add any capability to YAML that SQL DDL does not have, and vice versa. The formats are two syntaxes for the same model.

**Detection:** CI test that round-trips: SQL DDL -> SemanticViewDefinition -> JSON -> YAML -> SemanticViewDefinition and asserts equality. Coverage tool showing which model fields are tested in both parsing paths.

**Phase:** YAML parser implementation phase. The shared validation extraction should happen first, before the YAML parser is built.

---

## Moderate Pitfalls

### Pitfall 6: Dollar-Quoted String Parsing Edge Cases

**What goes wrong:** The `FROM YAML $$ ... $$` syntax requires parsing dollar-quoted strings in the DDL detection layer (`parse.rs`). Nested delimiters are the primary risk: if YAML content contains the sequence `$$`, the parser terminates early and produces a truncated definition. Tagged delimiters (`$yaml$...$yaml$`) help but introduce new edge cases: the tag must follow PostgreSQL identifier rules (no `$` in tag, no leading digits), and the parser must handle mismatched tags gracefully.

**Why it happens:** The existing DDL parser in `parse.rs` uses `match_keyword_prefix` for keyword detection and `body_parser.rs` for the body after `AS`. Dollar-quoting is a new parsing mode that does not exist anywhere in the current codebase. The body_parser's `split_at_depth0_commas` already handles single-quote escaping and paren depth, but dollar-quoting is a fundamentally different quoting mechanism.

**Consequences:** Truncated YAML definitions that deserialize into incomplete `SemanticViewDefinition` structs with missing required fields, causing confusing errors. Or: YAML content that happens to contain `$$` silently breaks.

**Prevention:**
1. Implement dollar-quote scanning as a dedicated function in `parse.rs`, not as an extension of `split_at_depth0_commas`.
2. Support tagged delimiters: `$yaml$...$yaml$`, `$sv$...$sv$` etc., with the plain `$$` as the default.
3. Scan for the exact closing delimiter (including tag) -- do not stop at the first `$$` if a tagged delimiter was used.
4. Validate the tag follows identifier rules (alphanumeric + underscore, no leading digit, no `$`).
5. Add proptest/fuzz cases for YAML content containing `$$`, `$yaml$`, single `$`, and other delimiter-like patterns.
6. Error message for unterminated dollar-quote should include the expected closing delimiter and the byte position where the opening was found.

**Detection:** Proptest generating random YAML-like strings containing `$` characters inside dollar-quoted blocks. Explicit test for `$$` inside YAML content with tagged delimiters.

**Phase:** DDL detection/rewriting phase (extends `parse.rs` for `FROM YAML` syntax).

---

### Pitfall 7: File I/O Security with `FROM YAML FILE`

**What goes wrong:** `FROM YAML FILE '/path/to/definition.yaml'` reads a file from the filesystem. DuckDB has security controls (`enable_external_access`, `allowed_directories`, `allowed_paths`) that restrict file access. If the extension reads files directly via `std::fs::read_to_string` (bypassing DuckDB's filesystem layer), it circumvents these security controls. Conversely, if it uses DuckDB's `read_text` function, it needs a connection to execute SQL, which introduces the familiar execution-lock deadlock problem (the extension holds the ClientContext lock during DDL processing).

**Why it happens:** The extension already does file I/O in `catalog.rs` (the v0.1.0 companion file migration), but that is a one-time migration of the extension's own data. Reading user-specified file paths is a different security surface. The `read_text` approach is correct from a security standpoint but architecturally problematic given the existing lock patterns.

**Consequences:** If bypassing DuckDB's filesystem: security controls are circumvented, path traversal attacks possible (e.g., `FROM YAML FILE '/etc/passwd'`), and the community extension registry may reject the extension. If using `read_text`: potential deadlocks from the execution lock pattern documented in TECH-DEBT.md item 9.

**Prevention:**
1. Use DuckDB's `read_text` function via the `catalog_conn` connection (which is separate from the main connection and used for PK resolution already). This connection is created at init time and does not hold the ClientContext lock.
2. Path validation: reject absolute paths outside of DuckDB's configured allowed directories. Query `PRAGMA enable_external_access` to check the setting before attempting file reads.
3. Size limit: read the file, check length before parsing (max 1MB for a semantic view YAML).
4. Relative path resolution: relative paths should resolve relative to the DuckDB database file location (matching DuckDB's convention for COPY and IMPORT), not relative to the process working directory.
5. Test: verify that `FROM YAML FILE` respects `enable_external_access=false` by setting the pragma and asserting the operation fails.

**Detection:** Integration test setting `enable_external_access=false` and verifying file read is rejected. Negative test with path traversal patterns (`../../../etc/passwd`).

**Phase:** YAML file reading phase, after inline YAML is working.

---

### Pitfall 8: Materialization Staleness is Silent

**What goes wrong:** A MATERIALIZATIONS clause declares that a table `monthly_revenue_agg` covers certain dimensions and metrics. The user populates this table once. Later, the raw data changes (new orders, corrections), but the materialization table is not refreshed. Queries routed to the materialization return stale data. There is no mechanism to detect or warn about this.

**Why it happens:** The v0.7.0 design explicitly scopes materialization as "routing to pre-existing tables" with no refresh mechanism. This is architecturally correct (materialization management is a future milestone), but users may not understand that the extension provides no freshness guarantees.

**Consequences:** Users get silently stale results when data changes after materialization was populated. Debugging this requires understanding that the query was routed to a materialization.

**Prevention:**
1. When a query is routed to a materialization, include a comment in the expanded SQL: `/* routed to materialization: monthly_revenue_agg */`. This makes materialization routing visible in `explain_semantic_view` output.
2. Document clearly that materializations are user-managed tables with no automatic refresh. The MATERIALIZATIONS clause is a declaration of coverage, not a refresh directive.
3. Consider adding an optional `STALE_AFTER` annotation to the MATERIALIZATIONS clause for future use, but do not implement enforcement in v0.7.0.
4. The `explain_semantic_view` function should indicate when a materialization was selected and which one, so users can debug unexpected results.

**Detection:** Unit test that `explain_semantic_view` output includes the materialization table name when routing occurs. Documentation review.

**Phase:** Materialization routing engine phase, as part of the routing decision output.

---

### Pitfall 9: Type Mismatches Between Materialization and Raw Table Expansion

**What goes wrong:** The materialization table was created with `CREATE TABLE ... AS SELECT ...` at some point, capturing column types. Later, the raw table schema changes (e.g., an INTEGER column becomes BIGINT due to DuckDB's auto-upgrade, or a new column is added). The materialization table retains the old types. When the routing engine substitutes the materialization table, the output column types differ from what would be produced by raw expansion. This breaks the typed output pipeline (`build_execution_sql` and `duckdb_vector_reference_vector`).

**Why it happens:** The extension infers column types at define time via LIMIT 0 query (`column_types_inferred` in `SemanticViewDefinition`). The materialization table has its own column types. These can diverge. The existing `build_execution_sql` cast wrapper handles some type mismatches, but it was designed for minor DuckDB optimizer differences (HUGEINT vs BIGINT), not for arbitrary materialization-vs-raw divergence.

**Consequences:** Type mismatch errors at query time. Or worse: silent truncation if a wider type (BIGINT) is cast to a narrower type (INTEGER) from the materialization.

**Prevention:**
1. At define time (when MATERIALIZATIONS clause is parsed), validate that the materialization table exists and its columns match the expected types. This is similar to the existing LIMIT 0 type inference.
2. At query time, when routing to a materialization, check that the output columns from the materialization query have types compatible with the expected output. Reuse the existing `build_execution_sql` cast wrapper.
3. Store the expected materialization column types in the `SemanticViewDefinition` so that type validation does not require re-querying the materialization table at query time.
4. If the materialization table has been dropped or its schema has changed, fall back to raw expansion with a warning (not an error).

**Detection:** Test with a materialization table whose column types intentionally differ from raw table expansion. Verify graceful degradation (fallback or clear error).

**Phase:** Materialization routing engine phase, during the query rewriting step.

---

### Pitfall 10: Materialization Matching Ignores Derived Metrics and Fact Inlining

**What goes wrong:** A metric like `gross_margin` is defined as a derived metric (`revenue - cost`, where `revenue` and `cost` are other metrics). The routing engine checks whether a materialization contains `gross_margin`, but the materialization table was populated using the raw expansion which inlines derived metrics into their component expressions. The materialization stores a column named `gross_margin` that contains the correct pre-computed values, but the routing engine does not understand that `gross_margin` in the materialization is the same as the derived metric `gross_margin` in the definition.

**Why it happens:** The existing expansion engine resolves derived metrics through `inline_derived_metrics` in `facts.rs`, which performs multi-pass expression substitution. A materialization table is a flat table with column names -- it does not retain the derivation chain. The routing engine must map metric names to materialization column names, not metric expressions.

**Consequences:** Derived metrics are never matched to materializations, causing them to always fall back to raw expansion even when the materialization has the pre-computed values.

**Prevention:**
1. Materialization matching should work by metric NAME, not by expression. The MATERIALIZATIONS clause declares which metric names are covered. If `gross_margin` is listed, the routing engine trusts that the materialization table has a column with that pre-computed value.
2. The MATERIALIZATIONS clause should list dimension and metric names, not expressions. Validation at define time checks that listed names exist in the semantic view definition.
3. For derived metrics, the user is responsible for ensuring the materialization table was populated with the correct derived values. The extension does not verify derivation chains against materialization contents.

**Detection:** Test with a derived metric that is covered by a materialization. Verify the routing engine selects the materialization.

**Phase:** Materialization routing engine phase, specifically the matching algorithm design.

---

## Minor Pitfalls

### Pitfall 11: YAML Multiline Strings and SQL Expression Fidelity

**What goes wrong:** YAML literal block scalars (`|`) preserve newlines, folded block scalars (`>`) fold newlines to spaces. SQL expressions in dimension/metric `expr` fields may contain significant whitespace (e.g., CASE WHEN statements). Using the wrong YAML scalar style silently alters the expression.

**Prevention:**
1. Document that metric/dimension expressions should use literal block scalars (`|`) or quoted strings, not folded block scalars (`>`).
2. Trim trailing whitespace/newlines from parsed expressions before storing them.
3. Test round-trip: YAML with multiline CASE WHEN expression -> parse -> serialize to JSON -> compare with expected.

**Phase:** YAML parser implementation phase.

---

### Pitfall 12: GET_DDL YAML Round-Trip Fidelity

**What goes wrong:** `GET_DDL('SEMANTIC_VIEW', 'name')` currently renders SQL DDL from stored `SemanticViewDefinition`. Adding YAML format output (`GET_DDL('SEMANTIC_VIEW', 'name', 'YAML')`) requires a YAML renderer. If the YAML renderer does not handle all model fields (comment, synonyms, access, non_additive_by, window_spec), the round-trip is lossy.

**Prevention:**
1. The YAML renderer should use serde's YAML serialization of `SemanticViewDefinition` directly, not a hand-rolled template. This ensures all fields are included automatically when the model grows.
2. Test: `GET_DDL -> parse back -> compare SemanticViewDefinition` for every model variant (with/without comments, synonyms, access modifiers, semi-additive, window specs).

**Phase:** GET_DDL YAML export phase, after the YAML parser is complete.

---

### Pitfall 13: Materialization Routing Interacts with USING RELATIONSHIPS

**What goes wrong:** A metric with `USING RELATIONSHIPS (rel_name)` specifies a join path. The materialization table was pre-computed using that specific join path. The routing engine matches the metric name but does not verify the USING context. If the query changes the USING relationship (or omits it), the materialization's pre-computed values may be wrong (computed over a different join path).

**Prevention:**
1. Materialization matching should be conservative: if any requested metric has `using_relationships`, skip materialization routing for that query entirely. The interaction between USING paths and materialization coverage is too complex for v0.7.0.
2. Future versions can support USING-aware materializations with explicit declaration of which relationship path the materialization covers.

**Phase:** Materialization routing engine phase, as an exclusion rule.

---

### Pitfall 14: Backward-Compatible Catalog Persistence for MATERIALIZATIONS

**What goes wrong:** Adding a `materializations` field to `SemanticViewDefinition` changes the JSON schema stored in `semantic_layer._definitions`. Pre-v0.7.0 stored views do not have this field. If the new code requires `materializations` to be present, existing views fail to load.

**Prevention:**
1. Follow the existing pattern: `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub materializations: Vec<Materialization>`. This is exactly how `facts`, `joins`, `tables`, `non_additive_by`, and every other optional field is handled.
2. Backward-compat test: deserialize pre-v0.7.0 JSON (without `materializations` field) and verify it loads with empty materializations vec.
3. This is a well-established pattern in this codebase -- 14 prior model extensions have used it successfully.

**Phase:** Model extension phase (first phase).

---

### Pitfall 15: Materialization Query Expansion Differs from Raw Expansion Column Order

**What goes wrong:** Raw expansion generates columns in the order: dimensions first, then metrics (this is the GROUP BY order). A materialization table may have columns in a different order, or may have additional columns not requested. If the routing engine does `SELECT * FROM materialization`, the column order differs from raw expansion, breaking callers that depend on positional column access.

**Prevention:**
1. When routing to a materialization, generate explicit `SELECT dim1, dim2, SUM(metric1), SUM(metric2) FROM materialization GROUP BY dim1, dim2` with columns in the same order as raw expansion would produce.
2. Never use `SELECT *` from the materialization table.
3. Quote all column names (using the existing `quote_ident` function) to handle reserved words.

**Phase:** Materialization routing engine phase, during SQL generation.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| YAML crate selection | Deprecated/unsound dependencies (Pitfall 3) | Use serde_yaml_ng or serde-saphyr; pin version; add fuzz target |
| Dollar-quote parsing | Nested delimiters truncate content (Pitfall 6) | Tagged delimiters ($yaml$...$yaml$); dedicated scanner function |
| YAML inline parsing | Anchor bombs cause OOM (Pitfall 4) | Input size cap (1MB); post-parse cardinality validation |
| YAML file reading | Security bypass of DuckDB file access controls (Pitfall 7) | Use DuckDB's read_text via catalog_conn; respect enable_external_access |
| YAML parser | Multiline string fidelity (Pitfall 11) | Document scalar styles; trim trailing whitespace; round-trip tests |
| Shared validation extraction | Model drift between SQL and YAML paths (Pitfall 5) | Extract validation from body_parser.rs; feature parity tests |
| GET_DDL YAML output | Lossy round-trip (Pitfall 12) | Use serde YAML serialization, not hand-rolled template |
| Materialization model | Backward-compat catalog breakage (Pitfall 14) | Follow existing serde(default, skip_serializing_if) pattern |
| Materialization matching | Non-additive re-aggregation (Pitfall 1) | Classify additivity at define time; reject non-additive subset matches |
| Materialization matching | Semi-additive/window metric bypass (Pitfall 2) | Exclude semi-additive and window metrics from routing |
| Materialization matching | Derived metric name resolution (Pitfall 10) | Match by metric name, not expression |
| Materialization matching | USING relationship interaction (Pitfall 13) | Skip routing for queries with USING metrics |
| Materialization query gen | Column order/type mismatches (Pitfalls 9, 15) | Explicit SELECT with quoted column names; type validation at define time |
| Materialization output | Staleness invisible to users (Pitfall 8) | SQL comment annotation; explain_semantic_view visibility |

## Sources

- [serde_yaml deprecation discussion](https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868)
- [serde_yaml 0.9.34+deprecated on docs.rs](https://docs.rs/crate/serde_yaml/latest)
- [serde_yml soundness issues](https://github.com/sebastienrousseau/serde_yml)
- [serde_yaml_ng maintained fork](https://github.com/acatton/serde-yaml-ng)
- [serde-saphyr pure-Rust YAML](https://github.com/bourumir-wyngs/serde-saphyr)
- [DuckDB securing extensions](https://duckdb.org/docs/stable/operations_manual/securing_duckdb/overview)
- [DuckDB file access with read_text](https://duckdb.org/docs/current/guides/file_formats/read_file)
- [Cube.dev pre-aggregation matching](https://cube.dev/docs/product/caching/matching-pre-aggregations)
- [Cube.dev non-additivity recipes](https://cube.dev/docs/product/caching/recipes/non-additivity)
- [Aggregation Consistency Errors in Semantic Layers](https://arxiv.org/pdf/2307.00417)
- [Billion laughs attack (Wikipedia)](https://en.wikipedia.org/wiki/Billion_laughs_attack)
- [PostgreSQL dollar-quoted string constants](https://www.geeksforgeeks.org/postgresql/postgresql-dollar-quoted-string-constants/)
- [Holistics non-additive measures documentation](https://docs.holistics.io/docs/modeling/non-additive-measures)
- Project design doc: `_notes/semantic-views-duckdb-design-doc.md` (pre-aggregation selection algorithm)
