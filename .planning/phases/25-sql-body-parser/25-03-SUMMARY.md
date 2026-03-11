---
phase: 25-sql-body-parser
plan: 03
subsystem: database
tags: [rust, duckdb, parser, ddl, vtab, json, semantic-views]

# Dependency graph
requires:
  - phase: 25-02
    provides: parse_keyword_body function and KeywordBody struct from body_parser.rs
provides:
  - AS-body dispatch in validate_create_body routing to rewrite_ddl_keyword_body
  - rewrite_ddl_keyword_body function serializing KeywordBody to JSON for SQL rewrite
  - DefineFromJsonVTab VTab accepting (name VARCHAR, json VARCHAR) positional params
  - Three _from_json function registrations in init_extension
  - Full end-to-end pipeline from "CREATE SEMANTIC VIEW name AS ..." DDL to stored definition
affects: [25-04, 26-query-expansion, future-phases-using-AS-body-DDL]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "JSON-bridge pattern: AS-body DDL parsed in Rust, serialized to JSON, embedded in SELECT * FROM fn(name, json)"
    - "Dual-VTab registration: same DefineFromJsonVTab struct registered under 3 function names with different DefineState flags"
    - "Offset arithmetic for parse errors: computed via trimmed_no_semi slicing rather than string search"

key-files:
  created: []
  modified:
    - src/parse.rs
    - src/ddl/define.rs
    - src/lib.rs

key-decisions:
  - "rewrite_ddl_keyword_body constructs SemanticViewDefinition directly from KeywordBody fields rather than calling a hypothetical into_definition() method"
  - "DefineFromJsonVTab reuses DefineBindData, DefineInitData, and DefineState from existing code — no new types introduced"
  - "kind parameter added to validate_create_body signature to enable AS-body dispatch without global state"
  - "AS detection checks after_name_trimmed starts with AS + (end-of-string or whitespace) to avoid false match on view names starting with AS"

patterns-established:
  - "JSON-bridge: parse body -> SemanticViewDefinition -> serde_json::to_string -> embed in SELECT * FROM fn_from_json(name, json)"
  - "VTab multi-registration: single struct registered under multiple names with different extra_info (DefineState flags)"

requirements-completed: [DDL-01, DDL-07]

# Metrics
duration: 8min
completed: 2026-03-11
---

# Phase 25 Plan 03: Wire AS-body parser into DDL pipeline Summary

**AS-body CREATE SEMANTIC VIEW DDL fully wired: parse.rs dispatches to body_parser, serializes JSON, and DefineFromJsonVTab stores the definition via the same persist/catalog logic as the paren-body path**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-11T23:21:06Z
- **Completed:** 2026-03-11T23:28:33Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- `validate_create_body` now detects the `AS` keyword path and routes to `rewrite_ddl_keyword_body`, keeping the old paren-body path intact
- `rewrite_ddl_keyword_body` calls `parse_keyword_body`, constructs `SemanticViewDefinition` from `KeywordBody`, serializes to JSON, and returns `SELECT * FROM create_semantic_view_from_json('name', '...')`
- `DefineFromJsonVTab` added to `define.rs`: accepts `(name VARCHAR, json VARCHAR)`, deserializes JSON, runs DDL-time type inference, persists to catalog — reusing all existing infrastructure
- Three `_from_json` variants registered in `init_extension` using existing `DefineState` instances
- All 5 new `phase25_parse_tests` pass; full `cargo test` green; `just build` succeeds

## Task Commits

Each task was committed atomically:

1. **Task 1: AS dispatch in parse.rs validate_create_body** - `8700278` (feat)
2. **Task 2: DefineFromJsonVTab in define.rs and register in lib.rs** - `014fe60` (feat)

## Files Created/Modified
- `src/parse.rs` - Added `use crate::body_parser::parse_keyword_body`, `rewrite_ddl_keyword_body` function, AS detection in `validate_create_body`, `kind` param on `validate_create_body`, 5 new phase25_parse_tests
- `src/ddl/define.rs` - Added `DefineFromJsonVTab` struct with full VTab implementation
- `src/lib.rs` - Added `DefineFromJsonVTab` to import, registered 3 `_from_json` function variants

## Decisions Made
- Added `kind: DdlKind` parameter to `validate_create_body` so the AS-body path can select the correct `_from_json` function name without global state
- `rewrite_ddl_keyword_body` constructs `SemanticViewDefinition` directly from `KeywordBody` fields (no intermediate conversion method needed)
- `DefineFromJsonVTab` reuses all existing data types (`DefineBindData`, `DefineInitData`, `DefineState`) — no new types introduced
- AS detection uses `after_name_trimmed.get(..2).is_some_and(|s| s.eq_ignore_ascii_case("AS")) && (len==2 || next_byte.is_ascii_whitespace())` to avoid false matches

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Pre-commit hook reformatted code (rustfmt line-length adjustments); required `cargo fmt` before second commit attempt. Not a regression.

## Next Phase Readiness
- Full AS-body DDL pipeline is end-to-end connected: `CREATE SEMANTIC VIEW name AS TABLES (...) DIMENSIONS (...) METRICS (...)` now routes through body parser, serializes to JSON, and stores via the same persist/catalog infrastructure as the paren-body path
- Plan 04 (if any) can build on this for SQL logic tests or additional DDL forms

---
*Phase: 25-sql-body-parser*
*Completed: 2026-03-11*
