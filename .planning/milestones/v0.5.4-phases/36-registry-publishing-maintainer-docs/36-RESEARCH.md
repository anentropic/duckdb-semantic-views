# Phase 36: Registry Publishing & Maintainer Docs - Research

**Researched:** 2026-03-27
**Domain:** DuckDB Community Extension Registry publishing, description.yml specification, MAINTAINER.md documentation
**Confidence:** HIGH

## Summary

Phase 36 is the final phase of v0.5.4. It creates the `description.yml` file required for DuckDB Community Extension Registry submission, updates MAINTAINER.md with multi-branch strategy and CE processes, creates an end-of-milestone Python example, and bumps Cargo.toml to 0.5.4. The technical surface is well-understood: the `rusty_quack` extension (maintained by DuckDB team members `samansmink` and `mlafeldt`) serves as the canonical reference for a Rust+cargo CE submission.

The key risk is the CE build pipeline: while the `rusty_quack` Rust extension proves `build: cargo` works, this project's hybrid Rust+C++ build (the `cc` crate compiles `shim.cpp` against the vendored DuckDB amalgamation) is more complex than `rusty_quack`. The CONTEXT.md decision D-07 wisely recommends submitting as a **draft PR** early to surface build issues.

**Primary recommendation:** Model `description.yml` exactly after `rusty_quack`'s proven format, with adjustments for this project's specifics (name, description, repo, ref, excluded_platforms). For MAINTAINER.md, make targeted surgical edits to existing sections per D-09. The hello_world must use native DDL syntax (CREATE SEMANTIC VIEW) and be fully self-contained.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** GitHub org is `anentropic` -- `repo.github: anentropic/duckdb-semantic-views`. Fix the `paul-rl` references in MAINTAINER.md.
- **D-02:** `hello_world` uses single-table native DDL + `semantic_view()` query. Simple CREATE SEMANTIC VIEW with one table, 1-2 dimensions, 1-2 metrics, then a query. Must work end-to-end.
- **D-03:** `extension.version` matches Cargo.toml version (currently `0.5.0`). Language: `Rust`, build: `cargo`.
- **D-04:** `excluded_platforms` should match existing CI `exclude_archs`: `wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl`
- **D-05:** `requires_toolchains`: `rust;python3` (Python needed for sqllogictest runner during CI)
- **D-06:** `repo.ref` points to the release commit SHA on main (after squash-merge). Dual-version support (andium/LTS) is a future concern -- initial submission targets main branch only.
- **D-07:** Submit as a draft PR to `duckdb/community-extensions` early. The hybrid Rust+C++ build pipeline is untested -- a draft PR surfaces build issues before final submission.
- **D-08:** The description.yml file lives in THIS repo (not in the community-extensions fork) so it can be tracked and versioned. The CE submission PR copies it to the fork.
- **D-09:** Targeted updates only -- fix username, update hello_world/CE section, add multi-branch section, add CE update process. Keep Prerequisites, Quick Start, Architecture, Testing, Fuzzing, CI sections as-is.
- **D-10:** Add "Multi-Version Branching Strategy" section documenting: main (latest DuckDB), duckdb/1.4.x (LTS), how to sync changes between branches.
- **D-11:** Update "Publishing to Community Extension Registry" section with correct native DDL hello_world, correct GitHub username, and step-by-step CE update process for new releases.
- **D-12:** Update "Worked Examples" section to use native DDL syntax (replace old function-based DDL examples that were retired in v0.5.2).
- **D-13:** Add "How to Bump DuckDB Version" section covering both branches (main and duckdb/1.4.x).
- **D-14:** Create `examples/snowflake_parity.py` demonstrating v0.5.4 features: UNIQUE constraints, cardinality inference, ALTER RENAME, SHOW SEMANTIC commands with LIKE/STARTS WITH/LIMIT.
- **D-15:** Bump Cargo.toml version to `0.5.4` as part of milestone close.
- **D-16:** Squash-merge milestone branch to main and tag `v0.5.4`. (This happens after phase execution, during `/gsd:complete-milestone`.)

### Claude's Discretion
- Exact description.yml `extended_description` wording
- MAINTAINER.md section ordering and formatting
- Python example file structure and data setup
- Whether to include a `Makefile` / `justfile` recipe for CE submission workflow

