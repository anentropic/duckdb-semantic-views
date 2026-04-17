# Project Research Summary

**Project:** DuckDB Semantic Views v0.6.0 — Snowflake SQL DDL Parity
**Domain:** DuckDB Rust extension — close all remaining feature gaps against Snowflake SQL DDL semantic views
**Researched:** 2026-04-09
**Confidence:** HIGH

## Executive Summary

v0.6.0 closes the remaining feature gaps between this extension and Snowflake's SQL DDL semantic views. The codebase (16,342 LOC, 487 tests) is mature with established patterns for every category of work: serde-driven model extensions, hand-written state machine parsing, VTab-per-DDL-verb DDL handling, and a single-pass SQL expansion engine. All seven target feature groups map cleanly onto existing extension points — no new Rust crates are required, and architectural boundaries remain unchanged.

The features fall into three tiers: Tier 1 (metadata, GET_DDL, SHOW enhancements) touches only the model layer and DDL VTabs with zero expansion pipeline impact. Tier 2 (wildcard selection, queryable FACTS) adds new resolution modes to the expansion pipeline without restructuring SQL shape. Tier 3 (semi-additive metrics, window function metrics) requires fundamentally different SQL generation. Build order must follow tier order so model changes stabilize before expansion changes begin.

The primary technical risk is semi-additive metric expansion (NON ADDITIVE BY). The existing single-pass expansion generates exactly one query shape; semi-additive requires a CTE-based two-stage approach using `ROW_NUMBER()` (not `LAST_VALUE IGNORE NULLS`, which has a DuckDB all-NULL crash bug on LTS 1.4.x). Window function metrics (PARTITION BY EXCLUDING) are architecturally orthogonal — research recommends parsing and storing the model now, with query-time expansion returning an error if queried (full expansion deferred or implemented last).

---

## Key Findings

### Recommended Stack

**Zero new crates required.** Every v0.6.0 feature builds on the existing dependency set (duckdb =1.10500.0, serde_json 1.x, strsim 0.11). No template engines, regex, date libraries, or parsing libraries needed.

DuckDB natively supports all required SQL constructs for semi-additive and window function metrics: `LAST_VALUE(expr IGNORE NULLS) OVER (PARTITION BY ... ORDER BY ...)`, window aggregates without GROUP BY, and CTE wrapping. Semi-additive should use `ROW_NUMBER()` for the snapshot selection CTE (safer than `LAST_VALUE` on DuckDB LTS).

### Expected Features

**Table stakes (must have for Snowflake parity):**
1. COMMENT on views and all objects (tables, dimensions, metrics, facts)
2. SYNONYMS on all objects (informational only — do NOT affect query resolution)
3. PRIVATE/PUBLIC access modifiers on facts and metrics (PRIVATE cannot be queried but CAN be referenced by derived metrics)
4. Semi-additive metrics (NON ADDITIVE BY) — snapshot aggregation via `ROW_NUMBER()` CTE pre-filter
5. Queryable FACTS — row-level query mode; mutually exclusive with METRICS in same query
6. ALTER SET/UNSET COMMENT
7. SHOW enhancements: synonyms/comment columns, IN SCHEMA/DATABASE scope, TERSE mode, SHOW COLUMNS

**Differentiators:**
1. GET_DDL reconstruction — round-trip DDL from stored JSON; validates model fidelity
2. Wildcard selection (`table_alias.*`) — query-time convenience
3. Window function metrics (PARTITION BY EXCLUDING) — DDL model + parse now, expansion last
4. Fan trap detection remains a DuckDB advantage over Snowflake (which silently produces wrong results)

**Anti-features (do not implement):**
- Direct SQL query interface (`SELECT AGG(metric) FROM sv GROUP BY dim`) — fundamentally different architecture
- SEMANTIC_VIEW() clause syntax — requires parser-level integration beyond current hook model
- Cortex AI integration (AI_SQL_GENERATION, AI_QUESTION_CATEGORIZATION) — Snowflake-specific
- COPY GRANTS — no DuckDB RBAC

### Architecture Approach

Seven feature groups fall into three integration tiers:

