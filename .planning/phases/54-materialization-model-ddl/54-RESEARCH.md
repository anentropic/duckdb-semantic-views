# Phase 54: Materialization Model & DDL - Research

**Researched:** 2026-04-19
**Domain:** Rust data model extension, SQL DDL body parser, serde persistence, YAML interop
**Confidence:** HIGH

## Summary

Phase 54 adds a new `MATERIALIZATIONS` clause to the existing `CREATE SEMANTIC VIEW` DDL, a corresponding `Materialization` struct to the data model, YAML deserialization support, persistence with backward compatibility, and define-time validation. This is a pure model-and-parser phase -- no query-time routing logic (that is Phase 55).

The codebase has a well-established pattern for adding new clauses: (1) add model struct(s) in `model.rs` with serde derives and `#[serde(default, skip_serializing_if)]` for backward compat, (2) add the clause keyword to `body_parser.rs` CLAUSE_KEYWORDS/CLAUSE_ORDER arrays and implement a `parse_materializations_clause` parser, (3) add the field to `SemanticViewDefinition`, (4) add DDL reconstruction in `render_ddl.rs`, (5) add define-time validation in `parse.rs` or inline in `parse_keyword_body`. The YAML path is free -- serde derives on the new structs automatically give YAML support via the existing `from_yaml`/`from_yaml_with_size_cap` methods.

**Primary recommendation:** Follow the FACTS clause pattern exactly (it was added post-initial-design and has the cleanest "add a new optional clause" implementation path).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| MAT-01 | MATERIALIZATIONS clause in SQL DDL with TABLE, DIMENSIONS, METRICS sub-clauses | Body parser clause addition pattern (CLAUSE_KEYWORDS, parse function, KeywordBody field) |
| MAT-06 | MATERIALIZATIONS works in both SQL DDL and YAML definitions | Automatic via serde Serialize/Deserialize derives on model structs -- same pattern as all existing clauses |
| MAT-07 | Materialization metadata persists across DuckDB restarts with backward compatibility | `#[serde(default, skip_serializing_if)]` on new field in SemanticViewDefinition -- identical to facts, window_spec, non_additive_by patterns |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Quality gate**: `just test-all` must pass (Rust tests + sqllogictest + DuckLake CI)
- **Test coverage**: Every phase needs unit tests, proptests, sqllogictest, and fuzz target consideration
- **Build**: `just build` for extension, `cargo test` for unit tests (no extension feature), `just test-sql` requires fresh build
- **Snowflake reference**: Use Snowflake semantic view behavior as guide when in doubt about SQL syntax

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde | 1.x | Serialize/Deserialize derives for model structs | Already used across all model structs [VERIFIED: Cargo.toml] |
| serde_json | 1.x | JSON serialization for catalog persistence | Already used for all persistence [VERIFIED: Cargo.toml] |
| yaml_serde | 0.10 | YAML serialization (aliased serde_yaml_ng) | Already added in Phase 51 [VERIFIED: Cargo.toml] |
| strsim | (existing) | Levenshtein distance for "did you mean?" suggestions | Already used in body_parser.rs for clause keyword suggestions [VERIFIED: body_parser.rs line 63] |

### Supporting
No new dependencies needed. All required functionality is provided by existing dependencies.

## Architecture Patterns

### Recommended Project Structure

No new files needed. Changes to existing files:

```
src/
  model.rs               # Add Materialization struct + materializations field on SemanticViewDefinition
  body_parser.rs          # Add "materializations" to CLAUSE_KEYWORDS/CLAUSE_ORDER, parse function
  parse.rs                # Add materializations field to SemanticViewDefinition construction in rewrite_ddl_keyword_body
  render_ddl.rs           # Add emit_materializations() and call from render_create_ddl
tests/
  (existing test files)   # Add materialization tests to model, body_parser, parse, render_ddl test modules
test/sql/
  phase54_materializations.test  # sqllogictest integration tests
```

### Pattern 1: New Model Struct (Materialization)

