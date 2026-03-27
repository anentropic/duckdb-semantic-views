.PHONY: clean clean_all

PROJ_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

EXTENSION_NAME=semantic_views

# Target DuckDB version — read from .duckdb-version (single source of truth)
TARGET_DUCKDB_VERSION=$(shell cat .duckdb-version)

# Pin the test-runner DuckDB pip package to match the build version.
# base.Makefile defaults to latest PyPI; strip `v` prefix for PEP 440 compliance.
DUCKDB_TEST_VERSION=$(subst v,,$(TARGET_DUCKDB_VERSION))

all: configure debug

# Include makefiles from DuckDB
include extension-ci-tools/makefiles/c_api_extensions/base.Makefile
include extension-ci-tools/makefiles/c_api_extensions/rust.Makefile

# Override UNSTABLE_C_API_FLAG AFTER the include (base.Makefile resets it).
# C_STRUCT_UNSTABLE ABI with C++ helper for parser hooks (Option A).
# Rust owns the entry point (semantic_views_init_c_api), C++ helper registers hooks.
UNSTABLE_C_API_FLAG=--abi-type C_STRUCT_UNSTABLE

# Auto-download DuckDB amalgamation (gitignored, ~25MB) if not present or wrong version.
# CI checks out the repo without these files; local devs fetch via `just update-headers`.
# The version comes from .duckdb-version via TARGET_DUCKDB_VERSION.
# A versioned cache under .amalgamation/<version>/ survives branch switches.
AMALGAMATION_URL=https://github.com/duckdb/duckdb/releases/download/$(TARGET_DUCKDB_VERSION)/libduckdb-src.zip
AMALGAMATION_CACHE=.amalgamation/$(TARGET_DUCKDB_VERSION)

# Check installed amalgamation matches TARGET_DUCKDB_VERSION; fetch/copy if not.
.PHONY: ensure_amalgamation
ensure_amalgamation:
	@INSTALLED=$$(grep -m1 '#define DUCKDB_VERSION' cpp/include/duckdb.hpp 2>/dev/null | sed 's/.*"\(.*\)"/\1/'); \
	if [ "$$INSTALLED" = "$(TARGET_DUCKDB_VERSION)" ]; then \
		exit 0; \
	fi; \
	echo "Amalgamation version mismatch (have=$${INSTALLED:-none}, want=$(TARGET_DUCKDB_VERSION))"; \
	if [ -f "$(AMALGAMATION_CACHE)/duckdb.cpp" ]; then \
		echo "Restoring from cache $(AMALGAMATION_CACHE)/..."; \
	else \
		echo "Downloading DuckDB $(TARGET_DUCKDB_VERSION) amalgamation..."; \
		mkdir -p "$(AMALGAMATION_CACHE)"; \
		curl -sL -o /tmp/libduckdb-src.zip "$(AMALGAMATION_URL)"; \
		unzip -o -j /tmp/libduckdb-src.zip "duckdb.hpp" "duckdb.cpp" -d "$(AMALGAMATION_CACHE)/"; \
		rm -f /tmp/libduckdb-src.zip; \
		echo "Cached $(AMALGAMATION_CACHE)/duckdb.{hpp,cpp}"; \
	fi; \
	mkdir -p cpp/include; \
	cp "$(AMALGAMATION_CACHE)/duckdb.hpp" cpp/include/duckdb.hpp; \
	cp "$(AMALGAMATION_CACHE)/duckdb.cpp" cpp/include/duckdb.cpp; \
	echo "Installed cpp/include/duckdb.{hpp,cpp} ($(TARGET_DUCKDB_VERSION))"

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
build_extension_library_debug: check_configure ensure_amalgamation
	DUCKDB_EXTENSION_NAME=$(EXTENSION_NAME) DUCKDB_EXTENSION_MIN_DUCKDB_VERSION=$(TARGET_DUCKDB_VERSION) cargo build $(CARGO_OVERRIDE_DUCKDB_RS_FLAG) $(TARGET_INFO) --no-default-features --features extension
	$(PYTHON_VENV_BIN) -c "from pathlib import Path;Path('$(EXTENSION_BUILD_PATH)/debug/extension/$(EXTENSION_NAME)').mkdir(parents=True, exist_ok=True)"
	$(PYTHON_VENV_BIN) -c "import shutil;shutil.copyfile('$(TARGET_PATH)/debug$(IS_EXAMPLE)/$(RUST_LIBNAME)', '$(EXTENSION_BUILD_PATH)/debug/$(EXTENSION_LIB_FILENAME)')"

