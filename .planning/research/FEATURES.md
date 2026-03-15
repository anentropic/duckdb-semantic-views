# Feature Landscape: v0.5.4 Snowflake-Parity & Registry Publishing

**Domain:** DuckDB Rust extension -- Snowflake-style cardinality inference, multi-version support, documentation, and community extension registry publishing
**Researched:** 2026-03-15
**Milestone:** v0.5.4 -- UNIQUE constraint + cardinality inference, multi-version DuckDB support, Zensical docs site, CE registry publishing
**Status:** Subsequent milestone research (v0.5.3 shipped 2026-03-15)
**Overall confidence:** HIGH (Snowflake DDL grammar verified from official docs; DuckDB CE registry format verified from live description.yml files; DuckDB release cycle docs verified; Zensical verified from GitHub)

---

## Scope

This document covers the feature surface for v0.5.4: aligning with Snowflake's constraint-based cardinality inference, supporting multiple DuckDB versions (1.4.x LTS and 1.5.x latest), publishing to the DuckDB community extension registry, and shipping a documentation site.

**What already exists (NOT in scope for research):**
- Full native CREATE SEMANTIC VIEW DDL with TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS
- PK/FK relationship model with explicit `ONE TO MANY` / `MANY TO ONE` / `ONE TO ONE` cardinality keywords
- Fan trap detection, role-playing dimensions, USING RELATIONSHIPS, derived metrics
- `PRIMARY KEY (col)` on TABLES clause, FK REFERENCES in RELATIONSHIPS
- Build.yml using `duckdb/extension-ci-tools` reusable workflow (v1.4.4)
- DuckDB Version Monitor CI workflow
- 441 tests, 13.5K LOC

**Focus:** Four feature areas and their interactions, complexity, and implementation sequencing.

---

## Table Stakes

Features that must ship before v0.5.4 can be considered a viable public release on the community extension registry.

### T1: UNIQUE Table Constraint + Snowflake-Style Cardinality Inference

| Aspect | Detail |
|--------|--------|
| **Feature** | Add `UNIQUE (col)` constraint to TABLES clause. Infer relationship cardinality from PK/UNIQUE declarations instead of requiring explicit `ONE TO MANY` / `MANY TO ONE` keywords. |
| **Why Expected** | Snowflake's semantic views infer cardinality from constraints. Explicit cardinality keywords are verbose and error-prone. Users already declare PK; UNIQUE is the natural companion for cardinality inference. This removes a significant syntax burden and aligns with Snowflake semantics. |
| **Complexity** | **Medium** |
| **Dependencies** | Body parser (add UNIQUE to TABLES clause grammar). Model (`TableRef` needs `unique_columns`). Graph module (infer cardinality from constraint metadata). Backward compat (existing explicit cardinality must still work during transition). |

**How Snowflake handles cardinality inference (verified from official docs):**

Snowflake's cardinality rules work as follows:

1. **TABLES clause** declares `PRIMARY KEY (col)` and/or `UNIQUE (col)` per logical table
2. **RELATIONSHIPS clause** uses `table_a(fk_col) REFERENCES table_b` -- the referenced columns must be a PRIMARY KEY or UNIQUE constraint on `table_b`
3. **Cardinality is inferred from the data characteristics of the FK column:**
   - If multiple rows in the FK table share the same FK value --> **many-to-one** relationship
   - If each row in the FK table has a unique FK value --> **one-to-one** relationship
4. **No explicit cardinality keywords exist** in Snowflake semantic views. There is no `ONE TO MANY` or `MANY TO ONE` syntax.
5. **Many-to-many is NOT supported.** Snowflake only recognizes many-to-one and one-to-one relationships.
6. **Self-references are prohibited.** "A table cannot reference itself."

**Snowflake TABLES syntax with UNIQUE (verified from official DDL grammar):**

```sql
TABLES (
  region AS schema.REGION PRIMARY KEY (r_regionkey),
  product AS schema.PRODUCTS PRIMARY KEY (product_id) UNIQUE (service_id),
  combo AS schema.COMBO_TABLE PRIMARY KEY (id) UNIQUE (area_id, product_id) UNIQUE (service_id)
)
```

