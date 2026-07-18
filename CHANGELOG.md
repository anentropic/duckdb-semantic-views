# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- docs-include-start -->

## [Unreleased]

### Changed

- **View name case normalization**: view names now fold to lowercase in every DDL statement and in `semantic_view()` / `explain_semantic_view()` lookup arguments — whether written quoted or not — so `CREATE SEMANTIC VIEW Sales`, `DROP SEMANTIC VIEW SALES`, and `DROP SEMANTIC VIEW "sales"` all refer to the same view. This follows DuckDB's identifier semantics, where double-quoted identifiers are case-insensitive too; quoting only lets a name carry whitespace or special characters, it does not make it case-sensitive. Previously unquoted names were byte-exact case-sensitive. **Migration**: lookups fold the requested name to lowercase and match the stored catalog name exactly, so a view is only reachable if its stored name is lowercase. Unquoted `CREATE` always stored a lowercase name, so those views are unaffected; only a view created earlier via a *quoted* mixed-case identifier (e.g. `CREATE SEMANTIC VIEW "Sales"`) kept its original casing and is no longer reachable by any spelling — drop and recreate it, or rename its catalog row to lowercase.
- **Dimension / metric / fact query references are matched case-insensitively**, following the same DuckDB identifier semantics as view names: a reference matches regardless of case whether written unquoted (`region`, `REGION`) or double-quoted (`"Region"`, `"region"`) — DuckDB treats double-quoted identifiers as case-insensitive too. A quoted reference is also correctly *stripped* of its quotes before matching (previously a quoted stored name was only reachable by a reference carrying the identical quote characters). The same key governs the adjacent query surfaces so they stay consistent: CREATE-time name-uniqueness validation (names differing only in case or quoting — `region`, `REGION`, `"Region"` — collide as duplicates) and `alias.*` wildcard de-duplication. Table-qualified references are split quote-aware, so a quoted name containing a dot (`"a.b"`) is no longer mis-split. Names embedded in stored **expressions** — derived-metric operands and inlined fact references — now match case- and quote-insensitively too, via the shared reference tokenizer (see Fixed). The internal name-field matchers are all unified on the same key: a name written quoted in a `MATERIALIZATIONS` clause routes to its unquoted declaration, a quoted window inner-metric reference resolves consistently, and both a `NON ADDITIVE BY` dimension and a window metric's `PARTITION BY` / `EXCLUDING` / `ORDER BY` dimension references written with a dotted qualifier or quotes (`o."order date"`) resolve against their declared dimension (see Fixed).

### Added

