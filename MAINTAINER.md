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
├── lib.rs                     # Extension entrypoint — registers table functions + the parser_override hook
├── model.rs                   # Core data types: SemanticViewDefinition, Dimension, Metric, Fact, Relationship…
├── errors.rs                  # Typed error surface (ParseError + optional caret) for the CREATE/parse boundary
├── ident.rs                   # Identifier grammar: quoting, case-folding, qualified-name splitting
├── expr_tokens.rs             # Quote/literal-aware tokenizer for stored SQL expressions (reference find/inline)
├── sql_lit.rs                 # SqlLit newtype — makes "forgot to escape a string literal" a compile error
├── util.rs                    # Shared lexical helpers (is_ident_byte, blank_sql_comments, dollar-tag grammar)
├── ffi_util.rs                # FFI seam helpers: buffer handoff, UTF-8-safe error truncation
├── render_ddl.rs              # SemanticViewDefinition → CREATE SEMANTIC VIEW text (GET_DDL)
├── render_yaml.rs             # SemanticViewDefinition → YAML
│
├── body_parser/               # Tokenizer + clause-body parser for the CREATE body (pure, always compiled)
│   ├── lexer.rs cursor.rs scan.rs clause_bounds.rs   #   token layer, cursor, clause bounds
│   ├── tables.rs relationships.rs metrics.rs entries.rs
│   ├── annotations.rs window.rs materializations.rs
│   └── mod.rs
├── parse/                     # Statement-level DDL orchestration + parser_override FFI (write side)
│   ├── ffi.rs                 #   FFI entry points: sv_parser_override_rust / sv_parse_function_rust
│   ├── detect.rs              #   DDL-prefix detection
│   ├── rewrite.rs             #   rewrite_to_native_sql: recognised DDL → native SQL (or error)
│   ├── create_body.rs         #   CREATE front door (validate_and_rewrite)
│   ├── native_sql.rs          #   INSERT/UPDATE/DELETE emission on _definitions
│   ├── show_clauses.rs        #   SHOW … clause parsing
│   └── mod.rs
├── graph/                     # Relationship graph: cardinality, join tree, toposort, derived-metric DAG
│   ├── relationship.rs cardinality.rs join_tree.rs toposort.rs
│   ├── derived_metrics.rs facts.rs using.rs names.rs
│   └── mod.rs
├── expand/                    # Query expansion: definition + QueryRequest → SQL (pure, always compiled)
│   ├── mod.rs resolution.rs join_resolver.rs sql_gen.rs select_spec.rs types.rs
│   ├── facts.rs fan_trap.rs semi_additive.rs window.rs wildcard.rs role_playing.rs materialization.rs
│   └── tests_*.rs             #   behaviour-named extracted test modules
├── catalog/                   # Reads/writes of semantic_layer._definitions
│   ├── mod.rs                 #   CatalogReader (fresh-per-call connection) + RAII PreparedStmt/QueryResult guards
│   └── writes.rs              #   write-side race guards
├── ddl/                       # DDL execution + read-side table functions (only compiled under --features extension)
│   ├── define.rs              #   CREATE-time enrichment (PK lookup, type inference)
│   ├── describe.rs get_ddl.rs list.rs
│   ├── show_columns.rs show_entities.rs show_dims_for_metric.rs show_materializations.rs
│   ├── read_ffi.rs read_yaml.rs alter_helpers_ffi.rs   #   FFI seam types (BorrowedConnection, dispatchers)
│   └── mod.rs
└── query/                     # Query interface
    ├── table_function.rs      #   semantic_view() — main table function (FFI-heavy, extension-only)
    ├── explain.rs             #   explain_semantic_view() — expanded SQL + EXPLAIN plan (extension-only)
    ├── wire.rs                #   Pure wire-format/SQL-shape helpers (always compiled + unit-tested)
    ├── error.rs               #   Query-specific error types (extension-only)
    └── mod.rs

