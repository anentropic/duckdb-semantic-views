#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2", "pytest>=7.0"]
# requires-python = ">=3.10"
# ///
"""
Phase 65.1 Wave 0 STUB — populated by Plan 12 (WR-09, decision D-21).

`LOAD semantic_views` must be idempotent across repeated calls within a
single process: registering the parser-override hook a second time must
not accumulate duplicate entries in `DBConfig::parser_extensions`. Plan 12
will land the check-before-append guard in `sv_register_parser_hooks`
(cpp/src/shim.cpp) and replace the body of this test with the real
assertions — sentinel checks against `duckdb_extensions()` plus a DDL
operation after the second LOAD that exercises the parser hook (decision
D-21).

Until Plan 12 ships this stub is skip-marked so the suite stays green.
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path  # noqa: F401  (stub — populated by Plan 12)


def test_load_extension_twice_idempotent():
    pytest.skip("populated by Plan 12 (WR-09 D-21)")


if __name__ == "__main__":
    # Allow `uv run test/integration/test_load_extension_twice_idempotent.py`
    # to exit 0 cleanly without pytest discovery.
    try:
        test_load_extension_twice_idempotent()
    except pytest.skip.Exception as exc:
        print(f"SKIPPED: {exc}")
        sys.exit(0)
