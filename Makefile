.PHONY: clean clean_all

PROJ_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

EXTENSION_NAME=semantic_views

# Target DuckDB version — read from .duckdb-version (single source of truth)
TARGET_DUCKDB_VERSION=$(shell cat .duckdb-version)

# Pin the test-runner DuckDB pip package to match the build version.
# base.Makefile defaults to latest PyPI; strip `v` prefix for PEP 440 compliance.
#
# Scoped to builds against the upstream release we target. A downstream
# distribution builds community extensions against its own engine, and its
# ci-tools fork installs its own runner package under its own versioning
# (Haybarn: `haybarn-cli`, published as pre-releases only). Pinning our release
# number there asks pip for a version that exists in no index and fails `make
# configure` on every platform, so leave the choice to the harness.
#
# DUCKDB_GIT_VERSION is the engine tag being built against: empty locally,
# `vX.Y.Z` in upstream CI, `haybarn-v1.5.5-rc1` (or `main`, for a nightly) when
# it is something else. `?=` so an explicit value from the environment wins
# either way.
ifeq ($(filter-out $(TARGET_DUCKDB_VERSION),$(DUCKDB_GIT_VERSION)),)
DUCKDB_TEST_VERSION ?= $(subst v,,$(TARGET_DUCKDB_VERSION))
endif

all: configure debug

# Include makefiles from DuckDB
include extension-ci-tools/makefiles/c_api_extensions/base.Makefile
include extension-ci-tools/makefiles/c_api_extensions/rust.Makefile

# Override UNSTABLE_C_API_FLAG AFTER the include (base.Makefile resets it).
# C_STRUCT_UNSTABLE ABI with C++ helper for parser hooks (Option A).
# Rust owns the entry point (semantic_views_init_c_api), C++ helper registers hooks.
UNSTABLE_C_API_FLAG=--abi-type C_STRUCT_UNSTABLE

# Run sqllogictest through a strict wrapper instead of the bare
# `python -m duckdb_sqllogictest` base.Makefile sets (overridden AFTER the
# include, and BEFORE the `:=` TEST_RUNNER_FILE_LIST_* definitions below, which
# capture it). The bare runner exits 0 when `require semantic_views` is
# SKIPPED -- a missing or renamed extension file skipped all 115 files and
# printed "115 test files passed" -- and also when a file cannot be parsed or
# runs zero statements. scripts/run_sqllogictest.py takes the same arguments
# and fails each of those; test/integration/test_sqllogictest_guards.py
# (`just test-sqllogictest-guards`) pins every case. The extension-ci-tools
# submodule is left untouched.
#
# The wrapper also fails any file that hits an error the runner silently
# ignores ("HTTP" / "Unable to connect" count as a PASS upstream), under
# SV_OFFLINE_EXTENSIONS=1 too. Offline runs excuse nothing here: the files in
# OFFLINE_EXTENSION_TESTS never reach the wrapper, because prepare_test_list
# below leaves them out of the list and prints their SKIPPED lines. (The
# wrapper's own --offline-excused flag is for running it directly.)
TEST_RUNNER=$(PYTHON_VENV_BIN) scripts/run_sqllogictest.py --required-extension semantic_views

# The DuckDB the C++ shim is compiled against. build.rs links the amalgamation
# INTO the extension binary (the parser hook the shim registers is C++ internals
# with no C-API equivalent), so it is an ABI contract with whatever engine loads
# the result -- not a convenience download.
#
# AMALGAMATION_SRC_DIR: an engine source tree supplied by the harness. A
#   distribution that builds community extensions against its own engine clones
#   it into ./duckdb (Haybarn's fork of _extension_distribution.yml does this
#   from `override_duckdb_repository`); when it is there, that IS the engine the
#   extension will be loaded into, so the amalgamation is generated from it.
#   Upstream community-extensions hands a C-API extension no such tree, so that
#   build keeps using the release below.
# AMALGAMATION_URL: the pinned upstream release, used when no tree is present
#   (local development, upstream CI). Gitignored, ~25 MB, cached under
#   .amalgamation/<id>/ so it survives branch switches.
#
# Both are overridable from the environment; see scripts/ensure_amalgamation.py
# for the selection rules, the provenance stamp and the version guard.
AMALGAMATION_SRC_DIR ?= duckdb
AMALGAMATION_URL ?= https://github.com/duckdb/duckdb/releases/download/$(TARGET_DUCKDB_VERSION)/libduckdb-src.zip

.PHONY: ensure_amalgamation
ensure_amalgamation:
	@$(PYTHON_BIN) scripts/ensure_amalgamation.py \
		--version "$(TARGET_DUCKDB_VERSION)" \
		--src-dir "$(AMALGAMATION_SRC_DIR)" \
		--url "$(AMALGAMATION_URL)"

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
# `cargo test` (catalog::tests::tc6_restart_persistence_survives_reopen:
# open -> persist -> close -> reopen -> init_catalog -> lookup).
TEST_LIST_PATH := test/sql/TEST_LIST

