# Phase 44: SHOW/DESCRIBE Metadata Surface + Enhancements - Research

**Researched:** 2026-04-10
**Domain:** DuckDB extension table functions, SQL rewrite layer, Snowflake SHOW/DESCRIBE parity
**Confidence:** HIGH

## Summary

Phase 44 surfaces the metadata annotations added in Phase 43 (COMMENT, SYNONYMS, PRIVATE/PUBLIC) through the existing SHOW and DESCRIBE introspection commands, and adds four new DDL forms: SHOW TERSE, SHOW ... IN SCHEMA/DATABASE, SHOW COLUMNS IN SEMANTIC VIEW, and enhanced DESCRIBE properties.

The implementation is entirely within the extension's Rust codebase -- no new external dependencies, no new DuckDB APIs. All changes fall into three categories: (1) adding columns to existing VTab output schemas, (2) adding new VTab registrations and DDL parse variants, and (3) extending the DDL rewrite layer in `parse.rs` to recognize new syntax forms. Phase 43 already stored all metadata fields in the model; this phase reads and displays them.

**Primary recommendation:** Split into two plans: Plan 01 adds synonyms/comment columns to existing SHOW commands and new DESCRIBE properties (SHOW-01, SHOW-06); Plan 02 adds the new DDL forms -- TERSE, IN SCHEMA/DATABASE, SHOW COLUMNS (SHOW-02, SHOW-03, SHOW-04, SHOW-05). This order ensures the foundation (column additions) is solid before the new syntax variants build on it.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SHOW-01 | SHOW SEMANTIC VIEWS/DIMENSIONS/METRICS/FACTS include synonyms and comment columns | Add 2 VARCHAR columns (`synonyms`, `comment`) to all 4 SHOW VTabs' row structs and output schemas. Read from model fields added in Phase 43. |
| SHOW-02 | SHOW SEMANTIC VIEWS IN SCHEMA schema_name filters by schema | New parse clause `IN SCHEMA <name>` for `DdlKind::Show`; filter rows in `ListSemanticViewsVTab::bind()` by `schema_name` match. Lift existing "IN is not valid for SHOW SEMANTIC VIEWS" error. |
| SHOW-03 | SHOW SEMANTIC VIEWS IN DATABASE db_name filters by database | New parse clause `IN DATABASE <name>` for `DdlKind::Show`; filter rows by `database_name` match. |
| SHOW-04 | SHOW TERSE SEMANTIC VIEWS returns reduced column set | New `DdlKind::ShowTerse` variant; new `ListTerseSemanticViewsVTab` with 5 columns (created_on, name, kind, database_name, schema_name) -- same as current SHOW but explicitly named "terse" for Snowflake alignment. |
| SHOW-05 | SHOW COLUMNS IN SEMANTIC VIEW returns unified dims+facts+metrics with kind column | New `DdlKind::ShowColumns` variant; new `ShowColumnsInSemanticViewVTab` with columns: database_name, schema_name, semantic_view_name, column_name, data_type, kind, expression, comment. |
| SHOW-06 | DESCRIBE SEMANTIC VIEW includes COMMENT, SYNONYMS, and ACCESS_MODIFIER properties | Extend `collect_*_rows` functions in `describe.rs` to emit COMMENT, SYNONYMS, ACCESS_MODIFIER property rows. View-level COMMENT emitted as object_kind=NULL. |
</phase_requirements>

## Standard Stack

No new libraries required. All work is within the existing Rust codebase.

### Core (existing, unchanged)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| duckdb-rs | 1.10500.0 | DuckDB Rust bindings (VTab trait, LogicalTypeHandle) | Project standard since v0.1.0 [VERIFIED: Cargo.toml] |
| serde / serde_json | 1.x | Model serialization/deserialization | Project standard since v0.1.0 [VERIFIED: Cargo.toml] |

No `npm install` or `cargo add` needed.

## Architecture Patterns

