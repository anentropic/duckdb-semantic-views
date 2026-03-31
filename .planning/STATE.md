---
gsd_state_version: 1.0
milestone: v0.5.4
milestone_name: Snowflake-Parity & Registry Publishing
status: v0.5.4 milestone complete
stopped_at: Completed 36-02-PLAN.md
last_updated: "2026-03-27T14:17:48.516Z"
progress:
  total_phases: 6
  completed_phases: 5
  total_plans: 12
  completed_plans: 11
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-15)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand
**Current focus:** Phase 36 — registry-publishing-maintainer-docs

## Current Position

Phase: 36
Plan: Not started

## Performance Metrics

**Velocity:**

- Total plans completed: 4 (v0.5.4)
- Average duration: 38min
- Total execution time: 134min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 33 | 2/2 | 39min | 19min |
| 34 | 2/2 | 95min | 47min |

**Recent Trend:**

- Last 5 plans: 33-01 (25min), 33-02 (14min), 34-01 (90min), 34-02 (5min)
- Trend: CI/infra plans are fast; compilation-heavy plans take longer

*Updated after each plan completion*
| Phase 34.1 P01 | 10min | 2 tasks | 8 files |
| Phase 34.1 P02 | 13min | 2 tasks | 8 files |
| Phase 34.1 P03 | 13min | 2 tasks | 7 files |
| Phase 34.1.1 P01 | 12min | 2 tasks | 3 files |
| Phase 35 P01 | 5min | 3 tasks | 6 files |
| Phase 36 P01 | 4min | 2 tasks | 3 files |
| Phase 36-registry-publishing-maintainer-docs P02 | 5min | 2 tasks | 2 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [v0.5.4 roadmap]: Cardinality inference before DuckDB upgrade -- do not mix feature changes with version changes
- [v0.5.4 roadmap]: Separate branches for dual-version support (main=1.5.x, andium=1.4.x) -- Cargo.toml version pin makes single-branch impractical
- [v0.5.4 roadmap]: Registry publishing last -- depends on stable code, dual builds, and docs
- [33-01]: Removed OneToMany variant entirely -- cardinality always from FK-side perspective
- [33-01]: ref_columns resolved at parse time in infer_cardinality, not deferred to graph
- [33-01]: Case-insensitive column matching via HashSet for PK/UNIQUE inference
- [33-02]: Replaced check_fk_pk_counts with validate_fk_references using exact HashSet matching
- [33-02]: ON clause synthesis prefers ref_columns, falls back to pk_columns for backward compat
- [33-02]: Test 6 redesigned with p33_user_tokens table to avoid VARCHAR-to-INTEGER type mismatch
- [34-01]: Separate TU with compat header (not combined TU) -- libpg_query macros in duckdb.cpp break shim code
- [34-01]: ODR compliance requires verbatim constructor match in compat header including ParserOverrideResult(std::exception&)
- [34-01]: Per-process sqllogictest execution for DuckDB 1.5.0 parser extension lifecycle compatibility
- [34-01]: date_trunc returns TIMESTAMP in DuckDB 1.5.0 -- updated all test assertions
- [34-02]: LTS branch duckdb/1.4.x created from 8f0b3fa (pre-upgrade commit) to preserve v1.4.4 state
- [34-02]: Cargo.toml version 0.5.4+duckdb1.4 on LTS branch uses semver build metadata for disambiguation
- [34-02]: Inline version bumping in Version Monitor replaces nonexistent just bump-duckdb recipe
- [34-02]: Dual-track Version Monitor: check-latest (main) + check-lts (duckdb/1.4.x) as parallel jobs
- [Phase 34.1]: AlterRename reuses DropState pattern (persist_conn + if_exists bool) for VTab consistency
- [Phase 34.1]: VTab pair pattern: single-view (1 param) + cross-view (0 params) sharing bind/init types for SHOW commands
- [Phase 34.1]: SHOW FACTS outputs 4 columns (no data_type) since Fact model lacks output_type field
- [Phase 34.1]: FOR METRIC detected inside ShowDimensions rewrite handler, not as separate DdlKind variant
- [Phase 34.1]: Fan-trap filtering reuses pub(crate) expand.rs helpers instead of duplicating logic
- [Phase 34.1]: rewrite_show_dims_for_metric extracted to keep rewrite_ddl under clippy line limit
- [Phase 34.1.1]: Parser-level filtering via WHERE/LIMIT injection -- no VTab changes needed
- [Phase 34.1.1]: Removed rewrite_show_dims_for_metric (absorbed into unified parse_show_filter_clauses)
- [Phase 35]: Removed intersphinx: DuckDB docs lack objects.inv, 404 breaks -W builds
- [Phase 35]: Docs.yml deploys only from main (not milestone branches) to protect published docs
- [Phase 35]: cancel-in-progress: false on Pages to avoid incomplete deployments
- [Phase 36]: Used union of Build.yml and D-04 for excluded_platforms (8 platforms)
- [Phase 36]: Replaced BSD-3-Clause LICENSE with MIT to match Cargo.toml canonical license
- [Phase 36]: Set description.yml ref to PLACEHOLDER_COMMIT_SHA for post-squash replacement
- [Phase 36-registry-publishing-maintainer-docs]: Updated MAINTAINER.md architecture sections (source tree, data flow, catalog persistence) to reflect current parser hook + pragma_query_t architecture

