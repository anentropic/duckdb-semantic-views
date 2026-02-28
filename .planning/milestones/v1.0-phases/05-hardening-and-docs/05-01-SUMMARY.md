---
phase: 05-hardening-and-docs
plan: "01"
subsystem: testing
tags: [cargo-fuzz, libfuzzer, arbitrary, fuzzing, ci, github-actions]

# Dependency graph
requires:
  - phase: 03-expansion-engine
    provides: "expand() function and model types (Dimension, Metric, Join, SemanticViewDefinition)"
provides:
  - "Three cargo-fuzz targets covering JSON parsing, SQL expansion, and query-time name injection"
  - "Seed corpus for JSON parsing fuzz target"
  - "Nightly CI workflow with crash reporting and corpus auto-commit PR"
  - "Justfile recipes for local fuzzing (fuzz, fuzz-all, fuzz-cmin)"
  - "Arbitrary derive on all model types behind feature flag"
affects: [05-02-docs]

# Tech tracking
tech-stack:
  added: [arbitrary, libfuzzer-sys, cargo-fuzz]
  patterns: [conditional-derive-behind-feature-flag, independent-fuzz-workspace, nightly-ci-fuzzing]

key-files:
  created:
    - fuzz/Cargo.toml
    - fuzz/fuzz_targets/fuzz_json_parse.rs
    - fuzz/fuzz_targets/fuzz_sql_expand.rs
    - fuzz/fuzz_targets/fuzz_query_names.rs
    - fuzz/corpus/fuzz_json_parse/seed_valid_minimal.json
    - fuzz/corpus/fuzz_json_parse/seed_valid_full.json
    - fuzz/corpus/fuzz_json_parse/seed_valid_joins.json
    - .github/workflows/NightlyFuzz.yml
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/model.rs
    - Justfile
    - .gitignore

key-decisions:
  - "conditional-arbitrary-derive: Arbitrary derive gated behind feature flag to avoid impacting default/extension builds"
  - "fuzz-crate-default-features: fuzz crate depends on default feature (duckdb/bundled) not extension; exercises pure Rust logic"
  - "separate-corpus-job: commit-corpus runs after all fuzz matrix jobs to avoid parallel push race condition"
  - "corpus-via-pr: corpus updates submitted as PR (peter-evans/create-pull-request) not direct push; consistent with DuckDB version monitor pattern"

patterns-established:
  - "Feature-gated derive: #[cfg_attr(feature = \"arbitrary\", derive(arbitrary::Arbitrary))] on model types"
  - "Independent fuzz workspace: fuzz/Cargo.toml with [workspace] key prevents parent workspace absorption"
  - "Nightly CI fuzzing: matrix strategy with continue-on-error, crash artifact upload, GitHub issue creation"

requirements-completed: [TEST-05]

# Metrics
duration: 3min
completed: 2026-02-26
---

# Phase 5 Plan 01: Fuzz Targets Summary

**Three cargo-fuzz targets covering JSON parsing, SQL expansion, and query-time name arrays with nightly CI workflow, crash reporting, and corpus auto-commit PR**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-26T11:03:41Z
- **Completed:** 2026-02-26T11:06:41Z
- **Tasks:** 2
- **Files modified:** 12

## Accomplishments
- Added `arbitrary` optional dependency with conditional `Arbitrary` derive on all four model types (Dimension, Metric, Join, SemanticViewDefinition)
- Created three fuzz targets: `fuzz_json_parse` (raw bytes to JSON parser), `fuzz_sql_expand` (arbitrary definitions + names to expand()), `fuzz_query_names` (fuzzed name arrays against fixed definition)
- Created seed corpus with three valid JSON definitions (minimal, full with filters, joins with source_table)
- Built nightly CI workflow with 3-target matrix, crash artifact upload, GitHub issue creation on failure, and separate corpus commit job using PR-based workflow
- Added fuzz, fuzz-all, and fuzz-cmin recipes to Justfile for local fuzzing

## Task Commits

Each task was committed atomically:

1. **Task 1: Add arbitrary feature flag, fuzz crate, targets, seed corpus** - `36df153` (feat)
2. **Task 2: Create nightly fuzz CI workflow** - `0c70553` (feat)

## Files Created/Modified
- `Cargo.toml` - Added `arbitrary` optional dependency and feature flag
- `Cargo.lock` - Updated with arbitrary dependency resolution
- `src/model.rs` - Added conditional `#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]` to all model types
- `fuzz/Cargo.toml` - Independent fuzz crate with three binary targets and semantic_views path dependency
- `fuzz/fuzz_targets/fuzz_json_parse.rs` - Target 1: feeds arbitrary bytes to SemanticViewDefinition::from_json()
- `fuzz/fuzz_targets/fuzz_sql_expand.rs` - Target 2: Arbitrary-derived SemanticViewDefinition + names to expand()
- `fuzz/fuzz_targets/fuzz_query_names.rs` - Target 3: fuzzed name arrays against fixed orders definition
- `fuzz/corpus/fuzz_json_parse/seed_valid_minimal.json` - Minimal seed (base_table only)
- `fuzz/corpus/fuzz_json_parse/seed_valid_full.json` - Full seed with dimensions, metrics, filters
- `fuzz/corpus/fuzz_json_parse/seed_valid_joins.json` - Seed with joins and source_table
- `.github/workflows/NightlyFuzz.yml` - Daily cron CI with matrix fuzzing, crash reporting, corpus PR
- `.gitignore` - Added fuzz/artifacts/ exclusion
- `Justfile` - Added fuzz, fuzz-all, fuzz-cmin recipes

## Decisions Made
- **Conditional Arbitrary derive:** Used `#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]` to avoid impacting default or extension builds
- **Default features for fuzz crate:** Fuzz crate depends on default feature (duckdb/bundled) not extension; fuzz targets exercise pure Rust logic (model parsing, expand()), not DuckDB loadable-extension stubs
- **Separate corpus commit job:** `commit-corpus` job runs after all fuzz matrix jobs complete (`needs: [fuzz]`) to avoid parallel push race conditions
- **PR-based corpus updates:** Uses peter-evans/create-pull-request@v7 (consistent with DuckDB version monitor workflow) instead of direct push

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Fuzz infrastructure complete, ready for MAINTAINER.md documentation (plan 05-02)
- MAINTAINER.md can document the fuzzer workflow including Justfile recipes and CI workflow
- All three fuzz targets are runnable locally via `cargo fuzz run <target>` or `just fuzz <target>`

## Self-Check: PASSED

All 8 created files verified present. Both task commits (36df153, 0c70553) verified in git log.

---
*Phase: 05-hardening-and-docs*
*Completed: 2026-02-26*
