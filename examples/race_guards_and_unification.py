#!/usr/bin/env python3
# /// script
# dependencies = [
#   "duckdb==1.5.2",
# ]
# requires-python = ">=3.10"
# ///
"""
uv run examples/race_guards_and_unification.py

Demonstrates v0.8.1 features:
  1. Architectural unification — every DDL form (CREATE / DROP / ALTER /
     DESCRIBE / SHOW) goes through the same parser_override path, with one
     net effect: identical behaviour under DuckDB's Bison and PEG parsers.
  2. DROP / ALTER race guards — a snapshot-consistent existence check on
     the caller's connection guarantees the user sees a clear error if a
     concurrent commit lands between the catalog pre-check and the DML.
  3. IF EXISTS keeps its silent-no-op contract on the same race.
  4. Friendly validation errors flow through DuckDB's FALLBACK_OVERRIDE
     mode, which used to silently drop them.

The race-guard scenario is simulated end-to-end by interleaving two
connections to the same in-process database. We can't actually trigger a
mid-statement commit from another session in a single-process demo, so we
show the error path by executing a DROP against a name that was already
removed in a prior statement on the SAME connection.
"""

import os
import tempfile

import duckdb

EXTENSION_PATH = os.environ.get(
    "SEMANTIC_VIEWS_EXTENSION_PATH",
    "build/debug/semantic_views.duckdb_extension",
)
EXT_DIR = os.path.dirname(os.path.abspath(EXTENSION_PATH))


def open_connection(db_path: str) -> "duckdb.DuckDBPyConnection":
    con = duckdb.connect(
        db_path,
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": EXT_DIR,
        },
    )
    con.execute(f"FORCE INSTALL '{os.path.abspath(EXTENSION_PATH)}'")
    con.execute("LOAD semantic_views")
    return con


def setup_base(con: "duckdb.DuckDBPyConnection") -> None:
    con.execute(
        """
        CREATE TABLE orders (
            id INTEGER PRIMARY KEY,
            region VARCHAR,
            amount DECIMAL(10, 2)
        );
        INSERT INTO orders VALUES
            (1, 'east', 10.00),
            (2, 'east', 20.00),
            (3, 'west', 30.00);
        """
    )


CREATE_VIEW = """
CREATE SEMANTIC VIEW sales AS
TABLES (o AS orders PRIMARY KEY (id))
DIMENSIONS (o.region AS o.region)
METRICS (o.total AS SUM(o.amount))
"""


def section(label: str) -> None:
    print()
    print("=" * 72)
    print(label)
    print("=" * 72)


def main() -> None:
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "v081_demo.duckdb")
        con = open_connection(db_path)
        setup_base(con)
        con.execute(CREATE_VIEW)

        # ------------------------------------------------------------------
        # 1. Unified path: DESCRIBE / SHOW work even after toggling the PEG
        #    parser. Pre-v0.8.1 these forms required Bison.
        # ------------------------------------------------------------------
        section("1. Unified parser path: DESCRIBE / SHOW under both parsers")
        for label, switch in [("Bison (default)", None), ("PEG", "enable_peg_parser")]:
            if switch:
                con.execute(f"CALL {switch}()")
            print(f"\n[{label}] DESCRIBE SEMANTIC VIEW sales (rowcount):")
            rows = con.execute("DESCRIBE SEMANTIC VIEW sales").fetchall()
            print(f"  {len(rows)} property rows")
            print(f"[{label}] SHOW SEMANTIC VIEWS:")
            for row in con.execute("SHOW SEMANTIC VIEWS").fetchall():
                print(f"  {row}")

        # Restore default parser AND the FALLBACK setting that disable_peg
        # resets — see TECH-DEBT item 21.
        con.execute("CALL disable_peg_parser()")
        con.execute("SET allow_parser_override_extension='FALLBACK'")

        # ------------------------------------------------------------------
        # 2. Race guard: DROP a missing name surfaces a clear error.
        # ------------------------------------------------------------------
        section("2. Race-guarded DROP: missing name surfaces a clear error")
        try:
            con.execute("DROP SEMANTIC VIEW does_not_exist")
        except Exception as exc:
            print(f"  caught: {exc}")

        # ------------------------------------------------------------------
        # 3. IF EXISTS: silent no-op for the same input.
        # ------------------------------------------------------------------
        section("3. DROP IF EXISTS: silent no-op for the same input")
        rows = con.execute("DROP SEMANTIC VIEW IF EXISTS does_not_exist").fetchall()
        print(f"  no error raised; result rows: {rows}")

        # ------------------------------------------------------------------
        # 4. Friendly validation error reaches the user.
        #    Pre-v0.8.1 this would have surfaced as
        #    `Parser Error: syntax error at or near "SEMANTIC"` because
        #    DuckDB silently drops DISPLAY_EXTENSION_ERROR in FALLBACK mode.
        # ------------------------------------------------------------------
        section("4. Validation error survives FALLBACK_OVERRIDE")
        try:
            con.execute("ALTER SEMANTIC VIEW sales RENAME TO sales")  # same name
        except Exception as exc:
            print(f"  caught: {exc}")
        try:
            con.execute("CREAT SEMANTIC VIEW typo AS TABLES (o)")  # near-miss
        except Exception as exc:
            print(f"  near-miss caught: {exc}")

        # ------------------------------------------------------------------
        # 5. Two-connection drop. Connection B drops the view; A's
        #    subsequent DROP errors clearly. With auto-commit (this demo)
        #    A's catalog pre-check sees committed state and fires the
        #    "does not exist" path. With a long-lived BEGIN open on A and
        #    a concurrent commit on B between A's pre-check and A's DELETE,
        #    the in-SQL race guard would fire instead and report
        #    "was concurrently dropped" — see the dedicated regression in
        #    test/integration/test_concurrent_ddl.py.
        # ------------------------------------------------------------------
        section("5. Cross-connection DROP: clear error on the second drop")
        con.execute(CREATE_VIEW.replace("sales", "shared_view"))
        con_b = con.cursor()
        con_b.execute("DROP SEMANTIC VIEW shared_view")
        try:
            con.execute("DROP SEMANTIC VIEW shared_view")
        except Exception as exc:
            print(f"  caught on second DROP: {exc}")


if __name__ == "__main__":
    main()