### Current Module Structure (relevant files)
```
src/
  parse.rs           # DDL detection (DdlKind enum) + rewrite to function calls
  ddl/
    mod.rs           # Module exports
    list.rs          # SHOW SEMANTIC VIEWS (ListSemanticViewsVTab)
    describe.rs      # DESCRIBE SEMANTIC VIEW (DescribeSemanticViewVTab)
    show_dims.rs     # SHOW SEMANTIC DIMENSIONS (single + all VTabs)
    show_metrics.rs  # SHOW SEMANTIC METRICS (single + all VTabs)
    show_facts.rs    # SHOW SEMANTIC FACTS (single + all VTabs)
    show_dims_for_metric.rs  # SHOW SEMANTIC DIMENSIONS ... FOR METRIC
  model.rs           # SemanticViewDefinition, Dimension, Metric, Fact, AccessModifier
  lib.rs             # Function registration (register_table_function_with_extra_info)
```

### Pattern 1: VTab Output Schema Extension (SHOW-01)
**What:** Add columns to existing VTab structs and bind functions
**When to use:** SHOW-01 (synonyms + comment columns on all SHOW commands)
**How it works in this codebase:**

Each SHOW command follows this pattern [VERIFIED: show_dims.rs, show_metrics.rs, show_facts.rs, list.rs]:

1. Row struct holds string fields (e.g., `ShowDimRow { database_name, schema_name, ... }`)
2. `bind_output_columns()` declares output schema via `bind.add_result_column()`
3. `collect_*()` helper reads from `SemanticViewDefinition` fields into row structs
4. `emit_rows()` writes row fields into output vectors via `flat_vector(N).insert(i, val)`

To add `synonyms` and `comment` columns:
- Add fields to row struct
- Add 2 more `add_result_column()` calls in `bind_output_columns()`
- Read from model fields in `collect_*()` (already present from Phase 43)
- Write into output vectors in `emit_rows()` (adjust vector indices)

**Synonyms formatting:** Snowflake shows synonyms as a JSON array string. Use the existing `format_json_array()` helper from `describe.rs`. [VERIFIED: describe.rs line 45-48]

**For SHOW SEMANTIC VIEWS (list.rs):** The view-level comment is `def.comment` on `SemanticViewDefinition`. Synonyms at the view level don't exist in the model (Snowflake also doesn't have view-level synonyms for SHOW VIEWS output), so the synonyms column should be empty for SHOW SEMANTIC VIEWS. Actually, on closer inspection, the Snowflake SHOW SEMANTIC VIEWS has a `comment` column but not a `synonyms` column. For SHOW SEMANTIC DIMENSIONS/METRICS/FACTS, both `synonyms` and `comment` columns are present. [CITED: docs.snowflake.com/en/sql-reference/sql/show-semantic-views, docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions]

### Pattern 2: New DdlKind Variant + VTab Registration (SHOW-04, SHOW-05)
**What:** Add new DDL forms detected by parser and backed by new table functions
**When to use:** SHOW TERSE, SHOW COLUMNS

The pattern is [VERIFIED: parse.rs, lib.rs]:

1. Add variant to `DdlKind` enum in `parse.rs`
2. Add `match_keyword_prefix()` call in `detect_ddl_prefix()` (longest-first ordering)
3. Add function name mapping in `function_name()`
4. Add rewrite logic in `rewrite_ddl()` match arm
5. Add fallthrough in `detect_ddl_kind()` (entry point from C++ hook)
6. Create new VTab struct implementing `duckdb::vtab::VTab` trait
7. Register in `lib.rs` via `register_table_function_with_extra_info()`

### Pattern 3: Parse Clause Extension (SHOW-02, SHOW-03)
**What:** Extend `parse_show_filter_clauses()` to recognize IN SCHEMA / IN DATABASE
**When to use:** SHOW SEMANTIC VIEWS IN SCHEMA/DATABASE

Currently, the `IN` clause for `DdlKind::Show` returns an error: "IN is not valid for SHOW SEMANTIC VIEWS". [VERIFIED: parse.rs line 340-342]

