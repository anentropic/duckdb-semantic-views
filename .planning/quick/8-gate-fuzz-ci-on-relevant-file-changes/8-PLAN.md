---
phase: quick-8
plan: 01
type: execute
wave: 1
depends_on: []
files_modified: [.github/workflows/Fuzz.yml]
autonomous: true
requirements: [QUICK-8]

must_haves:
  truths:
    - "Fuzz CI runs on pushes to main that modify src/**, fuzz/**, Cargo.toml, Cargo.lock, or .github/workflows/Fuzz.yml"
    - "Fuzz CI does NOT run on pushes that only modify docs, planning, tests, shim, or tooling files"
    - "Manual workflow_dispatch always triggers fuzz regardless of changed files"
  artifacts:
    - path: ".github/workflows/Fuzz.yml"
      provides: "Path-filtered fuzz workflow"
      contains: "paths:"
  key_links: []
---

<objective>
Add a `paths` filter to the Fuzz CI workflow so it only runs when files that could affect fuzz outcomes are modified.

Purpose: The fuzz job runs 3 targets at 10 minutes each (30 min total). It is expensive and should not trigger on documentation, planning, or tooling changes that cannot affect fuzz results.

Output: Updated `.github/workflows/Fuzz.yml` with `paths:` filter on the `push` trigger.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.github/workflows/Fuzz.yml
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add paths filter to Fuzz.yml push trigger</name>
  <files>.github/workflows/Fuzz.yml</files>
  <action>
Edit `.github/workflows/Fuzz.yml` to add a `paths:` filter under the `push` trigger. The `workflow_dispatch` trigger must remain unconditional.

Change the `on:` block from:

```yaml
on:
  push:
    branches: [main]
  workflow_dispatch:
```

to:

```yaml
on:
  push:
    branches: [main]
    paths:
      - 'src/**'
      - 'fuzz/**'
      - 'Cargo.toml'
      - 'Cargo.lock'
      - '.github/workflows/Fuzz.yml'
  workflow_dispatch:
```

Do NOT add paths-ignore (use allowlist, not denylist). Do NOT modify any other part of the workflow file -- the jobs, permissions, env, strategy, and steps must remain exactly as they are.

Rationale for each included path:
- `src/**` -- all Rust library source (model, expand modules used by fuzz targets)
- `fuzz/**` -- fuzz targets, seeds, corpus, fuzz Cargo.toml
- `Cargo.toml` -- root dependency changes that affect compilation
- `Cargo.lock` -- pinned dependency version changes

Paths deliberately excluded (cannot affect fuzz outcomes):
- `.github/workflows/` (other than Fuzz.yml itself) -- CI config for other workflows doesn't affect fuzz
- `shim/`, `build.rs` -- C++ shim only used for `extension` feature, fuzz targets use default features
- `tests/`, `sql/` -- integration/SQL tests, separate from fuzz
- `.planning/`, `docs/`, `README.md`, `CLAUDE.md` -- documentation
- `justfile`, `.pre-commit-config.yaml`, `dbt/`, `scripts/` -- tooling
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && grep -A 6 'push:' .github/workflows/Fuzz.yml | grep -q "paths:" && grep -q "src/\*\*" .github/workflows/Fuzz.yml && grep -q "fuzz/\*\*" .github/workflows/Fuzz.yml && grep -q "Cargo.toml" .github/workflows/Fuzz.yml && grep -q "Cargo.lock" .github/workflows/Fuzz.yml && grep -q "workflow_dispatch" .github/workflows/Fuzz.yml && echo "PASS: All path filters present and workflow_dispatch preserved"</automated>
  </verify>
  <done>Fuzz.yml has paths filter with 5 entries (src/**, fuzz/**, Cargo.toml, Cargo.lock, .github/workflows/Fuzz.yml) under push trigger, workflow_dispatch remains unconditional, no other changes to the workflow.</done>
</task>

</tasks>

<verification>
1. `paths:` block exists under `push:` trigger with exactly 4 entries
2. `workflow_dispatch:` has no `paths:` restriction
3. No other changes to the workflow (jobs, steps, permissions, env unchanged)
4. YAML is valid (no syntax errors)
</verification>

<success_criteria>
- `.github/workflows/Fuzz.yml` contains `paths:` filter gating push triggers to `src/**`, `fuzz/**`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/Fuzz.yml`
- `workflow_dispatch` remains unconditional for manual runs
- Workflow file is valid YAML with no other modifications
</success_criteria>

<output>
After completion, create `.planning/quick/8-gate-fuzz-ci-on-relevant-file-changes/8-SUMMARY.md`
</output>
