# Phase 7: Verification & Formal Closure - Research

**Researched:** 2026-02-26
**Domain:** Human verification, documentation, milestone archival
**Confidence:** HIGH

## Summary

Phase 7 is a verification and documentation phase -- no code changes are expected. The phase closes all remaining items from the v1.0 milestone audit (`v1.0-MILESTONE-AUDIT.md`) by: (1) executing the 8 human verification tasks that require a live environment, (2) documenting accepted design decisions and architectural limitations in a TECH-DEBT.md file, and (3) performing a readability review of MAINTAINER.md.

All 28 v0.1 requirements are already satisfied. Phase 6 closed the code-level tech debt (dead code, feature-gate, test portability, idempotency). Phase 7 addresses the remaining 7 "human verification needed" items plus the TECH-DEBT.md documentation deliverable. These are tasks a human must execute against real infrastructure (GitHub Actions, nightly toolchain, DuckLake environment) -- they cannot be automated or run in a sandboxed agent context.

**Primary recommendation:** Structure this as a single plan with two sections: (1) a verification checklist that the human executes sequentially, reporting pass/fail for each item, and (2) creation of TECH-DEBT.md documenting accepted decisions, deferred items, and known limitations for v0.2 reference. The TECH-DEBT.md content can be drafted by the planner using information already present in STATE.md, the milestone audit, and REQUIREMENTS.md.

## Standard Stack

### Core

No new libraries or tools required. All verification uses existing project infrastructure.

| Tool | Version | Purpose | Why Standard |
|------|---------|---------|--------------|
| `just` | Already installed | Runs test-sql, test-iceberg, fuzz commands | Project command runner |
| `cargo-fuzz` | Already installed | Runs fuzz targets with nightly toolchain | Required for TEST-05 verification |
| `gh` CLI | System install | Triggers GitHub workflows, views CI status | GitHub Actions interaction |
| Python 3 | System install | Runs DuckLake integration test | Required for test_ducklake.py |

### Supporting

None.

### Alternatives Considered

None -- this phase uses only existing tools.

## Architecture Patterns

### Verification Execution Order

The 8 human verification tasks have natural dependencies. The recommended execution order:

```
Step 1: Push to GitHub (if not already pushed)
   |
   +-- Step 2: Verify CI workflows (PullRequestCI, MainDistributionPipeline, CodeQuality)
   |
   +-- Step 3: Trigger DuckDB Version Monitor manually
   |
Step 4: Run `just test-sql` locally
   |
Step 5: Run `just setup-ducklake && just test-iceberg` locally
   |
Step 6: Run all 3 fuzz targets locally (requires nightly)
   |
Step 7: MAINTAINER.md readability review
   |
Step 8: Write TECH-DEBT.md
```

Steps 2 and 3 can run in parallel (both are GitHub-side). Steps 4-6 are sequential local commands. Steps 7-8 are documentation tasks.

### Pattern 1: CI Workflow Verification

**What:** Confirm that GitHub Actions workflows execute successfully.
**When to use:** After pushing all Phase 6 code changes to remote.

**Verification commands:**
```bash
# Push current state to GitHub
git push origin main

# Check CI workflow status (after push triggers them)
gh run list --workflow=CodeQuality.yml --limit=1
gh run list --workflow=MainDistributionPipeline.yml --limit=1

# For PullRequestCI, create a test PR or check existing PR runs
gh run list --workflow=PullRequestCI.yml --limit=1

# Manually trigger DuckDB Version Monitor
gh workflow run DuckDBVersionMonitor.yml
gh run list --workflow=DuckDBVersionMonitor.yml --limit=1 --json status,conclusion
```

**Expected outcomes:**
- CodeQuality: passing (fmt, clippy, cargo-deny, coverage >= 80%)
- MainDistributionPipeline: passing on all 5 platforms (linux_amd64, linux_arm64, osx_amd64, osx_arm64, windows_amd64)
- PullRequestCI: passing on linux_amd64
- DuckDBVersionMonitor: completes without error; if DuckDB is already at latest, reports "No action needed"; if newer version exists, opens a PR (either version-bump or breakage)

