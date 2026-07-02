# DuckDB Semantic Views — developer task runner
# Run `just` to see available commands

# Show available commands (default)
default:
    @just --list

# Set up complete local dev environment (one-time, new contributors)
# Downloads pinned DuckDB binary, installs dev tools, wires cargo-husky hooks
setup:
    @echo "Installing dev tools..."
    cargo install cargo-nextest --locked
    cargo install cargo-deny --locked
    cargo install cargo-llvm-cov --locked
    cargo install cargo-fuzz --locked
    cargo install cargo-sweep --locked
    git submodule update --init --recursive
    make configure
    @echo "Running cargo test to install cargo-husky hooks..."
    cargo test
    @echo "Setup complete. Run 'just build' to build the extension."

# Build debug extension
build:
    make debug

# Build release extension
build-release:
    make release

# Run tests via SQLLogicTest (exercises actual LOAD mechanism)
test:
    make test_debug

# Run Rust unit tests only (does NOT test LOAD — use 'just test' for that)
test-rust:
    cargo nextest run

# Run all lints
lint:
    cargo fmt --check
    cargo clippy -- -D warnings
    cargo deny check

# Format code in place
fmt:
    cargo fmt

# Check code coverage (requires cargo-llvm-cov)
coverage:
    cargo llvm-cov nextest --fail-under-lines 80

# Ensure the Python venv's duckdb pip package matches .duckdb-version.
# Called automatically by test recipes that use the venv runner.
[private]
_ensure-test-deps:
    @VER=$(cat .duckdb-version | sed 's/^v//'); \
    INSTALLED=$(configure/venv/bin/python -c "import duckdb; print(duckdb.__version__)" 2>/dev/null || echo ""); \
    if [ "$INSTALLED" = "$VER" ]; then \
      exit 0; \
    fi; \
    echo "duckdb pip version mismatch (have=${INSTALLED:-none}, want=$VER), reinstalling..."; \
    configure/venv/bin/pip install -q "duckdb==$VER"

# Run SQL logic tests for Phase 2 DDL via the SQLLogicTest runner.
#
# There is no standalone DuckDB CLI available in this project; SQL logic tests
# are run via the Python-based duckdb_sqllogictest runner (installed by
# `make configure` into its Python venv).  This recipe builds the debug extension
# and delegates to `make test_debug`, which invokes the runner against the full
# test/sql/ directory.  All files matching test/sql/**/*.test are executed.
#
# The test/sql/phase2_ddl.test file exercises the full DDL round-trip:
#   define_semantic_view, list_semantic_views, describe_semantic_view, drop_semantic_view.
test-sql: build _ensure-test-deps
    make test_debug

# Download jaffle-shop data and create DuckLake/Iceberg catalog for integration tests.
# Idempotent — safe to run multiple times. Data files are gitignored.
# Uses uv to run the script with its declared dependencies (PEP 723).
setup-ducklake:
    uv run configure/setup_ducklake.py

# Download jaffle-shop data (if needed) and run the local DuckLake integration test.
# Convenience wrapper — safe to run repeatedly; setup-ducklake is idempotent.
test-ducklake: setup-ducklake build
    uv run test/integration/test_ducklake.py

# Run DuckLake CI integration test (uses inline synthetic data, no setup required).
# Tests semantic_view against an in-memory DuckLake catalog with known synthetic rows.
# Set SEMANTIC_VIEWS_EXTENSION_PATH to override the extension path (default: build/debug/).
test-ducklake-ci: _ensure-test-deps
    uv run test/integration/test_ducklake_ci.py

# Run Python vtab crash reproduction tests against the built extension.
# Exercises 5 crash vectors (13 tests) for type mismatch, connection lifetime,
# bind-time execution, chunking, and pointer stability.
test-vtab-crash: build
    uv run test/integration/test_vtab_crash.py

# Run Python caret position integration tests against the built extension.
# Verifies that DuckDB error caret (^) renders at the correct character position
# when malformed DDL flows through the extension's parser hook pipeline.
test-caret: build
    uv run test/integration/test_caret_position.py

# Run ADBC end-to-end transactional DDL tests against the built extension.
# Exercises CREATE / DROP / ALTER SEMANTIC VIEW under an ADBC autocommit=False
# connection — proves the v0.8.0 transactional-DDL fix works for the original
# motivating bug.
test-adbc: build
    uv run test/integration/test_adbc_transactions.py

