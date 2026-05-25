#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2", "pytest>=7.0"]
# requires-python = ">=3.10"
# ///
"""
Phase 65.1 Wave 0 STUB — populated by Plan 09 (WR-03, decision D-18).

When DROP SEMANTIC VIEW or ALTER SEMANTIC VIEW runs against a fresh
read-only database that was never bootstrapped (no `semantic_layer._definitions`
table), the user must see the canonical "semantic view 'X' does not exist"
error wording, NOT the raw "Catalog Error: Table _definitions does not
exist" leak. Plan 09 will land the outer-CASE wrap in
`src/parse.rs::existence_guard_select` and replace the body of this test
with the real assertions (decision D-18).

Until Plan 09 ships this stub is skip-marked so the suite stays green.
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path  # noqa: F401  (stub — populated by Plan 09)


def test_drop_on_fresh_readonly_clear_error():
    pytest.skip("populated by Plan 09 (WR-03 D-18)")


if __name__ == "__main__":
    # Allow `uv run test/integration/test_drop_on_fresh_readonly_clear_error.py`
    # to exit 0 cleanly without pytest discovery — mirrors the invocation
    # convention used by the other test/integration/*.py scripts.
    try:
        test_drop_on_fresh_readonly_clear_error()
    except pytest.skip.Exception as exc:
        print(f"SKIPPED: {exc}")
        sys.exit(0)