**What:** A struct representing a named materialization with a table reference and covered dimensions/metrics.
**When to use:** This exact pattern -- adding a new data model struct with serde derives.
**Example:**
```rust
// Source: [model.rs existing patterns -- TableRef, Fact, Metric structs]
/// A named materialization declaration mapping a pre-aggregated table
/// to the dimensions and metrics it covers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Materialization {
    /// User-assigned name for this materialization (e.g., "daily_revenue_by_region").
    pub name: String,
    /// Fully qualified table name of the pre-aggregated table
    /// (e.g., "catalog.schema.daily_revenue_agg").
    pub table: String,
    /// Dimension names covered by this materialization.
    /// Must be a subset of the semantic view's declared dimensions.
    pub dimensions: Vec<String>,
    /// Metric names covered by this materialization.
    /// Must be a subset of the semantic view's declared metrics.
    pub metrics: Vec<String>,
}
```

### Pattern 2: Backward-Compatible Field Addition on SemanticViewDefinition

**What:** Adding an optional Vec field with `#[serde(default, skip_serializing_if)]` so old stored JSON loads without error.
**When to use:** Every time a new field is added to SemanticViewDefinition.
**Example:**
```rust
// Source: [model.rs -- facts, joins, tables fields all use this pattern]
/// Named materializations mapping pre-aggregated tables to covered dims/metrics.
/// Old stored JSON without this field deserializes with empty Vec.
/// Not serialized when empty to preserve backward-compatible JSON.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub materializations: Vec<Materialization>,
```

### Pattern 3: New Clause in Body Parser

**What:** Adding "materializations" to CLAUSE_KEYWORDS and CLAUSE_ORDER, then implementing a parse function.
**When to use:** When a new SQL DDL clause is needed.
**Key details from codebase analysis:**

1. `CLAUSE_KEYWORDS` array in body_parser.rs (line 54): Add `"materializations"`
2. `CLAUSE_ORDER` array (line 59): Add `"materializations"` -- it should come AFTER `"metrics"` since materializations reference dimension and metric names that must already be parsed
3. `find_clause_bounds` validates ordering, duplicates, and requires TABLES + at least one of DIMENSIONS/METRICS -- adding materializations as optional after metrics requires no changes to these validation rules
4. `parse_keyword_body` dispatches on `bound.keyword` -- add a `"materializations"` arm
5. `KeywordBody` struct (line 37): Add `materializations: Vec<Materialization>` field

**Clause ordering after this change:**
```
TABLES -> RELATIONSHIPS (opt) -> FACTS (opt) -> DIMENSIONS (opt) -> METRICS (opt) -> MATERIALIZATIONS (opt)
```

The ordering error message in `find_clause_bounds` (line 280) needs updating to include MATERIALIZATIONS.

### Pattern 4: Define-Time Validation

**What:** After parsing, validate that materialization dimension/metric names reference declared names.
**When to use:** In `parse_keyword_body` after all clauses are parsed, same location as NON ADDITIVE BY and window metric validation (lines 424-538).
**Example validation pattern:**
```rust
// Source: [body_parser.rs lines 424-448 -- NON ADDITIVE BY validation]
// Validate materialization dimension and metric references
for mat in &materializations {
    for dim_name in &mat.dimensions {
        let dim_exists = dimensions.iter().any(|d| d.name.eq_ignore_ascii_case(dim_name));
        if !dim_exists {
            let available = dimensions.iter().map(|d| d.name.clone()).collect::<Vec<_>>();
            let suggestion = crate::util::suggest_closest(dim_name, &available);
            let mut msg = format!(
                "Materialization '{}': dimension '{}' not found in semantic view dimensions.",
                mat.name, dim_name
            );
            if let Some(closest) = suggestion {
                use std::fmt::Write;
                let _ = write!(msg, " Did you mean '{closest}'?");
            }
            return Err(ParseError { message: msg, position: None });
        }
    }
    // Same for metrics...
}
```

### Pattern 5: DDL Syntax for MATERIALIZATIONS Clause

**What:** The SQL syntax users will write.
**Design decision:** Follow the existing pattern of `name AS (...)` entries with sub-clause keywords.

```sql
CREATE SEMANTIC VIEW my_view AS
TABLES (
    o AS orders PRIMARY KEY (id)
)
DIMENSIONS (
    o.region AS o.region
)
METRICS (
    o.revenue AS SUM(o.amount)
)
MATERIALIZATIONS (
    daily_rev AS (
        TABLE catalog.schema.daily_revenue_agg,
        DIMENSIONS (region),
        METRICS (revenue)
    )
)
```