For SHOW-02/SHOW-03, we need to:
1. Remove the error for `DdlKind::Show` when `IN` is followed by `SCHEMA` or `DATABASE`
2. Parse the schema/database name after the keyword
3. Pass the filter to the VTab bind function (via rewrite to SQL WHERE clause or via parameter)

**Recommended approach:** Rewrite to `SELECT * FROM list_semantic_views() WHERE schema_name = '<name>'` (for IN SCHEMA) or `WHERE database_name = '<name>'` (for IN DATABASE). This matches the existing filter suffix pattern. [VERIFIED: parse.rs build_filter_suffix()]

### Pattern 4: DESCRIBE Property Extension (SHOW-06)
**What:** Add new property rows to DESCRIBE output
**When to use:** SHOW-06 (COMMENT, SYNONYMS, ACCESS_MODIFIER in DESCRIBE)

The DESCRIBE VTab collects property rows via `collect_*_rows()` functions [VERIFIED: describe.rs]. Each function pushes `DescribeRow` structs with property/property_value pairs.

To add new properties:
- In `collect_table_rows()`: add COMMENT and SYNONYMS rows (when present)
- In `collect_fact_rows()`: add COMMENT, SYNONYMS, ACCESS_MODIFIER rows
- In `collect_dimension_rows()`: add COMMENT and SYNONYMS rows (no access modifier on dimensions)
- In `collect_metric_rows()`: add COMMENT, SYNONYMS, ACCESS_MODIFIER rows
- Add view-level COMMENT: new row with object_kind="", object_name="", property="COMMENT"

**Snowflake alignment:** Snowflake emits view-level COMMENT as object_kind=NULL, object_name=NULL. [CITED: docs.snowflake.com/en/sql-reference/sql/desc-semantic-view]

**ACCESS_MODIFIER values:** "PUBLIC" or "PRIVATE" string. Only on facts and metrics. [VERIFIED: model.rs AccessModifier enum]

### Anti-Patterns to Avoid
- **Adding columns without updating all test expectations:** The existing sqllogictest files use exact column counts in `query TTTTTT` directives. Adding columns to SHOW output requires updating ALL existing tests that reference these VTabs. This is the highest-risk change in this phase.
- **Breaking the filter suffix mechanism:** The `build_filter_suffix()` function references `name` column. If column ordering changes, WHERE clauses still work because they reference column names, not positions. Safe.
- **Forgetting to update the `emit_rows()` vector indices:** When adding columns, every `output.flat_vector(N)` index must be updated. Off-by-one errors here cause data corruption (columns swapped).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON array formatting for synonyms | Custom string builder | `format_json_array()` from describe.rs | Already exists, matches Snowflake format |
| Schema/database filtering | Custom VTab filter | SQL WHERE clause via rewrite | Existing pattern, zero VTab changes needed |
| TERSE column reduction | New complex VTab | Reuse ListSemanticViewsVTab data pattern | Same 5 columns as current SHOW VIEWS |

## Common Pitfalls

