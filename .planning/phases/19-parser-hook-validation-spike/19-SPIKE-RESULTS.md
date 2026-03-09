# Phase 19: Parser Hook Validation Spike -- Results

**Tested:** 2026-03-09
**Extension version:** v0.5.0 (DuckDB v1.4.4 amalgamation)
**Test file:** `test/sql/phase19_parser_hook_validation.test`

## Empirical Results

| # | DDL Statement | Error Type | Error Message (excerpt) | Hook Triggered? |
|---|--------------|------------|------------------------|-----------------|
| 1 | `DROP SEMANTIC VIEW x` | Parser Error | `syntax error at or near "SEMANTIC"` | YES |
| 2 | `DROP SEMANTIC VIEW IF EXISTS x` | Parser Error | `syntax error at or near "SEMANTIC"` | YES |
| 3 | `CREATE OR REPLACE SEMANTIC VIEW x (...)` | Parser Error | `syntax error at or near "SEMANTIC"` | YES |
| 4 | `CREATE SEMANTIC VIEW IF NOT EXISTS x (...)` | Success (hook intercepted) | View created with name "IF" (prefix overlap) | YES |
| 5 | `DESCRIBE SEMANTIC VIEW x` | Parser Error | `syntax error at or near "VIEW"` | YES |
| 6 | `SHOW SEMANTIC VIEWS` | Parser Error | `syntax error at or near "VIEWS"` | YES |
| 7 | `CREATE SEMANTIC VIEW x (...)` | Success | (already working in v0.5.0) | YES (proven) |

**Result: All 7 DDL prefixes trigger the parser fallback hook.** Research predictions confirmed with zero deviations.

## Analysis

### Prefixes 1-2: DROP SEMANTIC VIEW / DROP SEMANTIC VIEW IF EXISTS

DuckDB's `DROP` grammar accepts a fixed set of object type keywords after `DROP` (TABLE, VIEW, SEQUENCE, FUNCTION, MACRO, INDEX, etc.). `SEMANTIC` is not in this list. The parser fails at `SEMANTIC` with `syntax error at or near "SEMANTIC"`, producing a Parser Error that triggers the fallback hook.

Both `DROP SEMANTIC VIEW` and `DROP SEMANTIC VIEW IF EXISTS` fail at the same point -- the grammar never reaches `IF EXISTS` because `SEMANTIC` is the first unexpected token.

### Prefix 3: CREATE OR REPLACE SEMANTIC VIEW

After `CREATE OR REPLACE`, the parser expects TABLE, VIEW, FUNCTION, MACRO, etc. `SEMANTIC` is not in this list. The parser fails at `SEMANTIC`, producing the same Parser Error pattern.

### Prefix 5: DESCRIBE SEMANTIC VIEW

The `variable_show.y` grammar rule for `DESCRIBE qualified_name` accepts `DESCRIBE` followed by a single qualified name. `SEMANTIC` is parsed as a qualified name (regular identifier). Then `VIEW` is an unexpected extra token that does not match any grammar continuation. The parser fails at `VIEW` with `syntax error at or near "VIEW"`.

Note: The error position is at `VIEW`, not `SEMANTIC`. This is because `DESCRIBE SEMANTIC` is a valid statement (it would describe a table named "semantic"), but `DESCRIBE SEMANTIC VIEW x` has leftover tokens.

### Prefix 6: SHOW SEMANTIC VIEWS

Same pattern as DESCRIBE. `SHOW SEMANTIC` parses `SEMANTIC` as a qualified name. `VIEWS` is an unexpected extra token. The parser fails at `VIEWS` with `syntax error at or near "VIEWS"`.

### Prefix 7: CREATE SEMANTIC VIEW (baseline)

Already proven working in v0.5.0. The `detect_create_semantic_view` function matches the "create semantic view" prefix, the hook returns `PARSE_SUCCESSFUL`, and the rewrite path converts it to a `create_semantic_view()` function call.

### Prefix Overlap: CREATE SEMANTIC VIEW IF NOT EXISTS

This is the most informative finding from the spike. The statement `CREATE SEMANTIC VIEW IF NOT EXISTS test_view (...)` contains the prefix "CREATE SEMANTIC VIEW" as a substring. The current `detect_create_semantic_view` function checks only the "create semantic view" prefix, so it matches and returns `PARSE_DETECTED`.

The hook is triggered, and the current `parse_ddl_text` function processes it as follows:

