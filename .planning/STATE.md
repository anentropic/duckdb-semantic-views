---
gsd_state_version: 1.0
milestone: v0.6.0
milestone_name: Snowflake SQL DDL Parity
status: complete
stopped_at: Milestone v0.6.0 shipped
last_updated: "2026-04-14"
last_activity: 2026-04-14
progress:
  total_phases: 8
  completed_phases: 8
  total_plans: 16
  completed_plans: 16
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-04-14)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand
**Current focus:** Planning next milestone

## Current Position

Phase: -
Plan: -
Status: v0.6.0 milestone shipped — ready for next milestone
Last activity: 2026-04-14

Progress: [██████████] 100%

## Performance Metrics

**Velocity:**

- Total plans completed: 16 (v0.6.0)
- Average duration: --
- Total execution time: 0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 43 | 2 | - | - |
| 44 | 2 | - | - |
| 45 | 2 | - | - |
| 46 | 2 | - | - |
| 47 | 2 | - | - |
| 48 | 2 | - | - |
| 49 | 2 | - | - |
| 50 | 2 | - | - |

*Updated after each plan completion*
| Phase 45 P01 | 64min | 2 tasks | 7 files |
| Phase 46 P01 | 69min | 3 tasks | 14 files |
| Phase 46 P02 | 53min | 2 tasks | 6 files |
| Phase 47 P01 | 31min | 2 tasks | 8 files |
| Phase 47 P02 | 47min | 2 tasks | 6 files |
| Phase 48 P02 | 32min | 2 tasks | 8 files |
| Phase 49 P01 | 43min | 2 tasks | 16 files |
| Phase 49 P02 | 79min | 2 tasks | 19 files |
| Phase 50 P01 | 48min | 3 tasks | 4 files |
| Phase 50 P02 | 20min | 3 tasks | 10 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [v0.6.0 planning]: Build order follows tier model -- Tier 1 (model+DDL) before Tier 2 (expansion mods) before Tier 3 (expansion structural changes)
- [v0.6.0 planning]: Semi-additive uses ROW_NUMBER() CTE, not LAST_VALUE IGNORE NULLS (DuckDB LTS crash bug)
- [v0.6.0 planning]: All new model fields use #[serde(default)] for backward compatibility
- [Phase 45]: Generalize DdlKind::AlterRename to DdlKind::Alter with sub-operation dispatch for RENAME TO, SET COMMENT, UNSET COMMENT
- [Phase 46]: Wildcard module placed in expand/ (not query/) so unit tests run without extension feature
- [Phase 46]: Fact queries use LIMIT 0 type inference rather than DDL-time type map (facts use per-fact output_type)
- [Phase 47]: NON ADDITIVE BY extracted before USING in parse order to handle both together
- [Phase 47]: DESC defaults to NULLS FIRST when user does not specify NULLS (matches DuckDB/Snowflake)
- [Phase 47]: GET_DDL always emits explicit NULLS LAST/FIRST to avoid version divergence
- [Phase 47]: CTE with ROW_NUMBER() for semi-additive expansion (not LAST_VALUE due to DuckDB LTS crash)
- [Phase 47]: Fan trap check skips semi-additive metrics entirely (ROW_NUMBER handles fan-out)
- [Phase 47]: Effectively-regular classification: all NA dims in query -> standard aggregation, no CTE
- [Phase 48]: CTE __sv_agg aggregates inner metrics by all queried dims; outer SELECT applies window functions with computed PARTITION BY
- [Phase 48]: Window metrics and aggregate metrics are mutually exclusive in same query (WindowAggregateMixing error)
- [Phase 49]: Use .map_err() with descriptive string instead of into_inner() lock recovery for poisoned locks
- [Phase 49]: AssertUnwindSafe justified at FFI boundary: panics caught and converted to errors, no partially-mutated state observed
- [Phase 49]: MAX_DERIVATION_DEPTH=64 prevents stack overflow from linear metric chains that pass cycle detection
- [Phase 50]: Retain one golden-file anchor test while converting 4 others to property assertions for refactor resilience
- [Phase 50]: resolve_names uses 9 closure parameters for error construction at call sites, avoiding trait objects
- [Phase 50]: DimensionName/MetricName newtypes with AsRef<str> + Deref for seamless string interop

### Pending Todos

- [ ] Investigate WASM build strategy -- `.planning/todos/pending/2026-03-19-investigate-wasm-build-strategy.md`
- [ ] Explore dbt semantic layer integration -- `.planning/todos/pending/2026-03-19-explore-dbt-semantic-layer-integration-via-duckdb.md`
- [ ] Pre-aggregation materializations -- `.planning/todos/pending/2026-03-19-pre-aggregation-materializations-with-query-driven-suggestions.md`

### Blockers/Concerns

(None — v0.6.0 milestone complete. All prior blockers resolved.)

### Quick Tasks Completed

| # | Description | Date | Commit |
|---|-------------|------|--------|
| 260318-fzu | Remove HIERARCHIES syntax | 2026-03-18 | 72fb69d |
| 260320-ekj | Fix Windows CI per-process sqllogictest | 2026-03-20 | fc8d582 |
| 260321-i40 | Custom Pygments lexer for docs | 2026-03-21 | fb672de |
| 260322-1zx | Make PRIMARY KEY optional via catalog lookup | 2026-03-22 | d09e4cc |
| 260322-s2y | LIKE/STARTS WITH/LIMIT on SHOW VIEWS | 2026-03-22 | 285c3bc |
| 260329-frb | Sync DuckDBVersionMonitor | 2026-03-29 | eef265b |
| 260331-ta2 | Release recipe for CE registry | 2026-03-31 | 0390bab |
| 260412-v5h | Generate complete CHANGELOG.md | 2026-04-12 | d42d240 |

## Session Continuity

Last session: 2026-04-14T12:02:10.895Z
Stopped at: Completed 50-02-PLAN.md (Expand Module Refactoring)
Resume file: None