- Machine-checked round-trip guarantee between `CREATE SEMANTIC VIEW` parsing and `GET_DDL` rendering: a property test asserts `parse(render(definition)) == definition` over generated definitions (including quoted, unicode, and keyword-bearing identifiers), and two new fuzz targets exercise the body parser directly and enforce render/parse fixpoint stability.
- Stored semantic view definitions now carry a storage-format `schema_version`. Freshly created (or replaced) views are stamped with the current version, and a one-time, non-destructive upgrade pass on extension load stamps existing definitions that are verifiably current-format — giving future format changes a clean migration point (following the v0.1.0 companion-file migration precedent).
- **Broader Snowflake-syntax acceptance for easier DDL porting**: the table alias is now optional in the `TABLES` clause (`TABLES (orders PRIMARY KEY (id))` defaults the alias to the table name, matching Snowflake's `[alias AS] table`); a view-level `COMMENT = '...'` may be written in Snowflake's trailing position (after the last clause) as well as between the name and `AS` (specifying both is rejected); an explicit `PUBLIC` modifier is accepted on dimensions (a no-op, since public is the default — `PRIVATE` on a dimension is still rejected rather than silently downgraded); `WITH SYNONYMS (...)` is accepted without the `=`; and `DESC SEMANTIC VIEW` is accepted as an abbreviation of `DESCRIBE SEMANTIC VIEW`.

### Fixed

- A semantic view whose `RELATIONSHIPS` form a **cycle** (e.g. `a` references `b` and `b` references `a`) no longer hangs a query with unbounded memory growth. Such a definition parses successfully, and a query against it previously sent the fan-trap safety check's join-tree ancestor walk into an infinite loop — a cyclic relationship graph yields a cyclic parent map — allocating until the process was killed. The parent-chain walks now stop at the first revisited node, so expansion terminates. (Found by fuzzing.)
- A stray or leading comma in any clause list — `DIMENSIONS (a AS x,, b AS y)`, `TABLES (,o AS orders ...)` — is now rejected instead of being silently dropped. A single trailing comma (`METRICS (a AS ..., )`) is still tolerated.
- Malformed identifier slots in a view body are rejected instead of being silently stored as unqueryable names: a whitespace-separated multi-token name (`o.d junk AS ...`, which previously named the dimension `d junk`) and an empty quoted identifier `""` in a name or alias slot now error, matching the checks already applied to view names. An unqualified dimension/metric entry name whose expression happens to contain a dot (`region AS upper(o.region)`) now reports the missing `alias.name` qualifier instead of a misleading "Expected 'AS'".
- Documentation corrected against the implemented grammar and Snowflake's own syntax: the README no longer documents the `ONE TO ONE` / `ONE TO MANY` / `MANY TO ONE` cardinality annotations removed in v0.6.0 (cardinality is inferred from PK/UNIQUE constraints) and states the at-least-one-of-`DIMENSIONS`/`METRICS` rule correctly; the Snowflake comparison page no longer shows an `AS` keyword in the Snowflake `CREATE SEMANTIC VIEW` example (Snowflake has none) or an invalid `SEMANTIC_VIEW()` query form, adds pre-aggregation `WHERE` to the not-yet-supported list, and the DDL reference no longer shows `NON ADDITIVE BY` on a derived metric (which the parser rejects).

- **`NON ADDITIVE BY` snapshot polarity corrected to match Snowflake (breaking).** The rows are sorted by the non-additive dimensions and the rows sharing the *last ordering value* of that sort are aggregated (ties at that value all aggregate, via `RANK()`), so the default (ascending) direction now selects the **latest** snapshot and `DESC` selects the **earliest** — previously the mapping was inverted, and a view ported from Snowflake (or written to Snowflake's documented semantics) silently returned the opposite-end snapshot. **Migration**: a view that wrote `NON ADDITIVE BY (d DESC)` to get the latest snapshot should drop the `DESC` (write `NON ADDITIVE BY (d)`); one that wrote no direction to get the earliest should now add `DESC`. `NULLS` placement is unchanged and is kept as declared (only the direction is reversed internally). The default NULLS placement still follows the direction (`ASC` → `NULLS LAST`, `DESC` → `NULLS FIRST`), so a bare `NON ADDITIVE BY (d)` (latest, `NULLS LAST`) never lets a NULL key outrank a real snapshot, while `NON ADDITIVE BY (d DESC)` (earliest, `NULLS FIRST`) does; add an explicit `NULLS LAST` to exclude NULL keys regardless of direction.
- A **window metric** whose `OVER (... ORDER BY ...)` — or `PARTITION BY` / `PARTITION BY EXCLUDING` — names a dimension with a **dotted qualifier** (`ORDER BY o."order date"`) or quotes now expands correctly. A dotted `ORDER BY` reference was accepted at `CREATE` but, at query time, was matched against dimension names by bare name only, so it failed the required-dimension check even when the dimension was queried; a quoted reference was emitted as a doubled-quote non-column in the window's `OVER` clause. Every window dimension reference now resolves through the same bare-and-dotted, quote-aware resolver as the rest of the query layer and is emitted as the aggregation CTE's column alias, so the window query binds and runs.
- A `NON ADDITIVE BY` dimension written with a **dotted qualifier** (`NON ADDITIVE BY (o."order date")`) now expands correctly in the semi-additive snapshot query. Such a reference was accepted at `CREATE` but, at query time, was compared against dimension names by bare name only — so it missed its declared dimension and was emitted as a quoted non-column in the snapshot `ORDER BY`, failing at bind time. Every non-additive-dimension comparison (snapshot classification, grouping, partition/order emission, and the snapshot join) now resolves the reference through the same bare-and-dotted, quote-aware resolver used elsewhere, so a dotted (or quoted) non-additive dimension classifies, partitions, orders, and joins exactly like its bare spelling; when it is itself included in the query the metric is treated as effectively additive, matching the bare-name behaviour.
- A **semi-additive metric that snapshots on a role-playing dimension** — a metric written `m USING (<relationship>) NON ADDITIVE BY (<dim on the role-played table>)`, where that dimension is not itself in the query — now selects the snapshot for the role its `USING` clause names, instead of an arbitrary one. When one table is joined through several distinctly-named relationships (e.g. `flights → airports` as `dep_airport` / `arr_airport`), the snapshot's ranking dimension was resolved to a bare join instance whose edge was the first-declared relationship, ignoring the metric's `USING` — so a metric scoped to the arrival airport could be ranked by the departure airport's value, silently returning the wrong snapshot (and emitting a redundant extra join). The non-additive dimension is now resolved through the same role-playing `USING` context as ordinary queried dimensions, so it ranks by — and joins — the correct role; a metric with such a dimension but *no* `USING` to disambiguate the role is now rejected as ambiguous at query time (the same error a directly-queried role-playing dimension raises) rather than snapshotting by an arbitrary role.
- Ambiguous join "diamonds" are now rejected at `CREATE` instead of silently resolving to an arbitrary path. When a table is reachable from two *different* source tables (e.g. `orders → a → shared` and `orders → b → shared`), the join path is ambiguous; previously such a definition was accepted as long as the relationships were named, and a query then silently joined the shared table through whichever relationship was declared first — producing wrong numbers when the two paths point at different rows. Role-playing (multiple distinctly-named relationships from a *single* source table to one target, e.g. `flights → airports` via `dep`/`arr`) remains supported, and fan-in onto the base table is unaffected.
- Parse-error carets point more precisely at the offending token: an entry after a comma no longer drifts the caret left into the inter-entry whitespace, and a missing `(` after `NON ADDITIVE BY` / `USING` in a metric written with a leading `PRIVATE`/`PUBLIC` modifier now points at the expected-paren location instead of drifting left by the modifier's width.
- Dollar-quoted `FROM YAML` bodies now use one shared definition of a valid `$tag$` for both comment-blanking and extraction. A body opened with something that is not a valid tag — a digit-started `$1$` (`$1` is a bind parameter) or a tag containing whitespace — is rejected with a clear error instead of having the `--` / `/* */` runs inside its payload corrupted as if they were SQL comments.
- Derived-metric and fact expression inlining is now driven by one quote- and case-aware reference tokenizer, closing a class of substitution bugs: a metric/fact name appearing as the column part of a *different* table's qualified reference (`x.revenue`) or inside a single-quoted string literal (`'total revenue'`) is left untouched instead of being rewritten into invalid SQL, while a bare reference — written in any case, or double-quoted (`"Revenue"`) — inlines correctly against its declaration (a fact's own qualified form, `alias.name`, is still inlined). The fact/derived-metric CREATE-time validators and dependency scans share the same tokenizer, so they and the inliner can no longer disagree about what an expression references; a non-ASCII character abutting a name (`revenueΩ`) still never splits it into a spurious reference. Every remaining expression-text scanner now rides the same engine: a derived metric may reference a quoted metric name that contains a space (`"Total Revenue"`) and have it resolve as one reference at CREATE time (previously such a name was split and falsely reported as unknown), and the role-playing rewrite that scopes a dimension's source-table alias to a role alias touches only genuine qualifiers — a source-alias-like word inside a string literal, a same-named function call, or another table's qualified column is left intact rather than being corrupted by a blind text replace.
- Non-ASCII input no longer panics or corrupts: keyword scanning over UTF-8 text (`SHOW SEMANTIC VIEWS aΩΩ`, bodies containing multi-byte characters) previously raised "internal error (panic)", and `COMMENT` / `WITH SYNONYMS` payloads and quoted identifiers containing non-ASCII characters (`'café'`, `"café"`) were silently stored as mojibake. Error messages truncated to the FFI buffer are now cut on a character boundary instead of producing invalid UTF-8.
- DDL prefix keywords now require a word boundary: `DROP SEMANTIC VIEWS` (plural typo) no longer silently drops a view named `s`, and `CREATE SEMANTIC VIEWfoo` is no longer recognised.
- Name-only statements (`DROP` / `DESCRIBE` / `SHOW COLUMNS IN SEMANTIC VIEW`) now error on trailing garbage (`DROP SEMANTIC VIEW a b c`) instead of executing and silently discarding it; `ALTER` sub-operations do the same and now tolerate arbitrary whitespace between keywords.
- Body scanners are now quote- and string-aware: a `COMMENT = 'the PRIMARY KEY (id) lives here'` no longer fabricates a primary key from comment text; quoted identifiers containing commas, parens, or dots (`"a,b"`, `"tbl)x"`, `"a.b"`) no longer mis-split entries, close clauses early, or split at the inner dot; table-level `COMMENT` / `WITH SYNONYMS` on tables without PK/UNIQUE are stored instead of silently dropped; a column literally named `comment` is usable when quoted.
- SQL comments are handled correctly across the DDL surface: trailing comments are no longer absorbed into stored expressions or `ALTER ... RENAME TO` targets, comment text can no longer corrupt clause scanning, comments may appear between prefix keywords, and block comments nest per the SQL standard.
- `GET_DDL` output now re-parses to the same definition: relationships declared against a `UNIQUE` key render their `REFERENCES (columns)` list (previously dropped, silently rewiring the join to the primary key on re-parse), and view names that need quoting (mixed case, whitespace, non-ASCII) are quoted in the rendered header.
- `SHOW ... STARTS WITH` / `LIMIT` require word boundaries (`STARTSWITH`, `LIMIT5` are rejected); `NON ADDITIVE BY` accepts flexible whitespace including the no-space `BY(dim)` form; `READ_YAML_FROM_SEMANTIC_VIEW` resolves qualified names with quote awareness instead of splitting at dots inside quoted parts.
- Fact and metric queries combining a child-table fact/metric with a dimension on a shared parent table no longer raise a spurious ambiguity error when the parent is referenced by multiple child tables (regression in the previous role-playing ambiguity hardening).
- Querying a semantic view whose stored relationships lack foreign-key column metadata (a legacy pre-Phase-24 definition format) now fails with a clear "re-create it with `CREATE OR REPLACE SEMANTIC VIEW`" error instead of silently skipping the fan-trap safety check and returning mis-aggregated results — the relationship graph builds empty for such rows, so the check would otherwise pass vacuously.

## [0.10.4] - 2026-06-27

### Changed

- DuckDB version pin bumped to `v1.5.4`.

## [0.10.3] - 2026-06-13

### Fixed

- **Windows community-extension build fixed against the updated MSVC toolchain.** The vendored DuckDB amalgamation bundles `fmt` 6.1.2, which on MSVC selected `stdext::checked_array_iterator` (under `#ifdef _SECURE_SCL`). Recent Microsoft STL versions removed that symbol entirely, so the `windows-latest` build began failing to compile the amalgamation with `error C2653: 'stdext': is not a class or namespace name`. The Windows build-time patch applied to `duckdb.cpp` now disables that branch so `fmt` takes its portable raw-pointer path — the same code every non-Windows platform already compiled. No functional or API changes from 0.10.2; this only restores the Windows build so the community-extensions registry can keep producing a Windows binary as new DuckDB versions ship.

## [0.10.2] - 2026-06-04

### Fixed

- **Passthrough facts named after their own column now work.** A fact whose expression references its own name — the natural 1:1 passthrough, e.g. `FACTS (s.unit_price AS s.unit_price)` — was rejected at `CREATE` time with `cycle detected in facts: unit_price -> unit_price`, and even when the expression differed slightly it expanded incorrectly. Two issues are fixed: (1) a fact's own name in its own expression is now treated as a reference to the physical column, not a self-cycle (genuine cycles between distinct facts are still rejected); and (2) fact-reference inlining replaced the qualified (`alias.name`) and unqualified (`name`) forms in two sequential passes, so for an identity fact the second pass re-scanned the first's output and produced corrupt SQL (`s.unit_price` → `(s.(s.unit_price))`) — replacement now happens in a single non-re-scanning pass. `DESCRIBE SELECT * FROM semantic_view('v', facts := ['unit_price'])` returns the declared fact name as the output column. Note: as in Snowflake, each clause entry reads `name AS expression` (logical name before `AS`, SQL expression after) — the reverse of a plain SQL `expression AS alias`.

## [0.10.1] - 2026-06-03

### Fixed

- **Community-extension build now loads on DuckDB 1.5.3.** The v0.10.0 binary published to the community-extensions registry was compiled against DuckDB 1.5.2 (the `duckdb`/`libduckdb-sys` crates were pinned to `=1.10502.0`) and stamped for v1.5.2, so on DuckDB 1.5.3 `INSTALL semantic_views FROM community; LOAD semantic_views;` failed with `The file was built specifically for DuckDB version 'v1.5.2' and can only be loaded with that version of DuckDB`. All DuckDB version pins are bumped to 1.5.3 — `.duckdb-version`, the `duckdb`/`libduckdb-sys` crates, the distribution workflows, and the Python test/example headers — so the rebuilt extension targets and loads on DuckDB 1.5.3. No functional or API changes from 0.10.0.

## [0.10.0] - 2026-05-27

Connection-lifecycle and ADBC fixes. Two downstream regressions reported against v0.8.0/v0.9.0 — an in-process `read_only=True` reopen that hung indefinitely, and `SELECT … FROM semantic_view(...)` failing with `Catalog Error: Table X does not exist` through ADBC — were both rooted in extension-owned long-lived `duckdb_connection` handles whose catalog/schema search path diverged from the caller's. v0.10.0 retires both long-lived handles and moves every read-side callback to a per-call `Connection(*context.db)` that inherits the caller's context natively. The two symptoms resolve as consequences. PK auto-inference from `duckdb_constraints()` is removed in the same release as the architectural pivot (see Changed).

### Changed

- **In-process `read_only=True` reopen no longer hangs.** After a writable handle that did `LOAD semantic_views` + `CREATE SEMANTIC VIEW` is closed, a subsequent `duckdb.connect(path, read_only=True)` against the same path returns within milliseconds in the same Python process. Previously the reopen hung indefinitely (>45 s observed) because the extension's long-lived catalog connection kept the `Database` alive past the caller's `close()`. Both extension-owned long-lived `duckdb_connection` handles (catalog read connection and query expansion connection) are retired from `init_extension`; every read-side and DDL-rewrite callback now opens its own per-call `Connection(*context.db)` borrowed from the caller's `ClientContext`. A structural Rust test (`tests/no_long_lived_conn.rs`) fails CI if anyone re-introduces a long-lived native handle inside `init_extension`.
- **`SELECT … FROM semantic_view(...)` now works through ADBC and other clients with diverging catalog search paths.** All seven physical-table emission sites in the expansion engine (main expansion, FACTS, semi-additive metrics, window metrics, materialization routing, and `EXPLAIN SEMANTIC VIEW`) now emit fully-qualified `database.schema.table` references. Previously the main expansion path was qualified (since v0.9.0) but the four feature paths still emitted unqualified `FROM "table"` references that resolved against the extension's separate connection; through ADBC the caller's catalog search path was not visible to the extension's connection, surfacing as `Catalog Error: Table X does not exist`. A new `just test-adbc-queries` recipe runs seven end-to-end ADBC scenarios covering main / FACTS / semi-additive / window / materialization / multi-DB ATTACH; all pass on v0.10.0 and fail on v0.9.0.
- **PK auto-inference from `duckdb_constraints()` removed (BREAKING).** When `TABLES (a AS t)` declared no `PRIMARY KEY` and `t` had a physical PK in the catalog, the extension previously imported the catalog PK at DDL-time. This auto-fallback is gone: PRIMARY KEY in a semantic view is now treated as a logical user assertion (the Snowflake-aligned model), not a physical catalog import. Users must explicitly declare `PRIMARY KEY (cols)` or `UNIQUE (cols)` in the TABLES clause, or use `REFERENCES target(cols)` shorthand on the foreign side. The DDL fails fast with a clear "primary key required" error pointing at the explicit-declaration alternative. Migration: add the explicit `PRIMARY KEY (...)` clause to any TABLES entry that previously relied on the auto-fallback.
- `semantic_view(...)` and the `SHOW`/`DESCRIBE` family now raise a clear `Binder Error` when DuckDB's column-type inference fails at bind time. Previously the bind path fell back to `VARCHAR` or `DECIMAL(18,3)` silently, masking the underlying problem. If a query that previously succeeded with the wrong column type now fails, the error message will name the underlying cause — typically a missing source table, a broken `expr`, or a permissions issue surfaced by the expanded SQL.

### Fixed

- `DROP SEMANTIC VIEW` and `ALTER SEMANTIC VIEW` against a read-only database that was never bootstrapped now report `semantic view 'X' does not exist`. Previously the lower-level `Catalog Error: Table _definitions does not exist` leaked through.
- Extension registration failures during `LOAD semantic_views` now surface the underlying DuckDB exception message in the user-visible error. Previously the message was dropped and callers saw only a generic `Failed to register …`, which made ADBC, JDBC, and Python users unable to diagnose load-time failures.
- `LOAD semantic_views` is now idempotent. Repeated loads in the same process no longer accumulate duplicate parser-extension hooks in `DBConfig`.
- Quoted source-table references with embedded whitespace or SQL keywords in the `TABLES (...)` clause (e.g. `TABLES (a AS "my table" PRIMARY KEY (id))`) are now parsed correctly. Previously the body parser tokenised on whitespace before respecting quoted-identifier boundaries, causing the source-table name to be truncated. Resolves TECH-DEBT #24.
- Non-additive dimension lists and window `OVER (ORDER BY ...)` clauses are now tokenised with identifier-quoting awareness, so quoted columns containing whitespace or commas no longer split across tokens.
- Removed unreachable single-shot fallback branches in four table-function exec callbacks that would have produced unbounded row streams if local state were ever absent. Table-function registration now refuses callbacks without an `init_local`, so the invariant is enforced at registration time rather than papered over per call.

### Security

- Eliminated a SQL-injection surface in the `CREATE SEMANTIC VIEW v FROM YAML FILE '<path>'` helper. The path argument is now read via DuckDB's `FileSystem` API directly, removing the `SELECT content FROM read_text('…')` indirection that relied on quote-doubling. `enable_external_access` gating is preserved natively by `LocalFileSystem`.

### Removed

- Internal `type_cache` module and `type_id_to_display_name` helper. Both were unused after the read-side rebuild in v0.10.0 and have been deleted (~317 LOC purge). No user-visible API impact.

## [0.9.0] - 2026-05-17

### Added

- **Read-only database LOAD support.** ``LOAD semantic_views`` now succeeds on a read-only DuckDB database. Previously-defined semantic views can be queried via ``list_semantic_views()``, ``describe_semantic_view()``, and ``FROM semantic_view(...)`` against a database opened with ``read_only=True`` (Python) or ``--readonly`` (CLI). On a read-only database that was never bootstrapped, the catalog is treated as empty rather than raising a missing-table error: ``list_semantic_views()`` returns zero rows, and ``describe_semantic_view('x')`` / ``FROM semantic_view('x', ...)`` return the standard ``semantic view 'x' does not exist`` error for any name.
- **Read-only DDL surfaces DuckDB's standard error.** ``CREATE SEMANTIC VIEW``, ``DROP SEMANTIC VIEW``, and ``ALTER SEMANTIC VIEW`` against a read-only database fail with DuckDB's standard ``Cannot execute statement of type "..." on database "..." which is attached in read-only mode!`` error rather than the previous confusing schema-create failure at LOAD time.
- **`examples/readonly_load.py`** demonstrating the open-writable → bootstrap → close → reopen-readonly → query → catch-DDL-error workflow.
- **`just test-readonly` recipe + `test/integration/test_readonly_load.py`** Python integration test (three scenarios: fresh read-only file, bootstrapped reopen, DDL rejection) and `test/sql/readonly_load.test` writable-side smoke fixture. Wired into `just test-all`.

### Fixed

- **Quoted identifier handling in `CREATE / DROP / ALTER / DESCRIBE / SHOW COLUMNS SEMANTIC VIEW`.** All five DDL forms now accept any combination of quoted, partially-quoted, and unquoted fully-qualified names — e.g. `CREATE OR REPLACE SEMANTIC VIEW "memory"."main"."orders_sv" AS ...`, `CREATE SEMANTIC VIEW main."orders_sv" AS ...`, and `CREATE SEMANTIC VIEW orders_sv AS ...` all store the same bare key, and `FROM semantic_view('orders_sv', ...)` resolves them uniformly. `ALTER ... RENAME TO` normalises both the source name and the new-name target. Error messages reference the unquoted bare name. Previously, a quoted FQN was stored verbatim (quotes and all) as the lookup key, which made any subsequent `semantic_view('orders_sv', ...)` call return "view does not exist".
- **Triple-quoted identifiers in expanded SQL.** When a `TABLES (o AS "memory"."main"."orders" ...)` clause used a quoted source-table reference, the expansion path re-quoted each part producing strings like `"""memory"""."""main"""."""orders"""` in the generated `FROM` clause. The expansion now operates on parsed identifier parts and emits exactly one pair of quotes per part regardless of input shape, restoring `EXPLAIN SEMANTIC VIEW` legibility and (in the rare case the source-table reference had embedded special chars) correctness of the generated SQL.

### Known limitations

- The v0.1.0 → v0.2.0 companion-file migration cannot run on a read-only database (the migration INSERTs into ``semantic_layer._definitions`` which requires write access). Practical impact is near-zero — the companion-file format is four milestone versions stale and any database last opened with v0.2.0+ has already been migrated. If you have a v0.1.0-era database that has never been opened by any newer release, open it once writable to complete the migration before reverting to read-only.

## [0.8.0] - 2026-05-06

### Added

- **Transactional DDL.** `CREATE`, `DROP`, and `ALTER SEMANTIC VIEW` now participate in the caller's transaction. `BEGIN ... ROLLBACK` rolls back uncommitted catalog changes and `BEGIN ... COMMIT` persists them, matching the contract that ADBC, dbt, and other transaction-aware clients expect.
- **`parser_override` extension hook.** Recognised DDL is rewritten into native `INSERT` / `UPDATE` / `DELETE` against `semantic_layer._definitions` and executed on the caller's connection. Non-matching statements fall through to DuckDB's default parser unchanged.
- **All four `CREATE` forms transactional:** inline `AS` keyword body, inline `FROM YAML $$ ... $$`, `FROM YAML FILE '<path>'` (including `https://` and S3 paths via httpfs), and `CREATE OR REPLACE` / `CREATE IF NOT EXISTS` variants.
- **DROP / ALTER race guards.** Non-`IF EXISTS` `DROP SEMANTIC VIEW` and `ALTER SEMANTIC VIEW … RENAME / SET COMMENT / UNSET COMMENT` now emit a snapshot-consistent existence check on the caller's connection before the DML. If a concurrent commit lands between the catalog pre-check (committed-state read on a separate connection) and the DML, the user sees `semantic view '<name>' was concurrently dropped` instead of a silent no-op. `IF EXISTS` variants keep their silent-no-op contract.
- **`CatalogReader` RAII.** `prepared_lookup` and `execute_list_all` use internal `PreparedStmt` and `QueryResult` guards. Manual `duckdb_destroy_*` calls along error paths are gone.
- **`ParserOptions` size assert.** A static assert pins `sizeof(ParserOptions) == 32` against DuckDB v1.5.2 (the upstream version pinned via the `=1.10502.0` duckdb-rs crate). Silent layout drift previously surfaced as garbage parser errors at position 0; future DuckDB bumps now fail fast at compile time.
- **Actionable error when `allow_parser_override_extension` is `DEFAULT` or `STRICT`** (e.g. after `CALL disable_peg_parser()` resets the setting). Issuing semantic DDL on such a connection now produces `Parser Error: semantic_views: parser_override is not active for this connection (allow_parser_override_extension is 'DEFAULT' or 'STRICT'). Re-enable with: SET allow_parser_override_extension='FALLBACK';` with caret positioned at the start of the statement.
- **ADBC end-to-end test** (`test/integration/test_adbc_transactions.py`, runnable via `just test-adbc`) exercising `autocommit=False` rollback / commit semantics for inline, FROM YAML FILE, ALTER, and DROP forms — proves the original ADBC bug is fixed end-to-end.
- **Concurrent-CREATE Python integration test** (`test/integration/test_concurrent_ddl.py`, runnable via `just test-concurrent`).
- **`INSERT OR REPLACE` row-count, byte-identical rollback (MD5), and same-txn `list_semantic_views` visibility cases** in `v080_transactional_ddl.test`.
- **Type-inference under `BEGIN/COMMIT`** in `test_type_inference.py`.
- **Arbitrary-bytes FFI fuzz target** (`fuzz_parser_override_ffi`).
- **Caret-rendering sqllogictest fixtures** pinning caret alignment across CREATE / DROP / ALTER / multi-line / UTF-8 / multi-DB / extension-reload paths.
- **`peg_compat.test` regression coverage** that the override path keeps working under DuckDB's experimental PEG parser, so v0.8.0's transactional DDL survives the upcoming parser switch. Under PEG, every DDL form (including `DESCRIBE` and `SHOW`) works because parser_override fires before whichever parser is active.

### Changed

- **Architectural unification.** `parser_override` is the sole DDL entry point. Every recognised form — `CREATE` (all four variants), `DROP`, `ALTER`, `DESCRIBE`, `SHOW SEMANTIC *`, `GET_DDL`, `READ_YAML_FROM_SEMANTIC_VIEW` — is rewritten by a single Rust dispatch and re-parsed by DuckDB on the caller's connection. The legacy `parse_function` / `sv_ddl_internal` table-function fallback was retired (~1500 LOC net deletion). One execution path means transactional semantics, error reporting, and PEG/Bison compatibility are all uniform.
- **`CatalogState` HashMap removed.** All catalog reads now query `_definitions` directly through a single shared `CatalogReader`. This eliminates the divergence risk between the HashMap and the on-disk table that the old write-through-both pattern carried.

### Fixed

- **FFI UTF-8 hardening.** `sv_parser_override_rust` now validates input bytes with checked `from_utf8` instead of `from_utf8_unchecked`. Malformed input cleanly defers to the default parser instead of triggering UB.
- **`parse_table_function_call` tightening.** The internal helper now rejects `foo(,)`, `foo('a',)` (trailing comma), and `foo('a' 'b')` (missing comma between args). Previously these silently parsed as zero-arg or merged-arg calls.
- **Validation errors arrive as parse-time errors with caret rendering.** `CREATE`, `DROP`, and `ALTER SEMANTIC VIEW` validation failures (e.g. `semantic view 'X' does not exist`, unknown clause) surface as `Parser Error: ... LINE 1: ... ^` with the caret aligned to the offending token, matching DuckDB's native parser-error rendering. Internally, `parser_override` keeps the success / transactional path (rewrite to native SQL, re-parse on caller's connection); validation failures defer (`DISPLAY_ORIGINAL_ERROR`), the default parser fails on the unrecognised DDL prefix, and DuckDB calls `parse_function`, which re-runs validation and returns `DISPLAY_EXTENSION_ERROR` with `error_location` set to the offending byte offset.