Key grammar points:
- A table can have ONE `PRIMARY KEY` and MULTIPLE `UNIQUE` constraints
- Both can be composite (multiple columns)
- "If you already identified a column as a primary key column (by using PRIMARY KEY), do not add the UNIQUE clause for that column"

**Snowflake REFERENCES resolution (verified from official docs):**

```sql
RELATIONSHIPS (
  nation (n_regionkey) REFERENCES region,          -- references region's PRIMARY KEY
  orders (o_custkey) REFERENCES customer,          -- references customer's PRIMARY KEY
  detail (service_id) REFERENCES product(service_id) -- references product's UNIQUE(service_id)
)
```

When `REFERENCES table_alias` omits the column list, it resolves to the target table's PRIMARY KEY. When `REFERENCES table_alias(col)` specifies columns, they must match a declared UNIQUE or PRIMARY KEY constraint.

**Cardinality inference algorithm (our implementation):**

Since DuckDB semantic views are a preprocessor (we do not query the data at define time), we cannot infer cardinality from actual data like Snowflake does. Instead, we infer from **constraint declarations**:

| FK column constraint | Referenced constraint | Inferred cardinality |
|---------------------|----------------------|---------------------|
| No constraint (bare column) | PRIMARY KEY | **Many-to-one** (default FK pattern) |
| UNIQUE or PRIMARY KEY | PRIMARY KEY | **One-to-one** |
| No constraint | UNIQUE | **Many-to-one** |
| UNIQUE or PRIMARY KEY | UNIQUE | **One-to-one** |

This is the correct inference because:
- If the FK column has a UNIQUE/PK constraint, each FK value appears at most once --> one-to-one
- If the FK column has no uniqueness constraint, multiple rows can share the same FK value --> many-to-one
- One-to-many is the inverse of many-to-one (from the perspective of the referenced table looking back)

**What this replaces:**

Currently, the extension uses explicit cardinality keywords after REFERENCES:

```sql
-- Current (v0.5.3) syntax:
RELATIONSHIPS (
  order_to_customer AS o(customer_id) REFERENCES c ONE TO MANY
)

-- New (v0.5.4) syntax (Snowflake-aligned):
RELATIONSHIPS (
  order_to_customer AS o(customer_id) REFERENCES c
  -- Cardinality inferred: o.customer_id has no UNIQUE --> many-to-one from o to c
)
```

**Migration strategy:**
- Phase 1: Add UNIQUE to TABLES. Add inference logic. Keep explicit keywords working (backward compat).
- Phase 2: Deprecation warning when explicit keywords are used.
- Phase 3 (future): Remove explicit keywords entirely.

For v0.5.4, **both syntaxes should work.** Explicit keywords override inference when provided. This prevents a breaking change before registry publishing.

**Edge cases:**
- **No PK/UNIQUE on referenced table:** Error at define time. "table 'x' is referenced in a relationship but has no PRIMARY KEY or UNIQUE constraint"
- **FK column references UNIQUE, not PK:** Valid. UNIQUE columns are equally valid as reference targets.
- **Composite FK referencing composite UNIQUE:** Must match column count and names. Error if mismatch.
- **Explicit cardinality contradicts inference:** Honor the explicit keyword (user knows better than inference). Log a warning.
- **Stored JSON backward compatibility:** Old definitions without UNIQUE metadata must continue loading. `unique_columns` defaults to empty Vec (serde default).

**Confidence:** HIGH (Snowflake DDL grammar verified, inference rules derived from constraint semantics)

---

### T2: Community Extension Registry Publishing