### Pattern 2: Local Test Verification

**What:** Run the full test suite locally to confirm green status.
**When to use:** After Phase 6 code changes are committed.

**Verification commands:**
```bash
# SQLLogicTest suite (exercises LOAD mechanism + DDL + query)
just test-sql

# DuckLake/Iceberg integration test
just setup-ducklake
just test-iceberg

# Fuzz targets (nightly toolchain required)
just fuzz fuzz_json_parse 10
just fuzz fuzz_sql_expand 10
just fuzz fuzz_query_names 10
```

**Expected outcomes:**
- `just test-sql`: All test files pass (semantic_views.test, phase2_ddl.test, phase4_query.test)
- `just test-iceberg`: Python test passes, semantic_query against DuckLake tables returns correct results
- All 3 fuzz targets: Run for 10 seconds each without crashes

### Pattern 3: TECH-DEBT.md Structure

**What:** A document cataloging accepted design decisions, deferred items, and known limitations for v0.2 planning.
**When to use:** At milestone closure.

**Recommended structure:**
```markdown
# Tech Debt & Deferred Items (v0.1 -> v0.2)

## Accepted Design Decisions
[Items that were intentional trade-offs, not bugs]

## Deferred to v0.2
[Requirements explicitly moved to next milestone]

## Known Architectural Limitations
[Constraints inherent to the current approach]

## Test Coverage Gaps
[Areas with reduced coverage and why]
```

**Content sources:**
- `v1.0-MILESTONE-AUDIT.md` tech_debt section (15 items across 5 phases)
- `REQUIREMENTS.md` v0.2 Requirements section (5 items)
- `STATE.md` Decisions section (all accumulated decisions)
- `PROJECT.md` v0.2 Requirements section (sidecar replacement)

### Anti-Patterns to Avoid

- **Skipping verification steps because "CI already ran":** Prior CI runs may not cover the exact commit state after Phase 6. Each verification step must be confirmed against the current HEAD.
- **Writing TECH-DEBT.md as a vague wish list:** Every item must cite its origin (audit item, requirement ID, or decision ID) and state whether it is accepted-by-design, deferred-to-v0.2, or a known limitation.
- **Treating the MAINTAINER.md review as a checkbox:** The review should be done by someone who does not already know the project. Self-review by the author does not satisfy the "readability by someone unfamiliar with Rust" criterion.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CI status checking | Manual browser navigation | `gh run list` + `gh run view` | Scriptable, auditable, faster |
| Workflow triggering | GitHub UI clicks | `gh workflow run` | Reproducible, can be documented in TECH-DEBT.md |

**Key insight:** This phase is primarily manual execution and documentation. There is nothing to hand-roll -- the tools already exist.

## Common Pitfalls

### Pitfall 1: PullRequestCI Requires an Open PR

**What goes wrong:** PullRequestCI only triggers on pull_request events. Pushing to main does not trigger it.
**Why it happens:** The workflow is scoped to `on: pull_request`.
**How to avoid:** Either (a) create a dummy PR from a branch to main, or (b) verify PullRequestCI by checking the most recent PR run that included Phase 6 changes. If no such PR exists, create one.
**Warning signs:** `gh run list --workflow=PullRequestCI.yml` shows no recent runs.

### Pitfall 2: DuckDB Version Monitor May Find No New Version

**What goes wrong:** The monitor workflow reports "Already on latest DuckDB v1.4.4. No action needed." and no PR is created.
**Why it happens:** DuckDB v1.4.4 is still the latest release at the time of verification.
**How to avoid:** This is acceptable. The success criterion is "conditional PR logic confirmed" -- the workflow completing without error and correctly determining there is no update IS a successful verification. The PR logic can be verified by reading the workflow YAML and confirming the `steps.build.outcome` conditional logic is correct.
**Warning signs:** Workflow fails before reaching the version check (permissions, API errors).

### Pitfall 3: Nightly Toolchain Not Installed