### Known limitations

- `semantic_view(...)` queries do not see uncommitted writes to user tables in the same transaction. Expansion runs on a separate `query_conn`, which only sees committed state. Workaround: commit the user-table writes before querying. Inline expansion will be revisited when DuckDB 2.0's PEG grammar-extension API ships.
- A `CREATE SEMANTIC VIEW` issued in the same uncommitted transaction is not visible to subsequent reads in that transaction (e.g. `SHOW SEMANTIC VIEWS` will not list it until commit). With the HashMap gone, reads see only committed catalog state. Workaround: commit before reading. See TECH-DEBT item 19.
- `CALL disable_peg_parser()` resets `allow_parser_override_extension` to `default`, which silently bypasses parser_override hooks. Workaround: re-issue `SET allow_parser_override_extension='FALLBACK'` after disabling PEG. The extension installs `FALLBACK` on load, so a process that never enables PEG never sees this. See TECH-DEBT item 21.
- `CREATE SEMANTIC VIEW IF NOT EXISTS` is silent-no-op only against rows visible in the caller's MVCC snapshot. Two parallel processes that each see the row absent will both attempt the INSERT and the loser sees `ConstraintException: Duplicate key "name: <view>" violates primary key constraint` at commit — the same shape plain `CREATE` produces under contention. Multi-process bootstrap scripts should catch this and treat it as success. See TECH-DEBT item 23.

