#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.4.4"]
# requires-python = ">=3.9"
# ///
"""
End-to-end caret position verification for the semantic_views extension.

Phase 21 integration tests verify error *messages* flow through the pipeline
via sqllogictest, but sqllogictest `statement error` only matches message
substrings -- it cannot assert on the caret line.  This test closes the gap
by using Python `duckdb.ParserException` inspection to verify that DuckDB's
caret (^) renders at the correct character position when malformed DDL flows
through the full extension load pipeline.

Three representative error types are covered:
  1. Structural error -- missing opening paren (ERR-02)
  2. Clause-level error -- typo in keyword (ERR-01)
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
# Test 1: Structural error -- missing opening paren (ERR-02)
# ---------------------------------------------------------------------------

def test_caret_missing_paren():
    """Caret points where '(' is expected after view name."""
    conn = make_connection()
    try:
        query = "CREATE SEMANTIC VIEW myview tables := []"
        try:
            conn.execute(query)
            assert False, "Expected ParserException"
        except Exception as e:
            if "ParserException" not in type(e).__name__:
                raise
            error_text = str(e)
            assert "Expected '('" in error_text, (
                f"Expected error about missing '(' but got: {error_text}"
            )
            pos = extract_caret_position(error_text)
            assert pos is not None, f"No caret found in: {error_text}"
            # The caret should point at the space before 'tables'
            # "CREATE SEMANTIC VIEW myview" = 27 chars, caret at 27
            expected_pos = len("CREATE SEMANTIC VIEW myview")
            assert pos == expected_pos, (
                f"Caret at {pos}, expected {expected_pos}. "
                f"Char at pos: '{query[pos] if pos < len(query) else 'OOB'}'"
            )
            print(f"  Caret position: {pos} (space before 'tables') -- correct")
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# Test 2: Clause-level error -- typo in keyword (ERR-01)
# ---------------------------------------------------------------------------

def test_caret_clause_typo():
    """Caret points at misspelled clause keyword."""
    conn = make_connection()
    try:
        query = "CREATE SEMANTIC VIEW x (tbles := [], dimensions := [])"
        try:
            conn.execute(query)
            assert False, "Expected ParserException"
        except Exception as e:
            if "ParserException" not in type(e).__name__:
                raise
            error_text = str(e)
            assert "tables" in error_text.lower(), (
                f"Expected suggestion for 'tables' but got: {error_text}"
            )
            pos = extract_caret_position(error_text)
            assert pos is not None, f"No caret found in: {error_text}"
            # The caret should point at the 't' of 'tbles' (position 24)
            assert query[pos:pos + 5] == "tbles", (
                f"Caret at {pos}, expected 'tbles' but got "
                f"'{query[pos:pos + 5]}'"
            )
            print(f"  Caret position: {pos} (start of 'tbles') -- correct")
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# Test 3: Near-miss prefix error (ERR-03)
# ---------------------------------------------------------------------------

def test_caret_near_miss():
    """Caret points at start of near-miss prefix."""
    conn = make_connection()
    try:
        query = "CRETAE SEMANTIC VIEW x (tables := [])"
        try:
            conn.execute(query)
            assert False, "Expected ParserException"
        except Exception as e:
            if "ParserException" not in type(e).__name__:
                raise
            error_text = str(e)
            assert "CREATE SEMANTIC VIEW" in error_text, (
                f"Expected suggestion for 'CREATE SEMANTIC VIEW' "
                f"but got: {error_text}"
            )
            pos = extract_caret_position(error_text)
            assert pos is not None, f"No caret found in: {error_text}"
            # Caret should point at position 0 (start of statement)
            assert pos == 0, (
                f"Caret at {pos}, expected 0"
            )
            print(f"  Caret position: {pos} (start of statement) -- correct")
    finally:
        conn.close()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

ALL_TESTS = [
    ("test_caret_missing_paren", test_caret_missing_paren),
    ("test_caret_clause_typo", test_caret_clause_typo),
    ("test_caret_near_miss", test_caret_near_miss),
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
