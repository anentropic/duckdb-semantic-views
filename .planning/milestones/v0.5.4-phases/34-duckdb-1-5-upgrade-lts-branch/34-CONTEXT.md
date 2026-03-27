# Phase 34: DuckDB 1.5 Upgrade & LTS Branch - Context

**Gathered:** 2026-03-15
**Status:** Ready for planning

<domain>
## Phase Boundary

Upgrade the extension to build and pass all tests against DuckDB 1.5.x (latest) on main. Create a `duckdb/1.4.x` branch maintaining 1.4.x LTS compatibility. CI runs both versions independently. DuckDB Version Monitor updated for dual-track monitoring. New features, documentation site, and CE registry publishing are separate phases.

</domain>

<decisions>
## Implementation Decisions

### Branch strategy
- Main-first development: all new features land on `main` (targeting DuckDB 1.5.x)
- LTS branch named `duckdb/1.4.x` (not `andium`) — enables `duckdb/*` CI pattern matching
- Best-effort cherry-pick of features from main to `duckdb/1.4.x` — don't block main if cherry-picks are messy
- `duckdb/1.4.x` gets a suffixed Cargo.toml version (e.g., `0.5.4+duckdb1.4`) to distinguish from main

### Upgrade approach
- Push through breaking changes — fix whatever DuckDB 1.5 breaks in the C++ shim, build.rs, duckdb-rs API, or amalgamation
- Update Windows patches in build.rs for DuckDB 1.5 amalgamation layout — fix, don't skip
- Update extension-ci-tools submodule to v1.5.x tag on main; `duckdb/1.4.x` keeps v1.4.4 pin
- Ignore PEG parser as default behavior (Bison hooks should still work), but add a test that loads extension with PEG enabled to document compatibility status

### CI structure
- Branch-based CI: Build.yml on main builds against 1.5.x, Build.yml on `duckdb/1.4.x` builds against 1.4.x
- Use `duckdb/*` branch pattern in workflow triggers where appropriate
- Full `just test-all` quality gate on both branches (cargo test + sqllogictest + DuckLake CI + vtab crash + caret tests)
- No matrix — the branch IS the version selector

### Version monitor
- Single DuckDBVersionMonitor.yml with two jobs: `check-latest` (bumps main) and `check-lts` (bumps `duckdb/1.4.x`)
- Reuses existing PR-creation logic, adapted for each target branch

### Release & tagging
- Dual tags: `v0.5.4` on main (DuckDB 1.5.x), `v0.5.4-duckdb1.4` on `duckdb/1.4.x` (DuckDB 1.4.x)
- Tags released simultaneously to keep version numbers in sync
- CE `description.yml` uses commit hashes: `ref` from main, `andium` from `duckdb/1.4.x` branch
- Tags are for our release management; CE uses commit hashes

### Claude's Discretion
- Exact duckdb-rs version for 1.5.x (likely `=1.5.0` or `=1.10500.0` — determine from crates.io)
- Whether build.rs Windows patches need updating or can be removed for 1.5
- Specific shim.cpp changes needed for DuckDB 1.5 API
- How to structure the PEG compatibility test
- Order of operations for the upgrade (Cargo.toml first vs amalgamation first vs CI first)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### DuckDB version management
- `.duckdb-version` — Single source of truth for target DuckDB version (currently `v1.4.4`)
- `Cargo.toml` — `duckdb = "=1.4.4"` and `libduckdb-sys = "=1.4.4"` exact pins (lines 32-33)
- `Makefile` — Reads `.duckdb-version`, downloads amalgamation, sets `UNSTABLE_C_API_FLAG`
- `justfile` — `update-headers` recipe downloads amalgamation from GitHub releases

### Build infrastructure
- `build.rs` — C++ amalgamation compilation, symbol visibility, Windows macro patches
- `cpp/src/shim.cpp` — C++ parser hook registration (not read yet — must be checked for 1.5 compat)
- `.github/workflows/Build.yml` — Currently hardcoded to `v1.4.4` ci-tools tag (line 23)
- `.github/workflows/DuckDBVersionMonitor.yml` — Weekly check for new DuckDB releases, single-track

### CE registry
- DuckDB CE docs: https://duckdb.org/community_extensions/documentation — `description.yml` format
- Example with `andium` field: https://github.com/duckdb/community-extensions/blob/main/extensions/yaml/description.yml — Shows `repo.ref` + `repo.andium` dual-version pattern
- CE UPDATING guide: https://github.com/duckdb/community-extensions/blob/main/UPDATING.md — Release process

### Requirements
- `.planning/REQUIREMENTS.md` — DKDB-01 through DKDB-06 requirements for this phase

### Tech debt
- `TECH-DEBT.md` — Items #2 (version pinning), #10 (amalgamation compilation), #11 (C_STRUCT_UNSTABLE ABI)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `.duckdb-version` file + Makefile `TARGET_DUCKDB_VERSION` pattern — established version-pin mechanism, extend to both branches
- `DuckDBVersionMonitor.yml` — existing weekly check workflow, adapt for dual-track
- `justfile` `update-headers` recipe — amalgamation download, works for any version in `.duckdb-version`
- `build.rs` Windows patch infrastructure — patch-at-build-time pattern, may need marker updates for 1.5

### Established Patterns
- Exact version pin (`=1.4.4`) in Cargo.toml — same pattern for 1.5.x
- `CARGO_FEATURE_EXTENSION` gate in build.rs — C++ compilation only for extension builds
- `extension-ci-tools` submodule pinned to DuckDB version tag — update to v1.5.x on main
- CI `uses: duckdb/extension-ci-tools/.github/workflows/...@v1.4.4` — hardcoded tag, must update

### Integration Points
- `Cargo.toml` lines 32-33: `duckdb` and `libduckdb-sys` version pins
- `.duckdb-version`: consumed by Makefile, justfile, CI
- `.github/workflows/Build.yml` line 23: hardcoded ci-tools version tag
- `build.rs` lines 60-94: Windows patch markers tied to specific amalgamation line numbers
- `Makefile` line 23: `UNSTABLE_C_API_FLAG=--abi-type C_STRUCT_UNSTABLE`

</code_context>

<specifics>
## Specific Ideas

- Branch naming: `duckdb/1.4.x` (not `andium`) — enables `duckdb/*` glob matching in CI triggers
- DuckDB 1.5.0 is already released — this is a concrete upgrade, not speculative
- PEG parser test: load extension with PEG enabled, document whether hooks fire — don't fix issues, just document
- CE registry `andium` field confirmed: `description.yml` supports `repo.andium` alongside `repo.ref` for dual-version publishing

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 34-duckdb-1-5-upgrade-lts-branch*
*Context gathered: 2026-03-15*
