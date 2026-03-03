# Phase 14: DuckLake Integration Test Refresh and CI Job - Research

**Researched:** 2026-03-02
**Domain:** Python integration testing, GitHub Actions CI, DuckLake extension
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Test file structure
- Two separate test files: `test/integration/test_ducklake.py` (local, uses real jaffle-shop data) and `test/integration/test_ducklake_ci.py` (CI, generates synthetic data inline)
- Shared helpers module (`test/integration/test_ducklake_helpers.py`) for extension loading and DuckLake attach boilerplate — DRY where it makes sense
- Each test file handles its own data setup and assertions independently

#### Synthetic data (CI test)
- Inline generation in the test script — no downloads, no fixture files committed to repo
- Mirror real jaffle-shop schema: same columns as `raw_orders` (id, customer, ordered_at, store_id, subtotal, tax_paid, order_total) with 5-10 synthetic rows of known values
- Use known values so time dimension assertions can verify specific expected outputs

#### v0.2.0 API update
- Both test files use v0.2.0 scalar function DDL: `create_semantic_view()`, `drop_semantic_view()`, `semantic_view()`
- Do NOT use native `CREATE SEMANTIC VIEW` DDL
- `explain_semantic_view()` stays (already v0.2.0 compatible)

#### Test coverage expansion (both files)
- Keep the original 4 test cases (define view, query with dimension, global aggregate, explain)
- Add typed output assertions: verify a BIGINT metric (e.g., `count(*)`) returns a BIGINT column, not VARCHAR
- Add time dimension test: declare `ordered_at` as a time-typed dimension with day granularity, assert truncated values match expected dates

#### CI job placement
- Second job in `.github/workflows/PullRequestCI.yml` parallel with `linux-fast-check` (no `needs:` dependency)
- Standard job (not reusable workflow): ubuntu-latest, install Rust, `cargo build --features extension`, install Python/uv, run `uv run test/integration/test_ducklake_ci.py`
- Required check — blocks PR merge on failure

#### DuckLake extension installation
- `INSTALL ducklake` from community registry (no version pin)
- `allow_unsigned_extensions=true` in DuckDB config
- Project-local extension directory (e.g., `test/data/duckdb_extensions/`)

#### DuckDB Version Monitor
- Add step to `DuckDBVersionMonitor.yml` that runs DuckLake CI test after existing build+test step
- `continue-on-error: true` — DuckLake failure surfaces in PR body but doesn't block version-bump PR
- Include DuckLake result (pass/fail) in PR description

### Claude's Discretion
- Exact Justfile targets for running the CI test locally
- Whether to add `uv` via `pip install uv` or a dedicated action
- How to pass the built extension path to the Python test (env var, hardcoded path, etc.)
- Whether `test_ducklake.py` (local) needs the Justfile target updated or just the file contents

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

## Summary

Phase 14 updates the existing DuckLake integration test from v0.1.0 API (`define_semantic_view`, `semantic_query`) to the v0.2.0 API (`create_semantic_view`, `semantic_view`, `explain_semantic_view`), adds a CI-safe variant using inline synthetic data, wires a parallel CI job into PullRequestCI.yml, and adds DuckLake compatibility checking to the DuckDB version monitor.

The current test file (`test/integration/test_ducklake.py`) is 100% broken against the v0.2.0 codebase: it calls `define_semantic_view()` (removed), `semantic_query()` (renamed to `semantic_view()`), and passes raw JSON instead of the STRUCT/LIST arguments that `create_semantic_view()` requires. All 4 test cases will fail against the current build.

The v0.2.0 API has a 6-argument STRUCT/LIST signature. The `create_semantic_view` function takes: name (VARCHAR), tables (LIST of STRUCT), relationships (LIST of STRUCT), dimensions (LIST of STRUCT), time_dimensions (LIST of STRUCT), metrics (LIST of STRUCT). The test files need complete rewrites of the `define_semantic_view` calls plus renaming `semantic_query` → `semantic_view`.