fuzz/                          # Fuzz testing (independent Cargo crate; depends on semantic_views + "arbitrary")
├── fuzz_targets/              #   Eight targets — see the Fuzzing section for what each covers
│   ├── fuzz_json_parse.rs fuzz_yaml_parse.rs fuzz_ddl_parse.rs fuzz_keyword_body.rs
│   └── fuzz_sql_expand.rs fuzz_query_names.rs fuzz_render_roundtrip.rs fuzz_parser_override_ffi.rs
├── seeds/                     #   Committed seed inputs (per target)
└── corpus/                    #   Fuzzer-discovered inputs (gitignored)

test/
├── sql/                       # sqllogictest files (run via test/sql/TEST_LIST — drift is a CI error)
│   └── *.test                 #   e.g. phase4_query.test, phase29_facts.test, phase47_semi_additive.test
└── integration/               # Python integration suites (test_ducklake_ci.py, test_differential.py, …)

.github/workflows/             # CI pipelines — see the CI Workflows section for triggers
├── BuildQuick.yml BuildAll.yml            #   extension build + sqllogictest (branch / main)
├── CodeQuality.yml IntegrationChecks.yml  #   lint+coverage / DuckLake+Python integration
├── DocsCheck.yml Docs.yml                 #   docs build check (branches) / build+deploy (main)
└── Fuzz.yml DuckDBVersionMonitor.yml PublishExtension.yml
```

### Data Flow

Every recognised DDL form is rewritten by `parser_override` into native SQL that DuckDB then plans and executes on the caller's connection. There is no longer a separate `parse_function` / `sv_ddl_internal` table-function fallback (deleted in the v0.8.0 architectural unification (Phase 59) — see CHANGELOG).

```
DDL (CREATE / DROP / ALTER / DESCRIBE / SHOW / GET_DDL / READ_YAML / FROM YAML FILE)
   └── C++ parser_override hook (cpp/src/shim.cpp::sv_parser_override)
   └── Rust FFI trampoline (src/parse/ffi.rs::sv_parser_override_rust)
   └── rewrite_to_native_sql (src/parse/rewrite.rs)
         ├── validate_and_rewrite (canonical body parser → table-function-call SQL or sentinel)
         ├── rewrite_create / rewrite_yaml_file_create  (writes: INSERT/UPDATE/DELETE on _definitions)
         ├── drop / alter rewrite                        (writes: with race-guard SELECT prefix)
         └── (read-side pass-through: SELECT * FROM <read_side_fn>(...))
   └── publish_owned_sql → C++ Parser::ParseQuery (DEFAULT_OVERRIDE so the hook does not recurse)
   └── DuckDB executes the resulting SQLStatement(s) on the caller's connection.

semantic_view('shop', dimensions := [...], metrics := [...])
   └── Standard table function. Bind reads the definition via catalog::CatalogReader,
       expand/ generates SQL, DuckDB executes it.
```

`expand/` is pure Rust that converts a `SemanticViewDefinition` plus a `QueryRequest` into a SQL string. DuckDB handles all actual data processing. The generated SQL looks like:

```sql
SELECT
    "o"."region" AS "region",
    SUM("o"."amount") AS "revenue"
FROM "orders" AS "o"
LEFT JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
GROUP BY
    "o"."region"