Design rationale:
- `name AS (...)` pattern is consistent with RELATIONSHIPS clause structure
- TABLE sub-keyword for the physical table reference (distinguished from TABLES clause)
- DIMENSIONS/METRICS sub-lists reference declared dimension/metric names (not expressions)
- Comma-separated entries for multiple materializations
- Parenthesized sub-clause body contains the three sub-clauses

### Anti-Patterns to Avoid

- **Validating materialization table existence at define time:** The table may not exist yet (e.g., user defines the semantic view first, then creates the aggregation table via dbt). Validation should happen at query time (Phase 55). [CITED: STATE.md "Materialization table existence: define-time vs query-time validation TBD" -- this research resolves the TBD in favor of query-time]
- **Adding materializations to the required clause set:** MATERIALIZATIONS must be optional -- the vast majority of semantic views will not have any. The `find_clause_bounds` validation requiring TABLES + at least one of DIMS/METRICS remains unchanged.
- **Putting materializations before METRICS in clause order:** Materializations reference dimension and metric names, so they must be parsed after DIMENSIONS and METRICS for validation to work in `parse_keyword_body`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Backward-compatible JSON deserialization | Custom version migration logic | `#[serde(default, skip_serializing_if = "Vec::is_empty")]` | Proven pattern used 15+ times in this codebase; old JSON without the field silently gets `vec![]` |
| YAML support for Materialization struct | Custom YAML parser | serde Serialize/Deserialize derives | yaml_serde uses the same serde framework; derives give YAML for free |
| "Did you mean?" suggestions | Custom string matching | `crate::util::suggest_closest()` | Already implemented using strsim Levenshtein; used across all validation |
| Clause keyword parsing | New parser approach | Extend existing `find_clause_bounds` + `split_at_depth0_commas` | The existing body parser handles nesting, quoting, and error reporting correctly |

**Key insight:** The body parser's depth-tracking comma splitter (`split_at_depth0_commas`) already handles nested parentheses correctly, which is exactly what the MATERIALIZATIONS clause needs (the sub-body contains parenthesized DIMENSIONS/METRICS lists).

## Common Pitfalls

### Pitfall 1: Clause Ordering Validation Off-By-One
**What goes wrong:** Adding a new keyword to CLAUSE_ORDER at the wrong index causes all existing DDL with earlier clauses to fail ordering validation.
**Why it happens:** `find_clause_bounds` uses array position for ordering comparison. Materializations must be at index 5 (after metrics at index 4).
**How to avoid:** Add "materializations" as the LAST entry in both CLAUSE_KEYWORDS and CLAUSE_ORDER.
**Warning signs:** Existing sqllogictest tests for keyword body parsing fail.

### Pitfall 2: KeywordBody Construction Missing New Field
**What goes wrong:** The `rewrite_ddl_keyword_body` function in `parse.rs` constructs `SemanticViewDefinition` manually (line 1117-1130). Forgetting to add `materializations` field means materializations silently vanish during the parse-serialize-deserialize pipeline.
**Why it happens:** The field list in `rewrite_ddl_keyword_body` is explicit, not a spread operator.
**How to avoid:** Add `materializations: keyword_body.materializations` to the `SemanticViewDefinition` construction at parse.rs line ~1117.
**Warning signs:** DDL round-trip test (render_create_ddl -> parse_keyword_body -> render_create_ddl) loses materializations.

### Pitfall 3: Sub-Clause Parsing Depth Confusion
**What goes wrong:** The MATERIALIZATIONS clause has nested parentheses: `mat_name AS (TABLE t, DIMENSIONS (d1, d2), METRICS (m1))`. The outer `(...)` of MATERIALIZATIONS is already stripped by `find_clause_bounds`. But within that content, each entry has its own `(...)` sub-body, and within THAT, DIMENSIONS and METRICS have `(...)` lists.
**Why it happens:** Three levels of nesting: clause body -> entry sub-body -> dim/metric list.
**How to avoid:** Use `split_at_depth0_commas` for the top-level entries (splitting materializations), then for each entry, extract the name and sub-body via `AS (...)` pattern matching, then within the sub-body, use a keyword scanner similar to find_clause_bounds but for TABLE/DIMENSIONS/METRICS sub-keywords.
**Warning signs:** Entries with multiple dimensions/metrics parse incorrectly; trailing commas cause crashes.