### Deferred Ideas (OUT OF SCOPE)
- "Investigate WASM build strategy" -- tooling concern, not CE registry scope
- "Pre-aggregation materializations" -- feature work, not registry scope
- "dbt semantic layer integration" -- feature research, not registry scope

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CREG-01 | `description.yml` created with all required fields | Verified exact field format from `rusty_quack` reference and CE documentation |
| CREG-02 | `description.yml` includes `repo.ref` (latest) and `repo.andium` (LTS) commit hashes | D-06 overrides: initial submission targets main only (no andium). The `andium` field can be added later. |
| CREG-03 | `docs.hello_world` example works end-to-end | Must use native DDL (CREATE SEMANTIC VIEW), be self-contained (CREATE TABLE + INSERT + CREATE SEMANTIC VIEW + FROM semantic_view) |
| CREG-04 | PR submitted to `duckdb/community-extensions` and build pipeline passes | Human action: fork repo, copy description.yml, submit draft PR. Build pipeline uses extension-ci-tools. |
| CREG-05 | Extension installable via `INSTALL semantic_views FROM community` | Human verification after CE pipeline passes and extension is published |
| MAINT-01 | MAINTAINER.md documents multi-version branching strategy | Add section covering main (v1.5.x latest), duckdb/1.4.x (LTS), syncing |
| MAINT-02 | MAINTAINER.md documents CE registry update process | Update existing CE section with correct username, native DDL hello_world, release process |
| MAINT-03 | MAINTAINER.md documents how to bump DuckDB version on both branches | Extend existing "Updating the DuckDB Version Pin" section to cover both branches |

</phase_requirements>

## Standard Stack

This phase creates documentation and YAML configuration files. No new library dependencies are needed.

### Core
| Tool | Version | Purpose | Why Standard |
|------|---------|---------|--------------|
| DuckDB CE build pipeline | v1.5.0 | Builds extension across platforms | Official CI tooling from `duckdb/extension-ci-tools` |
| description.yml | CE spec | Extension descriptor for registry | Required format for CE submission |
| Python 3 | >=3.10 | Example scripts, test runner | Required by CE pipeline (requires_toolchains) |

### Supporting
| Tool | Version | Purpose | When to Use |
|------|---------|---------|-------------|
| `gh` CLI | any | Fork, clone, PR operations against community-extensions | CE submission workflow |
| `uv` | any | Run Python examples with PEP 723 dependencies | Example verification |

## Architecture Patterns

### description.yml Canonical Format

The following format is verified from the `rusty_quack` extension (the only Rust CE extension in the registry, maintained by DuckDB team members):

```yaml
extension:
  name: semantic_views
  description: "Semantic views -- a declarative layer for dimensions, metrics, and relationships"
  version: 0.5.4
  language: Rust
  build: cargo
  license: MIT
  excluded_platforms: "wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl"
  requires_toolchains: "rust;python3"
  maintainers:
    - anentropic

repo:
  github: anentropic/duckdb-semantic-views
  ref: <commit-sha-after-squash-merge-to-main>

docs:
  hello_world: |
    -- Create sample data
    CREATE TABLE demo(region VARCHAR, amount DECIMAL(10,2));
    INSERT INTO demo VALUES ('US', 100), ('US', 200), ('EU', 150);

    -- Define a semantic view
    CREATE SEMANTIC VIEW sales AS
      TABLES (d AS demo PRIMARY KEY (region))
      DIMENSIONS (d.region AS d.region)
      METRICS (d.revenue AS SUM(d.amount));

    -- Query it
    FROM semantic_view('sales', dimensions := ['region'], metrics := ['revenue']);
  extended_description: |
    Semantic views let you define dimensions, metrics, joins, and filters once,
    then query any combination. The extension handles GROUP BY, JOIN, and filter
    composition automatically. Supports multi-table joins with PK/FK relationships,
    fan trap detection, role-playing dimensions, derived metrics, and FACTS.

    Documentation: https://anentropic.github.io/duckdb-semantic-views/
```

**Source:** [rusty_quack description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) (HIGH confidence)

### Key Format Details

| Field | Format | Notes |
|-------|--------|-------|
| `name` | lowercase, `_` or `-` allowed | Must match extension LOAD name |
| `excluded_platforms` | semicolon-separated string | NOT a YAML list -- verified from rusty_quack |
| `requires_toolchains` | semicolon-separated string | Same format as excluded_platforms |
| `version` | freeform string | Matches Cargo.toml version |
| `ref` | full 40-char commit SHA | Points to main branch commit |
| `andium` | full 40-char commit SHA (optional) | Points to LTS branch commit for DuckDB 1.4.x builds |

