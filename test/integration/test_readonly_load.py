#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.3"]
# requires-python = ">=3.10"
# ///
"""
Read-only database LOAD regression tests for the semantic_views extension.

Phase 63 (v0.9.0) adds support for `LOAD semantic_views` against a read-only
DuckDB database. Prior to this milestone, LOAD failed at init_catalog's
`CREATE SCHEMA IF NOT EXISTS semantic_layer` with DuckDB's read-only error,
making it impossible to query a previously-defined semantic view from a
read-only DB (Iceberg snapshots, archival files, etc.).

Test scenarios (cover RO-01 through RO-05):
  (a) test_fresh_readonly_empty_list      — fresh read-only file, no
      _definitions table; LOAD succeeds; list_semantic_views() -> [];
      missing-view lookups produce a graceful "does not exist" / "not found"
      error (substring match — see RESEARCH §4 acceptance flexibility).
      Covers: RO-01, RO-03, RO-04.
  (b) test_bootstrapped_readonly_query_works — bootstrap a view writable,
      close, reopen read-only; LOAD succeeds; list/describe/semantic_view
      all return correct results.
      Covers: RO-01, RO-02.
  (c) test_readonly_ddl_fails             — bootstrapped read-only DB;
      CREATE/DROP/ALTER each fail with DuckDB's "read-only" error.
      Covers: RO-05.

Note on bootstrap-then-RO pattern: scenarios (b) and (c) bootstrap the DB
in a SUBPROCESS rather than the main process. Phase 62's OverrideContext
attaches a long-lived catalog connection to DBConfig that keeps the
DuckDB Database alive past the user's connection close, which prevents
reopening the same DB read-only in the SAME process (the open hangs on
the in-process database manager). Real users separate bootstrap and
read-only phases across process boundaries (CI build vs query runtime),
so the subprocess pattern reflects realistic usage. The same-process
RW-then-RO regression is logged in deferred-items.md as a follow-up
(Phase 64+ candidate).

Exit codes:
    0 = all tests passed
    1 = at least one test failed
"""

import subprocess
import sys
import tempfile
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path


EXT_DIR = get_ext_dir()
EXT_PATH = get_extension_path()


def open_writable(db_path: str):
    """Open a writable DuckDB connection with the extension installed and loaded."""
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


def open_readonly(db_path: str):
    """Open a read-only DuckDB connection and LOAD the extension.

    Note: FORCE INSTALL is unnecessary here — the extension is already
    installed by open_writable() in the same EXT_DIR cache.
    """
    import duckdb

    conn = duckdb.connect(
        db_path,
        read_only=True,
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": EXT_DIR,
        },
    )
    # Phase 63: this is the line under test. Pre-fix, this raised
    # `Cannot execute statement of type "CREATE_SCHEMA" ... in read-only mode`.
    conn.execute("LOAD semantic_views")
    return conn


def bootstrap_in_subprocess(db_path: str, ddl_statements: list[str]) -> None:
    """Bootstrap a DuckDB file in a subprocess so the parent stays clean.

    Phase 62's OverrideContext is attached per-DBConfig and keeps the
    catalog connection alive until process exit (intentional leak — see
    Phase 62 RESEARCH §Q2). After loading the extension into a writable
    in-process connection, the same DB cannot be reopened read-only in
    the SAME process (the open hangs because the Database is still
    referenced). Running bootstrap in a subprocess sidesteps this by
    letting the OS reclaim the DBConfig at process exit, releasing the
    file lock so the parent can reopen RO cleanly. This mirrors real
    deployments where bootstrap (CI/build job) and read-only query
    (production worker) are different processes.
    """
    script_lines = [
        "import duckdb",
        f"conn = duckdb.connect({db_path!r}, config={{"
        f'"allow_unsigned_extensions": "true", '
        f'"extension_directory": {str(EXT_DIR)!r}}})',
        f"conn.execute(\"FORCE INSTALL '{str(EXT_PATH)}'\")",
        "conn.execute('LOAD semantic_views')",
    ]
    for stmt in ddl_statements:
        script_lines.append(f"conn.execute({stmt!r})")
    script_lines.append("conn.close()")
    script = "\n".join(script_lines) + "\n"
    result = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"bootstrap subprocess failed (exit={result.returncode}):\n"
            f"STDOUT: {result.stdout}\nSTDERR: {result.stderr}"
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


