# Project Research Summary

**Project:** DuckDB Semantic Views v0.7.0 — YAML Definitions & Materialization Routing
**Domain:** DuckDB Rust extension — YAML definition format and materialization routing engine
**Researched:** 2026-04-18
**Confidence:** HIGH

## Executive Summary

v0.7.0 adds two architecturally independent feature tracks to a mature extension (705 tests, 25,983 LOC). Track A is a YAML definition format: users can write `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$` or `FROM YAML FILE '/path/to/file.yaml'` as an alternative to SQL keyword DDL. Track B is a materialization routing engine: a new `MATERIALIZATIONS` clause declares pre-existing aggregated tables, and at query time the extension transparently routes `semantic_view()` calls to those tables instead of expanding raw sources. Both tracks converge on the existing `SemanticViewDefinition` model struct as their common interface — YAML parsing produces the same struct SQL DDL produces, and materialization routing consumes an extended version of it.

The recommended approach is to implement YAML as a parse-time transformation: YAML input is parsed in Rust, converted to JSON, and fed into the existing `_from_json` table function pipeline. This avoids new table function registrations and keeps the DDL execution path identical. The single new dependency is `serde_yaml_ng 0.10`, a maintained fork of dtolnay's original serde_yaml. The obvious alternatives have critical problems: `serde_yaml` is archived, `serde_yml` has a live RUSTSEC-2025-0068 soundness advisory. Materialization routing requires only std library additions — a new `materialize.rs` module using `HashSet::is_subset` checks, inserted as a pre-check before the existing expansion pipeline.

The dominant risk across both tracks is correctness of the materialization routing engine. Re-aggregating non-additive metrics (AVG, COUNT DISTINCT), semi-additive metrics (NON ADDITIVE BY), and window function metrics through a GROUP BY wrapper over a pre-aggregated table produces silently wrong results — the worst possible failure mode for a semantic layer. The mitigation is conservative routing: classify metrics for additivity at define time and refuse to route to a materialization when any requested metric is non-additive and re-aggregation would be required. A secondary risk is model drift between the SQL DDL and YAML parsing paths as future features are added; a shared post-parse validation function and feature-parity test pairs are the prevention.

---

## Key Findings

### Recommended Stack

The only new dependency is `serde_yaml_ng = "0.10"`. Because `SemanticViewDefinition` and all nested structs already derive `serde::Serialize` and `serde::Deserialize`, YAML parsing is largely free once the dependency is added. All remaining features — dollar-quote detection, file I/O, materialization routing, re-aggregation SQL generation — use only the existing Rust standard library.

**Core technologies:**
- `serde_yaml_ng 0.10`: YAML deserialization/serialization — only maintained serde-compatible fork without security advisories; MIT license already allowed in `deny.toml`
- DuckDB `read_text()` via `catalog_conn`: YAML FILE loading — must use DuckDB's file abstraction (not `std::fs`) to respect `enable_external_access` security controls
- `std::collections::HashSet` (existing): Materialization routing — set-containment checks are the entire routing algorithm; no external query planner needed

**Critical rejections:** `serde_yaml` (archived March 2024), `serde_yml` (RUSTSEC-2025-0068, archived), `serde-saphyr` (pre-1.0 API, viable for future milestones).

### Expected Features

**Table stakes (must have):**
- T1: YAML inline parsing (`FROM YAML $$ ... $$`) — foundation for all YAML features
- T2: YAML file loading (`FROM YAML FILE '/path'`) — DX advantage; trivial once T1 exists
- T3: YAML round-trip export (`GET_DDL('SEMANTIC_VIEW', 'name', 'YAML')`) — version control workflow
- T4: Materialization declaration (`MATERIALIZATIONS` clause) — data model for routing
- T5: Query-time materialization routing — core value; without this T4 is dead metadata
- T6: Re-aggregation for subset matches — makes one wide materialization serve many queries; requires additivity gating

**Differentiators:**
- D1: Additivity metadata on `Metric` struct — stored at define time, makes T5/T6 robust; surfaces in SHOW output
- D2: `EXPLAIN MATERIALIZATION FOR SEMANTIC VIEW` — routing transparency; users cannot debug without it
- D3: Materializations section in YAML schema — completes YAML feature parity with SQL DDL

**Defer (v0.8+):** Automatic refresh, freshness tracking, AVG decomposition, granularity-based time matching, `filters` clause, cross-view materialization sharing.

### Architecture Approach

YAML and Materialization are independent tracks meeting only at `model.rs`. The recommended YAML-to-JSON-at-parse-time approach eliminates new table function registrations — the rewritten SQL is identical to the SQL DDL path, so the execution engine sees no difference.

**New components:**
1. `yaml_parser.rs` — YAML string → `SemanticViewDefinition` via `YamlDef` intermediate structs; converts to JSON for existing `_from_json` pipeline
2. `materialize.rs` — `route_query()` with set-containment matching; returns `RouteResult::Materialized(sql)` or `RouteResult::Fallback`
3. `render_yaml.rs` — serde-based YAML serialization of `SemanticViewDefinition`; used by extended `GET_DDL`

**Modified components:** `parse.rs` (FROM YAML detection + dollar-quote extraction), `model.rs` (Materialization struct + field), `body_parser.rs` (MATERIALIZATIONS clause), `query/table_function.rs` (routing pre-check before expand), `ddl/get_ddl.rs` (YAML format parameter).

