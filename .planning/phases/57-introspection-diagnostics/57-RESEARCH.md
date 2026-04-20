# Phase 57: Introspection & Diagnostics - Research

**Researched:** 2026-04-20
**Domain:** DuckDB extension introspection commands (Rust VTab, parser hooks)
**Confidence:** HIGH

## Summary

Phase 57 adds materialization awareness to three existing introspection surfaces: `explain_semantic_view()`, `DESCRIBE SEMANTIC VIEW`, and a new `SHOW SEMANTIC MATERIALIZATIONS` command. All three requirements are well-constrained by the established codebase patterns -- the project has five existing SHOW commands (`VIEWS`, `DIMENSIONS`, `METRICS`, `FACTS`, `COLUMNS`), a property-per-row DESCRIBE format, and a fully working EXPLAIN function.

The implementation touches four modules: `parse.rs` (new `DdlKind::ShowMaterializations` variant + detection + rewrite), `ddl/describe.rs` (new `collect_materialization_rows` function), a new `ddl/show_materializations.rs` (VTab pair: single-view + cross-view), and `query/explain.rs` (add materialization routing info to output header). The materialization model (`model::Materialization`) already has all needed fields (name, table, dimensions, metrics).

**Primary recommendation:** Follow the exact patterns from Phase 34.1 (SHOW DIMENSIONS/METRICS/FACTS) and Phase 41 (DESCRIBE rewrite). The new SHOW command is a direct structural clone of `show_dims.rs`. The DESCRIBE addition is a new `collect_materialization_rows` function appended after `collect_metric_rows`. The EXPLAIN change requires calling `try_route_materialization` separately in the explain bind path and adding a `-- Materialization:` header line.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| INTR-01 | `explain_semantic_view()` output includes materialization routing decision (materialization name or "none") and expanded SQL reflects the routed table | Explain VTab at `src/query/explain.rs` already calls `expand()` which performs routing; need to also call `try_route_materialization` pre-expand to get the name, and add a header line |
| INTR-02 | `DESCRIBE SEMANTIC VIEW` includes materialization entries | DESCRIBE at `src/ddl/describe.rs` uses property-per-row format with collect_*_rows helpers; add `collect_materialization_rows` following same pattern |
| INTR-03 | `SHOW SEMANTIC MATERIALIZATIONS IN view_name` lists all declared materializations with covered dimensions and metrics | New SHOW command following `show_dims.rs` pattern -- DdlKind variant, parser detection, VTab pair (single-view + cross-view), registration in lib.rs |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Quality gate**: `just test-all` (cargo test + sqllogictest + DuckLake CI) must pass
- **sqllogictest coverage required**: `cargo test` alone is incomplete
- **Build**: `just build` for extension binary; `just test-sql` requires fresh build
- **Reference**: Snowflake semantic views for SQL syntax/behavior guidance

## Architecture Patterns

### Recommended Project Structure Changes

```
src/
  parse.rs             # Add DdlKind::ShowMaterializations, detection, rewrite
  ddl/
    mod.rs             # Add pub mod show_materializations
    describe.rs        # Add collect_materialization_rows()
    show_materializations.rs  # NEW: ShowMatVTab + ShowMatAllVTab
  query/
    explain.rs         # Add materialization routing info to header
  lib.rs               # Register new VTabs + import
test/sql/
  phase57_introspection.test  # NEW: sqllogictest integration tests
  TEST_LIST            # Add phase57 entry
```

### Pattern 1: Adding a new SHOW SEMANTIC command (INTR-03)

**What:** The project has an established 7-step pattern for adding SHOW commands, validated by 5 existing implementations (VIEWS, TERSE VIEWS, DIMENSIONS, METRICS, FACTS). [VERIFIED: codebase grep]

**Steps:**
1. Add `DdlKind::ShowMaterializations` variant to `DdlKind` enum (parse.rs:27-42)
2. Add `match_keyword_prefix` detection in `detect_ddl_prefix` (parse.rs:97-162) -- must come BEFORE `show semantic views` to prevent prefix overlap
3. Add function name mapping in `function_name` (parse.rs:220-235)
4. Add rewrite handling in `rewrite_ddl` match arms (parse.rs:580-650)
5. Add validation handling in `validate_and_rewrite` (parse.rs:812+)
6. Add near-miss detection entry in `DDL_PREFIXES` (parse.rs:739-755)
7. Add name extraction in `extract_ddl_name` (parse.rs:661+)
8. Create `ddl/show_materializations.rs` with two VTab structs
9. Register VTabs in `lib.rs` extension module

