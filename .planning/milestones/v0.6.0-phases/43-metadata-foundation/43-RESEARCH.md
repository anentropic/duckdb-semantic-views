# Phase 43: Metadata Foundation - Research

**Researched:** 2026-04-10
**Domain:** Rust model/parser/serialization -- adding COMMENT, SYNONYMS, and PRIVATE/PUBLIC annotations to semantic view definitions
**Confidence:** HIGH

## Summary

Phase 43 adds three metadata annotation features to the semantic view model and DDL parser: COMMENT (string), SYNONYMS (string list), and PRIVATE/PUBLIC (enum). These are all "wide-and-shallow" changes -- they touch model structs, the body parser state machine, the parse.rs rewriter, and the expansion engine (for PRIVATE filtering), but none require deep architectural changes. The primary risk is backward compatibility: every new field in model.rs must use `#[serde(default)]` or old stored JSON will fail to deserialize, making the entire catalog inaccessible.

The DDL syntax follows Snowflake's conventions closely. COMMENT and SYNONYMS are trailing annotations on each object entry (after `AS expr`). PRIVATE/PUBLIC is a leading keyword before the `alias.name` portion of fact and metric entries. The view-level COMMENT sits between the view name and the `AS` keyword (adapting Snowflake's post-clause placement to this project's `AS`-prefixed body syntax).

**Primary recommendation:** Bundle all three metadata features (COMMENT, SYNONYMS, PRIVATE/PUBLIC) into a single phase because they touch the same files and share the same model/parser/serde patterns. The PRIVATE exclusion logic in the expansion engine is the only query-time behavior change; all other changes are parse-time and storage-only.

## Project Constraints (from CLAUDE.md)

- Quality gate: `just test-all` (Rust unit + proptest + sqllogictest + DuckLake CI)
- `cargo test` alone is incomplete -- sqllogictest covers DDL -> query integration paths
- `just test-sql` requires a fresh `just build` to pick up code changes
- If in doubt about SQL syntax or behaviour, refer to what Snowflake semantic views does

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| META-01 | User can add COMMENT = '...' to a semantic view in CREATE DDL | View-level comment field on SemanticViewDefinition + parse.rs pre-AS extraction |
| META-02 | User can add COMMENT = '...' to individual tables, dimensions, metrics, and facts in DDL | Comment field on TableRef, Dimension, Metric, Fact + body_parser trailing annotation parsing |
| META-03 | User can add WITH SYNONYMS = ('alias1', 'alias2') to tables, dimensions, metrics, and facts | Synonyms Vec<String> field on all 4 structs + body_parser trailing annotation parsing |
| META-04 | User can mark facts and metrics as PRIVATE or PUBLIC (default PUBLIC) | AccessModifier enum + leading keyword parsing in parse_single_metric_entry and parse_single_qualified_entry |
| META-05 | PRIVATE facts/metrics are hidden from query results but usable in derived metric/dimension expressions | Access check in expand/sql_gen.rs after find_metric/find_dimension resolution |
| META-06 | All metadata fields persist across restarts with backward-compatible JSON deserialization | `#[serde(default)]` on all new fields + roundtrip tests |
| META-07 | Pre-v0.6.0 stored views load without error (all new fields default to empty/PUBLIC) | `#[serde(default)]` ensures missing fields get default values; explicit test with old JSON format |
</phase_requirements>

## Standard Stack

### Core

No new library dependencies. Phase 43 uses only existing crate dependencies.

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde / serde_json | (existing) | Model serialization/deserialization | Already used for all model types |
| strsim | (existing) | "Did you mean?" fuzzy matching | Already used in body_parser.rs |
| proptest | (existing) | Property-based testing | Already used for parser and type tests |

### Alternatives Considered

None -- no new libraries needed.

## Architecture Patterns

### Pattern 1: Trailing Annotation Parsing (COMMENT, SYNONYMS)