### Multi-Version CE Support

The CE registry uses the `andium` field to build the extension against DuckDB 1.4.x (Andium LTS):
- `ref`: commit SHA compatible with latest DuckDB (currently 1.5.0)
- `andium`: commit SHA compatible with DuckDB 1.4.x LTS
- `ref_next`: temporary field for upcoming DuckDB releases (not needed initially)

Per D-06, the initial submission targets main branch only. The `andium` field can be added in a follow-up PR after the initial submission is accepted.

### MAINTAINER.md Section Updates

Per D-09, targeted updates only. Sections to modify:

1. **Quick Start** (line 40): Fix `paul-rl` to `anentropic` in git clone URL
2. **Publishing to Community Extension Registry** (lines 430-481): Rewrite with native DDL hello_world, correct username, CE update process
3. **Worked Examples** (lines 483+): Replace function-based DDL with native DDL syntax
4. **NEW: Multi-Version Branching Strategy**: Insert after "Updating the DuckDB Version Pin" section
5. **Rename/extend "Updating the DuckDB Version Pin"**: Cover both branches (main + duckdb/1.4.x)

### Python Example Pattern

Based on `basic_ddl_and_query.py` and `advanced_features.py`:

```python
#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.0"]
# requires-python = ">=3.10"
# ///
"""
uv run examples/snowflake_parity.py

Demonstrates v0.5.4 features: ...
"""
import duckdb

con = duckdb.connect(config={"allow_unsigned_extensions": "true"})
con.execute("LOAD 'build/debug/semantic_views.duckdb_extension'")

# ... sections with print headers ...
```

### Anti-Patterns to Avoid
- **Function-based DDL in examples:** All DDL must use native `CREATE SEMANTIC VIEW` syntax (function-based DDL was retired in v0.5.2)
- **Using `paul-rl` as GitHub org:** Must be `anentropic` everywhere
- **Including `andium` in initial description.yml:** D-06 says main-only for initial submission
- **YAML list format for excluded_platforms:** Must use semicolon-separated string format

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CE build pipeline | Custom CI for community builds | `duckdb/extension-ci-tools` reusable workflow | CE pipeline is standardized, handles all platforms |
| Extension signing/metadata | Manual metadata append | `build_extension_with_metadata` Make target | CI tools handle extension signing and version metadata |
| hello_world validation | Manual testing | Draft PR to CE repo | CE pipeline builds and tests the hello_world automatically |

## Common Pitfalls

### Pitfall 1: License Mismatch
**What goes wrong:** The LICENSE file says "BSD 3-Clause" but Cargo.toml says `license = "MIT"`. The description.yml must match the actual license.
**Why it happens:** LICENSE and Cargo.toml were likely set up at different times and drifted.
**How to avoid:** Verify the actual LICENSE file content. The current LICENSE says "BSD 3-Clause License, Copyright (c) 2026, Paul Garner." Either update Cargo.toml to `license = "BSD-3-Clause"` or update the LICENSE file. The description.yml `license` field must match whichever is canonical.
**Warning signs:** CE review may flag the mismatch. SPDX identifier for BSD 3-Clause is `BSD-3-Clause`.

### Pitfall 2: Hybrid Rust+C++ Build in CE Pipeline
**What goes wrong:** The CE build pipeline may not handle the C++ shim compilation correctly. This extension uses `cc` crate to compile `shim.cpp` and the DuckDB amalgamation (`duckdb.cpp`, ~300K lines). The amalgamation must be downloaded during build.
**Why it happens:** The `build: cargo` path in the CE pipeline delegates to `cargo build`, which triggers `build.rs`. But `build.rs` expects `cpp/include/duckdb.hpp` and `cpp/include/duckdb.cpp` to exist. The Makefile's `ensure_amalgamation` target downloads these, but the CE pipeline may not run Make targets before cargo.
**How to avoid:** Check whether `build.rs` handles the missing-amalgamation case gracefully. The `ensure_amalgamation` Make target downloads from GitHub releases. If the CE pipeline uses `make release` (as `extension-ci-tools/makefiles/c_api_extensions/rust.Makefile` likely does), this should work. Submit draft PR early (D-07) to validate.
**Warning signs:** Build failure with "failed to read cpp/include/duckdb.cpp" in CI.

