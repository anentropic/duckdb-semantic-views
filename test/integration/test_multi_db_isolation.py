#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2"]
# requires-python = ">=3.10"
# ///
"""
Multi-database isolation regression test for the semantic_views extension.

Loading the extension into more than one database in the same process must
keep each database's DDL routing isolated. Pre-fix the C++ shim held a
single file-static `sv_ddl_conn`; the second LOAD overwrote it, so any
DESCRIBE / SHOW SEMANTIC * statement on the first database silently
executed against the second database's connection.

After the fix, the per-load `ddl_conn` lives on `SemanticViewsParserInfo`
and is threaded through `TableFunction::function_info` to `sv_ddl_bind`,
so every DDL routes to the correct database.

Test scenario:
  1. Create two file-backed databases with distinct semantic views.
  2. Run DESCRIBE / list_semantic_views on each connection.
  3. Assert each connection sees its own view and only its own view.

Without the fix, DESCRIBE on the first connection would either return the
second DB's view metadata or raise a confusing error.

Exit codes:
    0 = all tests passed
    1 = at least one test failed
"""

import sys
import tempfile
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path


EXT_DIR = get_ext_dir()
EXT_PATH = get_extension_path()


def make_connection(db_path: str):
    import duckdb

    conn = duckdb.connect(
        db_path,
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": EXT_DIR,
        },
    )
    conn.execute(f"FORCE INSTALL '{EXT_PATH}'")
    conn.execute("LOAD semantic_views")
    return conn


def setup_db(conn, view_name: str, region_value: str):
    conn.execute(
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, "
        "amount DECIMAL(10,2), region VARCHAR)"
    )
    conn.execute(f"INSERT INTO orders VALUES (1, 100.00, '{region_value}')")
    conn.execute(
        f"""
        CREATE SEMANTIC VIEW {view_name} AS
          TABLES (o AS orders PRIMARY KEY (id))
          DIMENSIONS (o.region AS o.region)
          METRICS (o.total AS SUM(o.amount))
        """
    )


def run_test(name, test_fn):
    print(f"\n{'=' * 60}")
    print(f"TEST: {name}")
    print(f"{'=' * 60}")
    try:
        test_fn()
        print("  RESULT: PASS")
        return True
    except AssertionError as e:
        print(f"  RESULT: FAIL\n  {e}")
        return False
    except Exception as e:
        print(f"  RESULT: ERROR\n  {type(e).__name__}: {e}")
        traceback.print_exc()
        return False


def test_multi_db_isolation():
    with tempfile.TemporaryDirectory() as tmpdir:
        db1_path = str(Path(tmpdir) / "db1.duckdb")
        db2_path = str(Path(tmpdir) / "db2.duckdb")

        con1 = make_connection(db1_path)
        con2 = make_connection(db2_path)
        try:
            setup_db(con1, "view_db1", "US")
            setup_db(con2, "view_db2", "EU")

            # list_semantic_views on each connection should only see its own view.
            db1_views = {row[0] for row in con1.execute(
                "SELECT name FROM list_semantic_views()"
            ).fetchall()}
            db2_views = {row[0] for row in con2.execute(
                "SELECT name FROM list_semantic_views()"
            ).fetchall()}
            assert db1_views == {"view_db1"}, (
                f"db1 should see only view_db1, got: {db1_views}"
            )
            assert db2_views == {"view_db2"}, (
                f"db2 should see only view_db2, got: {db2_views}"
            )

            # DESCRIBE goes through the legacy parse_function/sv_ddl_bind
            # path — this is the path that was broken pre-fix.
            db1_desc = con1.execute("DESCRIBE SEMANTIC VIEW view_db1").fetchall()
            assert len(db1_desc) > 0, "db1 DESCRIBE returned no rows"

            db2_desc = con2.execute("DESCRIBE SEMANTIC VIEW view_db2").fetchall()
            assert len(db2_desc) > 0, "db2 DESCRIBE returned no rows"

            # Cross-DB DESCRIBE must error: each view is local to its own DB.
            try:
                con1.execute("DESCRIBE SEMANTIC VIEW view_db2").fetchall()
                raise AssertionError(
                    "con1 should not see view_db2 (cross-DB leak)"
                )
            except AssertionError:
                raise
            except Exception:
                pass  # expected — view_db2 doesn't exist in db1

            try:
                con2.execute("DESCRIBE SEMANTIC VIEW view_db1").fetchall()
                raise AssertionError(
                    "con2 should not see view_db1 (cross-DB leak)"
                )
            except AssertionError:
                raise
            except Exception:
                pass

            # Issuing SHOW SEMANTIC VIEWS alternately on both connections
            # exercises the per-call function_info routing through the
            # legacy parse_function/sv_ddl_bind path. Column 1 of the
            # SHOW result is `name` (column 0 is `created_on`).
            for _ in range(3):
                db1_now = {row[1] for row in con1.execute(
                    "SHOW SEMANTIC VIEWS"
                ).fetchall()}
                db2_now = {row[1] for row in con2.execute(
                    "SHOW SEMANTIC VIEWS"
                ).fetchall()}
                assert db1_now == {"view_db1"}, (
                    f"db1 SHOW should see only view_db1, got: {db1_now}"
                )
                assert db2_now == {"view_db2"}, (
                    f"db2 SHOW should see only view_db2, got: {db2_now}"
                )
        finally:
            con1.close()
            con2.close()


def main():
    print(f"Extension path: {EXT_PATH}")
    print(f"Extension dir:  {EXT_DIR}")

    if not Path(EXT_PATH).exists():
        print(f"ERROR: Extension not found at {EXT_PATH}", file=sys.stderr)
        print("Run `just build` first.", file=sys.stderr)
        sys.exit(1)

    results = [
        run_test("multi-DB DDL isolation", test_multi_db_isolation),
    ]

    print(f"\n{'=' * 60}")
    print(f"SUMMARY: {sum(results)}/{len(results)} passed")
    print(f"{'=' * 60}")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
