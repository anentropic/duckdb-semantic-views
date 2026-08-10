#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.5"]
# requires-python = ">=3.10"
# ///
"""
Regression test: `duckdb_interrupt()` must actually stop a `semantic_view()` query.

`semantic_view()` runs its expanded SQL eagerly inside `init_global`
(`cpp/src/shim.cpp`), on a *fresh* `Connection(*context.db)` — and DuckDB's
interrupt flag is per-`ClientContext`. `con.interrupt()` (and therefore ADBC's
`adbc_cancel()`, the CLI's Ctrl-C, and any framework timeout built on them) sets
the flag on the *caller's* context; pre-fix the inner query polled its own,
which nothing ever set. The inner query therefore ran to full completion and the
interrupt only surfaced afterwards, when the outer pipeline resumed.

Post-fix `init_global` drives the inner query through `PendingQuery` +
`ExecuteTask()` and polls the OUTER context's `interrupted` flag between tasks,
forwarding the cancel to the inner connection and raising a bare
`InterruptException`.

Two properties are asserted, because the pre-fix build satisfies one of them:

  1. TIMING — the query must abort shortly after the interrupt fires, not after
     the inner aggregate completes. This is the property that was broken; the
     pre-fix build reports INTERRUPT only after running the full query.
  2. ERROR SHAPE — the error must be a bare `Interrupted!`, NOT wrapped in
     `semantic_view: SQL execution failed: ...`. ADBC maps to
     ADBC_STATUS_CANCELLED on an exact string match against
     `InterruptException::INTERRUPT_MESSAGE`; a wrapped message would surface a
     cancelled query as ADBC_STATUS_INTERNAL.

A plain-SQL control runs first through the identical harness. It aborts early on
*every* build (DuckDB's own executor polls the flag correctly), so if the control
fails, the harness is wrong rather than the feature — the failure message says so.

The workload is self-calibrating: the dimension expression is a chain of `md5()`
calls whose depth doubles until one uninterrupted `semantic_view()` run costs at
least TARGET_BASELINE_S. Cost is scaled through the *expression* rather than the
row count so the fact table stays small and the grouped result stays 256 rows —
what is being timed is the inner aggregate, not result materialisation.

Usage:
    uv run test/integration/test_query_interrupt.py

Exit codes:
    0 = all scenarios passed
    1 = at least one scenario failed
"""

from __future__ import annotations

import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path

ROWS = 2_000_000

# Calibration: keep doubling the md5-chain depth until one uninterrupted
# semantic_view() run costs at least TARGET_BASELINE_S. MAX_DEPTH caps runtime.
START_DEPTH = 8
MAX_DEPTH = 128
TARGET_BASELINE_S = 3.0

# Below this the gap between "aborted early" and "ran to completion" is too
# small to distinguish from scheduling noise, and the test would pass
# vacuously against a pre-fix build. Hard failure, not a silent skip.
MIN_BASELINE_S = 1.5

# When the interrupt fires, relative to the start of the query.
FIRE_AT_S = 0.4

# An interrupted run must finish within this fraction of the uninterrupted one.
# A regressed build reports only after the inner query completes, landing at
# ~1.0; a working build stops as soon as the in-flight task ends. Placed to
# leave room for a task that takes as long as the whole abort has been observed
# to take under load, while still failing anything that ran to completion.
ABORT_FRACTION = 0.6

# Floor under the fraction above: never require an abort sooner than the fire
# delay plus one `ExecuteTask()` chunk, however short the calibrated baseline.
MIN_ABORT_SLACK_S = 1.5

# The abort limit must stay this far below the uninterrupted baseline, or the
# assertion cannot tell a working build from one that ran to completion. Checked
# against the measured baseline, so a machine where the floor dominates fails
# loudly instead of passing vacuously.
VACUITY_MARGIN = 0.75


def make_connection():
    """Create an in-memory DuckDB connection with the extension loaded."""
    import duckdb

    con = duckdb.connect(
        ":memory:",
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": get_ext_dir(),
        },
    )
    con.execute(f"FORCE INSTALL '{get_extension_path()}'")
    con.execute("LOAD semantic_views")
    # The progress bar kicks in on queries past ~2s and would spray redraws
    # through the test output.
    con.execute("SET enable_progress_bar = false")
    return con


