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

# Clean build artifacts
clean:
    make clean
    cargo clean
