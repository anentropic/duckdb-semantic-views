#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2"]
# requires-python = ">=3.10"
# ///
"""
Integration tests for DDL-time type inference of dimension/metric output_type.

Type inference for dimensions and metrics uses the LIMIT 0 query that runs at
DDL time via persist_conn. This only fires on file-backed databases -- in-memory
databases have no persist_conn, so output_type stays None. These tests use a
temp file-backed database to exercise the real inference path.

Verifies:
    1. Dimension output_type inferred (VARCHAR, DATE, TIMESTAMP)
    2. Metric output_type inferred (BIGINT for COUNT/SUM(int), DOUBLE for AVG)
    3. DECIMAL-typed metrics produce empty output_type (avoids lossy CAST)
    4. MIN/MAX preserve source column type
    5. SHOW SEMANTIC DIMENSIONS/METRICS shows inferred data_type
    6. DESCRIBE SEMANTIC VIEW shows DATA_TYPE property rows
    7. In-memory DB produces no type inference
    8. Derived metrics get inferred type
    9. Multi-table views with relationships get correct types

Usage:
    uv run test/integration/test_type_inference.py
"""

import os
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path


EXT_DIR = get_ext_dir()
EXT_PATH = get_extension_path()


def make_file_connection():
    """Create a file-backed DuckDB connection with the extension loaded."""
    import duckdb

    tmpdir = tempfile.mkdtemp()
    db_path = os.path.join(tmpdir, "test_type_inference.duckdb")
    con = duckdb.connect(
        db_path,
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": EXT_DIR,
        },
    )
    con.execute(f"FORCE INSTALL '{EXT_PATH}'")
    con.execute("LOAD semantic_views")
    return con, tmpdir


def make_memory_connection():
    """Create an in-memory DuckDB connection with the extension loaded."""
    import duckdb

    con = duckdb.connect(
        ":memory:",
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": EXT_DIR,
        },
    )
    con.execute(f"FORCE INSTALL '{EXT_PATH}'")
    con.execute("LOAD semantic_views")
    return con


def describe_data_types(con, view_name):
    """Run DESCRIBE SEMANTIC VIEW and return {(object_kind, object_name): data_type} dict.

    DESCRIBE on main returns property-per-row format:
      (object_kind, object_name, parent_entity, property, property_value)
    Filter for DATA_TYPE property rows only.
    """
    rows = con.execute(f"DESCRIBE SEMANTIC VIEW {view_name}").fetchall()
    return {
        (r[0], r[1]): r[4]
        for r in rows
        if r[3] == "DATA_TYPE"
    }


def show_dims_types(con, view_name):
    """Run SHOW SEMANTIC DIMENSIONS IN and return {name: data_type} dict."""
    rows = con.execute(f"SHOW SEMANTIC DIMENSIONS IN {view_name}").fetchall()
    # Columns: (database_name, schema_name, semantic_view_name, table_name,
    #           name, data_type, synonyms, comment)
    return {r[4]: r[5] for r in rows}


def show_metrics_types(con, view_name):
    """Run SHOW SEMANTIC METRICS IN and return {name: data_type} dict."""
    rows = con.execute(f"SHOW SEMANTIC METRICS IN {view_name}").fetchall()
    # Columns: (database_name, schema_name, semantic_view_name, table_name,
    #           name, data_type, synonyms, comment)
    return {r[4]: r[5] for r in rows}


