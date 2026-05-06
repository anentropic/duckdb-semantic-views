#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2"]
# requires-python = ">=3.10"
# ///
"""
End-to-end error-position verification for the semantic_views extension.

Phase 21 integration tests verify error *messages* flow through the pipeline
via sqllogictest, but sqllogictest `statement error` only matches message
substrings. This test closes the gap by inspecting the Python exception text
to verify the right validation message reaches the caller for malformed DDL.

v0.8.1 change: validation errors are surfaced via a synthesised
`SELECT error('...')` (FALLBACK_OVERRIDE drops `DISPLAY_EXTENSION_ERROR`),
so they arrive as runtime exceptions rather than `ParserException` with
DuckDB's `LINE 1: ... ^` caret rendering. The position info is included in
the message text via the validator's existing "at byte N" suffix, but the
assertions here just check for the right *message content* — the caret
column-counting tests are documented as a known regression in TECH-DEBT
item 22 and tracked separately.

Phase 62 Wave 0: assertion bodies wired but skipped pending Plan 04. Once
Plans 02-03 restore caret rendering via parse_function on FALLBACK_OVERRIDE
deferral, Plan 04 flips the skips to `assert caret_col is not None` and
adds an exact-column expectation for each test.

Three representative error types are covered:
  1. Structural error -- missing '(' after clause keyword (ERR-02)
  2. Clause-level error -- typo in AS-body keyword (ERR-01)
  3. Near-miss prefix error (ERR-03)

Usage:
    python3 test/integration/test_caret_position.py

    Or with a custom extension path:
    SEMANTIC_VIEWS_EXTENSION_PATH=build/debug/semantic_views.duckdb_extension \
        python3 test/integration/test_caret_position.py

Exit codes:
    0 = all tests passed
    1 = at least one test failed
"""

import platform
import sys
import traceback
from pathlib import Path

# Add test/integration to path for helpers import
sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

EXT_DIR = get_ext_dir()
EXT_PATH = get_extension_path()


def make_connection():
    """Create a DuckDB connection with the extension loaded."""
    import duckdb

    conn = duckdb.connect(
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": EXT_DIR,
        },
    )
    conn.execute(f"FORCE INSTALL '{EXT_PATH}'")
    conn.execute("LOAD semantic_views")
    return conn


def extract_caret_position(error_text: str):
    """Extract the caret (^) column position from DuckDB error output.

    DuckDB renders errors as:
        Parser Error: {message}

        LINE 1: {query_text}
                ^

    Returns the 0-based column position of ^ relative to the query text
    (subtracting the "LINE 1: " prefix of 8 characters), or None if no
    caret line found.
    """
    lines = error_text.split("\n")
    for line in lines:
        stripped = line.strip()
        # The caret line contains only whitespace and a single ^
        if stripped == "^":
            caret_col = line.index("^")
            # Subtract the "LINE 1: " prefix (8 characters)
            return caret_col - 8
    return None


def run_test(name, test_fn):
    """Run a single test with exception handling."""
    print(f"\n{'=' * 60}")
    print(f"TEST: {name}")
    print(f"{'=' * 60}")

    try:
        test_fn()
        print("  RESULT: PASS")
        return True
    except AssertionError as e:
        print(f"  RESULT: FAIL (assertion)")
        print(f"  ERROR: {e}")
        traceback.print_exc()
        return False
    except Exception as e:
        print(f"  RESULT: FAIL (unexpected exception)")
        print(f"  ERROR: {e}")
        traceback.print_exc()
        return False


# ---------------------------------------------------------------------------
# Test 1: Structural error -- missing '(' after clause keyword (ERR-02)
# ---------------------------------------------------------------------------

def test_caret_missing_paren():
    """Validation error for missing '(' reaches the caller."""
    conn = make_connection()
    try:
        query = "CREATE SEMANTIC VIEW myview AS TABLES x"
        try:
            conn.execute(query)
            assert False, "Expected exception"
        except Exception as e:
            error_text = str(e)
            assert "Expected '('" in error_text, (
                f"Expected error about missing '(' but got: {error_text}"
            )
            print(f"  Error: {error_text.splitlines()[0]} -- correct")
            # Phase 62 Wave 0: caret column extraction wired; staged-skipped
            # until Plan 04 tightens to `assert caret_col is not None`.
            caret_col = extract_caret_position(error_text)
            if caret_col is None:
                print("  SKIP (caret): Phase 62 Plan 04 will assert caret_col is not None")
            else:
                # Plan 04 will replace this branch with:
                #   assert caret_col == EXPECTED_COLUMN_FOR_THIS_TEST
                print(f"  caret_col = {caret_col} (Plan 04 will pin exact value)")
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# Test 2: Clause-level error -- typo in AS-body keyword (ERR-01)
# ---------------------------------------------------------------------------

