#!/usr/bin/env python3
"""Patch duckdb_sqllogictest to support notwindows/windows require directives.
Idempotent — safe to run on every test invocation.

Remove this script once extension-ci-tools updates its pinned
duckdb-sqllogictest-python commit to include platform detection.
"""

import importlib.util
import pathlib
import sys


def main() -> None:
    spec = importlib.util.find_spec('duckdb_sqllogictest')
    if spec is None:
        print("ERROR: duckdb_sqllogictest not found", file=sys.stderr)
        sys.exit(1)

    result_py = pathlib.Path(spec.origin).parent / 'result.py'
    content = result_py.read_text(encoding='utf-8')

    # Idempotency guard
    if "if param == 'notwindows':" in content:
        print("patch_sqllogictest: already applied.")
        return

    # Inject 'import sys' after 'import os' if missing
    if 'import sys' not in content:
        content = content.replace('import os\n', 'import os\nimport sys\n', 1)

    # Insert platform checks before the fallthrough return.
    # Anchored on the skip_reload block which immediately precedes it.
    SEARCH = (
        "            if param == 'skip_reload':\n"
        "                self.runner.skip_reload = True\n"
        "                return RequireResult.PRESENT\n"
        "            return RequireResult.MISSING\n"
    )
    REPLACE = (
        "            if param == 'skip_reload':\n"
        "                self.runner.skip_reload = True\n"
        "                return RequireResult.PRESENT\n"
        "            if param == 'notwindows':\n"
        "                return RequireResult.PRESENT if sys.platform != 'win32' else RequireResult.MISSING\n"
        "            if param == 'windows':\n"
        "                return RequireResult.PRESENT if sys.platform == 'win32' else RequireResult.MISSING\n"
        "            return RequireResult.MISSING\n"
    )

    if SEARCH not in content:
        print(
            "ERROR: anchor string not found — package may have been updated. "
            "Inspect configure/venv/.../duckdb_sqllogictest/result.py and update this script.",
            file=sys.stderr,
        )
        sys.exit(1)

    content = content.replace(SEARCH, REPLACE, 1)
    result_py.write_text(content, encoding='utf-8')
    print(f"patch_sqllogictest: patched {result_py}")


if __name__ == '__main__':
    main()
