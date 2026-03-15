# Project Research Summary

**Project:** DuckDB Semantic Views
**Domain:** DuckDB Rust extension -- Snowflake-parity cardinality inference, multi-version support, CE registry publishing, documentation site
**Researched:** 2026-03-15
**Confidence:** HIGH

## Executive Summary

v0.5.4 is a **publishing milestone**: it prepares the extension for its first public release on the DuckDB Community Extension Registry. The four workstreams -- UNIQUE constraint parsing with Snowflake-style cardinality inference, DuckDB 1.5.0 upgrade with LTS backport, Zensical documentation site, and CE registry submission -- are well-understood and have strong precedents in the DuckDB ecosystem. The `rusty_quack` extension proves Rust extensions can be published via `build: cargo`, and the `extension-ci-tools` repository provides dual-version workflows out of the box. The overall risk is **medium**, concentrated in two areas: the DuckDB 1.5.0 amalgamation build (unknown C++ API changes in `shim.cpp`) and the CE registry build pipeline (hybrid Rust+C++ is untested in the registry).

The recommended approach is: (1) implement UNIQUE constraints and cardinality inference first as a self-contained code change, (2) upgrade to DuckDB 1.5.0 and create the LTS backport branch, (3) build the documentation site in parallel with code work, and (4) submit to the CE registry last, after all code and docs are stable. This ordering respects the dependency chain (registry submission requires multi-version builds) and front-loads the riskiest work (inference logic and DuckDB version bump).

The key risk is **backward compatibility during the cardinality syntax change**. Stored JSON definitions from v0.5.3 contain explicit `Cardinality` enum values (`OneToMany`, `OneToOne`). The `Cardinality` enum must be preserved in the model for serde deserialization, and the parser should accept (but deprecate) explicit cardinality keywords during a transition period. Removing the enum or the parser support would break existing definitions -- the single most dangerous mistake this milestone could make.

## Key Findings

### Recommended Stack

DuckDB 1.5.0 "Variegata" (released 2026-03-09) is the primary target, with 1.4.4 "Andium" LTS maintained via a backport branch. The `duckdb-rs` crate changed its versioning scheme: DuckDB 1.5.0 maps to crate version `1.10500.0`. The `libduckdb-sys` crate is no longer needed as a direct dependency for 1.5.0 builds. Zensical (successor to MkDocs Material, by the same squidfunk team) is the documentation tool, deployed to GitHub Pages via the official bootstrap workflow. See [STACK.md](STACK.md) for full details.

**Core technologies:**
- **DuckDB 1.5.0 + 1.4.4 LTS**: dual-version support via separate git branches, matching `extension-ci-tools` convention
- **duckdb-rs 1.10500.0**: new versioning scheme, eliminates `libduckdb-sys` as direct dependency
- **Zensical 0.0.27**: TOML-configured static site generator, GitHub Pages deployment, MkDocs Material successor
- **Community Extension Registry**: `description.yml` with `build: cargo`, `requires_toolchains: "rust;python3"`, `excluded_platforms` for WASM/musl/mingw

### Expected Features

See [FEATURES.md](FEATURES.md) for detailed analysis including Snowflake DDL grammar verification.

**Must have (table stakes):**
- **T1: UNIQUE table constraint + cardinality inference** -- Snowflake-aligned; infer MANY-TO-ONE / ONE-TO-ONE from PK/UNIQUE declarations instead of explicit keywords
- **T2: Community Extension Registry publishing** -- `INSTALL semantic_views FROM community` is the only viable distribution path
- **T3: Multi-version DuckDB support (1.4.x + 1.5.x)** -- required for `andium` field in `description.yml`; LTS users cannot install without it
- **T4: Documentation site on GitHub Pages** -- README is insufficient for a registry-published extension

**Should have (differentiators):**
- **D1: MAINTAINER.md updates** -- dual-branch workflow, CE update process, version bump procedures
- **D2: TPC-H worked example** -- canonical analytics benchmark, Snowflake uses it too, demonstrates credibility