**What goes wrong:** `just fuzz` fails with "no such toolchain 'nightly'" or sanitizer errors.
**Why it happens:** The nightly toolchain was never installed, or it is outdated.
**How to avoid:** Run `rustup install nightly` before fuzzing. Run `rustup update nightly` if the installed nightly is too old.
**Warning signs:** `cargo +nightly --version` fails.

### Pitfall 4: DuckLake Setup Requires Network Access

**What goes wrong:** `just setup-ducklake` fails because it downloads the jaffle-shop dataset.
**Why it happens:** The setup script downloads CSV data from a remote source.
**How to avoid:** Ensure network access is available. Run `just setup-ducklake` before `just test-iceberg`. The setup is idempotent -- safe to run multiple times.
**Warning signs:** HTTP download errors, missing `configure/venv/bin/python3`.

### Pitfall 5: MAINTAINER.md Self-Review Bias

**What goes wrong:** The author reviews their own documentation and marks it as "readable."
**Why it happens:** The author already has deep context and cannot evaluate what a newcomer would find confusing.
**How to avoid:** Have someone unfamiliar with Rust (ideally a Python developer) follow the Quick Start through to running tests. Document any questions they ask as issues to address.
**Warning signs:** Review takes less than 15 minutes (suggests superficial scan, not a genuine following-the-steps review).

## Code Examples

No code is expected to be written in this phase. The deliverables are:

### TECH-DEBT.md Content Skeleton

```markdown
# Tech Debt & Deferred Items

## Accepted Design Decisions

### Catalog table name: `_semantic_views_catalog` vs `semantic_layer._definitions`
- **Origin:** Phase 2 audit item
- **Decision:** The implementation uses `semantic_layer._definitions` (schema + table).
  REQUIREMENTS.md originally specified `_semantic_views_catalog`. The implementation
  name is accepted as the correct design.
- **Action:** None needed. REQUIREMENTS.md DDL-05 updated to match.

### Native EXPLAIN deferred to v0.2
- **Origin:** QUERY-04 reworded during Phase 4
- **Decision:** `explain_semantic_view()` table function provides expanded SQL inspection.
  Native `EXPLAIN FROM semantic_query(...)` showing expanded SQL instead of physical plan
  requires C++ shim for EXPLAIN hook interception.
- **Action:** Tracked as QUERY-V2-03.

### Sidecar file persistence
- **Origin:** Phase 2, decision [02-04]
- **Decision:** DuckDB holds execution locks during scalar `invoke()`, preventing SQL
  execution. Sidecar file (.semantic_views) bridges this gap via plain file I/O.
- **Action:** Replace with `pragma_query_t` pattern in v0.2 C++ shim.

## Deferred to v0.2

| ID | Description | Reason |
|----|-------------|--------|
| QUERY-V2-01 | Native `CREATE SEMANTIC VIEW` DDL | Requires C++ shim for parser hooks |
| QUERY-V2-02 | Time dimensions with granularity coarsening | Scoped out of v0.1 |
| QUERY-V2-03 | Native EXPLAIN interception | Requires C++ shim for EXPLAIN hook |
| DIST-V2-01 | Community extension registry publishing | Requires upstream PR to duckdb/community-extensions |
| DIST-V2-02 | Real-world TPC-H demo | Documentation deliverable |

## Known Architectural Limitations

### FFI execution layer not fuzz-covered
- **What:** `execute_sql_raw` and `read_varchar_from_vector` in `table_function.rs`
  contain the highest-risk unsafe code but cannot be fuzz-tested standalone.
- **Why:** These functions require the DuckDB loadable-extension function-pointer stubs,
  which are only initialized at runtime when DuckDB loads the extension.
- **Mitigation:** SQLLogicTest exercises these paths with real data. Future
  `fuzz_varchar_read` target could be added if a test harness for the stubs is built.

### All output columns are VARCHAR
- **What:** `semantic_query()` casts all output to VARCHAR to avoid type mismatch panics.
- **Why:** Decision [04-03] varchar-output-columns. The FFI layer writes string data;
  DuckDB's vector types require exact type matching.
- **Impact:** Consumers must cast numeric columns. v0.2 may restore typed output.

## Test Coverage Gaps

### Iceberg test uses Python instead of SQLLogicTest
- **Origin:** Phase 4 audit item
- **Reason:** SQLLogicTest runner cannot dynamically install DuckDB extensions (DuckLake).
- **Mitigation:** `test/integration/test_ducklake.py` covers the same functionality.
```

