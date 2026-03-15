# Phase 32: Role-Playing Dimensions & USING RELATIONSHIPS - Research

**Researched:** 2026-03-14
**Domain:** Multi-path join resolution, role-playing dimensions, USING RELATIONSHIPS DDL/expansion
**Confidence:** HIGH

## Summary

Phase 32 relaxes the current diamond-rejection invariant in `graph.rs` to allow multiple named relationships between the same table pair, adds USING clause parsing to metrics in `body_parser.rs`, extends the expansion engine to generate relationship-scoped aliases and separate JOINs per relationship, and produces clear errors when ambiguous multi-path tables are queried without USING disambiguation.

The design follows Snowflake's semantic view approach: USING is declared on metrics at DDL time (not at query time), dimensions inherit relationship context from co-queried metrics, and ambiguous paths without USING produce define-time or query-time errors. Snowflake only supports USING on base metrics (not derived metrics), and derived metrics inherit the relationship paths of their referenced base metrics. This project should match that behavior.

**Primary recommendation:** Implement USING as a metric-level DDL annotation stored in the `Metric` model, relax diamond detection to allow named multi-path relationships, generate relationship-scoped aliases in expansion (`{alias}__{rel_name}`), and add ambiguity detection at query-time expansion.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| JOIN-01 | Multiple named relationships between same table pair accepted (diamond relaxed) | Diamond relaxation in `check_no_diamonds()` -- skip named multi-path relationships, keep rejecting unnamed diamonds |
| JOIN-02 | Metrics can declare `USING (relationship_name)` to select specific join path | USING clause parsing in body_parser metrics, stored in Metric model `using_relationships` field |
| JOIN-03 | Expansion generates separate JOINs with relationship-scoped aliases when USING is specified | Alias generation pattern: `{to_alias}__{rel_name}`, separate JOIN per relationship with scoped ON clause |
| JOIN-04 | Define-time validation rejects USING references to non-existent relationships | New validation in `graph.rs` checking metric USING names against `Join.name` values |
| JOIN-05 | Querying dimension from ambiguous multi-path table without USING produces clear error | Ambiguity detection in `expand.rs` at query time when dimension source_table has multiple inbound relationships |
| ROLE-01 | Same physical table joined via different named relationships produces distinct aliases | Alias pattern `{to_alias}__{rel_name}` ensures uniqueness per relationship |
| ROLE-02 | Dimensions from role-playing table resolve to correct alias based on co-queried metric's USING | Dimension-to-relationship resolution via metric USING context during expansion |
| ROLE-03 | Classic role-playing pattern works end-to-end (flights/airports) | sqllogictest with flights/airports tables, two relationships, two metrics with USING |
</phase_requirements>

## Architecture Patterns

### Current State (What Exists)

The relationship graph (`src/graph.rs`) validates a tree structure:
- `RelationshipGraph::from_definition()` builds adjacency lists from `Join` entries
- `check_no_diamonds()` rejects any node with >1 parent in `reverse` map
- `validate_graph()` calls: cycle detection -> diamond detection -> orphan detection -> FK/PK count -> source table reachability

The expansion engine (`src/expand.rs`):
- `resolve_joins_pkfk()` walks reverse edges to find needed aliases, returns them in topological order
- Each alias maps to exactly one physical table via `def.tables`
- JOIN generation: `LEFT JOIN "physical_table" AS "alias" ON synthesize_on_clause(...)`

The body parser (`src/body_parser.rs`):
- `parse_metrics_clause()` returns `Vec<(Option<String>, String, String)>` -- (source_alias, name, expr)
- No USING parsing exists yet

The model (`src/model.rs`):
- `Metric` struct has: name, expr, source_table, output_type
- `Join` struct has: table (to_alias), from_alias, fk_columns, name (Option), cardinality
- `Join.name` is already `Some(rel_name)` for all Phase 24+ relationships

### Required Changes

#### 1. Model Changes (`model.rs`)

Add `using_relationships` field to `Metric`:

```rust
/// Phase 32: Relationships to use for join path disambiguation.
/// Only valid for base metrics (source_table.is_some()).
/// Empty = no explicit path (uses default single-path resolution).
/// Serialized only when non-empty for backward compat.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub using_relationships: Vec<String>,
```