def bucket_expr(depth):
    """A CPU-heavy but low-cardinality grouping key: substr(md5(md5(...)), 1, 2)."""
    expr = "CAST(f.id AS VARCHAR)"
    for _ in range(depth):
        expr = f"md5({expr})"
    return f"substr({expr}, 1, 2)"


SV_SQL = """
    SELECT count(*) FROM semantic_view(
        'iq_view',
        dimensions := ['f.bucket'],
        metrics := ['f.total']
    )
"""


def plain_sql(depth):
    """Hand-written twin of what the semantic view expands to.

    Used as the control: DuckDB's own executor polls the interrupt flag, so this
    aborts early on every build, pre-fix and post-fix alike.
    """
    return f"""
        SELECT count(*) FROM (
            SELECT {bucket_expr(depth)} AS bucket, sum(f.amount) AS total
            FROM iq_facts f
            GROUP BY 1
        )
    """


def build_table(con):
    con.execute(
        f"""
        CREATE OR REPLACE TABLE iq_facts AS
        SELECT i AS id, (i % 1000)::DOUBLE AS amount
        FROM range({ROWS}) t(i)
        """
    )


def build_view(con, depth):
    con.execute("DROP SEMANTIC VIEW IF EXISTS iq_view")
    con.execute(
        f"""
        CREATE SEMANTIC VIEW iq_view AS
          TABLES (f AS iq_facts PRIMARY KEY (id))
          DIMENSIONS (f.bucket AS {bucket_expr(depth)})
          METRICS (f.total AS sum(f.amount))
        """
    )


def time_query(con, sql):
    """Run `sql` to completion, returning wall-clock seconds."""
    start = time.perf_counter()
    con.execute(sql).fetchall()
    return time.perf_counter() - start


def run_interrupted(con, sql, fire_at=FIRE_AT_S):
    """
    Run `sql`, firing con.interrupt() from another thread `fire_at` seconds in.

    Returns (elapsed_seconds, exception_or_None).
    """
    timer = threading.Timer(fire_at, con.interrupt)
    timer.start()
    start = time.perf_counter()
    try:
        con.execute(sql).fetchall()
        err = None
    except Exception as exc:  # noqa: BLE001 - the exception IS the observation
        err = exc
    elapsed = time.perf_counter() - start
    timer.cancel()
    # If the query somehow beat the timer, the flag may still be pending; a
    # throwaway statement clears it (DuckDB resets `interrupted` per query).
    try:
        con.execute("SELECT 1").fetchall()
    except Exception:  # noqa: BLE001 - swallowing a stale pending interrupt
        con.execute("SELECT 1").fetchall()
    return elapsed, err


def calibrate(con):
    """Grow the md5 chain until one uninterrupted semantic_view() run is slow enough."""
    depth = START_DEPTH
    while True:
        build_view(con, depth)
        baseline = time_query(con, SV_SQL)
        print(f"  calibration: md5 depth {depth:>4} -> semantic_view() baseline {baseline:.2f}s")
        if baseline >= TARGET_BASELINE_S or depth >= MAX_DEPTH:
            return depth, baseline
        depth *= 2


def check(label, passed, detail):
    print(f"  [{'PASS' if passed else 'FAIL'}] {label}: {detail}")
    return passed