### Pitfall 1: Existing Test Breakage from Column Addition
**What goes wrong:** Adding `synonyms` and `comment` columns to SHOW SEMANTIC DIMENSIONS/METRICS/FACTS changes the output from 6 to 8 columns. Every existing sqllogictest that uses `query TTTTTT` (6 T's) for these commands will fail.
**Why it happens:** sqllogictest column count directives (`query TTTTTT`) must exactly match output column count.
**How to avoid:** Update all existing tests in the same plan that adds the columns. Specifically:
- `test/sql/phase34_1_show_commands.test` -- all SHOW DIMENSIONS/METRICS/FACTS queries (6T -> 8T)
- `test/sql/phase34_1_1_show_filtering.test` -- all SHOW DIMENSIONS/METRICS/FACTS queries with LIKE/STARTS WITH (6T -> 8T)
- `test/sql/phase34_1_show_dims_for_metric.test` -- check if FOR METRIC output also changes
**Warning signs:** `just test-sql` fails with column count mismatch errors.

### Pitfall 2: SHOW SEMANTIC VIEWS Column Schema Divergence
**What goes wrong:** Snowflake SHOW SEMANTIC VIEWS has `comment` column but NOT `synonyms`. SHOW DIMENSIONS/METRICS/FACTS have BOTH `synonyms` and `comment`. Applying the same schema to all commands is wrong.
**Why it happens:** Assuming all SHOW commands get the same treatment.
**How to avoid:** SHOW SEMANTIC VIEWS adds only `comment` (1 new column, 5->6). SHOW DIMS/METRICS/FACTS adds both `synonyms` and `comment` (2 new columns, 6->8).
**Warning signs:** Tests expect wrong column counts.

### Pitfall 3: SHOW TERSE vs Current SHOW VIEWS Identity
**What goes wrong:** The current SHOW SEMANTIC VIEWS already outputs 5 columns (created_on, name, kind, database_name, schema_name) which is identical to Snowflake's TERSE output. After SHOW-01 adds `comment`, the full SHOW will have 6 columns and TERSE will remain at 5.
**Why it happens:** The pre-Phase 44 output was accidentally already TERSE-shaped.
**How to avoid:** Implement SHOW TERSE as a separate DdlKind and VTab that explicitly outputs the 5-column subset. The current list.rs VTab will grow to 6 columns (with comment). TERSE stays at 5.
**Warning signs:** TERSE and full SHOW return same columns.

### Pitfall 4: IN SCHEMA/DATABASE vs Existing IN view_name Conflict
**What goes wrong:** The parser currently treats `IN` as "view name follows" for SHOW DIMS/METRICS/FACTS. For SHOW VIEWS, `IN` was explicitly rejected. Adding `IN SCHEMA` and `IN DATABASE` requires disambiguating `IN <schema_name>` from the view-name form.
**Why it happens:** The parser peeks at the next token after `IN` but currently just reads it as a view name.
**How to avoid:** Check if token after `IN` is `SCHEMA` or `DATABASE` keyword. If yes, parse as scope filter. If no, treat as view name (existing behavior for DIMS/METRICS/FACTS) or error for SHOW VIEWS without SCHEMA/DATABASE.
**Warning signs:** `SHOW SEMANTIC VIEWS IN main` parsed as view name instead of schema name.

### Pitfall 5: DESCRIBE Property Ordering with Optional Properties
**What goes wrong:** When COMMENT/SYNONYMS/ACCESS_MODIFIER are not set (None/empty), the rows should be omitted (matching Snowflake behavior where absent properties are not shown).
**Why it happens:** Emitting empty-value rows clutters output and doesn't match Snowflake.
**How to avoid:** Only emit COMMENT row when `comment.is_some()`. Only emit SYNONYMS row when `synonyms` is non-empty. Only emit ACCESS_MODIFIER when access is PRIVATE (PUBLIC is the default and can be omitted, or always shown -- Snowflake shows it always for facts/metrics).
**Warning signs:** DESCRIBE output has many empty-value property rows.

### Pitfall 6: SHOW COLUMNS IN SEMANTIC VIEW vs IN view_name Ambiguity
**What goes wrong:** `SHOW COLUMNS IN SEMANTIC VIEW my_view` needs to parse as SHOW COLUMNS + IN SEMANTIC VIEW + view_name. But the parser might try to match `SHOW` as `DdlKind::Show` first.
**Why it happens:** The existing DDL detection logic matches `SHOW SEMANTIC` prefix; `SHOW COLUMNS` is a different prefix.
**How to avoid:** Add `SHOW COLUMNS IN SEMANTIC VIEW` as a new prefix in `detect_ddl_prefix()` BEFORE the `SHOW SEMANTIC VIEWS` match (longest-first ordering). The keyword sequence is `[show, columns, in, semantic, view]` (5 keywords).
**Warning signs:** `SHOW COLUMNS IN SEMANTIC VIEW x` gets rejected as "not a semantic view DDL statement".

## Code Examples

### Example 1: Adding synonyms/comment columns to SHOW DIMS (SHOW-01)

```rust
// Source: pattern from show_dims.rs, extended

struct ShowDimRow {
    database_name: String,
    schema_name: String,
    semantic_view_name: String,
    table_name: String,
    name: String,
    data_type: String,
    // NEW: Phase 44
    synonyms: String,    // JSON array string e.g. '["territory","area"]'
    comment: String,     // Plain text or empty
}

fn bind_output_columns(bind: &BindInfo) {
    // ... existing 6 columns ...
    // NEW columns at end:
    bind.add_result_column("synonyms", LogicalTypeHandle::from(LogicalTypeId::Varchar));
    bind.add_result_column("comment", LogicalTypeHandle::from(LogicalTypeId::Varchar));
}

fn collect_dims(view_name: &str, json: &str) -> Vec<ShowDimRow> {
    // ... existing logic ...
    // Add to each row:
    // synonyms: format_json_array(&d.synonyms),  // uses describe.rs helper
    // comment: d.comment.clone().unwrap_or_default(),
}
```

### Example 2: DESCRIBE with COMMENT/SYNONYMS/ACCESS_MODIFIER (SHOW-06)

```rust
// Source: pattern from describe.rs, extended

// In collect_dimension_rows(), after existing TABLE/EXPRESSION/DATA_TYPE rows:
if let Some(ref comment) = dim.comment {
    rows.push(DescribeRow {
        object_kind: "DIMENSION".to_string(),
        object_name: dim.name.clone(),
        parent_entity: parent.clone(),
        property: "COMMENT".to_string(),
        property_value: comment.clone(),
    });
}
if !dim.synonyms.is_empty() {
    rows.push(DescribeRow {
        object_kind: "DIMENSION".to_string(),
        object_name: dim.name.clone(),
        parent_entity: parent.clone(),
        property: "SYNONYMS".to_string(),
        property_value: format_json_array(&dim.synonyms),
    });
}

// For metrics/facts, add ACCESS_MODIFIER:
rows.push(DescribeRow {
    object_kind: object_kind.to_string(),
    object_name: metric.name.clone(),
    parent_entity: parent.clone(),
    property: "ACCESS_MODIFIER".to_string(),
    property_value: match metric.access {
        AccessModifier::Public => "PUBLIC".to_string(),
        AccessModifier::Private => "PRIVATE".to_string(),
    },
});
```

### Example 3: IN SCHEMA/DATABASE Parse Extension (SHOW-02/03)

```rust
// Source: pattern from parse.rs parse_show_filter_clauses(), extended

// In the IN clause handler, for DdlKind::Show:
if kind == DdlKind::Show || kind == DdlKind::ShowTerse {
    // Check for SCHEMA or DATABASE keyword after IN
    if rest.len() >= 6 && rest[..6].eq_ignore_ascii_case("SCHEMA") {
        rest = rest[6..].trim_start();
        let name_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        in_schema = Some(&rest[..name_end]);
        rest = rest[name_end..].trim_start();
    } else if rest.len() >= 8 && rest[..8].eq_ignore_ascii_case("DATABASE") {
        rest = rest[8..].trim_start();
        let name_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        in_database = Some(&rest[..name_end]);
        rest = rest[name_end..].trim_start();
    } else {
        return Err("SHOW SEMANTIC VIEWS requires IN SCHEMA <name> or IN DATABASE <name>".into());
    }
}
```

### Example 4: SHOW COLUMNS Rewrite (SHOW-05)

```rust
// New DdlKind variant and keyword prefix:
// In detect_ddl_prefix(), add BEFORE other SHOW matches:
// SHOW COLUMNS IN SEMANTIC VIEW (5 keywords)
if let Some(n) = match_keyword_prefix(b, &[b"show", b"columns", b"in", b"semantic", b"view"]) {
    return Some((DdlKind::ShowColumns, n));
}

// Rewrite: SHOW COLUMNS IN SEMANTIC VIEW my_view
// -> SELECT * FROM show_columns_in_semantic_view('my_view')
```

## State of the Art

| Old Approach (pre-Phase 44) | Current Approach (Phase 44) | Impact |
|---|---|---|
| SHOW DIMS/METRICS/FACTS: 6 columns | 8 columns (+synonyms, +comment) | All existing tests must update column counts |
| SHOW VIEWS: 5 columns | 6 columns (+comment) | All existing SHOW VIEWS tests must update |
| DESCRIBE: no metadata properties | COMMENT, SYNONYMS, ACCESS_MODIFIER rows | More rows per object in DESCRIBE output |
| IN rejected for SHOW VIEWS | IN SCHEMA/DATABASE accepted | Parser clause extension |
| No TERSE mode | SHOW TERSE returns 5-column subset | New DdlKind variant |
| No SHOW COLUMNS | SHOW COLUMNS IN SEMANTIC VIEW | New DdlKind + VTab |

## Column Schema Summary

### SHOW SEMANTIC VIEWS (after Phase 44)
| Column | Type | Source |
|--------|------|--------|
| created_on | VARCHAR | def.created_on |
| name | VARCHAR | catalog key |
| kind | VARCHAR | constant "SEMANTIC_VIEW" |
| database_name | VARCHAR | def.database_name |
| schema_name | VARCHAR | def.schema_name |
| **comment** | **VARCHAR** | **def.comment (NEW)** |

[CITED: docs.snowflake.com/en/sql-reference/sql/show-semantic-views]

### SHOW TERSE SEMANTIC VIEWS (new)
| Column | Type | Source |
|--------|------|--------|
| created_on | VARCHAR | def.created_on |
| name | VARCHAR | catalog key |
| kind | VARCHAR | constant "SEMANTIC_VIEW" |
| database_name | VARCHAR | def.database_name |
| schema_name | VARCHAR | def.schema_name |

[CITED: docs.snowflake.com/en/sql-reference/sql/show-semantic-views]

### SHOW SEMANTIC DIMENSIONS/METRICS/FACTS (after Phase 44)
| Column | Type | Source |
|--------|------|--------|
| database_name | VARCHAR | def.database_name |
| schema_name | VARCHAR | def.schema_name |
| semantic_view_name | VARCHAR | catalog key |
| table_name | VARCHAR | alias_map lookup |
| name | VARCHAR | item.name |
| data_type | VARCHAR | item.output_type |
| **synonyms** | **VARCHAR** | **format_json_array(&item.synonyms) (NEW)** |
| **comment** | **VARCHAR** | **item.comment (NEW)** |

[CITED: docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions, docs.snowflake.com/en/sql-reference/sql/show-semantic-facts]

### SHOW COLUMNS IN SEMANTIC VIEW (new)
| Column | Type | Source |
|--------|------|--------|
| database_name | VARCHAR | def.database_name |
| schema_name | VARCHAR | def.schema_name |
| semantic_view_name | VARCHAR | catalog key |
| column_name | VARCHAR | item.name |
| data_type | VARCHAR | item.output_type |
| kind | VARCHAR | "DIMENSION" / "FACT" / "METRIC" / "DERIVED_METRIC" |
| expression | VARCHAR | item.expr |
| comment | VARCHAR | item.comment |

[CITED: docs.snowflake.com/en/sql-reference/sql/show-columns] -- note: Snowflake's SHOW COLUMNS has more columns (null?, default, autoincrement, schema_evolution_record) that don't apply to semantic view objects. We emit the relevant subset.

### DESCRIBE SEMANTIC VIEW -- New Properties (Phase 44)
| Object Kind | New Property | Value |
|-------------|-------------|-------|
| (empty/NULL) | COMMENT | View-level comment text |
| TABLE | COMMENT | Table entry comment |
| TABLE | SYNONYMS | JSON array of synonyms |
| DIMENSION | COMMENT | Dimension comment |
| DIMENSION | SYNONYMS | JSON array of synonyms |
| FACT | COMMENT | Fact comment |
| FACT | SYNONYMS | JSON array of synonyms |
| FACT | ACCESS_MODIFIER | "PUBLIC" or "PRIVATE" |
| METRIC | COMMENT | Metric comment |
| METRIC | SYNONYMS | JSON array of synonyms |
| METRIC | ACCESS_MODIFIER | "PUBLIC" or "PRIVATE" |
| DERIVED_METRIC | COMMENT | Derived metric comment |
| DERIVED_METRIC | SYNONYMS | JSON array of synonyms |
| DERIVED_METRIC | ACCESS_MODIFIER | "PUBLIC" or "PRIVATE" |

[CITED: docs.snowflake.com/en/sql-reference/sql/desc-semantic-view]

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest (Python runner) + cargo test (Rust unit/proptest) |
| Config file | test/sql/*.test (sqllogictest), inline #[cfg(test)] modules (Rust) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SHOW-01 | synonyms + comment columns in SHOW output | integration (slt) | `just test-sql` | No -- Wave 0 |
| SHOW-02 | IN SCHEMA filtering | integration (slt) | `just test-sql` | No -- Wave 0 |
| SHOW-03 | IN DATABASE filtering | integration (slt) | `just test-sql` | No -- Wave 0 |
| SHOW-04 | TERSE mode | integration (slt) + unit | `just test-sql` | No -- Wave 0 |
| SHOW-05 | SHOW COLUMNS | integration (slt) + unit | `just test-sql` | No -- Wave 0 |
| SHOW-06 | DESCRIBE metadata props | integration (slt) | `just test-sql` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase44_show_metadata.test` -- covers SHOW-01 (new columns), SHOW-02 (IN SCHEMA), SHOW-03 (IN DATABASE), SHOW-04 (TERSE)
- [ ] `test/sql/phase44_show_columns.test` -- covers SHOW-05 (SHOW COLUMNS IN SEMANTIC VIEW)
- [ ] `test/sql/phase44_describe_metadata.test` -- covers SHOW-06 (DESCRIBE new properties)
- [ ] Update existing `test/sql/phase34_1_show_commands.test` -- column count changes (6T -> 8T for DIMS/METRICS/FACTS)
- [ ] Update existing `test/sql/phase34_1_1_show_filtering.test` -- column count changes (6T -> 8T)

### Existing Tests Requiring Update
These files will break when columns are added and must be updated as part of implementation:

| File | Current Query Type | Change Needed |
|------|-------------------|---------------|
| `phase34_1_show_commands.test` | `query TTTTTT` (6 cols) for DIMS/METRICS/FACTS | -> `query TTTTTTTT` (8 cols), add empty synonyms/comment columns |
| `phase34_1_1_show_filtering.test` | `query TTTTTT` (6 cols) for DIMS/METRICS/FACTS filtering | -> `query TTTTTTTT` (8 cols), add empty synonyms/comment columns |
| `phase34_1_show_dims_for_metric.test` | `query TTTT` (4 cols) | Likely unchanged (FOR METRIC has separate schema) |

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A (PRIVATE visibility is semantic, not security) |
| V5 Input Validation | yes | Existing SQL-escaped quote handling in parse.rs |
| V6 Cryptography | no | N/A |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via SCHEMA/DATABASE name | Tampering | Single-quote escaping in parse.rs rewrite (existing pattern) |
| View name injection in SHOW COLUMNS | Tampering | Same quote-escaping as existing SHOW commands |

No new attack surface beyond existing patterns. All user input flows through the same `replace('\'', "''")` escaping used by all existing SHOW commands. [VERIFIED: parse.rs line 468]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Snowflake SHOW SEMANTIC VIEWS does NOT include a `synonyms` column (only `comment`) | Column Schema Summary | Extra column in SHOW VIEWS output deviates from Snowflake |
| A2 | DESCRIBE ACCESS_MODIFIER should be emitted for ALL facts/metrics (not just PRIVATE ones) | Code Examples | Missing property rows for PUBLIC items |
| A3 | View-level COMMENT in DESCRIBE uses empty-string object_kind (matching sqllogictest `(empty)` convention) rather than literal NULL | DESCRIBE Properties | Output format mismatch |
| A4 | SHOW COLUMNS output should NOT include `null?`, `default`, `autoincrement`, `schema_evolution_record` columns from Snowflake | Column Schema Summary | Missing columns if user expects full Snowflake parity |
| A5 | SHOW TERSE supports the same IN SCHEMA/DATABASE filtering as full SHOW VIEWS | Pitfall 3 | Missing filter capability on TERSE mode |

## Open Questions (RESOLVED)

1. **SHOW COLUMNS column subset**
   - What we know: Snowflake SHOW COLUMNS has 12+ columns including null?, default, autoincrement. Our semantic view objects don't have these properties.
   - What's unclear: Should we emit these as empty columns for Snowflake compatibility, or only emit the relevant subset?
   - Recommendation: Emit the relevant subset only (8 columns as documented above). Users can't filter by null? or autoincrement on semantic view objects since those concepts don't apply. This matches the project philosophy of "Snowflake-aligned but DuckDB-native."

2. **DESCRIBE: Always emit ACCESS_MODIFIER or only when PRIVATE?**
   - What we know: Snowflake shows ACCESS_MODIFIER for facts and metrics. Our model defaults to PUBLIC.
   - What's unclear: Does Snowflake always show the property (even for PUBLIC), or only when non-default?
   - Recommendation: Always emit ACCESS_MODIFIER for facts and metrics (both PUBLIC and PRIVATE). This makes the access level always discoverable without relying on "absence means PUBLIC" logic.

3. **IN SCHEMA/DATABASE on SHOW DIMS/METRICS/FACTS**
   - What we know: Snowflake supports IN SCHEMA/DATABASE on ALL SHOW commands. Our requirements only specify IN SCHEMA/DATABASE for SHOW VIEWS (SHOW-02, SHOW-03).
   - What's unclear: Should we also add IN SCHEMA/DATABASE to SHOW DIMS/METRICS/FACTS?
   - Recommendation: Implement only what requirements specify (SHOW VIEWS). Future phases can extend if needed. The architecture makes it easy to add later.

## Sources

### Primary (HIGH confidence)
- [Snowflake SHOW SEMANTIC VIEWS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-views) - column schemas, TERSE output, IN SCHEMA/DATABASE syntax
- [Snowflake DESCRIBE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view) - property-per-row format, COMMENT/SYNONYMS/ACCESS_MODIFIER properties
- [Snowflake SHOW SEMANTIC DIMENSIONS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-dimensions) - 8-column output with synonyms and comment
- [Snowflake SHOW SEMANTIC FACTS](https://docs.snowflake.com/en/sql-reference/sql/show-semantic-facts) - 8-column output with synonyms and comment
- [Snowflake SHOW COLUMNS](https://docs.snowflake.com/en/sql-reference/sql/show-columns) - kind column (DIMENSION/FACT/METRIC), IN SEMANTIC VIEW syntax
- Codebase files: `src/ddl/list.rs`, `src/ddl/describe.rs`, `src/ddl/show_dims.rs`, `src/ddl/show_metrics.rs`, `src/ddl/show_facts.rs`, `src/parse.rs`, `src/model.rs`, `src/lib.rs` [VERIFIED: direct file reads]

### Secondary (MEDIUM confidence)
- None

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all changes within existing codebase patterns
- Architecture: HIGH - all patterns verified by reading existing implementation
- Pitfalls: HIGH - identified from concrete column count mismatches and parser logic
- Snowflake alignment: MEDIUM-HIGH - column schemas from official docs, but A1/A2/A4 are assumptions about exact behavior

**Research date:** 2026-04-10
**Valid until:** 2026-05-10 (stable domain, no external dependency drift risk)
