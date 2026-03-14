# Phase 29: FACTS Clause & Hierarchies - Research

**Researched:** 2026-03-14
**Domain:** DDL parsing, expression inlining, define-time validation, metadata exposure
**Confidence:** HIGH

## Summary

Phase 29 adds two features to the semantic views extension: (1) a FACTS clause for declaring named row-level sub-expressions that metrics can reference, and (2) a HIERARCHIES clause for declaring drill-down paths as pure metadata. Both are low-to-medium complexity additions that follow established patterns in the codebase.

The FACTS implementation requires three touchpoints: body_parser.rs (new clause parsing), expand.rs (expression inlining before metric expansion), and graph.rs/define.rs (define-time validation of fact source tables, cycles, and non-existent references). The Fact struct already exists in model.rs (scaffolded in Phase 11) and is already a field on SemanticViewDefinition. The HIERARCHIES implementation requires a new Hierarchy struct in model.rs, clause parsing in body_parser.rs, validation in define.rs, and a new column in describe.rs output. Neither feature touches the graph, the FFI layer, the catalog, or the query table function.

**Primary recommendation:** Implement FACTS first (metrics depend on facts for expression inlining), then HIERARCHIES (pure metadata, zero interaction with expansion). Both follow the existing clause-parsing pattern exactly. The expression inlining for facts must use word-boundary-aware substitution (not naive string replace) to avoid substring collisions.

<user_constraints>
## User Constraints (from design preference)

### Locked Decisions
- Match Snowflake semantic views syntax and behavior wherever possible
- Only deviate from Snowflake where forced by DuckDB constraints or existing extension architecture

