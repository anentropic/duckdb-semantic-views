# Phase 18: Verification and Integration - Context

**Gathered:** 2026-03-08
**Status:** Ready for planning

<domain>
## Phase Boundary

Full test suite passes with native DDL coexisting alongside function-based DDL, extension binary meets registry publication requirements, and the v0.5.0 milestone is closable. This phase does NOT include the Python vtab bind crash fix (separate phase) or user-facing documentation updates.

</domain>

<decisions>
## Implementation Decisions

### Blocker: Python vtab bind crash
- The crash flagged in Phase 17 UAT (Python `create_semantic_view()` panics at `duckdb::vtab::bind`) gets its own phase BEFORE Phase 18
- Phase 18 assumes the crash is already resolved
- Working hypothesis: the crash is in our code (not upstream), since DuckLake CI passes with the same Python vtab path

### Test coverage
- Existing `phase16_parser.test` satisfies VERIFY-02 (native DDL end-to-end cycle) — no new sqllogictest tests needed
- No additional edge case or error path tests for native DDL in this phase
- BUILD-04 (cargo test without C++ overhead): Claude verifies structurally (build.rs gates on CARGO_FEATURE_EXTENSION)

### Registry readiness
- Verify binary format only: correct footer ABI type, platform symbols, no CMake dependency
- Evaluate C_STRUCT_UNSTABLE vs CPP ABI trade-off — document recommendation but don't necessarily switch
- Do NOT check community-extensions repo submission requirements or set up publish CI

### Version bump
- Bump Cargo.toml version from 0.4.0 to 0.5.0 in this phase

### Documentation scope
- No README or MAINTAINER.md updates (user-facing docs deferred)
- Update TECH-DEBT.md with v0.5.0 decisions (statement rewrite approach, DDL connection isolation, amalgamation compilation, etc.)

### Regression handling
- Run `just test-all` at START for baseline assessment, then again at END as the pass/fail gate
- Triage failures: simple regressions (< 30 min fix) handled inline; complex regressions get separate phases
- Phase 18 success requires `just test-all` green at the end

### Claude's Discretion
- Exact sequence of verification steps
- Whether BUILD-04 needs an explicit test or is self-evident from code structure
- How to verify binary format (manual inspection vs automated check)
- Content and structure of TECH-DEBT.md updates

</decisions>

<specifics>
## Specific Ideas

- The crash bug phase should write a minimal Python repro script as its first step — the DuckLake CI test works, so the crash is context-dependent
- ABI evaluation should look at how other Rust DuckDB extensions (if any exist) handle the ABI type for registry publication

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `phase16_parser.test`: Already covers CREATE SEMANTIC VIEW + query + function DDL coexistence + case insensitivity (satisfies VERIFY-02)
- `Justfile:test-all`: Orchestrates cargo test + test-sql + test-ducklake-ci — the single verification command
- `build.rs`: Feature-gating logic for C++ compilation — `CARGO_FEATURE_EXTENSION` check at line 22

### Established Patterns
- Feature gating: `#[cfg(feature = "extension")]` for extension-only code, `default = ["duckdb/bundled"]` for cargo test
- Makefile ABI: `UNSTABLE_C_API_FLAG=--abi-type C_STRUCT_UNSTABLE` — the current ABI type
- Symbol visibility: `build.rs` exports only `semantic_views_init_c_api` on Linux (dynamic-list) and macOS (exported_symbols_list)

### Integration Points
- `Cargo.toml` line 4: version = "0.4.0" — needs bump to 0.5.0
- `TECH-DEBT.md`: needs new entries for v0.5.0 decisions
- `Makefile` line 14: UNSTABLE_C_API_FLAG — may change if ABI evaluation recommends CPP

</code_context>

<deferred>
## Deferred Ideas

- **Python vtab bind crash fix** — separate phase before Phase 18 (user decision)
- **User-facing documentation** (README, MAINTAINER.md with native DDL syntax) — future phase/milestone
- **Community registry submission** — future milestone after v0.5.0 spike proves the approach
- **Publish CI pipeline** — future milestone alongside registry submission

</deferred>

---

*Phase: 18-verification-and-integration*
*Context gathered: 2026-03-08*
