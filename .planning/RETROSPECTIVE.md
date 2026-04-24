# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v0.5.0 — Parser Extension Spike

**Shipped:** 2026-03-08
**Phases:** 5 | **Plans:** 8 | **Commits:** 45

### What Was Built
- C++ shim with vendored DuckDB amalgamation compiled via `cc` crate for parser hook access
- Parser fallback hook (`parse_function`) detecting `CREATE SEMANTIC VIEW` via Rust FFI trampoline
- Statement rewriting: native DDL rewritten to function-based DDL for execution
- Dedicated DDL connection for parser hook execution path (avoids lock conflicts)
- Runtime type validation before `duckdb_vector_reference_vector` (prevents Python crashes)
- Registry-ready binary with C_STRUCT_UNSTABLE ABI, 172 tests green

### What Worked
- Go/no-go phase (Phase 15) as first phase — resolved the highest-risk question before investing in parser work
- Statement rewriting approach — dramatically simpler than building a custom parser grammar
- Phase 17.1 decimal insertion for urgent Python crash investigation — clean disruption handling
- Phase 18 branch integration via cherry-pick — zero conflicts, clean merge of parallel work streams
- Milestone audit before completion — caught the Phase 15 VERIFICATION.md gap and REQUIREMENTS.md checkbox drift

### What Was Inefficient
- Phase 15 never produced a VERIFICATION.md — downstream phases covered the requirements, but the documentation gap persisted until audit
- REQUIREMENTS.md checkboxes drifted — 11 requirements were satisfied but showed `[ ]` until a bulk audit update
- Nyquist validation files created but never marked compliant — process gap carried forward from v0.2.0

### Patterns Established
- Static-linked amalgamation for parser hook access (bypasses `-fvisibility=hidden`)
- C_STRUCT entry + C++ helper function pattern (not CPP entry) for mixed Rust+C++ extensions
- Statement rewriting as parser hook strategy (simpler than custom grammar)
- Dedicated DDL connection from parser hook path (same pattern as semantic_query)
- `catch_unwind` at every FFI boundary for panic safety

### Key Lessons
1. Static linking against DuckDB amalgamation solves the `-fvisibility=hidden` problem — dynamic symbol resolution is impossible but static linking bypasses it entirely
2. Statement rewriting is a viable and simpler alternative to custom parser grammar — the parser hook only needs to detect the prefix, not parse the full statement
3. Phase verification documentation should be produced at phase completion, not deferred — the Phase 15 gap required retroactive closure
4. Python client exercises different code paths than CLI/sqllogictest — the vtab crash investigation (Phase 17.1) found defensive gaps not caught by Rust tests
5. Amalgamation compilation adds ~20MB to binary — acceptable for a spike, but selective linking should be explored for production

### Cost Observations
- 45 commits in 2 days
- 8 plans averaging ~7 min each (~56 min total execution)
- Notable: Phase 17.1 (Python crash investigation) was the only unplanned insertion — all other phases executed as roadmapped

---

## Milestone: v1.0 — MVP

**Shipped:** 2026-02-28
**Phases:** 7 | **Plans:** 18 | **Commits:** 99

### What Was Built
- Loadable DuckDB extension in Rust with function-based DDL for semantic view definitions
- Pure Rust expansion engine: GROUP BY inference, join dependency resolution, filter composition, identifier quoting
- `semantic_query` table function with FFI SQL execution via independent DuckDB connection
- `explain_semantic_view` for SQL expansion transparency
- Sidecar file persistence with atomic writes for catalog survival across restarts
- Multi-platform CI (5 targets), DuckDB version monitor, code quality gates
- Three cargo-fuzz targets, proptest property-based tests, DuckLake/Iceberg integration test
- Comprehensive MAINTAINER.md and TECH-DEBT.md for contributor onboarding

### What Worked
- TDD approach in Phase 3 (expansion engine) — 14 unit tests drove clean implementation
- Feature split (bundled/extension) — solved the fundamental DuckDB Rust extension testing problem early
- Phase-by-phase execution with summaries — clear audit trail and easy verification
- Property-based testing caught edge cases in GROUP BY inference that unit tests missed
- Sidecar file pattern — pragmatic workaround for DuckDB's execution lock limitation

### What Was Inefficient
- Phase 2 took longest (35 min, 4 plans) — DDL-05 persistence gap required an unplanned 4th plan
- Phase 4 table function FFI work (53 min) — duckdb_string_t decode and VARCHAR casting required multiple debugging iterations
- Some ROADMAP.md progress table entries were inconsistent (Phase 3 showed 0/3 but was complete)
- Audit identified tech debt that could have been caught during execution (dead code, feature-gate inconsistency)