**Defer (v0.5.5+):**
- Semi-additive metrics (NON ADDITIVE BY) -- expansion pipeline structural change, too risky before registry debut
- PEG parser migration -- experimental in 1.5.0, investigate but do not depend on it
- Many-to-many relationships -- Snowflake does not support them; recommend bridge table decomposition
- YAML definition format -- SQL DDL is the sole interface
- Stable C API migration -- parser hooks require C++ shim which uses unstable API

### Architecture Approach

All v0.5.4 code changes stay within the expansion-only preprocessor model. UNIQUE constraints add a new field (`unique_keys: Vec<Vec<String>>`) to `TableRef` in the model. Cardinality inference is a **post-parse semantic pass** in `graph.rs` -- the parser produces `Join` with default `ManyToOne`, then `infer_cardinality()` updates it based on PK/UNIQUE metadata before `validate_graph()` runs. The expansion pipeline (`expand.rs`) and fan trap detection (`check_fan_traps()`) are unchanged -- they read `Join.cardinality` which now comes from inference rather than explicit keywords. See [ARCHITECTURE.md](ARCHITECTURE.md) for component-level detail.

**Major components modified:**
1. **model.rs** -- add `unique_keys` to `TableRef` with backward-compatible serde (`#[serde(default)]`)
2. **body_parser.rs** -- parse UNIQUE in TABLES entries; deprecate explicit cardinality keywords in RELATIONSHIPS
3. **graph.rs** -- new `infer_cardinality()` function (post-parse, pre-validation)
4. **define.rs** -- wire inference into the bind chain before `validate_graph()`

**Components unchanged:** expand.rs, shim.cpp (verify only), catalog persistence, query table function

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for the full taxonomy (4 critical, 4 moderate, 5 minor).

1. **C1: Stored definition backward compatibility** -- removing `Cardinality` enum variants or parser support for explicit keywords breaks existing v0.5.3 definitions. Prevention: keep all enum variants, accept (but deprecate) explicit keywords, use `#[serde(default)]` on new fields.
2. **C2: DuckDB 1.5 amalgamation build breakage** -- `shim.cpp` uses internal C++ classes that may change signatures. Prevention: diff the 1.5.0 amalgamation against 1.4.4 before writing code; build first, features second.
3. **C3: CE registry build pipeline** -- hybrid Rust+C++ (`cc` crate compiling amalgamation) is untested in the `build: cargo` path. Prevention: submit a draft PR early, do not wait until all work is complete.
4. **C4: Composite key inference edge cases** -- partial PK references produce wrong cardinality if not handled correctly. Prevention: require EXACT match of FK columns to a full PK or UNIQUE constraint set; partial matches are errors.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: UNIQUE Constraints + Cardinality Inference

**Rationale:** Self-contained code change with no external dependencies. Must be done before registry submission because it affects the `hello_world` DDL example in `description.yml`. Front-loads the most complex new logic.
**Delivers:** UNIQUE parsing in TABLES, `infer_cardinality()` in graph.rs, backward-compatible deprecation of explicit keywords, updated tests and sqllogictest.
**Addresses:** T1 (UNIQUE + cardinality inference) from FEATURES.md
**Avoids:** C1 (stored definition compat -- keep enum, accept old syntax), C4 (composite key edge cases -- exact match rule), m1 (parser syntax -- extend existing tokenizer pattern)
**Estimated scope:** ~250 LOC across model.rs, body_parser.rs, graph.rs, define.rs + test updates

### Phase 2: DuckDB 1.5.0 Upgrade + LTS Branch

