# Phase 36: Registry Publishing & Maintainer Docs - Context

**Gathered:** 2026-03-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Create `description.yml` for the DuckDB Community Extension Registry, submit a draft PR, update MAINTAINER.md with multi-branch strategy and CE update process, and create an end-of-milestone Python example. This is the final phase of v0.5.4.

</domain>

<decisions>
## Implementation Decisions

### description.yml
- **D-01:** GitHub org is `anentropic` — `repo.github: anentropic/duckdb-semantic-views`. Fix the `paul-rl` references in MAINTAINER.md.
- **D-02:** `hello_world` uses single-table native DDL + `semantic_view()` query. Simple CREATE SEMANTIC VIEW with one table, 1-2 dimensions, 1-2 metrics, then a query. Must work end-to-end.
- **D-03:** `extension.version` matches Cargo.toml version (currently `0.5.0`). Language: `Rust`, build: `cargo`.
- **D-04:** `excluded_platforms` should match existing CI `exclude_archs`: `wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl`
- **D-05:** `requires_toolchains`: `rust;python3` (Python needed for sqllogictest runner during CI)
- **D-06:** `repo.ref` points to the release commit SHA on main (after squash-merge). Dual-version support (andium/LTS) is a future concern — initial submission targets main branch only.

### CE submission strategy
- **D-07:** Submit as a draft PR to `duckdb/community-extensions` early. The hybrid Rust+C++ build pipeline is untested — a draft PR surfaces build issues before final submission.
- **D-08:** The description.yml file lives in THIS repo (not in the community-extensions fork) so it can be tracked and versioned. The CE submission PR copies it to the fork.

### MAINTAINER.md updates
- **D-09:** Targeted updates only — fix username, update hello_world/CE section, add multi-branch section, add CE update process. Keep Prerequisites, Quick Start, Architecture, Testing, Fuzzing, CI sections as-is.
- **D-10:** Add "Multi-Version Branching Strategy" section documenting: main (latest DuckDB), duckdb/1.4.x (LTS), how to sync changes between branches.
- **D-11:** Update "Publishing to Community Extension Registry" section with correct native DDL hello_world, correct GitHub username, and step-by-step CE update process for new releases.
- **D-12:** Update "Worked Examples" section to use native DDL syntax (replace old function-based DDL examples that were retired in v0.5.2).
- **D-13:** Add "How to Bump DuckDB Version" section covering both branches (main and duckdb/1.4.x).

### Milestone close tasks
- **D-14:** Create `examples/snowflake_parity.py` demonstrating v0.5.4 features: UNIQUE constraints, cardinality inference, ALTER RENAME, SHOW SEMANTIC commands with LIKE/STARTS WITH/LIMIT.
- **D-15:** Bump Cargo.toml version to `0.5.4` as part of milestone close.
- **D-16:** Squash-merge milestone branch to main and tag `v0.5.4`. (This happens after phase execution, during `/gsd:complete-milestone`.)

### Claude's Discretion
- Exact description.yml `extended_description` wording
- MAINTAINER.md section ordering and formatting
- Python example file structure and data setup
- Whether to include a `Makefile` / `justfile` recipe for CE submission workflow

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Community Extension Registry
- `MAINTAINER.md` lines 430-481 -- Existing CE publishing section (outdated, needs updating)
- `https://duckdb.org/community_extensions/development` -- CE registry development guide (external)

### Multi-version branching
- `.duckdb-version` -- DuckDB version pin (currently `v1.5.0` on main)
- `.github/workflows/Build.yml` -- CI build matrix with dual DuckDB version support
- `.github/workflows/DuckDBVersionMonitor.yml` -- Dual-track version monitor

### Existing examples
- `examples/basic_ddl_and_query.py` -- v0.1.0 example (pattern to follow for new example)
- `examples/advanced_features.py` -- v0.5.3 example (pattern to follow)

### Project metadata
- `Cargo.toml` -- Extension name, version, description
- `docs/conf.py` -- GitHub URL (`anentropic/duckdb-semantic-views`)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `MAINTAINER.md`: Already has structure for CE publishing section — update rather than rewrite
- `examples/basic_ddl_and_query.py` and `examples/advanced_features.py`: Template for new Python example
- `.github/workflows/Build.yml`: Reference for which platforms/archs to exclude in description.yml

### Established Patterns
- Python examples use `duckdb` pip package, create in-memory database, LOAD extension from build path
- MAINTAINER.md uses table-based field references for structured information
- CI workflows follow PascalCase naming (`Build.yml`, `CodeQuality.yml`, `Docs.yml`)

### Integration Points
- `description.yml` needs to be created at repo root
- `MAINTAINER.md` at repo root — targeted sections to update
- `examples/` directory for new Python example
- `Cargo.toml` version field for milestone close bump

</code_context>

<specifics>
## Specific Ideas

- hello_world should be self-contained: CREATE TABLE, INSERT data, CREATE SEMANTIC VIEW, then FROM semantic_view(...)
- The draft PR to duckdb/community-extensions is the real test — Claude should prepare the description.yml but the user submits the PR manually

</specifics>

<deferred>
## Deferred Ideas

### Reviewed Todos (not folded)
- "Investigate WASM build strategy" -- tooling concern, not CE registry scope (keyword match false positive)
- "Pre-aggregation materializations" -- feature work, not registry scope
- "dbt semantic layer integration" -- feature research, not registry scope

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 36-registry-publishing-maintainer-docs*
*Context gathered: 2026-03-27*
