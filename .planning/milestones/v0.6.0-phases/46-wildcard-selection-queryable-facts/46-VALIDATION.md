---
phase: 46
slug: wildcard-selection-queryable-facts
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-12
---

# Phase 46 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust unit/proptest) + sqllogictest-rs |
| **Config file** | test/sql/TEST_LIST (test manifest) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 46-01-01 | 01 | 1 | WILD-01 | T-46-01 | Alias validated against def.tables allowlist | unit + slt | `cargo test wildcard` / `just test-sql` | ❌ W0 | ⬜ pending |
| 46-01-02 | 01 | 1 | WILD-02 | T-46-01 | Alias validated against def.tables allowlist | unit + slt | `cargo test wildcard` / `just test-sql` | ❌ W0 | ⬜ pending |
| 46-01-03 | 01 | 1 | WILD-03 | T-46-02 | PRIVATE items filtered by AccessModifier check | unit | `cargo test wildcard_private` | ❌ W0 | ⬜ pending |
| 46-02-01 | 02 | 1 | FACT-01 | — | N/A | unit + slt | `cargo test fact_query` / `just test-sql` | ❌ W0 | ⬜ pending |
| 46-02-02 | 02 | 1 | FACT-02 | — | N/A | slt | `just test-sql` | ❌ W0 | ⬜ pending |
| 46-02-03 | 02 | 1 | FACT-03 | — | N/A | unit + slt | `cargo test facts_metrics_mutual` / `just test-sql` | ❌ W0 | ⬜ pending |
| 46-02-04 | 02 | 1 | FACT-04 | — | N/A | unit | `cargo test fact_path` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase46_wildcard.test` — covers WILD-01, WILD-02, WILD-03
- [ ] `test/sql/phase46_fact_query.test` — covers FACT-01, FACT-02, FACT-03, FACT-04
- [ ] Unit tests in `src/expand/sql_gen.rs` — wildcard expansion, fact SQL generation
- [ ] Unit tests for fact path validation
- [ ] Add test files to `test/sql/TEST_LIST`

*Existing infrastructure covers test framework — no framework install needed.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