def _missing_view_error_substrings():
    """Acceptable substrings for "view not found" errors across reader paths.

    Per RESEARCH §4 (Coverage strategy — RO-05 acceptance flexibility),
    tests must not pin to one DuckDB error sentence. The reader paths use
    two distinct phrasings:
      - `describe_semantic_view`, SHOW commands, GET_DDL: "does not exist"
        (10 sites confirmed in src/ddl/* — see RESEARCH §3 Q4).
      - `semantic_view(...)` table function (src/query/error.rs:42):
        "Semantic view 'X' not found."
    Both are acceptable as a graceful catalog-miss surfacing.
    """
    return ("does not exist", "not found")


def _assert_missing_view_error(err_msg: str):
    subs = _missing_view_error_substrings()
    assert any(s in err_msg for s in subs), (
        f"expected one of {subs} in error, got: {err_msg}"
    )


def test_fresh_readonly_empty_list():
    """RO-01 / RO-03 / RO-04: fresh read-only DB, no _definitions table."""
    import duckdb

    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "fresh.duckdb")
        # RESEARCH §9 Risk 5: empty zero-byte files cannot be opened
        # read-only. Bootstrap a valid DuckDB header by opening writable
        # and closing — but do NOT load the extension, so semantic_layer
        # schema is never created.
        bootstrap = duckdb.connect(db)
        bootstrap.execute("SELECT 1")
        bootstrap.close()

        ro = open_readonly(db)
        try:
            # RO-03: list_semantic_views() returns empty (not a catalog error).
            rows = ro.execute("SELECT name FROM list_semantic_views()").fetchall()
            assert rows == [], f"expected empty list, got: {rows}"

            # RO-04: describe_semantic_view('missing') -> graceful catalog miss.
            try:
                ro.execute("FROM describe_semantic_view('missing')").fetchall()
                raise AssertionError("describe_semantic_view should have failed for missing view")
            except duckdb.Error as e:
                _assert_missing_view_error(str(e))

            # RO-04 sub: FROM semantic_view('missing', ...) -> graceful catalog miss.
            try:
                ro.execute(
                    "SELECT * FROM semantic_view('missing', dimensions := ['x'])"
                ).fetchall()
                raise AssertionError("semantic_view should have failed for missing view")
            except duckdb.Error as e:
                _assert_missing_view_error(str(e))
        finally:
            ro.close()


def test_bootstrapped_readonly_query_works():
    """RO-01 / RO-02: bootstrap writable, reopen read-only, query works.

    Bootstraps in a SUBPROCESS so the parent's in-process DBConfig leak
    (Phase 62 OverrideContext) doesn't keep the file locked. See module
    docstring + bootstrap_in_subprocess() for full rationale.
    """
    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "bootstrapped.duckdb")
        bootstrap_in_subprocess(
            db,
            [
                "CREATE TABLE orders ("
                "  id INTEGER PRIMARY KEY,"
                "  region VARCHAR,"
                "  amount DECIMAL(10,2)"
                ")",
                "INSERT INTO orders VALUES (1, 'US', 100), (2, 'EU', 200), (3, 'US', 50)",
                "CREATE SEMANTIC VIEW v AS "
                "  TABLES (o AS orders PRIMARY KEY (id)) "
                "  DIMENSIONS (o.region AS o.region) "
                "  METRICS (o.total AS SUM(o.amount))",
            ],
        )

        ro = open_readonly(db)
        try:
            # RO-02: list_semantic_views() returns the bootstrapped view.
            names = [r[0] for r in ro.execute(
                "SELECT name FROM list_semantic_views()"
            ).fetchall()]
            assert names == ["v"], f"expected ['v'], got: {names}"

            # RO-02: describe_semantic_view returns rows.
            desc = ro.execute("FROM describe_semantic_view('v')").fetchall()
            assert len(desc) > 0, "describe_semantic_view should return metadata rows"

            # RO-02: semantic_view returns aggregated rows.
            rows = ro.execute(
                "SELECT region, total FROM semantic_view("
                "  'v', dimensions := ['region'], metrics := ['total']"
                ") ORDER BY region"
            ).fetchall()
            regions = {r[0] for r in rows}
            assert regions == {"EU", "US"}, f"expected {{EU, US}}, got: {regions}"
        finally:
            ro.close()


