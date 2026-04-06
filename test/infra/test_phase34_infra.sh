#!/usr/bin/env bash
# test/infra/test_phase34_infra.sh
#
# Infrastructure assertion tests for Phase 34: DuckDB 1.5 Upgrade & LTS Branch.
#
# These tests assert observable file-system and git state, not code behavior.
# They run from the repo root and are safe to execute at any time.
#
# Usage:
#   bash test/infra/test_phase34_infra.sh
#
# Exit code 0 = all assertions green.
# Exit code 1 = one or more assertions failed.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

PASS=0
FAIL=0
FAILURES=()

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL + 1)); FAILURES+=("$1"); }

# ---------------------------------------------------------------------------
# DKDB-01 / DKDB-05: .duckdb-version exists and has a valid version
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-01/05: .duckdb-version exists and is consistent ==="

if [[ -f ".duckdb-version" ]]; then
  pass ".duckdb-version file exists"
  EXPECTED_VER=$(cat .duckdb-version | tr -d '[:space:]')
  if [[ "$EXPECTED_VER" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    pass ".duckdb-version has valid format (got: $EXPECTED_VER)"
  else
    fail ".duckdb-version has invalid format (got: '$EXPECTED_VER', expected vX.Y.Z)"
  fi
else
  fail ".duckdb-version file does not exist"
  EXPECTED_VER=""
fi

# Derive the expected duckdb-rs crate version from .duckdb-version
# DuckDB 1.X.Y -> duckdb-rs 1.1XY00.0 (e.g., 1.5.0 -> 1.10500.0, 1.5.1 -> 1.10501.0)
if [[ -n "$EXPECTED_VER" ]]; then
  MAJOR=$(echo "$EXPECTED_VER" | sed 's/v//' | cut -d. -f1)
  MINOR=$(echo "$EXPECTED_VER" | sed 's/v//' | cut -d. -f2)
  PATCH=$(echo "$EXPECTED_VER" | sed 's/v//' | cut -d. -f3)
  EXPECTED_CRATE="${MAJOR}.1$(printf '%02d' "$MINOR")$(printf '%02d' "$PATCH").0"
fi

# ---------------------------------------------------------------------------
# DKDB-01: Cargo.toml pins duckdb-rs consistently with .duckdb-version
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-01: Cargo.toml duckdb-rs pin matches .duckdb-version ==="

if [[ -n "$EXPECTED_CRATE" ]]; then
  if grep -q "version = \"=$EXPECTED_CRATE\"" Cargo.toml; then
    pass "Cargo.toml: duckdb crate pinned at =$EXPECTED_CRATE"
  else
    fail "Cargo.toml: duckdb crate not pinned at =$EXPECTED_CRATE"
  fi

  if grep -q "libduckdb-sys = \"=$EXPECTED_CRATE\"" Cargo.toml; then
    pass "Cargo.toml: libduckdb-sys pinned at =$EXPECTED_CRATE"
  else
    fail "Cargo.toml: libduckdb-sys not pinned at =$EXPECTED_CRATE"
  fi
else
  fail "Cargo.toml: cannot check pins (no valid .duckdb-version)"
fi

# ---------------------------------------------------------------------------
# DKDB-01: PEG compatibility test file exists (created in phase 34-01)
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-01: PEG parser compatibility test exists ==="

if [[ -f "test/sql/peg_compat.test" ]]; then
  LINES=$(wc -l < test/sql/peg_compat.test)
  pass "test/sql/peg_compat.test exists ($LINES lines)"
  if [[ "$LINES" -ge 15 ]]; then
    pass "test/sql/peg_compat.test has at least 15 lines"
  else
    fail "test/sql/peg_compat.test has only $LINES lines (minimum 15 required)"
  fi
else
  fail "test/sql/peg_compat.test does not exist"
fi

if grep -q 'peg_compat.test' test/sql/TEST_LIST; then
  pass "test/sql/TEST_LIST includes peg_compat.test"
else
  fail "test/sql/TEST_LIST does not include peg_compat.test"
fi

# ---------------------------------------------------------------------------
# DKDB-03: duckdb/1.4.x LTS branch exists locally
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-03: duckdb/1.4.x LTS branch exists ==="

if git branch | grep -q 'duckdb/1\.4\.x'; then
  pass "Local branch 'duckdb/1.4.x' exists"
else
  fail "Local branch 'duckdb/1.4.x' does not exist (run: git branch)"
fi

# ---------------------------------------------------------------------------
# DKDB-04: BuildAll.yml CI config references .duckdb-version consistently
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-04: BuildAll.yml references .duckdb-version consistently ==="

BUILD_YML=".github/workflows/BuildAll.yml"

if [[ -f "$BUILD_YML" ]]; then
  pass "$BUILD_YML exists"

  if [[ -n "$EXPECTED_VER" ]]; then
    if grep -q "@$EXPECTED_VER" "$BUILD_YML"; then
      pass "$BUILD_YML: uses extension-ci-tools@$EXPECTED_VER"
    else
      fail "$BUILD_YML: does not reference @$EXPECTED_VER"
    fi

    if grep -q "duckdb_version: $EXPECTED_VER" "$BUILD_YML"; then
      pass "$BUILD_YML: duckdb_version is $EXPECTED_VER"
    else
      fail "$BUILD_YML: duckdb_version is not $EXPECTED_VER"
    fi

    if grep -q "ci_tools_version: $EXPECTED_VER" "$BUILD_YML"; then
      pass "$BUILD_YML: ci_tools_version is $EXPECTED_VER"
    else
      fail "$BUILD_YML: ci_tools_version is not $EXPECTED_VER"
    fi
  else
    fail "$BUILD_YML: cannot check versions (no valid .duckdb-version)"
  fi
else
  fail "$BUILD_YML does not exist"
fi

# ---------------------------------------------------------------------------
# DKDB-06: DuckDBVersionMonitor.yml has dual-track jobs (check-latest + check-lts)
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-06: DuckDBVersionMonitor.yml has dual-track monitoring ==="

MONITOR_YML=".github/workflows/DuckDBVersionMonitor.yml"

if [[ -f "$MONITOR_YML" ]]; then
  pass "$MONITOR_YML exists"

  if grep -q 'check-latest:' "$MONITOR_YML"; then
    pass "$MONITOR_YML: 'check-latest' job exists"
  else
    fail "$MONITOR_YML: 'check-latest' job missing"
  fi

  if grep -q 'check-lts:' "$MONITOR_YML"; then
    pass "$MONITOR_YML: 'check-lts' job exists"
  else
    fail "$MONITOR_YML: 'check-lts' job missing"
  fi

  if grep -q 'ref: main' "$MONITOR_YML"; then
    pass "$MONITOR_YML: check-latest job checks out 'main' branch"
  else
    fail "$MONITOR_YML: check-latest job does not check out 'main' branch"
  fi

  if grep -q 'ref: duckdb/1\.4\.x' "$MONITOR_YML"; then
    pass "$MONITOR_YML: check-lts job checks out 'duckdb/1.4.x' branch"
  else
    fail "$MONITOR_YML: check-lts job does not reference 'duckdb/1.4.x'"
  fi

  if grep -q 'base: main' "$MONITOR_YML"; then
    pass "$MONITOR_YML: PRs for check-latest target 'main'"
  else
    fail "$MONITOR_YML: check-latest PRs do not target 'main'"
  fi

  if grep -q 'base: duckdb/1\.4\.x' "$MONITOR_YML"; then
    pass "$MONITOR_YML: PRs for check-lts target 'duckdb/1.4.x'"
  else
    fail "$MONITOR_YML: check-lts PRs do not target 'duckdb/1.4.x'"
  fi

  # Verify schedule trigger exists (weekly cron)
  if grep -q 'schedule:' "$MONITOR_YML"; then
    pass "$MONITOR_YML: has scheduled trigger (cron)"
  else
    fail "$MONITOR_YML: missing 'schedule:' trigger"
  fi

  # Verify manual dispatch exists
  if grep -q 'workflow_dispatch' "$MONITOR_YML"; then
    pass "$MONITOR_YML: has workflow_dispatch for manual triggering"
  else
    fail "$MONITOR_YML: missing 'workflow_dispatch' trigger"
  fi
else
  fail "$MONITOR_YML does not exist"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
TOTAL=$((PASS + FAIL))
echo ""
echo "========================================"
echo "Phase 34 Infrastructure Assertions"
echo "Passed: $PASS / $TOTAL"
echo "========================================"

if [[ "$FAIL" -gt 0 ]]; then
  echo ""
  echo "FAILED assertions:"
  for F in "${FAILURES[@]}"; do
    echo "  - $F"
  done
  echo ""
  echo "NOTE: DKDB-02 (LTS branch test suite passing) is manually-verified only."
  echo "      Run 'git checkout duckdb/1.4.x && just test-all' to verify."
  exit 1
fi

echo ""
echo "All infrastructure assertions green."
echo "NOTE: DKDB-02 (LTS branch test suite passing) is manually-verified only."
echo "      Run 'git checkout duckdb/1.4.x && just test-all' to verify."
exit 0
