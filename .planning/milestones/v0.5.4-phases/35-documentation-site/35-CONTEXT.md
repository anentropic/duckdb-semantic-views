# Phase 35: Documentation Site - Context

**Gathered:** 2026-03-27
**Status:** Ready for planning

<domain>
## Phase Boundary

Deploy the existing Sphinx documentation site to GitHub Pages with automated CI/CD. Documentation content is already written (via doc-writer workflow) -- this phase handles build validation, deployment pipeline, and README integration. No new documentation content is in scope.

</domain>

<decisions>
## Implementation Decisions

### Docs framework (pre-decided)
- **D-01:** Sphinx + Shibuya theme (already configured in `docs/conf.py`). NOT mkdocs-material or Zensical as ROADMAP/REQUIREMENTS previously stated. Update REQUIREMENTS.md DOCS-01 to reflect actual framework.
- **D-02:** sphinx-design extension for grid cards. sphinx-autobuild for local dev. Custom sqlgrammar_lexer for SQL grammar syntax highlighting.
- **D-03:** Build command: `uv run --project docs sphinx-build -b html docs docs/_build/html` (via `just docs-build`)

### Deployment mechanism
- **D-04:** Use GitHub's built-in `actions/deploy-pages` + `actions/upload-pages-artifact` for deployment. No third-party actions (e.g., peaceiris/actions-gh-pages).
- **D-05:** Deploy trigger: push to `main` branch only. Milestone branches do NOT trigger deployment.
- **D-06:** Workflow file: `.github/workflows/Docs.yml` (consistent with existing `Build.yml`, `CodeQuality.yml` naming)

### CI build validation
- **D-07:** Add Sphinx build check to `PullRequestCI.yml` so broken docs are caught before merge. This runs `sphinx-build` with `-W` (warnings-as-errors) but does NOT deploy.
- **D-08:** Both `Docs.yml` (deploy) and `PullRequestCI.yml` (check) use `uv` for Python dependency management, matching the existing `docs/pyproject.toml` setup.

### README integration
- **D-09:** Add a documentation badge (shields.io) near the top of README alongside any existing badges.
- **D-10:** Add a "Documentation" link in the README body pointing to the GitHub Pages URL.
- **D-11:** GitHub Pages URL format: `https://anentropic.github.io/duckdb-semantic-views/` (derived from `conf.py` github_url)

### Requirements alignment
- **D-12:** Update REQUIREMENTS.md DOCS-01 from "Zensical project configured with `zensical.toml`" to "Sphinx + Shibuya docs configured with `docs/conf.py` and `docs/pyproject.toml`"
- **D-13:** Update ROADMAP.md Phase 35 framework from "mkdocs-material" to "Sphinx + Shibuya"

### Claude's Discretion
- Exact GitHub Actions workflow YAML structure and job naming
- Whether to add `.nojekyll` file to built output
- sphinx-build warning handling details
- Badge style/color choices

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing docs infrastructure
- `docs/conf.py` -- Sphinx configuration, theme options, extensions, custom lexer setup
- `docs/pyproject.toml` -- Python dependencies (sphinx, shibuya, sphinx-design, sphinx-autobuild)
- `docs/index.rst` -- Site root with Diataxis structure (tutorials, how-to, explanation, reference)
- `docs/_ext/sqlgrammar_lexer.py` -- Custom Pygments lexer for SQL grammar blocks

### Build tooling
- `Justfile` lines 152-159 -- `docs-build` and `docs-serve` recipes

### CI/deployment context
- `.github/workflows/Build.yml` -- Existing CI pattern for workflow structure reference
- `.github/workflows/PullRequestCI.yml` -- PR CI workflow to extend with docs build check

### Doc-writer artifacts
- `.doc-writer/config.yaml` -- Persona, tone, doc system configuration
- `.doc-writer/gap-report.md` -- Confirms 0 undocumented symbols (all features documented)
- `.doc-writer/editor-report.md` -- Quality review of documentation content

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `docs/` directory: Complete Sphinx project with 19 RST pages across 4 Diataxis categories
- `docs/pyproject.toml`: Python project with all needed dependencies declared
- `just docs-build`: Working build recipe that produces `docs/_build/html/`
- `docs/_ext/sqlgrammar_lexer.py`: Custom Pygments lexer already registered in conf.py
- `docs/_static/css/s-layer.css`: Custom CSS for the Shibuya theme

### Established Patterns
- GitHub Actions workflows use `Build.yml`, `CodeQuality.yml`, `PullRequestCI.yml` naming (PascalCase)
- Python tooling uses `uv` (not pip/poetry) -- see `Justfile` and `docs/pyproject.toml`
- CI runs on `ubuntu-latest` per existing workflows

### Integration Points
- `PullRequestCI.yml` -- needs a new job for docs build validation
- `README.md` -- needs docs badge and link added
- `.github/workflows/` -- new `Docs.yml` workflow for Pages deployment
- GitHub repo settings -- Pages must be configured to use GitHub Actions as source

</code_context>

<specifics>
## Specific Ideas

No specific requirements -- open to standard approaches. The documentation content is already written and reviewed. This phase is purely infrastructure (CI/CD pipeline + README link).

</specifics>

<deferred>
## Deferred Ideas

### Reviewed Todos (not folded)
- "Investigate WASM build strategy" -- tooling concern, not docs-related (keyword match on "build" was false positive)
- "Pre-aggregation materializations" -- feature work, not docs (keyword match on "query" was false positive)
- "dbt semantic layer integration" -- feature research, not docs

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 35-documentation-site*
*Context gathered: 2026-03-27*