## [0.7.2] - 2026-05-01

### Fixed

- Parser hook now strips leading SQL comments before matching `CREATE / ALTER / DROP / SHOW SEMANTIC VIEW` DDL. Previously, any statement preceded by a `/* ... */` block comment or `-- ... \n` line comment was misclassified as not-our-statement and DuckDB surfaced `Parser Error: syntax error at or near "SEMANTIC"`. This made the extension unusable through dbt-duckdb (which unconditionally prepends a query annotation comment to every statement) and any other tool that prefixes annotations (sqlfluff, BI tools that prepend session/user metadata, etc.). Reported and diagnosed by an external user. Block comments are non-nesting, matching PostgreSQL/DuckDB semantics. Error-position byte offsets are preserved across the consumed comment span, so error carets continue to reference the original query string.

## [0.7.1] - 2026-04-26

### Added

- DDL-time type inference for dimensions and metrics: `data_type` / `DATA_TYPE` columns in SHOW and DESCRIBE output now display inferred types (VARCHAR, BIGINT, DOUBLE, DATE, etc.) instead of empty strings
- Type inference runs automatically at `CREATE SEMANTIC VIEW` time on file-backed databases via a LIMIT 0 probe query
- Supported types: VARCHAR, BOOLEAN, integer types (TINYINT through UBIGINT), FLOAT, DOUBLE, DATE, TIME, TIMESTAMP (all variants), INTERVAL, UUID, BLOB, BIT; DECIMAL and parameterized types intentionally left empty to avoid lossy CAST
- Derived metrics also receive inferred types when resolvable
- In-memory databases continue to show empty `data_type` (no persist connection available)

