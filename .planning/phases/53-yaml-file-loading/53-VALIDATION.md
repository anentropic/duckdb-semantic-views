---
phase: 53
slug: yaml-file-loading
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-18
---

# Phase 53 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) + sqllogictest runner |
| **Config file** | Cargo.toml + test/sql/*.test |
| **Quick run command** | `cargo test parse::tests::yaml_file` |
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
| 53-01-01 | 01 | 1 | YAML-02 | — | N/A | unit | `cargo test parse::tests::yaml_file` | ❌ W0 | ⬜ pending |
| 53-01-02 | 01 | 1 | YAML-02 | — | N/A | unit | `cargo test parse::tests::yaml_file` | ❌ W0 | ⬜ pending |
| 53-01-03 | 01 | 1 | YAML-02 | T-53-01 | File path SQL-escaped before read_text query | unit | `cargo test parse::tests::yaml_file` | ❌ W0 | ⬜ pending |
| 53-01-04 | 01 | 1 | YAML-02 | — | N/A | integration | `just test-sql` | ❌ W0 | ⬜ pending |
| 53-01-05 | 01 | 1 | YAML-07 | T-53-02 | enable_external_access=false blocks read_text | integration | `just test-sql` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase53_yaml_file.test` — sqllogictest integration tests for FROM YAML FILE
- [ ] Unit tests in `src/parse.rs` for `extract_single_quoted`, `rewrite_ddl_yaml_file_body`, FROM YAML FILE detection

*Existing infrastructure covers YAML parsing (Phase 51) and DDL integration (Phase 52).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Cloud storage paths (S3/GCS) | — | Requires cloud credentials | `FROM YAML FILE 's3://bucket/def.yaml'` with httpfs loaded |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