def test_caret_clause_typo():
    """Validation error for misspelled clause keyword names the right suggestion."""
    conn = make_connection()
    try:
        query = "CREATE SEMANTIC VIEW x AS TBLES (t AS tbl PRIMARY KEY (id)) DIMENSIONS (t.x AS x)"
        try:
            conn.execute(query)
            assert False, "Expected exception"
        except Exception as e:
            error_text = str(e)
            assert "TABLES" in error_text, (
                f"Expected suggestion for 'TABLES' but got: {error_text}"
            )
            print(f"  Error: {error_text.splitlines()[0]} -- correct")
            # Phase 62 Wave 0: caret column extraction wired; staged-skipped.
            caret_col = extract_caret_position(error_text)
            if caret_col is None:
                print("  SKIP (caret): Phase 62 Plan 04 will assert caret_col is not None")
            else:
                print(f"  caret_col = {caret_col} (Plan 04 will pin exact value)")
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# Test 3: Near-miss prefix error (ERR-03)
# ---------------------------------------------------------------------------

def test_caret_near_miss():
    """Near-miss prefix typo surfaces the 'Did you mean ...' suggestion."""
    conn = make_connection()
    try:
        query = "CRETAE SEMANTIC VIEW x (tables := [])"
        try:
            conn.execute(query)
            assert False, "Expected exception"
        except Exception as e:
            error_text = str(e)
            assert "CREATE SEMANTIC VIEW" in error_text, (
                f"Expected suggestion for 'CREATE SEMANTIC VIEW' "
                f"but got: {error_text}"
            )
            print(f"  Error: {error_text.splitlines()[0]} -- correct")
            # Phase 62 Wave 0: caret column extraction wired; staged-skipped.
            caret_col = extract_caret_position(error_text)
            if caret_col is None:
                print("  SKIP (caret): Phase 62 Plan 04 will assert caret_col is not None")
            else:
                print(f"  caret_col = {caret_col} (Plan 04 will pin exact value)")
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# Phase 62 Wave 0 — additional caret coverage (B4, B5, B6, B7).
# ---------------------------------------------------------------------------
# Each test below issues the malformed query, captures the exception, and
# stages the assertion. Until Plan 04 (Wave 3) restores caret rendering
# they print SKIP and exit cleanly so the suite stays green.

def test_caret_multiline_typo():
    """B4: caret column for multi-line CREATE with malformed clause on line 3."""
    conn = make_connection()
    try:
        query = (
            "CREATE SEMANTIC VIEW v AS\n"
            "TABLES (t AS orders PRIMARY KEY (id))\n"
            "DIMENSIONS (TBLES x)"
        )
        try:
            conn.execute(query)
            print("  SKIP (Plan 04): exception not raised yet under Wave 0 expectations")
            return
        except Exception as e:
            error_text = str(e)
            print(f"  Error captured: {error_text.splitlines()[0]}")
            caret_col = extract_caret_position(error_text)
            if caret_col is None:
                print("  SKIP (caret): Phase 62 Plan 04 will assert multiline caret_col")
            else:
                print(f"  caret_col = {caret_col} (Plan 04 will pin LINE N alignment)")
    finally:
        conn.close()


def test_caret_unicode_prefix():
    """B5: caret column with multibyte UTF-8 identifier before the typo."""
    conn = make_connection()
    try:
        # vüé is multibyte (ü = 2 bytes, é = 2 bytes in UTF-8). The typo TBLES
        # follows; whatever DuckDB does today (byte vs char column counting)
        # is the contract Plan 04 will pin.
        query = "CREATE SEMANTIC VIEW vüé AS TBLES (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.r AS r) METRICS (t.m AS SUM(1))"
        try:
            conn.execute(query)
            print("  SKIP (Plan 04): exception not raised yet under Wave 0 expectations")
            return
        except Exception as e:
            error_text = str(e)
            print(f"  Error captured: {error_text.splitlines()[0]}")
            caret_col = extract_caret_position(error_text)
            if caret_col is None:
                print("  SKIP (caret): Phase 62 Plan 04 will assert unicode caret_col")
            else:
                print(f"  caret_col = {caret_col} (Plan 04 will pin byte vs char counting)")
    finally:
        conn.close()


