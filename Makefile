.PHONY: clean clean_all

PROJ_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

EXTENSION_NAME=semantic_views

# Set to 1 to enable Unstable API (binaries will only work on TARGET_DUCKDB_VERSION, forwards compatibility will be broken)
# Note: currently extension-template-rs requires this, as duckdb-rs relies on unstable C API functionality
USE_UNSTABLE_C_API=1

# Target DuckDB version — updated automatically by DuckDBVersionMonitor workflow
TARGET_DUCKDB_VERSION=v1.4.4

all: configure debug

# Include makefiles from DuckDB
include extension-ci-tools/makefiles/c_api_extensions/base.Makefile
include extension-ci-tools/makefiles/c_api_extensions/rust.Makefile

configure: venv platform extension_version

debug: build_extension_library_debug build_extension_with_metadata_debug
release: build_extension_library_release build_extension_with_metadata_release

test: test_debug
test_debug: test_extension_debug
test_release: test_extension_release

clean: clean_build clean_rust
clean_all: clean_configure clean

# Override Rust build targets to pass --no-default-features --features extension.
#
# The `default` feature enables duckdb/bundled, which compiles DuckDB from source
# and links it into the binary — this is used for `cargo test` to enable
# Connection::open_in_memory() in unit tests.
#
# For the actual DuckDB extension binary we do NOT want bundled DuckDB;
# the extension must use the function-pointer stubs (duckdb/loadable-extension)
# so that it links against the DuckDB that loads it at runtime.
# --no-default-features removes duckdb/bundled; --features extension adds
# duckdb/loadable-extension + duckdb/vscalar.
build_extension_library_debug: check_configure
	DUCKDB_EXTENSION_NAME=$(EXTENSION_NAME) DUCKDB_EXTENSION_MIN_DUCKDB_VERSION=$(TARGET_DUCKDB_VERSION) cargo build $(CARGO_OVERRIDE_DUCKDB_RS_FLAG) $(TARGET_INFO) --no-default-features --features extension
	$(PYTHON_VENV_BIN) -c "from pathlib import Path;Path('$(EXTENSION_BUILD_PATH)/debug/extension/$(EXTENSION_NAME)').mkdir(parents=True, exist_ok=True)"
	$(PYTHON_VENV_BIN) -c "import shutil;shutil.copyfile('$(TARGET_PATH)/debug$(IS_EXAMPLE)/$(RUST_LIBNAME)', '$(EXTENSION_BUILD_PATH)/debug/$(EXTENSION_LIB_FILENAME)')"

build_extension_library_release: check_configure
	DUCKDB_EXTENSION_NAME=$(EXTENSION_NAME) DUCKDB_EXTENSION_MIN_DUCKDB_VERSION=$(TARGET_DUCKDB_VERSION) cargo build $(CARGO_OVERRIDE_DUCKDB_RS_FLAG) --release $(TARGET_INFO) --no-default-features --features extension
	$(PYTHON_VENV_BIN) -c "from pathlib import Path;Path('$(EXTENSION_BUILD_PATH)/release/extension/$(EXTENSION_NAME)').mkdir(parents=True, exist_ok=True)"
	$(PYTHON_VENV_BIN) -c "import shutil;shutil.copyfile('$(TARGET_PATH)/release$(IS_EXAMPLE)/$(RUST_LIBNAME)', '$(EXTENSION_BUILD_PATH)/release/$(EXTENSION_LIB_FILENAME)')"