## [0.7.0] - 2026-04-24

### Added

- YAML definition format: `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$` as an alternative to SQL DDL keyword body
- YAML file loading: `CREATE SEMANTIC VIEW name FROM YAML FILE '/path/to/file.yaml'` with DuckDB `enable_external_access` security enforcement
- Dollar-quoting for inline YAML: both untagged (`$$...$$`) and tagged (`$yaml$...$yaml$`) forms
- YAML export: `READ_YAML_FROM_SEMANTIC_VIEW('name')` scalar function with lossless round-trip fidelity
- Materialization declarations: `MATERIALIZATIONS` clause in SQL DDL and YAML for declaring pre-aggregated tables
- Materialization routing engine: transparent query redirection to pre-aggregated tables on exact dimension/metric match
- Semi-additive and window function metrics excluded from materialization routing (always expand from raw sources)
- `explain_semantic_view()` now includes materialization routing decision (`-- Materialization: <name>` or `-- Materialization: none`)
- `DESCRIBE SEMANTIC VIEW` includes MATERIALIZATION rows with table, dimensions, and metrics properties
- `SHOW SEMANTIC MATERIALIZATIONS [IN view_name]` command with LIKE/STARTS WITH/LIMIT filtering

## [0.6.0] - 2026-04-14

### Added