```

#### Two FALLBACK_OVERRIDE quirks worth knowing

1. **DuckDB silently drops `DISPLAY_EXTENSION_ERROR` from `parser_override` in FALLBACK mode** (`ParseInternal` in the v1.5.2 amalgamation). The success path is unaffected — `parser_override` rewrites recognised DDL into native SQL on the caller's connection. For *validation* errors (e.g. `semantic view 'X' does not exist`, unknown clause), `parser_override` instead returns `DISPLAY_ORIGINAL_ERROR`; the default parser then fails on the unrecognised DDL prefix; DuckDB calls our registered `parse_function`, which re-runs validation and returns `DISPLAY_EXTENSION_ERROR` with `error_location` set to the offending byte offset. `ParserException::SyntaxError` formats the caret automatically. See `sv_parse_function_rust` in `src/parse/ffi.rs`. (TECH-DEBT 22 was resolved by this mechanism in Phase 62; the older `sql_throwing` / synthesised-`SELECT error('...')` workaround was deleted.)
2. **`CALL disable_peg_parser()` resets `allow_parser_override_extension` to `default`,** which silently bypasses our hook. After toggling PEG you must re-issue `SET allow_parser_override_extension='FALLBACK'`. The extension installs `FALLBACK` on load, so a process that never enables PEG is unaffected. See TECH-DEBT.md item 21.

### Feature Flag Split

The crate has two Cargo feature configurations (think of features as build-time toggles, like Python's `extras_require`):

| Feature | When Used | What It Enables |
|---------|-----------|-----------------|
| `default` (`duckdb/bundled`) | `cargo test`, fuzzing | Compiles DuckDB from source into the binary. Enables `Connection::open_in_memory()` for unit tests. |
| `extension` (`duckdb/loadable-extension`, `duckdb/vscalar`) | `just build`, CI builds | Produces a loadable `.duckdb_extension` file. Uses function-pointer stubs instead of bundled DuckDB. |

This split exists because DuckDB loadable extensions cannot be tested as standalone binaries -- the function-pointer stubs are only initialized when DuckDB loads the extension at runtime. The `bundled` feature sidesteps this for unit tests.

The `ddl/` and `query/` modules are gated behind `#[cfg(feature = "extension")]` -- they are excluded from `cargo test` compilation because they use DuckDB APIs only available in the extension build.

### Catalog Persistence

Definitions live in `semantic_layer._definitions` (a regular DuckDB table that participates in normal transactional semantics). Two separate connections coexist per extension load:

- **Caller's connection.** Where DDL writes execute. `parser_override` produces `INSERT / UPDATE / DELETE ... RETURNING ...` SQL, DuckDB plans it on this connection, and the writes participate in whatever transaction the caller has open.
- **Catalog connection (`catalog_conn`).** Created at extension load time and held for read-side table functions (`describe_*`, `show_*`, `list_*`, `read_yaml_*`, `get_ddl`) and CREATE-time enrichment (PK lookup, type inference). Reads see committed state — never the caller's in-flight transaction.

The split exists because read-side table function bind hooks do not (currently) expose the executing connection through libduckdb-sys, so DESCRIBE / SHOW use `catalog_conn` even when called from inside an open transaction. Documented limitation; see TECH-DEBT item 19.

The in-memory `CatalogState` HashMap mirror was removed in v0.8.0 (Phase 58); Phase 61 added internal `PreparedStmt` / `QueryResult` RAII guards in `catalog` (`src/catalog/mod.rs`) so error paths no longer juggle manual `duckdb_destroy_*` calls.

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
| `just test-ducklake-ci` | DuckLake integration (synthetic data) | Builds extension, runs the Python DuckLake CI test — the leg included in `test-all` |
| `just test-ducklake` | DuckLake integration (real jaffle-shop data) | Builds extension, runs the Python test against DuckLake tables; requires `just setup-ducklake` first |
| `just test-integration` | Python integration suites | Runs the full `test/integration/*.py` suite list (caret, ADBC, multi-db, concurrency, differential, …) |
| `just test-all` | Rust + SQL logic + DuckLake CI + integration | Runs `test-rust`, `test-sql`, `test-ducklake-ci`, and `test-integration` sequentially |
| `just coverage` | Coverage report | Runs unit tests with `cargo-llvm-cov`, fails if below 80% line coverage |
| `just lint` | Code quality (authoritative) | `cargo fmt --check` + full default-features `cargo clippy` + `cargo deny check`. The clippy step compiles the ~25 MB bundled DuckDB, so a cold run is ~10 min. |
| `just lint-fast` | Fast lint (pre-commit) | `cargo fmt --check` + the extension-feature clippy with `SV_SKIP_CPP_BUILD` (no C++ build). What the pre-commit hook runs; lints the same production code in ~1 min cold, seconds warm. |

