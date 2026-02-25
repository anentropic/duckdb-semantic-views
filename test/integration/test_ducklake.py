#!/usr/bin/env python3
"""DuckLake/Iceberg integration test for the semantic_views extension.

Verifies that semantic_query works correctly against DuckLake-managed
tables (which use the Iceberg table format under the hood). This proves
the extension handles non-standard table sources end-to-end.

Prerequisites:
    Run `just setup-ducklake` first to download jaffle-shop data and
    create the DuckLake catalog.

Usage:
    python3 test/integration/test_ducklake.py

Exit codes:
    0 = all assertions passed
    1 = test failure or setup error

Requirement: TEST-04 (integration test with Apache Iceberg table source)
"""

import os
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent

# DuckLake catalog paths (must match setup_ducklake.py)
DATA_DIR = PROJECT_ROOT / "test" / "data"
CATALOG_DB = DATA_DIR / "test_catalog.duckdb"
DUCKLAKE_FILE = DATA_DIR / "jaffle.ducklake"
JAFFLE_DATA_DIR = DATA_DIR / "jaffle_data"

# Extension path
EXTENSION_PATH = PROJECT_ROOT / "build" / "debug" / "semantic_views.duckdb_extension"


def check_prerequisites():
    """Verify all required files exist."""
    missing = []
    if not CATALOG_DB.exists():
        missing.append(str(CATALOG_DB))
    if not DUCKLAKE_FILE.exists():
        missing.append(str(DUCKLAKE_FILE))
    if not EXTENSION_PATH.exists():
        missing.append(str(EXTENSION_PATH))

    if missing:
        print("ERROR: Missing prerequisites:")
        for m in missing:
            print(f"  - {m}")
        print()
        print("Run the following first:")
        print("  just build          # Build the extension")
        print("  just setup-ducklake # Download data and create DuckLake catalog")
        sys.exit(1)


def run_tests():
    """Run the DuckLake integration tests."""
    import duckdb

    print(f"DuckDB version: {duckdb.__version__}")
    print(f"Extension: {EXTENSION_PATH}")
    print(f"DuckLake catalog: {DUCKLAKE_FILE}")
    print()

    # Connect to the catalog database (which has DuckLake already set up).
    # Use the project-local extension directory to avoid needing ~/.duckdb.
    ext_dir = str(DATA_DIR / "duckdb_extensions")
    con = duckdb.connect(str(CATALOG_DB), config={
        "allow_unsigned_extensions": "true",
        "extension_directory": ext_dir,
    })

    # Load extensions
    con.execute(f"INSTALL '{EXTENSION_PATH}'")
    con.execute("LOAD semantic_views")
    con.execute("LOAD ducklake")

    # Attach the DuckLake catalog
    ducklake_uri = f"ducklake:{DUCKLAKE_FILE}"
    data_path = str(JAFFLE_DATA_DIR) + "/"
    con.execute(
        f"ATTACH '{ducklake_uri}' AS jaffle (DATA_PATH '{data_path}')"
    )

    # Verify DuckLake tables are accessible
    tables = con.execute(
        "SELECT table_name FROM information_schema.tables "
        "WHERE table_catalog = 'jaffle' ORDER BY table_name"
    ).fetchall()
    table_names = [t[0] for t in tables]
    print(f"DuckLake tables: {table_names}")
    assert "raw_orders" in table_names, "raw_orders table not found in DuckLake"
    assert "raw_customers" in table_names, "raw_customers table not found in DuckLake"

    passed = 0
    failed = 0

    # ---- Test 1: Define semantic view over DuckLake table ----
    print()
    print("Test 1: Define semantic view over DuckLake/Iceberg table")
    try:
        con.execute("""
            SELECT define_semantic_view(
                'jaffle_orders',
                '{"base_table":"jaffle.raw_orders","dimensions":[{"name":"status","expr":"status"}],"metrics":[{"name":"order_count","expr":"count(*)"},{"name":"total_orders","expr":"count(id)"}]}'
            )
        """)
        print("  PASS: View defined successfully")
        passed += 1
    except Exception as e:
        print(f"  FAIL: {e}")
        failed += 1

    # ---- Test 2: Query the DuckLake-backed semantic view ----
    print()
    print("Test 2: Query DuckLake-backed semantic view")
    try:
        result = con.execute("""
            SELECT * FROM semantic_query(
                'jaffle_orders',
                dimensions := ['status'],
                metrics := ['order_count']
            )
        """).fetchall()
        print(f"  Result: {result}")
        assert len(result) > 0, "Expected at least one row"
        # Verify the result has status and order_count columns
        statuses = {row[0] for row in result}
        print(f"  Statuses found: {statuses}")
        assert len(statuses) > 0, "Expected at least one distinct status"
        print("  PASS: Query returned correct results")
        passed += 1
    except Exception as e:
        print(f"  FAIL: {e}")
        failed += 1

    # ---- Test 3: Metrics-only query (global aggregate) ----
    print()
    print("Test 3: Global aggregate over DuckLake table")
    try:
        result = con.execute("""
            SELECT * FROM semantic_query(
                'jaffle_orders',
                metrics := ['order_count', 'total_orders']
            )
        """).fetchall()
        print(f"  Result: {result}")
        assert len(result) == 1, f"Expected 1 row, got {len(result)}"
        order_count = int(result[0][0])
        total_orders = int(result[0][1])
        assert order_count > 0, f"Expected positive order_count, got {order_count}"
        assert order_count == total_orders, (
            f"count(*) and count(id) should match: {order_count} vs {total_orders}"
        )
        print(f"  Total orders: {order_count}")
        print("  PASS: Global aggregate returned correct results")
        passed += 1
    except Exception as e:
        print(f"  FAIL: {e}")
        failed += 1

    # ---- Test 4: Explain on DuckLake-backed view ----
    print()
    print("Test 4: Explain on DuckLake-backed semantic view")
    try:
        result = con.execute("""
            SELECT * FROM explain_semantic_view(
                'jaffle_orders',
                dimensions := ['status'],
                metrics := ['order_count']
            )
        """).fetchall()
        lines = [row[0] for row in result]
        explain_text = "\n".join(lines)
        assert "jaffle_orders" in explain_text, "Expected view name in explain output"
        assert "raw_orders" in explain_text, "Expected base table in explain output"
        print(f"  Explain output: {len(lines)} lines")
        print("  PASS: Explain output contains expected metadata")
        passed += 1
    except Exception as e:
        print(f"  FAIL: {e}")
        failed += 1

    # ---- Cleanup ----
    print()
    print("Cleanup: dropping semantic view")
    try:
        con.execute("SELECT drop_semantic_view('jaffle_orders')")
    except Exception:
        pass  # Best-effort cleanup

    con.close()

    # ---- Summary ----
    print()
    print(f"Results: {passed} passed, {failed} failed, {passed + failed} total")
    if failed > 0:
        print("FAILED")
        sys.exit(1)
    else:
        print("ALL PASSED")


def main():
    check_prerequisites()
    run_tests()


if __name__ == "__main__":
    main()