- Metadata annotations: COMMENT, SYNONYMS (aliases), PRIVATE/PUBLIC access modifiers on views, tables, dimensions, metrics, and facts
- ALTER SEMANTIC VIEW SET COMMENT / UNSET COMMENT DDL for modifying view-level comments after creation
- GET_DDL('SEMANTIC_VIEW', 'name') scalar function for reconstructing re-executable CREATE OR REPLACE DDL from stored definitions
- SHOW TERSE SEMANTIC VIEWS for reduced-column introspection output
- SHOW COLUMNS IN SEMANTIC VIEW for a unified list of all dims, facts, and metrics with a `kind` column
- IN SCHEMA / IN DATABASE scope filtering for all SHOW SEMANTIC commands
- Wildcard selection (`table_alias.*`) in dimensions and metrics query parameters, expanding to all matching PUBLIC items
- Queryable FACTS via `facts := [...]` parameter in the table function for row-level unaggregated results
- Semi-additive metrics via NON ADDITIVE BY (dimension [ASC|DESC] [NULLS FIRST|LAST]) for snapshot-style aggregation using CTE-based ROW_NUMBER
- Window function metrics via PARTITION BY EXCLUDING for non-aggregated, partition-aware computation
- Synonyms and comment columns in all SHOW SEMANTIC command output
- Comment and access_modifier properties in DESCRIBE SEMANTIC VIEW output
- Mutual exclusion: facts + metrics in same query produces a blocking error
- Mutual exclusion: window function metrics + aggregate metrics in same query produces a blocking error
- SHOW SEMANTIC DIMENSIONS FOR METRIC shows `required=TRUE` for window partition dimensions