### The Critical Difference: `cargo test` vs `just test-sql`

`cargo test` (or `just test-rust`) runs unit tests with a **bundled** DuckDB compiled into the test binary. It exercises `model.rs`, `catalog/`, and `expand/` -- the pure Rust logic.

`just test-sql` builds the **actual extension binary** and loads it into a real DuckDB process via `LOAD`. This catches:
- ABI mismatches between the Rust code and the DuckDB version
- Registration bugs in the FFI entrypoint
- SQL logic errors in the DDL and query functions

**Always run `just test-sql` before submitting a PR.** A passing `cargo test` does not guarantee the extension loads correctly.

### How to read the 80% coverage gate (CI-6)

The `Coverage check (80% minimum)` job in `CodeQuality.yml` runs `cargo llvm-cov nextest` with the **default (bundled) feature set only**. Two consequences worth keeping in mind so the number isn't misread:

- **The `extension`-gated FFI code is excluded from the denominator.** Modules compiled only under `--features extension` (the `#[no_mangle] extern "C"` bind callbacks in `src/query/{table_function,explain}.rs`, `src/ddl/*_ffi` entrypoints, etc.) are not built during the coverage run, so they neither raise nor lower the percentage. The line coverage figure describes the *pure Rust core*, not the FFI seam. Pure logic that the FFI callbacks delegate to lives in always-compiled modules (e.g. `src/query/wire.rs`) specifically so it *is* covered; behaviour that only exists across the FFI boundary is exercised by `just test-sql` and the Python integration suites instead.
- **`nextest` does not run doc tests.** `cargo test` runs them; `cargo llvm-cov nextest` does not. Doc-test-only examples therefore contribute nothing to the coverage number even though they run (and can fail) under `just test-rust` / `cargo test`.

Net: treat the 80% figure as a floor on the bundled core's line coverage, not as end-to-end coverage of the shipped extension.

### Linting the extension-gated FFI layer

`cargo clippy` (and the `Clippy (pedantic lints, deny warnings)` CI step) compiles the **default** feature set, so it never lints the `#[cfg(feature = "extension")]` FFI modules. A second CI step, `Clippy (extension feature, deny warnings)`, closes that gap:

```bash
SV_SKIP_CPP_BUILD=1 cargo clippy --no-default-features --features extension -- -D warnings
```

`SV_SKIP_CPP_BUILD` makes `build.rs` skip the ~25 MB DuckDB amalgamation + C++ shim compile. Clippy only type-checks (it never links the final `cdylib`), so the C++ half is irrelevant and the check needs no amalgamation download — it runs in seconds. Use the same command locally before touching any FFI (`src/query/{table_function,explain}.rs`, `src/ddl/*_ffi.rs`, `src/parse/ffi.rs`) code. Do **not** set `SV_SKIP_CPP_BUILD` for a real `just build` — the extension won't link without the C++ shim.

