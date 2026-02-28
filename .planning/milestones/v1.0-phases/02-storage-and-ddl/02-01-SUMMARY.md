---
phase: 02-storage-and-ddl
plan: "01"
subsystem: database
tags: [rust, duckdb, serde_json, catalog, persistence, hashmap]

# Dependency graph
requires:
  - phase: 01-scaffold
    provides: Rust cdylib extension skeleton that loads; lib.rs entrypoint; Cargo.toml baseline
provides:
  - SemanticViewDefinition struct with serde deserialization and from_json validation (src/model.rs)
  - CatalogState type alias (Arc<RwLock<HashMap<String, String>>>) and init_catalog, catalog_insert, catalog_delete helpers (src/catalog.rs)
  - serde + serde_json deps; vscalar feature available via `extension` feature flag
  - Feature-based build split: `default=bundled` for tests, `extension=loadable-extension+vscalar` for DuckDB cdylib
affects:
  - 02-02 (define/drop scalar functions — use CatalogState, catalog_insert, catalog_delete)
  - 02-03 (list/describe table functions — use CatalogState, SemanticViewDefinition)
  - 03-expansion-engine (uses SemanticViewDefinition as the model type)

# Tech tracking
tech-stack:
  added:
    - serde 1 (derive feature)
    - serde_json 1
    - duckdb bundled feature (dev/test builds only)
  patterns:
    - Write-catalog-first: write to semantic_layer._definitions before updating HashMap; error propagates via ? without touching HashMap
    - CatalogState: Arc<RwLock<HashMap<String, String>>> shared mutable state pattern
    - Feature-gated extension entrypoint: #[cfg(feature = "extension")] gates loadable-extension-dependent code; default feature uses bundled DuckDB so unit tests work with Connection::open_in_memory()
    - Idempotent catalog init: CREATE SCHEMA/TABLE IF NOT EXISTS safe to call on every extension load

key-files:
  created:
    - src/model.rs (SemanticViewDefinition, Dimension, Metric, Join, from_json validation)
    - src/catalog.rs (CatalogState, init_catalog, catalog_insert, catalog_delete, 6 unit tests)
  modified:
    - Cargo.toml (serde/serde_json deps; features section: default=bundled, extension=loadable-extension+vscalar; duckdb default-features=false)
    - src/lib.rs (pub mod catalog; pub mod model; extension entrypoint gated behind cfg(feature = "extension"))
    - Makefile (build targets override to pass --no-default-features --features extension for DuckDB cdylib builds)

key-decisions:
  - "Feature-gated build split: default feature enables duckdb/bundled for cargo test (Connection::open_in_memory works); extension feature enables duckdb/loadable-extension+vscalar for DuckDB cdylib; Makefile passes --no-default-features --features extension"
  - "duckdb/loadable-extension replaces ALL DuckDB C API calls with function-pointer stubs initialized by DuckDB at extension load time — unit tests CANNOT use these stubs; bundled default feature resolves this without workspace restructuring"
  - "Write-catalog-first pattern: catalog_insert/catalog_delete write to semantic_layer._definitions first; HashMap updated only on Ok — prevents HashMap/catalog drift on error"
  - "serde deny_unknown_fields on SemanticViewDefinition: unknown JSON keys return parse error immediately — strict schema at definition time"

patterns-established:
  - "CatalogState pattern: Arc<RwLock<HashMap<String, String>>> shared across all DDL functions; read lock for queries, write lock only after successful catalog write"
  - "Cargo feature split for testable DuckDB extensions: default=bundled enables unit tests; extension=loadable-extension enables cdylib for DuckDB; Makefile uses --no-default-features --features extension for extension builds"

requirements-completed:
  - DDL-01
  - DDL-02
  - DDL-03
  - DDL-04
  - DDL-05

# Metrics
duration: 18min
completed: 2026-02-24
---

# Phase 2 Plan 1: Storage and DDL — Model and Catalog Foundation Summary

**SemanticViewDefinition Rust model with serde validation + CatalogState CRUD helpers backed by `semantic_layer._definitions` DuckDB table, with feature-gated build split enabling Connection-based unit tests in a loadable extension**

## Performance

- **Duration:** 18 min
- **Started:** 2026-02-24T07:02:46Z
- **Completed:** 2026-02-24T07:20:23Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- `src/model.rs`: `SemanticViewDefinition` with `Dimension`, `Metric`, `Join` sub-structs; `from_json` parses and validates JSON with `deny_unknown_fields`; 5 unit tests cover valid round-trip, missing fields, invalid JSON, unknown fields, and optional defaults
- `src/catalog.rs`: `CatalogState` type alias (`Arc<RwLock<HashMap<String, String>>>`); `init_catalog` creates schema+table idempotently and loads existing rows; `catalog_insert` validates JSON then writes catalog first; `catalog_delete` checks existence then writes catalog first; 6 unit tests covering all paths including idempotency and restart simulation
- Cargo feature split: `default = ["duckdb/bundled"]` makes `cargo test` work with `Connection::open_in_memory()`; `extension = ["duckdb/loadable-extension", "duckdb/vscalar"]` used by Makefile for the actual DuckDB cdylib; this resolves the fundamental incompatibility between duckdb's loadable-extension function-pointer stubs and standalone test binaries

## Task Commits

Each task was committed atomically:

1. **Task 1: SemanticViewDefinition model and Cargo.toml deps** - `f8d96db` (feat)
2. **Task 2: catalog.rs + feature-gated build split** - `fb63fde` (feat)