**Rationale:** Required before CE registry submission (the `andium` field needs a 1.4.x branch commit hash). Should be done after Phase 1 so the version bump does not conflict with feature changes. The riskiest infrastructure change.
**Delivers:** DuckDB 1.5.0 as primary target, `v1.4.x` backport branch, dual-CI configuration, updated Cargo.toml (`duckdb = "=1.10500.0"`), verified shim.cpp compilation against 1.5.0 amalgamation.
**Uses:** duckdb-rs 1.10500.0, extension-ci-tools@v1.5.0, dual-workflow Build.yml pattern from STACK.md
**Avoids:** C2 (amalgamation build -- diff before coding, build first), M1 (branch management -- feature dev on main only, LTS gets bug fixes), M4 (parser override -- verify `parse_function` still works in 1.5.0)
**Estimated scope:** ~50 LOC changes, ~200 lines config/CI changes

### Phase 3: Documentation Site

**Rationale:** Can run in parallel with Phase 1 or 2 since it is content-only with no code dependencies. Should be ready before registry submission so the extension has proper docs to link from the CE page.
**Delivers:** Zensical site at `<user>.github.io/duckdb-semantic-views/`, GitHub Actions deployment workflow, DDL reference, query reference, getting started guide, TPC-H example.
**Addresses:** T4 (documentation site), D2 (TPC-H example) from FEATURES.md
**Avoids:** M3 (Zensical deployment -- use official bootstrap workflow, set correct `site_url`), m4 (README duplication -- README becomes a pointer to the docs site)
**Estimated scope:** ~0 LOC, ~2000 words content, ~100 lines config/workflow

### Phase 4: Registry Publishing + MAINTAINER.md

**Rationale:** Final phase -- depends on all three prior phases. Requires stable code (Phase 1), dual-version builds (Phase 2), and documentation (Phase 3). This is the gate to the first public release.
**Delivers:** `description.yml` with `ref` + `andium` commit hashes, PR to `duckdb/community-extensions`, verified `INSTALL ... FROM community; LOAD` on all platforms, updated MAINTAINER.md with dual-branch workflow and CE update procedures.
**Addresses:** T2 (CE registry publishing), D1 (MAINTAINER.md updates) from FEATURES.md
**Avoids:** C3 (registry build pipeline -- test early with draft PR), M2 (cargo vs cmake -- mirror rusty_quack pattern), m2 (name consistency -- `semantic_views` everywhere), m5 (version alignment -- 0.5.4 in both Cargo.toml and description.yml)
**Estimated scope:** ~50 lines config, ~500 words documentation

### Phase Ordering Rationale

- **Phase 1 before Phase 2:** Do not mix feature changes with version changes. Cardinality inference is a semantic change that needs clean test results. The DuckDB 1.5.0 upgrade is a build/infrastructure change that should be verified on a stable codebase.
- **Phase 2 before Phase 4:** The `description.yml` `andium` field requires the LTS branch to exist with a stable commit hash. Multi-version builds must be green before submission.
- **Phase 3 in parallel:** Documentation is content-only and has zero code dependencies. It can be worked on during any phase without merge conflicts.
- **Phase 4 last:** Registry submission is the capstone. All code, tests, builds, and docs must be stable. Submit a draft PR early (after Phase 2) to catch CI issues, but finalize after Phase 3.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (DuckDB 1.5.0 Upgrade):** The amalgamation diff between 1.4.4 and 1.5.0 has not been performed. C++ API changes in `ParserExtension`, `TableFunction`, or `DBConfig` could require shim.cpp rewrites. The `duckdb-rs` 1.10500.0 API surface needs verification for breaking changes. The Windows `patch_duckdb_cpp_for_windows()` function may need marker updates.
- **Phase 4 (Registry Publishing):** The hybrid Rust+C++ build is untested in the CE registry CI. The `build: cargo` path's C++ compiler availability is unknown. A draft PR should be submitted as early as possible to surface issues.

