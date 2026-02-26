# Maintainer Guide

Everything a contributor needs to build, test, fuzz, and publish the DuckDB Semantic Views extension.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Quick Start](#quick-start)
- [Architecture Overview](#architecture-overview)
- [Building](#building)
- [Testing](#testing)
- [Loading the Extension](#loading-the-extension)
- [Updating the DuckDB Version Pin](#updating-the-duckdb-version-pin)
- [Fuzzing](#fuzzing)
- [Publishing to Community Extension Registry](#publishing-to-community-extension-registry)
- [Worked Examples](#worked-examples)
- [Troubleshooting](#troubleshooting)
- [CI Workflows](#ci-workflows)

---

## Prerequisites

Install these before anything else.

| Tool | Install | What It Does |
|------|---------|--------------|
| `rustup` | [rustup.rs](https://rustup.rs/) | Manages Rust toolchain versions (like `pyenv` for Python) |
| Rust stable | `rustup install stable` | The Rust compiler and package manager (`cargo`) |
| `just` | `cargo install just` or `brew install just` | Command runner (like `make` but simpler -- run `just` to see all commands) |
| Python 3 | You likely already have this | Needed for the SQLLogicTest runner and DuckLake integration tests |
| Git | With submodule support | The repo uses Git submodules for DuckDB CI tooling |

You do **not** need to install DuckDB separately. The build system downloads the correct DuckDB binary automatically (`make configure`), and unit tests compile DuckDB from source via the `bundled` feature.

## Quick Start

From zero to a working extension in five commands:

```bash
git clone --recurse-submodules https://github.com/paul-rl/duckdb-semantic-views.git
cd duckdb-semantic-views
just setup          # installs dev tools (cargo-nextest, cargo-deny, cargo-llvm-cov), downloads DuckDB test binary
just build          # builds the debug extension (.duckdb_extension file)
just test-rust      # runs Rust unit tests (~30 seconds)
just test-sql       # runs SQL logic tests via DuckDB's test runner
```

After `just build`, the extension binary is at `build/debug/semantic_views.duckdb_extension`.

## Architecture Overview

### Source Tree

```
src/
├── lib.rs                     # Extension entrypoint -- registers all functions with DuckDB at load time
├── model.rs                   # Data types: SemanticViewDefinition, Dimension, Metric, Join
│                              #   Handles JSON parsing/validation via serde
├── catalog.rs                 # In-memory catalog (HashMap) + sidecar file persistence
│                              #   Manages the semantic_layer._definitions table
├── expand.rs                  # SQL generation engine -- turns definitions + query requests into SQL
│                              #   Pure Rust, no DuckDB dependency at runtime
├── ddl/                       # DDL functions (only compiled for the extension build)
│   ├── mod.rs                 #   Module declarations
│   ├── define.rs              #   define_semantic_view() -- scalar function, creates a view
│   ├── drop.rs                #   drop_semantic_view() -- scalar function, removes a view
│   ├── list.rs                #   list_semantic_views() -- table function, lists all views
│   └── describe.rs            #   describe_semantic_view() -- table function, shows view details
└── query/                     # Query interface (only compiled for the extension build)
    ├── mod.rs                 #   Module declarations
    ├── table_function.rs      #   semantic_query() -- the main table function (FFI-heavy)
    ├── explain.rs             #   explain_semantic_view() -- shows expanded SQL and EXPLAIN plan
    └── error.rs               #   Query-specific error types

fuzz/                          # Fuzz testing (independent Cargo crate)
├── Cargo.toml                 #   Depends on semantic_views with "arbitrary" feature
├── fuzz_targets/
│   ├── fuzz_json_parse.rs     #   Target 1: arbitrary bytes -> JSON parser
│   ├── fuzz_sql_expand.rs     #   Target 2: arbitrary definitions + names -> expand()
│   └── fuzz_query_names.rs    #   Target 3: fuzzed name arrays against fixed definition
└── corpus/                    #   Seed inputs and fuzzer-discovered inputs (committed to repo)
    └── fuzz_json_parse/       #   Seed JSON files for the JSON parsing target

test/
├── sql/                       # SQL logic tests (run by DuckDB's test runner)
│   ├── semantic_views.test    #   Core semantic_views extension test
│   ├── phase2_ddl.test        #   DDL round-trip tests
│   └── phase4_query.test      #   Query interface tests
└── integration/               # Python integration tests
    └── test_ducklake.py       #   DuckLake/Iceberg integration test

.github/workflows/             # CI pipelines
├── PullRequestCI.yml          #   Fast PR checks (Linux x86_64 only)
├── MainDistributionPipeline.yml  # Full 5-platform build on main/release
├── CodeQuality.yml            #   Formatting, linting, coverage
├── DuckDBVersionMonitor.yml   #   Weekly check for new DuckDB releases
└── NightlyFuzz.yml            #   Daily fuzzing with crash reporting
```

### Data Flow

A semantic query goes through these stages:

```
1. User calls define_semantic_view('orders', '{"base_table":"orders", ...}')
   └── JSON string -> model.rs (SemanticViewDefinition::from_json) -> catalog.rs (HashMap + sidecar file)

2. User calls semantic_query('orders', dimensions := ['region'], metrics := ['revenue'])
   └── catalog.rs (lookup definition) -> expand.rs (generate SQL) -> DuckDB (execute SQL) -> results
```

The key insight: `expand.rs` is pure Rust that converts a `SemanticViewDefinition` plus a `QueryRequest` into a SQL string. DuckDB handles all actual data processing. The generated SQL looks like:

```sql
WITH "_base" AS (
    SELECT *
    FROM "orders"
    JOIN "customers" ON orders.customer_id = customers.id
    WHERE (status = 'active')
)
SELECT
    region AS "region",
    sum(amount) AS "revenue"
FROM "_base"
GROUP BY
    region
```

### Feature Flag Split

The crate has two Cargo feature configurations (think of features as build-time toggles, like Python's `extras_require`):

| Feature | When Used | What It Enables |
|---------|-----------|-----------------|
| `default` (`duckdb/bundled`) | `cargo test`, fuzzing | Compiles DuckDB from source into the binary. Enables `Connection::open_in_memory()` for unit tests. |
| `extension` (`duckdb/loadable-extension`, `duckdb/vscalar`) | `just build`, CI builds | Produces a loadable `.duckdb_extension` file. Uses function-pointer stubs instead of bundled DuckDB. |

This split exists because DuckDB loadable extensions cannot be tested as standalone binaries -- the function-pointer stubs are only initialized when DuckDB loads the extension at runtime. The `bundled` feature sidesteps this for unit tests.

The `ddl/` and `query/` modules are gated behind `#[cfg(feature = "extension")]` -- they are excluded from `cargo test` compilation because they use DuckDB APIs only available in the extension build.

### Sidecar Persistence

DuckDB holds execution locks during scalar function `invoke()`. This means the extension cannot execute SQL from within `define_semantic_view` or `drop_semantic_view` -- it would deadlock. Instead:

1. **During invoke:** The in-memory HashMap is updated, then serialized to a sidecar file (`<db>.semantic_views`) using plain filesystem I/O.
2. **On next load:** `init_catalog()` reads the sidecar and syncs its contents into the `semantic_layer._definitions` DuckDB table.

This bridge pattern ensures definitions persist across restarts without requiring SQL execution during invoke.

## Building

### Debug Build

```bash
just build          # builds build/debug/semantic_views.duckdb_extension
```

### Release Build

```bash
just build-release  # builds build/release/semantic_views.duckdb_extension (optimized, stripped)
```

### How the Build Works

- **`Cargo.toml`** (Rust's equivalent of `pyproject.toml`) defines dependencies, features, and lint configuration.
- **`Makefile`** delegates to `cargo build` with the correct feature flags: `--no-default-features --features extension`. This produces a `cdylib` (C-compatible dynamic library) that DuckDB can load.
- **`crate-type = ["cdylib", "lib"]`** in `Cargo.toml` means two output types:
  - `cdylib`: the `.duckdb_extension` shared library file (for DuckDB to load)
  - `lib`: a regular Rust library (for unit tests and fuzz targets to link against)

The Makefile also handles downloading the correct DuckDB version, running the SQLLogicTest runner, and packaging the extension with metadata.

### Common Build Errors

**Missing submodules:**
```
make[1]: *** No rule to make target 'extension-ci-tools/makefiles/...'. Stop.
```
Fix: `git submodule update --init --recursive`

**Wrong Rust version:**
```
error[E0658]: use of unstable library feature
```
Fix: `rustup update stable`

## Testing

### Test Types

| Command | What It Tests | How It Works |
|---------|---------------|--------------|
| `just test-rust` | Unit tests (model, catalog, expand) | Runs `cargo nextest run` with bundled DuckDB -- fast, no extension loading |
| `just test-sql` | SQL logic tests (DDL + query round-trips) | Builds the extension, loads it in DuckDB, runs `test/sql/*.test` files |
| `just test-iceberg` | DuckLake/Iceberg integration | Builds extension, runs Python test against DuckLake-managed Iceberg tables |
| `just test-all` | All three above | Runs unit, SQL logic, and DuckLake tests sequentially |
| `just coverage` | Coverage report | Runs unit tests with `cargo-llvm-cov`, fails if below 80% line coverage |
| `just lint` | Code quality | `cargo fmt --check` + `cargo clippy` + `cargo deny check` |

### The Critical Difference: `cargo test` vs `just test-sql`

`cargo test` (or `just test-rust`) runs unit tests with a **bundled** DuckDB compiled into the test binary. It exercises `model.rs`, `catalog.rs`, and `expand.rs` -- the pure Rust logic.

`just test-sql` builds the **actual extension binary** and loads it into a real DuckDB process via `LOAD`. This catches:
- ABI mismatches between the Rust code and the DuckDB version
- Registration bugs in the FFI entrypoint
- SQL logic errors in the DDL and query functions

**Always run `just test-sql` before submitting a PR.** A passing `cargo test` does not guarantee the extension loads correctly.

### DuckLake/Iceberg Tests

The DuckLake integration test requires one-time setup:

```bash
just setup-ducklake   # downloads jaffle-shop data, creates DuckLake catalog (idempotent)
just test-iceberg     # runs the integration test
```

This test verifies that `semantic_query` works against DuckLake-managed Iceberg tables with real data (the jaffle-shop dataset).

## Loading the Extension

After building (`just build`), load the extension in a Python DuckDB session:

```python
import duckdb

con = duckdb.connect()
con.install_extension('build/debug/semantic_views.duckdb_extension', force_install=True)
con.load_extension('semantic_views')
```

### Complete Worked Example

```python
import duckdb

con = duckdb.connect()
con.install_extension('build/debug/semantic_views.duckdb_extension', force_install=True)
con.load_extension('semantic_views')

# Create a test table
con.execute("""
    CREATE TABLE orders AS
    SELECT * FROM (VALUES
        ('US', 'completed', 100.0),
        ('US', 'completed', 200.0),
        ('EU', 'completed', 150.0),
        ('EU', 'pending',    50.0)
    ) AS t(region, status, amount)
""")

# Define a semantic view
con.execute("""
    SELECT define_semantic_view('orders', '{
        "base_table": "orders",
        "dimensions": [
            {"name": "region", "expr": "region"},
            {"name": "status", "expr": "status"}
        ],
        "metrics": [
            {"name": "revenue", "expr": "sum(amount)"},
            {"name": "order_count", "expr": "count(*)"}
        ],
        "filters": ["status = ''completed''"]
    }')
""")

# Query it -- the extension generates the GROUP BY and WHERE for you
result = con.execute("""
    FROM semantic_query('orders', dimensions := ['region'], metrics := ['revenue'])
""").fetchall()
print(result)
# [('EU', '150.0'), ('US', '300.0')]

# See what SQL the extension generates
con.execute("FROM explain_semantic_view('orders', dimensions := ['region'], metrics := ['revenue'])").fetchall()

# List all defined views
con.execute("FROM list_semantic_views()").fetchall()

# Remove a view
con.execute("SELECT drop_semantic_view('orders')")
```

For release builds, change the path to `build/release/semantic_views.duckdb_extension`.

## Updating the DuckDB Version Pin

The DuckDB version is pinned in four places:

| File | Field | Example |
|------|-------|---------|
| `Cargo.toml` | `duckdb` and `libduckdb-sys` version | `"=1.4.4"` |
| `Makefile` | `TARGET_DUCKDB_VERSION` | `v1.4.4` |
| `.github/workflows/PullRequestCI.yml` | `duckdb_version` | `v1.4.4` |
| `.github/workflows/MainDistributionPipeline.yml` | `duckdb_version` | `v1.4.4` |

### Steps to Update

1. Update `Cargo.toml`:
   ```toml
   duckdb = { version = "=1.5.0", default-features = false }
   libduckdb-sys = "=1.5.0"
   ```

2. Update `Makefile`:
   ```
   TARGET_DUCKDB_VERSION=v1.5.0
   ```

3. Update both CI workflow files (search for `duckdb_version:`):
   ```yaml
   duckdb_version: v1.5.0
   ```

4. Build and run all tests:
   ```bash
   just build && just test-all
   ```

5. Commit all four files together.

The `DuckDBVersionMonitor` workflow checks for new DuckDB releases weekly and opens a PR automatically if one is found. If the build passes, the PR bumps the version. If it fails, the PR tags `@copilot` to attempt an automated fix.

## Fuzzing

Fuzzing generates random inputs to find crashes -- like Python's `hypothesis` but for binary data. Instead of testing specific examples, the fuzzer mutates inputs guided by code coverage to find edge cases you would never write by hand.

### Setup

```bash
rustup install nightly        # cargo-fuzz needs the nightly Rust compiler
cargo install cargo-fuzz      # the fuzzing CLI tool
```

The nightly toolchain is only used for fuzzing. All other development uses stable.

### Running

```bash
just fuzz                         # run default target (fuzz_json_parse) for 5 minutes
just fuzz target=fuzz_sql_expand  # run a specific target for 5 minutes
just fuzz-all                     # run all three targets sequentially (15 min total)
cargo fuzz list                   # see available targets
```

### The Three Fuzz Targets

| Target | What It Fuzzes | What It Catches |
|--------|---------------|-----------------|
| `fuzz_json_parse` | Feeds arbitrary bytes to `SemanticViewDefinition::from_json()` | Panics in JSON parsing, unexpected serde behavior on malformed input |
| `fuzz_sql_expand` | Generates arbitrary `SemanticViewDefinition` structs + name arrays, feeds to `expand()` | Panics in SQL generation, assertion failures, malformed SQL from edge-case definitions |
| `fuzz_query_names` | Fuzzes dimension/metric name strings against a fixed known-good definition | SQL injection via user-supplied column names, quoting bugs, name resolution panics |

### Corpus Management

The fuzzer saves coverage-increasing inputs to `fuzz/corpus/<target>/`. This corpus is committed to the repo so everyone (and CI) starts from the same base.

```bash
just fuzz-cmin                          # minimize corpus for default target (removes redundant inputs)
just fuzz-cmin target=fuzz_sql_expand   # minimize a specific target's corpus
```

Over time the corpus grows as the fuzzer discovers new code paths. Minimize periodically to keep it small.

### Interpreting Crashes

When the fuzzer finds a crash, it saves the triggering input to `fuzz/artifacts/<target>/`:

```bash
# Reproduce a crash
cargo fuzz run fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-abc123

# See the debug representation of the crash input
cargo fuzz fmt fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-abc123
```

The `fuzz/artifacts/` directory is gitignored -- crash artifacts are debugging data, not part of the corpus.

### Nightly CI

The `NightlyFuzz.yml` workflow runs all three targets daily (5 minutes each). On a crash:

1. The crash artifact is uploaded as a GitHub Actions artifact
2. A GitHub issue is opened with the `bug` and `fuzzing` labels, including reproduction steps

After fuzzing, a separate job checks for new corpus entries and submits a PR via `peter-evans/create-pull-request` if new coverage-increasing inputs were found.

## Publishing to Community Extension Registry

To publish the extension to the [DuckDB Community Extension Registry](https://duckdb.org/community_extensions/development):

### One-Time Setup

1. Fork the [duckdb/community-extensions](https://github.com/duckdb/community-extensions) repository.

2. Create `extensions/semantic_views/description.yml`:

```yaml
extension:
  name: semantic_views
  description: Semantic views -- a declarative layer for dimensions, measures, and relationships
  version: 0.1.0
  language: Rust
  build: cargo
  license: MIT
  excluded_platforms: "wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl"
  requires_toolchains: "rust;python3"
  maintainers:
    - paul-rl

repo:
  github: paul-rl/duckdb-semantic-views
  ref: <commit-sha-of-release>

docs:
  hello_world: |
    SELECT define_semantic_view('demo', '{"base_table":"demo","dimensions":[{"name":"x","expr":"x"}],"metrics":[{"name":"total","expr":"sum(y)"}]}');
    FROM semantic_query('demo', dimensions := ['x'], metrics := ['total']);
  extended_description: |
    Semantic views let you define dimensions, metrics, joins, and filters once,
    then query with any combination. The extension handles GROUP BY, JOIN, and
    filter composition automatically.
```

3. Submit a PR to `duckdb/community-extensions`.

### Field Reference

| Field | Maps To |
|-------|---------|
| `name` | The extension name used in `LOAD semantic_views` |
| `language` / `build` | `Rust` / `cargo` -- tells the registry to use `cargo build` |
| `excluded_platforms` | Platforms we don't support (WASM, musl, mingw) -- matches our CI `exclude_archs` |
| `requires_toolchains` | `rust;python3` -- the registry CI needs these to build |
| `ref` | The Git commit SHA to build from -- update this for each new release |

### Subsequent Releases

For new versions, update the `ref` field in `description.yml` to the new release commit SHA and submit a PR.

## Worked Examples

### Adding a New DDL Function

Suppose you want to add a `rename_semantic_view(old_name, new_name)` function.

**1. Create the implementation file:**

Create `src/ddl/rename.rs`:

```rust
use duckdb::vscalar::{BindInfo, DataChunk, InitInfo, LogicalType, ScalarParams, VScalar};

use crate::catalog::{CatalogState, write_sidecar};

pub struct RenameState {
    pub catalog: CatalogState,
    pub db_path: std::sync::Arc<str>,
}

pub struct RenameSemanticView;

impl VScalar for RenameSemanticView {
    type State = RenameState;

    fn invoke(state: &Self::State, input: &DataChunk, output: &mut dyn ScalarParams) {
        // Read old_name and new_name from input parameters
        // Look up old_name in catalog, insert under new_name, remove old_name
        // Write sidecar to persist the change
        todo!()
    }

    fn parameters() -> Option<Vec<LogicalType>> {
        Some(vec![LogicalType::new_varchar(), LogicalType::new_varchar()])
    }

    fn return_type() -> LogicalType {
        LogicalType::new_varchar()
    }
}
```

**2. Register in `src/ddl/mod.rs`:**

```rust
pub mod define;
pub mod describe;
pub mod drop;
pub mod list;
pub mod rename;   // <-- add this
```

**3. Register in `src/lib.rs` (inside `init_extension`):**

```rust
con.register_scalar_function_with_state::<RenameSemanticView>(
    "rename_semantic_view",
    &RenameState {
        catalog: catalog_state.clone(),
        db_path: db_path.clone(),
    },
)?;
```

Note the `#[cfg(feature = "extension")]` gate on the `ddl` module -- the new function is automatically excluded from `cargo test` compilation.

**4. Add a SQL logic test:**

Create or extend a file in `test/sql/`:

```
statement ok
SELECT define_semantic_view('original', '{"base_table":"t","dimensions":[],"metrics":[]}');

statement ok
SELECT rename_semantic_view('original', 'renamed');

query I
FROM list_semantic_views()
----
renamed
```

**5. Update this architecture section** to include `rename.rs` in the source tree.

### Adding a New Metric Type

Suppose you want to add a `window` metric type that generates a window function instead of an aggregate.

**1. Update `src/model.rs`:**

Add an optional field to the `Metric` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Metric {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub source_table: Option<String>,
    #[serde(default)]
    pub metric_type: Option<String>,  // "aggregate" (default) or "window"
}
```

Because `metric_type` uses `#[serde(default)]` and `Option<String>`, existing JSON definitions without the field will deserialize correctly with `None` (treated as aggregate).

**2. Update `src/expand.rs`:**

In the `expand()` function, modify the SELECT item generation to handle window metrics differently:

```rust
for met in &resolved_mets {
    let item = if met.metric_type.as_deref() == Some("window") {
        format!("    {} OVER () AS {}", met.expr, quote_ident(&met.name))
    } else {
        format!("    {} AS {}", met.expr, quote_ident(&met.name))
    };
    select_items.push(item);
}
```

**3. Add unit tests in `src/expand.rs`:**

```rust
#[test]
fn test_window_metric_type() {
    let def = SemanticViewDefinition {
        base_table: "orders".to_string(),
        dimensions: vec![Dimension { name: "region".to_string(), expr: "region".to_string(), source_table: None }],
        metrics: vec![Metric {
            name: "running_total".to_string(),
            expr: "sum(amount)".to_string(),
            source_table: None,
            metric_type: Some("window".to_string()),
        }],
        filters: vec![],
        joins: vec![],
    };
    let req = QueryRequest {
        dimensions: vec!["region".to_string()],
        metrics: vec!["running_total".to_string()],
    };
    let sql = expand("orders", &def, &req).unwrap();
    assert!(sql.contains("sum(amount) OVER () AS \"running_total\""));
}
```

**4. Add a proptest property** (in the existing proptest section of `expand.rs`) to verify that `expand()` never panics with arbitrary metric_type values.

**5. Update fuzz seed corpus:** If the JSON schema changes (new fields), add a seed file to `fuzz/corpus/fuzz_json_parse/` with the new field so the fuzzer starts from a valid example.

## Troubleshooting

### "Symbol not found" or ABI mismatch when LOADing

```
Error: Catalog Error: Failed to load extension: Symbol not found
```

**Why:** The extension was built against a different DuckDB version than the one loading it. DuckDB's ABI is not stable across minor versions, so the extension binary must match exactly.

**Fix:** Ensure the DuckDB Python package version matches the pin in `Cargo.toml`:

```bash
pip show duckdb           # check installed version
grep 'duckdb.*version' Cargo.toml  # check pinned version
```

If they differ, either update the pin (see [Updating the DuckDB Version Pin](#updating-the-duckdb-version-pin)) or install the matching Python DuckDB version:

```bash
pip install duckdb==1.4.4
```

### `cargo test` passes but `just test-sql` fails

**Why:** `cargo test` uses the `bundled` feature, which compiles DuckDB from source into the test binary. This bypasses the extension loading mechanism entirely. The `ddl/` and `query/` modules are not even compiled during `cargo test`. So a passing `cargo test` only validates the pure Rust logic in `model.rs`, `catalog.rs`, and `expand.rs`.

`just test-sql` builds the actual extension (with `--features extension`) and loads it in a real DuckDB process. This catches FFI registration bugs, ABI mismatches, and SQL-level errors.

**Fix:** Always treat `just test-sql` as the authoritative test. If it fails but `cargo test` passes, the bug is likely in the DDL/query modules or the FFI entrypoint.

### Clippy pedantic lint failures

```
error: this function could have a #[must_use] attribute
```

**Why:** The project enables `clippy::pedantic` at deny level. Pedantic lints catch real issues but some are noisy.

**Fix:** If the lint is legitimate, fix it. If it is a false positive, add a targeted `#[allow(...)]` attribute:

```rust
#[allow(clippy::needless_pass_by_value)]  // required by FFI signature
fn my_function(val: String) { ... }
```

The priority = -1 pattern in `Cargo.toml` ensures individual lint `#[allow]` directives override the blanket `pedantic` deny. Three lints are globally allowed: `module_name_repetitions`, `missing_errors_doc`, and `missing_panics_doc`.

### Linker errors with "loadable-extension"

```
error: linking with `cc` failed
undefined reference to `duckdb_rs_extension_api_init`
```

**Why:** You are trying to compile test code with the `extension` feature. The `loadable-extension` feature replaces all DuckDB C API calls with function-pointer stubs that are only initialized when DuckDB loads the extension at runtime. A standalone binary (like a test) cannot use these stubs.

**Fix:** Use `cargo test` (default features, which enables `bundled`) for unit tests. Only the Makefile build (`just build`) should use `--features extension`.

### Fuzz target won't compile

```
error: `-Zsanitizer=address` is not a valid flag
```

**Why:** `cargo-fuzz` requires the nightly Rust toolchain for sanitizer support.

**Fix:**

```bash
rustup show          # check which toolchain is active
rustup install nightly
cargo fuzz run fuzz_json_parse  # cargo-fuzz automatically uses +nightly
```

Do **not** set a directory-level nightly override (`rustup override set nightly`) -- this would break the stable-toolchain builds for everything else.

## CI Workflows

| Workflow | Trigger | What It Does |
|----------|---------|--------------|
| **PullRequestCI** | Pull requests to `main` | Fast feedback: builds and tests on Linux x86_64 only. Uses the DuckDB extension CI tools reusable workflow. |
| **MainDistributionPipeline** | Push to `main` or `release/*` | Full build across 5 platforms: Linux x86_64, Linux arm64, macOS x86_64, macOS arm64, Windows x86_64. Excluded: WASM, musl, mingw variants. |
| **CodeQuality** | Push to `main`/`release/*` and PRs | Runs `cargo fmt --check`, `cargo clippy`, `cargo-deny` (license/advisory audit), and coverage check (80% minimum line coverage via `cargo-llvm-cov`). |
| **DuckDBVersionMonitor** | Weekly (Monday 09:00 UTC) | Queries the DuckDB GitHub API for the latest release. If newer than the current pin, updates all four version locations, builds, and tests. Opens a version-bump PR on success or a breakage PR (tagging `@copilot`) on failure. |
| **NightlyFuzz** | Daily (03:00 UTC) | Runs all three fuzz targets for 5 minutes each (15 minutes total). Uploads crash artifacts and opens a GitHub issue on any crash. After fuzzing, checks for new corpus entries and submits a PR if found. |

All workflows can be triggered manually via `workflow_dispatch` for debugging.