## Files Created/Modified

- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/src/model.rs` — SemanticViewDefinition, Dimension, Metric, Join structs with serde derive; from_json with deny_unknown_fields; 5 unit tests
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/src/catalog.rs` — CatalogState type alias; init_catalog (idempotent schema+table creation, row loading); catalog_insert (validate→check duplicate→write catalog→update HashMap); catalog_delete (check existence→delete catalog→remove from HashMap); 6 unit tests
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/Cargo.toml` — Added serde/serde_json deps; features section (default=bundled, extension=loadable-extension+vscalar); duckdb dep with default-features=false
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/src/lib.rs` — Added pub mod catalog; pub mod model; gated extension entrypoint behind #[cfg(feature = "extension")] module
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/Makefile` — Overrode build targets to add --no-default-features --features extension to cargo build calls

## Decisions Made

- **Feature-gated build split**: The `duckdb/loadable-extension` feature replaces ALL DuckDB C API calls with function-pointer stubs that require the DuckDB host to call `duckdb_rs_extension_api_init`. Standalone test binaries never have a DuckDB host, so any `Connection::open_in_memory()` panics. Adding `default = ["duckdb/bundled"]` to the package features makes `cargo test` compile with bundled DuckDB (direct calls, no stubs). The extension feature is only active when building the cdylib via Makefile.

- **`#[allow(clippy::unnecessary_wraps)]` on entrypoint**: The `duckdb_entrypoint_c_api` macro calls the entrypoint function via `?`, requiring `Result<(), Box<dyn Error>>` return type. The lint is a false positive — the `Result` is required by the macro interface.

- **`serde(deny_unknown_fields)` on SemanticViewDefinition**: Unknown JSON fields raise a parse error immediately at define time. This prevents silently accepting invalid definitions that would cause confusing failures at query time.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy::uninlined_format_args in model.rs from_json**
- **Found during:** Task 1 (model.rs implementation)
- **Issue:** `format!("...", name, e)` triggers `uninlined_format_args` under pedantic clippy
- **Fix:** Changed to `format!("invalid definition for semantic view '{name}': {e}")`
- **Files modified:** `src/model.rs`
- **Verification:** `cargo clippy -- -D warnings` exits 0
- **Committed in:** f8d96db (Task 1 commit)

**2. [Rule 3 - Blocking] Resolved loadable-extension/bundled incompatibility for unit tests**
- **Found during:** Task 2 (catalog.rs tests)
- **Issue:** `duckdb/loadable-extension` replaces DuckDB C API functions with function-pointer stubs initialized only when DuckDB loads the extension. Running `cargo test` panics with "DuckDB API not initialized" because the DuckDB host never calls the init function in test mode.
- **Fix:** Added `[features]` section to Cargo.toml with `default = ["duckdb/bundled"]` (for `cargo test`) and `extension = ["duckdb/loadable-extension", "duckdb/vscalar"]` (for cdylib builds). Updated `src/lib.rs` to gate the extension entrypoint behind `#[cfg(feature = "extension")]`. Updated `Makefile` to pass `--no-default-features --features extension` when building the DuckDB extension binary.
- **Files modified:** `Cargo.toml`, `src/lib.rs`, `Makefile`
- **Verification:** `cargo test` passes 11 tests; `cargo build --no-default-features --features extension` builds the cdylib
- **Committed in:** fb63fde (Task 2 commit)

**3. [Rule 1 - Bug] Fixed three clippy pedantic violations in catalog.rs and lib.rs**
- **Found during:** Task 2 verification (`cargo clippy --no-default-features --features extension -- -D warnings`)
- **Issue:** (a) `HashMap` unquoted in doc comment (`doc_markdown`), (b) redundant closure in `.map_err(|e| Box::from(e))` (`redundant_closure`), (c) `unnecessary_wraps` on entrypoint function required by macro
- **Fix:** (a) Added backticks around `HashMap` in doc comment, (b) changed to `.map_err(Box::<dyn std::error::Error>::from)`, (c) added `#[allow(clippy::unnecessary_wraps)]` with explanatory comment
- **Files modified:** `src/catalog.rs`, `src/lib.rs`
- **Verification:** `cargo clippy -- -D warnings` and `cargo clippy --no-default-features --features extension -- -D warnings` both exit 0
- **Committed in:** fb63fde (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 lint fix, 1 blocking build config, 1 three-part lint fix)
**Impact on plan:** All auto-fixes necessary for correctness and test enablement. The feature-gated build split is a foundational decision that all future phases must respect — document in CONTEXT for 02-02.

## Issues Encountered

- The loadable-extension vs bundled DuckDB incompatibility took extensive investigation (~10 min) before the correct solution was identified. The root cause: `loadable-extension` generates C API function-pointer stubs that only work when DuckDB initializes them at extension load time. The solution (feature-gated `default=bundled`) is clean and doesn't require a workspace split.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- `CatalogState`, `init_catalog`, `catalog_insert`, `catalog_delete` are ready for use by 02-02 scalar functions
- `SemanticViewDefinition` is ready for use by 02-03 table functions and Phase 3 expansion engine
- The `extension` feature flag must be used for all DuckDB extension builds in CI and local Makefile targets
- **Important for 02-02**: scalar function `invoke` receives a `&Connection` from the DuckDB host — it CAN call `catalog_insert`/`catalog_delete` directly using that connection; no second-connection workaround needed

---
*Phase: 02-storage-and-ddl*
*Completed: 2026-02-24*