**What:** After parsing an entry's core content (e.g., `alias.name AS expr`), the body parser scans for trailing annotation keywords: `COMMENT = '...'` and `WITH SYNONYMS = ('...', '...')`. These are optional and position-independent relative to each other but must follow the `AS expr` portion. [VERIFIED: Snowflake docs show this ordering]

**When to use:** Every TABLES, FACTS, DIMENSIONS, and METRICS entry.

**Implementation approach:**

The current entry parsers (`parse_single_table_entry`, `parse_single_qualified_entry`, `parse_single_metric_entry`) each return raw tuples. The trailing annotations must be parsed AFTER the core content is extracted but BEFORE the result is returned.

The `split_at_depth0_commas` function already correctly handles single-quoted strings and nested parens, so `COMMENT = 'text with commas, yes'` inside an entry will NOT cause a false split as long as it's within the entry boundaries. However, an entry like:

```sql
o.revenue AS SUM(o.amount) COMMENT = 'Total revenue'
```

The `, ` between entries is at depth-0. Within the entry, once `AS` is found, everything from `AS` to the next depth-0 comma is the entry. The challenge is separating the expression from the trailing annotations. [VERIFIED: codebase - split_at_depth0_commas at body_parser.rs:46]

**Approach for expression/annotation separation:**

For dimensions/facts (which use `parse_qualified_entries` -> `parse_single_qualified_entry`): After finding `AS`, take everything after `AS` as a candidate. Scan backward from the end for known trailing keywords (`COMMENT`, `WITH`). This is similar to how `USING` is already parsed for metrics.

A more robust approach: After finding `AS`, scan forward through the expression using the same depth-tracking logic (respecting parens and strings), and then look for `COMMENT` or `WITH` at depth-0 as word boundaries after the expression.

**Recommended pattern:** Extract trailing annotations by scanning for the LAST occurrence of `COMMENT` and `WITH SYNONYMS` keywords at depth-0 outside of parenthesized/quoted contexts. Return a structured result that includes the annotations.

```rust
// New return type for entry parsers
struct ParsedEntry {
    source_alias: String,
    bare_name: String,
    expr: String,
    comment: Option<String>,
    synonyms: Vec<String>,
}
```

### Pattern 2: Leading Keyword Parsing (PRIVATE/PUBLIC)

**What:** Before the `alias.name` portion of a FACTS or METRICS entry, an optional `PRIVATE` or `PUBLIC` keyword may appear. PUBLIC is the default and need not be specified. [VERIFIED: Snowflake docs - PRIVATE/PUBLIC precedes alias.name]

**When to use:** FACTS and METRICS clause entries only. Dimensions are always PUBLIC per Snowflake's rules.

**Implementation approach:**

In `parse_single_metric_entry` and `parse_single_qualified_entry` (when called for FACTS), check if the first token is `PRIVATE` or `PUBLIC`. If so, consume it and proceed with the rest.

```rust
// At start of entry parsing:
let (access, remaining) = if entry_upper.starts_with("PRIVATE ") {
    (AccessModifier::Private, &entry["PRIVATE ".len()..])
} else if entry_upper.starts_with("PUBLIC ") {
    (AccessModifier::Public, &entry["PUBLIC ".len()..])
} else {
    (AccessModifier::Public, entry)
};
```

### Pattern 3: View-Level Comment Parsing

**What:** The view-level COMMENT sits between the view name and the `AS` keyword in this project's DDL syntax. [ASSUMED -- adapting Snowflake's post-clause placement to the existing AS-keyword body structure]

**Syntax:**
```sql
CREATE SEMANTIC VIEW my_view
  COMMENT = 'Revenue analysis view'
  AS
  TABLES (...)
  DIMENSIONS (...)
  METRICS (...)
```

**Implementation:** In `parse.rs::validate_create_body`, after extracting the view name but before detecting the `AS` keyword, check for `COMMENT = '...'`. Extract the comment string, then proceed to find `AS`.

