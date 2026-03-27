---
phase: 33
slug: unique-constraints-cardinality-inference
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-15
---

# Phase 33 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust test + sqllogictest + Python integration |
| **Config file** | `Cargo.toml` (test config), `justfile` (task runner) |
| **Quick run command** | `cargo test` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 33-01-01 | 01 | 1 | CARD-01 | unit | `cargo test body_parser -- unique` | ❌ W0 | ⬜ pending |
| 33-01-02 | 01 | 1 | CARD-01 | unit | `cargo test model -- unique` | ❌ W0 | ⬜ pending |
| 33-01-03 | 01 | 1 | CARD-02 | unit | `cargo test body_parser -- multiple_unique` | ❌ W0 | ⬜ pending |
| 33-01-04 | 01 | 1 | CARD-03 | unit + sql | `cargo test graph -- fk_ref_validation` | ❌ W0 | ⬜ pending |
| 33-01-05 | 01 | 1 | CARD-04 | unit | `cargo test parse -- infer_cardinality` | ❌ W0 | ⬜ pending |
| 33-01-06 | 01 | 1 | CARD-05 | unit + sql | `cargo test body_parser -- no_cardinality_keywords` | ❌ W0 | ⬜ pending |
| 33-01-07 | 01 | 1 | CARD-06 | unit | `cargo test model -- cardinality_enum` | ❌ W0 | ⬜ pending |
| 33-01-08 | 01 | 1 | CARD-07 | unit + sql | `cargo test body_parser -- references_column_list` | ❌ W0 | ⬜ pending |
| 33-01-09 | 01 | 1 | CARD-08 | sql | `just test-sql` | ❌ W0 | ⬜ pending |
| 33-01-10 | 01 | 1 | CARD-09 | unit | `cargo test graph -- composite_fk_subset` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase33_cardinality_inference.test` — end-to-end sqllogictest for CARD-01 through CARD-09
- [ ] Update `test/sql/phase31_fan_trap.test` — remove cardinality keywords, use new syntax
- [ ] Update `test/sql/phase32_role_playing.test` — remove cardinality keywords, add UNIQUE where needed
- [ ] Update `test/sql/phase26_join_resolution.test` — if it uses cardinality keywords
- [ ] Unit tests in `src/model.rs` — Cardinality two-variant enum, TableRef with unique_constraints, Join with ref_columns
- [ ] Unit tests in `src/body_parser.rs` — UNIQUE parsing, REFERENCES(cols) parsing, no cardinality tokens
- [ ] Unit tests in `src/graph.rs` — CARD-03/09 validation functions
- [ ] Unit tests in `src/parse.rs` — cardinality inference function
- [ ] Property-based tests in `tests/expand_proptest.rs` and `tests/parse_proptest.rs` — may need updates for new model fields

*Existing infrastructure covers unit and sql test patterns; new test files needed for phase-specific behaviors.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