build_extension_library_release: check_configure ensure_amalgamation
	DUCKDB_EXTENSION_NAME=$(EXTENSION_NAME) DUCKDB_EXTENSION_MIN_DUCKDB_VERSION=$(TARGET_DUCKDB_VERSION) cargo build $(CARGO_OVERRIDE_DUCKDB_RS_FLAG) --release $(TARGET_INFO) --no-default-features --features extension
	$(PYTHON_VENV_BIN) -c "from pathlib import Path;Path('$(EXTENSION_BUILD_PATH)/release/extension/$(EXTENSION_NAME)').mkdir(parents=True, exist_ok=True)"
	$(PYTHON_VENV_BIN) -c "import shutil;shutil.copyfile('$(TARGET_PATH)/release$(IS_EXAMPLE)/$(RUST_LIBNAME)', '$(EXTENSION_BUILD_PATH)/release/$(EXTENSION_LIB_FILENAME)')"

# Patch installed duckdb_sqllogictest to add notwindows/windows platform detection.
# Idempotent. Remove once extension-ci-tools updates its pinned sqllogictest commit.
.PHONY: patch-runner
patch-runner: check_configure
	@$(PYTHON_VENV_BIN) scripts/patch_sqllogictest.py

# Use an explicit file list to control which tests run.
# test/sql/TEST_LIST enumerates the tests that are stable with the Python
# sqllogictest runner + external extension. phase2_restart.test is excluded
# because the Python runner cannot reload an external extension after the
# `restart` directive in a file-backed database (the extension's init_catalog
# may deadlock during reload). Restart persistence is verified separately via
# `cargo test` Rust integration tests.
TEST_LIST_PATH := test/sql/TEST_LIST
# --test-dir is passed alongside --file-list so the runner can resolve __TEST_DIR__
# (used by tests that create file-backed databases). The file list controls which
# tests are actually executed.
TEST_RUNNER_FILE_LIST_DEBUG  := $(TEST_RUNNER) --test-dir test/sql --file-list $(TEST_LIST_PATH) $(EXTRA_EXTENSIONS_PARAM) --external-extension build/debug/$(EXTENSION_NAME).duckdb_extension
TEST_RUNNER_FILE_LIST_RELEASE := $(TEST_RUNNER) --test-dir test/sql --file-list $(TEST_LIST_PATH) $(EXTRA_EXTENSIONS_PARAM) --external-extension build/release/$(EXTENSION_NAME).duckdb_extension

# Override base.Makefile test targets to patch runner before tests.
# SKIP_TESTS platforms (musl, mingw) resolve to tests_skipped before reaching these
# targets, so patch-runner is never called on those platforms — which is correct.
#
# DuckDB 1.5.0 changed parser extension lifecycle (ExtensionCallbackManager).
# Running all test files in a single sqllogictest process causes a segfault
# when the runner creates/destroys multiple databases sequentially. Work around
# by running each test file in a separate process. Each test passes in isolation.
# Uses mktemp instead of /dev/stdin because the Python sqllogictest runner
# resolves /dev/stdin to /proc/self/fd/0, which doesn't exist on Windows.
test_extension_debug_internal: patch-runner
	@echo "Running DEBUG tests.."
	@FAILED=0; \
	TOTAL=0; \
	TMPLIST=$$(mktemp); \
	while IFS= read -r testfile; do \
		TOTAL=$$((TOTAL + 1)); \
		echo "$$testfile" > "$$TMPLIST"; \
		if ! $(TEST_RUNNER) --test-dir test/sql --file-list "$$TMPLIST" $(EXTRA_EXTENSIONS_PARAM) --external-extension build/debug/$(EXTENSION_NAME).duckdb_extension; then \
			echo "FAILED: $$testfile"; \
			FAILED=$$((FAILED + 1)); \
		fi; \
	done < $(TEST_LIST_PATH); \
	rm -f "$$TMPLIST"; \
	echo "$$TOTAL tests run, $$FAILED failed"; \
	[ $$FAILED -eq 0 ]

test_extension_release_internal: patch-runner
	@echo "Running RELEASE tests.."
	@FAILED=0; \
	TOTAL=0; \
	TMPLIST=$$(mktemp); \
	while IFS= read -r testfile; do \
		TOTAL=$$((TOTAL + 1)); \
		echo "$$testfile" > "$$TMPLIST"; \
		if ! $(TEST_RUNNER) --test-dir test/sql --file-list "$$TMPLIST" $(EXTRA_EXTENSIONS_PARAM) --external-extension build/release/$(EXTENSION_NAME).duckdb_extension; then \
			echo "FAILED: $$testfile"; \
			FAILED=$$((FAILED + 1)); \
		fi; \
	done < $(TEST_LIST_PATH); \
	rm -f "$$TMPLIST"; \
	echo "$$TOTAL tests run, $$FAILED failed"; \
	[ $$FAILED -eq 0 ]