### Pitfall 4: YAML Field Naming Mismatch
**What goes wrong:** The serde field name in JSON is `materializations` but YAML users might expect a different key name (e.g., `materialization` singular).
**Why it happens:** serde defaults to the Rust field name.
**How to avoid:** Use the same name `materializations` in both JSON and YAML. The YAML schema already uses `dimensions` (plural), `metrics` (plural), `tables` (plural) -- consistent plural naming.
**Warning signs:** YAML definitions with materializations fail to deserialize.

### Pitfall 5: Empty Materializations Serialized in JSON
**What goes wrong:** If `skip_serializing_if = "Vec::is_empty"` is omitted, every stored definition (including pre-v0.7.0) gets a `"materializations":[]` field appended on re-serialization, bloating stored JSON.
**Why it happens:** Missing the skip_serializing_if annotation.
**How to avoid:** Always include `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
**Warning signs:** Stored JSON before/after round-trip comparison fails.

## Code Examples

### Materialization Struct Definition
```rust
// Source: [follows existing patterns in model.rs for TableRef, Fact, Metric]
/// A named materialization declaration mapping a pre-aggregated table
/// to the dimensions and metrics it covers.
///
/// At define time, only the dimension/metric name references are validated
/// (must match declared names). The TABLE is not validated for existence
/// (it may be created later by external tools like dbt).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Materialization {
    pub name: String,
    pub table: String,
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
}
```

### MATERIALIZATIONS Sub-Clause Parser
```rust
// Source: [follows parse_relationships_clause pattern in body_parser.rs]
pub(crate) fn parse_materializations_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<Materialization>, ParseError> {
    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();
    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        let mat = parse_single_materialization_entry(entry, entry_offset)?;
        result.push(mat);
    }
    Ok(result)
}
```

### DDL Syntax Example (SQL)
```sql
-- Single materialization
MATERIALIZATIONS (
    daily_rev AS (
        TABLE analytics.agg.daily_revenue,
        DIMENSIONS (region, date_dim),
        METRICS (revenue, order_count)
    )
)

-- Multiple materializations
MATERIALIZATIONS (
    daily_rev AS (
        TABLE daily_revenue_agg,
        DIMENSIONS (region, date_dim),
        METRICS (revenue, order_count)
    ),
    monthly_rev AS (
        TABLE monthly_revenue_agg,
        DIMENSIONS (region),
        METRICS (revenue)
    )
)
```

### YAML Syntax Example
```yaml
materializations:
  - name: daily_rev
    table: analytics.agg.daily_revenue
    dimensions:
      - region
      - date_dim
    metrics:
      - revenue
      - order_count
```

### DDL Reconstruction (render_ddl.rs)
```rust
// Source: [follows emit_facts pattern in render_ddl.rs]
fn emit_materializations(out: &mut String, def: &SemanticViewDefinition) {
    out.push_str("MATERIALIZATIONS (\n");
    for (i, mat) in def.materializations.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&mat.name);
        out.push_str(" AS (\n");
        out.push_str("        TABLE ");
        out.push_str(&mat.table);
        out.push_str(",\n");
        if !mat.dimensions.is_empty() {
            out.push_str("        DIMENSIONS (");
            out.push_str(&mat.dimensions.join(", "));
            out.push_str("),\n");
        }
        if !mat.metrics.is_empty() {
            out.push_str("        METRICS (");
            out.push_str(&mat.metrics.join(", "));
            out.push_str(")\n");
        }
        out.push_str("    )");
        if i + 1 < def.materializations.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(")\n");
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No materialization support | MATERIALIZATIONS clause (this phase) | v0.7.0 Phase 54 | Enables query-time routing in Phase 55 |
| Snowflake has no MATERIALIZATIONS clause | Custom extension (not Snowflake-aligned) | N/A | This is a novel feature -- Snowflake relies on engine optimization instead |