**Alternative considered:** Placing the view-level COMMENT after the last clause (inside the AS body). This was rejected because:
1. The AS body is parsed by `body_parser.rs::find_clause_bounds`, which expects only clause keywords (TABLES, RELATIONSHIPS, etc.) at the top level. Adding COMMENT as a non-clause keyword would complicate the state machine.
2. Placing COMMENT between name and AS is cleaner -- it's clearly a view-level property, not a clause-level property.

### Pattern 4: Serde Backward Compatibility

**What:** Every new model field MUST use `#[serde(default)]` so old stored JSON deserializes correctly. New fields should also use `skip_serializing_if` to keep JSON output clean. [VERIFIED: codebase pattern - model.rs uses this consistently on all optional fields]

**Example:**
```rust
// On SemanticViewDefinition:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub comment: Option<String>,

// On Dimension, Metric, Fact, TableRef:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub comment: Option<String>,

#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub synonyms: Vec<String>,

// On Metric, Fact:
#[serde(default, skip_serializing_if = "AccessModifier::is_default")]
pub access: AccessModifier,
```

### Pattern 5: PRIVATE Query-Time Enforcement

**What:** When a user requests a PRIVATE metric or fact via `semantic_view('view', metrics := ['private_metric'])`, the expansion engine must reject the request with a clear error. However, PRIVATE metrics/facts remain usable as building blocks for derived metrics. [VERIFIED: Snowflake docs -- private items cannot be queried directly but can be referenced by other metrics]

**Implementation location:** `expand/sql_gen.rs::expand()` -- immediately after `find_metric` resolves successfully (line ~69-79), add an access check:

```rust
let met = find_metric(def, name).ok_or_else(|| { ... })?;
// NEW: Access check
if met.access == AccessModifier::Private {
    return Err(ExpandError::PrivateMetric {
        view_name: view_name.to_string(),
        name: name.clone(),
    });
}
```

The `inline_derived_metrics` function (which resolves derived metric expressions) does NOT need modification -- it operates on ALL metrics regardless of access modifier, which is the correct behavior (derived metrics can reference private base metrics).

### Anti-Patterns to Avoid

- **Forgetting `#[serde(default)]` on any new field:** A single missing annotation renders the entire catalog inaccessible on upgrade. This is the #1 risk for this phase. [VERIFIED: STATE.md lists this as a blocker/concern]
- **Parsing COMMENT inside expression context:** `COMMENT` could appear as a SQL identifier in an expression (e.g., `CASE WHEN comment_count > 0 THEN ...`). The parser must only recognize `COMMENT` as an annotation keyword at the top level of an entry, after the expression, using word-boundary matching.
- **Modifying `find_clause_bounds` for view-level COMMENT:** The clause boundary scanner expects only the 5 known keywords. Adding COMMENT there would break the clean state machine. View-level COMMENT must be handled in `parse.rs`, NOT `body_parser.rs`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON backward compat | Custom migration/versioning code | serde `#[serde(default)]` attribute | Proven pattern throughout codebase; migration code is brittle |
| Fuzzy keyword suggestion | Custom edit distance | strsim crate (already a dependency) | Already used for "did you mean?" in body_parser.rs |
| Enum default values | Manual Default impl | `#[derive(Default)]` with `#[default]` variant | Same pattern as Cardinality enum in model.rs |

## Common Pitfalls

### Pitfall 1: Backward Compatibility Regression
**What goes wrong:** A new field added without `#[serde(default)]` causes deserialization failure for all pre-v0.6.0 stored views, making the catalog completely inaccessible.
**Why it happens:** Easy to forget the annotation when adding multiple fields across multiple structs.
**How to avoid:** Add explicit unit tests that deserialize old-format JSON (without the new fields) for EVERY struct that gets new fields (TableRef, Dimension, Metric, Fact, SemanticViewDefinition).
**Warning signs:** `cargo test` passes but `just test-sql` fails on phase42_persistence.test -- the restart test.

