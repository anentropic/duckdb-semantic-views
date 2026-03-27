# Phase 35: Documentation Site - Research

**Researched:** 2026-03-27
**Domain:** Sphinx documentation deployment to GitHub Pages via GitHub Actions
**Confidence:** HIGH

## Summary

Phase 35 is a CI/CD infrastructure phase. All documentation content is already written (19 RST pages across 4 Diataxis categories, 0 undocumented symbols per gap report). The Sphinx + Shibuya site builds locally with `just docs-build`. The work is: (1) create a GitHub Actions deployment workflow, (2) add a docs build check to PR CI, (3) fix one build warning, (4) add a docs badge and link to README, and (5) update REQUIREMENTS.md and ROADMAP.md to reflect the actual framework.

The existing project has strong CI patterns (`Build.yml`, `PullRequestCI.yml`, `CodeQuality.yml`) that establish naming conventions, trigger patterns, and concurrency settings. The deployment workflow follows GitHub's official `actions/configure-pages` + `actions/upload-pages-artifact` + `actions/deploy-pages` pattern, using `uv` for Python dependency management consistent with the existing `docs/pyproject.toml` setup.

**Primary recommendation:** Use a two-job workflow (build + deploy) in `Docs.yml`, add a sphinx-build check job to `PullRequestCI.yml`, add `sphinx.ext.githubpages` extension to `conf.py` for automatic `.nojekyll` handling, and fix the broken intersphinx mapping that causes `-W` failures.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Sphinx + Shibuya theme (already configured in `docs/conf.py`). NOT mkdocs-material or Zensical.
- **D-02:** sphinx-design extension for grid cards. sphinx-autobuild for local dev. Custom sqlgrammar_lexer for SQL grammar syntax highlighting.
- **D-03:** Build command: `uv run --project docs sphinx-build -b html docs docs/_build/html` (via `just docs-build`)
- **D-04:** Use GitHub's built-in `actions/deploy-pages` + `actions/upload-pages-artifact` for deployment. No third-party actions (e.g., peaceiris/actions-gh-pages).
- **D-05:** Deploy trigger: push to `main` branch only. Milestone branches do NOT trigger deployment.
- **D-06:** Workflow file: `.github/workflows/Docs.yml` (consistent with existing `Build.yml`, `CodeQuality.yml` naming)
- **D-07:** Add Sphinx build check to `PullRequestCI.yml` so broken docs are caught before merge. Runs `sphinx-build` with `-W` (warnings-as-errors) but does NOT deploy.
- **D-08:** Both `Docs.yml` (deploy) and `PullRequestCI.yml` (check) use `uv` for Python dependency management.
- **D-09:** Add a documentation badge (shields.io) near the top of README.
- **D-10:** Add a "Documentation" link in the README body pointing to the GitHub Pages URL.
- **D-11:** GitHub Pages URL format: `https://anentropic.github.io/duckdb-semantic-views/`
- **D-12:** Update REQUIREMENTS.md DOCS-01 from "Zensical" to "Sphinx + Shibuya docs configured with `docs/conf.py` and `docs/pyproject.toml`"
- **D-13:** Update ROADMAP.md Phase 35 framework from "mkdocs-material" to "Sphinx + Shibuya"

### Claude's Discretion
- Exact GitHub Actions workflow YAML structure and job naming
- Whether to add `.nojekyll` file to built output
- sphinx-build warning handling details
- Badge style/color choices

### Deferred Ideas (OUT OF SCOPE)
- None -- discussion stayed within phase scope

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DOCS-01 | Sphinx + Shibuya docs configured with `docs/conf.py` and `docs/pyproject.toml` | Already satisfied -- conf.py and pyproject.toml exist and build works. Requirement text needs updating per D-12. |
| DOCS-02 | GitHub Actions workflow deploys docs to GitHub Pages on push to main | Workflow pattern fully researched: two-job build+deploy using official actions. See Architecture Patterns. |
| DOCS-03 | Site structure includes: getting started, DDL reference, query reference, clause-level pages, examples | Already satisfied -- 19 RST pages verified in place. Gap report confirms 0 undocumented symbols. |
| DOCS-04 | README links to the documentation site | Badge + link pattern researched. See Code Examples. |