# Run ADBC end-to-end query tests against the built extension.
# Exercises SELECT ... FROM semantic_view(...) through adbc_driver_duckdb
# across main expand path, FACTS, semi-additive, window, materialization
# routing, non-default-schema base tables, and multi-DB ATTACH. Regression
# guard for EXPAND-CTX-01..03 (v0.10.0). Scenarios 3-7 are gated by
# SKIP_UNTIL_PLAN_02 until Phase 66 Plan 02 lands the qualify_and_quote_table_ref
# migration of the FACTS/semi-additive/window/materialization sites.
test-adbc-queries: build
    uv run test/integration/test_adbc_queries.py

# Run regression test for the v0.8.0 silent-truncation FFI buffer bug.
# Creates a semantic view large enough that the rewritten INSERT exceeds
# the legacy 64 KB shim buffer; pre-fix this would have produced a
# misleading "Parser Error: syntax error" instead of succeeding.
test-large-view: build
    uv run test/integration/test_large_view_rewrite.py

# Multi-DB DDL isolation regression: load the extension into two databases
# in the same process and verify DESCRIBE / SHOW route to the right database.
# Pre-fix the C++ shim held a global sv_ddl_conn that the second LOAD would
# overwrite, causing the first DB's DESCRIBE/SHOW to silently target the
# second DB's connection.
test-multi-db: build
    uv run test/integration/test_multi_db_isolation.py

# Read-only database LOAD regression: bootstrap a view writable, reopen
# the file read-only via duckdb.connect(path, read_only=True), and
# verify LOAD succeeds, list_semantic_views() returns the bootstrapped
# view, semantic_view(...) queries work, and CREATE/DROP/ALTER fail
# with DuckDB's standard read-only error. Phase 63 (v0.9.0). See
# test/sql/readonly_load.test for the writable-side smoke fixture and
# the explanation of why full read-only coverage lives here rather
# than in sqllogictest.
test-readonly: build
    uv run test/integration/test_readonly_load.py

# Concurrent CREATE on the same view name from two threads. PK constraint
# on _definitions(name) serializes the inserts; exactly one must succeed.
# Also indirectly exercises the v0.8.0 race-guard pattern for DROP/ALTER.
# Phase 65.1 adds two per-call Connection regressions covering the
# read-side (concurrent SHOW SEMANTIC ... reads) and write-side
# (concurrent CREATE/DROP/ALTER on overlapping names) of the per-call
# Connection model — both must run in CI so the borrow contract is
# guarded under contention.
test-concurrent: build
    uv run test/integration/test_concurrent_ddl.py
    uv run test/integration/test_concurrent_reads_per_call_conn.py
    uv run test/integration/test_concurrent_writes_per_call_conn.py

# `LOAD semantic_views` twice in one process must be idempotent (parser-hook
# re-registration guard, WR-09 D-21). Behavioral half of the structural test
# in tests/parser_hook_idempotent.rs — both must stay wired.
test-load-idempotent: build
    uv run test/integration/test_load_extension_twice_idempotent.py

# CREATE SEMANTIC VIEW ... FROM YAML FILE under the v0.10.0 read-elimination
# architecture (Phase 65 Plan 04 D-11): OR REPLACE / IF NOT EXISTS, friendly
# error surfaces, YAML size cap, transactional rollback, get_ddl round-trip.
test-yaml-file-create: build
    uv run test/integration/test_create_from_yaml_v010.py

# DROP/ALTER SEMANTIC VIEW on a fresh read-only DB that was never bootstrapped
# must surface the canonical "semantic view 'X' does not exist" wording, not a
# raw `_definitions` catalog leak (Phase 65.1 Plan 04, WR-03 D-18).
test-readonly-fresh-drop: build
    uv run test/integration/test_drop_on_fresh_readonly_clear_error.py

# All Python integration suites against the built extension. This is the
# exact set the `python-integration` CI job runs (IntegrationChecks.yml) —
# keep the two in sync by editing THIS recipe, not the workflow.
# (test-ducklake-ci is excluded: it has its own dedicated CI job.)
test-integration: test-vtab-crash test-caret test-adbc test-adbc-queries test-large-view test-multi-db test-readonly test-concurrent test-load-idempotent test-yaml-file-create test-readonly-fresh-drop