def run_tests() -> int:
    ext_path = get_extension_path()
    if not ext_path.exists():
        print(f"ERROR: extension not found at {ext_path}")
        print("Run `just build` first.")
        return 1

    con = make_connection()
    build_table(con)

    print(f"Calibrating workload ({ROWS:,} rows)...")
    depth, sv_baseline = calibrate(con)
    control_sql = plain_sql(depth)
    plain_baseline = time_query(con, control_sql)
    print(f"  plain-SQL twin baseline: {plain_baseline:.2f}s")

    # An abort is "early" if it lands well before the full run would have.
    #
    # The tolerance is a fraction of the UNINTERRUPTED run, not `fire delay +
    # fraction of it`. The regression this file guards makes the query run to
    # completion, so the two outcomes to separate are "stopped partway" and
    # "stopped at ~baseline" -- a fraction of the baseline sits between them by
    # construction, whatever the machine.
    #
    # The earlier formula (`FIRE_AT_S + max(1.0, 0.30 * baseline)`) budgeted for
    # the abort *delay* instead, and that delay does not scale with the
    # baseline: it is bounded by how long one `ExecuteTask()` chunk takes, which
    # is a property of the md5 depth, ~1.1s here and roughly constant across
    # baselines from 3.7s to 4.8s. Under load it stretched to 1.8s against a
    # 1.43s allowance and failed a build that had aborted at 46% of baseline --
    # a false positive, not a caught regression. See TECH-DEBT (v0.12.0).
    #
    # The floor keeps the limit off the fire delay plus one task on a fast
    # machine, where a small baseline would otherwise demand an impossibly
    # prompt abort.
    def deadline(baseline):
        return max(FIRE_AT_S + MIN_ABORT_SLACK_S, ABORT_FRACTION * baseline)

    ok = True

    print("\nPreconditions")
    ok &= check(
        "workload is slow enough to be conclusive",
        sv_baseline >= MIN_BASELINE_S,
        f"semantic_view() baseline {sv_baseline:.2f}s >= {MIN_BASELINE_S}s "
        f"(at md5 depth {depth}; raise MAX_DEPTH if this fails on a fast machine)",
    )
    if sv_baseline < MIN_BASELINE_S:
        # Everything below would be measuring noise.
        return 1

    # The abort limit is only meaningful if a build that never observed the
    # interrupt would breach it. Assert that against the MEASURED uninterrupted
    # run rather than trusting the constants: if calibration lands somewhere the
    # floor dominates, the limit can creep up to the baseline itself and every
    # assertion below starts passing vacuously -- including against the pre-fix
    # build this file exists to catch. Loud failure, not a silent green.
    sv_limit = deadline(sv_baseline)
    ok &= check(
        "the abort limit still discriminates a regressed build",
        sv_limit <= VACUITY_MARGIN * sv_baseline,
        f"limit {sv_limit:.2f}s <= {VACUITY_MARGIN:.0%} of baseline "
        f"{sv_baseline:.2f}s (a build that ran to completion would report "
        f"~{sv_baseline:.2f}s and fail, as intended)",
    )
    if sv_limit > VACUITY_MARGIN * sv_baseline:
        print(
            "  NOTE: the calibrated workload is too short for the abort floor "
            f"({FIRE_AT_S}s + {MIN_ABORT_SLACK_S}s). Raise TARGET_BASELINE_S so "
            "calibration picks a deeper md5 chain."
        )
        return 1

    print("\nControl: plain SQL through the identical harness")
    elapsed, err = run_interrupted(con, control_sql)
    limit = deadline(plain_baseline)
    control_ok = check(
        "plain SQL aborts early",
        err is not None and elapsed < limit,
        f"stopped after {elapsed:.2f}s (baseline {plain_baseline:.2f}s, limit {limit:.2f}s), "
        f"error={type(err).__name__ if err else 'None'}",
    )
    if not control_ok:
        print(
            "  NOTE: the control failing means the HARNESS is broken (interrupt "
            "never reached DuckDB at all), not that semantic_view() regressed."
        )
    ok &= control_ok

    print("\nsemantic_view(): cancellation must reach the inner query")
    elapsed, err = run_interrupted(con, SV_SQL)
    limit = deadline(sv_baseline)
    message = "" if err is None else str(err)

    ok &= check(
        "the query raises rather than completing",
        err is not None,
        f"error={type(err).__name__ if err else 'None'} after {elapsed:.2f}s",
    )
    ok &= check(
        "the query stops early instead of running to completion",
        err is not None and elapsed < limit,
        f"stopped after {elapsed:.2f}s (baseline {sv_baseline:.2f}s, limit {limit:.2f}s)",
    )
    ok &= check(
        "the error is a bare interrupt",
        "Interrupted!" in message,
        f"message={message!r}",
    )
    ok &= check(
        "the interrupt is not wrapped as an execution failure",
        "SQL execution failed" not in message,
        f"message={message!r}",
    )

    print("\nThe connection is still usable after the cancelled query")
    ok &= check(
        "follow-up query succeeds",
        con.execute("SELECT 42").fetchall() == [(42,)],
        "SELECT 42 returned 42",
    )

    print()
    print("ALL TESTS PASSED" if ok else "SOME TESTS FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(run_tests())