#### 2. Parser Changes (`body_parser.rs`)

Extend `parse_single_metric_entry()` to detect `USING (...)` between the name and `AS`:

```
alias.metric_name USING (rel_name1, rel_name2) AS sql_expr
```

The grammar becomes:
```
metric_entry ::= qualified_name [ USING "(" relationship_list ")" ] AS expr
               | bare_name AS expr          -- derived metric (no USING allowed)
```

Key detail: Find `USING` keyword (case-insensitive, word boundary) between the qualified name and `AS`. Extract parenthesized comma-separated relationship names. Derived metrics (no dot prefix) MUST NOT have USING (Snowflake constraint).

Return type changes from `(Option<String>, String, String)` to `(Option<String>, String, String, Vec<String>)` adding the using_relationships list.

#### 3. Diamond Relaxation (`graph.rs`)

Modify `check_no_diamonds()` to allow multi-parent nodes ONLY when ALL edges to that node are from named relationships. The key insight: in a valid role-playing setup, the same target alias (e.g., "airports" aliased as "a") is referenced by multiple named relationships from different source aliases, but the graph still treats it as one node with multiple parents.

However, the better approach is: **do not relax the graph structure at all**. Instead, recognize that role-playing relationships produce the same `to_alias` but the expansion engine generates DISTINCT aliases per relationship. The diamond check should allow multiple edges to the same `to_alias` when all relationships pointing to it have distinct names.

Updated `check_no_diamonds()`:
```rust
pub fn check_no_diamonds(&self, def: &SemanticViewDefinition) -> Result<(), String> {
    for (node, parents) in &self.reverse {
        if node != &self.root && parents.len() > 1 {
            // Check if all relationships to this node are distinctly named
            let rels_to_node: Vec<&Join> = def.joins.iter()
                .filter(|j| j.table.to_ascii_lowercase() == *node && !j.fk_columns.is_empty())
                .collect();
            let all_named = rels_to_node.iter().all(|j| j.name.is_some());
            let all_unique = /* check name uniqueness */;
            if !all_named || !all_unique {
                return Err(format!("diamond: two paths to '{}'...", node));
            }
            // Allow: all named relationships with distinct names = role-playing
        }
    }
    Ok(())
}
```

#### 4. USING Validation (`graph.rs`)

New `validate_using_relationships()` function:
- For each metric with non-empty `using_relationships`:
  - Each referenced name must exist in `def.joins` (match by `Join.name`)
  - Each referenced relationship must originate from the metric's `source_table` (Snowflake constraint: relationship must start from the metric's table)
  - Derived metrics (source_table is None) must NOT have USING
- Called from `define.rs` after existing validations

#### 5. Expansion Changes (`expand.rs`)

**Alias generation for role-playing joins:**

When a metric specifies USING, expansion must:
1. Generate a relationship-scoped alias: `{to_alias}__{rel_name}` (double underscore separator)
2. Emit a separate LEFT JOIN for each relationship-scoped alias using the relationship's specific FK columns
3. Rewrite dimension expressions to use the scoped alias instead of the bare alias

Example expansion for flights/airports:
```sql
SELECT
    "a__dep".city AS "departure_city",
    "a__arr".city AS "arrival_city",
    COUNT(*) AS "flight_count"
FROM "flights" AS "f"
LEFT JOIN "airports" AS "a__dep" ON "f"."departure_airport" = "a__dep"."airport_code"
LEFT JOIN "airports" AS "a__arr" ON "f"."arrival_airport" = "a__arr"."airport_code"
GROUP BY 1, 2
```

**Dimension-to-relationship resolution:**

At query time, when resolving which alias a dimension should use:
1. If the dimension's `source_table` has only one relationship path -> use it (no ambiguity)
2. If the dimension's `source_table` has multiple relationship paths -> look at co-queried metrics' USING clauses to determine which relationship-scoped alias to use
3. If no metric provides USING context for an ambiguous dimension -> return `ExpandError::AmbiguousPath`

**New ExpandError variant:**
```rust
AmbiguousPath {
    view_name: String,
    dimension_name: String,
    dimension_table: String,
    available_relationships: Vec<String>,
}
```