### Claude's Discretion
- Hierarchies syntax (Snowflake has no HIERARCHIES clause -- this is a differentiator borrowed from Cube.dev)
- Expression inlining strategy (implementation detail not specified by user)
- Fact validation rules (follow Snowflake's validation-rules page)

### Deferred Ideas (OUT OF SCOPE)
- Aggregate facts (COUNT in FACTS) -- explicitly listed in REQUIREMENTS.md Out of Scope
- PRIVATE/PUBLIC visibility modifiers -- no access control in DuckDB extensions
- WITH SYNONYMS -- AI/natural-language discovery not relevant for SQL-only DuckDB
- COMMENT on expressions -- no runtime effect, deferred
- Derived metrics (Phase 30)
- Semi-additive metrics (v0.5.4)
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FACT-01 | User can declare named row-level expressions in a FACTS clause (`alias.fact_name AS sql_expr`) | Snowflake syntax verified; Fact struct exists in model.rs; body_parser clause extension pattern established |
| FACT-02 | Metric expressions can reference fact names; expansion inlines the fact expression with parenthesization | Expansion inlining pattern documented; word-boundary substitution needed (STATE.md blocker) |
| FACT-03 | Facts can reference other facts; expansion resolves in topological order | Topological sort pattern exists in graph.rs (Kahn's algorithm); reusable for fact DAG |
| FACT-04 | Define-time validation rejects fact cycles and references to non-existent facts | Cycle detection via topological sort; source_table validation reuses check_source_tables_reachable pattern |
| FACT-05 | DESCRIBE SEMANTIC VIEW shows facts alongside dimensions and metrics | describe.rs already serializes JSON arrays for dimensions/metrics/filters/joins; add facts column |
| HIER-01 | User can declare drill-down paths in a HIERARCHIES clause (`name AS (dim1, dim2, dim3)`) | No Snowflake equivalent; syntax inspired by Cube.dev hierarchies; pure metadata |
| HIER-02 | Define-time validation rejects hierarchies referencing non-existent dimensions | Validation follows check_source_tables_reachable pattern; compare hierarchy levels against dimension names |
| HIER-03 | DESCRIBE SEMANTIC VIEW shows hierarchy definitions | Add hierarchies column to describe.rs output |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde/serde_json | 1.x | Fact/Hierarchy serialization | Already used for all model structs |
| strsim | 0.11.x | Fuzzy "did you mean" suggestions | Already used for dimension/metric/clause suggestions |
| proptest | 1.x | Property-based testing | Already used for parse/expand/output proptests |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| regex | (consider) | Word-boundary matching in fact inlining | If regex proves necessary for robust word-boundary substitution |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| regex for word-boundary | Manual char-boundary scan | regex adds a dependency; manual scan is zero-dependency but more code. Manual scan is preferred since the pattern is simple: check chars before/after match are not alphanumeric/underscore. |

**Installation:**
No new dependencies required. All needed crates are already in Cargo.toml.

## Architecture Patterns

### Modified Components
```
src/
  body_parser.rs  [MODIFY] -- add FACTS/HIERARCHIES clause parsing
  model.rs        [MODIFY] -- add Hierarchy struct, hierarchies field on SemanticViewDefinition
  expand.rs       [MODIFY] -- fact expression inlining before metric expansion
  graph.rs        [MODIFY] -- add fact source_table validation, fact cycle detection
  ddl/
    define.rs     [MODIFY] -- wire fact/hierarchy validation into bind()
    describe.rs   [MODIFY] -- add facts + hierarchies columns to DESCRIBE output
  parse.rs        [MODIFY] -- wire facts/hierarchies from KeywordBody into SemanticViewDefinition
```

### Pattern 1: Clause Extension in body_parser.rs
**What:** Adding FACTS and HIERARCHIES as new clauses to the keyword body parser.
**When to use:** Any new DDL clause keyword.
**Example:**
```rust
// 1. Extend CLAUSE_KEYWORDS
const CLAUSE_KEYWORDS: &[&str] = &[
    "tables", "relationships", "facts", "hierarchies", "dimensions", "metrics"
];

// 2. Extend CLAUSE_ORDER (Snowflake: FACTS before DIMENSIONS)
const CLAUSE_ORDER: &[&str] = &[
    "tables", "relationships", "facts", "hierarchies", "dimensions", "metrics"
];

// 3. FACTS parsing reuses parse_qualified_entries (same alias.name AS expr pattern)
// 4. HIERARCHIES needs a new parse function for: name AS (dim1, dim2, dim3)
```
Source: Existing body_parser.rs patterns at lines 18-24, 278-349.

### Pattern 2: Fact Expression Inlining in expand.rs
**What:** Before building the SELECT clause, scan each metric's `expr` for fact name references and replace them with the fact's expression (parenthesized).
**When to use:** FACT-02, FACT-03.
**Example:**
```rust
/// Inline fact expressions into a metric/fact expression string.
/// Uses word-boundary-aware substitution to avoid substring collisions.
/// Facts are resolved in topological order (leaf facts first).
fn inline_facts(expr: &str, facts: &[Fact]) -> String {
    let mut result = expr.to_string();
    // Process facts in topological order (dependencies first)
    for fact in facts {
        // Match qualified (alias.fact_name) and unqualified (fact_name)
        // using word-boundary checks (not naive .replace())
        let qualified = format!("{}.{}",
            fact.source_table.as_deref().unwrap_or(""), &fact.name);
        result = replace_word_boundary(&result, &qualified,
            &format!("({})", fact.expr));
        result = replace_word_boundary(&result, &fact.name,
            &format!("({})", fact.expr));
    }
    result
}

/// Replace `needle` in `haystack` only at word boundaries.
/// A word boundary means the character before and after the match
/// is NOT alphanumeric or underscore.
fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String {
    // Scan haystack for needle occurrences, check surrounding chars
    // ...
}
```
Source: STATE.md blocker note about word-boundary matching.

### Pattern 3: Topological Sort for Fact Dependencies
**What:** Facts can reference other facts. Before inlining, resolve dependencies in topological order.
**When to use:** FACT-03.
**Example:**
```rust
/// Topological sort of facts based on inter-fact references.
/// Returns facts in resolution order (leaf facts first).
/// Returns Err if a cycle is detected.
fn toposort_facts(facts: &[Fact]) -> Result<Vec<&Fact>, String> {
    // Build adjacency: for each fact, which other facts does its expr reference?
    // Use Kahn's algorithm (same pattern as RelationshipGraph::toposort)
    // Return in order where dependencies come before dependents
}
```
Source: graph.rs RelationshipGraph::toposort() at lines 90-143.

### Pattern 4: Hierarchy Parsing (New Syntax)
**What:** Hierarchies use a different syntax than dimensions/metrics: `name AS (dim1, dim2, dim3)`.
**When to use:** HIER-01.
**Example:**
```rust
/// Parse HIERARCHIES clause content.
/// Each entry: `name AS (dim1, dim2, dim3)` or `name AS (dim1, dim2, dim3)`
fn parse_hierarchies_clause(
    body: &str,
    base_offset: usize,
) -> Result<Vec<Hierarchy>, ParseError> {
    let entries = split_at_depth0_commas(body);
    let mut result = Vec::new();
    for (entry_start, entry) in entries {
        let entry_offset = base_offset + entry_start;
        // Find "AS" keyword
        // After AS, expect parenthesized comma-separated dimension names
        // Parse: name AS (dim1, dim2, dim3)
        result.push(parse_single_hierarchy(entry, entry_offset)?);
    }
    Ok(result)
}
```

### Anti-Patterns to Avoid
- **Naive string replace for fact inlining:** `expr.replace("net_price", ...)` will match `net_price_total` as a substring. Must use word-boundary-aware substitution.
- **Qualified-name-first matching without fallback:** When inlining `o.net_price`, must try qualified match first (`o.net_price`), then fall back to unqualified (`net_price`). But if a fact is named `net_price` and there is also a column `net_price`, the qualified form must take precedence.
- **Modifying the query table function:** Facts and hierarchies do NOT change the `semantic_view()` function signature. Facts are inlined during expansion; hierarchies are metadata-only.
- **Adding hierarchies to SQL expansion:** Hierarchies are PURE METADATA. They appear in DESCRIBE output only. They do NOT affect `expand()`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Topological sort for fact DAG | Custom DFS with visited set | Kahn's algorithm (same as graph.rs) | Kahn's naturally detects cycles via leftover nodes; proven pattern in this codebase |
| Clause keyword parsing | Custom tokenizer | Extend existing find_clause_bounds scanner | The scanner handles depth-tracking, string escaping, keyword suggestions already |
| "Did you mean?" suggestions | Custom string distance | strsim::levenshtein (already in dependencies) | Battle-tested, consistent with existing suggestion UX |
| JSON serialization of new structs | Manual JSON string building | serde_json derive macros | All existing structs use Serialize/Deserialize; same pattern |

**Key insight:** Phase 29 is almost entirely "extend existing patterns." The body_parser, model, expand, and describe modules all have established conventions. The only genuinely new code is the word-boundary substitution logic for fact inlining.

## Common Pitfalls

### Pitfall 1: Substring Collision in Fact Inlining
**What goes wrong:** Naive `str::replace("net_price", "(price * (1 - discount))")` also matches `net_price_total`, producing `(price * (1 - discount))_total`.
**Why it happens:** Metric expressions are free-form SQL strings where identifiers can share prefixes.
**How to avoid:** Use word-boundary-aware substitution. Check that the character before and after the match is not `[a-zA-Z0-9_]`. This is a manual char scan, not regex.
**Warning signs:** Tests pass with simple names but fail with names that are prefixes/suffixes of other names.

### Pitfall 2: Operator Precedence Lost During Inlining
**What goes wrong:** Fact `net_price AS price * (1 - discount)` inlined into `SUM(net_price + tax)` without parenthesization produces `SUM(price * (1 - discount) + tax)` -- correct here, but `SUM(net_price * 2)` becomes `SUM(price * (1 - discount) * 2)` which changes precedence if the fact expression has addition/subtraction.
**Why it happens:** Inlined expressions interact with the surrounding expression's operators.
**How to avoid:** Always parenthesize the inlined fact expression: replace `net_price` with `(price * (1 - discount))`. The outer parens protect operator precedence.
**Warning signs:** Arithmetic results differ from expected values when facts contain mixed operators.

### Pitfall 3: Fact Resolution Order Matters
**What goes wrong:** Fact A references fact B, but B is inlined after A. The expression for A still contains `B` as a symbol, not B's expanded expression.
**Why it happens:** Facts were processed in declaration order, not dependency order.
**How to avoid:** Topologically sort facts before inlining. Process leaf facts (no dependencies) first, then facts that depend on already-resolved facts.
**Warning signs:** Multi-level fact chains produce SQL with unresolved fact names.

### Pitfall 4: DESCRIBE Output Column Count Change
**What goes wrong:** Adding new columns to DESCRIBE SEMANTIC VIEW changes the column count from 6 to 8. Existing sqllogictest assertions on column count will break.
**Why it happens:** describe.rs declares fixed columns; test assertions match exact column count.
**How to avoid:** Update phase28_e2e.test DESCRIBE assertions to account for new columns. Use `query TTTTTTTT` (8 T's) instead of `query TTTTTT` (6 T's).
**Warning signs:** sqllogictest DESCRIBE tests fail with column count mismatch.

### Pitfall 5: KeywordBody Struct Must Include Facts and Hierarchies
**What goes wrong:** parse_keyword_body builds a KeywordBody but the new facts/hierarchies fields are not included. The rewrite_ddl_keyword_body function in parse.rs constructs SemanticViewDefinition from KeywordBody but does not wire facts/hierarchies.
**Why it happens:** Two separate wiring points: body_parser.rs produces KeywordBody, parse.rs consumes it to build SemanticViewDefinition.
**How to avoid:** Update both: (1) KeywordBody struct to include `facts: Vec<Fact>` and `hierarchies: Vec<Hierarchy>`, (2) rewrite_ddl_keyword_body to pass facts/hierarchies into SemanticViewDefinition.
**Warning signs:** DDL parses successfully but facts/hierarchies are empty in stored JSON.

### Pitfall 6: Clause Ordering Enforcement
**What goes wrong:** FACTS appears after DIMENSIONS in user DDL. The body parser rejects it because CLAUSE_ORDER enforces strict ordering.
**Why it happens:** Snowflake requires FACTS before DIMENSIONS. The CLAUSE_ORDER array enforces this.
**How to avoid:** Place "facts" and "hierarchies" between "relationships" and "dimensions" in CLAUSE_ORDER. Document the required ordering in error messages.
**Warning signs:** Valid DDL rejected with "clause out of order" error.

## Code Examples

### Example 1: Full DDL with FACTS and HIERARCHIES
```sql
-- Snowflake-aligned FACTS syntax
CREATE SEMANTIC VIEW sales_analysis AS
  TABLES (
    o AS orders PRIMARY KEY (id),
    li AS line_items PRIMARY KEY (id),
    c AS customers PRIMARY KEY (id)
  )
  RELATIONSHIPS (
    order_items AS o(id) REFERENCES li(order_id),
    order_customer AS o(customer_id) REFERENCES c
  )
  FACTS (
    li.net_price AS li.extended_price * (1 - li.discount),
    li.tax_amount AS li.net_price * li.tax_rate
  )
  HIERARCHIES (
    location AS (country, region, city)
  )
  DIMENSIONS (
    c.country AS c.country,
    c.region AS c.region,
    c.city AS c.city,
    o.order_date AS o.order_date
  )
  METRICS (
    o.total_revenue AS SUM(li.net_price),
    o.total_tax AS SUM(li.tax_amount)
  );
```

### Example 2: Expanded SQL (after fact inlining)
```sql
-- Query: dimensions=['region'], metrics=['total_revenue']
-- Fact net_price inlined into metric expression:
SELECT
    "c"."region" AS "region",
    SUM(("li"."extended_price" * (1 - "li"."discount"))) AS "total_revenue"
FROM "orders" AS "o"
LEFT JOIN "line_items" AS "li" ON "o"."id" = "li"."order_id"
LEFT JOIN "customers" AS "c" ON "o"."customer_id" = "c"."id"
GROUP BY
    1
```

### Example 3: Fact-referencing-fact (multi-level inlining)
```sql
-- Definition:
-- FACTS (
--   li.net_price AS li.extended_price * (1 - li.discount),
--   li.tax_amount AS li.net_price * li.tax_rate
-- )
-- METRICS (o.total_tax AS SUM(li.tax_amount))

-- Step 1: Resolve net_price (leaf fact, no dependencies)
--   net_price = li.extended_price * (1 - li.discount)

-- Step 2: Resolve tax_amount (depends on net_price)
--   tax_amount = (li.extended_price * (1 - li.discount)) * li.tax_rate
--   (net_price was already inlined and parenthesized)

-- Step 3: Inline into metric
--   SUM(li.tax_amount) -> SUM(((li.extended_price * (1 - li.discount)) * li.tax_rate))

-- Final expanded SQL:
SELECT SUM((("li"."extended_price" * (1 - "li"."discount")) * "li"."tax_rate")) AS "total_tax"
FROM "orders" AS "o"
LEFT JOIN "line_items" AS "li" ON "o"."id" = "li"."order_id"
GROUP BY 1
```

## Snowflake Alignment Analysis

### FACTS Clause -- Maps Cleanly
| Snowflake Feature | This Extension | Status |
|-------------------|----------------|--------|
| `alias.fact_name AS sql_expr` | Same syntax | Aligned |
| Facts are row-level (unaggregated) | Same -- facts are inlined into aggregate expressions | Aligned |
| Facts can reference other facts | Same -- resolved via topological sort | Aligned |
| Facts can reference dimensions | Not implemented -- facts only reference columns and other facts | **Deviation** (minor, simplification) |
| Aggregate facts (COUNT in FACTS) | Explicitly out of scope | **Deviation** (documented in REQUIREMENTS.md Out of Scope) |
| PRIVATE modifier | Not implemented | **Deviation** (no access control in DuckDB) |
| WITH SYNONYMS | Not implemented | **Deviation** (no AI/NLP layer) |
| COMMENT = '...' | Not implemented | **Deviation** (deferred, can add later) |

### HIERARCHIES Clause -- Extension Differentiator
| Feature | This Extension | Snowflake |
|---------|----------------|-----------|
| `name AS (dim1, dim2, dim3)` | Implemented | **No equivalent** |
| Pure metadata (no query impact) | Yes | N/A |
| Validation against dimensions | Yes -- all levels must reference declared dimensions | N/A |
| DESCRIBE output | Yes -- shown alongside dimensions/metrics | N/A |

**Snowflake does not have a HIERARCHIES clause.** This feature is borrowed from Cube.dev's hierarchy concept and is a differentiator for this extension. Cube.dev uses `hierarchies: { name: { levels: [dim1, dim2] } }` in JavaScript; this extension uses SQL DDL syntax `HIERARCHIES (name AS (dim1, dim2, dim3))`.

### Required Deviations (forced by DuckDB/architecture constraints)
1. **No PRIVATE/PUBLIC modifiers** -- DuckDB extensions have no access control mechanism
2. **No aggregate facts** -- This blurs the row-level boundary; aggregation belongs in METRICS (explicit design decision documented in REQUIREMENTS.md)
3. **No WITH SYNONYMS** -- No AI/natural-language discovery layer in DuckDB
4. **No COMMENT** -- Can be added later without breaking changes; deferred for simplicity

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Fact struct exists but unused | FACTS clause parsed, inlined, validated | Phase 29 (now) | Facts become usable; metrics can reference named sub-expressions |
| No hierarchy support | HIERARCHIES as pure metadata | Phase 29 (now) | BI tools can discover drill-down paths via DESCRIBE |
| 6-column DESCRIBE output | 8-column DESCRIBE output (add facts, hierarchies) | Phase 29 (now) | Breaking change to DESCRIBE schema -- existing tests must update |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (unit + proptest), sqllogictest, DuckLake CI |
| Config file | Cargo.toml, test/sql/TEST_LIST, justfile |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| FACT-01 | Parse FACTS clause entries | unit | `cargo test body_parser::tests::parse_facts` | Wave 0 |
| FACT-01 | Fact struct round-trip serialization | unit | `cargo test model::tests::fact_roundtrip` | Exists (model.rs) |
| FACT-02 | Fact expression inlined in metric | unit | `cargo test expand::tests::fact_inlining` | Wave 0 |
| FACT-02 | Word-boundary substitution correctness | unit | `cargo test expand::tests::word_boundary` | Wave 0 |
| FACT-03 | Multi-level fact resolution (topo sort) | unit | `cargo test expand::tests::fact_toposort` | Wave 0 |
| FACT-04 | Fact cycle rejection at define time | unit | `cargo test graph::tests::fact_cycle_detected` | Wave 0 |
| FACT-04 | Non-existent fact reference rejection | unit | `cargo test graph::tests::fact_unknown_ref` | Wave 0 |
| FACT-05 | DESCRIBE shows facts column | sqllogictest | `just test-sql` (phase29 test) | Wave 0 |
| HIER-01 | Parse HIERARCHIES clause entries | unit | `cargo test body_parser::tests::parse_hierarchies` | Wave 0 |
| HIER-02 | Hierarchy referencing non-existent dim rejected | unit | `cargo test graph::tests::hierarchy_unknown_dim` | Wave 0 |
| HIER-03 | DESCRIBE shows hierarchies column | sqllogictest | `just test-sql` (phase29 test) | Wave 0 |
| ALL | FACTS + HIERARCHIES DDL end-to-end | sqllogictest | `just test-sql` | Wave 0 |
| ALL | Adversarial FACTS/HIERARCHIES parsing | proptest | `cargo test parse_proptest` | Wave 0 |
| ALL | Fuzz FACTS clause parsing | fuzz | `cargo +nightly fuzz run fuzz_ddl_parse` | Extend existing |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase29_facts_hierarchies.test` -- end-to-end FACTS + HIERARCHIES DDL and query
- [ ] Unit tests for FACTS clause parsing in body_parser.rs
- [ ] Unit tests for HIERARCHIES clause parsing in body_parser.rs
- [ ] Unit tests for fact inlining (word-boundary, parenthesization, multi-level) in expand.rs
- [ ] Unit tests for fact cycle detection and unknown-fact-reference validation
- [ ] Unit tests for hierarchy validation (unknown dimension reference)
- [ ] Proptest extension for FACTS/HIERARCHIES clause parsing with adversarial input
- [ ] Fuzz target extension for FACTS clause in fuzz_ddl_parse.rs seed corpus

## Open Questions

1. **Should facts be inlined into dimension expressions too?**
   - What we know: Snowflake allows dimensions to reference facts. Our REQUIREMENTS.md only specifies metric-to-fact references (FACT-02).
   - What's unclear: Whether dimension-to-fact inlining is needed for Phase 29.
   - Recommendation: Implement metric-to-fact inlining only for Phase 29. Dimension-to-fact can be added later if needed. The inlining function is generic enough to apply to any expression string.

2. **Should the HIERARCHIES parentheses be required or optional?**
   - What we know: REQUIREMENTS.md says `name AS (dim1, dim2, dim3)` with parens.
   - What's unclear: Whether `name AS dim1, dim2, dim3` (without parens) is more natural SQL.
   - Recommendation: Require parentheses. They disambiguate hierarchy levels from subsequent comma-separated hierarchy entries. `location AS (country, region, city), time AS (year, month)` is unambiguous; without parens, the commas are ambiguous.

3. **Where should fact validation live?**
   - What we know: graph.rs handles relationship graph validation. Fact validation (cycles, unknown refs) is a separate DAG.
   - What's unclear: Should fact validation go in graph.rs or a new module?
   - Recommendation: Add fact validation functions to graph.rs (or a new `facts.rs` if the module grows large). The validation pattern is identical to graph cycle detection. Keep it simple in Phase 29; refactor to a separate module in Phase 30 if derived metric DAG validation makes the file too large.

## Sources

### Primary (HIGH confidence)
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- FACTS clause grammar, parameter syntax
- [Snowflake Semantic View SQL Examples](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- Worked FACTS examples, fact-to-fact references, metric-to-fact references
- [Snowflake Validation Rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- Expression reference hierarchy, cycle prevention, granularity rules
- [Snowflake Semantic View Overview](https://docs.snowflake.com/en/user-guide/views-semantic/overview) -- Expression hierarchy: facts are row-level, metrics aggregate facts
- Project source code: `src/model.rs` (Fact struct at line 55), `src/body_parser.rs` (clause parsing), `src/expand.rs` (expansion pipeline), `src/graph.rs` (validation), `src/ddl/describe.rs` (DESCRIBE output), `src/parse.rs` (DDL rewrite wiring)

### Secondary (MEDIUM confidence)
- [Cube.dev Hierarchies Reference](https://cube.dev/docs/product/data-modeling/reference/hierarchies) -- Hierarchy concept with levels array (inspiration for HIERARCHIES clause)
- `.planning/research/FEATURES.md` -- v0.5.3 feature landscape (T1: FACTS, D3: Hierarchies sections)
- `.planning/research/ARCHITECTURE.md` -- Component integration map, anti-patterns

### Tertiary (LOW confidence)
- None -- all findings verified against official docs or direct codebase analysis.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all patterns exist in codebase
- Architecture: HIGH -- direct analysis of body_parser.rs, expand.rs, model.rs, graph.rs, describe.rs, parse.rs
- Pitfalls: HIGH -- word-boundary collision identified in STATE.md; operator precedence is a known text-substitution concern; DESCRIBE column count change is mechanical

**Research date:** 2026-03-14
**Valid until:** 2026-04-14 (stable -- no external dependency changes expected)