### Pitfall 2: COMMENT Inside Expression False Match
**What goes wrong:** An expression like `CASE WHEN comment IS NOT NULL THEN 1 ELSE 0 END` is incorrectly parsed -- the parser thinks `comment` is the COMMENT annotation keyword.
**Why it happens:** Naive keyword matching without proper context awareness.
**How to avoid:** Only scan for COMMENT/WITH SYNONYMS AFTER the expression has been fully extracted. Use the existing depth-tracking pattern (parens/quotes) to find the expression boundary first.
**Warning signs:** Tests with expressions containing `comment` as an identifier fail to parse.

### Pitfall 3: PRIVATE Keyword Ambiguity with Table Aliases
**What goes wrong:** If a user has a table alias named `PRIVATE` (unlikely but possible), `PRIVATE.metric_name AS ...` could be misinterpreted as the PRIVATE access modifier followed by `.metric_name`.
**Why it happens:** Parsing `PRIVATE` as a leading keyword without checking what follows.
**How to avoid:** After consuming `PRIVATE` or `PUBLIC`, verify the next token is a qualified name (contains a `.`). If PRIVATE is followed by `.`, it's a table alias, not an access modifier. This is the same disambiguation pattern used for the `USING` keyword in metric parsing.
**Warning signs:** Tests with table aliases like `private_table.metric AS ...` fail.

### Pitfall 4: Single-Quote Escaping in COMMENT Strings
**What goes wrong:** `COMMENT = 'It''s a test'` fails to parse because escaped single quotes are not handled.
**Why it happens:** Naive string extraction that stops at the first `'` instead of handling `''` escaping.
**How to avoid:** Use the same escaped-quote handling already present in `split_at_depth0_commas` (body_parser.rs:56-60). The `''` escape convention is already supported for expression parsing.
**Warning signs:** Tests with comments containing apostrophes fail.

### Pitfall 5: JSON Rewrite Path Must Include New Fields
**What goes wrong:** The `rewrite_ddl_keyword_body` function in parse.rs constructs `SemanticViewDefinition` from `KeywordBody`. If `KeywordBody` doesn't carry comment/synonyms/access data, the information is lost during the parse-to-JSON-to-function-call bridge.
**Why it happens:** The `KeywordBody` struct is an intermediate representation. New fields must be added to both `KeywordBody` and the `SemanticViewDefinition` construction in `rewrite_ddl_keyword_body`.
**How to avoid:** Update the `KeywordBody` struct to carry all metadata. Update the mapping code in `rewrite_ddl_keyword_body` (parse.rs:841-853).
**Warning signs:** Metadata appears to parse successfully but is not present after CREATE.

## Code Examples

### Model Changes

```rust
// Source: Existing pattern in model.rs (verified from codebase)

/// Access modifier for facts and metrics.
/// Default is Public -- private items cannot be queried directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum AccessModifier {
    #[default]
    Public,
    Private,
}

impl AccessModifier {
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Public)
    }
}

// Added to SemanticViewDefinition:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub comment: Option<String>,

// Added to TableRef, Dimension, Metric, Fact:
#[serde(default, skip_serializing_if = "Option::is_none")]
pub comment: Option<String>,

#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub synonyms: Vec<String>,

// Added to Metric and Fact only:
#[serde(default, skip_serializing_if = "AccessModifier::is_default")]
pub access: AccessModifier,
```

### DDL Syntax Examples

```sql
-- Source: Snowflake CREATE SEMANTIC VIEW docs (adapted for this project's AS-body syntax)

-- View-level comment:
CREATE SEMANTIC VIEW revenue_analysis
  COMMENT = 'Revenue analysis view for Q1 2026'
  AS
  TABLES (
    o AS orders PRIMARY KEY (order_id) COMMENT = 'All customer orders' WITH SYNONYMS = ('sales_orders', 'purchase_records'),
    c AS customers PRIMARY KEY (customer_id) COMMENT = 'Customer master data'
  )
  FACTS (
    o.amount_fact AS o.amount COMMENT = 'Raw order amount' WITH SYNONYMS = ('order_amount'),
    PRIVATE o.internal_margin AS o.amount - o.cost
  )
  DIMENSIONS (
    o.order_date AS o.created_at COMMENT = 'Date the order was placed' WITH SYNONYMS = ('purchase_date'),
    c.customer_name AS c.name WITH SYNONYMS = ('buyer_name', 'client_name')
  )
  METRICS (
    o.total_revenue AS SUM(o.amount) COMMENT = 'Total revenue across all orders',
    PRIVATE o.total_cost AS SUM(o.cost),
    net_profit AS total_revenue - total_cost COMMENT = 'Revenue minus cost'
  )
```