Phases with standard patterns (skip research-phase):
- **Phase 1 (UNIQUE + Inference):** Well-documented Snowflake semantics, existing parser patterns in `body_parser.rs`, established serde backward-compat approach.
- **Phase 3 (Documentation Site):** Zensical bootstrap is a copy-paste workflow. GitHub Pages deployment is standard. Content draws from existing README and examples.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | DuckDB 1.5.0 release verified, duckdb-rs 1.10500.0 on crates.io, extension-ci-tools tags confirmed via GitHub API, Zensical verified from PyPI and GitHub |
| Features | HIGH | Snowflake DDL grammar verified from official docs, CE registry format verified from live rusty_quack descriptor, cardinality inference rules derived from constraint semantics |
| Architecture | HIGH | Direct codebase analysis, component boundaries clear, post-parse inference is a clean pattern |
| Pitfalls | MEDIUM-HIGH | Critical pitfalls well-identified, but DuckDB 1.5.0 amalgamation changes and CE registry CI behavior are unknown until tested |

**Overall confidence:** HIGH

### Gaps to Address

- **DuckDB 1.5.0 amalgamation compatibility:** The `shim.cpp` C++ compilation against the 1.5.0 amalgamation has not been tested. This must be the first task in Phase 2 -- build before features.
- **CE registry CI for hybrid Rust+C++ extensions:** No documented precedent for a `build: cargo` extension that also compiles C++ via the `cc` crate. Submit a draft PR early in Phase 4 to validate.
- **duckdb-rs 1.10500.0 API changes:** The crate uses Rust edition 2024 and Arrow 57. Our code can stay on edition 2021, but any breaking API changes in `duckdb-rs` (renamed methods, changed traits) need investigation.
- **Branching strategy for dual-version Cargo.toml:** STACK.md recommends single-branch dual-CI (Option A), while FEATURES.md and ARCHITECTURE.md recommend separate branches (Option B). The STACK.md recommendation is based on the `parse_function` API being unchanged, but the Cargo.toml version pin (`duckdb = "=1.10500.0"` vs `duckdb = "=1.4.4"`) creates a hard dependency split. **Resolution: use separate branches** (Option B). The extension-ci-tools ecosystem is built around branch-per-version. Single-branch dual-CI works for extensions that do not pin crate versions, but our exact-version pin makes it impractical.

## Sources

### Primary (HIGH confidence)
- [DuckDB 1.5.0 "Variegata" announcement](https://duckdb.org/2026/03/09/announcing-duckdb-150) -- release details, PEG parser status, C API additions
- [duckdb-rs 1.10500.0 release](https://github.com/duckdb/duckdb-rs/releases/tag/v1.10500.0) -- new versioning scheme, edition 2024, Arrow 57
- [rusty_quack description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) -- canonical Rust CE submission template
- [Community Extensions UPDATING.md](https://github.com/duckdb/community-extensions/blob/main/UPDATING.md) -- dual-version strategy, `ref` + `andium` system
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- UNIQUE/PK grammar, cardinality inference rules
- [DuckDB release cycle](https://duckdb.org/docs/stable/dev/release_cycle) -- LTS schedule, version naming
- [DuckDB extension versioning](https://duckdb.org/docs/stable/extensions/versioning_of_extensions) -- ABI types, binary compatibility

### Secondary (MEDIUM confidence)
- [Zensical documentation](https://zensical.org/docs/) -- setup, deployment, configuration
- [Zensical bootstrap workflow](https://github.com/zensical/zensical/blob/master/python/zensical/bootstrap/.github/workflows/docs.yml) -- GitHub Pages deployment template
- [DuckDB parser_override_function_t PR](https://github.com/duckdb/duckdb/pull/19126) -- new parser hook mechanism (1.5.0)
- [Community Extensions Rust guidance (Issue #54)](https://github.com/duckdb/community-extensions/issues/54) -- Rust extension pitfalls

### Tertiary (LOW confidence)
- CE registry CI behavior for `build: cargo` with C++ dependencies -- inferred from `rusty_quack` (pure Rust) pattern; needs validation via draft PR

---
*Research completed: 2026-03-15*
*Ready for roadmap: yes*