1. Strips the "create semantic view" prefix (20 chars)
2. Remaining text: `IF NOT EXISTS test_view (...)`
3. Extracts view name: first token = `IF`
4. Finds body: everything between first `(` and last `)` = the tables/dimensions/metrics block
5. Rewrites to: `SELECT * FROM create_semantic_view('IF', tables := [...], ...)`
6. Execution succeeds -- a view named "IF" is created

This is incorrect behavior (the view should be named "test_view" and the IF NOT EXISTS semantics should be applied), but it proves the critical fact: **the hook path is reachable for this prefix**. Phase 20 can fix this by checking longer prefixes first in the detection function.

**Verified empirically:** `list_semantic_views()` shows a view named "IF" after executing the statement.

## Scope Decision

**All 7 DDL prefixes CAN use native syntax via the parser fallback hook.**

No prefix produced a Catalog Error or Binder Error. Every prefix either:
- Produces a Parser Error (prefixes 1-3, 5-6), confirming the fallback hook path is reachable, OR
- Is already intercepted by the existing hook (prefixes 4 and 7), confirming the hook processes it

### v0.5.1 Native DDL Scope

| DDL Statement | v0.5.1 Approach | Requirement |
|--------------|-----------------|-------------|
| `CREATE SEMANTIC VIEW` | Native DDL (existing, v0.5.0) | -- |
| `CREATE OR REPLACE SEMANTIC VIEW` | Native DDL | DDL-05 |
| `CREATE SEMANTIC VIEW IF NOT EXISTS` | Native DDL | DDL-06 |
| `DROP SEMANTIC VIEW` | Native DDL | DDL-03 |
| `DROP SEMANTIC VIEW IF EXISTS` | Native DDL | DDL-04 |
| `DESCRIBE SEMANTIC VIEW` | Native DDL | DDL-07 |
| `SHOW SEMANTIC VIEWS` | Native DDL | DDL-08 |

### Implementation Notes for Phase 20

1. **Detection function must check longer prefixes first.** The prefix ordering must be:
   - `CREATE OR REPLACE SEMANTIC VIEW` (before `CREATE SEMANTIC VIEW`)
   - `CREATE SEMANTIC VIEW IF NOT EXISTS` (before `CREATE SEMANTIC VIEW`)
   - `CREATE SEMANTIC VIEW`
   - `DROP SEMANTIC VIEW IF EXISTS` (before `DROP SEMANTIC VIEW`)
   - `DROP SEMANTIC VIEW`
   - `DESCRIBE SEMANTIC VIEW`
   - `SHOW SEMANTIC VIEWS`

   The current "create semantic view" prefix check would incorrectly match both `CREATE OR REPLACE SEMANTIC VIEW` and `CREATE SEMANTIC VIEW IF NOT EXISTS` if checked first. Checking longer prefixes first eliminates the overlap.

2. **Error position differs by prefix type.** DROP and CREATE OR REPLACE errors occur at "SEMANTIC", while DESCRIBE and SHOW errors occur at "VIEW"/"VIEWS". This does not affect implementation but is worth knowing for error reporting in Phase 21.

3. **DESCRIBE SEMANTIC and SHOW SEMANTIC are valid DuckDB statements** (they describe/show a table named "semantic"). The detection function must require the full multi-word prefix (`DESCRIBE SEMANTIC VIEW`, `SHOW SEMANTIC VIEWS`) to avoid intercepting valid DuckDB statements.

4. **All target functions already exist** and are registered at extension init time (`create_or_replace_semantic_view`, `create_semantic_view_if_not_exists`, `drop_semantic_view`, `drop_semantic_view_if_exists`, `describe_semantic_view`, `list_semantic_views`). Phase 20 only needs to extend detection and rewrite logic.

5. **Three-connection lock conflict (P3 blocker)** should be tested early in Phase 20 for the DROP path, where `sv_ddl_conn` executes the rewritten SQL which internally calls `drop_semantic_view` -> `persist_conn` for catalog table delete. The connections are used sequentially (not concurrently) by the rewrite pattern, but empirical confirmation is prudent.

## Conclusion

Research predictions from 19-RESEARCH.md are **fully confirmed**. All 7 DDL prefixes produce Parser Errors (or are already intercepted by the hook), meaning the parser fallback hook path is reachable for every planned native DDL statement.

Phase 20 can proceed with full native DDL coverage for all 6 new requirements (DDL-03 through DDL-08). No statements need to fall back to function-only interfaces.

The only implementation subtlety is prefix ordering in the detection function: longer prefixes must be checked before shorter ones to avoid the overlap demonstrated by prefix 4 (`CREATE SEMANTIC VIEW IF NOT EXISTS` matching the shorter `CREATE SEMANTIC VIEW` prefix).
