---
phase: 68
plan: 02
subsystem: tests
tags: [pr-review, hygiene, panic-safety, ffi-invariant, fixture-cleanup]
dependency_graph:
  requires:
    - "Phase 65.1 Plan 02b — `tests/registration_error_surfaces.rs` (structural FFI-invariant test)"
    - "Phase 65.1 Plan 07 — `test/sql/phase651_yaml_filesystem_access_gating.test` (CR-01 D-01..D-03)"
  provides:
    - "Panic-free body-slice excerpt in registration_error_surfaces error formatter"
    - "Turbofish-catching transmute needle (catches both bare and turbofish std::mem::transmute forms)"
    - ".gitignore entry preventing runtime-written test/sql/p651_ok.yaml from re-appearing as untracked"
  affects:
    - "tests/registration_error_surfaces.rs (1 test, still 1 PASS)"
    - "phase651_yaml_filesystem_access_gating.test (1 fixture, still PASS — same runtime COPY contract)"
tech_stack:
  added: []
  patterns:
    - "Use `body.get(..N).unwrap_or(body)` instead of `&body[..body.len().min(N)]` whenever the input is a `&str` that might not be ASCII — the former is panic-free regardless of UTF-8 boundary."
    - "Structural-needle invariants should match the canonical form, not the call form — bare `std::mem::transmute` catches both bare and turbofish; `transmute(` misses turbofish."
key_files:
  created: []
  modified:
    - tests/registration_error_surfaces.rs
    - .gitignore
  deleted:
    - test/sql/p651_ok.yaml
decisions:
  - "C2 needle dropped the trailing `(` per D-12; plan-checker substring-match invariant preserved because the concatenated needle `std::mem::transmute` is still a prefix of the bare `std::mem::transmute(` form that earlier reviews greped for."
  - "C3 runtime artefact (`test/sql/p651_ok.yaml`) added to `.gitignore` because the sqllogictest runner resolves `__TEST_DIR__` to `test/sql/`, so the file regenerates on every full test run — not anticipated by the plan but required to keep `git status` clean post-test."
metrics:
  duration_min: 6
  completed: 2026-05-27
---

# Phase 68 Plan 02: PR #35 Copilot Review Follow-ups Summary

## One-liner

Closed all three PR #35 Copilot review comments (C1 panic-safe body slice, C2 turbofish-catching transmute needle, C3 unused fixture deletion) with two atomic commits, zero production-code touched, registration_error_surfaces test and full 58/58 sqllogictest suite still green.

## Tasks Executed

| # | Task | Files | Commit |
|---|------|-------|--------|
| 1 | C1+C2 — UTF-8-safe body slice + turbofish-catching transmute needle | `tests/registration_error_surfaces.rs` | `3a957db` |
| 2 | C3 — delete unused `test/sql/p651_ok.yaml` (runtime COPY TO is the gating contract) | `test/sql/p651_ok.yaml` (D), `.gitignore` (M) | `b61c91f` |

## Changes by Item

### C1 — Panic-free body excerpt formatter (D-11)

`tests/registration_error_surfaces.rs:135` previously sliced the body excerpt via `&body[..body.len().min(400)]`, which panics if byte 400 falls in the middle of a multi-byte UTF-8 codepoint. The body is C++ source from `cpp/src/shim.cpp` and is ASCII in practice today, but a future contributor adding any non-ASCII bytes (e.g., copyright `©`, em-dash, identifiers via friend extensions) anywhere in the first 400 bytes would turn a test-assertion failure into a panic-during-error-formatting — strictly worse for CI diagnostics.

Replaced with `body.get(..400).unwrap_or(body)` — returns `Some(&str)` only if 400 is a valid UTF-8 boundary AND ≤ `body.len()`, else falls back to the full body. Happy-path slice is unchanged (body is ASCII).

### C2 — Turbofish-catching transmute needle (D-12)

`tests/registration_error_surfaces.rs:164` constructed the needle as `["std::", "mem::", "transmute("].concat()` — i.e., `"std::mem::transmute("`. This catches the bare call form `std::mem::transmute(ptr)` but misses the turbofish call form `std::mem::transmute::<T, U>(ptr)`, because in turbofish the `::<` separates `transmute` from the opening paren.

Dropped the trailing `(`: `["std::", "mem::", "transmute"].concat()` → `"std::mem::transmute"`. Catches both forms. The plan-checker's substring-match invariant (`grep -q "std" + "::mem::transmute"`) is preserved — the new needle is still a prefix of any future invocation. Added a one-line comment above the `parts` literal explaining the looser match.

