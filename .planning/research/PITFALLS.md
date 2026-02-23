# PITFALLS — DuckDB Semantic Views Extension

**Research type:** Project Research — Pitfalls dimension
**Milestone context:** Greenfield — what do DuckDB extension projects and semantic layer implementations commonly get wrong?
**Date:** 2026-02-23

---

## Purpose

This document catalogs concrete mistakes, gotchas, and failure modes specific to:
1. Building DuckDB extensions in Rust
2. Implementing a semantic layer / query expansion engine
3. Designing DDL for persistent schema objects in DuckDB
4. Handling DuckDB's extension versioning and ABI model

Each pitfall includes: what goes wrong, warning signs, prevention strategy, and which project phase should address it.

---

## Part 1 — DuckDB Extension Development in Rust

### P1.1 — ABI breakage across DuckDB minor versions

**What goes wrong:**
DuckDB does not guarantee ABI stability between minor versions (e.g., 1.1.x → 1.2.x). The C extension API (`duckdb_extension.h`) changes when DuckDB adds or modifies internal struct layouts, function signatures, or the extension entry point contract. A `.duckdb_extension` binary compiled against DuckDB 1.1 will fail to load on DuckDB 1.2 with a cryptic error ("extension was compiled for a different version" or silent segfault). This is especially sharp in Rust because `duckdb-rs` wraps the C bindings, and the Rust crate version must exactly match the DuckDB runtime version.

**Warning signs:**
- The `duckdb-rs` crate version in `Cargo.toml` diverges from the DuckDB binary installed locally.
- CI passes on one DuckDB version but fails on another.
- Users report "extension not loading" without a clear error message — this is often an ABI mismatch.
- The extension entry point symbol (`_duckdb_extension_api_version`) is missing or returns a version the runtime rejects.

**Prevention strategy:**
- Pin `duckdb-rs` to the exact DuckDB version you target and document this prominently in README.
- Use the DuckDB community extension CI pipeline, which compiles against a fixed DuckDB version per release slot. Do not assume local dev and CI are using the same DuckDB build.
- Build the extension with `-C link-arg=-Wl,--no-undefined` (or the macOS/Windows equivalent) to catch missing symbols at link time rather than at load time.
- Version your extension releases against DuckDB releases explicitly: `v0.1.0-duckdb1.1`, not just `v0.1.0`. Consider automating this with a matrix CI job.
- Read the DuckDB changelog before each DuckDB upgrade; look specifically for "Extension API changes" sections.

**Phase:** Address in Phase 1 (project scaffold / extension skeleton). Lock versions before writing any business logic.

---

### P1.2 — `duckdb-rs` is not the official DuckDB Rust API

**What goes wrong:**
`duckdb-rs` is a community crate that wraps DuckDB's C API. It is not maintained by DuckDB GmbH and has historically lagged behind the official C extension SDK. The official DuckDB extension template (used for first-party and community extensions) is C++-based (`duckdb/extension-template`). Building in Rust on top of this template requires either: (a) using `duckdb-rs` and accepting its lag and coverage gaps, or (b) writing raw `unsafe` Rust FFI against the C headers directly. Neither option gives you the ergonomics of a first-class SDK.

**Warning signs:**
- `duckdb-rs` does not expose the extension API hooks you need (e.g., `AddParserExtension`, `AddStatementRewriter`, catalog hooks).
- You find yourself writing `unsafe extern "C"` blocks to reach C functions that `duckdb-rs` doesn't wrap.
- The `duckdb-rs` version on crates.io was last updated months before the DuckDB version you need.

**Prevention strategy:**
- Evaluate at project start whether `duckdb-rs` exposes the specific hooks required: custom parser extension (for `CREATE SEMANTIC VIEW` DDL), table function registration (for the query syntax), and catalog object serialization. If any hook is missing, plan for raw FFI wrappers early.
- Consider the Rust-in-C++ model: write the extension entry point in a thin C++ shim using the official template, and call into a Rust static library for all business logic. This gives you access to the full C++ extension API while keeping the semantic expansion logic in safe Rust.
- Budget 1–2 weeks to evaluate FFI coverage before committing to a pure-Rust approach.

