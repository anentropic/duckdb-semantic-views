# Phase 65: OverrideContext Connection Teardown — Context

**Gathered:** 2026-05-21
**Status:** Ready for research/planning
**Source:** `/gsd:discuss-phase 65 --assumptions` (root-cause framing exchange)

<domain>
## Phase Boundary

Stop the extension's long-lived `OverrideContext` catalog connection from keeping the underlying DuckDB `Database` alive past the caller's `close()`, so that an in-process `connect(path) → LOAD → CREATE SEMANTIC VIEW → close → connect(path, read_only=True)` sequence no longer hangs (>45s currently observed).

The phase's framing is **not** "find the cheapest fix that suppresses the symptom" — it is "find the **correct** model for an extension that needs internal native handles tied to a `Database`'s lifetime, and confirm whether our current handle-ownership pattern is the actual mistake." The symptom (RW→RO reopen hang) is treated as evidence we are mis-using DuckDB's extension/connection lifecycle, not as a bug to be papered over.

Out of scope (Phase 66 territory): ADBC / expansion qualification, the `qualify_and_quote_table_ref` wiring gaps in fact / semi-additive / window / materialization expansion paths, CHANGELOG, version bump, and milestone close.
</domain>

<decisions>
## Implementation Decisions

### Investigation framing (LOCKED)

- **D-01** — Root-cause investigation, not symptom suppression. Track (a) "deterministic teardown of `OverrideContext`'s `duckdb_connection`" is the primary path. Track (b) "detect access-mode mismatch in `init_extension` and surface a clear error" is **not** acceptable as the shipping fix on its own merits. (b) is admissible only as a documented limitation if (a) is demonstrably impossible after exhausting DuckDB 1.5.x C-API surface — and then alongside a real explanation of why no correct mechanism exists. (See [[feedback-root-cause-over-hacks]].)
  - Note: this narrows LIFE-02 as written in REQUIREMENTS.md. LIFE-02 lists (a) OR (b) as acceptable; user framing has locked (a) as primary with (b) only as last-resort documented limitation.

- **D-02** — Re-litigating Phase 62's "intentional bounded leak" decision is **in scope**. RESEARCH §Q2 in `.planning/phases/62-caret-restoration-lru-removal/` documented the leak as intentional and bounded. With a real downstream user hitting the resulting symptom, that decision is now treated as the load-bearing question this phase exists to revisit, not as a settled constraint.