### Changed

- FFI catch_unwind wrapping on all 25 entry points (Rust panics no longer unwind through C++ stack frames)
- Graceful lock-poison handling across all catalog and query paths (error return instead of panic)
- Cycle detection and MAX_DERIVATION_DEPTH=64 limit for derived metrics and facts
- DimensionName/MetricName newtypes with case-insensitive semantics replace bare strings in query resolution
- Resolution loop deduplication via generic resolve_names helper

## [0.5.5] - 2026-04-05

### Added

- Snowflake-aligned column schemas for all SHOW SEMANTIC commands (VIEWS, DIMENSIONS, METRICS, FACTS)
- Snowflake-aligned DESCRIBE SEMANTIC VIEW property-per-row format
- Metadata fields: created_on timestamp, database_name, schema_name on semantic view model
- Per-fact output_type metadata

### Changed

- Refactored expand.rs into expand/ module directory (7 submodules)
- Refactored graph.rs into graph/ module directory (5 submodules)
- Extracted shared util.rs and errors.rs as leaf modules to break circular dependencies

## [0.5.4] - 2026-03-31

### Added

- UNIQUE constraints on tables in TABLES clause with automatic cardinality inference for relationships
- Implicit PK reference resolution (REFERENCES target without column list resolves to target's PRIMARY KEY)
- ALTER SEMANTIC VIEW RENAME TO for renaming views
- SHOW SEMANTIC DIMENSIONS / METRICS / FACTS introspection commands
- LIKE, STARTS WITH, and LIMIT filtering for all SHOW SEMANTIC commands
- Documentation site (Sphinx + Shibuya theme on GitHub Pages)
- Community Extension Registry descriptor (description.yml)
- MAINTAINER.md contributor documentation

### Changed

- DuckDB version support: 1.5.x (latest) + 1.4.x LTS with dual CI matrix
- Relationship cardinality inferred from PK/UNIQUE constraints instead of explicit keywords

### Removed

- Explicit cardinality keywords on relationships (breaking: views must be recreated)

## [0.5.3] - 2026-03-15

### Added

- FACTS clause for named reusable row-level sub-expressions in semantic view definitions
- Derived metrics (metric-on-metric composition with DAG resolution and cycle detection)
- Fan trap detection with blocking errors for one-to-many aggregation fan-out
- Role-playing dimensions (same table via multiple join paths)
- USING RELATIONSHIPS clause for explicit join path selection in queries
- Multi-level fact inlining with proper parenthesization for operator precedence

## [0.5.2] - 2026-03-13

### Added

- SQL keyword DDL body: TABLES, RELATIONSHIPS, DIMENSIONS, METRICS clauses replace function-call syntax
- PK/FK relationship model with table aliases and graph-validated JOIN synthesis
- Alias-based query expansion with qualified column names (direct FROM+JOIN instead of CTE flattening)
- Parser robustness: token-based keyword matching tolerates arbitrary whitespace
- Adversarial input hardening (null bytes, embedded semicolons, Unicode homoglyphs, control characters)

### Removed

- Function-call DDL body syntax (breaking: `define_semantic_view()` interface retired)

## [0.5.1] - 2026-03-09

### Added

- DROP SEMANTIC VIEW and DROP SEMANTIC VIEW IF EXISTS
- CREATE OR REPLACE SEMANTIC VIEW
- CREATE SEMANTIC VIEW IF NOT EXISTS
- DESCRIBE SEMANTIC VIEW
- SHOW SEMANTIC VIEWS
- Error location reporting with character positions (caret indicators in DuckDB output)
- Clause-level error hints and "did you mean?" fuzzy suggestions for misspelled clause/view names
- Parser property-based tests (proptests) for DDL parsing

## [0.5.0] - 2026-03-08

### Added

- Native `CREATE SEMANTIC VIEW` DDL syntax via C++ parser extension hook
- Parser fallback hook registration (C_STRUCT entry + C++ helper)
- Rust FFI trampoline for detecting `CREATE SEMANTIC VIEW` prefix
- Statement rewriting pipeline (native DDL to function-based execution)
- Dedicated DDL connection to avoid lock conflicts

## [0.4.0] - 2026-03-03

### Changed

- Time truncation expressed via dimension `expr` directly (e.g., `date_trunc('month', created_at)`)
- DDL simplified from 6 to 4 named parameters
- Query function simplified from 3 to 2 named parameters

### Removed

- `time_dimensions` DDL parameter (breaking)
- `granularities` query parameter (breaking)

## [0.3.0] - 2026-03-03

### Changed

- Replaced binary-read dispatch with zero-copy vector references (`duckdb_vector_reference_vector`)
- Streaming chunk-by-chunk output instead of collect-all-then-write
- Type mismatches handled at SQL generation time via `build_execution_sql` cast wrapper

### Removed

- ~600 LOC of per-type read/write dispatch code

## [0.2.0] - 2026-03-03

### Added

- C++ shim infrastructure for Rust+C++ boundary (vendored DuckDB amalgamation via cc crate)
- Time dimensions with granularity coarsening and per-query granularity override
- `pragma_query_t` catalog persistence (replaced sidecar file with DuckDB-native table persistence)
- Scalar function DDL interface (`define_semantic_view()`)
- Snowflake-aligned STRUCT/LIST DDL syntax
- EXPLAIN support for expanded SQL inspection
- Typed output columns (zero-copy vector reference with runtime type validation)
- DuckDB type-mapping with property-based tests
- DuckLake integration test suite and CI

### Removed

- Sidecar file persistence (replaced by pragma_query_t)

## [0.1.0] - 2026-02-28

### Added

- Initial extension scaffold using `duckdb/extension-template-rs`
- Multi-platform CI build matrix (Linux x86_64/arm64, macOS x86_64/arm64, Windows x86_64)
- Scheduled DuckDB version monitor with automated PR creation
- Code quality gates: `rustfmt`, `clippy` (pedantic), `cargo-deny`, 80% coverage
- Developer task runner (`just`) with `just setup` one-command dev environment
- Pre-commit hooks via `cargo-husky` (rustfmt + clippy)
- Semantic view definition storage and round-trip persistence across DuckDB restarts
- Expansion engine: automatic GROUP BY and JOIN generation from dimension/metric declarations
- Query interface via table function `semantic_view('view', dimensions := [...], metrics := [...])`
- `list_semantic_views()` and `describe_semantic_view()` introspection functions
- Fuzz targets for FFI boundary testing

[Unreleased]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.10.4...HEAD
[0.10.4]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.10.3...v0.10.4
[0.10.3]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.10.2...v0.10.3
[0.10.2]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.10.1...v0.10.2
[0.10.1]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.7.2...v0.8.0
[0.7.2]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.5.5...v0.6.0
[0.5.5]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.5.2...tags/v0.5.3
[0.5.2]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/anentropic/duckdb-semantic-views/compare/v1.0...v0.2.0
[0.1.0]: https://github.com/anentropic/duckdb-semantic-views/releases/tag/v1.0
