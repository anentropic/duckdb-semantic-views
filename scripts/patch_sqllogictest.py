#!/usr/bin/env python3
"""
Patch duckdb_sqllogictest to:
  1. Support notwindows/windows require directives.
  2. Handle EXTENSION statement types for parser extension query results.

Idempotent — safe to run on every test invocation.

Remove this script once extension-ci-tools updates its pinned
duckdb-sqllogictest-python commit to include these fixes.
"""

import importlib.util
import pathlib
import sys


def main() -> None:
    spec = importlib.util.find_spec("duckdb_sqllogictest")
    if spec is None:
        print("ERROR: duckdb_sqllogictest not found", file=sys.stderr)
        sys.exit(1)

    result_py = pathlib.Path(spec.origin).parent / "result.py"
    content = result_py.read_text(encoding="utf-8")

    applied = []

    # --- Patch 1: Platform detection (notwindows/windows) ---

    if "if param == 'notwindows':" not in content:
        # Inject 'import sys' after 'import os' if missing
        if "import sys" not in content:
            content = content.replace("import os\n", "import os\nimport sys\n", 1)

        SEARCH_1 = (
            "            if param == 'skip_reload':\n"
            "                self.runner.skip_reload = True\n"
            "                return RequireResult.PRESENT\n"
            "            return RequireResult.MISSING\n"
        )
        REPLACE_1 = (
            "            if param == 'skip_reload':\n"
            "                self.runner.skip_reload = True\n"
            "                return RequireResult.PRESENT\n"
            "            if param == 'notwindows':\n"
            "                return RequireResult.PRESENT if sys.platform != 'win32' else RequireResult.MISSING\n"
            "            if param == 'windows':\n"
            "                return RequireResult.PRESENT if sys.platform == 'win32' else RequireResult.MISSING\n"
            "            return RequireResult.MISSING\n"
        )

        if SEARCH_1 not in content:
            print(
                "ERROR: platform patch anchor not found — package may have been updated. "
                "Inspect configure/venv/.../duckdb_sqllogictest/result.py and update this script.",
                file=sys.stderr,
            )
            sys.exit(1)

        content = content.replace(SEARCH_1, REPLACE_1, 1)
        applied.append("platform")

    # --- Patch 2: EXTENSION statement type for parser extension queries ---
    # DuckDB reports parser extension statements as StatementType.EXTENSION with
    # expected_result_type = [CHANGED_ROWS, QUERY_RESULT, NOTHING]. The default
    # is_query_result function returns False for these (len != 1), causing the
    # runner to treat them as CHANGED_ROWS (1 BIGINT column) instead of multi-
    # column query results. Fix: treat EXTENSION type with QUERY_RESULT as a
    # query result.

    if "StatementType.EXTENSION" not in content:
        SEARCH_2 = (
            "                return len(statement.expected_result_type) == 1\n"
        )
        REPLACE_2 = (
            "                if hasattr(duckdb, 'StatementType') and hasattr(duckdb.StatementType, 'EXTENSION'):\n"
            "                    if statement.type == duckdb.StatementType.EXTENSION:\n"
            "                        return True\n"
            "                return len(statement.expected_result_type) == 1\n"
        )

        if SEARCH_2 not in content:
            print(
                "ERROR: extension patch anchor not found — package may have been updated. "
                "Inspect configure/venv/.../duckdb_sqllogictest/result.py and update this script.",
                file=sys.stderr,
            )
            sys.exit(1)

        content = content.replace(SEARCH_2, REPLACE_2, 1)
        applied.append("extension")

    if applied:
        result_py.write_text(content, encoding="utf-8")
        print(f"patch_sqllogictest: patched ({', '.join(applied)})")
    else:
        print("patch_sqllogictest: already applied.")


if __name__ == "__main__":
    main()
