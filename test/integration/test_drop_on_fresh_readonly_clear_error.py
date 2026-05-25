#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2", "pytest>=7.0"]
# requires-python = ">=3.10"
# ///
"""
Phase 65.1 Plan 04 — WR-03 integration test (decision D-18).

When DROP SEMANTIC VIEW or ALTER SEMANTIC VIEW runs against a fresh
read-only database that was never bootstrapped (no ``semantic_layer._definitions``
table), the user must see the canonical "semantic view 'X' does not exist"
error wording, NOT the raw "Catalog Error: Table _definitions does not
exist" leak. Phase 65.1 Plan 04 lands the outer-CASE wrap in
``src/parse.rs::existence_guard_select`` and this test pins the user-visible
contract:

  1. ``DROP SEMANTIC VIEW nonexistent`` (no IF EXISTS) errors with substring
     ``semantic view 'nonexistent' does not exist`` AND the error message
     does NOT contain ``_definitions`` (the leakage we are fixing).
  2. ``ALTER SEMANTIC VIEW nonexistent SET COMMENT = 'x'`` errors with
     the same substring and the same absence of ``_definitions``.
  3. ``DROP SEMANTIC VIEW IF EXISTS nonexistent`` succeeds silently — the
     IF EXISTS short-circuit path tolerates the never-bootstrapped state
     (W-06 pinned assertion).

The DB is freshly created (a minimal DuckDB header — single SELECT 1
through a no-extension connection — so the file exists with a valid
header). The semantic_views extension is NEVER LOADed on a writable
handle, so ``init_catalog`` never runs and ``semantic_layer._definitions``
is never created. We then reopen the same file with ``read_only=True``
and LOAD the extension. Because the RO open skips the writable
``init_catalog`` path, ``_definitions`` is genuinely absent. The
DROP/ALTER guard SQL then has to short-circuit on the missing table —
this is the exact WR-03 reproduction.
"""

import os
import sys
import tempfile
from pathlib import Path

import duckdb
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path

EXT_DIR = get_ext_dir()
EXT_PATH = get_extension_path()


def _open(db_path: str, *, read_only: bool, load_ext: bool = True):
    """Open a DuckDB connection at ``db_path``. ``read_only`` selects mode.
    ``load_ext`` controls whether the semantic_views extension is FORCE
    INSTALLed and LOADed before returning. Defaults True; set False to
    produce a writable seed handle that lays down a DuckDB header
    WITHOUT running ``init_catalog`` (so ``semantic_layer._definitions``
    is never created)."""
    conn = duckdb.connect(
        db_path,
        read_only=read_only,
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": EXT_DIR,
        },
    )
    if load_ext:
        conn.execute(f"FORCE INSTALL '{EXT_PATH}'")
        conn.execute("LOAD semantic_views")
    return conn


def test_drop_on_fresh_readonly_clear_error():
    """WR-03 (D-17, D-18): DROP/ALTER on never-bootstrapped RO DB must
    surface the canonical "semantic view 'X' does not exist" wording, not
    the lower-level "Catalog Error: Table _definitions does not exist"
    implementation leak.

    Also pins (W-06): ``DROP SEMANTIC VIEW IF EXISTS nonexistent`` is a
    silent no-op on the same DB shape.
    """
    with tempfile.TemporaryDirectory() as tmp:
        db_path = os.path.join(tmp, "test.duckdb")

        # 1. Seed phase: open writable WITHOUT loading the extension. This
        #    lays down a valid DuckDB header but does NOT run init_catalog,
        #    so `semantic_layer._definitions` is genuinely absent. (If we
        #    LOADed the extension here, init_catalog would CREATE the
        #    `_definitions` table on the writable handle and the WR-03
        #    case would be unreachable — see Plan 04 SUMMARY for the
        #    investigation that surfaced this constraint.)
        seed = _open(db_path, read_only=False, load_ext=False)
        seed.execute("SELECT 1")
        seed.close()

        # 2. Reopen the same file read-only with the extension LOADed.
        ro = _open(db_path, read_only=True)
        try:
            # 3a. Plain DROP on never-bootstrapped RO DB: must error with
            #     canonical wording, must NOT leak `_definitions`.
            with pytest.raises(Exception) as drop_exc:
                ro.execute("DROP SEMANTIC VIEW nonexistent")
            drop_msg = str(drop_exc.value)
            assert "semantic view 'nonexistent' does not exist" in drop_msg, (
                f"DROP error did not carry canonical wording. Got: {drop_msg!r}"
            )
            assert "_definitions" not in drop_msg, (
                f"DROP error leaked internal `_definitions` table name. Got: {drop_msg!r}"
            )

            # 3b. Plain ALTER on never-bootstrapped RO DB: same expectations.
            with pytest.raises(Exception) as alter_exc:
                ro.execute("ALTER SEMANTIC VIEW nonexistent SET COMMENT = 'x'")
            alter_msg = str(alter_exc.value)
            assert "semantic view 'nonexistent' does not exist" in alter_msg, (
                f"ALTER error did not carry canonical wording. Got: {alter_msg!r}"
            )
            assert "_definitions" not in alter_msg, (
                f"ALTER error leaked internal `_definitions` table name. Got: {alter_msg!r}"
            )

            # 3c. W-06: DROP ... IF EXISTS on the same DB shape MUST succeed
            #     silently (no exception). The IF EXISTS rewrite path emits a
            #     plain DELETE against `semantic_layer._definitions` without
            #     the existence guard (src/parse.rs::rewrite_drop). On a
            #     never-bootstrapped RO DB this DELETE will fail with
            #     "Catalog Error: Table _definitions does not exist" UNLESS
            #     the IF EXISTS path is itself made soft for missing
            #     `_definitions`. The assertion below pins the contract and
            #     will surface any drift in Plan 04 Task 2 implementation
            #     (Option A vs Option B in the plan action).
            ro.execute("DROP SEMANTIC VIEW IF EXISTS nonexistent")
        finally:
            ro.close()


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"]))
