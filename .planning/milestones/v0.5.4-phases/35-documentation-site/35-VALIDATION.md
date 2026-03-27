---
phase: 35
slug: documentation-site
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-27
---

# Phase 35 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Sphinx build + GitHub Actions |
| **Config file** | `docs/conf.py` |
| **Quick run command** | `uv run --project docs sphinx-build -W -b html docs docs/_build/html` |
| **Full suite command** | `just docs-build` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `uv run --project docs sphinx-build -W -b html docs docs/_build/html`
- **After every plan wave:** Run `just docs-build`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 35-01-01 | 01 | 1 | DOCS-01 | build | `uv run --project docs sphinx-build -W -b html docs docs/_build/html` | :white_check_mark: | :white_square_button: pending |
| 35-01-02 | 01 | 1 | DOCS-02 | workflow | `cat .github/workflows/Docs.yml` | :x: W0 | :white_square_button: pending |
| 35-01-03 | 01 | 1 | DOCS-03 | build | `ls docs/_build/html/tutorials/ docs/_build/html/reference/ docs/_build/html/how-to/` | :white_check_mark: | :white_square_button: pending |
| 35-01-04 | 01 | 1 | DOCS-04 | grep | `grep -i 'documentation\|docs' README.md` | :x: W0 | :white_square_button: pending |

*Status: :white_square_button: pending · :white_check_mark: green · :x: red · :warning: flaky*

---

## Wave 0 Requirements

- [ ] `.github/workflows/Docs.yml` — GitHub Pages deployment workflow
- [ ] PullRequestCI.yml docs job — build check on PRs

*Existing infrastructure covers docs build; CI workflows need creation.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| GitHub Pages live deployment | DOCS-02 | Requires repo settings change + push to main | 1. Set Pages source to "GitHub Actions" in repo settings. 2. Push to main. 3. Verify site loads at anentropic.github.io/duckdb-semantic-views/ |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
