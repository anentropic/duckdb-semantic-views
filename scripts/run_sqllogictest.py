#!/usr/bin/env python3
"""
Strict wrapper around the duckdb_sqllogictest runner.

Drop-in for ``python -m duckdb_sqllogictest`` (same arguments; the Makefile
points ``TEST_RUNNER`` here), run with the configure/venv interpreter. It runs
the upstream runner in-process and then refuses to call a run green when the
runner would have reported success without actually testing anything:

1. ``require semantic_views`` that cannot be satisfied FAILS the file instead
   of skipping it. This uses the runner's own ``required_requires`` mechanism
   (which upstream has no CLI flag for). A missing or renamed extension file
   used to skip every file and exit 0.
2. Every file named in ``--file-list`` / ``--file-path`` must produce a result.
   The runner logs a file it cannot parse to its skip log and moves on, with
   no SKIPPED line and no failure.
3. A file that requires semantic_views and reports SUCCESS must have executed
   at least one ``statement`` / ``query``. (``mode skip`` with no ``unskip``
   reports SUCCESS having run nothing.)
4. A SKIPPED file is a failure unless it was skipped by a ``require`` for
   something other than a required extension -- e.g. ``require windows`` /
   ``require notwindows`` (see scripts/patch_sqllogictest.py), which BuildAll
   legitimately hits on Windows/macOS. Those skips are listed loudly at the end.
   Anything else (``halt``, unsupported statements, decorators) is a failure.

The required extensions default to ``semantic_views``; pass
``--required-extension NAME`` (repeatable) to change that. Every other
argument is forwarded to the runner untouched.

The upstream package is pinned by extension-ci-tools and already patched by
scripts/patch_sqllogictest.py; this wrapper hooks four of its methods
(``SQLLogicRunner.reset``, ``SQLLogicContext.execute_statement`` /
``execute_query``, ``SQLLogicTestExecutor.execute_test``). If a runner bump
renames them, the ``getattr`` checks below fail loudly rather than letting the
guards go quiet.
"""

from __future__ import annotations

import argparse
import sys
from dataclasses import dataclass, field

from duckdb_sqllogictest import python_runner, result
from duckdb_sqllogictest.result import ExecuteResult


@dataclass
class FileOutcome:
    path: str
    result: str = "NOT RUN"
    executed: int = 0
    skip_reason: str = ""
    requires: set[str] = field(default_factory=set)


def _require_params(test) -> set[str]:
    params = set()
    for statement in test.statements:
        if type(statement).__name__ == "Require":
            params.add(statement.header.parameters[0].lower())
    return params


def _hook(cls, name):
    original = getattr(cls, name, None)
    if original is None:
        sys.exit(
            f"ERROR: duckdb_sqllogictest no longer has {cls.__name__}.{name}; "
            "scripts/run_sqllogictest.py must be updated for the new runner"
        )
    return original


def main() -> int:
    pre = argparse.ArgumentParser(add_help=False)
    pre.add_argument("--required-extension", action="append", default=None)
    pre.add_argument("--file-list")
    pre.add_argument("--file-path")
    ours, _ = pre.parse_known_args()
    required = {e.lower() for e in (ours.required_extension or ["semantic_views"])}

    # Strip our own option; forward everything else to the upstream runner.
    forwarded = []
    skip_next = False
    for arg in sys.argv[1:]:
        if skip_next:
            skip_next = False
            continue
        if arg == "--required-extension":
            skip_next = True
            continue
        if arg.startswith("--required-extension="):
            continue
        forwarded.append(arg)

    if ours.file_list:
        with open(ours.file_list) as f:
            expected = f.read().split()
    elif ours.file_path:
        expected = [ours.file_path]
    else:
        expected = None  # directory glob: no fixed list to check against

    outcomes: dict[str, FileOutcome] = {}
    current: list[FileOutcome] = []

    # 1. Required extensions fail instead of skipping.
    orig_reset = _hook(result.SQLLogicRunner, "reset")

    def reset(self):
        orig_reset(self)
        self.required_requires.update(required)

    result.SQLLogicRunner.reset = reset

    # 3. Count executed statements/queries (loop bodies included).
    for method in ("execute_statement", "execute_query"):
        orig = _hook(result.SQLLogicContext, method)

        def counted(self, statement, _orig=orig):
            if current:
                current[-1].executed += 1
            return _orig(self, statement)

        setattr(result.SQLLogicContext, method, counted)

    # 2./4. Record each file's result and skip reason.
    orig_execute_test = _hook(python_runner.SQLLogicTestExecutor, "execute_test")

    def execute_test(self, test):
        outcome = FileOutcome(path=test.path, requires=_require_params(test))
        outcomes[test.path] = outcome
        current.append(outcome)
        skip_log_before = len(self.skip_log)
        try:
            res = orig_execute_test(self, test)
        finally:
            current.pop()
        outcome.result = res.type.name
        if res.type == ExecuteResult.Type.SKIPPED and len(self.skip_log) > skip_log_before:
            outcome.skip_reason = self.skip_log[-1]
        return res

    python_runner.SQLLogicTestExecutor.execute_test = execute_test

    sys.argv = [sys.argv[0], *forwarded]
    exit_code = 0
    try:
        python_runner.SQLLogicPythonRunner().run()
    except SystemExit as e:  # the runner calls exit(1) on the first ERROR
        exit_code = e.code if isinstance(e.code, int) else 1
    if exit_code:
        print(f"sqllogictest: FAILED (runner exit {exit_code})", file=sys.stderr)
        return exit_code

    problems: list[str] = []
    allowed_skips: list[str] = []

    if expected is not None:
        for path in expected:
            if path not in outcomes:
                problems.append(
                    f"{path}: never ran (the runner could not parse it, or skipped it unannounced)"
                )
    if not outcomes:
        problems.append("no test files ran at all")

    for outcome in outcomes.values():
        needs_required = bool(outcome.requires & required)
        if outcome.result == "SUCCESS":
            if needs_required and outcome.executed == 0:
                problems.append(
                    f"{outcome.path}: reported SUCCESS but executed 0 statements/queries"
                )
        elif outcome.result == "SKIPPED":
            reason = outcome.skip_reason
            # "<file>:<line> require <param>: Missing" for a non-required param
            # is a legitimate platform/feature skip; everything else is not.
            marker = " require "
            param = ""
            if marker in reason and reason.endswith(": Missing"):
                param = reason.split(marker, 1)[1][: -len(": Missing")].strip().lower()
            if param and param not in required:
                allowed_skips.append(reason)
            else:
                problems.append(f"{outcome.path}: SKIPPED ({reason or 'no reason logged'})")
        else:
            problems.append(f"{outcome.path}: result {outcome.result}")

    for line in allowed_skips:
        print(f"SKIPPED (allowed, require not met on this platform): {line}")

    ran = sum(1 for o in outcomes.values() if o.result == "SUCCESS")
    statements = sum(o.executed for o in outcomes.values())
    if problems:
        print(
            f"sqllogictest: FAILED -- {len(problems)} file(s) did not really run "
            f"(required extensions: {', '.join(sorted(required))}):",
            file=sys.stderr,
        )
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1

    print(
        f"sqllogictest: {ran} file(s) ran, {statements} statements/queries executed"
        + (f", {len(allowed_skips)} platform skip(s)" if allowed_skips else "")
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