# Run all tests: Rust unit tests + SQL logic tests + DuckLake integration + all Python integration suites
# Note: test-iceberg requires `just setup-ducklake` first. test-ducklake-ci uses synthetic data.
# _ensure-test-deps runs early to catch pip version mismatches before slow builds.
test-all: _ensure-test-deps test-rust test-sql test-ducklake-ci test-integration

# Check that fuzz targets compile (requires nightly)
check-fuzz:
    cargo +nightly check --manifest-path fuzz/Cargo.toml

# Verify test/sql/TEST_LIST matches the .test files on disk. The sqllogictest
# runner executes ONLY files named in TEST_LIST, so a .test file missing from
# it is silently skipped — this check makes that a hard error (CI runs it too).
check-test-list:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! diff <(find test/sql -maxdepth 1 -name '*.test' | sort) <(sort test/sql/TEST_LIST); then
      echo "ERROR: test/sql/TEST_LIST is out of sync with test/sql/*.test" >&2
      echo "  (diff above: '<' = on disk but not in TEST_LIST, '>' = in TEST_LIST but not on disk)" >&2
      exit 1
    fi

# Run the full CI suite locally (lint + test-list sync + test + fuzz check)
ci: lint check-test-list test-all check-fuzz docs-check

# Run a single fuzz target (default: fuzz_json_parse, 5 min timeout)
fuzz target="fuzz_json_parse" time="300":
    cargo +nightly fuzz run {{target}} fuzz/corpus/{{target}} fuzz/seeds/{{target}} -- -max_total_time={{time}}

# Run all six fuzz targets sequentially (5 min each, 30 min total)
fuzz-all time="300":
    cargo +nightly fuzz run fuzz_json_parse fuzz/corpus/fuzz_json_parse fuzz/seeds/fuzz_json_parse -- -max_total_time={{time}}
    cargo +nightly fuzz run fuzz_sql_expand fuzz/corpus/fuzz_sql_expand fuzz/seeds/fuzz_sql_expand -- -max_total_time={{time}}
    cargo +nightly fuzz run fuzz_query_names fuzz/corpus/fuzz_query_names fuzz/seeds/fuzz_query_names -- -max_total_time={{time}}
    cargo +nightly fuzz run fuzz_ddl_parse fuzz/corpus/fuzz_ddl_parse fuzz/seeds/fuzz_ddl_parse -- -max_total_time={{time}}
    cargo +nightly fuzz run fuzz_yaml_parse fuzz/corpus/fuzz_yaml_parse fuzz/seeds/fuzz_yaml_parse -- -max_total_time={{time}}
    cargo +nightly fuzz run fuzz_parser_override_ffi fuzz/corpus/fuzz_parser_override_ffi fuzz/seeds/fuzz_parser_override_ffi -- -max_total_time={{time}}

# Minimize corpus for a fuzz target (removes inputs that don't add coverage)
fuzz-cmin target="fuzz_json_parse":
    cargo +nightly fuzz cmin {{target}}

# Re-fetch vendored DuckDB amalgamation (duckdb.hpp + duckdb.cpp) from GitHub release.
# Run after DuckDB version bump. Version is read from .duckdb-version.
# Downloads to .amalgamation/<version>/ cache, then copies to cpp/include/.
update-headers:
    @VER=$(cat .duckdb-version); \
    CACHE=".amalgamation/$VER"; \
    if [ -f "$CACHE/duckdb.cpp" ]; then \
      echo "Cache hit: $CACHE/duckdb.hpp+cpp"; \
    else \
      echo "Fetching DuckDB $VER amalgamation..."; \
      mkdir -p "$CACHE"; \
      curl -sL -o /tmp/libduckdb-src.zip \
        "https://github.com/duckdb/duckdb/releases/download/$VER/libduckdb-src.zip"; \
      unzip -o -j /tmp/libduckdb-src.zip "duckdb.hpp" "duckdb.cpp" -d "$CACHE/"; \
      rm /tmp/libduckdb-src.zip; \
      echo "Cached $CACHE/duckdb.hpp+cpp"; \
    fi; \
    mkdir -p cpp/include; \
    cp "$CACHE/duckdb.hpp" cpp/include/duckdb.hpp; \
    cp "$CACHE/duckdb.cpp" cpp/include/duckdb.cpp; \
    echo "Installed cpp/include/duckdb.hpp+cpp ($VER)"