</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` (Rust unit tests, property-based tests, sqllogictest, DuckLake CI tests)
- **Build:** `just build` for debug build, `cargo test` for Rust tests, `just test-sql` for SQL logic tests
- **Branch strategy:** All work on `milestone/v0.5.4` branch. Verify current branch before committing.
- **Important:** This phase adds CI/CD workflows and edits documentation metadata -- no Rust code changes, so `just test-all` should be a no-op verification (existing tests must still pass).

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Sphinx | 9.1.0 | Documentation site generator | Already installed in `docs/pyproject.toml`, verified working |
| Shibuya | (via pyproject.toml) | Sphinx theme | Already configured in `docs/conf.py` |
| sphinx-design | (via pyproject.toml) | Grid cards, tabs, dropdowns | Already in use for index.rst grid layout |
| sphinx.ext.githubpages | built-in | Creates `.nojekyll` in output | Built into Sphinx, zero config, solves GitHub Pages Jekyll bypass |

### CI/CD Actions
| Action | Version | Purpose | Why Standard |
|--------|---------|---------|--------------|
| actions/checkout | v4 | Check out repo | Used in all existing workflows |
| actions/setup-python | v5 | Install Python | Required for uv/Sphinx |
| astral-sh/setup-uv | v7 | Install uv package manager | Project uses uv for docs deps (latest stable) |
| actions/configure-pages | v5 | Prepare GitHub Pages environment | Official GitHub pattern |
| actions/upload-pages-artifact | v3 | Upload built HTML as artifact | Official GitHub pattern |
| actions/deploy-pages | v4 | Deploy artifact to GitHub Pages | Official GitHub pattern, per D-04 |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Official deploy-pages | peaceiris/actions-gh-pages | Third-party, REJECTED by D-04 |
| sphinx.ext.githubpages | Manual `touch .nojekyll` | Extension is cleaner, built-in, one-line change |
| actions/configure-pages | Skip it | Needed for Pages env setup; official starter workflows include it |

## Architecture Patterns

### Recommended Workflow Structure

Two-job `Docs.yml` workflow (build then deploy):

```
.github/workflows/
  Build.yml              # existing: extension binaries
  CodeQuality.yml        # existing: lint, format, coverage
  Fuzz.yml               # existing: fuzz targets
  DuckDBVersionMonitor.yml # existing: version check
  PullRequestCI.yml      # existing: PR fast check (+ new docs-check job)
  Docs.yml               # NEW: build + deploy docs to Pages
```

### Pattern 1: Two-Job Build-Deploy Workflow
**What:** Separate `build` and `deploy` jobs. Build job installs Python, uv, runs sphinx-build, uploads artifact. Deploy job (needs: build) deploys to GitHub Pages. Deploy runs only on push to main.
**When to use:** Standard pattern for GitHub Pages deployment.
**Example:**
```yaml
# Source: https://github.com/actions/starter-workflows/blob/main/pages/static.yml
# + https://github.com/rst2pdf/rst2pdf.github.io/blob/main/.github/workflows/static.yml
name: Docs

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: "pages"
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - uses: astral-sh/setup-uv@v7
        with:
          enable-cache: true
      - name: Build docs
        run: uv run --project docs sphinx-build -b html -W docs docs/_build/html
      - uses: actions/configure-pages@v5
      - uses: actions/upload-pages-artifact@v3
        with:
          path: docs/_build/html

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: actions/deploy-pages@v4
```

### Pattern 2: PR Docs Build Check Job
**What:** Add a new job to `PullRequestCI.yml` that runs `sphinx-build -W` without deploying. Catches broken docs before merge.
**When to use:** Per D-07, every PR should validate docs build.
**Example:**
```yaml
  docs-check:
    name: Docs build check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - uses: astral-sh/setup-uv@v7
        with:
          enable-cache: true
      - name: Build docs (warnings as errors)
        run: uv run --project docs sphinx-build -b html -W docs docs/_build/html
