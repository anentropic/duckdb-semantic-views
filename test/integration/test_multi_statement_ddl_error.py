#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==2.0.0.dev2610151406"]
# requires-python = ">=3.10"
# ///
"""
Semantic-view DDL inside a multi-statement string runs (TECH-DEBT #85).

`parser_override` sees the WHOLE query string and declines a string holding
several statements. DuckDB's grammar then splits it and runs each statement,
the semantic-view ones through the extension's grammar productions. Before
CREATE moved onto the grammar, the parse stub refused such a CREATE with "one
semantic-view DDL statement per call" (and, earlier still, blamed the override
setting).

The grammar bridge learns a statement's source text from the query the override
recorded on this thread, and only when every token matches it. The controls
check that a stale record from another query never lends its text or its caret.

Each scenario runs on a fresh in-memory database and reports independently:

  ddl_after_other_statements   CREATE TABLE; INSERT; CREATE SEMANTIC VIEW in one
                               execute() -- the examples/type_inference.py shape.
  ddl_before_other_statement   CREATE SEMANTIC VIEW ...; SELECT 1 in one execute().
  ddl_after_read_call          SELECT * FROM semantic_view(...); CREATE SEMANTIC
                               VIEW ... -- a string the override REWRITES (the
                               read call's search path) rather than declines.
  error_shows_its_own_line     an invalid CREATE on the second line of a
                               multi-statement string: the caret says LINE 2.
  control_override_disabled    the override really is off (SET
                               allow_parser_override_extension='DEFAULT'), DDL
                               sent alone: it runs through the grammar.
  control_single_statement     DDL alone with a trailing semicolon still works.
  control_stale_caret_query    an invalid DDL in a multi-statement string on
                               one database, then the same invalid DDL alone on
                               another database whose override is off: the
                               second error must not quote the first query.
  control_stale_caret_success  a multi-statement DDL that succeeds through the
                               grammar on one database, then an invalid DDL on
                               another whose override is off: likewise. This is
                               the case that leaves a record behind.
  control_stale_caret_rewrite  the same, but the first DDL is one statement the
                               override rewrites and runs.
  control_stale_caret_parse_error  a query that fails to parse before reaching
                               its DDL on one database, then an invalid DDL on
                               another whose override is off: likewise.

Run: uv run test/integration/test_multi_statement_ddl_error.py (or
`just test-multi-statement-error`).
"""

import sys
from pathlib import Path

import duckdb

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_extension_path

EXT_PATH = get_extension_path()

DDL = (
    "CREATE SEMANTIC VIEW sales AS "
    "TABLES (o AS orders PRIMARY KEY (id)) "
    "DIMENSIONS (o.region AS o.region) "
    "METRICS (o.revenue AS SUM(o.amount))"
)
BAD_DDL = DDL.replace("DIMENSIONS", "DIMENSIONZ", 1)
BAD_MSG = "Unknown clause keyword 'DIMENSIONZ'"


def connect():
    con = duckdb.connect(config={"allow_unsigned_extensions": "true"})
    con.execute(f"LOAD '{EXT_PATH}'")
    return con


def error_of(con, sql):
    try:
        con.execute(sql)
    except duckdb.Error as e:
        return str(e)
    return None


def views(con):
    return sorted(r[0] for r in con.execute("SELECT name FROM list_semantic_views()").fetchall())


def scenario_ddl_after_other_statements():
    con = connect()
    try:
        con.execute(
            "CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE); "
            "INSERT INTO orders VALUES (1, 'US', 100); " + DDL + ";"
        )
        rows = con.execute(
            "SELECT * FROM semantic_view('sales', dimensions := ['region'], metrics := ['revenue'])"
        ).fetchall()
    finally:
        con.close()
    assert rows == [("US", 100.0)], f"every statement must run; got {rows!r}"


def scenario_ddl_before_other_statement():
    con = connect()
    try:
        con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        con.execute(DDL + "; SELECT 1;")
        names = views(con)
    finally:
        con.close()
    assert names == ["sales"], f"the CREATE must run; got {names!r}"