**This is also what the pre-commit hook runs** (`just lint-fast` / `.cargo-husky/hooks/pre-commit`). The default-features `cargo clippy -- -D warnings` compiles the bundled DuckDB amalgamation — a ~10 min COLD build (its cargo profile differs from `cargo test`'s, so a green `cargo test` does not warm it, and tight disk evicts it between runs), which used to stall every `git commit`. The extension-feature clippy lints the same production code with no C++ build, because every `#[cfg(not(feature = "extension"))]` item in `src/` is either inside a `#[cfg(test)]` module (clippy without `--tests` skips it) or the `test_helpers` module (which carries `#[allow(clippy::pedantic, ...)]`). CI's default-features `Clippy (pedantic lints, deny warnings)` step remains the authoritative full-coverage gate.

### DuckLake/Iceberg Tests

The DuckLake integration test requires one-time setup:

```bash
just setup-ducklake   # downloads jaffle-shop data, creates DuckLake catalog (idempotent)
just test-ducklake    # runs the integration test against the real jaffle-shop data
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

Both branches run the full build pipeline on push (`BuildAll` on `main`, `BuildQuick` on
other branches), plus `CodeQuality` and `IntegrationChecks`. The DuckDB Version Monitor
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
just fuzz-all                     # run all eight targets sequentially (5 min each, 40 min total)
just fuzz-all 60                  # run all eight targets for 60 seconds each
cargo +nightly fuzz list          # see available targets
```

### The Eight Fuzz Targets

| Target | What It Fuzzes | What It Catches |
|--------|---------------|-----------------|
| `fuzz_json_parse` | Feeds arbitrary bytes to `SemanticViewDefinition::from_json()` | Panics in JSON parsing, unexpected serde behavior on malformed input |
| `fuzz_yaml_parse` | Feeds arbitrary bytes to the YAML import path | Panics / serde surprises in YAML deserialization |
| `fuzz_ddl_parse` | Arbitrary bytes → `plan_rewrite` (the full CREATE/DDL front door) | Panics or hangs in DDL parsing on malformed statements |
| `fuzz_keyword_body` | Arbitrary bytes → `parse_keyword_body` (bypasses prefix detection) | Panics in the clause-body parser; asserts anything parsed also renders |
| `fuzz_render_roundtrip` | Generated definitions → normalize once via `parse(render(def))` → assert `render` is idempotent on the parser-produced def | Grammar drift between `render_ddl` and the body parser (dropped field, reordered clause, mis-quoted identifier). Uses the converge-once invariant, not the strong `render(parse(render(def))) == render(def)` fixpoint — that is unsatisfiable on arbitrary defs (a free-form `expr`'s surrounding whitespace is trimmed by the parser and cannot be quote-protected) |
| `fuzz_sql_expand` | Arbitrary `SemanticViewDefinition` + name arrays → `expand()` | Panics/assertion failures in SQL generation; quote/paren imbalance in the emitted SQL |
| `fuzz_query_names` | Fuzzes dimension/metric name strings against a fixed known-good definition | SQL injection via user-supplied column names, quoting bugs, name resolution panics |
| `fuzz_parser_override_ffi` | Drives the `parser_override` FFI entry path with fuzzed input | Panics crossing the FFI boundary; unexpected rc / error propagation |

> **Note:** most targets accumulate a coverage corpus under `fuzz/corpus/<target>/` (gitignored) seeded from `fuzz/seeds/<target>/` (committed). Both directories are passed to libFuzzer — `cargo fuzz run <target> fuzz/corpus/<target> fuzz/seeds/<target> -- …` in `Fuzz.yml` and the `just fuzz` / `just fuzz-all` recipes — so committed seed files ARE used as starting inputs. `Fuzz.yml` creates the (gitignored) dirs before running; the older "corpus/seed wiring is a CI gap" note is resolved (CI-1, #135).

> **Fuzz oracle design (TECH-DEBT #33):** the two struct-domain targets (`fuzz_render_roundtrip`, `fuzz_sql_expand`) and `fuzz_query_names` use hand-rolled *structural* oracles (converge-once render idempotence; balanced quotes/parens) rather than executing SQL, so they stay fast and DuckDB-free. Their preconditions must cover every fragment interpolated verbatim into the output — the recurring bug class was an *incomplete* precondition, not a wrong approach. The heavier "does DuckDB accept/return the right rows for the expanded SQL" oracle lives in the proptests, which execute against in-memory DuckDB: `tests/differential_proptest.rs` covers the single-table aggregation path, `tests/star_schema_proptest.rs` covers the two-table join / fan-trap fence (a `ManyToOne` star — a parent-table metric must be rejected, and every accepted query must match a hand-written `LEFT JOIN` oracle; the regression guard for the EXP-1/2/3 fence fixes), `tests/multi_hop_join_proptest.rs` extends that to a three-table `ManyToOne` chain (`t → u → w`) — a metric on the parent *or* grandparent must be rejected, and every accepted root-grain query must match a hand-written chained `LEFT JOIN` oracle (selecting the grandparent dimension without the parent forces the resolver to pull in the intermediate table), `tests/semi_additive_proptest.rs` covers the semi-additive (`NON ADDITIVE BY`) snapshot path against an independent `MAX`/`MIN` + `IS NOT DISTINCT FROM` oracle (randomized `ASC`/`DESC`, duplicate timestamps and NULLs), and `tests/window_metric_proptest.rs` covers the window-metric partition path against an independent correlated-subquery oracle (randomized `PARTITION BY EXCLUDING` vs explicit `PARTITION BY`, `SUM`/`COUNT`/`MIN`/`MAX`, NULL partition keys). The exact `parse(render(def)) == def` round-trip lives in `tests/roundtrip_proptest.rs`. A full trust-boundary redesign was considered and declined — see TECH-DEBT #33.

### Corpus Management

The fuzzer saves coverage-increasing inputs to `fuzz/corpus/<target>/`, which is **gitignored** (`.gitignore`: "grows locally, bootstraps from fuzz/seeds/ on fresh clone") — it is regenerated locally and in CI, not committed. The shared base everyone (and CI) starts from is the committed `fuzz/seeds/<target>/`. To add a durable repro or a known-tricky input the whole team should start from, commit it under `fuzz/seeds/<target>/`, **not** the corpus.

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

The `Fuzz.yml` workflow runs all eight targets (10 minutes each) on any push that touches `src/**`, `fuzz/**`, or the Cargo manifests (a path-filtered trigger, so documentation-only pushes skip it). Crash detection works by checking for artifact files (not the fuzzer exit code), so build failures or timeouts do not trigger false positives.

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

In the `parse` module (the `DdlKind` enum lives in `src/parse/`), extend it:

```rust
pub enum DdlKind {
    Create { or_replace: bool, if_not_exists: bool },
    Drop { if_exists: bool },
    Describe,
    Show,
    AlterRename { if_exists: bool, new_name: String },  // <-- add this
}
```

**2. Update the parse layer:**

Add prefix detection in `src/parse/detect.rs` and the rewrite-to-native-SQL handling in `src/parse/rewrite.rs` for the `ALTER SEMANTIC VIEW … RENAME TO` form.

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

Suppose you want to add a `window` metric type that generates a window function instead of an aggregate. (Window metrics already exist — see `WindowSpec` in `src/model.rs` and `src/expand/window.rs`; this walks the general shape of such a change.)

**1. Update the body parser:**

In `src/body_parser/metrics.rs`, extend the METRICS clause parsing to accept the window modifier:

```sql
METRICS (
    o.running_total WINDOW AS SUM(o.amount),
    o.revenue AS SUM(o.amount)
)
```

**2. Update the expansion layer (`src/expand/`):**

In the SELECT-item generation (`src/expand/sql_gen.rs`), handle window metrics differently:

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

**Why:** `cargo test` uses the `bundled` feature, which compiles DuckDB from source into the test binary. This bypasses the extension loading mechanism entirely. The `ddl/` and `query/` modules are not even compiled during `cargo test`. So a passing `cargo test` only validates the pure Rust logic in `model.rs`, `catalog/`, and `expand/`.

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
| **BuildQuick** | Pull requests (skips doc-only changes) | Fast feedback: extension build + full sqllogictest suite on Linux x86_64 only, via the DuckDB extension-ci-tools reusable workflow. No `push` trigger (runs on PRs + manual dispatch) — `main` gets the full platform matrix from BuildAll. |
| **BuildAll** | Push to `main` (skips doc-only changes) | Full build across 5 platforms: Linux x86_64/arm64, macOS x86_64/arm64, Windows x86_64. Runs sqllogictest on each built platform except `linux_arm64`. Excludes WASM, musl, mingw variants. |
| **CodeQuality** | Push to `main` + pull requests (skips doc-only changes) | `TEST_LIST` sync check; `cargo fmt --check`; clippy (default **and** `--features extension`); doctests (default + the FFI `compile_fail` ABI guard); extension-feature unit tests; `cargo-deny` (license/advisory audit); 80%-line coverage floor via `cargo-llvm-cov`. |
| **IntegrationChecks** | Push to `main` + pull requests (skips doc-only changes) | DuckLake CI integration test **and** the full Python integration suite (`just test-integration`), each building the debug extension. |
| **DocsCheck** | Pull requests | Sphinx docs build with `-W` (warnings as errors). Deliberately **not** path-filtered, so documentation/text-only changes are still validated when the heavier workflows skip. No `push` trigger (runs on PRs + manual dispatch) — `main` gets the build+deploy from Docs. |
| **Docs** | Push to `main` | Same `-W` Sphinx build, then deploys the site to GitHub Pages. |
| **Fuzz** | Push touching `src/**`, `fuzz/**`, or the Cargo manifests | Runs all eight fuzz targets for 10 minutes each. Detects crashes via artifact files (not exit codes), uploads them, opens a `bug`/`fuzzing` issue, and fails the job on any crash. |
| **DuckDBVersionMonitor** | Weekly (Monday 09:00 UTC) + manual | Queries the DuckDB GitHub API for the latest / LTS release. If newer than the pin, updates all derived version locations, builds, and tests, then opens a version-bump PR on success or a breakage PR (tagging `@copilot`) on failure. |
| **PublishExtension** | Manual (`workflow_dispatch`) only | Release automation for the Community Extension registry. |

Most workflows also accept a manual `workflow_dispatch` trigger for debugging.

**PR vs push triggers (CI-2).** The PR-validating workflows run on `pull_request` — **every** PR, same-repo **and** fork — so PRs from forks / non-collaborators, which never fire this repo's `push` events, get full CI (they previously got none). `CodeQuality` and `IntegrationChecks` also run on `push` to `main` to validate the merged state; because that `push` is `main`-only, a same-repo PR's branch push does **not** trigger them, so each PR runs exactly **once** via `pull_request` — no fork-detection guard or double-run logic. `BuildQuick` and `DocsCheck` have no `push` trigger (they run on `pull_request` plus manual `workflow_dispatch`), because `main` is already covered by `BuildAll` (full matrix) and `Docs` (build + deploy). `Fuzz` stays `push`-triggered: it needs `issues: write` to file crash issues (which fork PRs cannot grant) and fuzzing untrusted fork code is best avoided, so it runs on same-repo branch pushes and `main`, never on fork PRs. Trade-off: a branch pushed with **no open PR** gets no CI — the check runs the moment a PR exists (and re-runs on every new commit via `pull_request: synchronize`).

**Documentation-only skip.** `BuildAll`, `BuildQuick`, `CodeQuality`, and `IntegrationChecks` carry a shared `paths-ignore` (`**/*.md`, `_notes/**`, `docs/**`, `LICENSE`), so a change that touches only documentation/text does not run the extension build, sqllogictest, Rust lint/coverage, or the integration suites. `Fuzz` achieves the same via its `paths` allowlist. `DocsCheck` is intentionally exempt so prose changes still get the `-W` docs build. A change that also touches any non-doc file runs everything as normal. When editing these triggers, keep the four `paths-ignore` lists in sync.

> **Gotcha:** `push` path filters are evaluated against the commits a push *introduces* (the `before..after` range), not the net diff. This now matters only for **Fuzz** (the one PR-relevant workflow still triggered on branch pushes): a branch **force-pushed on top of already-merged history** can trigger it even for a doc-only net change, because the push range then includes earlier `src/**` commits (e.g. a squash-merge). The `pull_request`-triggered workflows (`BuildQuick`, `CodeQuality`, `IntegrationChecks`, `DocsCheck`) evaluate path filters against the PR's net diff, so they are unaffected.
