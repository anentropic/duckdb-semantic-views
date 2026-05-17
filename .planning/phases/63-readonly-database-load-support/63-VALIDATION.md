---
phase: 63
slug: readonly-database-load-support
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-15
---

# Phase 63 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Source of truth: `63-RESEARCH.md` §4 "Validation Architecture".

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Frameworks** | `cargo test` (Rust unit + proptest + doctest), `sqllogictest` via `just test-sql`, `pytest`-style Python integration via `uv run` |
| **Config files** | `Cargo.toml`, `justfile` (recipe `test-readonly` to be added in this phase), `test/sql/readonly_load.test` (NEW), `test/integration/test_readonly_load.py` (NEW) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` (must include new `test-readonly` recipe) |
| **Phase gate command** | `just ci` |
| **Estimated runtime** | ~5–15 s quick; ~5–10 min full; ~6–12 min phase gate |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd-verify-work`:** Run `just ci` (must be green)
- **Max feedback latency:** ~15 s (cargo test) per task; ~10 min full suite per wave

---

## Per-Task Verification Map

> See RESEARCH §4 "Phase Requirements → Test Map" for the canonical mapping. Plan tasks must reference these test artifacts in their `<acceptance_criteria>`.

| Test artifact | REQ coverage | Type | Command | Status |
|---------------|--------------|------|---------|--------|
| `src/catalog.rs` unit `init_catalog_skips_writes_on_readonly` | RO-01 | unit | `cargo test catalog::tests::init_catalog_skips_writes_on_readonly` | ❌ W0 |
| `src/catalog.rs` unit `lookup_returns_none_when_table_missing` | RO-04 | unit | `cargo test catalog::tests::lookup_returns_none_when_table_missing` | ❌ W0 |
| `src/catalog.rs` unit `list_all_returns_empty_when_table_missing` | RO-03 | unit | `cargo test catalog::tests::list_all_returns_empty_when_table_missing` | ❌ W0 |
| `src/catalog.rs` unit `list_names_returns_empty_when_table_missing` | RO-03 | unit | `cargo test catalog::tests::list_names_returns_empty_when_table_missing` | ❌ W0 |
| `src/lib.rs` unit pinning `current_setting('access_mode')` lowercased | RO-01 | unit | `cargo test lib::tests::access_mode_lowercased_on_readonly_open` | ❌ W0 |
| `test/integration/test_readonly_load.py::test_fresh_readonly_empty_list` | RO-01, RO-03, RO-04 | integration | `uv run test/integration/test_readonly_load.py` | ❌ W0 |
| `test/integration/test_readonly_load.py::test_bootstrapped_readonly_query_works` | RO-01, RO-02 | integration | same | ❌ W0 |
| `test/integration/test_readonly_load.py::test_readonly_ddl_fails` | RO-05 | integration | same | ❌ W0 |
| `test/sql/readonly_load.test` (subject to Wave 0 spike) | TEST-01 | sqllogictest | `just test-sql` | ❌ W0 |
| `just test-all` chain includes `test-readonly` | TEST-02, TEST-03 | CI | `just test-all` | ❌ W0 |
| `just ci` green | TEST-03 | CI | `just ci` | runs after all above land |
| Manual review of `CHANGELOG.md` `## [0.9.0]` section | DOC-01 | review | `grep -n '## \[0.9.0\]' CHANGELOG.md` | ❌ W0 |
| Manual review of `docs/explanation/transactional-ddl-and-limitations.rst` "Read-only databases" section | DOC-02 | review + docs build | `just docs-check` | ❌ W0 |
| Manual review of three reference pages with one-line note | DOC-03 | review | `grep -nE 'writable database' docs/reference/{create,drop,alter}-semantic-view.rst` | ❌ W0 |
| Manual review of `README.md` LOAD section mentions read-only | DOC-04 | review | `grep -n 'read.only' README.md` | ❌ W0 |
| `examples/readonly_load.py` runs end-to-end | DOC-05 | smoke run | `uv run examples/readonly_load.py` | ❌ W0 |
| `Cargo.toml` and `description.yml` bumped to `0.9.0` | REL-01 | review | `grep -nE '^(version|extension):' Cargo.toml description.yml` | ❌ W0 |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky · W0 = file does not yet exist; created during Wave 0*

---

## Wave 0 Requirements

- [ ] **Spike**: `grep -rn 'readonly' python_runner/` to determine whether the project's Python sqllogictest runner supports `load <path> readonly`. Outcome decides scope of `test/sql/readonly_load.test` (full vs. minimal smoke + deferral comment).
- [ ] `test/integration/test_readonly_load.py` — three test functions plus shared connection-builder helper modelled on `test/integration/test_multi_db_isolation.py:45-57`.
- [ ] `test/sql/readonly_load.test` — populated per spike outcome.
- [ ] `src/catalog.rs::tests` — four unit tests (`init_catalog_skips_writes_on_readonly`, `lookup_returns_none_when_table_missing`, `list_all_returns_empty_when_table_missing`, `list_names_returns_empty_when_table_missing`).
- [ ] `src/lib.rs::tests` — one unit test pinning lowercased `read_only` from `current_setting('access_mode')`.
- [ ] `justfile` — new `test-readonly` recipe + add to `test-all` chain.
- [ ] `CHANGELOG.md` — `## [0.9.0]` section under standard headings; `[Unreleased]` reset; bottom-of-file compare links updated.
- [ ] `docs/explanation/transactional-ddl-and-limitations.rst` — new "Read-only databases" subsection.
- [ ] `docs/reference/{create,drop,alter}-semantic-view.rst` — one-line `.. note::` after Variants section in each.
- [ ] `README.md` — one-line note in Quick start.
- [ ] `examples/readonly_load.py` — full PEP-723 script mirroring `examples/transactional_ddl.py`.
- [ ] `Cargo.toml` + `description.yml` — version bump to `0.9.0`.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CHANGELOG entry under correct Keep-a-Changelog headings | DOC-01 | Style/format judgement | Visual review against CHANGELOG.md format rules in `CLAUDE.md` |
| Docs site renders the new "Read-only databases" section without broken refs | DOC-02 | Sphinx warnings need human eyes | `just docs-check` and inspect output |
| README LOAD section reads naturally (no awkward insertion) | DOC-04 | Prose flow judgement | Read README.md "Quick start" / "Load" section after edit |

---

## Coverage Strategy — RO-05 Acceptance Flexibility

DuckDB read-only error message (`cpp/include/duckdb.cpp:273011-273013`):

> `Cannot execute statement of type "INSERT" on database "<name>" which is attached in read-only mode!`

Tests must assert on substring `"read-only"` (case-insensitive). Strict full-sentence matching is brittle and not required by RO-05 ("DuckDB's standard 'cannot write to read-only database' error or the closest equivalent").

For DROP on a fresh read-only DB without `IF EXISTS`, accept either:
- DuckDB's read-only error (preferred), OR
- A "does not exist" error (catalog miss occurs before INSERT in the rewrite pipeline).

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify command or list a Wave 0 dependency
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all `❌ W0` references in the task map above
- [ ] No watch-mode flags in any test command
- [ ] Feedback latency budget met (~15 s per task, ~10 min per wave)
- [ ] `nyquist_compliant: true` set in frontmatter once plan is finalised and all task maps land

**Approval:** pending