### Pitfall 3: hello_world Requires Data Setup
**What goes wrong:** The hello_world example fails if it assumes tables already exist.
**Why it happens:** The CE documentation page runs the hello_world in a fresh DuckDB instance -- no persistent state.
**How to avoid:** The hello_world must be fully self-contained: CREATE TABLE, INSERT data, CREATE SEMANTIC VIEW, then query. Keep it minimal (the CE page has limited display space).
**Warning signs:** hello_world that starts with CREATE SEMANTIC VIEW without creating underlying tables first.

### Pitfall 4: excluded_platforms Missing Arch Variants
**What goes wrong:** Build fails on platforms like `linux_arm64_musl` or `windows_arm64` that the extension cannot support.
**Why it happens:** The CI `exclude_archs` list differs from the description.yml list. The Build.yml excludes `linux_arm64_musl` and `windows_arm64` in addition to the platforms in D-04.
**How to avoid:** Cross-reference Build.yml `exclude_archs` with description.yml `excluded_platforms`. Build.yml currently excludes: `linux_amd64_musl;linux_arm64_musl;windows_arm64;windows_amd64_mingw;wasm_mvp;wasm_eh;wasm_threads`. D-04 lists: `wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl`. The Build.yml also excludes `linux_arm64_musl` and `windows_arm64` but does NOT exclude `windows_amd64_rtools`. Reconcile these lists.
**Warning signs:** CE pipeline attempting to build on unsupported platforms.

### Pitfall 5: ref SHA Must Be From main Branch
**What goes wrong:** Using a milestone branch commit SHA as `ref` -- the CE pipeline clones the repo and checks out that SHA, but the branch structure may differ.
**Why it happens:** The milestone branch will be squash-merged to main before submission, producing a new single commit. The SHA must be that merge commit, not a milestone branch commit.
**How to avoid:** Use a placeholder in description.yml during development, replace with actual main commit SHA after squash-merge. D-16 confirms squash-merge happens during `/gsd:complete-milestone`.
**Warning signs:** Using `git log --oneline -1` on the milestone branch instead of main.

### Pitfall 6: Outdated Worked Examples in MAINTAINER.md
**What goes wrong:** MAINTAINER.md still contains function-based DDL examples (`define_semantic_view()`, `semantic_query()`) that were retired in v0.5.2.
**Why it happens:** The MAINTAINER.md "Complete Worked Example" section (lines 238-288) uses the old JSON-based `define_semantic_view()` function, not native DDL.
**How to avoid:** Update all worked examples to use `CREATE SEMANTIC VIEW ... AS TABLES (...) DIMENSIONS (...) METRICS (...)` and `semantic_view()` table function (not `semantic_query()`).
**Warning signs:** Any reference to `define_semantic_view`, `drop_semantic_view`, `list_semantic_views()`, `semantic_query()` in MAINTAINER.md examples.

## Code Examples

### Self-Contained hello_world for description.yml

```sql
-- Source: Verified against existing test/sql patterns and basic_ddl_and_query.py

-- Create sample data
CREATE TABLE demo(region VARCHAR, amount DECIMAL(10,2));
INSERT INTO demo VALUES ('US', 100), ('US', 200), ('EU', 150);

-- Define a semantic view
CREATE SEMANTIC VIEW sales AS
  TABLES (d AS demo PRIMARY KEY (region))
  DIMENSIONS (d.region AS d.region)
  METRICS (d.revenue AS SUM(d.amount));

-- Query it
FROM semantic_view('sales', dimensions := ['region'], metrics := ['revenue']);
```

### MAINTAINER.md Worked Example (Native DDL)

```python
# Source: Updated from existing Loading the Extension section (lines 226-290)
import duckdb

con = duckdb.connect(config={"allow_unsigned_extensions": "true"})
con.execute("LOAD 'build/debug/semantic_views.duckdb_extension'")

# Create sample data
con.execute("""
CREATE TABLE orders (
    id INTEGER, region VARCHAR, status VARCHAR, amount DECIMAL(10,2)
);
INSERT INTO orders VALUES
    (1, 'US', 'completed', 100.00),
    (2, 'US', 'completed', 200.00),
    (3, 'EU', 'completed', 150.00),
    (4, 'EU', 'pending',    50.00);
""")

# Define a semantic view (native DDL)
con.execute("""
CREATE SEMANTIC VIEW shop AS
  TABLES (o AS orders PRIMARY KEY (id))
  DIMENSIONS (
    o.region AS o.region,
    o.status AS o.status
  )
  METRICS (
    o.revenue     AS SUM(o.amount),
    o.order_count AS COUNT(*)
  );
""")

# Query with any dimension/metric combination
result = con.execute("""
    SELECT * FROM semantic_view('shop',
        dimensions := ['region'],
        metrics := ['revenue']
    ) ORDER BY region
""").fetchall()
print(result)
# [('EU', Decimal('200.00')), ('US', Decimal('300.00'))]

# See the generated SQL
con.execute("""
    SELECT * FROM explain_semantic_view('shop',
        dimensions := ['region'],
        metrics := ['revenue']
    )
""").fetchall()

# List all views
con.execute("SHOW SEMANTIC VIEWS").fetchall()

# Describe a view
con.execute("DESCRIBE SEMANTIC VIEW shop").fetchall()

# Remove a view
con.execute("DROP SEMANTIC VIEW shop")
```