**Example -- Row struct (from `show_dims.rs` pattern):**
```rust
// [VERIFIED: src/ddl/show_dims.rs line 16-25]
struct ShowMatRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    name: String,           // materialization name
    table: String,          // materialization table reference
    dimensions: String,     // JSON array of covered dimension names
    metrics: String,        // JSON array of covered metric names
}
```

**Column schema:** 7 VARCHAR columns: `database_name`, `schema_name`, `semantic_view_name`, `name`, `table`, `dimensions`, `metrics`. This follows the Snowflake-aligned pattern used by existing SHOW commands but replaces `table_name`/`data_type` with materialization-specific columns.

### Pattern 2: Adding DESCRIBE rows for materializations (INTR-02)

**What:** DESCRIBE uses a property-per-row format with 5 columns: `(object_kind, object_name, parent_entity, property, property_value)`. Each object type has a `collect_*_rows` helper that appends to the rows vec. [VERIFIED: src/ddl/describe.rs]

**Example -- Materialization rows:**
```rust
// Follow collect_table_rows / collect_relationship_rows pattern
fn collect_materialization_rows(def: &SemanticViewDefinition, rows: &mut Vec<DescribeRow>) {
    for mat in &def.materializations {
        rows.push(DescribeRow {
            object_kind: "MATERIALIZATION".to_string(),
            object_name: mat.name.clone(),
            parent_entity: String::new(),
            property: "TABLE".to_string(),
            property_value: mat.table.clone(),
        });
        rows.push(DescribeRow {
            object_kind: "MATERIALIZATION".to_string(),
            object_name: mat.name.clone(),
            parent_entity: String::new(),
            property: "DIMENSIONS".to_string(),
            property_value: format_json_array(&mat.dimensions),
        });
        rows.push(DescribeRow {
            object_kind: "MATERIALIZATION".to_string(),
            object_name: mat.name.clone(),
            parent_entity: String::new(),
            property: "METRICS".to_string(),
            property_value: format_json_array(&mat.metrics),
        });
    }
}
```

**Call site:** In `DescribeSemanticViewVTab::bind()` at `describe.rs:523-528`, add call after `collect_metric_rows`.

### Pattern 3: Adding materialization info to EXPLAIN (INTR-01)

**What:** `explain_semantic_view()` builds a three-part output: header (view name, dims, metrics), expanded SQL, DuckDB plan. The routing info needs to appear in the header section. [VERIFIED: src/query/explain.rs:190-211]

**Current expand() return:** `expand()` returns `Result<String, ExpandError>` -- it performs routing internally but does not expose which materialization was selected. [VERIFIED: src/expand/sql_gen.rs:267]

