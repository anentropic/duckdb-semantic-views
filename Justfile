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

# Run all tests: Rust unit tests + SQL logic tests + DuckLake integration + vtab crash + caret position
# Note: test-iceberg requires `just setup-ducklake` first. test-ducklake-ci uses synthetic data.
# _ensure-test-deps runs early to catch pip version mismatches before slow builds.
test-all: _ensure-test-deps test-rust test-sql test-ducklake-ci test-vtab-crash test-caret

# Run a single fuzz target (default: fuzz_json_parse, 5 min timeout)
fuzz target="fuzz_json_parse" time="300":
    cargo +nightly fuzz run {{target}} fuzz/corpus/{{target}} fuzz/seeds/{{target}} -- -max_total_time={{time}}

# Run all three fuzz targets sequentially (5 min each, 15 min total)
fuzz-all time="300":
    cargo +nightly fuzz run fuzz_json_parse fuzz/corpus/fuzz_json_parse fuzz/seeds/fuzz_json_parse -- -max_total_time={{time}}
    cargo +nightly fuzz run fuzz_sql_expand fuzz/corpus/fuzz_sql_expand fuzz/seeds/fuzz_sql_expand -- -max_total_time={{time}}
    cargo +nightly fuzz run fuzz_query_names fuzz/corpus/fuzz_query_names fuzz/seeds/fuzz_query_names -- -max_total_time={{time}}

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