# Clean build artifacts
clean:
    make clean
    cargo clean

# Reclaim stale target artifacts older than 14 days without invalidating current build cache.
# Intended for milestone completion (see CLAUDE.md "Milestone Completion"). Auto-installs cargo-sweep on first run.
clean-stale:
    @command -v cargo-sweep >/dev/null 2>&1 || cargo install cargo-sweep
    @echo "Sweeping target/ artifacts not touched in 14 days..."
    cargo sweep --time 14
    @echo "Done. Current build cache preserved; next incremental build remains fast."

# Check Sphinx documentation builds without warnings (mirrors CI)
docs-check:
    uv run --project docs sphinx-build -b html -W docs docs/_build/html

# Build Sphinx documentation to docs/_build/html (clean build)
docs-build:
    rm -rf docs/_build
    uv run --project docs sphinx-build -b html docs docs/_build/html

# Serve docs locally with live-reload (http://127.0.0.1:8000)
docs-serve:
    uv run --project docs sphinx-autobuild docs docs/_build/html

# NOTE: sed -i '' is macOS-specific (BSD sed). This matches the dev environment (darwin).
# Set CE_REPO env var to override the CE fork path (default: ~/Documents/Dev/Sources/community-extensions).
# Automate CE registry release: update description.yml, copy to CE fork, open PR
release:
    #!/usr/bin/env bash
    set -euo pipefail
    # --- Precondition checks ---
    BRANCH=$(git branch --show-current)
    if [ "$BRANCH" != "main" ]; then
      echo "ERROR: must be on 'main' branch (currently on '$BRANCH')" >&2
      exit 1
    fi
    if ! command -v gh &>/dev/null; then
      echo "ERROR: 'gh' CLI not found. Install: https://cli.github.com/" >&2
      exit 1
    fi
    if ! git diff --quiet || ! git diff --cached --quiet; then
      echo "ERROR: working tree is not clean. Commit or stash changes first." >&2
      exit 1
    fi
    # --- Extract values ---
    SHA=$(git rev-parse HEAD)
    VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
    if [ -z "$VERSION" ]; then
      echo "ERROR: could not extract version from Cargo.toml" >&2
      exit 1
    fi
    echo "Releasing v$VERSION (ref: $SHA)"
    # --- Update description.yml in this repo ---
    sed -i '' "s/^  ref: .*/  ref: $SHA/" description.yml
    sed -i '' "s/^  version: .*/  version: $VERSION/" description.yml
    git add description.yml
    git commit -m "chore: update description.yml for v$VERSION release"
    git push origin main
    echo "Committed and pushed description.yml update to main"
    # --- Copy to CE fork and open PR ---
    CE_REPO="${CE_REPO:-$HOME/Documents/Dev/Sources/community-extensions}"
    if [ ! -d "$CE_REPO" ]; then
      echo "ERROR: CE fork directory not found at '$CE_REPO'" >&2
      echo "  Clone your fork of duckdb/community-extensions there, or set CE_REPO env var." >&2
      exit 1
    fi
    mkdir -p "$CE_REPO/extensions/semantic_views"
    cp description.yml "$CE_REPO/extensions/semantic_views/description.yml"
    echo "Copied description.yml to CE fork"
    cd "$CE_REPO"
    git checkout semantic-views
    git add extensions/semantic_views/description.yml
    git commit -m "Update semantic_views to v$VERSION"
    git push origin semantic-views
    echo "Pushed to CE fork"
    PR_URL=$(gh pr create \
      --repo duckdb/community-extensions \
      --head anentropic:semantic-views \
      --base main \
      --title "Update semantic_views to v$VERSION" \
      --body "Update semantic_views extension to v$VERSION (ref: $SHA)")
    echo ""
    echo "=== Release Summary ==="
    echo "  Version: v$VERSION"
    echo "  SHA:     $SHA"
    echo "  PR:      $PR_URL"