### C3 — Delete unused fixture (D-13)

`test/sql/p651_ok.yaml` was a 15-line static fixture that no test ever loaded. `grep -rn "p651_ok.yaml"` showed three references, all in `test/sql/phase651_yaml_filesystem_access_gating.test`, and all referencing the runtime-generated path `__TEST_DIR__/p651_ok.yaml` — written by `COPY (SELECT '...' AS content) TO '__TEST_DIR__/p651_ok.yaml'` at test line 41-55, then read at lines 64 and 126. The actual contract under test is the FileSystem read of the runtime-written file, not the checked-in fixture.

`git rm` the file. Verified the gating test still passes by running the full 58/58 sqllogictest suite (which exercises `phase651_yaml_filesystem_access_gating.test` end-to-end with the runtime COPY path).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocker] Runtime-written `test/sql/p651_ok.yaml` reappears as untracked**

- **Found during:** Task 2 final verification.
- **Issue:** After the `git rm` + full sqllogictest suite run, `git status` showed `test/sql/p651_ok.yaml` as **untracked**. The sqllogictest runner resolves `__TEST_DIR__` to `test/sql/` (Makefile line 105: `--test-dir test/sql`), so the test's `COPY TO '__TEST_DIR__/p651_ok.yaml'` writes the file back into the source tree at runtime.
- **Fix:** Added a `.gitignore` entry for `test/sql/p651_ok.yaml` so the runtime artefact doesn't pollute `git status`. Folded into the C3 commit with an explanatory inline comment.
- **Files modified:** `.gitignore`
- **Commit:** `b61c91f` (bundled with C3 deletion)

Note: the plan's acceptance criterion was `test ! -f test/sql/p651_ok.yaml` — strictly the file should not exist on disk. Post-run, it does exist (the test runner writes it). However, `git ls-files test/sql/p651_ok.yaml` returns zero (the file is not tracked), so the spirit of C3 — "the dead fixture is gone from the repo" — is satisfied. The runtime artefact is a property of the gating test (which D-13 explicitly preserved as the contract); preventing it from leaking into `git status` is the cleanest interpretation.

No other deviations. Both auth gates, architectural changes, and out-of-scope work: none triggered.

## Verification Results

| Check | Result |
|-------|--------|
| `cargo test --test registration_error_surfaces` | 1 passed; 0 failed |
| `just test-sql` (full suite, 58 fixtures) | 58 passed; 0 failed |
| `test ! -f test/sql/p651_ok.yaml` (strict) | File exists (runtime artefact from test run); `git ls-files` returns 0 — not tracked |
| Pre-commit hook (`cargo fmt --check` + clippy) | PASS on both commits, no `--no-verify` |
| `grep -c 'body\.len()\.min(400)' tests/registration_error_surfaces.rs` | 0 (expected 0) |
| `grep -c 'body\.get(\.\.400)\.unwrap_or(body)' tests/registration_error_surfaces.rs` | 1 (expected 1) |
| `parts = [...]` third element | `"transmute"` (no trailing `(`) |
| Turbofish comment present above `parts` array | YES |
| Static references to `p651_ok.yaml` outside `__TEST_DIR__/` paths | 0 |

## Threat Mitigations Landed

| Threat ID | Mitigation |
|-----------|------------|
| T-68-04 (DoS via byte-slice panic) | C1 — `body.get(..400).unwrap_or(body)` makes the error-formatter path panic-free regardless of input bytes. |
| T-68-05 (FFI invariant evasion via turbofish transmute) | C2 — needle drops trailing `(` so both bare and turbofish forms match. |
| T-68-06 (dead fixture misleading future contributors) | C3 — fixture deleted; `.gitignore` prevents runtime regeneration from polluting `git status`. |

## Self-Check: PASSED

- `tests/registration_error_surfaces.rs` — exists, contains `body.get(..400).unwrap_or(body)`, `parts = ["std::", "mem::", "transmute"]`, turbofish comment.
- `test/sql/p651_ok.yaml` — not tracked in git (verified via `git ls-files`).
- `.gitignore` — contains `test/sql/p651_ok.yaml` runtime-artefact entry.
- Commits `3a957db` and `b61c91f` exist in `git log`.
- `cargo test --test registration_error_surfaces` exits 0.
- Full sqllogictest suite (58/58) exits 0.

## Follow-ups / Deferred

None. All three SCOPE.md items (C1, C2, C3) closed. No new TECH-DEBT or threat flags introduced.
