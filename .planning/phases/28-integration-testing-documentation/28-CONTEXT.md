# Phase 28: Integration Testing & Documentation - Context

**Gathered:** 2026-03-13
**Status:** Ready for planning

<domain>
## Phase Boundary

End-to-end validation of the complete DDL-to-query pipeline using the new AS-body PK/FK syntax, README rewrite to show only the new syntax, and retirement of the function-based CREATE DDL interface (`create_semantic_view()`, `create_or_replace_semantic_view()`, `create_semantic_view_if_not_exists()`).

Phase 24 (PK/FK Model) is cancelled -- its model struct work was already completed in Phase 25-01. Requirements DDL-06 and MDL-01 through MDL-05 are closed as won't-do since the function DDL interface they targeted is being retired.

</domain>

<decisions>
## Implementation Decisions

### Function DDL removal
- Retire the 3 CREATE function DDL variants: `create_semantic_view()`, `create_or_replace_semantic_view()`, `create_semantic_view_if_not_exists()`
- Remove `DefineSemanticViewVTab`, `parse_args.rs`, and the 3 function registrations in `lib.rs`
- Keep all non-CREATE functions: `explain_semantic_view()`, `semantic_view()`, `drop_semantic_view()`, `drop_semantic_view_if_exists()`, `list_semantic_views()`, `describe_semantic_view()`
- Keep `explain_semantic_view()` as a table function -- it reveals the expanded SQL and DuckDB execution plan, which native `EXPLAIN` on `semantic_view()` cannot (DuckDB sees the table function as a black box)
- The `_from_json` VTab variants stay -- they are the backend for native DDL rewriting
- Cancel Phase 24 entirely -- mark as cancelled/superseded, close DDL-06 and MDL-01 through MDL-05 as won't-do

### Existing test file handling
- Rewrite test files that exercise unique scenarios (restart persistence, error reporting, etc.) to use AS-body syntax
- Delete test files that overlap with newer phase test files (phases 25-27 already cover AS-body DDL and PK/FK joins thoroughly)

### README restructuring
- Clean slate -- show only AS-body PK/FK syntax, no mention of function DDL
- Streamlined structure: How it works, Quick start (single table), Multi-table (PK/FK relationships), DDL reference, Building
- Drop Explain and Lifecycle as separate sections (fold into examples if useful)
- Update version line to "v0.5.2 -- early-stage, not yet on the community registry"
- Use orders/customers/products e-commerce domain for examples

### E2E test design
- Full result verification with known inserted data -- assert exact result rows, not just execution success
- 3+ table PK/FK semantic view scenario (orders/customers/products domain)
- Test both `semantic_view()` queries and `explain_semantic_view()` output (verify expanded SQL contains expected FROM/JOIN/GROUP BY clauses)
- Test organization at Claude's discretion (single file vs split by concern)

### Claude's Discretion
- Test file organization (single comprehensive vs split by concern)
- Which existing test files are "valuable" vs redundant
- README section ordering and exact content structure
- Whether to keep the "Function syntax" note in README or omit entirely

</decisions>

<specifics>
## Specific Ideas

- `EXPLAIN SELECT * FROM semantic_view(...)` returns only a black-box `SEMANTIC_VIEW ~1 row` node -- this is why `explain_semantic_view()` must stay (it shows the actual expanded SQL + physical plan for that SQL)
- The native DDL path (`CREATE SEMANTIC VIEW ... AS ...`) uses `DefineFromJsonVTab`, which is completely independent of the function DDL path (`DefineSemanticViewVTab`) -- they share only the persistence layer (`persist_define()` + `catalog_insert/upsert()`)

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/ddl/define.rs:DefineFromJsonVTab` -- the VTab that native DDL uses; stays as-is
- `src/ddl/define.rs:persist_define()` + `catalog_insert/upsert()` -- shared persistence; stays as-is
- `src/query/explain.rs:ExplainSemanticViewVTab` -- explain function; stays as-is
- `src/query/table_function.rs:SemanticViewVTab` -- query function; stays as-is
- `test/sql/phase25_keyword_body.test` -- already uses AS-body syntax; good reference for new tests
- `test/sql/phase26_join_resolution.test` -- already tests PK/FK joins; good reference

### Files to remove
- `src/ddl/parse_args.rs` -- function DDL argument parser (used only by DefineSemanticViewVTab)
- `DefineSemanticViewVTab` in `src/ddl/define.rs` -- the function DDL VTab implementation
- 3 function registrations in `src/lib.rs` (create_semantic_view, create_or_replace, create_if_not_exists)

### Files to evaluate for rewrite/delete
- `test/sql/phase2_ddl.test` -- uses function DDL syntax
- `test/sql/phase2_restart.test` -- tests persistence across restart; unique scenario, worth rewriting
- `test/sql/phase4_query.test` -- tests query functionality; may overlap with phase26/27 tests
- `test/sql/semantic_views.test` -- original integration test; likely redundant

### Integration Points
- `src/lib.rs:init_extension()` -- function registration site; remove 3 CREATE function registrations
- `src/ddl/mod.rs` -- module declarations; remove parse_args module
- `README.md` -- full rewrite

</code_context>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope.

</deferred>

---

*Phase: 28-integration-testing-documentation*
*Context gathered: 2026-03-13*