**Key design decision for dimension resolution:**

Snowflake's approach: dimensions don't have their own USING clause. Instead, when a query includes metrics that specify USING, the dimension resolves based on the metric's relationship context. If two metrics use different relationships to the same role-playing table, the dimensions from that table appear once per relationship (each scoped to the metric's USING).

For this project, the simpler and more deterministic approach:
- If a dimension's source table is reached by exactly one relationship -> unambiguous, use it
- If a dimension's source table is reached by multiple relationships -> the dimension is ambiguous
- Ambiguous dimensions require a co-queried metric with USING that selects ONE path to that table
- If multiple metrics use different USING paths to the same table, each metric gets its own scoped alias; dimensions from that table can only be queried when a single USING path is in effect

#### 6. Join Resolution Changes (`expand.rs`)

`resolve_joins_pkfk()` must be extended to handle USING:
- When a metric specifies USING, include those specific relationships (generating scoped aliases)
- When a dimension comes from a role-playing table, use the USING-scoped alias
- Fan trap detection must also account for USING-scoped paths

### Recommended Project Structure for Changes

```
src/
  model.rs          # Add using_relationships to Metric
  body_parser.rs    # Parse USING clause in metrics
  graph.rs          # Relax diamond check, add validate_using_relationships
  expand.rs         # Scoped aliases, ambiguity detection, USING-aware join resolution
  ddl/define.rs     # Call validate_using_relationships
test/
  sql/phase32_role_playing.test  # End-to-end sqllogictest
```

### Anti-Patterns to Avoid

- **Generating aliases at parse time:** Aliases must be generated at expansion time because the same definition may be queried with different dimension/metric combinations
- **Storing scoped aliases in the model:** The model stores logical relationships; expansion generates physical aliases
- **Allowing USING on derived metrics:** Snowflake prohibits this and for good reason -- derived metrics inherit paths from their base metrics
- **Query-time USING:** Snowflake defines USING at DDL time on metrics, not at query time. Follow this pattern.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Ambiguity detection | Custom relationship counting | Walk `graph.reverse` for multi-parent nodes | Graph already tracks parents |
| Alias uniqueness | Manual string concatenation | `format!("{to_alias}__{rel_name}")` with consistent separator | Double underscore unlikely in user aliases |
| USING parsing | New tokenizer | Extend existing `parse_single_metric_entry` with keyword detection | Reuse existing depth-0 comma splitting and keyword finding |

## Common Pitfalls

### Pitfall 1: Diamond Relaxation Breaking Existing Definitions
**What goes wrong:** Relaxing diamond detection too broadly allows invalid graph structures
**Why it happens:** Not distinguishing between named role-playing relationships and unnamed accidental diamonds
**How to avoid:** Only allow multi-parent when ALL relationships to that node are named with distinct names
**Warning signs:** Existing test definitions that currently produce diamond errors start silently succeeding

### Pitfall 2: Alias Collision with Double-Underscore Separator
**What goes wrong:** User table alias contains `__`, colliding with generated scoped aliases
**Why it happens:** No validation on alias naming conventions
**How to avoid:** The risk is low (__ is unusual in SQL aliases), but document the convention. If a collision occurs, the SQL will still be valid since the aliases are quoted.

### Pitfall 3: Dimension Resolution Without USING Context
**What goes wrong:** Query includes a dimension from a role-playing table but no metric provides USING context for that table
**Why it happens:** User expects dimension to "just work" without specifying which relationship path to use
**How to avoid:** Produce a clear error listing available relationships. Error message format: "dimension 'X' is ambiguous because table 'Y' is reached via multiple relationships: [rel1, rel2]. Use a metric with USING (rel_name) to disambiguate."

### Pitfall 4: Fan Trap Detection with Scoped Aliases
**What goes wrong:** Fan trap detection doesn't account for USING-scoped join paths
**Why it happens:** `check_fan_traps()` uses unscoped aliases in the cardinality map
**How to avoid:** When USING is specified, the fan trap check should use the specific relationship path, not the generic table alias

