---
phase: quick-2
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - configure/setup_ducklake.py
  - test/integration/test_ducklake.py
  - justfile
  - .gitignore
autonomous: true
requirements: [QUICK-2]

must_haves:
  truths:
    - "setup_ducklake.py runs via `uv run` without needing configure/venv"
    - "test_ducklake.py runs via `uv run` without needing configure/venv"
    - "justfile recipes use `uv run` instead of venv python path"
    - "dbt/ directory is gitignored"
    - "fuzz/Cargo.lock is tracked in git"
  artifacts:
    - path: "configure/setup_ducklake.py"
      provides: "PEP 723 inline metadata declaring duckdb dependency"
      contains: "# /// script"
    - path: "test/integration/test_ducklake.py"
      provides: "PEP 723 inline metadata declaring duckdb dependency"
      contains: "# /// script"
    - path: "justfile"
      provides: "Updated recipes using uv run"
      contains: "uv run"
    - path: ".gitignore"
      provides: "dbt/ exclusion"
      contains: "dbt/"
  key_links:
    - from: "justfile:setup-ducklake"
      to: "configure/setup_ducklake.py"
      via: "uv run configure/setup_ducklake.py"
      pattern: "uv run configure/setup_ducklake.py"
    - from: "justfile:test-iceberg"
      to: "test/integration/test_ducklake.py"
      via: "uv run test/integration/test_ducklake.py"
      pattern: "uv run test/integration/test_ducklake.py"
---

<objective>
Convert `configure/setup_ducklake.py` and `test/integration/test_ducklake.py` to self-contained uv scripts using PEP 723 inline metadata, eliminating their dependency on `configure/venv`. Update justfile recipes accordingly. Also commit `fuzz/Cargo.lock` and gitignore `dbt/`.

Purpose: The venv is owned by the DuckDB extension-ci-tools build system (Makefile). The two Python scripts that reference it directly are not part of that build system -- they are standalone developer/test scripts that should declare their own dependencies via PEP 723 inline metadata and be invoked with `uv run`.

Output: Updated Python scripts with inline metadata, updated justfile, committed fuzz/Cargo.lock, gitignored dbt/
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@configure/setup_ducklake.py
@test/integration/test_ducklake.py
@justfile
@.gitignore
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add PEP 723 inline metadata to Python scripts and update justfile</name>
  <files>configure/setup_ducklake.py, test/integration/test_ducklake.py, justfile</files>
  <action>
**configure/setup_ducklake.py:**
Add PEP 723 inline script metadata block immediately after the shebang line (`#!/usr/bin/env python3`) and before the module docstring. The metadata block declares the `duckdb` dependency so `uv run` auto-installs it:

```python
# /// script
# dependencies = ["duckdb"]
# requires-python = ">=3.9"
# ///
```

Keep the existing shebang (`#!/usr/bin/env python3`) -- it serves as fallback for direct execution. The PEP 723 block goes between the shebang and the triple-quoted docstring.

**test/integration/test_ducklake.py:**
Same treatment -- add identical PEP 723 metadata block after the shebang and before the docstring:

```python
# /// script
# dependencies = ["duckdb"]
# requires-python = ">=3.9"
# ///
```

**justfile:**
Update these two recipes to use `uv run` instead of `./configure/venv/bin/python3`:

1. `setup-ducklake` (line 69): Change `./configure/venv/bin/python3 configure/setup_ducklake.py` to `uv run configure/setup_ducklake.py`
2. `test-iceberg` (line 74): Change `./configure/venv/bin/python3 test/integration/test_ducklake.py` to `uv run test/integration/test_ducklake.py`
3. Update the comment on line 67 (`# Uses the project venv Python which has duckdb installed.`) to `# Uses uv to run the script with its declared dependencies (PEP 723).`

Do NOT modify the Makefile -- `PYTHON_VENV_BIN` and the `venv` target are part of the upstream DuckDB extension-ci-tools build system and are still needed for `make configure`, `make test_debug`, metadata appending, etc.
  </action>
  <verify>
    <automated>grep -q '# /// script' configure/setup_ducklake.py && grep -q '# /// script' test/integration/test_ducklake.py && grep -q 'uv run' justfile && ! grep -q 'configure/venv' justfile && echo "PASS" || echo "FAIL"</automated>
  </verify>
  <done>Both Python scripts contain PEP 723 inline metadata declaring duckdb dependency. Justfile recipes invoke scripts via `uv run` with no references to configure/venv. Makefile is untouched.</done>
</task>

<task type="auto">
  <name>Task 2: Gitignore dbt/ and commit fuzz/Cargo.lock</name>
  <files>.gitignore, fuzz/Cargo.lock</files>
  <action>
**.gitignore:**
Add a new section at the end of .gitignore (after the DuckLake test data section):

```
# dbt reference material (not part of the project)
dbt/
```

**fuzz/Cargo.lock:**
Stage `fuzz/Cargo.lock` for commit. This file is currently untracked and should be version-controlled so fuzz builds are reproducible (Cargo recommends committing Cargo.lock for binaries/applications, which fuzz targets effectively are).

No content changes needed to fuzz/Cargo.lock itself -- just `git add` it.
  </action>
  <verify>
    <automated>grep -q '^dbt/$' .gitignore && git ls-files --error-unmatch fuzz/Cargo.lock 2>/dev/null; test $? -ne 0 && echo "fuzz/Cargo.lock not yet tracked (will be after commit)" || echo "fuzz/Cargo.lock tracked"; echo "PASS"</automated>
  </verify>
  <done>.gitignore contains `dbt/` entry. fuzz/Cargo.lock is staged/committed to git.</done>
</task>

</tasks>

<verification>
1. `grep '# /// script' configure/setup_ducklake.py` -- PEP 723 metadata present
2. `grep '# /// script' test/integration/test_ducklake.py` -- PEP 723 metadata present
3. `grep 'uv run' justfile` -- both recipes updated
4. `grep 'configure/venv' justfile` -- should return nothing (no venv references remain in justfile)
5. `grep 'dbt/' .gitignore` -- dbt directory ignored
6. `git show HEAD -- fuzz/Cargo.lock | head -1` -- fuzz/Cargo.lock is committed
</verification>

<success_criteria>
- `uv run configure/setup_ducklake.py --help` would resolve duckdb dependency automatically (actual execution requires network)
- justfile has zero references to configure/venv
- Makefile is unchanged (still uses PYTHON_VENV_BIN for build system)
- dbt/ directory is gitignored
- fuzz/Cargo.lock is version-controlled
</success_criteria>

<output>
After completion, create `.planning/quick/2-convert-setup-ducklake-py-to-uv-script-r/2-SUMMARY.md`
</output>
