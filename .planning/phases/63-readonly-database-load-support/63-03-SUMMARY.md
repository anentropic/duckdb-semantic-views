---
phase: 63
plan: 03
subsystem: user-facing documentation + example
tags: [readonly, docs, changelog, example, milestone-close-prep]
dependency-graph:
  requires:
    - "63-01 (read-only LOAD core)"
    - "63-02 (test infrastructure — for cross-reference vocabulary alignment)"
  provides:
    - "CHANGELOG.md ## [0.9.0] section with two ### Added bullets + ### Known limitations"
    - "docs/explanation/transactional-ddl-and-limitations.rst Read-Only Databases section + Summary cross-ref"
    - "Three reference pages (create/drop/alter-semantic-view.rst) cross-reference the readonly explanation"
    - "README.md Quick start callout for read-only support + docs-site link"
    - "examples/readonly_load.py PEP-723 end-to-end demo (subprocess bootstrap workaround for in-process RW→RO hang)"
  affects:
    - "Plan 04 (release): the [0.9.0] heading already carries a 2026-05-15 date — Plan 04 must re-confirm at tag time"
tech-stack:
  added: []
  patterns: ["subprocess-bootstrap workaround in example for Phase 62 OverrideContext leak", "Sphinx :ref: cross-reference under explanation-txn-ddl-readonly label", "Markdown blockquote callout in README (no Sphinx admonitions in user-facing GitHub README)"]
key-files:
  created:
    - "examples/readonly_load.py (191 lines, PEP-723 + subprocess bootstrap)"
    - ".planning/phases/63-readonly-database-load-support/63-03-SUMMARY.md"
  modified:
    - "CHANGELOG.md (+15 lines: [0.9.0] section + compare links)"
    - "docs/explanation/transactional-ddl-and-limitations.rst (+56 lines: new section + Summary cross-ref)"
    - "docs/reference/create-semantic-view.rst (+4 lines: writable database note)"
    - "docs/reference/drop-semantic-view.rst (+4 lines: writable database note)"
    - "docs/reference/alter-semantic-view.rst (+4 lines: writable database note)"
    - "README.md (+2 lines: Quick start callout)"
decisions:
  - "Bootstrap-in-subprocess pattern adopted for examples/readonly_load.py to sidestep Phase 62's OverrideContext in-process RW→RO hang (documented in deferred-items.md). Mirrors test/integration/test_readonly_load.py::bootstrap_in_subprocess(). Without this the example hangs at the read-only reopen step — verified by running an initial in-process draft that produced two stuck Python processes at 100% CPU. Real-world deployments separate bootstrap (build/CI job) from read-only query (analytics worker) across processes, so the subprocess workaround mirrors actual production usage."
  - "Used Markdown blockquote (>) format for the README callout rather than emoji or HTML admonition. README is rendered by GitHub, not Sphinx; blockquote is the canonical GitHub callout style and matches the file's existing prose-only voice."
  - "All three reference-page notes use IDENTICAL wording for greppability and consistency. Did not vary per page — see acceptance criteria."
  - "CHANGELOG section uses ONLY ### Added and ### Known limitations subheadings per CLAUDE.md Keep-a-Changelog rule. No ad-hoc ### Phase 63 / ### Tests / ### Docs subheadings."
metrics:
  duration: "~25 min"
  completed: 2026-05-15
---

# Phase 63 Plan 03: User-Facing Documentation + Example Summary

Five user-facing surfaces document and demonstrate v0.9.0's read-only LOAD support: the CHANGELOG entry under Keep-a-Changelog headings, a new "Read-Only Databases" section in the transactional-DDL explanation page (with cross-references from the three reference pages), a one-line README callout, and a runnable PEP-723 example script. `just docs-check` (Sphinx -W) and `just ci` (full quality gate including the Plan 02 `test_readonly_load.py` integration test) both green.

## What Shipped

**Files modified (6):**

- `CHANGELOG.md` (+15 lines) — `## [0.9.0] - 2026-05-15` section under standard `### Added` (4 bullets) and `### Known limitations` (1 bullet) headings; bottom-of-file `[Unreleased]` link bumped to `v0.9.0...HEAD` and `[0.9.0]` compare link added.
- `docs/explanation/transactional-ddl-and-limitations.rst` (+56 lines) — new `Read-Only Databases` section between `_explanation-txn-ddl-drop-alter-race` and `_explanation-txn-ddl-peg`, with label `_explanation-txn-ddl-readonly`, `versionadded:: 0.9.0` directive, three numbered behaviour shifts, `Bootstrap-then-reopen workflow` Python code block, and a `.. note::` documenting the v0.1.0 companion-file migration limitation. Summary `See also:` block gains a top-of-list bullet pointing to the new section.
- `docs/reference/create-semantic-view.rst` (+4 lines) — third `.. note::` admonition after Statement Variants: "Requires a writable database... See :ref:\`explanation-txn-ddl-readonly\`."
- `docs/reference/drop-semantic-view.rst` (+4 lines) — same identical `.. note::`.
- `docs/reference/alter-semantic-view.rst` (+4 lines) — same identical `.. note::`.
- `README.md` (+2 lines) — Markdown blockquote callout at end of `## Quick start` section linking to the docs-site Read-Only Databases anchor.

**Files created (1):**