**Key architectural constraint:** File I/O for `FROM YAML FILE` must happen at bind time via `read_text()` in the rewritten SQL, not at parse time. The parser hook context does not have access to the execution engine.

### Critical Pitfalls

1. **Re-aggregation of non-additive metrics produces silently wrong results** — AVG, COUNT DISTINCT, PERCENTILE cannot be re-aggregated with GROUP BY. Prevention: classify additivity at define time (parse outermost aggregate function); refuse subset-dimension routing for non-additive metrics; default Unknown → NonAdditive.

2. **Semi-additive and window metrics bypass correctness** — NON ADDITIVE BY requires CTE-based ROW_NUMBER snapshot selection; window metrics require inner-aggregation + outer-window pipelines. A pre-aggregated materialization has already collapsed raw rows. Prevention: if any requested metric is semi-additive or windowed, always fall back to raw expansion.

3. **serde_yaml ecosystem fragmentation** — `serde_yaml` archived, `serde_yml` has live RUSTSEC soundness advisory (segfaults in serializer). Prevention: use `serde_yaml_ng 0.10` specifically; add `fuzz_yaml_parse` target; document in TECH-DEBT.md.

4. **YAML anchor/alias bombs** — 1KB YAML with nested anchors expands to gigabytes, crashing DuckDB process. Prevention: 1MB input size cap before parsing; post-parse cardinality validation; anchor bomb patterns in fuzz target.

5. **Dual-format model drift** — SQL DDL and YAML paths diverge as future features are added to one but not the other. Prevention: extract shared post-parse validation function; create feature-parity test pairs; use serde-based YAML renderer (not hand-rolled templates).

6. **File I/O security bypass** — `std::fs::read_to_string` circumvents DuckDB's `enable_external_access`. Prevention: use `read_text()` via `catalog_conn`; test failure when `enable_external_access=false`.

---

## Suggested Build Order (8 Phases)

1. **YAML Parser Core** — `serde_yaml_ng` dependency, `YamlDef` structs, YAML → `SemanticViewDefinition` conversion, shared post-parse validation extraction. Additivity enum on Metric established here (needed by Phase 5).
2. **Dollar-Quoting and DDL Integration** — `FROM YAML` detection in `validate_create_body()`, dollar-quote extraction, YAML-to-JSON-at-parse-time rewrite. End-to-end `CREATE ... FROM YAML $$...$$`.
3. **YAML File Loading** — `FROM YAML FILE '/path'` syntax, `read_text()` subquery rewrite, file security boundary.
4. **Materialization Model and DDL** — `Materialization` struct, `MATERIALIZATIONS` clause in `body_parser.rs`, define-time validation, backward-compat tests.
5. **Materialization Routing Engine (Exact Match)** — `materialize.rs` with `route_query()`, integration in `table_function.rs`, exclusion rules for semi-additive/window/USING metrics.
6. **Re-Aggregation for Subset Matches** — Subset-dimension routing with GROUP BY, aggregate function mapping (SUM→SUM, COUNT→SUM, MIN→MIN, MAX→MAX), additivity gating.
7. **YAML Export and YAML+MATERIALIZATIONS** — `render_yaml.rs`, `GET_DDL('SEMANTIC_VIEW', 'name', 'YAML')` format parameter, materializations in YAML schema, round-trip tests.
8. **Introspection and Diagnostics** — `EXPLAIN MATERIALIZATION FOR SEMANTIC VIEW`, materialization entries in DESCRIBE, optional `SHOW SEMANTIC MATERIALIZATIONS`.

### Phase Ordering Rationale
- Tracks A (YAML, Phases 1-3) and B (Materialization, Phases 4-6) are fully independent and can be interleaved
- D1 (additivity metadata) placed in Phase 1 rather than polish because Phase 5 routing correctness depends on it
- Exact-match routing (Phase 5) separated from re-aggregation (Phase 6) to provide a safe shippable increment
- YAML export (Phase 7) placed after Phase 4 so materializations are included in the round-trip format

### Research Flags

**No additional research needed:** Phases 1-4, 7, 8 use standard patterns with direct codebase precedent.

**Elevated testing (correctness risk):** Phase 5 (routing exclusion rules — negative test cases mandatory), Phase 6 (aggregate detection from SQL strings — proptest mandatory; conservative Unknown→NonAdditive default).

---

## Open Questions

- **`serde_yaml_ng` anchor bomb handling:** Verify whether the crate limits anchor expansion or if we need manual protection.
- **Dollar-quote behavior in parser hook:** DuckDB supports `$$` at SQL level, but our parser hook fires before DuckDB's parser. Needs integration test.
- **`catalog_conn` availability for file I/O:** Verify this connection is accessible in the file-loading code path.
- **Materialization table existence validation:** Define-time vs query-time validation of materialization table existence.
- **Additivity for complex expressions:** `SUM(CASE WHEN ... THEN amount END)` is additive but not trivially detectable. May need conservative heuristic.

---

*Synthesized from: STACK.md, FEATURES.md, ARCHITECTURE.md, PITFALLS.md*
*All 4 research agents completed with HIGH confidence*