- **D-03** — Bounded scope, signal surfacing. The Phase 65 *fix* stays scoped to the `OverrideContext` / `query_conn` lifecycle. The Phase 65 *research* explicitly looks for whether the same anti-pattern (long-lived native handles whose lifetime isn't coupled to `DatabaseInstance`/`DBConfig`) exists elsewhere in the extension. Findings that are not part of the Phase 65 fix get surfaced as new TECH-DEBT entries, new `deferred-items.md` lines, or a follow-up phase proposal — never silently absorbed and never silently dropped. (See [[feedback-bounded-scope-with-signal-surfacing]].)

### Research orientation (LOCKED)

- **D-04** — Reproduce + instrument first. Before any fix attempt, reproduce the hang and prove which reference is actually keeping `Database` alive. The candidate is `query_conn` opened in `init_extension` (`src/lib.rs:493-508`) and stashed on `SemanticViewsParserInfo` via `OverrideContext`. The repro should confirm that hypothesis or rule it out — fixing the wrong thing because the symptom matches is worse than no fix.

- **D-05** — Survey DuckDB upstream + other community extensions. Read DuckDB 1.5.x source for extension state lifecycle (`DBConfig`, `DatabaseInstance`, `extension_callbacks`, extension-unload surface). Read source of at least 2–3 other community extensions that own native handles tied to `Database` lifetime (httpfs, iceberg, ducklake, postgres scanner — pick by relevance). Goal: find the canonical pattern, not just an API call that compiles. Anchor the analysis to a specific DuckDB version and record that version in RESEARCH.md.

- **D-06** — Question the "long-lived connection" premise. Phase 58/62's "DDL needs a separate connection to avoid lock conflicts" may no longer be true given how `parser_override` is now wired. If a short-lived per-DDL `duckdb_connect` / `duckdb_disconnect` pair is the correct shape, `OverrideContext` may not need to own a connection at all — eliminating the lifetime question entirely. Research must weigh this against caching cost / lock-contention risk with evidence, not assertion.

### Solution shapes under consideration (claude's discretion to choose, with evidence)

- **D-07** — Likely candidates, in approximate order of architectural cleanliness:
  1. Don't cache a connection — open/close per DDL invocation. Eliminates lifetime concern; cost is connection-open overhead per DDL.
  2. Cache but couple teardown deterministically — register a destructor / callback that fires on `DatabaseInstance` / `DBConfig` drop and closes `query_conn` before the last `Database` reference releases.
  3. Hold a non-owning / weak handle — viable only if DuckDB's C-API exposes such a thing.
  4. (Documented limitation) Detect access-mode mismatch at `init_extension`, error early. Only if 1–3 are conclusively ruled out.

  Picking among these is for research/planner output — not pre-decided here. The constraint is "support your choice with evidence and against the alternatives."

### Scope fence

- **D-08** — Phase 65 ships a fix for the in-process RW→RO reopen hang and nothing else. Even if research turns up adjacent broken lifecycle patterns, they get surfaced (per D-03) as new findings — not folded into this phase.

- **D-09** — RO→RW reverse direction (if it has the same hang shape) is **not** mandated as in scope. If the chosen fix happens to cover it as a side effect of doing the right thing, that's fine and should be noted. If covering it would require extra work beyond the RW→RO fix, surface as a separate finding.

### Claude's Discretion

- Specific mechanism for deterministic teardown (callback, Drop impl on a wrapper, manual `duckdb_disconnect` invocation in a known-safe place) — depends on what DuckDB 1.5.x actually exposes; research determines.
- Test structure for `test_in_process_bootstrap_then_readonly` (LIFE-03) — naming, watchdog implementation, whether to parametrize across fresh/previously-bootstrapped DBs.
- Whether to add a fresh deferred-items entry for any newly surfaced findings or extend `deferred-items.md` for v0.9.0 Phase 63 in place.
</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before researching, planning, or implementing.**

### Source notes (root-cause analysis)
- `_notes/v0.9.1_readonly_reopen_hang.md` — Item 1: the downstream bug report, root-cause hypothesis, candidate fix paths (a)/(b), and explicit test requirements. **Primary phase brief.**
- `_notes/error_with_adbc.md` — adjacent Item 2 (Phase 66 scope); read only to confirm what is *not* this phase's concern.

### Phase 62 prior art (the "intentional bounded leak" being re-litigated)
- `.planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md` — RESEARCH §Q2 (or equivalent section) where the long-lived `OverrideContext` catalog connection was deemed an intentional bounded leak.
- `.planning/phases/62-caret-restoration-lru-removal/62-PLAN.md` and any sibling plans — how the OverrideContext attachment to `SemanticViewsParserInfo` was wired.
- Phase 62 commits on `main` — the actual implementation as shipped in v0.8.0.

### Phase 63 prior art (the workaround being removed)
- `.planning/milestones/v0.9.0-phases/63-readonly-database-load-support/deferred-items.md` — entry "In-process RW→RO reopen of the same DB hangs (Phase 62 OverrideContext leak)". LIFE-04 mandates updating this in place with the resolution + forward pointer.
- `test/integration/test_readonly_load.py` — current subprocess-based test; LIFE-03 adds the in-process variant alongside, does **not** delete the subprocess version.

### Source files (extension-owned connection wiring)
- `src/lib.rs:493-508` — where `query_conn` is opened via `duckdb_connect(db_handle)` at `init_extension`.
- `src/ddl/` (parser_override + define.rs) — DDL execution paths that consume `OverrideContext`.
- Wherever `OverrideContext` is defined and attached to `SemanticViewsParserInfo` (grep for the type to find current location post-Phase 62).

### Project conventions
- `CLAUDE.md` (repo root) — quality gate is `just test-all`; phases need unit tests + proptests + sqllogictest; check current branch before committing.
- `MEMORY.md` (auto-memory) — relevant feedback entries: `feedback_root_cause_over_hacks.md`, `feedback_bounded_scope_with_signal_surfacing.md`, `feedback_documented_limitations.md`, `feedback_no_parallel_builds.md`, `feedback_worktree_isolation.md`.

### DuckDB upstream (research must consult)
- DuckDB 1.5.x C-API headers — extension lifecycle, `DBConfig`, `DatabaseInstance`, `duckdb_connect` / `duckdb_disconnect`, any `extension_callbacks` / unload surface.
- 2–3 other community extensions (planner's choice — httpfs, iceberg, ducklake, postgres scanner) — read for canonical "extension owns native state tied to Database lifetime" patterns.
</canonical_refs>

<specifics>
## Specific Ideas

### Reproduction
The minimal repro from the downstream report (use as the starting test, before any fix):

```python
w = duckdb.connect(path)
w.execute("INSTALL semantic_views FROM community")  # or from local build
w.execute("LOAD semantic_views")
w.execute("CREATE TABLE sales_data (...)")
w.execute("CREATE SEMANTIC VIEW sales_view AS ...")
w.close()

r = duckdb.connect(path, read_only=True)  # currently hangs >45s
```

Without `CREATE SEMANTIC VIEW`, the RO open returns instantly. That isolation is informative — the issue is specifically about state established by DDL persisting past `w.close()`.

### Watchdog test
`test_in_process_bootstrap_then_readonly` should wrap the second `connect(..., read_only=True)` in a watchdog (5s per roadmap success criterion 1) and fail on timeout rather than wait indefinitely. The test must fail on v0.9.0 baseline and pass on v0.9.1 (per LIFE-03 + success criterion 3).

### Broader-audit deliverable
RESEARCH.md should include a "Long-lived native handles audit" section listing every long-lived native handle the extension owns (connections, prepared statements, parser-info slots, anything cached in `static` / `Once` / `OnceLock` / `Mutex`), with a one-line note on whether each is correctly coupled to the parent `Database` lifetime. Anything not correctly coupled becomes a new TECH-DEBT entry, deferred-items line, or follow-up phase proposal — per D-03.
</specifics>

<deferred>
## Deferred Ideas

- ADBC / cross-connection expansion qualification (`_notes/error_with_adbc.md` + `_notes/v0.9.1_readonly_reopen_hang.md` Item 2) — Phase 66.
- CHANGELOG entry, Cargo.toml + description.yml version bump, milestone tagging — Phase 66.
- Broader audit findings *outside* the `OverrideContext` / `query_conn` lifecycle — surface as new entries during research per D-03, but do not fold into Phase 65's fix.
- RO→RW reverse direction hang (if any) — surface as finding if discovered; not in Phase 65 scope unless covered as a side effect of doing (a) correctly (D-09).
</deferred>

---

*Phase: 65-overridecontext-connection-teardown*
*Context gathered: 2026-05-21 via /gsd:discuss-phase --assumptions exchange (written by orchestrator from conversation)*
