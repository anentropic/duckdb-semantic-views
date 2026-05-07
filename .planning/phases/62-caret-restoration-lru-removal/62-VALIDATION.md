---
phase: 62
slug: caret-restoration-lru-removal
status: complete
nyquist_compliant: true
wave_0_complete: true
created: 2026-05-06
updated: 2026-05-06
---

# Phase 62 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. See `62-RESEARCH.md` §6 (Validation Architecture) for the canonical test inventory; this file is the executable contract derived from it.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust unit + proptest + sqllogictest + ADBC/Python integration |
| **Config file** | `Cargo.toml`, `tests/sqllogictest/`, `justfile` |
| **Quick run command** | `cargo test --no-default-features` |
| **Full suite command** | `just test-all` (Rust unit + proptest + sqllogictest + DuckLake CI) |
| **Estimated runtime** | ~120 seconds (full suite); ~25 seconds (cargo test only) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --no-default-features` (Rust unit + proptest)
- **After every plan wave:** Run `just test-all` (full suite — required by CLAUDE.md quality gate)
- **Before `/gsd-verify-work`:** Full suite must be green; `just ci` (clippy + fmt + cargo-deny + fuzz compile) must pass
- **Max feedback latency:** 30 seconds for unit/proptest; 120 seconds for full suite

---

## Per-Task Verification Map

> Populated by the planner. Every task in `62-NN-PLAN.md` files MUST appear here with an automated verification command OR a Wave 0 dependency that installs the test fixture before the task runs.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 62-01-01 | 01 | 0 | (none — internal) | — | N/A | scaffold | (Wave 0 install) | ❌ W0 | ⬜ pending |
| _to be filled by planner_ | | | | | | | | | |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Derived from `62-RESEARCH.md` §6 ("Wave 0 gap list"). Planner MUST include a Wave 0 plan that creates / scaffolds these before functional tasks run:

- [ ] `tests/sqllogictest/error_caret_create.test` — caret rendering for CREATE error paths (CREATE TABLE-clause failures, duplicate dimension, unknown column)
- [ ] `tests/sqllogictest/error_caret_drop.test` — caret rendering for DROP failures (non-existent view without IF EXISTS)
- [ ] `tests/sqllogictest/error_caret_alter.test` — caret rendering for ALTER failures (non-existent view, invalid clause)
- [ ] `tests/sqllogictest/error_caret_multiline.test` — multi-line statement caret position correctness
- [ ] `tests/sqllogictest/error_caret_unicode.test` — Unicode column reporting (B5 from research)
- [ ] `tests/sqllogictest/lru_removed_isolation.test` — multi-DB isolation preserved without LRU; concurrent open/close of >16 DBs does not silently evict
- [ ] `tests/concurrent_create.py` (extend) — concurrent CREATE on >16 attached DBs verifies no eviction-induced failure
- [ ] `csrc/parser_extension_compat.hpp` — `static_assert(sizeof(ParserExtensionParseResult) == …)` to pin layout (Risk F from research)
- [ ] `tests/proptest_caret_position.rs` — proptest: position byte offset is preserved through `validate_and_rewrite` for arbitrary leading whitespace / comments

*Reference: research §6 (Validation Architecture) lists 19 behavioural test properties; Wave 0 covers the gap list. Planner is free to consolidate into fewer files but must keep all behavioural properties covered.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Caret rendering visual fidelity in `duckdb` CLI | (TECH-DEBT 22) | Terminal rendering of `LINE 1: …^` is human-perceptible; sqllogictest matches the textual error but not visual alignment | Run `duckdb` CLI, attempt `CREATE SEMANTIC VIEW bad AS TABLES (missing_table)`, confirm caret aligns under the offending token |
| ADBC end-to-end with caret | (TECH-DEBT 22) | ADBC error propagation differs from CLI; verify `error_location` survives the ADBC boundary | Run `python examples/race_guards_and_unification.py` with deliberate bad CREATE; inspect ADBC error message |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references (9 items above)
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s for unit/proptest; < 120s for full suite
- [ ] `just test-all` green BEFORE phase verification (CLAUDE.md quality gate)
- [ ] `nyquist_compliant: true` set in frontmatter (set by planner once all tasks mapped)

**Approval:** pending