**Phase:** Address in Phase 1 (architecture decision). The custom DDL syntax (`CREATE SEMANTIC VIEW`) and the table function / parser hook requirements make this a high-priority early decision.

---

### P1.3 — Custom parser extension hooks are underdocumented and fragile

**What goes wrong:**
Adding a new SQL statement type (`CREATE SEMANTIC VIEW`) requires hooking into DuckDB's parser at a level that the extension API exposes only partially. DuckDB supports `AddParserExtension` for injecting a custom parser callback, but the callback receives a raw token stream and must return a parsed statement that DuckDB's planner can accept. The internal statement types accepted by the planner are not part of the stable extension API. Many extension authors end up parsing their DDL to a generic `ExtensionStatement` and then resolving it in a custom catalog function — a pattern that is not well-documented and breaks subtly when DuckDB changes how it dispatches statements.

**Warning signs:**
- You cannot find documented examples of `AddParserExtension` being used in community extensions.
- The DuckDB source for `ParserExtension` callback types changes between releases without announcement.
- Your `CREATE SEMANTIC VIEW` statement parses correctly in one DuckDB version but the planner rejects the resulting AST node in another.

**Prevention strategy:**
- Study the handful of community extensions that add custom DDL (e.g., `spatial` extension's custom type DDL) as implementation references. Read the source, not just the docs.
- Design the initial DDL as a `CREATE ... AS SELECT` or a function call that DuckDB already knows how to parse. For example, `SELECT create_semantic_view('name', ...)` is crude but bypasses the parser hook problem entirely for an MVP. Graduate to proper DDL once the expansion logic is proven.
- If using custom DDL, write integration tests that execute `CREATE SEMANTIC VIEW` in a fresh DuckDB process — not just a test harness that bypasses the parser. Catch parser regressions immediately.
- Keep a note of the DuckDB commit where you validated your parser hook works; re-validate on every DuckDB bump.

**Phase:** Phase 1 (syntax design decision). Phase 2 (implementation). Decide on the MVP syntax before investing in the catalog schema.

---

### P1.4 — Build system: CMake / Makefile wrapper complexity for Rust + C++ hybrid

**What goes wrong:**
The official DuckDB extension template uses CMake + `vcpkg` + a Makefile wrapper. When introducing Rust, you add Cargo into this build graph. The two build systems do not compose naturally: CMake does not know about Cargo's incremental compilation, Cargo does not know about CMake's build graph, and the linking step (producing a `.duckdb_extension` shared library with the correct symbol exports) requires careful coordination. Common failures: Rust symbols get stripped, the extension entry point is not exported with C linkage, or `cargo build` succeeds but the resulting `.so` / `.dylib` / `.dll` fails to load.

**Warning signs:**
- The `.duckdb_extension` binary loads in local `LOAD 'path/to/ext'` but fails when installed via `INSTALL`.
- Symbol stripping removes the `_duckdb_extension_api_version` or `_duckdb_init` symbols.
- The build works on macOS (where dylib linking is lenient) but fails on Linux (where symbol visibility defaults are stricter).
- CI produces different `.duckdb_extension` sizes than local builds — a sign of different link flags.

**Prevention strategy:**
- Use `#[no_mangle]` and `extern "C"` on all symbols that must be visible to DuckDB's extension loader. Add `visibility("default")` attributes on Linux.
- Add a `[lib] crate-type = ["cdylib"]` in `Cargo.toml`. Verify the symbol is present after build with `nm -D target/.../libext.so | grep duckdb_init`.
- Write a smoke test that does `LOAD 'path'` in a fresh DuckDB process as part of CI, not just unit tests. This catches linker and ABI issues that unit tests never see.
- Consider starting from an existing Rust DuckDB extension (e.g., `duckdb-rs` examples or community Rust extensions) rather than adapting the C++ template from scratch.

**Phase:** Phase 1 (build scaffold). Fix this before any feature work.

---

### P1.5 — Community extension registry: signing and CI requirements

**What goes wrong:**
The DuckDB community extension registry (`community-extensions.duckdb.org`) requires that extensions are built by a specific GitHub Actions pipeline controlled by the DuckDB team, signed with DuckDB's key, and submitted via a PR to the `duckdb/community-extensions` repository. You cannot self-host a signed community extension. The registry builds extensions for all supported platforms (Linux x86_64, Linux ARM64, macOS ARM64, macOS x86_64, Windows x86_64) against a specific DuckDB version. If your extension requires native dependencies (e.g., a C library for SQL parsing), those dependencies must be available in the build environment or vendored.

**Warning signs:**
- Your extension compiles locally but uses a system library that is not in the community extension CI image.
- Your extension uses Rust features or nightly-only APIs not available in the Rust toolchain version used by the community extension CI.
- You try to publish for a DuckDB version that the registry does not yet support.

**Prevention strategy:**
- Minimize native dependencies. The semantic views extension should be pure Rust (no C library dependencies beyond the DuckDB C API itself, which is provided by the build environment).
- Check the `duckdb/community-extensions` repository's CI configuration before designing the build to understand the exact toolchain versions used.
- Test the extension on all target platforms early (use cross-compilation or GitHub Actions matrix). Linking behavior differs significantly between macOS, Linux, and Windows.
- Register intent to publish early by opening a draft PR to `duckdb/community-extensions`. The DuckDB team can flag blockers before you have a finished extension.

**Phase:** Phase 1 (project setup). Phase 3 (pre-release). The signing requirement means you cannot wait until "done" to figure out distribution.

---

## Part 2 — Semantic Layer / Query Expansion Engine

### P2.1 — Silent correctness failures in GROUP BY inference

**What goes wrong:**
The expansion engine infers a `GROUP BY` clause from the list of requested dimensions. If the dimension resolution is wrong — for example, if a dimension expression contains a column that is not uniquely named across joined tables, or if the expression is an alias that DuckDB resolves differently in `SELECT` vs `GROUP BY` — the emitted SQL will produce results that are wrong in a non-obvious way. The query succeeds, returns rows, but the numbers are incorrect. This is the hardest class of bug to catch: no error, no crash, wrong answer.

**Concrete failure modes:**
- Two joined tables both have a column named `id`. The dimension expression `id` is ambiguous; DuckDB resolves it to one table, but the semantic view definition intended the other.
- A dimension is defined as a SQL expression (`YEAR(order_date) AS year`). The `GROUP BY` emits `YEAR(order_date)` but the `SELECT` emits the alias `year`. On DuckDB this works, but the expression-vs-alias behavior differs from PostgreSQL and can cause confusion if users try to run the same SQL elsewhere.
- A measure uses a subquery or window function that is illegal inside an aggregate. The emitted SQL fails, but the error message points at the generated SQL rather than the user's semantic view definition.

**Warning signs:**
- Test results have row counts that don't match hand-written SQL for the same query.
- The same semantic query with and without a specific dimension returns the same numbers (a dimension that has no effect is a sign of an incorrect GROUP BY).
- Metrics return wildly high numbers — a classic sign of a fan-out join where GROUP BY didn't de-duplicate correctly.

**Prevention strategy:**
- Always fully qualify column references in emitted SQL: `table_alias.column_name`, never bare `column_name`. This eliminates ambiguity in multi-table joins.
- Generate a reference SQL for every test case by hand and assert exact equality (not just row-count equality) in expansion tests.
- Test every supported metric type (SUM, COUNT, COUNT DISTINCT, AVG, MIN, MAX) against a small known dataset where the correct answer is computable by hand.
- Test the fan-out case explicitly: a semantic view that joins two tables where the join multiplies rows. Verify the metric is not double-counted.
- Build a test harness that runs expansion against TPC-H with known expected values before shipping.

**Phase:** Phase 2 (expansion engine). These correctness tests must gate the MVP, not be deferred.

---

### P2.2 — Non-additive metric handling: COUNT DISTINCT, percentiles, HLL

**What goes wrong:**
`COUNT(DISTINCT x)` is not additive — you cannot sum `COUNT(DISTINCT x)` across pre-aggregated partitions and get the correct total. This is explicitly called out in Cube's pre-aggregation matching algorithm. The semantic layer must know which metrics are additive and refuse to serve non-additive metrics from pre-aggregated tables at coarser granularity. If this check is omitted, pre-aggregation selection silently returns wrong answers: the `COUNT DISTINCT` of customers per region at monthly granularity served from a daily rollup will be wrong.

For v0.1 (expansion only, no pre-aggregation), this is a design-time concern: the metric type system must model additivity even if pre-aggregation isn't implemented yet, because getting it wrong in v0.1 makes v0.2 harder.

**Warning signs:**
- The semantic view definition accepts `COUNT(DISTINCT customer_id)` as a measure without recording its additivity class.
- Tests only cover `SUM` and `COUNT(*)` metrics.
- The design doc's metric representation is a plain SQL expression string without metadata about additivity.

**Prevention strategy:**
- Classify metrics by additivity at definition time: `additive` (SUM, COUNT, MIN, MAX), `semi-additive` (distinct count at a given grain), `non-additive` (percentile, ratio, any custom expression). Store this classification in the catalog serialization.
- For v0.1, emit the correct SQL expression regardless of additivity class (the expansion is still correct at raw table granularity). Mark non-additive metrics as not pre-aggregation eligible in the metadata so v0.2 doesn't accidentally misuse them.
- Document the additivity class in the DDL (e.g., `MEASURE dau AS COUNT(DISTINCT user_id) TYPE non_additive`) so users understand the constraint.

**Phase:** Phase 1 (DDL design) and Phase 2 (expansion engine). Must be in the data model from the start.

---

### P2.3 — Relationship / join inference producing cartesian products

**What goes wrong:**
When the expansion engine infers JOINs from entity relationships, a missing or incorrect join condition produces a cartesian product. DuckDB will execute this — it won't error — and return wildly incorrect results. This is especially dangerous with multi-hop joins: `orders → customers → regions`. If the join from `customers` to `regions` uses the wrong key, every order is joined to every region.

A subtler version: the semantic view definition allows multiple paths between two tables (different join keys for different contexts). If the expansion engine picks the wrong path based on which dimensions/metrics were requested, the join is semantically wrong even though the SQL is valid.

**Warning signs:**
- Row counts in expansion output are orders of magnitude larger than expected.
- Metric values are exact multiples of the correct value (e.g., 3x, 5x) — a sign of join fan-out.
- Adding or removing a dimension changes metric values (it shouldn't — the metric total should be stable across different dimension groupings).

**Prevention strategy:**
- Validate join conditions at semantic view definition time, not just at expansion time. Check that the join key columns actually exist in the referenced tables.
- In integration tests, assert that metric totals are stable across different dimension combinations: `SUM(revenue) GROUP BY region` should equal `SUM(revenue)` (the ungrouped total). Any divergence is a join correctness failure.
- Limit multi-hop join inference in v0.1 to explicit, linear chains. Do not attempt to infer join paths algorithmically from the relationship graph — require the definition to specify the join chain explicitly.

**Phase:** Phase 2 (expansion engine). Include join-correctness tests as first-class acceptance criteria.

---

### P2.4 — WHERE clause placement in expanded SQL

**What goes wrong:**
The user's `WHERE` clause in a semantic view query (e.g., `WHERE order_date >= '2025-01-01'`) must be placed correctly in the expanded SQL. Three failure modes:
1. The filter is applied after aggregation (HAVING instead of WHERE) — filters rows out of the result set instead of reducing the rows before aggregation. For a time filter this produces wrong metrics.
2. The filter is pushed inside a subquery that is later joined — the join produces a partial result.
3. Row-level filters defined in the semantic view itself (`CREATE SEMANTIC VIEW ... WHERE status = 'active'`) interact with user-supplied WHERE clauses in an unexpected way (AND vs OR composition).

**Warning signs:**
- `WHERE order_date BETWEEN ...` on a metric produces a different result than running the equivalent hand-written SQL with the same filter.
- Row-level filters in the semantic view definition appear to be ignored in some query combinations.
- Adding a WHERE clause changes the metric total in a way that doesn't match a hand-written GROUP BY.

**Prevention strategy:**
- Distinguish between pre-aggregation filters (applied before GROUP BY in the inner subquery) and post-aggregation filters (applied on the outer query). Route user-supplied dimension filters to pre-aggregation position; route metric-level filters (e.g., `WHERE total_revenue > 1000`) to post-aggregation position.
- Semantic view row-level filters (`CREATE SEMANTIC VIEW ... WHERE status = 'active'`) are always AND-composed with user WHERE clauses. Make this explicit in the design and test the composition.
- Write a test case for each WHERE placement scenario with known correct outputs.

**Phase:** Phase 2 (expansion engine). This is a correctness requirement, not a nice-to-have.

---

### P2.5 — SQL identifier quoting and injection

**What goes wrong:**
The expansion engine builds SQL strings from user-supplied identifiers: semantic view names, dimension names, metric names, table names, column expressions. If identifiers are not properly quoted, two failure modes occur:
1. A dimension named `year` (a reserved word in SQL) causes a parse error in the emitted SQL.
2. A malicious or careless table name like `my_table; DROP TABLE orders; --` produces SQL injection in the emitted query. This is a DuckDB extension — it runs with full database privileges.

**Warning signs:**
- Identifiers that are SQL reserved words cause mysterious parse errors in expansion output.
- The expansion engine uses string concatenation (`format!("SELECT {} FROM {}", dim, table)`) without quoting.

**Prevention strategy:**
- Quote all identifiers in emitted SQL: `"dimension_name"`, `"table_name"`. In DuckDB, double-quotes are the standard SQL identifier quoting mechanism.
- Never accept user-supplied SQL expressions directly into emitted SQL without a validation/sanitization step. For metric and dimension expressions defined in the semantic view DDL, treat them as trusted (they were already accepted into the catalog), but validate at `CREATE SEMANTIC VIEW` time.
- Use a SQL builder library or template approach rather than raw string concatenation for emitted SQL. This makes quoting systematic.

**Phase:** Phase 1 (design) and Phase 2 (implementation). Quoting must be baked in from the first SQL generation code.

---

### P2.6 — Time dimension granularity coarsening edge cases

**What goes wrong:**
Time granularity coarsening (`day → month → quarter → year`) is more nuanced than it appears:
- `date_trunc('month', ts)` returns a `TIMESTAMP`, not a `DATE`. Downstream queries that expect a `DATE` type will silently receive a `TIMESTAMP`.
- Week granularity is locale-dependent: ISO week starts Monday, US week starts Sunday. `date_trunc('week', ts)` uses ISO. If users expect US weeks, results are wrong by 0–6 days.
- Fiscal year / quarter granularity (common in analytics) does not exist in DuckDB's `date_trunc`. Extensions that promise `FISCAL_QUARTER` granularity must implement it manually, which is complex.
- When a user requests `year` granularity for a time series that crosses a DST boundary, `date_trunc` in DuckDB operates in UTC. If the base data has timezone-aware timestamps, results can be off by one day near DST transitions.

**Warning signs:**
- Time-series test data at the day level does not aggregate to the expected month-level totals.
- Week-level queries return unfamiliar week boundaries.
- Type errors downstream when the emitted SQL uses `date_trunc` and the result is joined to a `DATE` column.

**Prevention strategy:**
- For v0.1, support only the granularities that map directly and unambiguously to DuckDB's `date_trunc`: `second`, `minute`, `hour`, `day`, `week` (ISO), `month`, `quarter`, `year`.
- Document the week convention (ISO) explicitly. Do not support fiscal granularities in v0.1.
- Cast the `date_trunc` output to `DATE` when the time dimension column is a `DATE` type, not a `TIMESTAMP`.
- Include time-series test cases that span month boundaries, year boundaries, and (if timestamps are supported) DST transitions.

**Phase:** Phase 2 (expansion engine). Include in DDL design (what granularities are declared valid).

---

### P2.7 — Schema evolution: semantic view definitions going stale

**What goes wrong:**
A semantic view is defined against a set of base tables. When those tables evolve (columns renamed, dropped, type changed), the semantic view definition becomes invalid. If the extension discovers this only at query time (when expansion produces SQL against a non-existent column), users get a cryptic DuckDB error message that does not reference the semantic view definition. Worse, if a column type changes (e.g., from INTEGER to BIGINT), the expansion may silently succeed with wrong implicit casts.

**Warning signs:**
- A semantic view that worked yesterday throws a "column not found" error today, with no mention of the semantic view in the error.
- Numeric metrics that used INTEGER columns now return BIGINT values after a schema change, causing downstream type comparison failures.
- Semantic view definitions reference tables that have been renamed.

**Prevention strategy:**
- At `CREATE SEMANTIC VIEW` time, validate that all referenced tables and columns exist and record the column types. Store this in the catalog entry.
- Add a `VALIDATE SEMANTIC VIEW name` command (or equivalent) that reruns this validation against the current schema without executing a query.
- On expansion failure due to a missing column, emit an error that identifies the semantic view, the dimension or metric that failed, and the expected column — not just a raw SQL error.
- For v0.1, explicit validation at creation time is sufficient. Automatic schema change detection (catalog triggers) is a future enhancement.

**Phase:** Phase 2 (expansion engine) for validation. Phase 1 (catalog design) for storing column type metadata.

---

## Part 3 — DDL and Catalog Persistence

### P3.1 — DuckDB catalog persistence model for custom schema objects

**What goes wrong:**
DuckDB's catalog (the internal registry of tables, views, functions, and types) is session-local for in-memory databases and file-based for persistent databases (`.duckdb` files). Custom extension objects — including `SEMANTIC VIEW` definitions — are not automatically persisted in the DuckDB catalog in the same way as tables and views. Extension authors must implement their own persistence mechanism.

The common failure: an extension registers a custom catalog entry type during `LOAD`. The user creates a semantic view. When DuckDB is restarted, the extension is auto-loaded (via `autoload`), but the semantic view definitions are gone because there is no mechanism to reload them from the catalog file. The user gets silent data loss without any error.

**Warning signs:**
- `SELECT * FROM semantic_views` returns results, but after `DuckDB.connect('file.duckdb')` (restart), the query fails.
- The extension registers objects into DuckDB's catalog at load time but does not hook the catalog serialization/deserialization path.
- Tests only run against fresh in-memory DuckDB instances, never against a restarted persistent database.

**Prevention strategy:**
- Do not rely on DuckDB's internal catalog for persistence of semantic view definitions. Store definitions separately:
  - **Option A (simple, recommended for v0.1):** Persist semantic view definitions in a dedicated DuckDB table (`_semantic_views_catalog`) inside the user's database file. This is a regular DuckDB table; it survives restarts automatically. On extension load, read from this table to reconstruct the in-memory representation.
  - **Option B (complex, not recommended for v0.1):** Hook DuckDB's catalog serialization extension points (if they exist) to serialize custom catalog entries. This requires deep knowledge of DuckDB internals and may not be exposed in the extension API.
- Write a test that: creates a semantic view, closes the DuckDB connection, reopens the `.duckdb` file, verifies the semantic view is still queryable. This must be a passing test before v0.1 ships.

**Phase:** Phase 1 (catalog design). The persistence model is a foundational architectural decision that affects the DDL design, the in-memory representation, and the extension initialization path.

---

### P3.2 — Serialization format for semantic view definitions

**What goes wrong:**
The semantic view definition (dimensions, measures, joins, relationships, row filters) must be serialized to the persistence store and deserialized on load. Failure modes:
- Using a format (e.g., bincode, custom binary) that is not inspectable by users. When the extension has a bug in deserialization, there is no way to recover the definition without the old extension version.
- Using a format that is not forward-compatible. Adding a new field (e.g., a `time_zone` field on a time dimension) causes deserialization of older definitions to fail.
- Storing the definition as a raw SQL string (the original `CREATE SEMANTIC VIEW` statement) but not re-parsing it on load. If the parser changes, old definitions silently behave differently.

**Warning signs:**
- The serialization format is not JSON or a similarly human-readable format.
- There are no migration tests that create a definition in version N-1 format and load it in version N.
- The definition is stored as an opaque blob in the catalog table.

**Prevention strategy:**
- Store the canonical definition as a JSON blob (or TOML/YAML) in the `_semantic_views_catalog` table. JSON is human-readable, forward-compatible with optional fields, and inspectable via DuckDB's JSON functions (`json_extract`, etc.).
- Use serde with `#[serde(default)]` on all optional fields so new fields added in future versions don't break deserialization of old definitions.
- Store the extension version alongside each definition. On load, if the stored version is older than the current extension, run a migration function.
- Write migration tests from v0.1 format to v0.2 format as part of the extension upgrade process.

**Phase:** Phase 1 (catalog design). JSON + serde with versioning is the recommended approach from the start.

---

### P3.3 — In-memory catalog and multi-connection consistency

**What goes wrong:**
DuckDB supports multiple concurrent connections to the same database file. An extension that maintains its in-memory catalog (reconstructed from the persistence table at load time) is not automatically aware of changes made by other connections. Connection A creates a semantic view; connection B's in-memory catalog does not see it. Connection B deletes a semantic view; connection A's in-memory cache is stale.

For a single-user analytical tool (DuckDB's primary use case), this may be acceptable, but it must be a conscious decision.

**Warning signs:**
- Two DuckDB connections to the same file are opened; a semantic view created in one is not visible in the other.
- A `DROP SEMANTIC VIEW` in one connection causes unexpected errors in another.
- The extension caches the catalog at load time and never refreshes it.

**Prevention strategy:**
- For v0.1, document the single-connection assumption explicitly. DuckDB's primary analytical use case is typically single-process.
- Design the in-memory catalog to be reconstructable from the persistence table at any point. Avoid caching that is hard to invalidate.
- Use DuckDB's write-ahead log (WAL) semantics: writes to the `_semantic_views_catalog` table are transactional and durable when the connection commits. This is automatic if you use a regular DuckDB table as the persistence store.

**Phase:** Phase 1 (design decision). Document the limitation, don't solve it prematurely.

---

### P3.4 — DROP SEMANTIC VIEW and transaction safety

**What goes wrong:**
`DROP SEMANTIC VIEW` must atomically remove the definition from the persistence table and the in-memory catalog. If the operation removes from one but fails on the other (e.g., a crash between the two steps), the catalog is inconsistent. The next time the extension loads, it either fails to find the in-memory entry (definition appears deleted) or fails to find the persistence entry (definition appears to exist only in memory — will be lost on next restart).

**Warning signs:**
- `DROP SEMANTIC VIEW` is implemented as two separate steps without a transaction.
- Tests do not cover `DROP` followed by restart followed by `CREATE` with the same name.

**Prevention strategy:**
- Make persistence-table writes (INSERT/DELETE on `_semantic_views_catalog`) the source of truth. The in-memory catalog is derived. Update the in-memory cache only after the persistence transaction commits.
- On extension load, always reconstruct the in-memory catalog from the persistence table. Never cache across restarts.
- Test the `DROP` → restart → re-create sequence explicitly.

**Phase:** Phase 2 (DDL implementation).

---

## Part 4 — DuckDB Extension Versioning

### P4.1 — Extension API version vs. DuckDB version: two separate things

**What goes wrong:**
DuckDB has two version numbers relevant to extensions:
1. **DuckDB version** (e.g., `1.2.0`): the database runtime version.
2. **Extension API version** (e.g., `v1`): a separate versioning scheme for the extension entry point ABI.

Extensions compiled against extension API `v0` cannot load in a DuckDB runtime that requires `v1`. The extension API version is embedded in the compiled binary via the `_duckdb_extension_api_version` symbol. If this symbol is absent or returns an incompatible value, DuckDB refuses to load the extension with a version mismatch error.

The Rust-specific failure: if you set the extension API version via a constant in your `Cargo.toml` or a build script, and that constant gets out of sync with the DuckDB C headers you compiled against, you get a runtime mismatch that is hard to diagnose.

**Warning signs:**
- "Extension version mismatch" error when loading, even though the DuckDB version numbers look compatible.
- The `_duckdb_extension_api_version` symbol in the compiled library returns `0` (the default from an uninitialized constant).
- Different behavior between `LOAD '/path/to/ext'` and `INSTALL ext; LOAD ext` — the latter goes through a version check that the former may not.

**Prevention strategy:**
- Derive the extension API version constant from the DuckDB C headers used at compile time, not from a hardcoded Rust constant. Use a `build.rs` script that extracts the version from `duckdb_extension.h` and writes it to a generated file.
- Test the `INSTALL; LOAD` path in CI, not just `LOAD '/path'`. The community extension registry will use `INSTALL; LOAD`.
- When upgrading DuckDB, upgrade both the DuckDB binary AND the header files (via `duckdb-rs` version bump or manual header update) atomically.

**Phase:** Phase 1 (build scaffold). Phase 3 (pre-release validation).

---

### P4.2 — Extension must be rebuilt for every supported DuckDB release

**What goes wrong:**
The DuckDB community extension registry builds separate extension binaries for each supported DuckDB release (e.g., `1.1.x`, `1.2.x`). There is no forward compatibility — an extension binary for DuckDB 1.1 does not load in DuckDB 1.2. This means every time DuckDB releases a new version, you must submit a new extension build. If your extension has not been rebuilt for the latest DuckDB release, users on that version cannot install it.

This creates an ongoing maintenance burden that is easy to underestimate at project start.

**Warning signs:**
- Your extension is only tested against one DuckDB version in CI.
- You don't have an automated process to rebuild and resubmit for each new DuckDB release.
- User bug reports mention "extension not found" for a DuckDB version you have not targeted.

**Prevention strategy:**
- Set up a CI matrix from day one: test against the last two DuckDB stable releases and the current RC/nightly (if available). Use GitHub Actions matrix builds.
- Follow the `duckdb/community-extensions` contribution process, which handles multi-version builds via the registry CI. Once your extension is in the registry, the registry CI handles rebuilds for new DuckDB releases (with a PR to update the DuckDB version pin).
- Monitor the DuckDB release schedule. New minor releases happen roughly quarterly.

**Phase:** Phase 3 (pre-release) through ongoing maintenance.

---

### P4.3 — Autoload and trust model

**What goes works:**
DuckDB 0.10+ introduced autoload: extensions registered in the official or community registry are automatically loaded when a SQL statement references them. For the community extension registry, extensions must be signed by the DuckDB team's key. Unsigned extensions require `SET allow_unsigned_extensions = true`, which is not enabled by default.

**What goes wrong:**
If you distribute the extension outside the registry (e.g., users download the `.duckdb_extension` binary directly from GitHub releases), they must enable unsigned extension loading. This is a friction point and a potential security concern for enterprise users. Worse, if your GitHub release binary is not built with the correct extension API version for the user's DuckDB installation, the error message ("unsigned extension" vs "version mismatch") is confusing.

**Warning signs:**
- Users report "Extension is not trusted" or "allow_unsigned_extensions" errors.
- You are distributing extension binaries outside the community extension registry.
- The README does not explain the trust model.

**Prevention strategy:**
- Target the community extension registry as the primary distribution channel. This is the only way to get signed, autoloadable extensions.
- For development and testing, document the `SET allow_unsigned_extensions = true; LOAD '/path'` workflow clearly.
- Do not rely on GitHub Releases as a primary distribution mechanism — use it only as a fallback with clear documentation of the trust requirement.

**Phase:** Phase 3 (pre-release). Design for registry distribution from the start.

---

## Summary: Pitfall Priority by Phase

| Phase | Critical Pitfalls | Why |
|-------|-------------------|-----|
| Phase 1 (Project Scaffold) | P1.1 ABI lock, P1.2 duckdb-rs coverage, P1.3 parser hook design, P1.4 build system, P1.5 registry requirements, P3.1 persistence model, P3.2 serialization format, P4.1 API version | Foundational architectural decisions; wrong choices here require expensive rework |
| Phase 2 (Expansion Engine) | P2.1 GROUP BY correctness, P2.2 non-additive metrics, P2.3 join fan-out, P2.4 WHERE placement, P2.5 identifier quoting, P2.6 time granularity, P2.7 schema evolution, P3.4 DROP safety | Correctness requirements that gate the MVP |
| Phase 3 (Pre-release) | P1.5 registry process, P4.2 multi-version builds, P4.3 trust model | Distribution and maintenance concerns |

---

## Research Notes

This document is based on:
- DuckDB extension API documentation and community extension registry requirements (as of DuckDB 1.x)
- Cube.dev pre-aggregation matching algorithm (sourced from `_notes/semantic-views-duckdb-design-doc.md`)
- Snowflake semantic view design patterns (sourced from `_notes/semantic-views-duckdb-design-doc.md`)
- Rust FFI/C interop patterns for shared library extension development
- General semantic layer implementation experience (dbt Semantic Layer, Metriql, LookML)
- Project requirements from `.planning/PROJECT.md`

Web access was not available during this research session. Specific DuckDB version numbers, registry API details, and `duckdb-rs` crate status should be verified against current documentation before implementation begins.
