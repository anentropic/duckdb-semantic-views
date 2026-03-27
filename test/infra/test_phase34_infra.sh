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
# DKDB-01 / DKDB-05: .duckdb-version on main tracks latest (v1.5.0)
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-01/05: .duckdb-version tracks DuckDB 1.5.0 ==="

if [[ -f ".duckdb-version" ]]; then
  pass ".duckdb-version file exists"
  VERSION=$(cat .duckdb-version | tr -d '[:space:]')
  if [[ "$VERSION" == "v1.5.0" ]]; then
    pass ".duckdb-version contains 'v1.5.0' (got: $VERSION)"
  else
    fail ".duckdb-version should be 'v1.5.0' but got '$VERSION'"
  fi
else
  fail ".duckdb-version file does not exist"
fi

# ---------------------------------------------------------------------------
# DKDB-01: Cargo.toml pins duckdb-rs at =1.10500.0 (DuckDB 1.5.0 encoding)
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-01: Cargo.toml duckdb-rs pin encodes DuckDB 1.5.0 ==="

if grep -q 'version = "=1\.10500\.0"' Cargo.toml; then
  pass "Cargo.toml: duckdb crate pinned at =1.10500.0"
else
  fail "Cargo.toml: duckdb crate not pinned at =1.10500.0 (expected '=1.10500.0')"
fi

if grep -q 'libduckdb-sys = "=1\.10500\.0"' Cargo.toml; then
  pass "Cargo.toml: libduckdb-sys pinned at =1.10500.0"
else
  fail "Cargo.toml: libduckdb-sys not pinned at =1.10500.0 (expected '=1.10500.0')"
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
# DKDB-04: Build.yml CI config references DuckDB 1.5.0 and duckdb/* triggers
# ---------------------------------------------------------------------------
echo ""
echo "=== DKDB-04: Build.yml references DuckDB 1.5.0 and duckdb/* triggers ==="

BUILD_YML=".github/workflows/Build.yml"

if [[ -f "$BUILD_YML" ]]; then
  pass "$BUILD_YML exists"

  if grep -q "'duckdb/\*'" "$BUILD_YML" || grep -q '"duckdb/\*"' "$BUILD_YML" || grep -qF "- 'duckdb/*'" "$BUILD_YML"; then
    pass "$BUILD_YML: on.push.branches includes 'duckdb/*' trigger"
  else
    fail "$BUILD_YML: on.push.branches missing 'duckdb/*' trigger"
  fi

  if grep -q '@v1\.5\.0' "$BUILD_YML"; then
    pass "$BUILD_YML: uses extension-ci-tools@v1.5.0"
  else
    fail "$BUILD_YML: does not reference @v1.5.0"
  fi

  if grep -q 'duckdb_version: v1\.5\.0' "$BUILD_YML"; then
    pass "$BUILD_YML: duckdb_version is v1.5.0"
  else
    fail "$BUILD_YML: duckdb_version is not v1.5.0"
  fi

  if grep -q 'ci_tools_version: v1\.5\.0' "$BUILD_YML"; then
    pass "$BUILD_YML: ci_tools_version is v1.5.0"
  else
    fail "$BUILD_YML: ci_tools_version is not v1.5.0"
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
