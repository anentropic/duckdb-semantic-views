# Phase 14: DuckLake Integration Test Refresh and CI Job - Context

**Gathered:** 2026-03-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Update the existing DuckLake integration test from v0.1.0 API to v0.2.0 API, add a CI-runnable variant using inline synthetic data, wire a parallel CI job into PullRequestCI.yml, and add DuckLake compatibility check to the DuckDB version monitor. No new test infrastructure beyond what's needed for DuckLake.

</domain>

<decisions>
## Implementation Decisions

### Test file structure
- Two separate test files: `test/integration/test_ducklake.py` (local, uses real jaffle-shop data) and `test/integration/test_ducklake_ci.py` (CI, generates synthetic data inline)
- Shared helpers module (e.g., `test/integration/test_ducklake_helpers.py`) for extension loading and DuckLake attach boilerplate — DRY where it makes sense
- Each test file handles its own data setup and assertions independently

### Synthetic data (CI test)
- Inline generation in the test script — no downloads, no fixture files committed to repo
- Mirror real jaffle-shop schema: same columns as `raw_orders` (id, customer, ordered_at, store_id, subtotal, tax_paid, order_total) with 5-10 synthetic rows of known values
- Use known values so time dimension assertions can verify specific expected outputs (e.g., insert rows with known `ordered_at` dates, assert truncated day values match)

### v0.2.0 API update
- Both test files (local and CI) use v0.2.0 scalar function DDL: `create_semantic_view()`, `drop_semantic_view()`, `semantic_view()`
- Do NOT use native `CREATE SEMANTIC VIEW` DDL (Phases 11 DDL-01/DDL-02 are still pending in roadmap)
- `explain_semantic_view()` stays (already v0.2.0 compatible)

### Test coverage expansion (both files)
- Keep the original 4 test cases (define view, query with dimension, global aggregate, explain)
- Add typed output assertions: verify a BIGINT metric (e.g., `count(*)`) returns a BIGINT column, not VARCHAR
- Add time dimension test: declare `ordered_at` as a time-typed dimension with day granularity, assert truncated values match expected dates using known synthetic data

### CI job placement
- Add a second job to `.github/workflows/PullRequestCI.yml` that runs in parallel with the existing `linux-fast-check` job (no `needs:` dependency — both start at the same time)
- Standard job (not using reusable extension-ci-tools workflow): runs on ubuntu-latest, installs Rust, builds extension with `cargo build --features extension`, installs Python/uv, runs `uv run test/integration/test_ducklake_ci.py`
- Required check — blocks PR merge on failure (treat DuckLake compatibility as first-class)

### DuckLake extension installation
- Install latest DuckLake from community registry (no version pin) — `INSTALL ducklake`
- `allow_unsigned_extensions=true` in DuckDB config (consistent with local test, required for our unsigned dev extension)
- Project-local extension directory (e.g., `test/data/duckdb_extensions/`) — same pattern as local test, no writes to `~/.duckdb`

### DuckDB Version Monitor
- Add a step to `DuckDBVersionMonitor.yml` that also runs the DuckLake CI test (`uv run test/integration/test_ducklake_ci.py`) when a new DuckDB version is detected
- Run after the existing build+test step, using `continue-on-error: true` so DuckLake failure is surfaced in the PR body but doesn't block the version-bump PR from being created
- Include DuckLake result in the PR description (pass/fail)

### Claude's Discretion
- Exact Justfile targets for running the CI test locally
- Whether to add `uv` to the CI runner via `pip install uv` or a dedicated action
- How to pass the built extension path to the Python test (env var, hardcoded path, etc.)
- Whether `test_ducklake.py` (local) needs the Justfile target updated or just the file contents

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `test/integration/test_ducklake.py`: existing test structure to update — 4 test cases, `check_prerequisites()` + `run_tests()` pattern
- `.github/workflows/PullRequestCI.yml`: add parallel job alongside `linux-fast-check`
- `.github/workflows/DuckDBVersionMonitor.yml`: add DuckLake step after existing build+test step
- `Justfile` `test-iceberg` and `test-all` targets: may need updating to reference new CI test file or keep as-is for local use

### Established Patterns
- `uv run <script>` pattern for Python tests (PEP 723 inline dependencies) — use for CI too
- `allow_unsigned_extensions=true` + local `extension_directory` — established in existing test, use same in CI test
- Extension loaded from `build/debug/semantic_views.duckdb_extension` in local test; CI build path may differ (`release` build or explicit cargo output path)
- DuckLake attachment: `ATTACH 'ducklake:<path>' AS <name> (DATA_PATH '<dir>')`

### Integration Points
- New job in `PullRequestCI.yml` — runs in parallel, no dependency on `linux-fast-check`
- New step in `DuckDBVersionMonitor.yml` — runs conditionally when `is_new=true`, after `steps.build`
- `test/integration/` directory — add `test_ducklake_ci.py` and `test_ducklake_helpers.py` alongside existing `test_ducklake.py`

</code_context>

<specifics>
## Specific Ideas

- CI test creates DuckLake data entirely in-memory / in a temp dir — no filesystem artifacts left behind after the job
- Phase 14 is a "refresh and wire-up" phase — the goal is correctness and CI coverage, not new features
- The version monitor PR body should indicate whether DuckLake also passed/failed alongside the main build result

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 14-ducklake-integration-test-refresh-and-ci-job*
*Context gathered: 2026-03-02*
