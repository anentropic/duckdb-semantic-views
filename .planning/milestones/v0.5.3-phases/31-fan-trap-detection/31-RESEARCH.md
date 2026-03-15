# Phase 31: Fan Trap Detection - Research

**Researched:** 2026-03-14
**Domain:** Semantic layer cardinality modeling and fan trap detection
**Confidence:** HIGH

## Summary

Fan trap detection for this project requires three changes to the existing codebase: (1) extending the relationship parser in `body_parser.rs` to accept optional `MANY TO ONE` / `ONE TO ONE` / `ONE TO MANY` cardinality keywords after the `REFERENCES <alias>` clause, (2) storing cardinality on the `Join` model struct, and (3) adding a fan trap check in `expand.rs` that blocks queries where a metric source table sits on the "one" side of a one-to-many edge relative to a dimension's source table -- meaning the metric values would be duplicated (fanned out) during the join.

The existing infrastructure is well-suited for this: the `RelationshipGraph` in `graph.rs` already tracks directed edges with `from_alias -> to_alias` semantics, the `expand()` function already resolves which joins are needed and has access to source tables for both dimensions and metrics, and the `ExpandError` enum already propagates as `Box<dyn Error>` through the `bind()` function to block queries with descriptive errors.

**Primary recommendation:** Add a `Cardinality` enum to `model.rs`, parse it in `body_parser.rs`, store it on `Join`, and add a `check_fan_traps()` function called from `expand()` after join resolution that walks the relationship graph with cardinality annotations to detect fan-out paths between metric source tables and dimension source tables.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Cardinality is declared after REFERENCES using plain SQL keywords: `MANY TO ONE`, `ONE TO ONE`, `ONE TO MANY`
- No `CARDINALITY` keyword prefix -- just the cardinality type directly
- Example: `order_to_customer AS o(customer_id) REFERENCES c MANY TO ONE`
- When omitted, defaults to `MANY TO ONE` (most common FK pattern)
- No underscores in the keywords (not `MANY_TO_ONE`)
- **Block the query** when a metric aggregates across a one-to-many boundary (Snowflake-style)
- Query fails with a descriptive error explaining the fan trap risk
- This is a hard error, not a warning -- prevents inflated results
- Deviation from original FAN-02/FAN-03 which specified warnings; user chose blocking after reviewing trade-offs
- `MANY TO ONE` -- standard FK pattern (many rows reference one PK row). Default.
- `ONE TO ONE` -- unique FK (1:1 mapping between tables)
- `ONE TO MANY` -- reverse direction (PK side declares it has many referencing rows)
- No `MANY TO MANY` support (matches Snowflake -- not supported)
- Detection happens at query expansion time (in expand.rs), not at DDL time

### Claude's Discretion
- Error message format and wording
- Internal representation of cardinality enum
- Graph traversal algorithm for detecting fan traps
- How to handle chains of relationships (transitive fan-out detection)

### Deferred Ideas (OUT OF SCOPE)
- Smart per-aggregate-type detection (block SUM/COUNT but allow MIN/MAX) -- future enhancement
- `SHOW SEMANTIC DIMENSIONS FOR METRIC` compatibility query (Snowflake feature)
- Configurable warn vs block via pragma
- `MANY TO MANY` support via bridge tables
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FAN-01 | Relationships can optionally declare cardinality type (one_to_one, one_to_many, many_to_one) | Parser extension in `body_parser.rs` after REFERENCES clause; `Cardinality` enum on `Join` model; default `ManyToOne` when omitted; backward-compatible serde |
| FAN-02 | Query expansion blocks when a metric aggregates across a one-to-many boundary that could inflate results (UPDATED: error, not warning) | New `FanTrap` variant in `ExpandError`; `check_fan_traps()` function in `expand.rs` using `RelationshipGraph` with cardinality annotations |
| FAN-03 | Fan trap detection produces a blocking error (UPDATED: was "warnings do not block", now blocking) | `ExpandError::FanTrap` propagates through `bind()` as `Box<dyn Error>`, naturally blocking the query; descriptive error message names relationship and tables |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde + serde_json | 1.x | Serialize/deserialize `Cardinality` enum on `Join` | Already used for all model types |
| strsim | 0.11 | Fuzzy suggestions in error messages | Already used project-wide |
| proptest | 1.9 | Property-based testing for cardinality parsing | Already in dev-dependencies |