def run_tests():
    import duckdb

    print(f"DuckDB version: {duckdb.__version__}")
    print(f"Extension: {EXT_PATH}")
    print()

    passed = 0
    failed = 0

    # ---- Setup: file-backed DB with varied column types ----
    con, tmpdir = make_file_connection()

    try:
        con.execute("""
            CREATE TABLE orders (
                id INTEGER PRIMARY KEY,
                customer VARCHAR,
                region VARCHAR,
                amount DECIMAL(10,2),
                quantity INTEGER,
                price DOUBLE,
                order_date DATE,
                created_at TIMESTAMP
            )
        """)
        con.execute("""
            INSERT INTO orders VALUES
                (1, 'alice', 'east', 100.50, 5, 20.10, '2024-01-15', '2024-01-15 10:00:00'),
                (2, 'bob',   'west', 200.75, 10, 20.075, '2024-02-10', '2024-02-10 12:00:00'),
                (3, 'alice', 'east', 50.25,  3, 16.75, '2024-03-05', '2024-03-05 14:00:00')
        """)

        con.execute("""
            CREATE TABLE customers (
                id INTEGER PRIMARY KEY,
                name VARCHAR,
                tier INTEGER
            )
        """)
        con.execute("INSERT INTO customers VALUES (1, 'alice', 1), (2, 'bob', 2)")

        con.execute("""
            CREATE SEMANTIC VIEW test_inference AS
            TABLES (o AS orders PRIMARY KEY (id))
            DIMENSIONS (
                o.region       AS o.region,
                o.customer     AS o.customer,
                o.order_date   AS o.order_date,
                o.created_at   AS o.created_at
            )
            METRICS (
                o.order_count  AS COUNT(*),
                o.total_qty    AS SUM(o.quantity),
                o.total_amount AS SUM(o.amount),
                o.avg_price    AS AVG(o.price),
                o.max_date     AS MAX(o.order_date),
                o.min_qty      AS MIN(o.quantity)
            )
        """)

        # ---- Test 1: VARCHAR dimensions via SHOW ----
        print("Test 1: VARCHAR dimensions get inferred data_type via SHOW")
        try:
            dim_types = show_dims_types(con, "test_inference")
            assert dim_types["region"] == "VARCHAR", f"region: {dim_types['region']}"
            assert dim_types["customer"] == "VARCHAR", f"customer: {dim_types['customer']}"
            print(f"  PASS: region=VARCHAR, customer=VARCHAR")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 2: DATE dimension via SHOW ----
        print("Test 2: DATE dimension gets inferred data_type")
        try:
            dim_types = show_dims_types(con, "test_inference")
            assert dim_types["order_date"] == "DATE", f"order_date: {dim_types['order_date']}"
            print(f"  PASS: order_date=DATE")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 3: TIMESTAMP dimension via SHOW ----
        print("Test 3: TIMESTAMP dimension gets inferred data_type")
        try:
            dim_types = show_dims_types(con, "test_inference")
            assert dim_types["created_at"] == "TIMESTAMP", f"created_at: {dim_types['created_at']}"
            print(f"  PASS: created_at=TIMESTAMP")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 4: COUNT(*) -> BIGINT ----
        print("Test 4: COUNT(*) metric infers as BIGINT")
        try:
            met_types = show_metrics_types(con, "test_inference")
            assert met_types["order_count"] == "BIGINT", f"order_count: {met_types['order_count']}"
            print(f"  PASS: order_count=BIGINT")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 5: SUM(integer) -> BIGINT ----
        print("Test 5: SUM(integer) metric infers as BIGINT")
        try:
            met_types = show_metrics_types(con, "test_inference")
            assert met_types["total_qty"] == "BIGINT", f"total_qty: {met_types['total_qty']}"
            print(f"  PASS: total_qty=BIGINT")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 6: SUM(decimal) -> empty (avoids lossy CAST) ----
        print("Test 6: SUM(decimal) stays empty (DECIMAL not set to avoid lossy CAST)")
        try:
            met_types = show_metrics_types(con, "test_inference")
            assert met_types["total_amount"] == "", \
                f"total_amount should be empty, got '{met_types['total_amount']}'"
            print(f"  PASS: total_amount='' (intentionally empty)")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 7: AVG() -> DOUBLE ----
        print("Test 7: AVG() metric infers as DOUBLE")
        try:
            met_types = show_metrics_types(con, "test_inference")
            assert met_types["avg_price"] == "DOUBLE", f"avg_price: {met_types['avg_price']}"
            print(f"  PASS: avg_price=DOUBLE")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 8: MAX(date) -> DATE ----
        print("Test 8: MAX(date) preserves source column type")
        try:
            met_types = show_metrics_types(con, "test_inference")
            assert met_types["max_date"] == "DATE", f"max_date: {met_types['max_date']}"
            print(f"  PASS: max_date=DATE")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 9: MIN(integer) -> INTEGER ----
        print("Test 9: MIN(integer) preserves source column type")
        try:
            met_types = show_metrics_types(con, "test_inference")
            assert met_types["min_qty"] == "INTEGER", f"min_qty: {met_types['min_qty']}"
            print(f"  PASS: min_qty=INTEGER")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 10: DESCRIBE SEMANTIC VIEW shows DATA_TYPE ----
        print("Test 10: DESCRIBE SEMANTIC VIEW shows DATA_TYPE property rows")
        try:
            dt = describe_data_types(con, "test_inference")
            assert ("DIMENSION", "region") in dt, f"Missing DIMENSION region DATA_TYPE row"
            assert dt[("DIMENSION", "region")] == "VARCHAR", \
                f"region: {dt[('DIMENSION', 'region')]}"
            assert ("METRIC", "order_count") in dt, f"Missing METRIC order_count DATA_TYPE row"
            assert dt[("METRIC", "order_count")] == "BIGINT", \
                f"order_count: {dt[('METRIC', 'order_count')]}"
            assert ("DIMENSION", "order_date") in dt
            assert dt[("DIMENSION", "order_date")] == "DATE"
            print(f"  PASS: DESCRIBE shows populated DATA_TYPE rows")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 11: Query results unchanged ----
        print("Test 11: Query results unchanged with inferred types")
        try:
            result = con.execute("""
                SELECT * FROM semantic_view(
                    'test_inference',
                    dimensions := ['region'],
                    metrics := ['order_count', 'total_qty', 'avg_price']
                )
            """).fetchall()
            assert len(result) == 2, f"Expected 2 rows (east/west), got {len(result)}"
            for row in result:
                assert isinstance(row[0], str), f"region should be str: {type(row[0])}"
                assert isinstance(row[1], int), f"order_count should be int: {type(row[1])}"
                assert isinstance(row[2], int), f"total_qty should be int: {type(row[2])}"
                assert isinstance(row[3], float), f"avg_price should be float: {type(row[3])}"
            print(f"  PASS: {result}")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 12: Derived metrics get inferred type ----
        print("Test 12: Derived metric gets inferred data_type")
        try:
            con.execute("""
                CREATE SEMANTIC VIEW derived_test AS
                TABLES (o AS orders PRIMARY KEY (id))
                DIMENSIONS (o.region AS o.region)
                METRICS (
                    o.order_count AS COUNT(*),
                    o.total_qty   AS SUM(o.quantity),
                    avg_qty       AS total_qty / order_count
                )
            """)
            met_types = show_metrics_types(con, "derived_test")
            dt = met_types.get("avg_qty", "")
            assert dt != "", f"Derived metric should have data_type, got empty"
            print(f"  PASS: avg_qty={dt}")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Test 13: Multi-table view with joins ----
        print("Test 13: Multi-table view with relationships gets correct types")
        try:
            con.execute("""
                CREATE SEMANTIC VIEW multi_test AS
                TABLES (
                    o AS orders PRIMARY KEY (id),
                    c AS customers PRIMARY KEY (id)
                )
                RELATIONSHIPS (
                    order_customer AS o(quantity) REFERENCES c
                )
                DIMENSIONS (
                    c.cust_name AS c.name,
                    o.order_date AS o.order_date
                )
                METRICS (
                    o.cnt AS COUNT(*)
                )
            """)
            dim_types = show_dims_types(con, "multi_test")
            met_types = show_metrics_types(con, "multi_test")
            assert dim_types["cust_name"] == "VARCHAR", \
                f"cust_name: expected VARCHAR, got '{dim_types['cust_name']}'"
            assert dim_types["order_date"] == "DATE", \
                f"order_date: expected DATE, got '{dim_types['order_date']}'"
            assert met_types["cnt"] == "BIGINT", \
                f"cnt: expected BIGINT, got '{met_types['cnt']}'"
            print(f"  PASS: cust_name=VARCHAR, order_date=DATE, cnt=BIGINT")
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1

        # ---- Cleanup ----
        print()
        print("Cleanup: dropping semantic views")
        for v in ["test_inference", "derived_test", "multi_test"]:
            try:
                con.execute(f"DROP SEMANTIC VIEW {v}")
            except Exception:
                pass

    finally:
        con.close()
        shutil.rmtree(tmpdir, ignore_errors=True)

    # ---- Test 14: In-memory DB produces no type inference ----
    print("Test 14: In-memory DB produces empty data_type (no persist_conn)")
    try:
        mem_con = make_memory_connection()
        mem_con.execute("CREATE TABLE mem_t (id INTEGER PRIMARY KEY, region VARCHAR, qty INTEGER)")
        mem_con.execute("INSERT INTO mem_t VALUES (1, 'east', 5)")
        mem_con.execute("""
            CREATE SEMANTIC VIEW mem_test AS
            TABLES (o AS mem_t PRIMARY KEY (id))
            DIMENSIONS (o.region AS o.region)
            METRICS (o.total AS SUM(o.qty))
        """)
        dim_types = show_dims_types(mem_con, "mem_test")
        met_types = show_metrics_types(mem_con, "mem_test")
        assert dim_types["region"] == "", f"In-memory dim should be empty, got '{dim_types['region']}'"
        assert met_types["total"] == "", f"In-memory metric should be empty, got '{met_types['total']}'"
        mem_con.close()
        print(f"  PASS: dim='', metric='' (both empty as expected)")
        passed += 1
    except Exception as e:
        print(f"  FAIL: {e}")
        failed += 1

    # ---- Summary ----
    print()
    print(f"Results: {passed} passed, {failed} failed, {passed + failed} total")
    if failed > 0:
        print("FAILED")
        sys.exit(1)
    else:
        print("ALL PASSED")


if __name__ == "__main__":
    run_tests()