### Patterns Established
- Cargo feature split pattern: `default=["duckdb/bundled"]` for testing, `extension=["duckdb/loadable-extension"]` for builds
- Manual FFI entrypoint pattern: capture raw duckdb_database handle for independent connections
- Sidecar file persistence: write-to-tmp-then-rename for atomic writes
- VARCHAR-cast wrapper pattern for safe FFI value reading
- CTE-based expansion with flat namespace for join flattening
- PRAGMA database_list for host DB path resolution (not filtered by name)

### Key Lessons
1. DuckDB's execution locks during scalar `invoke` make SQL execution from within callbacks impossible — design for this constraint from the start
2. `duckdb-rs` loadable-extension feature replaces ALL C API calls with stubs — standalone test binaries can't use them; the bundled/extension feature split is mandatory
3. Property-based tests are more valuable for SQL generation than additional unit tests — they explore the combinatorial space automatically
4. Manual FFI is sometimes necessary even with good Rust bindings — the duckdb_entrypoint_c_api macro hides the database handle needed for independent connections
5. Always prototype the highest-risk integration point first (Phase 4's re-entrant query execution was flagged early)

### Cost Observations
- Total execution time: ~90 min across 18 plans
- Average plan duration: 5 min (median), 6 min (mean)
- Longest phase: Query Interface (53 min, 3 plans) — FFI debugging dominated
- Shortest phases: Hardening (6 min, 2 plans), Tech Debt (3 min, 1 plan)
- Documentation/verification plans completed in 1-3 min each

---

## Milestone: v0.2.0 — Native DDL + Time Dimensions

**Shipped:** 2026-03-03
**Phases:** 8 (including 11.1) | **Plans:** 25 | **Commits:** 125

### What Was Built
- C++ shim infrastructure with cc crate, vendored DuckDB headers, feature-gated compilation
- Time dimensions with date_trunc codegen, granularity coarsening (day/week/month/year), per-query override
- pragma_query_t catalog persistence replacing sidecar file — transactional, write-first pattern
- Scalar function DDL (create_semantic_view, drop_semantic_view) after architecture pivot from parser hooks
- Snowflake-aligned STRUCT/LIST DDL syntax with 6-arg typed parameters
- Typed output columns with binary-read dispatch (replacing all-VARCHAR)
- 36 property-based tests for type dispatch covering TIMESTAMP, BOOLEAN, DECIMAL, LIST, ENUM, NULL
- DuckLake integration test refresh to v0.2.0 API with parallel CI job

### What Worked
- Architecture pivot handled cleanly — Phase 11 discovered parser hooks impossible, pivoted to scalar DDL without wasted effort
- Binary-read dispatch (Phase 13) with PBTs caught real bugs: TIMESTAMP all-NULL, BOOLEAN UB, DECIMAL-as-string
- Phase 11.1 (inserted decimal phase) worked well for urgent syntax alignment without disrupting roadmap numbering
- pragma_query_t write-first pattern with separate persist_conn solved the deadlock-free persistence problem elegantly
- Quick tasks (6 total) kept CI green without disrupting phase flow

### What Was Inefficient
- Phase 11 plans 01-03 built C++ parser hook infrastructure that was ultimately discarded when `-fvisibility=hidden` was discovered
- ROADMAP.md progress table drifted from reality (Phase 9 showed "0/?" despite being complete; Phase 11 showed "2/4")
- REQUIREMENTS.md traceability table was never updated after initial creation — all TIME/PERSIST requirements stayed "Pending" despite phases completing
- Phase 12 SUMMARY files had empty `provides` fields — one-liner extraction failed for these

### Patterns Established
- Write-first pragma persistence: invoke → pragma → table → in-memory (avoids lock conflicts)
- cc crate C++ compilation gated behind `CARGO_FEATURE_EXTENSION` env var
- Symbol visibility: `--version-script` on Linux, `-exported_symbols_list` on macOS
- Binary-read type dispatch: match on DuckDB logical type, read directly from chunk (no VARCHAR cast)
- LIMIT 0 type inference at define time for zero-cost column type discovery
- Decimal phase insertion (11.1) for urgent work between existing phases

### Key Lessons
1. Python's DuckDB compiles ALL C++ with `-fvisibility=hidden` — any extension feature depending on C++ symbol resolution is impossible when loaded via Python
2. C API function pointers (via `loadable-extension` stubs) are the ONLY reliable entry point — design all extension interfaces around them
3. PBT-driven type dispatch is dramatically more effective than manual test cases — 36 properties found 3 real bugs that unit tests missed
4. Keep traceability tables updated during execution, not just at milestone close — stale tables create confusion
5. Quick tasks for CI fixes are essential — 6 quick tasks kept the pipeline green without blocking phase work

### Cost Observations
- 125 commits in 3 days
- 8 phases, 25 plans, ~102 commits of substance + 23 CI/fmt fixes
- Notable: Phase 11 architecture pivot (parser hook → scalar DDL) was the highest-risk moment; recovery was clean

---

## Milestone: v0.3.0 — Zero-Copy Query Pipeline

**Shipped:** 2026-03-03
**Scope:** Single refactor of `src/query/table_function.rs` (-738, +151 lines)

### What Was Built
- Zero-copy vector reference pipeline replacing binary-read dispatch
- `StreamingState` with `Mutex` for chunk-by-chunk streaming (reduced peak memory)
- `build_execution_sql` cast wrapper for type mismatch handling at SQL generation time
- `tests/vector_reference_test.rs` validating lifetime safety, multi-chunk, LIST/STRUCT types

### What Worked
- The refactor was a clean replacement — zero-copy is simpler, faster, and eliminates an entire category of type dispatch bugs
- `duckdb_vector_reference_vector` shares buffer ownership, confirmed by dedicated tests
- Moving type mismatch handling to SQL generation time (`build_execution_sql`) is more maintainable than handling it at read/write time
- Done outside GSD planning process — appropriate for a focused single-file refactor

### Key Lessons
1. `duckdb_vector_reference_vector` creates shared ownership (not a shallow alias) — source chunk destruction is safe after reference
2. Type mismatches between bind-time and runtime are better handled at SQL generation time than at read/write time
3. Binary-read dispatch was over-engineered for what DuckDB already handles natively — let DuckDB own the data format

### Patterns Established
- Zero-copy vector transfer: `duckdb_vector_reference_vector(dst, src)` per column per chunk
- SQL-time type casting via wrapper query for known mismatch patterns
- `StreamingState` with `Mutex<Option<...>>` for lazy-init streaming in VTab `func()`

---

## Milestone: v0.5.1 — DDL Polish

**Shipped:** 2026-03-09
**Phases:** 5 (19-23) | **Plans:** 9 | **Commits:** ~30

### What Was Built
- Empirical validation spike (Phase 19) confirming all 7 DDL prefixes trigger parser fallback hook
- `DdlKind` enum with multi-prefix detection/rewrite for DROP, CREATE OR REPLACE, IF NOT EXISTS, DESCRIBE, SHOW
- C++ result-forwarding pipeline with dynamic column schemas per DDL form (DESCRIBE: 6 cols, SHOW: 2 cols)
- `ParseError` struct with byte-accurate positions; tri-state `sv_validate_ddl_rust` FFI for clause hints and "did you mean" suggestions
- README DDL syntax reference with full lifecycle example (create → query → describe → show → drop)
- 33 property-based tests for all 7 parser public functions + Python caret integration test through full extension pipeline

### What Worked
- Phase 19 empirical spike before implementation — eliminated all uncertainty about which DDL forms are viable before a single line of production code was written
- Tri-state FFI design (0=not-semantic, 1=valid, 2=invalid) — clean separation of prefix detection, validation, and rewriting responsibilities
- Phase 21 gap closure (Phase 21-03) — `scan_clause_keywords` dual-delimiter fix caught a real correctness bug in error reporting
- Milestone audit before archiving — caught all gaps, all 16 requirements triple-confirmed

### What Was Inefficient
- Phase 21 required 3 plans (01: core implementation, 02: integration tests, 03: gap closure) — the delimiter gate bug wasn't caught until tests were written in Plan 02
- Nyquist VALIDATION.md files created for all 5 phases but none populated — same process gap from v0.5.0 carried forward (all 5 marked as "MISSING" in audit)
- Progress table in ROADMAP.md had formatting inconsistencies (Milestone and Plans columns mixed up in Phase 19 row)

### Patterns Established
- Dual-delimiter clause keyword scanning (`:=` and `(`) — both syntaxes present in the codebase for different contexts
- Proptest `arb_case_variant` strategy for case-insensitive DDL testing — `vec(bool)` drives per-character casing
- Parameterized DDL form testing via index strategy into const arrays (avoids proptest macro limitations)
- Python caret extraction pattern: subtract `LINE 1: ` prefix (8 chars) to get 0-based offset

### Key Lessons
1. Empirical spikes before parser extension work eliminate the risk of building infrastructure for an impossible goal — Phase 19 took 1 plan vs. days of wasted effort
2. Tri-state FFI (not-semantic / valid / invalid) cleanly handles prefix detection, validation failure, and validation success as distinct code paths
3. Integration tests written immediately after implementation catch delimiter/protocol bugs that unit tests miss — Plan 02 found the `(` syntax gap that Plan 01 missed
4. Nyquist VALIDATION.md process needs a completion gate — if validation files exist but are empty, they provide false confidence

### Cost Observations
- 5 phases, 9 plans in 2 days
- Phase 23 (proptests) was the largest plan by LOC (+655 lines for `parse_proptest.rs`)
- Notable: milestone audit found zero gaps — first milestone to pass audit clean since v0.5.0

---

## Milestone: v0.5.2 — SQL DDL & PK/FK Relationships

**Shipped:** 2026-03-13
**Phases:** 5 active (Phase 24 cancelled) | **Plans:** 14 | **Commits:** 89

### What Was Built
- SQL keyword body parser: state-machine clause boundary detection for TABLES, RELATIONSHIPS, DIMENSIONS, METRICS
- Parser robustness hardening: token-based DDL detection, adversarial input safety, fuzz_ddl_parse target
- RelationshipGraph module: Kahn's algorithm toposort, diamond/cycle detection, define-time validation
- Alias-based FROM+JOIN expansion with qualified column refs, replacing CTE flattening pattern
- Function DDL retirement: DefineSemanticViewVTab + parse_args.rs removed; native DDL is sole interface
- README rewritten with AS-body PK/FK syntax examples; 3-table E2E integration test

### What Worked
- Phase 24 cancellation with absorption into Phase 25-01 — model fields added as auto-fix rather than separate phase, saved a full phase of overhead
- JSON-bridge pattern (AS-body parsed in Rust → JSON → DefineFromJsonVTab) — avoided building a second VTab, reused existing infrastructure
- Kahn's algorithm for toposort — naturally detects cycles as leftover nodes, simpler than DFS-based approaches
- Bidirectional join lookup in expand.rs — handles FK source and target aliases without separate traversal
- Phase 28 integration testing caught real issues (DESCRIBE JSON field names, phase28_e2e.test stale data)

### What Was Inefficient
- Phase 24 planned as separate phase but all its work absorbed into Phase 25-01 — planning overhead for a phase that never executed
- ROADMAP.md plan checkboxes inconsistent (some phases showed `[ ]` on completed plans) — manual tracking drift
- STATE.md progress showed 93% because the CLI counted Phase 24 as incomplete rather than cancelled
- Nyquist VALIDATION.md files still mostly drafts (4/5 partial) — same process gap from v0.5.0 and v0.5.1

### Patterns Established
- State-machine clause parser (`find_clause_bounds`) for multi-keyword SQL body parsing
- `skip_serializing_if` on new model fields for backward-compatible JSON evolution
- RelationshipGraph with adjacency list + reverse edges for O(1) validation
- Flat FROM+LEFT JOIN expansion pattern (no CTE) with qualified column scoping
- Phase cancellation with absorption — cancel phase, add its work as auto-fix to downstream plan

### Key Lessons
1. Phase cancellation is better than phase execution when the work naturally fits into downstream plans — avoid the overhead of a separate phase for model-only changes
2. JSON-bridge patterns are powerful for integrating new parser paths with existing VTab infrastructure — parse differently, serialize to the same format
3. Topological sort ordering produces deterministic SQL regardless of DDL declaration order — critical for test stability
4. Retiring old interfaces early (function DDL) reduces test and maintenance surface area — -400 LOC and simpler test files
5. Define-time graph validation (cycles, diamonds) prevents confusing query-time errors — fail fast at DDL, not at SELECT

### Cost Observations
- 89 commits in 5 days
- 14 plans averaging ~13 min each (~180 min total execution)
- Phase 24 cancelled (0 execution time), Phase 25 was largest (4 plans, ~55 min)
- Notable: function DDL retirement (Phase 28-01) was cleaner than expected — 2 tasks, 18 min, -400 LOC

---

## Milestone: v0.5.3 — Advanced Semantic Features

**Shipped:** 2026-03-15
**Phases:** 4 (29-32) | **Plans:** 8 | **Commits:** 66

### What Was Built
- FACTS clause: named row-level sub-expressions with DAG validation (Kahn's algorithm) and word-boundary-safe expression inlining
- HIERARCHIES clause: drill-down path metadata validated against declared dimensions at define time
- Derived metrics: metric-on-metric composition with stacked inlining, aggregate prohibition, and transitive join resolution
- Fan trap detection: cardinality model (MANY TO ONE / ONE TO ONE / ONE TO MANY) with LCA-based tree path analysis blocking one-to-many aggregation fan-out
- Role-playing dimensions: same table via multiple named relationships with scoped aliases ({alias}__{rel_name})
- USING RELATIONSHIPS: explicit join path selection per metric with ambiguity detection and transitive USING inheritance
- DESCRIBE extended to 8 columns (facts + hierarchies) with backward-compatible null-to-[] fallback

### What Worked
- Phase ordering (FACTS → derived → fan traps → USING) was correct — each phase built cleanly on prior work
- Reusing established patterns (Kahn's algorithm, word-boundary matching, skip_serializing_if) accelerated implementation
- TDD approach in Phases 31-32 — writing failing tests first caught semantic misunderstandings (e.g., USING controls dimension alias, not metric aggregation)
- Fan trap deviation decision (blocking errors vs. warnings) was made during planning, not during implementation — clean execution
- Semi-additive metrics deferral to v0.5.4 kept scope tight — only feature requiring structural pipeline change

### What Was Inefficient
- Phase 29 Plan 01 was the longest plan (72 min) due to adding hierarchies field to 23+ struct literals across the codebase
- Pre-commit hook formatting failures required repeated re-staging (occurred in nearly every plan)
- Proptest arb_identifier() generated SQL keywords (e.g., "as_") causing parser confusion — pre-existing issue that surfaced in Phase 31
- Nyquist VALIDATION.md files created but not marked compliant (same pattern from prior milestones)

### Patterns Established
- Clause ordering in DDL: TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS
- Fact inlining: toposort → resolve in order → parenthesize → apply to metric expressions
- Derived metric resolution: inline_derived_metrics resolves ALL metrics (base + derived) in one pass
- Cardinality model: skip_serializing_if with is_default() for backward-compatible enum defaults
- Scoped alias pattern: {to_alias}__{rel_name} for role-playing JOINs with double-underscore separator
- Diamond relaxation: allow multi-path when all relationships have unique names

### Key Lessons
1. Adding a new field to a widely-used struct (SemanticViewDefinition) creates a large blast radius of required changes — consider Default derive or builder pattern for future struct extensions
2. Word-boundary matching (is_word_boundary_byte) is essential for expression inlining — naive string replacement causes substring collisions (e.g., "net_price" matching in "net_price_total")
3. USING semantics must be clearly defined before implementation — "USING controls dimension alias resolution, not metric aggregation" was a crucial design insight
4. Fan trap detection as blocking errors is safer than warnings — users don't read warnings
5. Derived metrics need transitive dependency walking for both join resolution and USING context inheritance

### Cost Observations
- 66 commits in 2 days
- 8 plans averaging 20 min each (156 min total execution)
- Phase 29 was slowest (87 min, 2 plans) — structural model changes dominated
- Phases 30-32 averaged 23 min each — patterns established in Phase 29 accelerated later phases
- Notable: fastest milestone execution yet on a per-plan basis outside of Phase 29

---

## Milestone: v0.5.4 — Snowflake-Parity & Registry Publishing

**Shipped:** 2026-03-27
**Phases:** 6 (33-36, including 34.1, 34.1.1 inserted) | **Plans:** 12 | **Commits:** 117

### What Was Built
- Snowflake-style cardinality inference: UNIQUE constraints + PK/FK matching replaces explicit keywords; two-variant Cardinality enum
- DuckDB 1.5.0 upgrade with parser_extension_compat.hpp, per-process test runner, LTS branch (duckdb/1.4.x)
- DDL surface parity: ALTER SEMANTIC VIEW RENAME TO, 6 SHOW SEMANTIC commands (DIMS/METRICS/FACTS single+cross-view, FOR METRIC fan-trap-aware)
- SHOW command filtering: LIKE, STARTS WITH, LIMIT clause parsing via WHERE/LIMIT injection
- Sphinx + Shibuya documentation site on GitHub Pages with CI/CD and PR build checks
- CE registry readiness: description.yml, MIT license, MAINTAINER.md with multi-branch workflow, snowflake_parity.py example

### What Worked
- Integration checker at milestone audit found 3 real bugs in example files (wrong IF EXISTS position, removed cardinality keywords) — valuable safety net
- Phase 34.1 and 34.1.1 decimal insertions for urgent Snowflake DDL parity — clean scope additions without disrupting main roadmap
- Parser-level SHOW filtering (WHERE/LIMIT injection) — zero VTab changes needed, single implementation covers all 4 SHOW kinds
- Documentation site shipped separately from code (Phase 35) — clean dependency on stable DDL syntax before writing docs
- Phase 36 Plan 03 designed as non-autonomous with human-action gates — correct for CE submission which requires human GitHub action

### What Was Inefficient
- Phase 34 never received a VERIFICATION.md — same gap pattern from v0.5.0 Phase 15; caught during milestone audit
- Nyquist VALIDATION.md files created for all 6 phases but none completed until retroactive audit (same recurring pattern)
- Example files (advanced_features.py) not updated when Phase 33 removed cardinality keywords — regression not caught until integration checker
- 2 inserted phases (34.1, 34.1.1) expanded a 4-phase milestone to 6 — scope creep from Snowflake comparison analysis

### Patterns Established
- parser_extension_compat.hpp for DuckDB version-specific type re-declarations
- Per-process sqllogictest execution for parser extension lifecycle compatibility
- VTab pair pattern: single-view (1 param) + cross-view (0 params) sharing bind/init types
- Parser-level filter clause injection (LIKE→ILIKE, STARTS WITH→LIKE prefix%, LIMIT) for SHOW commands
- PLACEHOLDER_COMMIT_SHA workflow for CE submission (replaced after squash-merge)

### Key Lessons
1. Integration checkers at milestone boundary catch cross-phase bugs that phase-level verification misses — examples not updated after breaking model changes
2. Decimal phase insertion (34.1, 34.1.1) works well but increases milestone scope — consider whether insertions should trigger scope review
3. DuckDB 1.5.0 moved types from headers to .cpp — compat headers must match exactly including ALL constructors (ODR violations cause segfaults)
4. Non-autonomous plans with human-action gates are the correct pattern for external-dependency work (CE submission)
5. Nyquist validation needs a completion gate in execute-phase, not just file creation — 10th consecutive milestone with this gap

### Cost Observations
- 117 commits in 13 days
- 12 plans, 5 quick tasks
- Phase 34 Plan 01 was longest (90 min) — DuckDB version upgrade with C++ compat investigation
- Phase 36 plans were fastest (4-5 min each) — documentation and config only
- Notable: integration checker agent cost ~122K tokens but found 3 real bugs

---

## Milestone: v0.5.5 — SHOW/DESCRIBE Alignment & Refactoring

**Shipped:** 2026-04-05
**Phases:** 6 | **Plans:** 11 | **Commits:** 71

### What Was Built
- Extracted util.rs and errors.rs leaf modules, breaking expand<->graph and parse<->body_parser circular dependencies
- Split expand.rs (4,299 lines) into 7 submodules and graph.rs (2,333 lines) into 5 submodules — module directories with mod.rs re-exports
- Added created_on, database_name, schema_name metadata fields captured at define time, with backward-compatible deserialization
- Aligned all SHOW commands to Snowflake column schemas (5-col VIEWS, 6-col DIMS/METRICS/FACTS, 4-col DIMS FOR METRIC)
- Rewrote DESCRIBE to property-per-row format (5 VARCHAR columns, 6 object kinds)
- Hardened persistence: TOCTOU fix with single write locks, parameterized prepared statements

### What Worked
- Refactoring-first phase ordering — behavior-preserving splits (37-38) created stable foundation for feature work (39-41)
- Module directory pattern with pub(super) — clean internal API boundaries without exposing internals
- Phase 42 as explicit code review follow-up — addressed findings that accumulated during feature phases
- Parallel phase execution where dependencies allowed (40 and 41 both depended on 39, not each other)
- Milestone audit before completion caught TIDY checkbox paperwork and missing SUMMARY.md files

### What Was Inefficient
- Phase 42 executed without producing SUMMARY.md files (3 plans) — paperwork gap carried to audit
- REQUIREMENTS.md TIDY checkboxes not updated during Phase 42 execution — required bulk fix at milestone close
- Phase 39 took 107min (longest plan) — metadata capture via DuckDB SQL functions required more investigation than expected
- VALIDATION.md nyquist_compliant frontmatter never flipped to true across any phase — persistent process gap

### Patterns Established
- Module directory decomposition pattern: mod.rs re-exports, submodules use pub(super), test blocks distributed to submodules
- Leaf module extraction for breaking circular dependencies (shared types/functions with zero intra-crate imports)
- Define-time metadata capture via DuckDB SQL functions (now(), current_database(), current_schema())
- Property-per-row VTab pattern for DESCRIBE (DescribeRow struct with bind-time collection)
- Parameterized prepared statements via execute_parameterized helper

### Key Lessons
1. Behavior-preserving refactoring phases should always precede feature phases that depend on the refactored code — Phase 37-38 made 39-41 much smoother
2. Module splits of 4,000+ line files are safe when you preserve the exact public API surface and move tests to their correct submodules
3. DuckDB SQL functions (now(), typeof()) are the right way to capture metadata at define time — Rust SystemTime would give different semantics
4. Property-per-row format is more flexible than JSON blobs for introspection VTabs — easier to filter, sort, and extend
5. Code review phases (like Phase 42) are valuable for catching accumulated issues, but should produce SUMMARY.md files like any other phase

### Cost Observations
- 71 commits in 5 days (2026-04-01 → 2026-04-05)
- 11 plans, 68 files modified, +6,785 / -5,086 lines (net +1,699)
- Notable: Phase 39 (metadata storage) was disproportionately slow at 107min vs 5-32min for other plans

---

## Milestone: v0.6.0 — Snowflake SQL DDL Parity

**Shipped:** 2026-04-14
**Phases:** 8 (43-50) | **Plans:** 16 | **Commits:** 114

### What Was Built
- Metadata annotations: COMMENT, SYNONYMS, PRIVATE/PUBLIC on all DDL objects with backward-compatible serde persistence
- Introspection enhancements: SHOW TERSE, IN SCHEMA/DATABASE scoping, SHOW COLUMNS, metadata columns in SHOW/DESCRIBE output
- ALTER SET/UNSET COMMENT + GET_DDL round-trip DDL reconstruction (first VScalar in the extension)
- Wildcard selection (table_alias.*) with PRIVATE exclusion + queryable FACTS (row-level unaggregated mode)
- Semi-additive metrics (NON ADDITIVE BY) with CTE-based ROW_NUMBER snapshot selection, effectively-regular classification
- Window function metrics (PARTITION BY EXCLUDING) with CTE-based inner aggregation + outer window SELECT
- Security hardening: FFI catch_unwind on all 25 entry points, graceful lock-poison handling, cycle detection + depth limits
- Code quality: 38 new unit tests for untested modules, resolve_names generic helper, DimensionName/MetricName newtypes

### What Worked
- Tier ordering (model+DDL → expansion mods → structural pipeline changes) — earlier phases were incremental, later phases could build on stable foundation
- All new model fields use #[serde(default)] from the start — zero backward-compatibility issues through 8 phases of model evolution
- CTE-based expansion patterns (semi-additive, window) kept the main expansion pipeline clean — new metric types are separate modules, not branches in the main expand()
- Security hardening as a dedicated phase (49) after all features complete — clean audit of the full surface area without feature pressure
- Code quality phase (50) as final phase caught real issues: 38 new tests found untested paths, resolve_names deduplication removed 150+ LOC
- Milestone audit passed clean (34/34 requirements) — the only gaps were REQUIREMENTS.md checkboxes (documentation tracking, not implementation)

### What Was Inefficient
- REQUIREMENTS.md checkboxes still not updated during phase execution — 16 checkboxes remained unchecked despite verified implementations (same pattern from v0.5.0+)
- VALIDATION.md files created but never marked nyquist_compliant across all 8 phases — persistent process gap
- Phase 44 SUMMARY one-liner extraction returned "Status:" for 2 plans — frontmatter parsing issue in gsd-tools
- Some phase SUMMARY files had inconsistent frontmatter (missing requirements_completed field for most plans)

### Patterns Established
- CTE-based metric expansion: semi_additive.rs (ROW_NUMBER snapshot) and window.rs (inner agg + outer window) as separate expand/ submodules
- Effectively-regular classification: when all NON ADDITIVE BY dimensions appear in query, skip CTE and aggregate normally
- PARTITION BY EXCLUDING as set difference: window partitions = queried dims - excluded dims, computed at query time
- FFI catch_unwind boundary pattern: AssertUnwindSafe justified, panic → error conversion at every C++ entry point
- Graceful lock-poison handling: .map_err() with descriptive string (not into_inner() recovery)
- DimensionName/MetricName newtypes with case-insensitive Eq/Hash for compile-time domain safety
- Generic resolve_names helper with closure-based error construction (9 params) for resolution loop deduplication
- NaGroup named struct replacing anonymous tuple for semi-additive grouping

### Key Lessons
1. CTE-based expansion modules (one per metric type) scale better than branching in the main expand() — semi-additive and window metrics are completely independent code paths
2. Effectively-regular classification is critical for semi-additive correctness — when all NA dimensions are queried, the CTE is unnecessary overhead and can produce wrong results
3. FFI catch_unwind + AssertUnwindSafe is safe at FFI boundaries when no partially-mutated state is observable — the justification must be per-site
4. Newtypes for query resolution names (DimensionName/MetricName) catch entire categories of case-sensitivity bugs at compile time
5. Code quality phases at milestone end are high-value — Phase 50 found real coverage gaps and refactoring opportunities that accumulated across 7 prior phases
6. REQUIREMENTS.md checkbox drift is the most persistent process gap across all milestones — needs automation or a phase-completion hook

### Cost Observations
- 114 commits in 10 days (2026-04-05 → 2026-04-14)
- 16 plans, 166 files modified, +35,682 / -4,023 lines (net +31,659)
- Phases 49-50 (security + quality) accounted for ~190 min — valuable cleanup time
- Notable: Phase 47 P01 was fastest feature plan (31 min) — model+parser pattern fully established by Phase 43

---

## Milestone: v0.7.0 — YAML Definitions & Materialization Routing

**Shipped:** 2026-04-24
**Phases:** 7 | **Plans:** 7 | **Commits:** 78

### What Was Built
- YAML definition format: inline (`FROM YAML $$...$$`), file-based (`FROM YAML FILE`), and export (`READ_YAML_FROM_SEMANTIC_VIEW`) with lossless round-trip
- Materialization routing: MATERIALIZATIONS clause in DDL and YAML, transparent query routing to pre-aggregated tables on exact dim/metric match, semi-additive/window exclusion
- Materialization introspection: EXPLAIN routing header, DESCRIBE MATERIALIZATION rows, SHOW SEMANTIC MATERIALIZATIONS command
- Dollar-quote extraction (tagged and untagged), sentinel protocol for Rust-to-C++ file loading, tagged dollar-quote collision avoidance

### What Worked
- Single-plan-per-phase structure: 7 phases with 1 plan each kept execution lean and focused
- Dual-track roadmap (YAML 51-53, Materialization 54-55, convergence 56-57) allowed independent development with clean integration
- Pure-function routing design: `try_route_materialization()` has no side effects, no DB access — trivially testable
- Sentinel protocol for FFI file loading: clean separation of Rust parsing and C++ file reading
- `from_yaml_with_size_cap` inherited from Phase 51 gave all YAML paths automatic DoS protection

### What Was Inefficient
- UAT initially written with wrong DDL syntax (NON ADDITIVE BY after AS instead of before) — revealed that the body parser silently accepts modifiers as part of the expression string when placed after AS
- REQUIREMENTS.md checkbox tracking fell behind — 10/19 checked at milestone close despite all being implemented

### Patterns Established
- Dollar-quote extraction pattern reusable for any multi-line string literal in DDL
- VTab pair pattern (single-view + cross-view AllVTab) for SHOW commands — now used by 5 SHOW commands
- Feature-gated re-export pattern for cross-module access to extension-only code
- YAML-JSON equivalence testing via proptest strategies

### Key Lessons
- The body parser DDL syntax ordering (modifiers before AS) should produce a warning or error when modifiers appear after AS — silent acceptance is a user experience gap
- Fastest milestone yet (7 days, 7 phases) — the model/parser/test patterns from v0.5.2-v0.6.0 are now fully mature
- yaml_serde (serde_yaml_ng) was a seamless drop-in; the serde ecosystem makes adding new serialization formats trivial when the model already derives serde traits

### Cost Observations
- 7 phases in 7 days — average <1 day per phase
- Lightest plan overhead of any milestone: 1 plan per phase, no revisions needed
- Security audit covered 22 threats across 7 phases with 0 open threats

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Commits | Phases | Key Change |
|-----------|---------|--------|------------|
| v1.0 | 99 | 7 | Initial release — established all patterns |
| v0.2.0 | 125 | 8 | Architecture pivot (parser hook → scalar DDL), typed output, PBTs |
| v0.3.0 | 1 | — | Zero-copy refactor — replaced binary-read dispatch (-600 LOC) |
| v0.4.0 | — | — | Breaking simplification — removed time_dimensions/granularities |
| v0.5.0 | 45 | 5 | Parser extension spike — native DDL via C++ shim + statement rewriting |
| v0.5.1 | ~30 | 5 | DDL Polish — 7 DDL verbs, error location reporting, 33 parser PBTs + Python caret tests |
| v0.5.2 | 89 | 5 | SQL DDL body + PK/FK relationships, graph validation, function DDL retired |
| v0.5.3 | 66 | 4 | FACTS, derived metrics, hierarchies, fan traps, role-playing dims, USING |
| v0.5.4 | 117 | 6 | Cardinality inference, DuckDB 1.5.0, DDL parity, SHOW filtering, docs site, CE readiness |
| v0.5.5 | 71 | 6 | Module refactoring, Snowflake-aligned SHOW/DESCRIBE, metadata storage, persistence hardening |
| v0.6.0 | 114 | 8 | Metadata annotations, semi-additive/window metrics, queryable FACTS, wildcard selection, GET_DDL, security hardening |
| v0.7.0 | 78 | 7 | YAML definitions (inline + file + export), materialization routing, materialization introspection |

### Cumulative Quality

| Milestone | Total Tests | PBT Properties | Fuzz Targets | Integration Tests |
|-----------|------------|----------------|-------------|-------------------|
| v1.0 | ~30 | 4 properties (256 cases each) | 3 targets | 2 (SQLLogicTest + DuckLake) |
| v0.2.0 | 136 | 40 properties (256+ cases each) | 3 targets | 3 (SQLLogicTest + DuckLake CI + DuckLake local) |
| v0.3.0 | 136+ | 40 properties | 3 targets | 3 + vector_reference_test |
| v0.5.0 | 172 | 40 properties | 3 targets | 4 (SQLLogicTest + DuckLake CI + vector_reference + vtab_crash) |
| v0.5.1 | 222+ | 73 properties (40 output + 33 parser) | 3 targets | 5 (+ Python caret integration) |
| v0.5.2 | 282+ | 73+ properties | 4 targets | 7 sqllogictest + DuckLake CI + Python crash + caret |
| v0.5.3 | 441 | 80+ properties | 4 targets | 11 sqllogictest + DuckLake CI + Python crash + caret |
| v0.5.4 | 482 | 80+ properties | 4 targets | 18 sqllogictest + DuckLake CI + Python crash + caret + 22 infra assertions |
| v0.5.5 | 487 | 80+ properties | 4 targets | 19 sqllogictest + DuckLake CI + Python crash + caret + 22 infra assertions |
| v0.6.0 | 705 | 80+ properties | 4 targets | 32 sqllogictest + DuckLake CI + Python crash + caret + 22 infra assertions |
| v0.7.0 | 823 | 82+ properties | 6 targets | 36 sqllogictest + DuckLake CI + Python crash + caret |

### Top Lessons (Verified Across Milestones)

1. Design around DuckDB's execution lock constraint from the start — it affects every callback pattern
2. The bundled/extension feature split is the foundational pattern for testable DuckDB Rust extensions
3. Property-based tests catch bugs that unit tests miss — especially for type dispatch and SQL generation
4. Keep traceability/progress tables updated during execution, not at milestone close
5. Static linking against DuckDB amalgamation is the path for C++ features in Python-compatible extensions — `-fvisibility=hidden` blocks all dynamic approaches
