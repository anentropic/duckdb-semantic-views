# Requirements: DuckDB Semantic Views v0.5.4

**Defined:** 2026-03-15
**Core Value:** A DuckDB user can define a semantic view once and query it with any combination of dimensions and metrics, without writing GROUP BY or JOIN logic by hand

## v0.5.4 Requirements

### Cardinality Model

- [x] **CARD-01**: TABLES clause supports `UNIQUE (col, ...)` constraint alongside existing `PRIMARY KEY (col)`
- [x] **CARD-02**: A table can have one PRIMARY KEY and multiple UNIQUE constraints (composite allowed)
- [ ] **CARD-03**: Referenced columns in RELATIONSHIPS must match a declared PRIMARY KEY or UNIQUE constraint on the target table -- error at define time if not
- [x] **CARD-04**: Cardinality inferred from constraints: FK column has PK/UNIQUE = one-to-one; FK column bare = many-to-one
- [x] **CARD-05**: Explicit cardinality keywords (ONE TO MANY, MANY TO ONE, ONE TO ONE, MANY TO MANY) removed from parser
- [x] **CARD-06**: ManyToMany variant removed from Cardinality enum
- [x] **CARD-07**: `REFERENCES target` (no column list) resolves to target's PRIMARY KEY; `REFERENCES target(col)` resolves to matching PK or UNIQUE
- [ ] **CARD-08**: Fan trap detection continues to work using inferred cardinality values
- [ ] **CARD-09**: Composite FK referencing a subset of a composite PK is rejected -- only exact PK/UNIQUE match is valid

### Multi-Version DuckDB

- [ ] **DKDB-01**: Extension builds and all tests pass against DuckDB 1.5.x (latest)
- [ ] **DKDB-02**: Extension builds and all tests pass against DuckDB 1.4.x (Andium LTS)
- [ ] **DKDB-03**: `andium` branch maintained for 1.4.x LTS compatibility
- [ ] **DKDB-04**: Build.yml runs CI for both DuckDB versions
- [ ] **DKDB-05**: `.duckdb-version` on main tracks latest; `.duckdb-version` on andium tracks LTS
- [ ] **DKDB-06**: DuckDB Version Monitor updated to check both latest and LTS releases

### Documentation Site

- [ ] **DOCS-01**: Zensical project configured with `zensical.toml` and `docs/` directory structure
- [ ] **DOCS-02**: GitHub Actions workflow deploys docs to GitHub Pages on push to main
- [ ] **DOCS-03**: Site structure includes: getting started, DDL reference, query reference, clause-level pages, examples, architecture overview
- [ ] **DOCS-04**: README links to the documentation site

### Community Extension Registry

- [ ] **CREG-01**: `description.yml` created with all required fields (name, description, version, language, build, license, maintainers, excluded_platforms, requires_toolchains)
- [ ] **CREG-02**: `description.yml` includes `repo.ref` (latest) and `repo.andium` (LTS) commit hashes
- [ ] **CREG-03**: `docs.hello_world` example in descriptor works end-to-end
- [ ] **CREG-04**: PR submitted to `duckdb/community-extensions` and build pipeline passes
- [ ] **CREG-05**: Extension installable via `INSTALL semantic_views FROM community`

### Maintainer Documentation

- [ ] **MAINT-01**: MAINTAINER.md documents multi-version branching strategy (main vs andium)
- [ ] **MAINT-02**: MAINTAINER.md documents CE registry update process (how to update description.yml, cut a release)
- [ ] **MAINT-03**: MAINTAINER.md documents how to bump DuckDB version on both branches

## Future Requirements

### Semi-Additive Metrics

- **SEMI-01**: `NON ADDITIVE BY (dim [ASC|DESC], ...)` syntax on metrics
- **SEMI-02**: Expansion generates window function subquery for non-additive dimensions

### PEG Parser Migration

- **PEG-01**: Investigate DuckDB PEG parser as replacement for C++ shim (when PEG becomes default, likely DuckDB 2.0)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Semi-additive metrics (NON ADDITIVE BY) | Expansion pipeline structural change; defer to post-registry-publish stability |
| PEG parser migration | Experimental in DuckDB 1.5; opt-in only; API may change |
| YAML definition format | SQL DDL is sole interface; YAML is future path |
| Many-to-many relationships | Structurally impossible under Snowflake-style inference (referenced side must have PK/UNIQUE) |
| Data-driven cardinality inference | We are a preprocessor; no query execution at define time |
| Backward compat for explicit cardinality keywords | Pre-release; finding the right design |
| WASM support | Parser hooks require C++ shim; incompatible with WASM targets |
| TPC-H worked example | Deferred to future milestone |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| CARD-01 | Phase 33 | Complete |
| CARD-02 | Phase 33 | Complete |
| CARD-03 | Phase 33 | Pending |
| CARD-04 | Phase 33 | Complete |
| CARD-05 | Phase 33 | Complete |
| CARD-06 | Phase 33 | Complete |
| CARD-07 | Phase 33 | Complete |
| CARD-08 | Phase 33 | Pending |
| CARD-09 | Phase 33 | Pending |
| DKDB-01 | Phase 34 | Pending |
| DKDB-02 | Phase 34 | Pending |
| DKDB-03 | Phase 34 | Pending |
| DKDB-04 | Phase 34 | Pending |
| DKDB-05 | Phase 34 | Pending |
| DKDB-06 | Phase 34 | Pending |
| DOCS-01 | Phase 35 | Pending |
| DOCS-02 | Phase 35 | Pending |
| DOCS-03 | Phase 35 | Pending |
| DOCS-04 | Phase 35 | Pending |
| CREG-01 | Phase 36 | Pending |
| CREG-02 | Phase 36 | Pending |
| CREG-03 | Phase 36 | Pending |
| CREG-04 | Phase 36 | Pending |
| CREG-05 | Phase 36 | Pending |
| MAINT-01 | Phase 36 | Pending |
| MAINT-02 | Phase 36 | Pending |
| MAINT-03 | Phase 36 | Pending |

**Coverage:**
- v0.5.4 requirements: 27 total
- Mapped to phases: 27
- Unmapped: 0

---
*Requirements defined: 2026-03-15*
*Last updated: 2026-03-15 after roadmap creation*