### Pitfall 5: Derived Metric USING Inheritance
**What goes wrong:** Derived metric references two base metrics with conflicting USING paths to the same table
**Why it happens:** Each base metric uses a different relationship to the same role-playing table
**How to avoid:** This is actually valid -- the derived metric computes from both paths. `collect_derived_metric_source_tables` must track which relationship each source table comes through.

### Pitfall 6: Body Parser Ambiguity Between USING and AS
**What goes wrong:** Metric entry `foo.bar USING (some_rel) AS SUM(foo.x)` -- the parser must distinguish USING keyword from an identifier
**Why it happens:** Token parsing is sensitive to keyword ordering
**How to avoid:** Use `find_keyword_ci()` with word-boundary matching (already exists in body_parser). Look for USING between the qualified name and AS.

## Code Examples

### DDL Example: Flights and Airports (Role-Playing Pattern)

```sql
CREATE SEMANTIC VIEW flights_by_airport AS
  TABLES (
    f AS flights PRIMARY KEY (flight_id),
    a AS airports PRIMARY KEY (airport_code)
  )
  RELATIONSHIPS (
    dep_airport AS f(departure_code) REFERENCES a,
    arr_airport AS f(arrival_code) REFERENCES a
  )
  DIMENSIONS (
    a.city AS a.city,
    a.country AS a.country,
    f.carrier AS f.carrier
  )
  METRICS (
    f.departure_count USING (dep_airport) AS COUNT(*),
    f.arrival_count USING (arr_airport) AS COUNT(*),
    total_flights AS departure_count + arrival_count
  );
```

### Expected Expansion

Query: `dimensions := ['city'], metrics := ['departure_count']`

```sql
SELECT
    "a__dep_airport"."city" AS "city",
    COUNT(*) AS "departure_count"
FROM "flights" AS "f"
LEFT JOIN "airports" AS "a__dep_airport" ON "f"."departure_code" = "a__dep_airport"."airport_code"
GROUP BY
    1
```

Query: `dimensions := ['carrier'], metrics := ['departure_count', 'arrival_count']`

```sql
SELECT
    "f"."carrier" AS "carrier",
    COUNT(*) AS "departure_count",
    COUNT(*) AS "arrival_count"
FROM "flights" AS "f"
LEFT JOIN "airports" AS "a__dep_airport" ON "f"."departure_code" = "a__dep_airport"."airport_code"
LEFT JOIN "airports" AS "a__arr_airport" ON "f"."arrival_code" = "a__arr_airport"."airport_code"
GROUP BY
    1
```

### Ambiguity Error Example

Query: `dimensions := ['city'], metrics := ['total_flights']`
(total_flights is derived, references both departure_count and arrival_count which use different relationships)

Expected error:
```
semantic view 'flights_by_airport': dimension 'city' is ambiguous -- table 'a' is reached via
multiple relationships: [dep_airport, arr_airport]. Specify a metric with USING to disambiguate,
or use a dimension from a non-ambiguous table.
```

### USING Parsing Example

Input metric entry: `f.departure_count USING (dep_airport) AS COUNT(*)`

Parse result:
```rust
(
    Some("f"),           // source_alias
    "departure_count",   // name
    "COUNT(*)",          // expr
    vec!["dep_airport"], // using_relationships
)
```

### Validation Error Example

```sql
METRICS (
    f.bad_metric USING (nonexistent_rel) AS COUNT(*)
)
```

Error: `unknown relationship 'nonexistent_rel' in USING clause of metric 'bad_metric'. Available: [dep_airport, arr_airport]`

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Reject all diamonds unconditionally | Allow named multi-path relationships | Phase 32 (this phase) | Enables role-playing dimension pattern |
| Single alias per table | Relationship-scoped aliases (`alias__rel`) | Phase 32 (this phase) | Same physical table can appear multiple times with different aliases |
| Metric = (name, expr, source_table) | Metric = (name, expr, source_table, using_relationships) | Phase 32 (this phase) | Metrics can select specific join paths |

## Open Questions

