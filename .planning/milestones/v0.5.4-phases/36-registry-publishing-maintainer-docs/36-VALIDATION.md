---
phase: 36
slug: registry-publishing-maintainer-docs
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-27
---

# Phase 36 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | grep/file checks + just test-all |
| **Config file** | N/A (no new test framework) |
| **Quick run command** | `grep -c "name: semantic_views" description.yml && just test-rust` |
| **Full suite command** | `just test-all` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run quick check (grep for key fields)
- **After every plan wave:** Run `just test-all`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 36-01-01 | 01 | 1 | CREG-01 | grep | `grep -c "name: semantic_views" description.yml` | :x: W0 | :white_square_button: pending |
| 36-01-02 | 01 | 1 | CREG-03 | grep | `grep -c "hello_world" description.yml` | :x: W0 | :white_square_button: pending |
| 36-02-01 | 02 | 1 | MAINT-01 | grep | `grep -c "Multi-Version" MAINTAINER.md` | :white_check_mark: | :white_square_button: pending |
| 36-02-02 | 02 | 1 | MAINT-02 | grep | `grep -c "Subsequent Releases" MAINTAINER.md` | :white_check_mark: | :white_square_button: pending |
| 36-02-03 | 02 | 1 | MAINT-03 | grep | `grep -c "Bump DuckDB" MAINTAINER.md` | :x: W0 | :white_square_button: pending |

*Status: :white_square_button: pending · :white_check_mark: green · :x: red · :warning: flaky*

---

## Wave 0 Requirements

- [ ] `description.yml` — CE registry descriptor (CREG-01)
- [ ] MAINTAINER.md "How to Bump DuckDB Version" section — (MAINT-03)

*Existing infrastructure covers test suite; new artifacts need creation.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CE draft PR submission | CREG-04 | Requires GitHub fork + PR creation | 1. Fork duckdb/community-extensions. 2. Copy description.yml. 3. Submit draft PR. 4. Verify CI passes. |
| Extension installable from community | CREG-05 | Requires CE pipeline to complete build | 1. Wait for CE build. 2. Run INSTALL semantic_views FROM community. 3. LOAD semantic_views. 4. Run hello_world query. |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