| Aspect | Detail |
|--------|--------|
| **Feature** | Submit `description.yml` to `duckdb/community-extensions` repository. Pass CI build. Get listed at `duckdb.org/community_extensions/extensions/semantic_views`. |
| **Why Expected** | The extension claims to fill a gap in DuckDB's ecosystem. Being in the registry makes it installable via `INSTALL semantic_views FROM community;` and discoverable. Without registry presence, adoption is near zero. |
| **Complexity** | **Low-Medium** (mostly configuration, not code) |
| **Dependencies** | Multi-version DuckDB support (T3) for the `andium` field. Build pipeline must pass on all required platforms. Excluded platforms must be declared. |

**What a successful Rust extension submission looks like (verified from live `rusty_quack` descriptor):**

```yaml
extension:
  name: semantic_views
  description: Semantic views for DuckDB - declarative dimensions, metrics, and relationships
  version: 0.5.4
  language: Rust
  build: cargo
  license: MIT
  excluded_platforms: "wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl"
  requires_toolchains: "rust;python3"
  maintainers:
    - paulbouwer

repo:
  github: paulbouwer/duckdb-semantic-views
  andium: <commit_hash_for_1.4.x_LTS>
  ref: <commit_hash_for_latest>

docs:
  hello_world: |
    -- Create a semantic view
    CREATE SEMANTIC VIEW sales AS
      TABLES (o AS orders PRIMARY KEY (id))
      DIMENSIONS (o.region AS o.region)
      METRICS (o.revenue AS SUM(o.amount));
    -- Query it
    FROM semantic_view('sales', dimensions := ['region'], metrics := ['revenue']);
  extended_description: |
    Semantic views provide a declarative layer for DuckDB that lets you define
    dimensions, metrics, relationships, facts, and hierarchies once, then query
    with any combination without writing GROUP BY or JOIN logic by hand.
```

**Key fields explained:**
- `build: cargo` -- tells the CE build system to use `cargo build` instead of `cmake`
- `excluded_platforms` -- WASM not supported (parser hooks need C++ shim), musl not supported (linking issues), mingw not supported (CC compilation)
- `requires_toolchains: "rust;python3"` -- Rust for the extension, Python3 for test tooling
- `repo.ref` -- git commit hash for the latest DuckDB version build (currently 1.5.0)
- `repo.andium` -- git commit hash for the 1.4.x LTS build (named after the LTS codename)

**Release process (verified from UPDATING.md):**
1. Extension is built whenever the descriptor is updated (targets latest stable DuckDB)
2. Extension is rebuilt when a new DuckDB version releases (all CE extensions rebuilt)
3. The `andium` field provides the commit for the 1.4.x LTS line -- extensions are built for both
4. After LTS EOL (September 2026), the `andium` field is removed

**Review criteria (from documentation + community observations):**
- Extension must be public, open-source, hosted on GitHub
- `description.yml` must have all required fields
- Build must succeed on all non-excluded platforms
- CI auto-detects added functions, types, settings by comparing DuckDB catalog before/after load
- No formal code review -- the build pipeline is the gatekeeper
- Documentation page auto-generated from `docs.hello_world` and `docs.extended_description`

**What must be ready before submission:**
1. Build passes on: `linux_amd64`, `linux_arm64`, `osx_amd64`, `osx_arm64`, `windows_amd64`
2. Extension loads cleanly: `INSTALL 'path/to/semantic_views.duckdb_extension'; LOAD semantic_views;`
3. `hello_world` example works end-to-end
4. No secrets or credentials in the repository
5. MIT license file present

**Confidence:** HIGH (verified from live `rusty_quack` descriptor and UPDATING.md)

---

### T3: Multi-Version DuckDB Support (Andium LTS + Latest)

| Aspect | Detail |
|--------|--------|
| **Feature** | Support both DuckDB 1.4.x (Andium LTS, EOL Sep 2026) and DuckDB 1.5.x (Variegata, latest). Ship extension binaries for both. |
| **Why Expected** | DuckDB 1.4.x is the LTS release -- many production users stay on LTS. The community extension registry uses the `andium` field to build for LTS. Without LTS support, users on 1.4.x cannot install the extension. |
| **Complexity** | **Medium-High** |
| **Dependencies** | CI/CD changes (two build targets). Cargo.toml dependency management (duckdb crate version). Potential code changes if APIs differ between 1.4 and 1.5. Feature flags or conditional compilation. |