- `examples/readonly_load.py` (191 lines) — PEP-723 script (`duckdb==1.5.2` only) with two scenarios: (1) bootstrap in subprocess → reopen read-only in parent → query → DROP catches read-only error; (2) fresh read-only DB → empty list → describe missing view → CREATE catches catalog error. Mirrors the `bootstrap_in_subprocess` pattern from `test/integration/test_readonly_load.py` to sidestep the Phase 62 in-process RW→RO hang.

## Verification

| Step | Command | Result |
|------|---------|--------|
| 1 | `grep -n "## \[0.9.0\]" CHANGELOG.md` | 1 match (line 14) |
| 2 | `grep -n "_explanation-txn-ddl-readonly" docs/explanation/transactional-ddl-and-limitations.rst` | 2 matches (label + Summary cross-ref) |
| 3 | `grep -c "Requires a writable database" docs/reference/{create,drop,alter}-semantic-view.rst` | 1 each |
| 4 | `grep -n "Read-only databases" README.md` | 1 match (line 62) |
| 5 | `uv run examples/readonly_load.py` | exits 0; both scenarios print expected output (LOAD OK on read-only DB; queries return rows; DDL fails as expected) |
| 6 | `just docs-check` (Sphinx `-W`) | green; clean rebuild from empty `_build` confirms no broken refs |
| 7 | `just ci` | green; full chain (`lint test-all check-fuzz docs-check`) including `test_readonly_load.py 3/3 PASS` and clippy/fmt passing |
| 8 | `git branch --show-current` | `milestone/v0.9.0` |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] Example hung in single process (Phase 62 OverrideContext leak)**

- **Found during:** Task 5 verification. The first draft of `examples/readonly_load.py` (faithful to the plan body, which used in-process `rw = duckdb.connect(...)`/`rw.close()` followed by `ro = duckdb.connect(..., read_only=True)`) hung indefinitely at the read-only reopen step. Verified by `ps aux`: two Python processes spinning at 100% CPU each, no log output.
- **Issue:** Phase 62's `OverrideContext` is attached per-DBConfig and keeps the catalog connection alive until process exit. After loading the extension into a writable in-process connection, the same DB cannot be reopened read-only in the SAME process. This is a known limitation (`deferred-items.md` §"In-process RW→RO reopen…") that the Plan 02 integration test already worked around with a `bootstrap_in_subprocess` helper.
- **Fix:** Adopt the same subprocess-bootstrap pattern in the example. Bootstrap step (open writable + LOAD + CREATE + close) runs in `subprocess.run([sys.executable, "-c", script])`; the parent reopens the same file read-only after the child exits. Documented inline in the module docstring + a dedicated comment block before `bootstrap_in_subprocess()`.
- **Files modified:** `examples/readonly_load.py` (rewritten before first commit; no separate commit)
- **Commit:** `d3a99ec` (folded into the Task 5 commit since the in-process draft never landed)

### Documented Carryovers

- **Plan 02 deferred `just ci`** to Plan 04 because docs-check would fail until Plan 03 landed. Plan 03 ran `just ci` end-to-end and it is now green — Plan 04 will re-run as the final pre-tag gate per CLAUDE.md.
- **Pre-existing clippy errors documented in deferred-items.md:** Plan 01's deferred clippy backlog must have been cleaned up between Plan 01 and Plan 03 (or the gate relaxed) — `just ci` includes `lint` and exits 0, so the practical state is now green. No Phase 63 action required.

## Authentication Gates

None — no auth steps required for this plan.

## Branch + Hand-off

- **Branch:** `milestone/v0.9.0` (verified before each commit)
- **Commits added by this plan (5):**
  - `4e171ce` — `docs(63-03): add [0.9.0] CHANGELOG section for read-only LOAD support`
  - `9cfa83b` — `docs(63-03): add Read-Only Databases explanation section`
  - `4678cef` — `docs(63-03): add read-only constraint note to CREATE/DROP/ALTER reference pages`
  - `a4cbe8a` — `docs(63-03): add read-only support callout to README Quick start`
  - `d3a99ec` — `docs(63-03): add examples/readonly_load.py end-to-end demo`
- **Hand-off:** Plan 04 (release) bumps `Cargo.toml` + `description.yml` to `0.9.0`, re-runs `just ci` for the final pre-tag green build, and tags `v0.9.0`. The `## [0.9.0] - 2026-05-15` date in CHANGELOG.md should be re-confirmed against the actual tag day; if Plan 04 lands on a different date, update the heading and the bottom-of-file compare link (the link target depends only on the tag string, not the date).

## Self-Check: PASSED

Verified files exist:
- FOUND: `CHANGELOG.md` (modified, 283 lines)
- FOUND: `docs/explanation/transactional-ddl-and-limitations.rst` (modified, 221 lines)
- FOUND: `docs/reference/create-semantic-view.rst` (modified, 698 lines)
- FOUND: `docs/reference/drop-semantic-view.rst` (modified, 63 lines)
- FOUND: `docs/reference/alter-semantic-view.rst` (modified, 191 lines)
- FOUND: `README.md` (modified, 261 lines)
- FOUND: `examples/readonly_load.py` (created, 191 lines)
- FOUND: `.planning/phases/63-readonly-database-load-support/63-03-SUMMARY.md` (this file)

Verified commits:
- FOUND: `4e171ce` — Task 1 CHANGELOG
- FOUND: `9cfa83b` — Task 2 explanation section
- FOUND: `4678cef` — Task 3 reference notes
- FOUND: `a4cbe8a` — Task 4 README callout
- FOUND: `d3a99ec` — Task 5 example script