**Tier 1 — Model + DDL only (no expansion impact):**
- Metadata fields (COMMENT, SYNONYMS, PRIVATE/PUBLIC) on model structs with `#[serde(default)]`
- Body parser extensions for new annotation suffixes
- SHOW/DESCRIBE output column additions
- ALTER SET/UNSET COMMENT (new DdlKind variant)
- GET_DDL (table function reading stored JSON, reconstructing DDL text)
- SHOW enhancements (TERSE, IN scope, SHOW COLUMNS)

**Tier 2 — Expansion modifications (no SQL shape change):**
- Wildcard selection: resolve `alias.*` to matching dimension/metric names before existing resolution loop
- Queryable FACTS: separate expansion mode without GROUP BY, no aggregation

**Tier 3 — Expansion structural changes (different SQL shape):**
- Semi-additive metrics: CTE-based two-stage expansion (ROW_NUMBER snapshot selection → GROUP BY aggregation)
- Window function metrics: expansion without GROUP BY, PARTITION BY EXCLUDING

### Critical Pitfalls

1. **JSON backward compatibility (CRITICAL):** Adding 5+ new fields across Metric, Dimension, Fact, and SemanticViewDefinition. A single missing `#[serde(default)]` renders the entire catalog inaccessible. Batch all model changes in Phase 1 with a v0.5.5 JSON deserialization test as gate.
2. **Semi-additive expansion scope (CRITICAL):** CTE composable wrapper, not inline branching. Use separate CTE per semi-additive metric for correctness over optimization.
3. **DuckDB LAST_VALUE IGNORE NULLS crash (CRITICAL):** Use `ROW_NUMBER()` instead on LTS 1.4.x branch.
4. **Window metrics + GROUP BY incompatible (CRITICAL):** Detect and error at expand time — cannot coexist with aggregate metrics.
5. **GET_DDL round-trip quoting (MODERATE):** Expressions survive as opaque SQL strings, but structural identifiers must be re-quoted. Validate with round-trip proptest.
6. **SHOW IN SCHEMA/DATABASE model mismatch (MODERATE):** DuckDB has no native `IN SCHEMA` for extension SHOW. Filter on stored metadata fields (database_name, schema_name from v0.5.5).
7. **Parser annotation ambiguity (MODERATE):** Grammar for COMMENT/SYNONYMS after expressions needs design decision: right-to-left keyword scan vs fixed-order grammar.

---

## Suggested Build Order (6 Phases)

1. **Metadata Foundation** — Model struct fields + body_parser.rs extensions + backward-compat JSON test. Prerequisite for everything else.
2. **SHOW/DESCRIBE Metadata Surface + SHOW Enhancements** — Surface metadata columns, TERSE mode, IN SCHEMA/DATABASE, SHOW COLUMNS.
3. **ALTER SET/UNSET COMMENT + GET_DDL** — DDL-only changes; GET_DDL validates round-trip fidelity.
4. **Wildcard Selection + Queryable FACTS** — Tier 2 expansion modifications; familiarizes with expansion pipeline before Tier 3.
5. **Semi-Additive Metrics (NON ADDITIVE BY)** — Highest-complexity expansion change; CTE-based snapshot aggregation.
6. **Window Function Metrics** — Parse and store PARTITION BY EXCLUDING in model; implement expansion or return error if queried.

---

## Open Questions

- **Mixed regular + semi-additive metrics:** When a query requests both, should expansion generate a CTE-based split query or forbid mixing? Snowflake allows mixing. Design decision needed in Phase 5.
- **Semi-additive + fan trap interaction:** Should fan trap check skip semi-additive metrics entirely, or produce a warning?
- **NON ADDITIVE BY sort order:** Snowflake syntax says ASC but "last row" semantics implies DESC. Needs exact verification.
- **GET_DDL registration:** Scalar function via `create_scalar_function` needs verification; VTab (table function) is proven fallback.
- **Window metric SHOW COLUMNS `required` column:** Exact behavior needs verification against Snowflake.
- **PARTITION BY EXCLUDING grammar:** Body parser design for window function metrics needs careful design.

---

*Synthesized from: STACK.md, FEATURES.md, ARCHITECTURE.md, PITFALLS.md*
*All 4 research agents completed with HIGH confidence*