**Primary recommendation:** Write the CI test first (it's the net-new deliverable), extract shared boilerplate into the helpers module, then update the local test to match — sharing helper functions to stay DRY.

## Standard Stack

### Core
| Component | Version | Purpose | Why Standard |
|-----------|---------|---------|--------------|
| Python (uv PEP 723) | >=3.9 | Test runner | Established pattern in project — `uv run` for inline dependencies |
| duckdb Python package | latest matching extension | DuckDB connection for test | Already used in `test_ducklake.py` |
| GitHub Actions ubuntu-latest | ubuntu-24.04 | CI runner | Used by existing `linux-fast-check` job |
| dtolnay/rust-toolchain@stable | stable | Rust build in CI | Already used in DuckDBVersionMonitor.yml |
| actions/checkout@v4 | v4 | Repo checkout | Project standard |

### Supporting
| Component | Version | Purpose | When to Use |
|-----------|---------|---------|-------------|
| uv | via `pip install uv` | PEP 723 script runner | CI env — no `uv` pre-installed on ubuntu-latest |
| peter-evans/create-pull-request@v7 | v7 | PR creation in version monitor | Already used in DuckDBVersionMonitor.yml |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `pip install uv` | dedicated uv action | `pip install uv` is simpler, fewer deps |
| extension_directory in test/data/ | `~/.duckdb` | project-local is deterministic, no home dir pollution |

## Architecture Patterns

### v0.2.0 API Call Pattern

The `create_semantic_view` 6-arg STRUCT/LIST signature (from `src/ddl/parse_args.rs` and `test/sql/phase4_query.test`):

```python
con.execute("""
    SELECT create_semantic_view(
        'view_name',
        [{'alias': 'o', 'table': 'jaffle.raw_orders'}],
        [],
        [{'name': 'store_id', 'expr': 'store_id', 'source_table': 'o'}],
        [{'name': 'ordered_at', 'expr': 'ordered_at', 'granularity': 'day'}],
        [{'name': 'order_count', 'expr': 'count(*)', 'source_table': 'o'},
         {'name': 'total_revenue', 'expr': 'sum(order_total)', 'source_table': 'o'}]
    )
""")
```

Query with `semantic_view` (renamed from `semantic_query`):
```python
result = con.execute("""
    SELECT * FROM semantic_view(
        'view_name',
        dimensions := ['store_id'],
        metrics := ['order_count']
    )
""").fetchall()
```

Cleanup:
```python
con.execute("SELECT drop_semantic_view('view_name')")
```

### Synthetic Data Pattern for CI

```python
import tempfile, os

def setup_ducklake_ci(con, ext_dir: str):
    """Create a DuckLake catalog in a temp directory with synthetic jaffle-shop data."""
    tmpdir = tempfile.mkdtemp()
    ducklake_file = os.path.join(tmpdir, "test.ducklake")
    data_dir = os.path.join(tmpdir, "data") + "/"
    os.makedirs(data_dir, exist_ok=True)

    # Create DuckLake catalog (in-memory catalog db, file-based ducklake metadata)
    catalog_path = os.path.join(tmpdir, "catalog.duckdb")
    catalog_con = duckdb.connect(catalog_path, config={
        "allow_unsigned_extensions": "true",
        "extension_directory": ext_dir,
    })
    catalog_con.execute("INSTALL ducklake")
    catalog_con.execute("LOAD ducklake")
    catalog_con.execute(f"ATTACH 'ducklake:{ducklake_file}' AS jaffle (DATA_PATH '{data_dir}')")

    # Create table with known values
    catalog_con.execute("""
        CREATE TABLE jaffle.raw_orders (
            id INTEGER, customer VARCHAR, ordered_at DATE,
            store_id INTEGER, subtotal INTEGER, tax_paid INTEGER, order_total INTEGER
        )
    """)
    catalog_con.execute("""
        INSERT INTO jaffle.raw_orders VALUES
            (1, 'alice', '2024-01-15', 1, 1000, 80, 1080),
            (2, 'bob',   '2024-01-15', 2, 2000, 160, 2160),
            (3, 'alice', '2024-02-10', 1, 500, 40, 540),
            (4, 'charlie','2024-02-10', 2, 1500, 120, 1620),
            (5, 'bob',   '2024-03-05', 1, 800, 64, 864)
    """)
    catalog_con.close()
    return catalog_path, ducklake_file, data_dir
```

### Shared Helpers Module Pattern

`test/integration/test_ducklake_helpers.py` should expose:
- `get_ext_dir()` — returns project-local extension directory path, creating it if needed
- `load_extension(con, extension_path)` — INSTALL + LOAD semantic_views
- `attach_ducklake(con, ducklake_file, data_dir, alias='jaffle')` — ATTACH DuckLake catalog
- `teardown(con, view_names)` — drop views + close connection

### Typed Output Assertions

After Phase 12/13, `count(*)` returns BIGINT (not VARCHAR). Assert column type:
```python
# duckdb Python returns int for BIGINT columns
result = con.execute("""
    SELECT * FROM semantic_view('view', metrics := ['order_count'])
""").fetchall()
assert isinstance(result[0][0], int), f"Expected int (BIGINT), got {type(result[0][0])}"
```

For DATE time dimensions:
```python
import datetime
result = con.execute("""
    SELECT * FROM semantic_view('view', dimensions := ['ordered_at'], metrics := ['order_count'])
""").fetchall()
# date_trunc('day', date) returns a date
for row in result:
    assert isinstance(row[0], datetime.date), f"Expected date, got {type(row[0])}"
# Known values: rows on 2024-01-15 and 2024-02-10 and 2024-03-05
dates = sorted({row[0] for row in result})
assert datetime.date(2024, 1, 15) in dates
assert datetime.date(2024, 2, 10) in dates
```

### CI Job Structure

New parallel job in `PullRequestCI.yml`:
```yaml
ducklake-ci-check:
  name: DuckLake integration test (Linux x86_64)
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Install Rust stable
      uses: dtolnay/rust-toolchain@stable
    - name: Build extension
      run: cargo build --features extension
    - name: Install Python and uv
      run: |
        python3 -m pip install --upgrade pip
        pip install uv
    - name: Run DuckLake CI integration test
      run: uv run test/integration/test_ducklake_ci.py
      env:
        SEMANTIC_VIEWS_EXTENSION_PATH: target/debug/semantic_views.duckdb_extension
```

### Extension Path: Debug vs Release

The local test hardcodes `build/debug/semantic_views.duckdb_extension` (CMake build output path). The CI uses `cargo build --features extension` which outputs to `target/debug/semantic_views.duckdb_extension`. The CI test should read from `SEMANTIC_VIEWS_EXTENSION_PATH` env var with a fallback:

```python
import os
from pathlib import Path
PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent
EXTENSION_PATH = Path(os.environ.get(
    "SEMANTIC_VIEWS_EXTENSION_PATH",
    PROJECT_ROOT / "build" / "debug" / "semantic_views.duckdb_extension"
))
```

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DuckDB typing check | Custom type introspection | Python isinstance() on duckdb fetchall() result | DuckDB Python maps DuckDB types to Python types automatically |
| Temp dir management | Manual cleanup | `tempfile.mkdtemp()` + finally block | handles edge cases, CI cleanup is automatic |
| uv dependency management | requirements.txt | PEP 723 `# dependencies = [...]` inline | Consistent with existing test_ducklake.py pattern |

## Common Pitfalls

### Pitfall 1: DuckDB BIGINT returned as int, DATE as datetime.date
**What goes wrong:** Test asserts `result[0][0] == "5"` expecting VARCHAR, but BIGINT count returns Python `int`.
**Why it happens:** Phase 12/13 changed output from all-VARCHAR to typed. Old test assertions on string equality fail silently (they were wrong even before).
**How to avoid:** Assert `isinstance(result[0][0], int)` for BIGINT, `isinstance(row[0], datetime.date)` for DATE.

### Pitfall 2: Extension path mismatch (CMake vs Cargo)
**What goes wrong:** CI uses `cargo build` → output in `target/debug/`, but local test hardcodes `build/debug/` (CMake path).
**Why it happens:** Two build systems, different output directories.
**How to avoid:** Use `SEMANTIC_VIEWS_EXTENSION_PATH` env var with fallback. CI job sets the env var explicitly.

### Pitfall 3: DuckLake community extension may fail if network access blocked
**What goes wrong:** `INSTALL ducklake` tries to download from community registry; some CI environments have network restrictions.
**Why it happens:** DuckLake is not bundled with DuckDB.
**How to avoid:** `INSTALL ducklake` on ubuntu-latest GitHub Actions works — GitHub Actions VMs have unrestricted outbound network access (confirmed by DuckDB CI tooling pattern).

### Pitfall 4: Old API calls break silently
**What goes wrong:** `define_semantic_view` or `semantic_query` calls raise `CatalogException: Function 'define_semantic_view' not found`. Test catches generic Exception, marks as FAIL, continues.
**Why it happens:** Test has `except Exception as e: print(f"FAIL: {e}"); failed += 1` — doesn't abort on first failure.
**How to avoid:** Both tests must use ONLY v0.2.0 API. Check for any remaining old calls.

### Pitfall 5: `continue-on-error` in version monitor must be on DuckLake step only
**What goes wrong:** Setting `continue-on-error: true` on build step hides real build failures.
**Why it happens:** Copy-paste error.
**How to avoid:** Only the DuckLake step gets `continue-on-error: true`. Build step fails the job if it fails.

### Pitfall 6: DuckLake catalog needs DATA_PATH trailing slash
**What goes wrong:** `ATTACH 'ducklake:...' AS ... (DATA_PATH '/path/to/data')` without trailing slash may fail.
**Why it happens:** DuckLake DATA_PATH convention — directory path needs trailing slash.
**How to avoid:** Always append `/` to DATA_PATH. Current test already does this: `str(JAFFLE_DATA_DIR) + "/"`.

### Pitfall 7: Synthetic DuckLake catalog state isolation
**What goes wrong:** CI test runs in same process, catalog from one test bleeds into another test run.
**Why it happens:** DuckDB connection shares in-memory state.
**How to avoid:** Create a fresh temp directory for each test run (e.g., `tempfile.mkdtemp()` at test start), call `con.close()` in finally block.

## Code Examples

### Current v0.1.0 → v0.2.0 Migration Map

| v0.1.0 call | v0.2.0 replacement |
|-------------|-------------------|
| `SELECT define_semantic_view('name', '{"base_table":..., "dimensions":[...], "metrics":[...]}')` | `SELECT create_semantic_view('name', tables, [], dims, [], metrics)` |
| `SELECT * FROM semantic_query('name', dimensions := [...], metrics := [...])` | `SELECT * FROM semantic_view('name', dimensions := [...], metrics := [...])` |
| `SELECT drop_semantic_view('name')` | `SELECT drop_semantic_view('name')` (unchanged) |
| `SELECT * FROM explain_semantic_view(...)` | `SELECT * FROM explain_semantic_view(...)` (unchanged) |

### Version Monitor DuckLake Step

```yaml
- name: Run DuckLake integration test
  if: steps.latest.outputs.is_new == 'true'
  id: ducklake_test
  run: |
    python3 -m pip install uv
    SEMANTIC_VIEWS_EXTENSION_PATH=target/release/semantic_views.duckdb_extension \
      uv run test/integration/test_ducklake_ci.py
  continue-on-error: true

- name: Open version-bump PR (build passed)
  if: steps.latest.outputs.is_new == 'true' && steps.build.outcome == 'success'
  uses: peter-evans/create-pull-request@v7
  with:
    body: |
      Automated version bump to DuckDB ${{ steps.latest.outputs.latest }}.

      Build and tests passed on Linux x86_64.
      DuckLake compatibility: ${{ steps.ducklake_test.outcome == 'success' && '✓ passed' || '✗ failed' }}
      ...
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `define_semantic_view(name, json_string)` | `create_semantic_view(name, tables, rels, dims, time_dims, metrics)` | Phase 11.1 (2026-03-02) | Breaks test_ducklake.py entirely |
| `semantic_query(name, ...)` | `semantic_view(name, ...)` | Phase 11.1 (2026-03-02) | Breaks test_ducklake.py entirely |
| All-VARCHAR output | Typed output (BIGINT, DATE, DECIMAL) | Phase 12 (2026-03-02) | String equality assertions in old test are wrong for BIGINT |
| Sidecar `.semantic_views` file | `semantic_layer._definitions` DuckDB table | Phase 10 (2026-03-01) | CI test must use file-backed DB (not in-memory) for persistence |

## Open Questions

1. **Release vs debug build in version monitor**
   - What we know: `DuckDBVersionMonitor.yml` runs `make configure && make test_release` (release build)
   - What's unclear: Release build puts the extension in `target/release/` or `build/release/`? The Makefile uses CMake — output is `build/release/semantic_views.duckdb_extension`
   - Recommendation: For version monitor, use `build/release/semantic_views.duckdb_extension` (not `target/`). For PR CI ducklake job, use `cargo build --features extension` → `target/debug/`.

2. **Justfile target for local CI test**
   - What we know: `just test-ducklake` runs `test_ducklake.py`. No target for CI variant.
   - Recommendation: Add `test-ducklake-ci` target pointing to `test_ducklake_ci.py`.

## Sources

### Primary (HIGH confidence)
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/src/lib.rs` — confirmed registered function names: `create_semantic_view`, `semantic_view`, `explain_semantic_view`, `drop_semantic_view`
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/src/ddl/parse_args.rs` — confirmed 6-arg STRUCT/LIST signature
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/test/sql/phase4_query.test` — confirmed v0.2.0 syntax working examples
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/test/integration/test_ducklake.py` — current v0.1.0 test to update
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/.github/workflows/PullRequestCI.yml` — CI structure for parallel job
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/.github/workflows/DuckDBVersionMonitor.yml` — version monitor for DuckLake step

### Secondary (MEDIUM confidence)
- DuckLake ATTACH syntax from existing test: `ATTACH 'ducklake:{file}' AS {alias} (DATA_PATH '{dir}/')` — confirmed working in test_ducklake.py

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — confirmed from existing project files
- Architecture: HIGH — v0.2.0 API confirmed from lib.rs + parse_args.rs + phase4_query.test
- Pitfalls: HIGH for API migration pitfalls (confirmed from codebase), MEDIUM for GitHub Actions network access

**Research date:** 2026-03-02
**Valid until:** 2026-04-02 (stable project codebase, low churn expected)