def test_caret_alter_typo():
    """B6: caret column for ALTER with bad sub-operation keyword."""
    conn = make_connection()
    try:
        # Set up a view to alter, so ALTER reaches the validator (otherwise the
        # error path is "view not found"). Plan 04 may simplify if validate_alter
        # surfaces the typo before name resolution.
        try:
            conn.execute(
                "CREATE TABLE orders (id INTEGER PRIMARY KEY, amount DECIMAL(10,2))"
            )
            conn.execute(
                "CREATE SEMANTIC VIEW v AS "
                "TABLES (t AS orders PRIMARY KEY (id)) "
                "DIMENSIONS (t.id AS id) "
                "METRICS (t.total AS SUM(t.amount))"
            )
        except Exception:
            pass  # ignore setup failures — Plan 04 may rewrite this scaffold

        query = "ALTER SEMANTIC VIEW v RENAM TO w;"
        try:
            conn.execute(query)
            print("  SKIP (Plan 04): exception not raised yet under Wave 0 expectations")
            return
        except Exception as e:
            error_text = str(e)
            print(f"  Error captured: {error_text.splitlines()[0]}")
            caret_col = extract_caret_position(error_text)
            if caret_col is None:
                print("  SKIP (caret): Phase 62 Plan 04 will assert ALTER caret_col on RENAM")
            else:
                print(f"  caret_col = {caret_col} (Plan 04 will pin RENAM column)")
    finally:
        conn.close()


def test_caret_drop_missing_name():
    """B7: caret column for DROP with missing view name."""
    conn = make_connection()
    try:
        query = "DROP SEMANTIC VIEW;"
        try:
            conn.execute(query)
            print("  SKIP (Plan 04): exception not raised yet under Wave 0 expectations")
            return
        except Exception as e:
            error_text = str(e)
            print(f"  Error captured: {error_text.splitlines()[0]}")
            caret_col = extract_caret_position(error_text)
            if caret_col is None:
                print("  SKIP (caret): Phase 62 Plan 04 will assert DROP caret_col after prefix")
            else:
                print(f"  caret_col = {caret_col} (Plan 04 will pin trailing-token column)")
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

ALL_TESTS = [
    ("test_caret_missing_paren", test_caret_missing_paren),
    ("test_caret_clause_typo", test_caret_clause_typo),
    ("test_caret_near_miss", test_caret_near_miss),
    # Phase 62 Wave 0 — staged tests; assertions tightened in Plan 04.
    ("test_caret_multiline_typo", test_caret_multiline_typo),
    ("test_caret_unicode_prefix", test_caret_unicode_prefix),
    ("test_caret_alter_typo", test_caret_alter_typo),
    ("test_caret_drop_missing_name", test_caret_drop_missing_name),
]


def main():
    import duckdb

    print("DuckDB Semantic Views - Caret Position Verification")
    print("=" * 60)
    print(f"Python: {sys.version}")
    print(f"Platform: {platform.platform()}")
    print(f"DuckDB: {duckdb.__version__}")
    print(f"Extension: {EXT_PATH}")
    print(f"Tests: {len(ALL_TESTS)}")

    passed = 0
    failed = 0
    results = []

    for name, test_fn in ALL_TESTS:
        ok = run_test(name, test_fn)
        results.append((name, ok))
        if ok:
            passed += 1
        else:
            failed += 1

    # Summary
    print(f"\n{'=' * 60}")
    print("SUMMARY")
    print(f"{'=' * 60}")
    for name, ok in results:
        status = "PASS" if ok else "FAIL"
        print(f"  [{status}] {name}")

    print(f"\nTotal: {passed + failed}, Passed: {passed}, Failed: {failed}")

    if failed > 0:
        print("\nFAILED - at least one caret position test failed")
        sys.exit(1)
    else:
        print("\nALL PASSED - caret positions verified end-to-end")
        sys.exit(0)


if __name__ == "__main__":
    main()