# Offline mode for sandboxes that cannot reach extensions.duckdb.org (LOCAL USE
# ONLY). A few tests need a DuckDB extension that is neither bundled in the pip
# `duckdb` wheel nor in this repo, so DuckDB must download it on first use:
#   test/sql/peg_compat.test  -- `CALL enable_peg_parser()` autoloads `autocomplete`
# Behind a proxy that blocks the download, that file fails at its first PEG
# statement and, because the runner stops at the first failing file, halts the
# whole suite. With SV_OFFLINE_EXTENSIONS=1 those files are left out of the run
# and each is reported with a loud SKIPPED line; every other file still runs.
#
# Deliberately an explicit opt-in, NOT a sqllogictest `require autocomplete`:
# `require` skips silently whenever the extension cannot be loaded, CI
# included, so a broken download in CI would turn this coverage off without a
# red. Without the variable nothing changes -- CI runs the file and fails hard
# if it cannot load the extension -- and the variable is refused outright when
# `CI` is set (GitHub Actions sets CI=true), so it cannot leak into CI.
OFFLINE_EXTENSION_TESTS := test/sql/peg_compat.test
ifeq ($(SV_OFFLINE_EXTENSIONS),1)
ifneq ($(CI),)
$(error SV_OFFLINE_EXTENSIONS=1 skips tests and is for local offline runs only; refusing because CI='$(CI)')
endif
EFFECTIVE_TEST_LIST := build/test_list_offline
else
EFFECTIVE_TEST_LIST := $(TEST_LIST_PATH)
endif

# Writes $(EFFECTIVE_TEST_LIST) in offline mode and prints one SKIPPED line per
# left-out file. A no-op otherwise.
.PHONY: prepare_test_list
prepare_test_list:
ifeq ($(SV_OFFLINE_EXTENSIONS),1)
	@mkdir -p $(dir $(EFFECTIVE_TEST_LIST))
	@grep -vxF $(foreach t,$(OFFLINE_EXTENSION_TESTS),-e $(t)) $(TEST_LIST_PATH) > $(EFFECTIVE_TEST_LIST)
	@for t in $(OFFLINE_EXTENSION_TESTS); do \
	  echo "SKIPPED (SV_OFFLINE_EXTENSIONS=1): $$t -- needs a DuckDB extension downloaded from extensions.duckdb.org; NOT run"; \
	done
endif

# --test-dir is passed alongside --file-list so the runner can resolve __TEST_DIR__
# (used by tests that create file-backed databases). The file list controls which
# tests are actually executed.
TEST_RUNNER_FILE_LIST_DEBUG  := $(TEST_RUNNER) --test-dir test/sql --file-list $(EFFECTIVE_TEST_LIST) $(EXTRA_EXTENSIONS_PARAM) --external-extension build/debug/$(EXTENSION_NAME).duckdb_extension
TEST_RUNNER_FILE_LIST_RELEASE := $(TEST_RUNNER) --test-dir test/sql --file-list $(EFFECTIVE_TEST_LIST) $(EXTRA_EXTENSIONS_PARAM) --external-extension build/release/$(EXTENSION_NAME).duckdb_extension

# Override base.Makefile test targets to patch runner before tests.
# SKIP_TESTS platforms (musl, mingw) resolve to tests_skipped before reaching these
# targets, so patch-runner is never called on those platforms — which is correct.
#
# HISTORY (TC-10): these targets used to run each .test file in its OWN
# sqllogictest process. DuckDB 1.5.0 changed the parser-extension lifecycle
# (ExtensionCallbackManager) such that one process creating and destroying
# several databases in sequence segfaulted, so the per-file loop was the only
# way to get a green suite. `probe_isolation_debug_internal` existed to detect
# the upstream fix rather than let the workaround outlive its cause.
#
# The probe fired on DuckDB v1.5.5 (2026-08-09): the single-process run no
# longer crashes. Both targets are back on the single-process
# TEST_RUNNER_FILE_LIST_* path, which is simpler and stops spawning one process
# per file. Evidence: 3/3 local runs at 111/111 SUCCESS (~33 s vs ~39 s for the
# loop), plus a workflow_dispatch BuildAll run confirming windows_amd64 and
# osx_arm64 — the two platforms that actually execute the release suite.
#
# If the crash ever returns, the loop is in this file's history; restore it
# along with the probe rather than papering over an intermittent segfault.
#
# The `@echo` after each run is deliberate: without it a passing single-process
# run prints only per-file SUCCESS lines, and the "did the suite actually run,
# or did it silently skip?" question this project has been bitten by twice
# (see MAINTAINER.md, CI Workflows) needs a greppable end-of-run marker.
test_extension_debug_internal: patch-runner prepare_test_list
	@echo "Running DEBUG tests.."
	@$(TEST_RUNNER_FILE_LIST_DEBUG)
	@echo "$$(wc -l < $(EFFECTIVE_TEST_LIST) | tr -d ' ') test files passed (single process)$(if $(filter 1,$(SV_OFFLINE_EXTENSIONS)), -- $(words $(OFFLINE_EXTENSION_TESTS)) SKIPPED offline (SV_OFFLINE_EXTENSIONS=1),)"

test_extension_release_internal: patch-runner prepare_test_list
	@echo "Running RELEASE tests.."
	@$(TEST_RUNNER_FILE_LIST_RELEASE)
	@echo "$$(wc -l < $(EFFECTIVE_TEST_LIST) | tr -d ' ') test files passed (single process)$(if $(filter 1,$(SV_OFFLINE_EXTENSIONS)), -- $(words $(OFFLINE_EXTENSION_TESTS)) SKIPPED offline (SV_OFFLINE_EXTENSIONS=1),)"