def scenario_ddl_after_read_call():
    con = connect()
    try:
        con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        con.execute(DDL.replace("sales", "existing", 1))
        con.execute("SELECT * FROM semantic_view('existing', metrics := ['revenue']); " + DDL)
        names = views(con)
    finally:
        con.close()
    assert names == ["existing", "sales"], f"the CREATE must run; got {names!r}"


def scenario_error_shows_its_own_line():
    con = connect()
    try:
        msg = error_of(con, "SELECT 1;\n" + BAD_DDL)
    finally:
        con.close()
    assert msg is not None and BAD_MSG in msg, f"expected {BAD_MSG!r}; got: {msg}"
    assert "LINE 2:" in msg, f"the caret must point into line 2; got: {msg}"


def scenario_control_override_disabled():
    con = connect()
    try:
        con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        con.execute("SET allow_parser_override_extension = 'DEFAULT'")
        con.execute(DDL)
        names = views(con)
    finally:
        con.close()
    assert names == ["sales"], f"the CREATE must run through the grammar; got {names!r}"


def scenario_control_single_statement():
    con = connect()
    try:
        con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        con.execute(DDL + ";")
        rows = con.execute(
            "SELECT * FROM semantic_view('sales', dimensions := ['region'], metrics := ['revenue'])"
        ).fetchall()
    finally:
        con.close()
    assert rows == [], f"empty table must give no rows; got {rows!r}"


def check_not_quoted_after(first):
    """Run `first` on one database, then BAD_DDL on another whose override is
    off. The second error must be its own, with no caret from `first` (which
    puts its DDL on line 2)."""
    off, on = connect(), connect()
    try:
        on.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        off.execute("SET allow_parser_override_extension = 'DEFAULT'")
        first(on)
        msg = error_of(off, BAD_DDL)
    finally:
        off.close()
        on.close()
    assert msg is not None and BAD_MSG in msg, f"expected {BAD_MSG!r}; got: {msg}"
    assert "LINE 2:" not in msg, f"must not quote another query; got: {msg}"


def scenario_control_stale_caret_query():
    def first(on):
        msg = error_of(on, "SELECT 'other query';\n" + BAD_DDL)
        assert msg is not None and "LINE 2:" in msg, f"setup: unexpected first error: {msg}"

    check_not_quoted_after(first)


def scenario_control_stale_caret_success():
    # A string the override declines and the grammar runs, with the DDL on its
    # second line. Nothing clears the record after it (an error would: DuckDB
    # then calls the parse stub, which does), so this is the record a later
    # query on the thread finds.
    check_not_quoted_after(lambda on: on.execute("SELECT 1;\n" + DDL))


def scenario_control_stale_caret_rewrite():
    # One statement, on its second line, that the override rewrites and runs.
    check_not_quoted_after(lambda on: on.execute("\n" + DDL))


def scenario_control_stale_caret_parse_error():
    # A query that fails to parse before the override handles any of it, with
    # the DDL on its second line.
    check_not_quoted_after(lambda on: error_of(on, "SELEC 1;\n" + DDL))


SCENARIOS = [
    scenario_ddl_after_other_statements,
    scenario_ddl_before_other_statement,
    scenario_ddl_after_read_call,
    scenario_error_shows_its_own_line,
    scenario_control_override_disabled,
    scenario_control_single_statement,
    scenario_control_stale_caret_query,
    scenario_control_stale_caret_success,
    scenario_control_stale_caret_rewrite,
    scenario_control_stale_caret_parse_error,
]


def main() -> int:
    failed = []
    for scenario in SCENARIOS:
        name = scenario.__name__.removeprefix("scenario_")
        try:
            scenario()
            print(f"ok    {name}")
        except AssertionError as e:
            failed.append(name)
            print(f"FAIL  {name}: {e}")
    print(f"\n{len(SCENARIOS) - len(failed)}/{len(SCENARIOS)} scenarios passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