**DuckDB release cycle (verified from official docs):**

| Version | Codename | Type | Release | EOL |
|---------|----------|------|---------|-----|
| 1.4.0 | Andium | LTS | Sep 2025 | Sep 2026 |
| 1.4.3 | Andium | LTS patch | Dec 2025 | Sep 2026 |
| 1.5.0 | Variegata | Latest | Mar 2026 | Next release |
| 2.0 | (planned) | Next major | Sep 2026 | TBD |

**Extension versioning model (verified from DuckDB docs):**

The extension uses `C_STRUCT_UNSTABLE` ABI, which means:
- Extension binary is pinned to an exact DuckDB version
- Not binary-compatible across DuckDB minor versions
- Each DuckDB version needs its own build

The "Stable C API" (`C_STRUCT` ABI) would provide binary compatibility across versions, but our extension uses parser hooks via C++ shim which requires the unstable API.

**Branching strategy for multi-version support (verified from UPDATING.md):**

The community extension registry expects:
- `repo.ref` -- commit hash targeting latest stable (1.5.x)
- `repo.andium` -- commit hash targeting LTS (1.4.x)

Two approaches for maintaining both:

**Approach A: Separate branches (recommended by DuckDB docs)**
- `main` branch targets latest (DuckDB 1.5.x, `duckdb = "=1.5.0"` in Cargo.toml)
- `v1.4-andium` branch targets LTS (DuckDB 1.4.x, `duckdb = "=1.4.4"` in Cargo.toml)
- `description.yml` uses `ref: <main commit>` and `andium: <andium branch commit>`
- Bug fixes applied to both branches (cherry-pick or merge)

**Approach B: Cargo feature flags (more complex, not standard)**
- Single branch with conditional compilation: `#[cfg(feature = "duckdb14")]`
- Separate Cargo.toml profiles or workspace members
- Not recommended -- the DuckDB crate version pin (`= 1.4.4`) is a hard dependency

**Recommendation: Approach A (separate branches).**

The current project already has a DuckDB Version Monitor CI that detects new releases. The workflow needs updating to:
1. Check both latest AND LTS releases
2. Maintain the andium branch alongside main
3. The `description.yml` provides commit hashes for both

**DuckDB 1.5.0 changes relevant to this extension:**
- **PEG parser (experimental, opt-in):** DuckDB 1.5.0 ships an experimental PEG parser that allows extensions to extend the SQL grammar at runtime. This could eventually replace our C++ shim approach for parser hooks. However, it is opt-in (`enable_peg_parser()`) and not the default, so the current `parse_function` fallback approach must remain the primary mechanism.
- **No breaking C API changes identified** in the 1.4 --> 1.5 transition (based on available release notes).
- **The `duckdb-rs` crate** needs a version compatible with 1.5.0 (check crates.io for `duckdb = "=1.5.0"` availability).

**What needs to happen:**
1. Create `v1.4-andium` branch from current main (which targets 1.4.4)
2. Bump main to DuckDB 1.5.x (update `Cargo.toml`, `.duckdb-version`, `Build.yml`)
3. Verify build and tests pass on both versions
4. Update Build.yml to run both `duckdb-stable-build` (1.5.x) and `duckdb-next-build` (main)
5. Create `description.yml` with both `ref` and `andium` hashes

**Confidence:** HIGH (verified from UPDATING.md, DuckDB release cycle docs, and live descriptor examples)

---

### T4: Documentation Site (Zensical on GitHub Pages)

| Aspect | Detail |
|--------|--------|
| **Feature** | Ship a documentation site at `<user>.github.io/duckdb-semantic-views/` using Zensical (the successor to MkDocs Material). Covers: getting started, DDL reference, query reference, examples, architecture overview. |
| **Why Expected** | The current documentation is a README.md. For a community extension targeting the registry, users need proper documentation with search, navigation, and examples. A GitHub Pages site is free and standard for open-source projects. |
| **Complexity** | **Low-Medium** (content writing, not code) |
| **Dependencies** | None (independent of all other features). Content draws from existing README, MAINTAINER.md, examples/, and design doc. |

