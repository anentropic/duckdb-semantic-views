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
- [Multi-Version Branching Strategy](#multi-version-branching-strategy)
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
git clone --recurse-submodules https://github.com/anentropic/duckdb-semantic-views.git
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
├── lib.rs                     # Extension entrypoint -- registers functions and parser hook with DuckDB
├── model.rs                   # Data types: SemanticViewDefinition, Dimension, Metric, Fact, etc.
│                              #   Handles validation and semantic model representation
├── catalog.rs                 # In-memory catalog (HashMap) + pragma_query_t persistence
│                              #   Manages the semantic_layer._definitions table
├── expand.rs                  # SQL generation engine -- turns definitions + query requests into SQL
│                              #   Pure Rust, no DuckDB dependency at runtime
├── body_parser.rs             # DDL body parser -- state machine for TABLES/RELATIONSHIPS/FACTS/
│                              #   HIERARCHIES/DIMENSIONS/METRICS clauses
├── ddl_kind.rs                # DdlKind enum -- all DDL statement variants (CREATE, DROP, ALTER, etc.)
├── parser_trampoline.rs       # Rust FFI trampoline for parser hook -- detects and parses DDL statements
├── ddl/                       # DDL execution (only compiled for the extension build)
│   ├── mod.rs                 #   Module declarations
│   ├── create.rs              #   CREATE SEMANTIC VIEW execution
│   ├── drop.rs                #   DROP SEMANTIC VIEW execution
│   ├── alter.rs               #   ALTER SEMANTIC VIEW execution
│   ├── show.rs                #   SHOW SEMANTIC VIEWS / DIMENSIONS / METRICS / FACTS
│   └── describe.rs            #   DESCRIBE SEMANTIC VIEW
└── query/                     # Query interface (only compiled for the extension build)
    ├── mod.rs                 #   Module declarations
    ├── table_function.rs      #   semantic_view() -- the main table function (FFI-heavy)
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
├── BuildQuick.yml             #   Fast PR checks (Linux x86_64 only)
├── BuildAll.yml               #   Full 5-platform build on main
├── CodeQuality.yml            #   Formatting, linting, coverage
├── DuckDBVersionMonitor.yml   #   Weekly check for new DuckDB releases
└── Fuzz.yml                   #   Fuzzing on push to main, with crash reporting
```

### Data Flow

A semantic query goes through these stages:

```
1. User runs CREATE SEMANTIC VIEW shop AS TABLES (...) DIMENSIONS (...) METRICS (...)
   └── Parser hook (C++ shim) -> Rust trampoline -> body_parser.rs -> catalog.rs (persist via pragma_query_t)

2. User calls semantic_view('shop', dimensions := ['region'], metrics := ['revenue'])
   └── catalog.rs (lookup definition) -> expand.rs (generate SQL) -> DuckDB (execute SQL) -> results
```

The key insight: `expand.rs` is pure Rust that converts a `SemanticViewDefinition` plus a `QueryRequest` into a SQL string. DuckDB handles all actual data processing. The generated SQL looks like:

```sql
SELECT
    "o"."region" AS "region",
    SUM("o"."amount") AS "revenue"
FROM "orders" AS "o"
LEFT JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
GROUP BY
    "o"."region"
```

### Feature Flag Split

The crate has two Cargo feature configurations (think of features as build-time toggles, like Python's `extras_require`):

| Feature | When Used | What It Enables |
|---------|-----------|-----------------|
| `default` (`duckdb/bundled`) | `cargo test`, fuzzing | Compiles DuckDB from source into the binary. Enables `Connection::open_in_memory()` for unit tests. |
| `extension` (`duckdb/loadable-extension`, `duckdb/vscalar`) | `just build`, CI builds | Produces a loadable `.duckdb_extension` file. Uses function-pointer stubs instead of bundled DuckDB. |

This split exists because DuckDB loadable extensions cannot be tested as standalone binaries -- the function-pointer stubs are only initialized when DuckDB loads the extension at runtime. The `bundled` feature sidesteps this for unit tests.

The `ddl/` and `query/` modules are gated behind `#[cfg(feature = "extension")]` -- they are excluded from `cargo test` compilation because they use DuckDB APIs only available in the extension build.

### Catalog Persistence

DDL statements (CREATE, DROP, ALTER SEMANTIC VIEW) are detected by the parser hook and routed through a dedicated DDL connection (`persist_conn`) that avoids lock conflicts with the main execution connection. The catalog uses `pragma_query_t` for persistence:

1. **During DDL:** The parser hook detects the statement, the Rust trampoline parses it, and the DDL handler executes via a separate connection to avoid deadlock.
2. **Persistence:** Definitions are stored in the `semantic_layer._definitions` table via `pragma_query_t` (write-first pattern).
3. **On load:** `init_catalog()` reads the definitions table and populates the in-memory HashMap.

The sidecar file approach was eliminated in v0.2.0 in favor of transactional catalog persistence.

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

This test verifies that `semantic_view()` works against DuckLake-managed Iceberg tables with real data (the jaffle-shop dataset).

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

# Create sample data
con.execute("""
CREATE TABLE orders (
    id INTEGER, region VARCHAR, status VARCHAR, amount DECIMAL(10,2)
);
INSERT INTO orders VALUES
    (1, 'US', 'completed', 100.00),
    (2, 'US', 'completed', 200.00),
    (3, 'EU', 'completed', 150.00),
    (4, 'EU', 'pending',    50.00);
""")

# Define a semantic view (native DDL)
con.execute("""
CREATE SEMANTIC VIEW shop AS
  TABLES (o AS orders PRIMARY KEY (id))
  DIMENSIONS (
    o.region AS o.region,
    o.status AS o.status
  )
  METRICS (
    o.revenue     AS SUM(o.amount),
    o.order_count AS COUNT(*)
  );
""")

# Query with any dimension/metric combination
result = con.execute("""
    SELECT * FROM semantic_view('shop',
        dimensions := ['region'],
        metrics := ['revenue']
    ) ORDER BY region
""").fetchall()
print(result)
# [('EU', Decimal('200.00')), ('US', Decimal('300.00'))]

# See the generated SQL
con.execute("""
    SELECT * FROM explain_semantic_view('shop',
        dimensions := ['region'],
        metrics := ['revenue']
    )
""").fetchall()

# List all views
con.execute("SHOW SEMANTIC VIEWS").fetchall()

# Describe a view
con.execute("DESCRIBE SEMANTIC VIEW shop").fetchall()

# Remove a view
con.execute("DROP SEMANTIC VIEW shop")
```

For release builds, change the path to `build/release/semantic_views.duckdb_extension`.

## Updating the DuckDB Version Pin

The DuckDB version is pinned in a single source of truth: **`.duckdb-version`** (repo root). All other locations are derived from it.

| File | How It Gets the Version |
|------|------------------------|
| `.duckdb-version` | **Source** — single `vX.Y.Z` line |
| `Makefile` | Reads `.duckdb-version` via `$(shell cat .duckdb-version)` |
| `Cargo.toml` | `duckdb` and `libduckdb-sys` `"=X.Y.Z"` — updated by monitor workflow |
| `test/**/*.py`, `configure/*.py` | PEP 723 `"duckdb==X.Y.Z"` — updated by monitor workflow |
| `.github/workflows/BuildAll.yml` | `uses:` tag, `duckdb_version`, `ci_tools_version` — updated by monitor workflow |
| `.github/workflows/BuildQuick.yml` | `uses:` tag, `duckdb_version`, `ci_tools_version` — updated by monitor workflow |

### Steps to Update Manually

Replace `X.Y.Z` below with the target DuckDB version (e.g., `1.5.1`). The duckdb-rs crate version encodes the DuckDB version as `1.1XY0Z.0` (e.g., DuckDB 1.5.1 = crate 1.10501.0).

1. Update `.duckdb-version`:
   ```
   vX.Y.Z
   ```

2. Update `Cargo.toml` (using the encoded crate version):
   ```toml
   duckdb = { version = "=1.1XY0Z.0", default-features = false }
   libduckdb-sys = "=1.1XY0Z.0"
   ```

3. Update Python PEP 723 headers (all `# dependencies = ["duckdb==..."]` lines):
   ```bash
   find . -name '*.py' -exec grep -l 'duckdb==' {} \; | xargs sed -i '' 's/duckdb==[^"]*/duckdb==X.Y.Z/g'
   ```

4. Update both CI workflow files (`BuildAll.yml` and `BuildQuick.yml`) — the `uses:` tag, `duckdb_version`, and `ci_tools_version`:
   ```yaml
   uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@vX.Y.Z
   ...
     duckdb_version: vX.Y.Z
     ci_tools_version: vX.Y.Z
   ```

5. Download the new amalgamation, build, and run all tests:
   ```bash
   just update-headers && just build && just test-all
   ```

6. Commit all files together.

The `DuckDBVersionMonitor` workflow automates all of this: it checks for new DuckDB releases weekly, updates `.duckdb-version` and all derived locations (including CI workflow files), then opens a PR. If the build passes, the PR bumps the version. If it fails, the PR tags `@copilot` to attempt an automated fix.

### Version Monitor PAT

The version monitor workflow needs a fine-grained Personal Access Token (PAT) stored as a repository secret named `VERSION_MONITOR_PAT`. This is required because GitHub's default `GITHUB_TOKEN` lacks the `workflow` scope, so it cannot push changes to files under `.github/workflows/`.

**Creating the PAT:**

1. Go to [github.com/settings/personal-access-tokens](https://github.com/settings/personal-access-tokens) (Settings > Developer settings > Fine-grained tokens)
2. Click "Generate new token"
3. Set a descriptive name (e.g., `duckdb-semantic-views version monitor`)
4. Set expiration (recommend 1 year — add a calendar reminder to rotate)
5. Under **Repository access**, select "Only select repositories" and choose `anentropic/duckdb-semantic-views`
6. Under **Permissions**, grant:
   - **Contents:** Read and write
   - **Pull requests:** Read and write
   - **Workflows:** Read and write
7. Click "Generate token" and copy the value

**Saving as a repository secret:**

1. Go to the repo Settings > Secrets and variables > Actions
2. Click "New repository secret"
3. Name: `VERSION_MONITOR_PAT`
4. Value: paste the token
5. Click "Add secret"

If the PAT expires or is revoked, the version monitor will fall back to `GITHUB_TOKEN` and skip workflow file updates (logging a warning). The PR body will note the manual step required.

### Bumping DuckDB on the LTS Branch

The `duckdb/1.4.x` branch follows the same process but targets LTS releases:

1. Check out the LTS branch: `git checkout duckdb/1.4.x`
2. Update `.duckdb-version` to the new LTS version (e.g., `v1.4.5`)
3. Update `Cargo.toml`: `duckdb = { version = "=1.4.5", ... }` and `libduckdb-sys = "=1.4.5"`
4. Update Python PEP 723 headers and CI workflow files (same sed/search as main)
5. Run `just build && just test-all`
6. The Cargo.toml `version` field on LTS uses build metadata: `0.5.4+duckdb1.4`

The DuckDB Version Monitor has separate jobs for latest (`check-latest`) and LTS (`check-lts`)
that automate this process on their respective branches.

## Multi-Version Branching Strategy

The extension supports two DuckDB version tracks via separate branches:

| Branch | DuckDB Version | Purpose | Version Format |
|--------|---------------|---------|----------------|
| `main` | Latest (currently 1.5.x) | Primary development, CE registry `ref` | `0.5.4` |
| `duckdb/1.4.x` | 1.4.x (Andium LTS) | LTS compatibility | `0.5.4+duckdb1.4` |

### Development Workflow

1. **New features**: Develop on milestone branches (e.g., `milestone/v0.5.4`), squash-merge to `main`
2. **Cherry-pick to LTS**: After main is stable, cherry-pick relevant commits to `duckdb/1.4.x`
3. **Version bumps**: Each branch tracks its own DuckDB version in `.duckdb-version`

### Syncing Changes Between Branches

```bash
# Cherry-pick a commit from main to LTS
git checkout duckdb/1.4.x
git cherry-pick <commit-sha>
# Resolve any DuckDB API differences (e.g., parser_extension_compat.hpp)
just test-all
```

### CI Coverage

Both branches run the full Build.yml pipeline on push. The DuckDB Version Monitor
checks for new releases of both the latest and LTS version lines (weekly, Monday 09:00 UTC).

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
just fuzz fuzz_sql_expand         # run a specific target for 5 minutes
just fuzz fuzz_sql_expand 10      # run a specific target for 10 seconds
just fuzz-all                     # run all three targets sequentially (15 min total)
just fuzz-all 60                  # run all three targets for 60 seconds each
cargo +nightly fuzz list          # see available targets
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
just fuzz-cmin fuzz_sql_expand          # minimize a specific target's corpus
```

Over time the corpus grows as the fuzzer discovers new code paths. Minimize periodically to keep it small.

### Interpreting Crashes

When the fuzzer finds a crash, it saves the triggering input to `fuzz/artifacts/<target>/`:

```bash
# Reproduce a crash
cargo +nightly fuzz run fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-abc123

# See the debug representation of the crash input
cargo +nightly fuzz fmt fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-abc123
```

The `fuzz/artifacts/` directory is gitignored -- crash artifacts are debugging data, not part of the corpus.

### CI Fuzzing

The `Fuzz.yml` workflow runs all three targets on push to `main` (10 minutes each). Crash detection works by checking for artifact files (not the fuzzer exit code), so build failures or timeouts do not trigger false positives.

On a real crash:

1. The crash artifact is uploaded as a GitHub Actions artifact
2. A GitHub issue is opened with the `bug` and `fuzzing` labels
3. The job fails (red status)

**Reproducing a CI crash locally:**

```bash
# 1. Go to the Actions run linked in the issue
# 2. Scroll to the bottom of the run page and download the crash artifact zip
# 3. Unzip it into the fuzz artifacts directory:
mkdir -p fuzz/artifacts/fuzz_json_parse
unzip crash-fuzz_json_parse-*.zip -d fuzz/artifacts/fuzz_json_parse/

# 4. Reproduce the crash:
cargo +nightly fuzz run fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-*

# 5. Inspect the crash input:
cargo +nightly fuzz fmt fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-*
```

You can also download artifacts via the `gh` CLI:

```bash
# List artifacts from a specific run
gh run view <run-id> --repo anentropic/duckdb-semantic-views

# Download the crash artifact
gh run download <run-id> --repo anentropic/duckdb-semantic-views --name crash-fuzz_json_parse-<sha>
```

## Publishing to Community Extension Registry

The extension is published to the [DuckDB Community Extension Registry](https://duckdb.org/community_extensions/development).

### description.yml

The registry descriptor lives at `description.yml` in the repo root. It specifies the extension
metadata, build configuration, and hello_world example that appears on the CE page.

Key fields:
| Field | Value | Notes |
|-------|-------|-------|
| `name` | `semantic_views` | Must match the LOAD name |
| `language` / `build` | `Rust` / `cargo` | CE pipeline runs `cargo build` |
| `license` | `MIT` | Must match LICENSE file |
| `excluded_platforms` | semicolon-separated | Platforms we cannot support (WASM, musl, mingw, etc.) |
| `requires_toolchains` | `rust;python3` | Build-time dependencies for CE CI |
| `ref` | 40-char commit SHA | Points to main branch commit to build from |

### Submitting a New Release

The `just release` recipe automates steps 3-7 below:

1. Complete the milestone on the milestone branch
2. Squash-merge to `main` and tag the release (e.g., `v0.5.4`)
3. Run `just release`

This will:
- Verify you're on `main` with a clean working tree and `gh` CLI installed
- Extract the version from `Cargo.toml` and the current commit SHA
- Update `description.yml` with the new `ref` and `version`, and commit
- Copy `description.yml` to the CE fork (default: `~/Documents/Dev/Sources/community-extensions`, override with `CE_REPO` env var)
- Check out the `semantic-views` branch, commit, push, and open a PR to `duckdb/community-extensions`

**Prerequisites:**
- A fork of [duckdb/community-extensions](https://github.com/duckdb/community-extensions) cloned locally
- The `gh` CLI authenticated with permissions to create PRs

**After running:**
- Wait for the CE build pipeline to pass (builds across all non-excluded platforms)
- After merge, the extension is installable via:
  ```sql
  INSTALL semantic_views FROM community;
  LOAD semantic_views;
  ```

### Adding LTS Support

To publish builds for DuckDB 1.4.x (Andium LTS), add the `andium` field to `description.yml`:
```yaml
repo:
  github: anentropic/duckdb-semantic-views
  ref: <main-branch-sha>
  andium: <duckdb-1.4.x-branch-sha>
```
The CE pipeline will build from `andium` SHA against DuckDB 1.4.x.

## Worked Examples

### Adding a New DDL Statement

Suppose you want to add an `ALTER SEMANTIC VIEW old_name RENAME TO new_name` statement.

Since v0.5.2, DDL is handled via parser hooks -- the C++ shim detects `CREATE/DROP/ALTER/SHOW/DESCRIBE SEMANTIC VIEW` prefixes and routes them through the Rust FFI trampoline. New DDL statements follow the same pattern.

**1. Add the DDL kind:**

In `src/ddl_kind.rs`, extend the `DdlKind` enum:

```rust
pub enum DdlKind {
    Create { or_replace: bool, if_not_exists: bool },
    Drop { if_exists: bool },
    Describe,
    Show,
    AlterRename { if_exists: bool, new_name: String },  // <-- add this
}
```

**2. Update the parser trampoline:**

In `src/parser_trampoline.rs`, add detection for the `ALTER SEMANTIC VIEW` prefix and parse the `RENAME TO` clause.

**3. Add a SQL logic test:**

Create or extend a file in `test/sql/`:

```
statement ok
CREATE SEMANTIC VIEW original AS
  TABLES (t AS my_table PRIMARY KEY (id))
  DIMENSIONS (t.x AS t.x)
  METRICS (t.total AS SUM(t.y));

statement ok
ALTER SEMANTIC VIEW original RENAME TO renamed

query I
SHOW SEMANTIC VIEWS
----
renamed

statement ok
DROP SEMANTIC VIEW renamed
```

**4. Update this architecture section** to document the new DDL kind.

### Adding a New Metric Type

Suppose you want to add a `window` metric type that generates a window function instead of an aggregate.

**1. Update the body parser:**

In `src/body_parser.rs`, extend the METRICS clause parsing to accept a `WINDOW` keyword modifier:

```sql
METRICS (
    o.running_total WINDOW AS SUM(o.amount),
    o.revenue AS SUM(o.amount)
)
```

**2. Update `src/expand.rs`:**

In the `expand()` function, modify the SELECT item generation to handle window metrics differently:

```rust
for met in &resolved_mets {
    let item = if met.is_window {
        format!("    {} OVER () AS {}", met.expr, quote_ident(&met.name))
    } else {
        format!("    {} AS {}", met.expr, quote_ident(&met.name))
    };
    select_items.push(item);
}
```

**3. Add unit tests:**

```rust
#[test]
fn test_window_metric_type() {
    // Parse a DDL body with WINDOW metric modifier
    let body = r#"
        TABLES (o AS orders PRIMARY KEY (id))
        DIMENSIONS (o.region AS o.region)
        METRICS (o.running_total WINDOW AS SUM(o.amount))
    "#;
    let def = parse_body(body).unwrap();
    assert!(def.metrics[0].is_window);
}
```

**4. Add a proptest property** to verify that `expand()` never panics with arbitrary metric configurations.

**5. Update fuzz targets:** Add a seed file to `fuzz/corpus/fuzz_ddl_parse/` with the new WINDOW modifier so the fuzzer starts from a valid example.

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
cargo +nightly fuzz run fuzz_json_parse  # must use +nightly for sanitizer flags
```

Do **not** set a directory-level nightly override (`rustup override set nightly`) -- this would break the stable-toolchain builds for everything else.

## CI Workflows

| Workflow | Trigger | What It Does |
|----------|---------|--------------|
| **BuildQuick** | Push to non-main branches | Fast feedback: builds and tests on Linux x86_64 only. Uses the DuckDB extension CI tools reusable workflow. |
| **BuildAll** | Push to `main` | Full build across 5 platforms: Linux x86_64, Linux arm64, macOS x86_64, macOS arm64, Windows x86_64. Excluded: WASM, musl, mingw variants. |
| **CodeQuality** | Push to `main`/`release/*` and PRs | Runs `cargo fmt --check`, `cargo clippy`, `cargo-deny` (license/advisory audit), and coverage check (80% minimum line coverage via `cargo-llvm-cov`). |
| **DuckDBVersionMonitor** | Weekly (Monday 09:00 UTC) | Queries the DuckDB GitHub API for the latest release. If newer than the current pin, updates all four version locations, builds, and tests. Opens a version-bump PR on success or a breakage PR (tagging `@copilot`) on failure. |
| **Fuzz** | Push to `main` | Runs all three fuzz targets for 10 minutes each. Detects crashes by checking for artifact files (not exit codes). Uploads crash artifacts, opens a GitHub issue, and fails the job on any crash. |

All workflows can be triggered manually via `workflow_dispatch` for debugging.
