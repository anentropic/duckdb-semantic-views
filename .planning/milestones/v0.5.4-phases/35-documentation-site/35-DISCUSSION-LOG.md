# Phase 35: Documentation Site - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md -- this log preserves the alternatives considered.

**Date:** 2026-03-27
**Phase:** 35-documentation-site
**Areas discussed:** Deployment mechanism, CI build validation, README integration, Requirements alignment
**Mode:** --auto (all decisions auto-selected)

---

## Deployment Mechanism

| Option | Description | Selected |
|--------|-------------|----------|
| actions/deploy-pages | GitHub's built-in Pages deployment action, no third-party deps | :heavy_check_mark: |
| peaceiris/actions-gh-pages | Popular third-party action, more features | |
| Branch-based deploy | Push built HTML to gh-pages branch | |

**User's choice:** [auto] actions/deploy-pages (recommended default)
**Notes:** Official GitHub action, no supply chain risk from third-party actions. Matches modern GitHub Pages best practice.

---

## CI Build Validation

| Option | Description | Selected |
|--------|-------------|----------|
| Add to PullRequestCI.yml | Run sphinx-build -W on PRs to catch broken docs before merge | :heavy_check_mark: |
| Deploy-only validation | Only validate docs during deployment (errors caught late) | |
| Separate docs CI workflow | Dedicated workflow for docs PRs (overhead for small project) | |

**User's choice:** [auto] Add to PullRequestCI.yml (recommended default)
**Notes:** Catches broken cross-references, missing includes, and RST syntax errors before merge.

---

## README Integration

| Option | Description | Selected |
|--------|-------------|----------|
| Badge + prose link | shields.io badge at top + documentation link in body | :heavy_check_mark: |
| Badge only | Just the badge, no prose link | |
| Prose link only | Text link in body, no badge | |

**User's choice:** [auto] Badge + prose link (recommended default)
**Notes:** Standard open source pattern. Badge provides visual indicator, prose link provides context.

---

## Requirements Alignment

| Option | Description | Selected |
|--------|-------------|----------|
| Update to Sphinx + Shibuya | Change DOCS-01 and ROADMAP to reflect actual framework choice | :heavy_check_mark: |
| Keep as-is | Leave Zensical/mkdocs-material references (would confuse downstream agents) | |

**User's choice:** [auto] Update to Sphinx + Shibuya (recommended default)
**Notes:** ROADMAP said mkdocs-material, REQUIREMENTS said Zensical, but actual implementation is Sphinx + Shibuya. Updating prevents confusion.

---

## Claude's Discretion

- Exact GitHub Actions workflow YAML structure and job naming
- Whether to add `.nojekyll` file
- sphinx-build warning handling details
- Badge style/color choices

## Deferred Ideas

None -- all discussion stayed within phase scope.