**Why Zensical (verified from GitHub and official site):**

Zensical is the successor to Material for MkDocs, built by the same team (squidfunk). It was created because MkDocs has been unmaintained since August 2024.

| Criterion | Zensical | MkDocs Material | Docusaurus |
|-----------|----------|-----------------|------------|
| Markdown-native | Yes | Yes | Yes (MDX) |
| Search | Built-in | Built-in | Built-in |
| GitHub Pages deploy | Native GH Actions | Native GH Actions | Requires custom setup |
| Maintenance | Active (2025-2026) | Unmaintained since Aug 2024 | Active |
| Python dependency | Yes (pip install) | Yes (pip install) | Node.js |
| Familiar to DuckDB community | Very (DuckDB docs use MkDocs conventions) | Yes | Less so |
| Config compatibility | MkDocs Material compatible | Native | Different config |

**Use Zensical** because it is the maintained successor to the tool the DuckDB ecosystem already uses, requires minimal setup, and deploys to GitHub Pages with a single workflow.

**Documentation structure for a DuckDB extension:**

The community extension page at `duckdb.org/community_extensions/extensions/semantic_views` is auto-generated from `description.yml`. The extension's own docs site should cover what the CE page cannot:

```
docs/
  index.md              -- Overview + quick start
  getting-started.md    -- Installation, first semantic view, first query
  reference/
    ddl.md              -- CREATE/DROP/DESCRIBE/SHOW syntax reference
    query.md            -- semantic_view() function reference
    clauses/
      tables.md         -- TABLES clause (PK, UNIQUE)
      relationships.md  -- RELATIONSHIPS clause (FK REFERENCES, cardinality)
      facts.md          -- FACTS clause
      hierarchies.md    -- HIERARCHIES clause
      dimensions.md     -- DIMENSIONS clause
      metrics.md        -- METRICS clause (USING, derived)
  examples/
    basic.md            -- Single table, dims + metrics
    multi-table.md      -- Star schema with joins
    role-playing.md     -- Airports/flights pattern
    fan-traps.md        -- What fan traps are, how detection works
    tpch.md             -- TPC-H worked example
  architecture.md       -- How the extension works (preprocessor model)
  contributing.md       -- Developer guide (from MAINTAINER.md)
```

**GitHub Pages deployment workflow:**

```yaml
name: Deploy Documentation
on:
  push:
    branches: [main]
    paths: ['docs/**', 'zensical.yml']
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - run: pip install zensical
      - run: zensical build
      - uses: peaceiris/actions-gh-pages@v4
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: ./site
```

**What well-documented DuckDB extensions look like (from community extension pages):**
- Auto-generated: function list, type list, settings list, download metrics
- Manually provided: description, hello_world example, extended_description
- Best extensions (e.g., h3, shellfs) have dedicated external documentation sites linked from the CE page

**Confidence:** HIGH (Zensical verified from GitHub, GH Pages deployment is standard)

---

## Differentiators

Features that improve the extension beyond minimum registry requirements.

### D1: MAINTAINER.md Updates for Branching Strategy and CE Publishing

| Aspect | Detail |
|--------|--------|
| **Feature** | Update MAINTAINER.md with: multi-version branching workflow, CE registry update process, how to cut an LTS patch, how to bump to a new DuckDB version for both branches. |
| **Value Proposition** | The extension is pre-release and the user is not deeply familiar with Rust/C++. Clear contributor documentation prevents the extension from becoming unmaintainable after initial publishing. |
| **Complexity** | **Low** |
| **Dependencies** | T2 (CE registry) and T3 (multi-version) must be implemented first so the docs reflect reality. |

**Confidence:** HIGH (documentation task, no technical risk)

---

### D2: TPC-H Worked Example