### Supporting
No new dependencies are needed. All work uses existing libraries already in the project.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Explicit cardinality keywords | Infer from PK/UNIQUE constraints (Snowflake approach) | Inference requires runtime data introspection; explicit is simpler, deterministic, and matches user's locked decision |
| Blocking error | Warning message | User explicitly chose blocking; warnings risk silently-wrong results reaching end users |

## Architecture Patterns

### Recommended Changes by File

```
src/
  model.rs         # Add Cardinality enum, add field to Join
  body_parser.rs   # Parse MANY TO ONE / ONE TO ONE / ONE TO MANY after REFERENCES
  expand.rs        # Add check_fan_traps(), new ExpandError::FanTrap variant
  graph.rs         # Add cardinality_map to RelationshipGraph (or parallel HashMap)
```

### Pattern 1: Cardinality Enum on Join

**What:** A three-variant enum stored on `Join`, defaulting to `ManyToOne`.
**When to use:** Every relationship declaration.
**Example:**

```rust
/// Cardinality of a relationship from the FK (from_alias) side to the PK (to_alias) side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Cardinality {
    /// Many rows in from_alias reference one row in to_alias (standard FK).
    #[default]
    ManyToOne,
    /// One row in from_alias references one row in to_alias (unique FK).
    OneToOne,
    /// One row in from_alias is referenced by many rows in to_alias.
    /// This is the "reverse" direction -- the relationship fans OUT from from_alias.
    OneToMany,
}
```

The `Join` struct gets:
```rust
/// Phase 31: Cardinality of this relationship.
/// Defaults to ManyToOne when omitted in DDL (most common FK pattern).
/// Old stored JSON without this field deserializes as ManyToOne.
#[serde(default)]
pub cardinality: Cardinality,
```

`#[serde(default)]` ensures backward compatibility: old JSON without the field deserializes to `ManyToOne` (the `Default` impl).

### Pattern 2: Fan Trap Detection Algorithm

**What:** At expand time, for each (metric, dimension) pair where both have `source_table`, walk the join path between them and check if any edge is traversed in the fan-out direction.

**When to use:** After resolving dimensions and metrics, before generating SQL.