### Backward Compatibility Test

```rust
// Source: Existing pattern in model.rs tests (verified from codebase)

#[test]
fn pre_v060_json_deserializes_with_defaults() {
    // JSON from v0.5.5 -- no comment, synonyms, or access fields
    let json = r#"{
        "base_table": "orders",
        "tables": [{"alias": "o", "table": "orders", "pk_columns": ["id"]}],
        "dimensions": [{"name": "region", "expr": "region", "source_table": "o"}],
        "metrics": [{"name": "revenue", "expr": "SUM(amount)", "source_table": "o"}],
        "facts": [{"name": "amount", "expr": "amount", "source_table": "o"}],
        "created_on": "2026-04-01T00:00:00Z",
        "database_name": "memory",
        "schema_name": "main"
    }"#;
    let def = SemanticViewDefinition::from_json("orders", json).unwrap();
    
    // All new fields should have defaults
    assert!(def.comment.is_none());
    for dim in &def.dimensions {
        assert!(dim.comment.is_none());
        assert!(dim.synonyms.is_empty());
    }
    for met in &def.metrics {
        assert!(met.comment.is_none());
        assert!(met.synonyms.is_empty());
        assert_eq!(met.access, AccessModifier::Public);
    }
    for fact in &def.facts {
        assert!(fact.comment.is_none());
        assert!(fact.synonyms.is_empty());
        assert_eq!(fact.access, AccessModifier::Public);
    }
}
```

### Expansion Error for PRIVATE Metrics

