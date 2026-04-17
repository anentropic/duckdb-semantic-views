# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.5.5...v0.6.0
[0.5.5]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.5.2...tags/v0.5.3
[0.5.2]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/paul-rl/duckdb-semantic-views/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/paul-rl/duckdb-semantic-views/compare/v1.0...v0.2.0
[0.1.0]: https://github.com/paul-rl/duckdb-semantic-views/releases/tag/v1.0