| Aspect | Detail |
|--------|--------|
| **Feature** | A TPC-H-based semantic view definition demonstrating all features (multi-table joins, facts, derived metrics, hierarchies, fan trap awareness). Ship as `examples/tpch.py` and document on the docs site. |
| **Value Proposition** | TPC-H is the universal analytics benchmark. A worked example against it demonstrates credibility and gives users a copy-paste starting point. Snowflake's own semantic view docs use TPC-H as the canonical example. |
| **Complexity** | **Low** |
| **Dependencies** | All DDL features from v0.5.3 already exist. Just needs writing. |

**Confidence:** HIGH (TPC-H is well-understood, all features already implemented)

---

### D3: PEG Parser Investigation (Future-Proofing)

| Aspect | Detail |
|--------|--------|
| **Feature** | Investigate DuckDB 1.5.0's experimental PEG parser for grammar extension support. Determine if it can replace the current C++ shim for `CREATE SEMANTIC VIEW` parsing. |
| **Value Proposition** | The C++ shim compiles the full DuckDB amalgamation (~20MB binary size). If the PEG parser allows native grammar extensions via the C API, the shim could be eliminated, dramatically reducing binary size and build complexity. |
| **Complexity** | **Research only** -- no implementation in v0.5.4 |
| **Dependencies** | DuckDB 1.5.0 support (T3). PEG parser is opt-in and experimental. |

**Current state (verified from DuckDB 1.5.0 release notes and GitHub):**
- PEG parser is opt-in via `enable_peg_parser()` setting
- Already used for auto-complete suggestions
- Grammar extension support for extensions is a stated goal
- NOT the default parser -- the traditional YACC parser remains default
- Experimental status means API may change

**Recommendation:** Do NOT depend on PEG parser for v0.5.4. Keep the C++ shim. File a tech debt item to revisit when PEG parser becomes default (likely DuckDB 2.0, Sep 2026).

**Confidence:** MEDIUM (PEG parser exists but is experimental; grammar extension API not fully documented)

---

## Anti-Features

Features to explicitly NOT build in v0.5.4.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| **Remove explicit cardinality keywords** | Breaking change before registry debut. Users with existing definitions using `ONE TO MANY` would break. | Support both syntaxes. Inference is the default; explicit overrides inference. Deprecation in future milestone. |
| **Many-to-many relationship support** | Snowflake does not support it. The current extension's `Cardinality` enum does not include `ManyToMany`. Adding it requires bridge table patterns that complicate fan trap detection. | Document as not supported. Recommend bridge table decomposition pattern. |
| **PEG parser migration** | Experimental, opt-in, API may change. Premature to depend on it. | File TECH-DEBT.md item. Revisit at DuckDB 2.0. |
| **Stable C API migration** | Would provide binary compatibility across versions. But parser hooks require C++ shim which uses unstable API. Migration would require DuckDB to expose parser hooks via stable C API. | Stay on `C_STRUCT_UNSTABLE`. Use two-branch strategy for multi-version. |
| **Semi-additive metrics (NON ADDITIVE BY)** | Deferred from v0.5.3. Requires structural changes to the expansion pipeline (window function subquery injection). Adds complexity before registry debut. | Defer to v0.5.5+. Document as planned. |
| **Pre-aggregation / materialization** | Out of scope per PROJECT.md. | DuckDB handles execution. Document as non-goal. |
| **YAML definition format** | Adds second definition path. SQL DDL is the sole interface. | Defer. SQL DDL first; YAML is a future path. |
| **Data-driven cardinality inference** | Snowflake counts distinct values at query time to determine one-to-one vs many-to-one. We are a preprocessor -- no query execution at define time. | Use constraint-based inference (UNIQUE/PK declarations). Document that inference is from constraints, not data. |
| **WASM support** | Parser hooks require C++ shim compilation which is not compatible with WASM targets. | Exclude `wasm_mvp;wasm_eh;wasm_threads` in `description.yml`. |
| **Windows Arm64 / MinGW** | Build toolchain complexity for Rust + C++ cross-compilation. | Exclude `windows_arm64;windows_amd64_mingw` in `description.yml`. |

---

## Feature Dependencies