### Multi-Version Branching Section Content

```markdown
## Multi-Version Branching Strategy

The extension supports two DuckDB version tracks via separate branches:

| Branch | DuckDB Version | Purpose | Version Format |
|--------|---------------|---------|----------------|
| `main` | Latest (currently 1.5.x) | Primary development, CE registry `ref` | `0.5.4` |
| `duckdb/1.4.x` | 1.4.x (Andium LTS) | LTS compatibility | `0.5.4+duckdb1.4` |

### Development Workflow

1. **New features**: Develop on milestone branches (e.g., `milestone/v0.5.4`), merge to `main`
2. **Cherry-pick to LTS**: After main is stable, cherry-pick relevant commits to `duckdb/1.4.x`
3. **Version bumps**: Each branch tracks its own DuckDB version in `.duckdb-version`

### Syncing Changes Between Branches

```bash
# Cherry-pick a commit from main to LTS
git checkout duckdb/1.4.x
git cherry-pick <commit-sha>
# Resolve any DuckDB API differences
just test-all
```

### CI Coverage

Both branches run the full Build.yml pipeline on push. The DuckDB Version Monitor
checks for new releases of both the latest and LTS version lines (weekly, Monday 09:00 UTC).
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `build: cmake` only | `build: cargo` for Rust extensions | ~2024-2025 | Rust extensions use cargo directly, no cmake wrapper needed |
| `ref_next` for upcoming versions | `ref_next` + `andium` for LTS | DuckDB 1.4.0 (Sep 2025) | `andium` field added for LTS track support |
| CE docs say "only cmake supported" | `rusty_quack` proves `cargo` works | Active | Documentation is outdated; `rusty_quack` is the authoritative example |

**Deprecated/outdated:**
- CE documentation claiming `build: cmake` is the only option -- `build: cargo` is proven via `rusty_quack`
- Function-based DDL examples in MAINTAINER.md (retired in v0.5.2)
- `paul-rl` GitHub org references in MAINTAINER.md -- should be `anentropic`

## Open Questions

1. **License mismatch: BSD-3-Clause vs MIT**
   - What we know: LICENSE file says "BSD 3-Clause", Cargo.toml says `license = "MIT"`. The existing MAINTAINER.md description.yml template also says MIT.
   - What's unclear: Which is the intended license?
   - Recommendation: The user must decide. If BSD-3-Clause is correct, update Cargo.toml to `license = "BSD-3-Clause"` and use that in description.yml. If MIT is correct, update the LICENSE file. Flag this to the user before writing description.yml.

2. **excluded_platforms reconciliation**
   - What we know: Build.yml excludes `linux_amd64_musl;linux_arm64_musl;windows_arm64;windows_amd64_mingw;wasm_mvp;wasm_eh;wasm_threads`. D-04 lists `wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl`. These differ: Build.yml includes `linux_arm64_musl` and `windows_arm64` but not `windows_amd64_rtools`.
   - What's unclear: Should description.yml match Build.yml exactly, or use the broader D-04 list?
   - Recommendation: Use the union of both lists to be safe. Include all platforms that either CI excludes or that D-04 specifies: `wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl;linux_arm64_musl;windows_arm64`.

3. **CE pipeline handling of DuckDB amalgamation download**
   - What we know: The build.rs expects `cpp/include/duckdb.cpp` and `cpp/include/duckdb.hpp`. The Makefile `ensure_amalgamation` target auto-downloads these. The CE pipeline likely invokes `make release` which triggers `ensure_amalgamation`.
   - What's unclear: Whether the CE pipeline allows network access during build to download the amalgamation from GitHub releases.
   - Recommendation: Draft PR (D-07) will test this. If it fails, the amalgamation files may need to be committed to the repo (they are currently gitignored, ~25MB).

4. **Correct `language` field value**
   - What we know: `rusty_quack` uses `language: Rust`. This extension has a C++ shim. CE docs mention "Rust & C++" as a valid value.
   - What's unclear: Whether `Rust` or `Rust & C++` is the correct value for a hybrid extension.
   - Recommendation: Use `language: Rust` per D-03 (locked decision). The C++ is compiled by the Rust `cc` crate build script, so from the CE pipeline's perspective this is a cargo-built Rust extension.

## Environment Availability

Step 2.6: SKIPPED (no external dependencies identified). This phase creates configuration files, documentation, and Python examples. The only runtime dependency is the built extension binary, which is verified by existing CI.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | just test-all (cargo test + sqllogictest + ducklake-ci + vtab-crash + caret) |
| Config file | Justfile |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CREG-01 | description.yml has correct fields | manual | Validate YAML structure | n/a (config file, not code) |
| CREG-02 | description.yml has ref (and optionally andium) SHAs | manual | Verify SHA exists on branch | n/a |
| CREG-03 | hello_world works end-to-end | integration | `just build && echo "..." \| duckdb` | Wave 0 manual test |
| CREG-04 | PR submitted to CE repo | human-action | Draft PR submission | n/a |
| CREG-05 | Extension installable from community | human-action | `INSTALL semantic_views FROM community; LOAD semantic_views;` | n/a |
| MAINT-01 | MAINTAINER.md multi-branch docs | manual review | Read MAINTAINER.md | n/a |
| MAINT-02 | MAINTAINER.md CE update process | manual review | Read MAINTAINER.md | n/a |
| MAINT-03 | MAINTAINER.md DuckDB version bump | manual review | Read MAINTAINER.md | n/a |

### Sampling Rate
- **Per task commit:** `just test-all` (ensures no regressions from doc/config changes)
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green + manual hello_world verification + MAINTAINER.md review

### Wave 0 Gaps
None -- this phase does not modify Rust source code. Existing test infrastructure covers all regression detection. The hello_world validation is a manual one-time check against a fresh DuckDB + built extension.

## Sources

### Primary (HIGH confidence)
- [rusty_quack description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) -- canonical Rust CE extension format, verified fields and values
- [shellfs description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/shellfs/description.yml) -- verified `andium` field format (full commit SHA)
- Project files: MAINTAINER.md (lines 430-481), Build.yml, DuckDBVersionMonitor.yml, Cargo.toml, build.rs, existing examples

### Secondary (MEDIUM confidence)
- [DuckDB CE documentation](https://duckdb.org/community_extensions/documentation) -- field specification (note: says only `cmake` for build, but `rusty_quack` contradicts)
- [DuckDB CE development guide](https://duckdb.org/community_extensions/development) -- submission process, ref/ref_next workflow
- [DuckDB CE UPDATING.md](https://github.com/duckdb/community-extensions/blob/main/UPDATING.md) -- version update process, ref_next workflow
- [DuckDB CE FAQ](https://duckdb.org/community_extensions/faq) -- Rust extension development guidance
- [rusty_quack CE page](https://duckdb.org/community_extensions/extensions/rusty_quack) -- auto-detected function listing format

### Tertiary (LOW confidence)
- [extension-template-rs](https://github.com/duckdb/extension-template-rs) -- template is "experimental", may not reflect current CE pipeline behavior
- [community-extensions issue #54](https://github.com/duckdb/community-extensions/issues/54) -- discussion of Rust extension guidance (evolving)

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass before verification
- **Test suite:** Rust unit tests + proptest + sqllogictest + DuckLake CI + vtab crash + caret position tests
- **Build:** `just build` for debug, `cargo test` for unit tests, `just test-sql` requires fresh build
- **Milestone branch:** All work on `milestone/v0.5.4` (currently `gsd/v0.5.4-*`), NOT main

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, well-understood tools
- Architecture (description.yml): HIGH -- verified from working `rusty_quack` example
- Architecture (MAINTAINER.md): HIGH -- targeted edits to existing file with clear scope from CONTEXT.md
- Pitfalls: MEDIUM -- CE build pipeline for hybrid Rust+C++ is untested (D-07 addresses this with draft PR)
- hello_world format: HIGH -- verified from CE page and existing test patterns

**Research date:** 2026-03-27
**Valid until:** 2026-04-27 (stable domain -- CE spec changes slowly)