### Verification Report Template

The planner should create verification evidence in the plan summary:

```markdown
## Verification Results

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 | CI: PullRequestCI | PASS/FAIL | gh run URL |
| 2 | CI: MainDistributionPipeline | PASS/FAIL | gh run URL |
| 3 | CI: CodeQuality | PASS/FAIL | gh run URL |
| 4 | DuckDB Version Monitor | PASS/FAIL | gh run URL or "no new version" |
| 5 | just test-sql | PASS/FAIL | output summary |
| 6 | just test-iceberg | PASS/FAIL | output summary |
| 7 | Fuzz targets (3x) | PASS/FAIL | "N runs, 0 crashes" |
| 8 | MAINTAINER.md review | PASS/DEFERRED | reviewer notes |
```

## State of the Art

Not applicable -- this phase involves project-specific verification, not evolving technology choices.

## Open Questions

1. **PullRequestCI trigger mechanism**
   - What we know: PullRequestCI only fires on pull_request events to main/release branches.
   - What's unclear: Whether the human should create a test PR specifically for Phase 7 verification, or whether an existing PR run from Phase 6 changes is sufficient evidence.
   - Recommendation: If Phase 6 changes were pushed directly to main (no PR), create a short-lived branch and PR to trigger PullRequestCI. If they were merged via PR, the existing run is sufficient.

2. **MAINTAINER.md reviewer availability**
   - What we know: The audit requires "reviewed for readability by someone unfamiliar with Rust."
   - What's unclear: Whether the project author has access to a suitable reviewer.
   - Recommendation: If no reviewer is available, mark this item as DEFERRED in the verification report and note it as a pre-release task. The MAINTAINER.md content is already comprehensive based on review of its 690-line contents. A self-review noting specific areas of concern is an acceptable fallback.

3. **Whether TECH-DEBT.md should live at repo root or in .planning/**
   - What we know: The milestone audit calls for "a TECH-DEBT.md file for v0.2 reference."
   - What's unclear: Optimal location for the file.
   - Recommendation: Place at repo root (`TECH-DEBT.md`) alongside `MAINTAINER.md`. This makes it visible to contributors browsing the repository. The `.planning/` directory is for internal project management artifacts.

## Sources

### Primary (HIGH confidence)

- **Direct codebase inspection** - all findings verified by reading project files:
  - `.planning/v1.0-MILESTONE-AUDIT.md` - complete tech debt inventory (15 items, 7 human verification)
  - `.planning/REQUIREMENTS.md` - all 28 v0.1 requirements confirmed complete, v0.2 requirements listed
  - `.planning/STATE.md` - accumulated decisions from all 6 phases
  - `.planning/ROADMAP.md` - Phase 7 success criteria
  - `.github/workflows/*.yml` - all 5 CI workflow definitions inspected
  - `Justfile` - all verification commands confirmed present
  - `MAINTAINER.md` - full 690-line document reviewed for structure and completeness
  - `fuzz/fuzz_targets/` - all 3 fuzz target source files inspected
  - `test/sql/*.test` - all 3 SQLLogicTest files confirmed present
  - `test/integration/test_ducklake.py` - DuckLake test confirmed present

### Secondary (MEDIUM confidence)

- **Phase 6 verification report** (`06-VERIFICATION.md`) - confirms Phase 6 tech debt items are resolved

## Metadata

**Confidence breakdown:**
- Verification checklist: HIGH - all items are directly derived from the milestone audit's human verification section
- TECH-DEBT.md content: HIGH - all content is sourced from existing project documentation
- Execution order: HIGH - dependencies between verification steps are straightforward
- MAINTAINER.md review: MEDIUM - the success criterion depends on reviewer availability

**Research date:** 2026-02-26
**Valid until:** 2026-03-26 (stable milestone closure, no external dependencies changing)