```rust
// Source: Existing ExpandError pattern in expand/types.rs (verified from codebase)

// New variant:
PrivateMetric {
    view_name: String,
    name: String,
},

// Display impl:
ExpandError::PrivateMetric { view_name, name } => {
    write!(f, "Metric '{name}' in view '{view_name}' is private and cannot be queried directly. \
               Private metrics can only be used in derived metric expressions.")
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No metadata annotations | COMMENT, SYNONYMS, PRIVATE/PUBLIC on all objects | v0.6.0 (this phase) | Self-documenting semantic layer |
| All facts/metrics publicly queryable | PRIVATE modifier hides internal building blocks | v0.6.0 (this phase) | Cleaner user-facing API, encapsulation |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | View-level COMMENT goes between view name and AS keyword | Architecture Pattern 3 | Parser change location is different; moderate rework. Snowflake puts it after all clauses, but this project uses AS-body which changes the grammar. |
| A2 | PRIVATE keyword is NOT supported on dimensions (Snowflake rule) | Architecture Pattern 2 | If user wants private dimensions, the parser needs to support it for DIMENSIONS clause too -- minor additional work. |
| A3 | Synonyms are pure metadata with no query-time resolution | Don't Hand-Roll | If synonyms should resolve to their canonical names at query time, expansion engine needs changes -- significant additional work. Snowflake explicitly says "informational purposes only." |

## Open Questions (RESOLVED)

1. **View-level COMMENT position in DDL syntax**
   - What we know: Snowflake puts view-level COMMENT after all clauses. This project uses `AS` before the body, which Snowflake does not.
   - What's unclear: Whether COMMENT should go before `AS` (between name and AS) or after all clauses (inside/after the AS body).
   - RESOLVED: Place between view name and AS. This is cleaner for the parser and aligns with the conceptual level (view-level property, not body-level property). The alternative (after last clause) requires modifying find_clause_bounds.

2. **Should PRIVATE apply to dimensions?**
   - What we know: Snowflake says "PUBLIC is the only supported access_modifier for DIMENSIONS."
   - What's unclear: Whether we should allow PRIVATE on dimensions anyway for forward compatibility.
   - RESOLVED: Follow Snowflake -- dimensions are always PUBLIC. If someone writes `PRIVATE o.date_dim AS ...` in DIMENSIONS, emit a parse error.

3. **Should COMMENT and SYNONYMS ordering be flexible or fixed?**
   - What we know: Snowflake allows both `COMMENT = '...' WITH SYNONYMS = (...)` and `WITH SYNONYMS = (...) COMMENT = '...'` -- they are independent trailing annotations.
   - What's unclear: Whether to enforce a fixed order or allow either order.
   - RESOLVED: Allow either order. The parser should scan for both keywords in any relative position after the expression.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | proptest + sqllogictest + cargo test |
| Config file | Cargo.toml (proptest), Makefile (sqllogictest) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| META-01 | View-level COMMENT in CREATE DDL | unit + sqllogictest | `cargo test model::tests` + `just test-sql` | Wave 0 |
| META-02 | Object-level COMMENT in CREATE DDL | unit + sqllogictest | `cargo test body_parser::tests` + `just test-sql` | Wave 0 |
| META-03 | WITH SYNONYMS on all objects | unit + sqllogictest | `cargo test body_parser::tests` + `just test-sql` | Wave 0 |
| META-04 | PRIVATE/PUBLIC on facts and metrics | unit + sqllogictest | `cargo test body_parser::tests` + `just test-sql` | Wave 0 |
| META-05 | PRIVATE exclusion at query time | unit | `cargo test expand::` | Wave 0 |
| META-06 | Persist and deserialize correctly | unit + sqllogictest | `cargo test model::tests` + restart test in sqllogictest | Wave 0 |
| META-07 | Pre-v0.6.0 JSON backward compat | unit | `cargo test model::tests::pre_v060` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase43_metadata.test` -- new sqllogictest for COMMENT/SYNONYMS/PRIVATE DDL
- [ ] Backward compat unit tests in model.rs for pre-v0.6.0 JSON
- [ ] Expansion unit tests for PRIVATE rejection in expand/sql_gen.rs tests

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | N/A |
| V3 Session Management | No | N/A |
| V4 Access Control | Tangential | PRIVATE modifier is visibility, not access control. DuckDB has no RBAC. |
| V5 Input Validation | Yes | Body parser validates COMMENT string is properly quoted; SQL injection surface mitigated by parameterized persistence (existing pattern). |
| V6 Cryptography | No | N/A |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via COMMENT string | Tampering | Parameterized prepared statements (already in place via persist.rs) |
| Denial via very long COMMENT | Availability | No explicit limit needed -- DuckDB handles storage limits; COMMENT is stored as JSON string in a VARCHAR column |

## Sources

### Primary (HIGH confidence)
- Snowflake CREATE SEMANTIC VIEW docs: https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view -- verified COMMENT, SYNONYMS, PRIVATE/PUBLIC syntax and positioning
- Codebase: model.rs, body_parser.rs, parse.rs, expand/sql_gen.rs, ddl/describe.rs -- verified current model structure, parser patterns, serde conventions, expansion flow

### Secondary (MEDIUM confidence)
- Snowflake DESCRIBE SEMANTIC VIEW docs: https://docs.snowflake.com/en/sql-reference/sql/desc-semantic-view -- ACCESS_MODIFIER, COMMENT, SYNONYMS property output

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing patterns
- Architecture: HIGH -- clear extension points in parser and model; well-understood codebase patterns
- Pitfalls: HIGH -- backward compat risk is well-known and explicitly documented in STATE.md

**Research date:** 2026-04-10
**Valid until:** 2026-05-10 (stable domain, no external dependency changes expected)