```
T1: UNIQUE + Cardinality Inference
  |
  +-(informs)-> T2: CE Registry Publishing (description.yml uses correct semantics)

T3: Multi-Version DuckDB Support (1.4.x + 1.5.x)
  |
  +-(required by)-> T2: CE Registry Publishing (andium field needs LTS branch)

T4: Documentation Site (Zensical)
  |  (independent -- can be built in parallel)

T2: CE Registry Publishing
  |
  +-(required by)-> D1: MAINTAINER.md Updates (docs must reflect reality)

D2: TPC-H Example (independent -- depends only on v0.5.3 features)
D3: PEG Parser Investigation (independent research -- no implementation)
```

**Critical path:** T3 (multi-version) --> T2 (CE publishing)
**Parallel work:** T1 (UNIQUE/inference), T4 (docs), D2 (TPC-H example)

---

## Complexity Assessment Summary

| Feature | Complexity | Est. LOC | Risk | Phase Order |
|---------|------------|----------|------|-------------|
| T1: UNIQUE + Cardinality Inference | Medium | ~250 | Low-Medium -- additive change, backward compatible | 1st |
| T3: Multi-Version DuckDB Support | Medium-High | ~50 code, ~200 config | Medium -- DuckDB 1.5 API compatibility unknown until tested | 1st (parallel) |
| T4: Documentation Site (Zensical) | Low-Medium | ~0 code, ~2000 words content | Low -- configuration + writing | 2nd (parallel) |
| T2: CE Registry Publishing | Low-Medium | ~0 code, ~50 config | Medium -- first submission, build pipeline unknown | 3rd (depends on T3) |
| D1: MAINTAINER.md Updates | Low | ~500 words | None | 4th (after T2/T3) |
| D2: TPC-H Worked Example | Low | ~100 code | None | Anytime |
| D3: PEG Parser Investigation | Research only | 0 | None | Anytime |
| **Total** | **Medium** | **~300 LOC + ~3000 words + ~250 config** | **Medium** | |

---

## MVP Recommendation

### Phase 1: Foundations (UNIQUE inference + DuckDB 1.5 compatibility)

Build the two features that must exist before registry submission:

1. **UNIQUE + Cardinality Inference (T1):** Add `UNIQUE (col)` to TABLES grammar. Add `unique_columns: Vec<Vec<String>>` to `TableRef`. Implement inference logic in graph module. Keep explicit cardinality keywords working. Update existing tests.

2. **Multi-Version DuckDB (T3):** Create `v1.4-andium` branch from current main. Bump main to DuckDB 1.5.x. Verify build + tests on both. Update Build.yml with dual workflow targets.

### Phase 2: Documentation + Example (parallel with Phase 1)

3. **Documentation Site (T4):** Set up Zensical project structure. Write core content pages. Configure GitHub Pages deployment workflow. Link from README.

4. **TPC-H Worked Example (D2):** Write `examples/tpch.py` demonstrating multi-table semantic view against TPC-H data.

### Phase 3: Registry Submission

5. **CE Registry Publishing (T2):** Create `description.yml`. Submit PR to `duckdb/community-extensions`. Monitor build pipeline. Fix any platform-specific failures. Verify the auto-generated documentation page.

6. **MAINTAINER.md Updates (D1):** Document the dual-branch workflow, CE update process, and version bump procedures.

### Deferral Rationale

- **T1 before T2:** UNIQUE inference should be in place before the first public release. It is a syntax improvement that affects the "hello world" example in the descriptor.
- **T3 before T2:** The `andium` field in `description.yml` requires the LTS branch to exist. Without it, LTS users cannot install the extension.
- **T4 parallel with T1/T3:** Documentation does not depend on code changes. Content can be written from existing features.
- **D3 is research only:** PEG parser investigation informs future milestones, not v0.5.4 implementation.
- **Semi-additive metrics deferred again:** The expansion pipeline structural change is too risky before the first public release. Better to ship a stable, well-documented subset first.

---

## Sources

### Snowflake Official Documentation (HIGH confidence)