**Design note:** Snowflake semantic views do NOT have a MATERIALIZATIONS clause. [VERIFIED: Snowflake docs at docs.snowflake.com/en/sql-reference/sql/create-semantic-view]. This is a custom extension inspired by Cube.dev's pre-aggregation model, adapted for DuckDB's preprocessor architecture. The design doc at `_notes/semantic-views-duckdb-design-doc.md` explicitly describes the two-phase approach (expand + substitute) and Cube.dev's matching algorithm. [VERIFIED: design doc lines 119-160]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | TABLE sub-keyword in MATERIALIZATIONS entry uses unquoted table name | DDL Syntax | Low -- can add quoting support later if needed |
| A2 | Materialization names are case-insensitive for matching (consistent with dim/metric names) | Validation | Low -- case-insensitive is standard for SQL identifiers |
| A3 | Materialization table existence is NOT validated at define time | Anti-Patterns | Medium -- if user expects define-time table validation, this could be surprising; but dbt workflow requires it |
| A4 | MATERIALIZATIONS clause appears last (after METRICS) in clause order | Clause Ordering | Low -- natural ordering since it references dims/metrics |
| A5 | Each materialization must have at least one dimension or one metric | Validation | Low -- a materialization with neither is semantically meaningless |

## Open Questions (RESOLVED)

1. **Duplicate materialization names**
   - What we know: Dimension and metric names must be unique within their respective clauses.
   - What's unclear: Should materialization names be unique? Almost certainly yes.
   - Recommendation: Validate uniqueness at define time, same as dimension/metric names. Error message: "Duplicate materialization name 'X'."

2. **Empty DIMENSIONS or METRICS in a materialization**
   - What we know: A materialization must cover at least something to be useful.
   - What's unclear: Can a materialization have DIMENSIONS but no METRICS, or vice versa?
   - Recommendation: Allow either to be empty (but not both). A metrics-only materialization is valid for "totals" tables. A dimensions-only materialization is less useful but should not be prohibited.

3. **Materialization table name format**
   - What we know: The TABLE sub-clause needs to accept qualified names like `catalog.schema.table`.
   - What's unclear: Should we validate the format (e.g., at most 3 parts)?
   - Recommendation: Accept any string as the table name. The format validation will happen at query time when DuckDB tries to resolve it.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + proptest + sqllogictest-rs |
| Config file | justfile + Cargo.toml `[dev-dependencies]` |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MAT-01 | MATERIALIZATIONS clause parses correctly in SQL DDL | unit + sqllogictest | `cargo test materialization` / `just test-sql` | Wave 0 |
| MAT-06 | YAML definitions produce same representation | unit | `cargo test yaml_materialization` | Wave 0 |
| MAT-07 | Persistence with backward compat | unit + sqllogictest | `cargo test materialization` / `just test-sql` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase54_materializations.test` -- covers MAT-01, MAT-07 (DDL + persistence)
- [ ] Unit tests in body_parser.rs test module -- covers MAT-01 (parser)
- [ ] Unit tests in model.rs test module -- covers MAT-06, MAT-07 (serde round-trip)
- [ ] Unit tests in render_ddl.rs test module -- covers MAT-01 (DDL reconstruction)
- [ ] Round-trip test (render -> parse -> render) -- covers MAT-01 correctness

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A (materialization table access is DuckDB's concern) |
| V5 Input Validation | yes | Existing body parser depth tracking, size cap on YAML, parameterized persistence |
| V6 Cryptography | no | N/A |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via materialization table name | Tampering | Table name stored as data in JSON, not interpolated into SQL at define time. At query time (Phase 55), the table name will be quoted in generated SQL. |
| Oversized MATERIALIZATIONS clause | DoS | Existing YAML_SIZE_CAP (1 MiB) and body parser depth limits apply |

## Sources

### Primary (HIGH confidence)
- model.rs -- Existing struct patterns (TableRef, Fact, Metric, SemanticViewDefinition)
- body_parser.rs -- CLAUSE_KEYWORDS, CLAUSE_ORDER, parse functions, validation patterns
- render_ddl.rs -- DDL reconstruction patterns
- parse.rs -- rewrite_ddl_keyword_body SemanticViewDefinition construction
- catalog.rs -- Persistence pattern (JSON in semantic_layer._definitions)
- ddl/define.rs -- VTab bind-time validation flow

### Secondary (MEDIUM confidence)
- [Snowflake CREATE SEMANTIC VIEW docs](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- Confirmed no MATERIALIZATIONS clause exists in Snowflake
- _notes/semantic-views-duckdb-design-doc.md -- Pre-aggregation selection algorithm design

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies needed; all patterns exist in codebase
- Architecture: HIGH -- follows established clause addition pattern used 5 times previously
- Pitfalls: HIGH -- documented from direct codebase analysis of parser mechanics

**Research date:** 2026-04-19
**Valid until:** 2026-05-19 (stable -- no external dependency changes expected)
