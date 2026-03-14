---
gsd_state_version: 1.0
milestone: v0.5.3
milestone_name: Advanced Semantic Features
status: active
stopped_at: null
last_updated: "2026-03-14T00:00:00.000Z"
last_activity: 2026-03-14 -- Roadmap created for v0.5.3
progress:
  total_phases: 4
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-14)

**Core value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand -- the extension handles expansion, DuckDB handles execution.
**Current focus:** Phase 29 - FACTS Clause & Hierarchies

## Current Position

Phase: 29 (1 of 4 in v0.5.3) (FACTS Clause & Hierarchies)
Plan: 0 of ? in current phase
Status: Ready to plan
Last activity: 2026-03-14 -- Roadmap created for v0.5.3

Progress: [..........] 0% (v0.5.3)

## Performance Metrics

**Velocity:**
- Total plans completed: 0 (v0.5.3)
- Average duration: --
- Total execution time: --

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| - | - | - | - |

**Recent Trend:**
- Last 5 plans: --
- Trend: --

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [v0.5.3 roadmap]: FACTS + Hierarchies first (FACTS unblocks derived metrics; hierarchies are pure metadata)
- [v0.5.3 roadmap]: Role-Playing and USING combined into single phase (tightly coupled -- USING enables role resolution)
- [v0.5.3 roadmap]: Semi-additive metrics deferred to v0.5.4 (only feature requiring expansion pipeline structural change)

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 30]: Derived metric expression substitution needs word-boundary matching to avoid substring collisions
- [Phase 30]: Facts must be parenthesized when inlined to preserve operator precedence
- [Phase 32]: Diamond rejection relaxation must be atomic with USING-aware expansion
- [Phase 32]: Dimension-USING scope inheritance needs design decision during planning

## Session Continuity

Last session: 2026-03-14
Stopped at: Roadmap created for v0.5.3 milestone
Resume file: None