def test_readonly_ddl_fails():
    """RO-05: CREATE / DROP / ALTER on bootstrapped read-only DB -> DuckDB read-only error.

    Bootstraps in a SUBPROCESS so the parent's in-process DBConfig leak
    (Phase 62 OverrideContext) doesn't keep the file locked. See module
    docstring + bootstrap_in_subprocess() for full rationale.
    """
    import duckdb

    with tempfile.TemporaryDirectory() as tmp:
        db = str(Path(tmp) / "ddl_ro.duckdb")
        bootstrap_in_subprocess(
            db,
            [
                "CREATE TABLE orders (id INTEGER PRIMARY KEY, amount DECIMAL(10,2))",
                "CREATE SEMANTIC VIEW v AS "
                "  TABLES (o AS orders PRIMARY KEY (id)) "
                "  DIMENSIONS (o.id AS o.id) "
                "  METRICS (o.t AS SUM(o.amount))",
            ],
        )

        ro = open_readonly(db)
        try:
            # RO-05: DROP on existing view → DELETE on caller fails read-only.
            try:
                ro.execute("DROP SEMANTIC VIEW v")
                raise AssertionError("DROP should fail on read-only DB")
            except duckdb.Error as e:
                msg = str(e).lower()
                assert "read-only" in msg, f"expected 'read-only' substring, got: {e}"

            # RO-05: ALTER RENAME on existing view → UPDATE fails read-only.
            try:
                ro.execute("ALTER SEMANTIC VIEW v RENAME TO w")
                raise AssertionError("ALTER should fail on read-only DB")
            except duckdb.Error as e:
                msg = str(e).lower()
                assert "read-only" in msg, f"expected 'read-only' substring, got: {e}"

            # RO-05: ALTER SET COMMENT on existing view → UPDATE fails read-only.
            try:
                ro.execute("ALTER SEMANTIC VIEW v SET COMMENT = 'hi'")
                raise AssertionError("ALTER SET COMMENT should fail on read-only DB")
            except duckdb.Error as e:
                msg = str(e).lower()
                assert "read-only" in msg, f"expected 'read-only' substring, got: {e}"

            # RO-05: CREATE new view → INSERT fails read-only.
            try:
                ro.execute(
                    "CREATE SEMANTIC VIEW w AS "
                    "  TABLES (o AS orders PRIMARY KEY (id)) "
                    "  DIMENSIONS (o.id AS o.id) "
                    "  METRICS (o.c AS COUNT(*))"
                )
                raise AssertionError("CREATE should fail on read-only DB")
            except duckdb.Error as e:
                msg = str(e).lower()
                assert "read-only" in msg, f"expected 'read-only' substring, got: {e}"
        finally:
            ro.close()


if __name__ == "__main__":
    results = [
        run_test("test_fresh_readonly_empty_list", test_fresh_readonly_empty_list),
        run_test("test_bootstrapped_readonly_query_works", test_bootstrapped_readonly_query_works),
        run_test("test_readonly_ddl_fails", test_readonly_ddl_fails),
    ]
    passed = sum(results)
    total = len(results)
    print(f"\n{'=' * 60}")
    print(f"SUMMARY: {passed}/{total} tests passed")
    print(f"{'=' * 60}")
    sys.exit(0 if passed == total else 1)