1. **Dimension expression rewriting for scoped aliases**
   - What we know: Dimension expressions use `alias.column` format (e.g., `a.city`). When the alias is scoped to a relationship (`a__dep_airport`), the expression must be rewritten.
   - What's unclear: Should the dimension expression be rewritten at expansion time by string replacement, or should the dimension store the original alias and expansion resolves the scoped alias?
   - Recommendation: Rewrite at expansion time. The dimension stores `a.city`, expansion replaces `a.` with `a__dep_airport.` when USING context determines the relationship. This keeps the model clean and pushes complexity to expansion.

2. **Multiple USING relationships on one metric**
   - What we know: Snowflake allows `USING (rel1, rel2)` for metrics that need to traverse multiple relationships (cascading joins through intermediate role-playing tables).
   - What's unclear: Whether this project needs multi-relationship USING for Phase 32 or if single-relationship USING covers the core use case.
   - Recommendation: Support the full `Vec<String>` syntax in parsing/model but validate that each relationship in the list points to a different target table. The flights/airports pattern only needs single-relationship USING, but the infrastructure should support multiple.

3. **Dimension queries without any metrics**
   - What we know: `SELECT DISTINCT` mode (dimensions only, no metrics) currently works.
   - What's unclear: If a user queries only dimensions from a role-playing table without any metric USING context, how to resolve.
   - Recommendation: This is inherently ambiguous. Produce `AmbiguousPath` error. The user must include at least one metric with USING to disambiguate which instance of the role-playing table to use for its dimensions.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (unit + proptest) + sqllogictest-bin + DuckLake CI |
| Config file | Cargo.toml (dev-dependencies), justfile (test recipes) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| JOIN-01 | Named multi-path relationships accepted | unit | `cargo test graph::tests::diamond_relaxation -x` | Wave 0 |
| JOIN-02 | USING clause parsing | unit + proptest | `cargo test body_parser::tests::parse_metrics_using -x` | Wave 0 |
| JOIN-03 | Scoped alias generation in expansion | unit | `cargo test expand::tests::using_scoped_aliases -x` | Wave 0 |
| JOIN-04 | USING validation rejects nonexistent relationship | unit | `cargo test graph::tests::validate_using -x` | Wave 0 |
| JOIN-05 | Ambiguous dimension error | unit | `cargo test expand::tests::ambiguous_path_error -x` | Wave 0 |
| ROLE-01 | Distinct aliases per relationship | unit | `cargo test expand::tests::role_playing_aliases -x` | Wave 0 |
| ROLE-02 | Dimension resolves via metric USING | unit | `cargo test expand::tests::dimension_using_resolution -x` | Wave 0 |
| ROLE-03 | End-to-end flights/airports | sqllogictest | `just test-sql` (phase32_role_playing.test) | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase32_role_playing.test` -- covers ROLE-03 end-to-end
- [ ] Unit tests for diamond relaxation, USING parsing, scoped alias generation, ambiguity detection
- [ ] Proptest for USING clause parsing with adversarial input
- [ ] Fuzz target for USING clause parsing (optional, covered by existing fuzz_ddl_parse)

## Sources

### Primary (HIGH confidence)
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- USING clause syntax, relationship definition grammar
- [Snowflake semantic views SQL guide](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- flights/airports role-playing example, USING semantics, cascading relationships
- [Snowflake YAML spec](https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec) -- using_relationships field, relationship definitions
- [Snowflake validation rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- multi-path constraints, FK/PK validation

### Secondary (HIGH confidence - project codebase)
- `src/model.rs` (line 36-145) -- Metric and Join struct definitions, current fields
- `src/graph.rs` (line 150-160) -- `check_no_diamonds()` implementation
- `src/expand.rs` (line 282-346) -- `resolve_joins_pkfk()` join resolution
- `src/body_parser.rs` (line 762-843) -- `parse_metrics_clause()` and `parse_single_metric_entry()`
- `src/ddl/define.rs` (line 115-128) -- Validation call chain in `bind()`

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, extends existing Rust patterns
- Architecture: HIGH -- Snowflake's approach is well-documented, codebase patterns are clear
- Pitfalls: HIGH -- diamond relaxation and alias scoping are well-understood from Snowflake's model
- USING parsing: HIGH -- extends existing body_parser patterns with keyword detection

**Research date:** 2026-03-14
**Valid until:** 2026-04-14 (stable domain, no external dependency changes expected)
