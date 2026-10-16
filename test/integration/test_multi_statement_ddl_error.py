#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==2.0.0.dev2610151406"]
# requires-python = ">=3.10"
# ///
"""
Semantic-view DDL inside a multi-statement string names the real cause (TECH-DEBT #85).

`parser_override` sees the WHOLE query string and declines a string holding
several statements. The default parser then splits it and hands the
semantic-view statement alone to `parse_function`, which validates it fine and
used to explain that as "parser_override is not active ... Re-enable with: SET
allow_parser_override_extension='FALLBACK'" -- wrong, since the override IS
active, and the advice changes nothing.

Each scenario runs on a fresh in-memory database and reports independently:

  ddl_after_other_statements   CREATE TABLE; INSERT; CREATE SEMANTIC VIEW in one
                               execute() -- the examples/type_inference.py shape.
  ddl_before_other_statement   CREATE SEMANTIC VIEW ...; SELECT 1 in one execute().
  ddl_after_read_call          SELECT * FROM semantic_view(...); CREATE SEMANTIC
                               VIEW ... -- a string the override REWRITES (the
                               read call's search path) rather than declines.
  control_override_disabled    the override really is off (SET
                               allow_parser_override_extension='DEFAULT'), DDL
                               sent alone: the original "not active" message
                               must still be what the user sees.
  control_single_statement     DDL alone with a trailing semicolon still works.
  control_stale_other_database a multi-statement DDL error on one database,
                               then the same DDL alone on another database
                               whose override is off: the second must get the
                               "not active" message, not the first one's.
  control_stale_after_success  the same, but the first database's string was
                               ONE statement that merely holds a `;` (a
                               trailing comment) and succeeded.
  control_stale_after_typo     the same, but the first database's string
                               failed on a typo before reaching its DDL.

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
NEW_HINT = "one statement per call"
OLD_HINT = "parser_override is not active"


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


def check_multi(sql):
    con = connect()
    try:
        if not sql.startswith("CREATE TABLE"):
            con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        msg = error_of(con, sql)
    finally:
        con.close()
    assert msg is not None, "a multi-statement string with semantic-view DDL must still fail"
    assert NEW_HINT in msg, f"error must say {NEW_HINT!r}; got: {msg}"
    assert OLD_HINT not in msg, f"error must not blame the override setting; got: {msg}"


def scenario_ddl_after_other_statements():
    check_multi(
        "CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE); "
        "INSERT INTO orders VALUES (1, 'US', 100); " + DDL + ";"
    )


def scenario_ddl_before_other_statement():
    check_multi(DDL + "; SELECT 1;")


def scenario_ddl_after_read_call():
    con = connect()
    try:
        con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        con.execute(DDL.replace("sales", "existing", 1))
        msg = error_of(
            con, "SELECT * FROM semantic_view('existing', metrics := ['revenue']); " + DDL
        )
    finally:
        con.close()
    assert msg is not None, "a multi-statement string with semantic-view DDL must still fail"
    assert NEW_HINT in msg, f"error must say {NEW_HINT!r}; got: {msg}"
    assert OLD_HINT not in msg, f"error must not blame the override setting; got: {msg}"


def scenario_control_override_disabled():
    con = connect()
    try:
        con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        con.execute("SET allow_parser_override_extension = 'DEFAULT'")
        msg = error_of(con, DDL)
    finally:
        con.close()
    assert msg is not None, "DDL with the override switched off must fail"
    assert OLD_HINT in msg, f"real 'override off' case must keep its message; got: {msg}"
    assert NEW_HINT not in msg, f"real 'override off' case must not blame splitting; got: {msg}"


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


def scenario_control_stale_other_database():
    off, on = connect(), connect()
    try:
        for con in (off, on):
            con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        off.execute("SET allow_parser_override_extension = 'DEFAULT'")
        first = error_of(on, "SELECT 1; " + DDL)
        msg = error_of(off, DDL)
    finally:
        off.close()
        on.close()
    assert first is not None and NEW_HINT in first, f"setup: expected {NEW_HINT!r}; got: {first}"
    assert msg is not None, "DDL with the override switched off must fail"
    assert OLD_HINT in msg, f"override-off database must keep its message; got: {msg}"
    assert NEW_HINT not in msg, f"must not reuse another database's error; got: {msg}"


def check_not_blamed_after(first_sql, expect_first_error):
    off, on = connect(), connect()
    try:
        for con in (off, on):
            con.execute("CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE)")
        off.execute("SET allow_parser_override_extension = 'DEFAULT'")
        first = error_of(on, first_sql)
        msg = error_of(off, DDL)
    finally:
        off.close()
        on.close()
    assert (first is not None) == expect_first_error, f"setup: unexpected first result: {first}"
    assert msg is not None, "DDL with the override switched off must fail"
    assert OLD_HINT in msg, f"override-off database must keep its message; got: {msg}"
    assert NEW_HINT not in msg, f"must not reuse another query's record; got: {msg}"


def scenario_control_stale_after_success():
    check_not_blamed_after(DDL + "; -- created", expect_first_error=False)


def scenario_control_stale_after_typo():
    check_not_blamed_after("SELEC 1; " + DDL, expect_first_error=True)


SCENARIOS = [
    scenario_ddl_after_other_statements,
    scenario_ddl_before_other_statement,
    scenario_ddl_after_read_call,
    scenario_control_override_disabled,
    scenario_control_single_statement,
    scenario_control_stale_other_database,
    scenario_control_stale_after_success,
    scenario_control_stale_after_typo,
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