**Algorithm (HIGH confidence -- derived from project's existing graph infrastructure):**

1. Build `RelationshipGraph` from definition (already done in `resolve_joins_pkfk`).
2. Build a cardinality map: `HashMap<(from_alias, to_alias), Cardinality>` from `def.joins`.
3. For each resolved metric with `source_table = Some(M)`:
   - For each resolved dimension with `source_table = Some(D)`:
     - If M == D, skip (same table, no fan-out).
     - Find the join path from M to D through the relationship tree.
     - For each edge on the path, check the traversal direction vs. cardinality:
       - If we traverse an edge from the "one" side to the "many" side (fan-out direction), the metric at M will be duplicated.
     - Fan-out direction means:
       - Edge `(A, B)` with `ManyToOne`: traversing A->B is safe (many->one), traversing B->A is fan-out (one->many).
       - Edge `(A, B)` with `OneToOne`: both directions are safe.
       - Edge `(A, B)` with `OneToMany`: traversing A->B is fan-out (one->many), traversing B->A is safe.
4. If fan-out detected, return `ExpandError::FanTrap` with relationship name and table details.

**Path finding in a tree:** Since the graph is validated as a tree at define time (no diamonds, no cycles), the path between any two nodes is unique. Use the reverse map to find parents, walking both nodes up to the root, then derive the path.

### Pattern 3: Parser Extension for Cardinality Keywords

**What:** After consuming `REFERENCES <to_alias>`, optionally consume `MANY TO ONE`, `ONE TO ONE`, or `ONE TO MANY` as three consecutive case-insensitive tokens.

**When to use:** In `parse_single_relationship_entry` in `body_parser.rs`.

**Approach:**

```
// After extracting to_alias from "REFERENCES <to_alias>":
// Check if remaining text starts with MANY/ONE (case-insensitive)
// If MANY TO ONE -> Cardinality::ManyToOne
// If ONE TO ONE  -> Cardinality::OneToOne
// If ONE TO MANY -> Cardinality::OneToMany
// If no match    -> default ManyToOne (omitted)
```

The tricky part: `to_alias` extraction currently takes everything after `REFERENCES` until end of entry. Must stop at `MANY` or `ONE` keywords. Split remaining text, take first token as `to_alias`, check rest for cardinality.

### Pattern 4: ExpandError::FanTrap Variant

**What:** New error variant that provides actionable information.

```rust
/// A metric aggregates across a one-to-many boundary, risking inflated results.
FanTrap {
    view_name: String,
    metric_name: String,
    metric_table: String,
    dimension_name: String,
    dimension_table: String,
    relationship_name: String,
},
```

**Display format example:**
```
semantic view 'sales': fan trap detected -- metric 'revenue' (table 'orders') would be
duplicated when joined to dimension 'line_item_status' (table 'line_items') via
relationship 'order_to_items' (ONE TO MANY). This would inflate aggregation results.
Remove the dimension, use a metric from the same table, or adjust the relationship cardinality.
```

### Anti-Patterns to Avoid

- **Checking cardinality at DDL time:** The user locked that detection happens at query expansion time. DDL only stores the cardinality; validation is at expand time when we know which metrics and dimensions are combined.
- **Modifying `validate_graph` for fan traps:** Fan traps are not structural graph errors -- a valid model can have one-to-many relationships that are fine for some query combinations but not others. Keep validation separate from detection.
- **Storing cardinality on `RelationshipGraph` edges:** Instead, build a separate cardinality lookup from `def.joins` at expand time. This keeps `RelationshipGraph` focused on topology and avoids changing its API for other consumers.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tree path finding | BFS/DFS with visited tracking | Walk both nodes to root via `reverse` map, intersect | Tree has unique paths; parent-walking is O(depth) and simpler |
| Cardinality serde | Custom serializer | `#[derive(Serialize, Deserialize)]` with `#[serde(default)]` | Serde handles enum serialization natively; Default gives backward compat |
| Error propagation | Custom error handling in table_function | Existing `ExpandError -> Box<dyn Error>` pipeline | Already wired up in `bind()` |

## Common Pitfalls

### Pitfall 1: Backward Compatibility with Stored JSON
**What goes wrong:** Adding `cardinality` field to `Join` breaks deserialization of old stored definitions that lack the field.
**Why it happens:** `serde` strict mode rejects unknown/missing fields.
**How to avoid:** Use `#[serde(default)]` on the field and `#[derive(Default)]` on the enum, ensuring `ManyToOne` is the default.
**Warning signs:** Old sqllogictest tests fail on definition load.

### Pitfall 2: to_alias Extraction Conflicts with Cardinality Keywords
**What goes wrong:** The current parser takes everything after `REFERENCES` as the `to_alias`, so `REFERENCES c MANY TO ONE` would set `to_alias = "c MANY TO ONE"`.
**Why it happens:** `after_paren[refs_pos + "REFERENCES".len()..].trim()` greedily takes all remaining text.
**How to avoid:** After extracting the first word as `to_alias`, check if remaining tokens form a cardinality keyword sequence.
**Warning signs:** Relationship parsing tests pass but produce wrong `table` field on Join.

### Pitfall 3: Direction Confusion in Fan Trap Detection
**What goes wrong:** Confusing which direction is "safe" vs "fan-out" for each cardinality type.
**Why it happens:** Edge direction (`from_alias -> to_alias`) represents FK->PK, but cardinality describes the logical relationship direction.
**How to avoid:** Clear mental model: in `li_to_order AS li(order_id) REFERENCES o MANY TO ONE`, `from=li`, `to=o`, cardinality=ManyToOne. Walking li->o is safe (many go to one). Walking o->li is fan-out (one fans to many). For `MANY TO ONE`, fan-out happens when traversing the REVERSE edge.
**Warning signs:** Fan traps not detected when they should be, or false positives on safe joins.

### Pitfall 4: Derived Metrics and Fan Traps
**What goes wrong:** Derived metrics have no `source_table` -- they reference other metrics. Fan trap check must resolve their transitive source tables.
**Why it happens:** `collect_derived_metric_source_tables()` already exists for join resolution but fan trap check needs to apply it too.
**How to avoid:** For derived metrics, walk the dependency graph to find all base metric source tables, then check fan traps for each.
**Warning signs:** Derived metric `profit = revenue - cost` (both from `li`) passes fan trap check but shouldn't when combined with a dimension from `o` via a one-to-many edge.

### Pitfall 5: Metrics Without source_table on Single-Table Views
**What goes wrong:** Single-table views (no relationships) have metrics with `source_table = None` and should never trigger fan trap detection.
**Why it happens:** No joins means no cardinality, no fan-out risk.
**How to avoid:** Early return from `check_fan_traps()` if `def.joins` is empty or no joins have cardinality annotations.
**Warning signs:** Single-table view queries unexpectedly blocked.

## Code Examples

### Cardinality Enum Definition
```rust
// Source: project model.rs pattern
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub enum Cardinality {
    #[default]
    ManyToOne,
    OneToOne,
    OneToMany,
}
```

### Parser Extension (after REFERENCES)
```rust
// Source: body_parser.rs parse_single_relationship_entry pattern
// After extracting to_alias, check remaining text for cardinality keywords:
fn parse_cardinality(remaining: &str) -> Cardinality {
    let upper = remaining.trim().to_ascii_uppercase();
    if upper == "MANY TO ONE" {
        Cardinality::ManyToOne
    } else if upper == "ONE TO ONE" {
        Cardinality::OneToOne
    } else if upper == "ONE TO MANY" {
        Cardinality::OneToMany
    } else {
        Cardinality::ManyToOne // default when omitted
    }
}
```

### Fan Trap Check in expand.rs
```rust
// Source: expand.rs resolve_joins_pkfk pattern
fn check_fan_traps(
    view_name: &str,
    def: &SemanticViewDefinition,
    resolved_dims: &[&Dimension],
    resolved_mets: &[&Metric],
) -> Result<(), ExpandError> {
    if def.joins.is_empty() {
        return Ok(());
    }
    let Ok(graph) = RelationshipGraph::from_definition(def) else {
        return Ok(());
    };

    // Build cardinality map: (from_lower, to_lower) -> Cardinality
    let card_map: HashMap<(String, String), Cardinality> = def.joins.iter()
        .filter(|j| !j.fk_columns.is_empty())
        .map(|j| (
            (j.from_alias.to_ascii_lowercase(), j.table.to_ascii_lowercase()),
            j.cardinality,
        ))
        .collect();

    // For each metric+dimension pair, check for fan-out on the join path
    for met in resolved_mets {
        let met_tables = get_metric_source_tables(met, &def.metrics);
        for dim in resolved_dims {
            if let Some(ref dim_table) = dim.source_table {
                for met_table in &met_tables {
                    // Find path and check each edge for fan-out direction
                    // ...
                }
            }
        }
    }
    Ok(())
}
```

### DDL Example with Cardinality
```sql
CREATE SEMANTIC VIEW sales AS
  TABLES (
    o AS orders PRIMARY KEY (id),
    li AS line_items PRIMARY KEY (id),
    c AS customers PRIMARY KEY (id)
  )
  RELATIONSHIPS (
    li_to_order AS li(order_id) REFERENCES o MANY TO ONE,
    order_to_customer AS o(customer_id) REFERENCES c MANY TO ONE
  )
  DIMENSIONS (
    o.region AS o.region,
    c.name AS c.name
  )
  METRICS (
    li.revenue AS SUM(li.extended_price),
    o.order_count AS COUNT(*)
  );

-- This query should be BLOCKED:
-- o.order_count (from 'o') joined to li dimensions would fan out
-- because li->o is MANY TO ONE, meaning o->li is ONE TO MANY
SELECT * FROM semantic_view('sales',
  dimensions := ['region'],
  metrics := ['revenue']);  -- OK: li->o is MANY TO ONE (safe direction)

-- This would be BLOCKED if we had a dimension from li and a metric from o:
-- Because walking o->li means one order fans to many line items
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No cardinality | Explicit `MANY TO ONE` / `ONE TO ONE` / `ONE TO MANY` keywords | Phase 31 (this phase) | Enables fan trap detection |
| No fan trap detection | Block queries that cross one-to-many boundaries for metrics | Phase 31 (this phase) | Prevents silently-wrong aggregation results |
| Snowflake: infer from constraints | DuckDB ext: explicit keywords | Design choice | Explicit is simpler, no runtime data introspection needed |

**Industry context:** Snowflake semantic views (GA mid-2025) infer cardinality from PK/UNIQUE constraints rather than explicit keywords. Our explicit approach is more deterministic and avoids needing runtime data introspection. Both approaches share the same goal: prevent fan-out inflation in aggregation.

## Open Questions

1. **Edge case: base table metrics with no source_table**
   - What we know: Some older definitions have metrics with `source_table = None` on multi-table views (legacy format). These are treated as base-table metrics.
   - What's unclear: Should fan trap detection treat `None` source_table as the root/base table alias?
   - Recommendation: Yes -- `source_table = None` maps to the base table (first in TABLES). This is consistent with how `resolve_joins_pkfk` handles it.

2. **Transitive fan-out through chains**
   - What we know: `o -> li` (ManyToOne from li side) means `o -> li` is fan-out. If there's a chain `c -> o -> li`, a metric from `c` queried with a dimension from `li` crosses two edges.
   - What's unclear: Should we detect fan-out if ANY edge in the path fans out, or only when the metric is on the "one" side of a specific edge?
   - Recommendation: Any fan-out edge on the path from metric source to dimension source should trigger detection. A single fan-out edge anywhere in the path means the metric values get duplicated.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust test + proptest 1.9 + sqllogictest |
| Config file | `Cargo.toml` (dev-dependencies), `test/sql/TEST_LIST` |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| FAN-01 | Cardinality parsed from DDL and stored on Join | unit | `cargo test -- cardinality` | No -- Wave 0 |
| FAN-01 | Cardinality round-trips through serde | unit | `cargo test -- cardinality` | No -- Wave 0 |
| FAN-01 | Old JSON without cardinality deserializes as ManyToOne | unit | `cargo test -- cardinality` | No -- Wave 0 |
| FAN-01 | Cardinality parsing proptest (case variations, whitespace) | proptest | `cargo test -- cardinality` | No -- Wave 0 |
| FAN-02 | Fan trap detected and blocks query with error | unit | `cargo test -- fan_trap` | No -- Wave 0 |
| FAN-02 | Fan trap not triggered for safe join directions | unit | `cargo test -- fan_trap` | No -- Wave 0 |
| FAN-02 | Fan trap with derived metrics (transitive sources) | unit | `cargo test -- fan_trap` | No -- Wave 0 |
| FAN-02 | Fan trap error message names relationship and tables | unit | `cargo test -- fan_trap` | No -- Wave 0 |
| FAN-02 | Transitive fan-out through chains detected | unit | `cargo test -- fan_trap` | No -- Wave 0 |
| FAN-03 | Fan trap error propagates and blocks query execution | integration (slt) | `just test-sql` | No -- Wave 0 |
| FAN-03 | Query without fan trap succeeds normally | integration (slt) | `just test-sql` | No -- Wave 0 |
| FAN-03 | End-to-end: DDL with cardinality, query blocked, safe query succeeds | integration (slt) | `just test-sql` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Unit tests in `model.rs` for `Cardinality` enum serde
- [ ] Unit tests in `body_parser.rs` for cardinality keyword parsing
- [ ] Unit tests in `expand.rs` for `check_fan_traps` function
- [ ] Proptest in `tests/parse_proptest.rs` for cardinality clause variations
- [ ] sqllogictest `test/sql/phase31_fan_trap.test` for end-to-end scenarios
- [ ] Add `test/sql/phase31_fan_trap.test` to `test/sql/TEST_LIST`

## Sources

### Primary (HIGH confidence)
- Project source code: `src/model.rs`, `src/body_parser.rs`, `src/expand.rs`, `src/graph.rs` -- direct inspection of current data structures, parser patterns, expansion logic, and graph infrastructure
- Project tests: `tests/parse_proptest.rs`, `tests/expand_proptest.rs`, `test/sql/phase30_derived_metrics.test` -- established testing patterns
- `31-CONTEXT.md` -- user's locked decisions on syntax, behavior, and scope

### Secondary (MEDIUM confidence)
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- confirmed Snowflake does NOT use explicit cardinality keywords (infers from constraints)
- [Snowflake validation rules](https://docs.snowflake.com/en/user-guide/views-semantic/validation-rules) -- confirmed Snowflake prevents fan-out through granularity validation rules
- [Datacadamia fan trap reference](https://www.datacadamia.com/data/type/cube/semantic/fan_trap) -- fan trap definition: one-to-many join path causing metric duplication

### Tertiary (LOW confidence)
- [ThoughtSpot schemas article](https://www.thoughtspot.com/fact-and-dimension/schemas-scale-how-avoid-common-data-modeling-traps) -- general industry context on fan traps and chasm traps
- [Sigma fan trap quickstart](https://quickstarts.sigmacomputing.com/guide/tables_fan_traps/index.html) -- alternative approaches to fan trap resolution

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing libraries
- Architecture: HIGH -- patterns directly derived from existing codebase inspection
- Pitfalls: HIGH -- identified from direct code analysis of parser and model patterns
- Fan trap algorithm: HIGH -- standard graph-theoretic approach applied to existing tree-validated graph

**Research date:** 2026-03-14
**Valid until:** 2026-04-14 (stable domain, no external dependency changes expected)