- [CREATE SEMANTIC VIEW DDL grammar](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- TABLES clause with PRIMARY KEY and UNIQUE, RELATIONSHIPS with REFERENCES, no explicit cardinality keywords
- [Using SQL commands for semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- UNIQUE interaction with REFERENCES, cardinality inference from PK/UNIQUE declarations
- [Validation rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- Referenced columns must be PK or UNIQUE; many-to-one vs one-to-one from FK value uniqueness; self-references prohibited; circular relationships prohibited
- [Semantic view YAML specification](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- Relationship types automatically inferred, no explicit join_type or relationship_type
- [Semantic view overview](https://docs.snowflake.com/en/user-guide/views-semantic/overview) -- Transitive cardinality inference across relationship chains
- [Querying semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/querying) -- Dimension granularity must be equal or lower than metric granularity
- [Semantic view example (TPC-H)](https://docs.snowflake.com/en/user-guide/views-semantic/example) -- Full worked example with PRIMARY KEY on all tables, no UNIQUE needed for standard FK pattern

### DuckDB Community Extension Registry (HIGH confidence)

- [Community extension documentation](https://duckdb.org/community_extensions/documentation) -- description.yml format, submission process
- [Community extension development guide](https://duckdb.org/community_extensions/development) -- build system, platform support, testing
- [Community extension FAQ](https://duckdb.org/community_extensions/faq) -- licensing, review criteria
- [rusty_quack descriptor (live)](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) -- Rust extension example with `build: cargo`, `andium` field, excluded platforms
- [shellfs descriptor (live)](https://github.com/duckdb/community-extensions/blob/main/extensions/shellfs/description.yml) -- C++ extension with `andium` field
- [UPDATING.md](https://github.com/duckdb/community-extensions/blob/main/UPDATING.md) -- Dual-branch strategy, `ref` + `ref_next`/`andium` system, release process
- [Rust extension template](https://github.com/duckdb/extension-template-rs) -- Build pipeline, CI workflow, platform targets

### DuckDB Release Cycle (HIGH confidence)

- [Release cycle documentation](https://duckdb.org/docs/stable/dev/release_cycle) -- LTS schedule, version naming, extension categories
- [Extension versioning](https://duckdb.org/docs/stable/extensions/versioning_of_extensions) -- Stable vs unstable API, binary compatibility, versioning tiers
- [DuckDB 1.5.0 announcement](https://duckdb.org/2026/03/09/announcing-duckdb-150) -- PEG parser, VARIANT type, GEOMETRY built-in
- [DuckDB 1.4.0 LTS announcement](https://duckdb.org/2025/09/16/announcing-duckdb-140) -- Andium LTS, 1 year support, community vs extended support

### Zensical Documentation (MEDIUM confidence)

- [Zensical GitHub repository](https://github.com/zensical/zensical) -- Successor to MkDocs Material, by same team (squidfunk)
- [Zensical blog announcement](https://squidfunk.github.io/mkdocs-material/blog/2025/11/05/zensical/) -- Created because MkDocs unmaintained since Aug 2024
- [Zensical setup guide](https://zensical.org/docs/create-your-site/) -- Configuration, GitHub Pages deployment
- [Zensical publish guide](https://zensical.org/docs/publish-your-site/) -- GitHub Actions workflow for deployment

### Project Source Code (HIGH confidence -- direct analysis)

- `src/model.rs` -- `TableRef.pk_columns`, `Cardinality` enum (ManyToOne/OneToOne/OneToMany), `Join.cardinality`
- `src/body_parser.rs` -- TABLES clause parser (PRIMARY KEY parsing exists, UNIQUE not yet)
- `src/graph.rs` -- `RelationshipGraph`, cardinality tracking, fan trap detection
- `Cargo.toml` -- `duckdb = "=1.4.4"` version pin, feature flags
- `.github/workflows/Build.yml` -- extension-ci-tools@v1.4.4, single-version build
- `.github/workflows/DuckDBVersionMonitor.yml` -- automated version detection