**Approach:** In the explain bind path, call `try_route_materialization` independently (it's a pure function with no side effects) before calling `expand()`. This tells us the materialization name. `expand()` will independently perform the same routing and return the correct SQL. This avoids changing `expand()`'s return type, which would cascade changes across the codebase. [VERIFIED: src/expand/materialization.rs -- try_route_materialization is pub(crate) and pure]

**Implementation detail:** The explain VTab does NOT currently have access to `try_route_materialization` because it's in `expand::materialization` (pub(crate)). Since `explain.rs` is in `query/` which is within the same crate, it has access via `crate::expand::materialization::try_route_materialization`. However, `try_route_materialization` takes `&[&Dimension]` and `&[&Metric]` references which require resolving the dimensions/metrics -- the explain bind path already does this via `expand()`. We need to resolve dims/mets before calling try_route to get the materialization name.

**Solution:** Re-use the name resolution done in explain bind (or resolve dims/mets separately), then call `try_route_materialization`. The cleanest approach: resolve dims/mets, call `try_route_materialization` for the name, call `expand()` for the SQL. The resolve step duplicates some work that expand() also does, but it's trivial (name lookups) and keeps the API clean.

Actually, looking more carefully at the code: the explain bind path calls `expand()` directly (line 190-191). The expand function resolves dims/mets internally. To avoid duplicating resolution, the simplest approach is:

1. Add a new `pub(crate)` function in `expand/materialization.rs` or `expand/sql_gen.rs` that takes a `SemanticViewDefinition` and `QueryRequest` and returns `Option<String>` (the materialization name, not SQL).
2. Call this in explain bind before calling expand().
3. Add `-- Materialization: <name or none>` to the header.

Alternatively, just scan `def.materializations` inline to find the match (it's simple HashSet comparison). But better to use the existing `try_route_materialization` to avoid logic duplication.

**Cleanest approach:** Add a `find_routing_materialization_name` function that returns `Option<&str>` (just the name) using the same logic as `try_route_materialization` but returning the name instead of SQL. This avoids building SQL we don't need.

### Anti-Patterns to Avoid
- **Changing `expand()` return type:** Would cascade changes across semantic_view VTab, explain VTab, and all test code. Not worth it for a diagnostic line.
- **Duplicating materialization matching logic:** Always delegate to a shared function. Don't inline HashSet matching in explain.rs.
- **Forgetting the "all" variant:** Every SHOW command has a single-view form (`IN view_name`) and a cross-view form (no arguments). Both are needed.
- **Forgetting filter clause support:** SHOW SEMANTIC MATERIALIZATIONS should support LIKE/IN/STARTS WITH/LIMIT like other SHOW commands.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON array formatting | String concatenation | `format_json_array()` from `describe.rs` | Already handles quoting, commas, empty arrays |
| VTab boilerplate | Custom output emission | Clone `show_dims.rs` pattern | 5 existing implementations validate the pattern |
| Materialization matching | Inline HashSet logic | `try_route_materialization` or extracted helper | Logic is already correct and tested |
| Parser prefix detection | String::starts_with | `match_keyword_prefix` | Handles case-insensitive matching with whitespace tolerance |

## Common Pitfalls

### Pitfall 1: Parser prefix ordering
**What goes wrong:** New SHOW prefix shadows or is shadowed by existing prefixes
**Why it happens:** `detect_ddl_prefix` uses first-match-wins with longest-first ordering
**How to avoid:** `SHOW SEMANTIC MATERIALIZATIONS` (3 keywords) must be placed BEFORE `SHOW SEMANTIC VIEWS` (3 keywords) in the prefix list, since both start with `SHOW SEMANTIC`. Check that the keyword after `SEMANTIC` distinguishes them. Actually, since "MATERIALIZATIONS" != "VIEWS", they won't overlap. But place it in the right position among the other 3-keyword SHOW forms.
**Warning signs:** Test "SHOW SEMANTIC MATERIALIZATIONS" being detected as `DdlKind::Show`

### Pitfall 2: Forgetting DDL_PREFIXES for near-miss detection
**What goes wrong:** Typos like "SHOW SEMANTIC MATERIALIZATION" (missing S) don't get helpful error messages
**Why it happens:** Near-miss detection uses a separate `DDL_PREFIXES` array
**How to avoid:** Add `"show semantic materializations"` to `DDL_PREFIXES` array
**Warning signs:** No "Did you mean?" suggestion for near-misses

### Pitfall 3: EXPLAIN resolution path
**What goes wrong:** Materialization name detection disagrees with actual routing in `expand()`
**Why it happens:** Using different matching logic in explain vs expand
**How to avoid:** Use the same function (or a thin wrapper) for both. The `try_route_materialization` function in `expand/materialization.rs` is the single source of truth.
**Warning signs:** EXPLAIN says "Materialization: region_agg" but the SQL doesn't use the materialization table

### Pitfall 4: Forgetting to handle empty materializations in DESCRIBE
**What goes wrong:** No-op when no materializations declared, which is correct -- but tests must verify this
**Why it happens:** `collect_materialization_rows` will simply not add any rows when `def.materializations` is empty
**How to avoid:** Test both with-materializations and without-materializations cases in DESCRIBE
**Warning signs:** DESCRIBE output changes for pre-existing views without materializations (should not change)

### Pitfall 5: Missing `pub mod show_materializations` in ddl/mod.rs
**What goes wrong:** Compilation error -- VTab types not visible to lib.rs
**Why it happens:** New module file not registered in parent mod.rs
**How to avoid:** Checklist includes mod.rs update

## Code Examples

### SHOW SEMANTIC MATERIALIZATIONS output format

```
-- For: SHOW SEMANTIC MATERIALIZATIONS IN my_view
database_name | schema_name | semantic_view_name | name       | table          | dimensions       | metrics
memory        | main        | my_view           | region_agg | my_region_agg  | ["region"]      | ["total_revenue","order_count"]
```

### DESCRIBE SEMANTIC VIEW materialization rows

```
-- Added after existing METRIC/DERIVED_METRIC rows:
MATERIALIZATION | region_agg | (empty) | TABLE      | my_region_agg
MATERIALIZATION | region_agg | (empty) | DIMENSIONS | ["region"]
MATERIALIZATION | region_agg | (empty) | METRICS    | ["total_revenue","order_count"]
```

### EXPLAIN output with materialization routing

```
-- Semantic View: my_view
-- Dimensions: region
-- Metrics: total_revenue, order_count
-- Materialization: region_agg
--
-- Expanded SQL:
SELECT
    "region",
    "total_revenue",
    "order_count"
FROM "my_region_agg"
--
-- DuckDB Plan:
...
```

When no materialization matches:
```
-- Materialization: none
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No materialization introspection | Phase 57 adds it | v0.7.0 | Users can diagnose routing decisions |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | SHOW SEMANTIC MATERIALIZATIONS should support LIKE/IN/STARTS WITH/LIMIT filter clauses like other SHOW commands | Architecture Patterns | LOW -- consistent UX; can always add later |
| A2 | DESCRIBE MATERIALIZATION rows use empty string for parent_entity | Architecture Patterns | LOW -- materializations are view-level, no parent table |
| A3 | EXPLAIN header line format: `-- Materialization: <name or none>` | Architecture Patterns | LOW -- display-only, easy to adjust |
| A4 | SHOW MATERIALIZATIONS columns: 7 columns (db, schema, sv_name, name, table, dimensions, metrics) | Architecture Patterns | LOW -- follows existing pattern, can adjust |

## Open Questions

1. **Should `SHOW SEMANTIC MATERIALIZATIONS` support `FOR METRIC` filtering?**
   - What we know: SHOW DIMENSIONS supports `FOR METRIC` to filter dims by metric. Materializations cover both dims and metrics.
   - What's unclear: Is filtering materializations by a specific metric useful?
   - Recommendation: Skip `FOR METRIC` in v0.7.0 -- keep it simple. Can add later if users want it.

2. **Should materializations appear in `SHOW COLUMNS IN SEMANTIC VIEW`?**
   - What we know: SHOW COLUMNS shows (name, type) pairs for queryable columns. Materializations are metadata, not queryable columns.
   - Recommendation: No -- materializations are not columns. They appear in DESCRIBE and their own SHOW command.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest + cargo test |
| Config file | `test/sql/TEST_LIST` |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| INTR-01 | explain_semantic_view() includes materialization routing info | integration (sqllogictest) | `just test-sql` | Wave 0 |
| INTR-01 | explain output says "none" when no materialization matches | integration (sqllogictest) | `just test-sql` | Wave 0 |
| INTR-02 | DESCRIBE includes MATERIALIZATION object_kind rows | integration (sqllogictest) | `just test-sql` | Wave 0 |
| INTR-02 | DESCRIBE without materializations unchanged | integration (sqllogictest) | `just test-sql` | Wave 0 |
| INTR-03 | SHOW SEMANTIC MATERIALIZATIONS IN view_name lists mats | integration (sqllogictest) | `just test-sql` | Wave 0 |
| INTR-03 | SHOW SEMANTIC MATERIALIZATIONS (cross-view) works | integration (sqllogictest) | `just test-sql` | Wave 0 |
| INTR-03 | Parser detection for SHOW SEMANTIC MATERIALIZATIONS | unit | `cargo test` | Wave 0 |
| INTR-03 | Near-miss detection for SHOW SEMANTIC MATERIALIZATIONS | unit | `cargo test` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase57_introspection.test` -- covers INTR-01, INTR-02, INTR-03
- [ ] `test/sql/TEST_LIST` -- add phase57 entry
- [ ] Parse detection unit tests in `parse.rs` `#[cfg(test)]` -- INTR-03

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | -- |
| V3 Session Management | no | -- |
| V4 Access Control | no | -- |
| V5 Input Validation | yes | Existing `match_keyword_prefix` for parser, safe_name SQL escaping via `.replace('\'', "''")` |
| V6 Cryptography | no | -- |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via view name in SHOW | Tampering | Single-quote escaping in rewrite_ddl (existing pattern) |
| SQL injection via LIKE pattern | Tampering | LIKE pattern passed through existing filter suffix builder (already safe) |

No new security surfaces introduced -- all new code follows established input handling patterns.

## Sources

### Primary (HIGH confidence)
- `src/query/explain.rs` -- full ExplainSemanticViewVTab implementation
- `src/ddl/describe.rs` -- full DescribeSemanticViewVTab with property-per-row format
- `src/ddl/show_dims.rs` -- canonical SHOW command pattern (single + cross-view VTabs)
- `src/parse.rs` -- DdlKind enum, detection, rewriting, near-miss, validation
- `src/expand/materialization.rs` -- try_route_materialization pure function
- `src/expand/sql_gen.rs:267-366` -- expand() routing integration point
- `src/model.rs:211-227` -- Materialization struct fields
- `src/lib.rs:280-595` -- VTab registration pattern
- `test/sql/phase55_materialization_routing.test` -- existing materialization tests

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all patterns established
- Architecture: HIGH -- 5 existing SHOW commands and DESCRIBE format provide exact templates
- Pitfalls: HIGH -- all pitfalls are structural and verified via codebase analysis

**Research date:** 2026-04-20
**Valid until:** 2026-05-20 (stable codebase patterns, no external dependencies)
