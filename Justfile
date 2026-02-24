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

# Run SQL logic tests for Phase 2 DDL via the SQLLogicTest runner.
#
# There is no standalone DuckDB CLI available in this project; SQL logic tests
# are run via the Python-based duckdb_sqllogictest runner (installed by
# `make configure` into configure/venv).  This recipe builds the debug extension
# and delegates to `make test_debug`, which invokes the runner against the full
# test/sql/ directory.  All files matching test/sql/**/*.test are executed.
#
# The test/sql/phase2_ddl.test file exercises the full DDL round-trip:
#   define_semantic_view, list_semantic_views, describe_semantic_view, drop_semantic_view.
test-sql: build
    make test_debug

# Run all tests: Rust unit tests + SQL logic tests (via SQLLogicTest runner)
test-all: test-rust test-sql

# Clean build artifacts
clean:
    make clean
    cargo clean
