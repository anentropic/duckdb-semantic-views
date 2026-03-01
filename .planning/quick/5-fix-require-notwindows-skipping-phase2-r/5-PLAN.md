---
phase: quick-5
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - scripts/patch_sqllogictest.py
  - Makefile
autonomous: true
requirements: [PERSIST-01, PERSIST-02]
must_haves:
  truths:
    - "phase2_restart.test executes its assertions on Linux/macOS instead of being skipped"
    - "patch is idempotent — running it twice costs ~1ms and prints 'already applied'"
    - "make test_debug re-applies the patch automatically without manual steps"
  artifacts:
    - path: "scripts/patch_sqllogictest.py"
      provides: "Idempotent patcher for duckdb_sqllogictest result.py"
    - path: "Makefile"
      provides: "patch-runner target wired as prereq of test_extension_debug_internal and test_extension_release_internal"
  key_links:
    - from: "Makefile test_extension_debug_internal"
      to: "scripts/patch_sqllogictest.py"
      via: "patch-runner prerequisite"
      pattern: "test_extension_debug_internal: patch-runner"
    - from: "scripts/patch_sqllogictest.py"
      to: "configure/venv/.../duckdb_sqllogictest/result.py"
      via: "importlib.util.find_spec + pathlib write"
      pattern: "find_spec.*duckdb_sqllogictest"
---

<objective>
Fix `require notwindows` in `test/sql/phase2_restart.test` being incorrectly treated as
"skip on all platforms" by the installed `duckdb_sqllogictest` Python runner.

Purpose: The restart test is the only test exercising real catalog persistence
(PERSIST-01/02). It is silently skipped today on every platform due to a bug in
`duckdb_sqllogictest/result.py` — `notwindows` falls through to `RequireResult.MISSING`
instead of checking `sys.platform`. Fixing this makes Phase 10 persistence actually tested.

Output: `scripts/patch_sqllogictest.py` + Makefile wiring. The upstream package is pinned
via a submodule we don't control, so a local patch script is the only viable approach.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md

Key facts about the environment:
- `PYTHON_VENV_BIN` is defined in `extension-ci-tools/makefiles/c_api_extensions/base.Makefile`
  (line 24: `./configure/venv/bin/python3` on non-Windows)
- `check_configure` target is defined in base.Makefile (line 270) — asserts venv exists
- `TEST_RUNNER_DEBUG` / `TEST_RUNNER_RELEASE` defined in base.Makefile (lines 108-109)
- `test_extension_debug_internal` and `test_extension_release_internal` are defined in
  base.Makefile (lines 164-170) — our Makefile overrides them
- On SKIP_TESTS=1 platforms (musl, mingw), Make resolves to `tests_skipped` target —
  `test_extension_debug_internal` is never invoked, so `patch-runner` is never called
- The venv installs `duckdb_sqllogictest` from a pinned GitHub commit (base.Makefile line 253)
</context>

<tasks>

<task type="auto">
  <name>Task 1: Create scripts/patch_sqllogictest.py</name>
  <files>scripts/patch_sqllogictest.py</files>
  <action>
Create `scripts/patch_sqllogictest.py` with the exact content below. Do not alter the
string literals — they must match the installed package verbatim.

```python
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
```

After creating the file, make it executable:
`chmod +x scripts/patch_sqllogictest.py`
  </action>
  <verify>
    <automated>python3 /Users/paul/Documents/Dev/Personal/duckdb-semantic-views/scripts/patch_sqllogictest.py --help 2>&1 || python3 /Users/paul/Documents/Dev/Personal/duckdb-semantic-views/scripts/patch_sqllogictest.py 2>&1; test -f /Users/paul/Documents/Dev/Personal/duckdb-semantic-views/scripts/patch_sqllogictest.py</automated>
  </verify>
  <done>File exists at scripts/patch_sqllogictest.py, is executable, and imports without syntax errors.</done>
</task>

<task type="auto">
  <name>Task 2: Wire patch-runner into Makefile test targets</name>
  <files>Makefile</files>
  <action>
Append the following block to the bottom of `Makefile` (after the existing
`build_extension_library_release` target). Do NOT touch base.Makefile or rust.Makefile —
those are submodule files.

```makefile

# Patch installed duckdb_sqllogictest to add notwindows/windows platform detection.
# Idempotent. Remove once extension-ci-tools updates its pinned sqllogictest commit.
.PHONY: patch-runner
patch-runner: check_configure
	@$(PYTHON_VENV_BIN) scripts/patch_sqllogictest.py

# Override base.Makefile test targets to patch runner before tests.
# SKIP_TESTS platforms (musl, mingw) resolve to tests_skipped before reaching these
# targets, so patch-runner is never called on those platforms — which is correct.
test_extension_debug_internal: patch-runner
	@echo "Running DEBUG tests.."
	@$(TEST_RUNNER_DEBUG)

test_extension_release_internal: patch-runner
	@echo "Running RELEASE tests.."
	@$(TEST_RUNNER_RELEASE)
```

Note: Make allows later definitions of a target's recipe to override earlier ones.
The `test_extension_debug_internal: patch-runner` line adds `patch-runner` as an
additional prerequisite AND replaces the recipe from base.Makefile with an identical
recipe — this is the standard Make override pattern.

Tab characters are required before the recipe lines (`@$(PYTHON_VENV_BIN) ...`,
`@echo ...`, `@$(TEST_RUNNER_DEBUG)`). Use actual tab characters, not spaces.
  </action>
  <verify>
    <automated>make -n test_debug 2>&1 | grep -q "patch_sqllogictest" && echo "patch-runner wired correctly" || echo "MISSING: patch-runner not in test_debug chain"</automated>
  </verify>
  <done>
`make -n test_debug` (dry run) shows `patch_sqllogictest.py` in the execution chain.
After `make configure`, running `make patch-runner` prints "patched ..." or "already applied."
Running `make patch-runner` a second time prints "already applied." (idempotency confirmed).
  </done>
</task>

</tasks>

<verification>
After both tasks:

1. Dry-run check: `make -n test_debug` output includes `scripts/patch_sqllogictest.py`
2. Manual patch test (requires venv): `make configure && make patch-runner` → prints "patched ..."
3. Idempotency: `make patch-runner` again → prints "already applied."
4. Full test run (requires built extension): `make test_debug` — `phase2_restart.test`
   should show test assertions, not "Skipping tests.."
</verification>

<success_criteria>
- `scripts/patch_sqllogictest.py` exists and is executable
- `Makefile` contains `patch-runner` target as prereq of both `test_extension_*_internal` targets
- `make -n test_debug` confirms patch-runner is in the chain without running anything
- On non-Windows platforms: `phase2_restart.test` runs instead of being skipped
- Patch is idempotent (second invocation exits 0 with "already applied.")
</success_criteria>

<output>
After completion, create `.planning/quick/5-fix-require-notwindows-skipping-phase2-r/5-SUMMARY.md`

Include:
- What was changed (script + Makefile)
- How to remove it later (when extension-ci-tools updates its pinned commit)
- Commit hash
</output>