### Pending Todos

- [ ] Investigate WASM build strategy (extension vs custom DuckDB build) — `.planning/todos/pending/2026-03-19-investigate-wasm-build-strategy.md`
- [ ] Explore dbt semantic layer integration via DuckDB — `.planning/todos/pending/2026-03-19-explore-dbt-semantic-layer-integration-via-duckdb.md`
- [ ] Pre-aggregation materializations with query-driven suggestions — `.planning/todos/pending/2026-03-19-pre-aggregation-materializations-with-query-driven-suggestions.md`

### Blockers/Concerns

- [Research]: CE registry build pipeline for hybrid Rust+C++ is untested -- submit draft PR early in Phase 36
- [RESOLVED 34-01]: DuckDB 1.5.0 amalgamation compatibility with shim.cpp -- fixed via parser_extension_compat.hpp
- [RESOLVED 34-01]: duckdb-rs 1.10500.0 API changes -- no breaking changes, all 467 Rust tests pass

### Roadmap Evolution

- Phase 34.1 inserted after Phase 34: Close DDL gaps vs Snowflake: ALTER SEMANTIC VIEW, SHOW SEMANTIC DIMENSIONS/FACTS/METRICS (URGENT)
- Phase 34.1.1 inserted after Phase 34.1: Close gaps with Snowflake SHOW SEMANTIC DDLs — add LIKE, STARTS WITH, LIMIT (URGENT)

### Quick Tasks Completed

| # | Description | Date | Commit | Status | Directory |
|---|-------------|------|--------|--------|-----------|
| 260318-fzu | remove HIERARCHIES syntax, no backward compat considerations needed | 2026-03-18 | 72fb69d | | [260318-fzu-remove-hierarchies-syntax-no-backward-co](./quick/260318-fzu-remove-hierarchies-syntax-no-backward-co/) |
| 260320-ekj | Fix Windows CI: replace /dev/stdin with temp file in per-process sqllogictest loop | 2026-03-20 | fc8d582 | | [260320-ekj-fix-windows-ci-replace-dev-stdin-with-te](./quick/260320-ekj-fix-windows-ci-replace-dev-stdin-with-te/) |
| 260321-i40 | Custom Pygments lexer for SQL grammar syntax highlighting in reference docs | 2026-03-21 | fb672de | | [260321-i40-custom-pygments-lexer-for-sql-grammar-sy](./quick/260321-i40-custom-pygments-lexer-for-sql-grammar-sy/) |
| 260322-1zx | Make PRIMARY KEY optional in TABLES clause via catalog metadata lookup | 2026-03-22 | d09e4cc | Verified | [260322-1zx-make-primary-key-optional-by-referring-t](./quick/260322-1zx-make-primary-key-optional-by-referring-t/) |
| 260322-s2y | Add LIKE/STARTS WITH/LIMIT filtering to SHOW SEMANTIC VIEWS | 2026-03-22 | 285c3bc | Verified | [260322-s2y-add-like-starts-with-limit-filtering-to-](./quick/260322-s2y-add-like-starts-with-limit-filtering-to-/) |
| 260329-frb | Sync DuckDBVersionMonitor with current build workflows | 2026-03-29 | eef265b | | [260329-frb-sync-duckdbversionmonitor-yml-with-curre](./quick/260329-frb-sync-duckdbversionmonitor-yml-with-curre/) |
| 260331-ta2 | Add justfile release recipe for CE registry publishing | 2026-03-31 | 0390bab | | [260331-ta2-write-a-justfile-recipe-for-the-release-](./quick/260331-ta2-write-a-justfile-recipe-for-the-release-/) |

## Session Continuity

Last activity: 2026-03-31 - Completed quick task 260331-ta2: release recipe
Last session: 2026-03-27T10:34:09.249Z
Stopped at: Completed 36-02-PLAN.md
Resume file: None