```

### Pattern 3: README Badge + Link
**What:** Shields.io static badge at top of README, plus inline documentation link.
**Example:**
```markdown
[![Docs](https://img.shields.io/badge/docs-GitHub%20Pages-blue)](https://anentropic.github.io/duckdb-semantic-views/)
```

### Anti-Patterns to Avoid
- **Third-party deployment actions:** Rejected by D-04. Use only official `actions/deploy-pages`.
- **Deploying from milestone branches:** D-05 restricts deploy to `main` only. PR/milestone branches only build-check.
- **Skipping `-W` flag in CI:** Without warnings-as-errors, broken cross-references and missing includes slip through undetected.
- **Using `pip` instead of `uv`:** Project convention is uv (D-08, Justfile pattern).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| `.nojekyll` file | Manual `touch .nojekyll` in workflow | `sphinx.ext.githubpages` extension | Built-in Sphinx extension, automatically creates `.nojekyll` in output dir. One line in `conf.py`. |
| GitHub Pages deployment | Custom rsync/push to gh-pages branch | `actions/deploy-pages@v4` | Official action, handles environment, OIDC token, status reporting. Per D-04. |
| Python dependency install | `pip install -r requirements.txt` | `uv run --project docs` | Project convention. Faster, deterministic resolution. |

## Common Pitfalls

### Pitfall 1: Intersphinx DuckDB Mapping Fails
**What goes wrong:** `sphinx-build -W` fails with: `WARNING: failed to reach any of the inventories with the following issues: intersphinx inventory 'https://duckdb.org/docs/objects.inv' not fetchable due to 404 Client Error`
**Why it happens:** DuckDB documentation does not publish a Sphinx `objects.inv` file. The mapping in `conf.py` (`"duckdb": ("https://duckdb.org/docs", None)`) will always fail.
**How to avoid:** Remove the `intersphinx_mapping` for duckdb from `conf.py`, or if intersphinx links are used nowhere (likely), remove `sphinx.ext.intersphinx` from extensions entirely. Alternatively, keep the extension but remove the duckdb mapping.
**Warning signs:** Local build with `-W` flag exits with error code. CI build fails immediately.
**Verified:** Tested locally -- `uv run --project docs sphinx-build -b html -W docs /tmp/test` produces exactly this warning.

### Pitfall 2: GitHub Pages Not Enabled in Repository Settings
**What goes wrong:** Deploy step fails with `Error: Unable to create deployment` or similar.
**Why it happens:** GitHub Pages must be configured to use "GitHub Actions" as the source in the repository Settings > Pages tab BEFORE the first workflow run.
**How to avoid:** Document this as a manual prerequisite. The `github-pages` environment must exist.
**Warning signs:** First deployment fails, subsequent pushes also fail.

### Pitfall 3: Missing Permissions Block
**What goes wrong:** Deployment fails with authentication errors.
**Why it happens:** `actions/deploy-pages` requires `pages: write` and `id-token: write` permissions at the workflow level. Without these, the OIDC token verification fails.
**How to avoid:** Include `permissions:` block at workflow level (not job level) as shown in the official starter workflow.
**Warning signs:** `Error: Deployment request failed with status 403`.

### Pitfall 4: Concurrency Group Cancels Production Deployments
**What goes wrong:** A rapid push while a deployment is running cancels the in-progress deployment.
**Why it happens:** Using `cancel-in-progress: true` (the pattern from Build.yml/PullRequestCI.yml) on the deploy workflow.
**How to avoid:** Use `cancel-in-progress: false` for the Pages deployment workflow. Queued runs are skipped, but in-progress deployments complete. This is the official starter workflow pattern.
**Warning signs:** Partially deployed sites, missing files on live site.

### Pitfall 5: Wrong Build Output Path
**What goes wrong:** Deployed site shows directory listing or 404.
**Why it happens:** `upload-pages-artifact` `path:` does not match Sphinx build output directory.
**How to avoid:** Sphinx output goes to `docs/_build/html` (per Justfile docs-build recipe). The `path:` in `upload-pages-artifact` must be `docs/_build/html`.
**Warning signs:** Site deploys but shows no content.

## Code Examples

### conf.py: Add githubpages Extension
```python
# Source: https://www.sphinx-doc.org/en/master/usage/extensions/githubpages.html
# Adds .nojekyll to output directory automatically
extensions = [
    "sphinx.ext.intersphinx",  # NOTE: remove duckdb mapping or remove entirely
    "sphinx_design",
    "sphinx.ext.githubpages",  # ADD THIS
]
```

### conf.py: Fix Intersphinx
```python
# Option A: Remove the broken mapping (keep extension for future use)
intersphinx_mapping = {}

# Option B: Remove the extension entirely (if no :external: refs exist)
extensions = [
    "sphinx_design",
    "sphinx.ext.githubpages",
]
```

### Docs.yml: Complete Workflow
```yaml
# Source: official GitHub starter workflow + rst2pdf example
# File: .github/workflows/Docs.yml
name: Docs

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: "pages"
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'

      - uses: astral-sh/setup-uv@v7
        with:
          enable-cache: true

      - name: Build documentation
        run: uv run --project docs sphinx-build -b html -W docs docs/_build/html

      - name: Setup Pages
        uses: actions/configure-pages@v5

      - name: Upload artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: docs/_build/html

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

### PullRequestCI.yml: New docs-check Job
```yaml
  docs-check:
    name: Docs build check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'

      - uses: astral-sh/setup-uv@v7
        with:
          enable-cache: true

      - name: Build docs (warnings as errors)
        run: uv run --project docs sphinx-build -b html -W docs docs/_build/html
```

### README Badge
```markdown
[![Docs](https://img.shields.io/badge/docs-GitHub%20Pages-blue)](https://anentropic.github.io/duckdb-semantic-views/)
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| peaceiris/actions-gh-pages (push to gh-pages branch) | actions/deploy-pages (artifact-based deployment) | 2022-2023 | Official GitHub approach, no branch pollution |
| pip install sphinx | uv run --project (uses pyproject.toml) | 2024 | Faster, deterministic, project already uses uv |
| Manual .nojekyll | sphinx.ext.githubpages extension | Sphinx 1.3+ | Automatic, no workflow step needed |
| setup-uv@v3 | setup-uv@v7 | 2025-2026 | Latest stable, auto-caching on GitHub runners |

## Open Questions

1. **Does the repo have intersphinx references to DuckDB docs?**
   - What we know: `intersphinx_mapping` has a `duckdb` entry that 404s. Extension is in `conf.py`.
   - What's unclear: Whether any RST files use `:external:` or `:ref:` cross-references to duckdb docs.
   - Recommendation: Grep RST files for `:external:duckdb:` or similar patterns. If none found, remove both the mapping and (optionally) the extension. If some found, remove only the duckdb mapping.

2. **GitHub Pages repo settings**
   - What we know: Requires manual configuration in repo Settings > Pages > Source = "GitHub Actions"
   - What's unclear: Whether this is already configured (likely not, since this is a new setup)
   - Recommendation: Document as a manual step. Cannot be automated via workflow.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| uv | Docs build | Yes | 0.9.18 | -- |
| Python 3 | Sphinx | Yes | 3.11.1 (local); 3.12 in CI | -- |
| Sphinx | Docs build | Yes (via uv) | 9.1.0 | -- |

**Missing dependencies with no fallback:** None.

**Missing dependencies with fallback:** None.

Note: CI runs on `ubuntu-latest` with `actions/setup-python` and `astral-sh/setup-uv`, so local tool versions only matter for local testing. The CI environment is self-contained.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | just test-all (Rust unit + proptest + sqllogictest + DuckLake CI) |
| Config file | Justfile, Cargo.toml |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DOCS-01 | Sphinx + Shibuya configured, builds successfully | smoke | `uv run --project docs sphinx-build -b html -W docs docs/_build/html` | N/A (build command, not test file) |
| DOCS-02 | GitHub Actions workflow deploys on push to main | manual-only | Verify by pushing to main and checking Pages | N/A (requires GitHub infra) |
| DOCS-03 | Site structure includes all required sections | smoke | `uv run --project docs sphinx-build -b html -W docs docs/_build/html` (build succeeds = all pages present) | N/A |
| DOCS-04 | README links to docs site | manual-only | Visual inspection of README.md | N/A |

### Sampling Rate
- **Per task commit:** `just test-all` (verify no regressions from conf.py / README changes)
- **Per wave merge:** `just test-all` + local Sphinx build with `-W`
- **Phase gate:** Full suite green + local Sphinx build clean + manual verification of workflow YAML

### Wave 0 Gaps
None -- this phase creates CI/CD configuration files and edits docs metadata. No new test files needed. Existing `just test-all` ensures no regressions. The Sphinx build itself serves as the validation.

## Sources

### Primary (HIGH confidence)
- Local project files: `docs/conf.py`, `docs/pyproject.toml`, `Justfile`, `.github/workflows/*.yml` -- direct inspection
- Local build test: `uv run --project docs sphinx-build -b html -W docs /tmp/test` -- verified intersphinx warning
- [GitHub official starter workflow](https://raw.githubusercontent.com/actions/starter-workflows/main/pages/static.yml) -- workflow pattern
- [actions/deploy-pages README](https://github.com/actions/deploy-pages) -- v4, permissions, environment config
- [actions/upload-pages-artifact README](https://github.com/actions/upload-pages-artifact) -- v3, path input
- [actions/configure-pages README](https://github.com/actions/configure-pages) -- v5, purpose
- [astral-sh/setup-uv README](https://github.com/astral-sh/setup-uv) -- v7, enable-cache
- [uv GitHub Actions guide](https://docs.astral.sh/uv/guides/integration/github/) -- official uv CI patterns

### Secondary (MEDIUM confidence)
- [rst2pdf Sphinx GitHub Pages workflow](https://github.com/rst2pdf/rst2pdf.github.io/blob/main/.github/workflows/static.yml) -- verified real-world example using uv + Sphinx + deploy-pages
- [Sphinx githubpages extension](https://www.sphinx-doc.org/en/master/usage/extensions/githubpages.html) -- creates `.nojekyll` automatically
- [Lorna Jane: Publish to GitHub Pages with Sphinx](https://lornajane.net/posts/2025/publish-to-github-pages-with-sphinx) -- workflow structure reference

### Tertiary (LOW confidence)
- None -- all findings verified with primary or secondary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all tools already installed and verified locally; GitHub Actions versions confirmed from official READMEs
- Architecture: HIGH -- workflow pattern matches official GitHub starter workflow and verified real-world examples
- Pitfalls: HIGH -- intersphinx issue verified by local build test; permissions/environment requirements from official docs

**Research date:** 2026-03-27
**Valid until:** 2026-04-27 (stable domain; action versions may minor-bump but patterns are stable)
