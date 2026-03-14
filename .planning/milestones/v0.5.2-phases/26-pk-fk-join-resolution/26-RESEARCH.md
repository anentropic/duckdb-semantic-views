# Phase 26: PK/FK Join Resolution - Research

**Researched:** 2026-03-13
**Domain:** Graph-based join synthesis from PK/FK declarations, define-time graph validation
**Confidence:** HIGH

## Summary

Phase 26 transforms the PK/FK metadata produced by Phase 24 (model) and Phase 25 (parser) into actual SQL JOIN ON clauses during query expansion. The core work is: (1) build a directed graph from relationship declarations at define time, (2) validate that graph is a tree rooted at the base table (rejecting cycles, diamonds, self-references, orphan tables, unreachable dims/metrics), (3) use topological sort to determine join ordering, and (4) synthesize `LEFT JOIN ... ON from_alias.fk_col = to_alias.pk_col` clauses from the positionally-matched FK/PK column pairs.

No new Cargo dependencies are needed. The graph algorithms (cycle detection, diamond detection, topological sort) are straightforward to hand-write using `HashMap`/`HashSet` from `std::collections`. The relationship graph for any practical semantic view is tiny (typically 2-10 nodes), so algorithmic efficiency is irrelevant -- correctness and clear error messages are the priorities.

**Primary recommendation:** Use Kahn's algorithm (BFS-based topological sort) for join ordering. It naturally detects cycles (nodes remaining after queue drains = cycle participants) and produces deterministic output when using a sorted queue seed. Implement graph validation as a standalone function in a new `src/graph.rs` module, called from `define.rs` at CREATE time. Update `expand.rs` to use graph-based transitive join resolution and PK/FK ON clause synthesis, replacing the legacy `resolve_joins` fixed-point loop and `append_join_on_clause` `join_columns` path.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- All generated joins use LEFT JOIN (globally, replacing current bare JOIN / INNER)
- Relationship graph must be a tree rooted at the base table (first table in TABLES clause)
- Cycles rejected with path-naming error: "cycle detected in relationships: orders -> customers -> orders"
- Diamonds rejected with path-naming error: "diamond: two paths to 'products' via 'orders' and 'inventory'"
- Self-references explicitly rejected: "table 'employees' cannot reference itself"
- Every dim/metric source_table alias must be reachable from base table via relationship graph -- error at CREATE time
- Orphan tables (declared in TABLES but not connected) error at CREATE time
- Error messages use existing `strsim` fuzzy-match for "did you mean?" suggestions
- FK columns positionally match PK columns: `o(customer_id) REFERENCES c` means `customer_id` maps to `c.pk_columns[0]`
- Error at CREATE time if FK column count != PK column count on referenced table
- No explicit REFERENCES column naming (`REFERENCES c(id)` not supported)
- Snowflake semantic view DDL is the design reference

### Claude's Discretion
- Topological sort algorithm choice (Kahn's vs DFS-based)
- Graph data structure (adjacency list, edge list, etc.)
- Where validation code lives (expand.rs, new module, or define.rs)
- Exact error message wording beyond the patterns specified above
- How to handle the transition from old `join_columns`/`on` fields to new PK/FK synthesis in `append_join_on_clause`

### Deferred Ideas (OUT OF SCOPE)
- Self-referencing relationships (employee -> manager hierarchy) -- requires role-playing dimension support
- Explicit column naming on REFERENCES side (`REFERENCES c(id)`) -- deferred to UNIQUE constraint support
- Join type configuration in DDL syntax -- industry consensus is no configuration
- Cardinality inference from PK/UNIQUE metadata
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| EXP-02 | JOIN ON clauses synthesized from PK/FK declarations | PK/FK ON clause synthesis pattern: positional FK-to-PK matching using `Join.from_alias`/`Join.fk_columns` -> `TableRef.pk_columns` on the referenced table. Update `append_join_on_clause` to use this path when `fk_columns` is non-empty. |
| EXP-03 | Join ordering via topological sort of relationship graph | Kahn's algorithm on adjacency list built from `Join.from_alias` -> `Join.table` edges. Deterministic ordering via sorted initial queue. Replaces declaration-order emission. |
| EXP-04 | Transitive join inclusion -- requesting dims from A and C auto-joins through B | Graph-based BFS/DFS from base table to target tables, collecting all intermediate nodes. Replaces the current `resolve_joins` fixed-point ON-substring matching heuristic. |
| EXP-06 | Define-time validation: relationship graph must be a tree (error on diamonds/cycles) | Validate at CREATE time in `define.rs` before persisting. Detect cycles (Kahn's leftover nodes), diamonds (parent-count > 1 during BFS), self-references (from_alias == to_alias edge), orphan tables, unreachable source_table aliases. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| std::collections::HashMap | stdlib | Adjacency list graph representation | No external dependency needed for tiny graphs |
| std::collections::HashSet | stdlib | Visited tracking, cycle/diamond detection | Already used extensively in expand.rs |
| std::collections::VecDeque | stdlib | BFS queue for Kahn's algorithm | Standard BFS container |
| strsim | 0.11 | Fuzzy-match "did you mean?" suggestions | Already a Cargo dep, used in expand.rs and body_parser.rs |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde/serde_json | 1.x | Model serialization (existing) | No changes needed -- existing model fields suffice |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Kahn's (BFS) | DFS-based toposort | Kahn's naturally detects cycles (leftover nodes); DFS requires separate visited/in-stack tracking. Kahn's is simpler for this use case. |
| HashMap adjacency list | petgraph crate | Overkill -- graph has < 10 nodes typically. Project constraint: zero new dependencies. |

## Architecture Patterns

### Recommended Module Structure
```
src/
  graph.rs          # NEW: RelationshipGraph, validate_graph(), toposort()
  expand.rs         # MODIFY: resolve_joins_from_graph(), new append_pkfk_on_clause()
  ddl/define.rs     # MODIFY: call validate_graph() before persisting
  model.rs          # NO CHANGES -- Phase 24 fields already present
  body_parser.rs    # NO CHANGES -- already produces correct Join structs
```

### Pattern 1: Relationship Graph as Adjacency List
**What:** Build a directed graph where nodes are table aliases and edges represent FK->PK relationships. The base table is the root (in-degree 0).
**When to use:** At define time (validation) and at expand time (join ordering + transitive inclusion).
**Example:**
```rust
use std::collections::{HashMap, HashSet, VecDeque};

/// A directed relationship graph built from TABLES + RELATIONSHIPS.
/// Nodes = table aliases. Edges = from_alias -> to_alias (FK direction).
pub struct RelationshipGraph {
    /// Adjacency list: from_alias -> vec of to_aliases
    edges: HashMap<String, Vec<String>>,
    /// Reverse adjacency: to_alias -> vec of from_aliases (for parent tracking)
    reverse: HashMap<String, Vec<String>>,
    /// All declared table aliases
    all_nodes: HashSet<String>,
    /// The root node (base table alias, first in TABLES)
    root: String,
}
```

### Pattern 2: Kahn's Algorithm for Topological Sort + Cycle Detection
**What:** BFS from nodes with in-degree 0. If not all nodes are visited, remaining nodes form cycles.
**When to use:** Join ordering (EXP-03) and cycle detection (EXP-06).
**Example:**
```rust
/// Returns topologically sorted aliases, or Err with cycle path if graph has cycles.
pub fn toposort(&self) -> Result<Vec<String>, String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for node in &self.all_nodes {
        in_degree.entry(node).or_insert(0);
    }
    for targets in self.edges.values() {
        for t in targets {
            *in_degree.entry(t).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<String> = VecDeque::new();
    // Seed with root (base table) first for determinism
    if in_degree.get(self.root.as_str()) == Some(&0) {
        queue.push_back(self.root.clone());
    }
    // Add other zero-in-degree nodes in sorted order for determinism
    let mut others: Vec<&str> = in_degree.iter()
        .filter(|(k, v)| **v == 0 && **k != self.root.as_str())
        .map(|(k, _)| *k)
        .collect();
    others.sort();
    for o in others {
        queue.push_back(o.to_string());
    }

    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.clone());
        if let Some(neighbors) = self.edges.get(&node) {
            for next in neighbors {
                if let Some(deg) = in_degree.get_mut(next.as_str()) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next.clone());
                    }
                }
            }
        }
    }

    if order.len() != self.all_nodes.len() {
        // Remaining nodes are in a cycle -- find and report the cycle path
        Err(format_cycle_error(&self.edges, &order, &self.all_nodes))
    } else {
        Ok(order)
    }
}
```

### Pattern 3: Diamond Detection via Parent Count
**What:** During BFS from root, track how many parents each node has been reached from. If any node has > 1 parent in the reverse direction, it's a diamond.
**When to use:** EXP-06 define-time validation.
**Example:**
```rust
/// Check that the relationship graph is a tree (each non-root node has exactly one parent).
/// Returns Err with diamond description if any node is reachable via multiple paths.
pub fn check_no_diamonds(&self) -> Result<(), String> {
    for (node, parents) in &self.reverse {
        if node != &self.root && parents.len() > 1 {
            return Err(format!(
                "diamond: two paths to '{}' via '{}' and '{}'",
                node, parents[0], parents[1]
            ));
        }
    }
    Ok(())
}
```

### Pattern 4: PK/FK ON Clause Synthesis
**What:** For a relationship `from_alias(fk_col1, fk_col2) REFERENCES to_alias`, generate `from_alias.fk_col1 = to_alias.pk_col1 AND from_alias.fk_col2 = to_alias.pk_col2` where pk_cols come from the referenced table's `pk_columns`.
**When to use:** EXP-02 during SQL expansion.
**Example:**
```rust
/// Generate ON clause from PK/FK positional matching.
/// `join.fk_columns` maps positionally to `referenced_table.pk_columns`.
fn synthesize_on_clause(
    join: &Join,
    tables: &[TableRef],
) -> String {
    // Find the referenced table (join.table stores the to_alias)
    let to_ref = tables.iter()
        .find(|t| t.alias.eq_ignore_ascii_case(&join.table))
        .expect("referenced table must exist (validated at define time)");

    let pairs: Vec<String> = join.fk_columns.iter()
        .zip(to_ref.pk_columns.iter())
        .map(|(fk, pk)| format!(
            "{}.{} = {}.{}",
            quote_ident(&join.from_alias),
            quote_ident(fk),
            quote_ident(&join.table),
            quote_ident(pk),
        ))
        .collect();

    pairs.join(" AND ")
}
```

### Pattern 5: Graph-Based Transitive Join Resolution
**What:** Given needed table aliases (from requested dims/metrics), BFS backwards from needed tables toward root to find all intermediate tables that must be joined.
**When to use:** EXP-04, replacing the current `resolve_joins` fixed-point ON-substring heuristic.
**Example:**
```rust
/// Given a set of needed table aliases, find all aliases that must be joined
/// (including intermediate tables on the path from root to each needed table).
fn resolve_needed_joins(
    graph: &RelationshipGraph,
    needed_aliases: &HashSet<String>,
) -> Vec<String> {
    // For each needed alias, walk reverse edges back to root, collecting all intermediates
    let mut all_needed: HashSet<String> = HashSet::new();
    for alias in needed_aliases {
        let mut current = alias.clone();
        while current != graph.root {
            all_needed.insert(current.clone());
            // Walk to parent (guaranteed single parent -- tree validated at define time)
            if let Some(parents) = graph.reverse.get(&current) {
                current = parents[0].clone();
            } else {
                break; // base table reached
            }
        }
    }

    // Return in topological order (from root outward)
    graph.toposort().unwrap()
        .into_iter()
        .filter(|a| all_needed.contains(a))
        .collect()
}
```

### Anti-Patterns to Avoid
- **ON-clause substring matching for transitive resolution:** The current `resolve_joins` uses `on_lower.contains(&other_lower)` which is a fragile text heuristic. Replace with proper graph traversal.
- **Declaration-order join emission:** The current code emits joins in declaration order. With a validated tree graph, topological order is the correct and deterministic ordering.
- **Validation at query time:** All graph structure errors (cycles, diamonds, orphans, unreachable tables) must be caught at CREATE time. Query-time (`expand()`) should assume the graph is valid.
- **Modifying model.rs:** The Phase 24 fields (`pk_columns`, `from_alias`, `fk_columns`, `name`) already exist. No model changes needed.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Fuzzy string matching | Custom edit distance | `strsim::levenshtein` | Already a dependency, used everywhere in the codebase |
| SQL identifier quoting | Custom escaper | `quote_ident()` / `quote_table_ref()` in expand.rs | Handles double-quote escaping, dot-qualified names |
| JSON serialization | Custom formatter | `serde_json` | Already used for model persistence |

**Key insight:** The graph algorithms themselves are simple enough to hand-write (< 50 lines each). The project explicitly forbids new dependencies. `std::collections` provides everything needed.

## Common Pitfalls

### Pitfall 1: Edge Direction Confusion
**What goes wrong:** The relationship `o(customer_id) REFERENCES c` could be modeled as edge `o -> c` or `c -> o`. Wrong direction breaks topological sort and parent tracking.
**Why it happens:** FK "points to" PK, but the data flow is "from base outward."
**How to avoid:** Model edges as `from_alias -> to_alias` (matching the REFERENCES direction). In `o(customer_id) REFERENCES c`, the edge is `o -> c`. The base table has in-degree 0. Topological order flows from root outward.
**Warning signs:** Topological sort puts leaf tables first instead of root first.

### Pitfall 2: Case Sensitivity in Alias Matching
**What goes wrong:** Graph node "O" doesn't match source_table "o", causing false "unreachable" errors.
**Why it happens:** Aliases stored as user-typed casing; comparisons must be case-insensitive.
**How to avoid:** Normalize all aliases to lowercase when building the graph. Use `eq_ignore_ascii_case` for all lookups. This is consistent with existing patterns throughout the codebase.
**Warning signs:** Tests pass with exact casing but fail with mixed casing.

### Pitfall 3: Forgetting to Validate FK Count vs PK Count
**What goes wrong:** `o(col1, col2) REFERENCES c` where `c` has `PRIMARY KEY (pk1)` -- 2 FK cols vs 1 PK col. At expand time, `zip` silently truncates, producing wrong SQL.
**Why it happens:** `zip` on iterators of different lengths silently drops excess elements.
**How to avoid:** Validate `join.fk_columns.len() == referenced_table.pk_columns.len()` during define-time graph validation. Error before persisting.
**Warning signs:** Generated ON clause has fewer conditions than expected.

### Pitfall 4: Legacy `join_columns` Path Collision
**What goes wrong:** Old Phase 11.1 definitions use `join_columns` (Vec<JoinColumn>). New Phase 24 definitions use `fk_columns` + `from_alias`. Both paths exist in `append_join_on_clause`.
**Why it happens:** The model has backward-compatible fields from multiple eras.
**How to avoid:** Check `fk_columns` first (non-empty = Phase 24 path). Fall back to `join_columns` (non-empty = Phase 11.1 path). Fall back to `on` string (legacy). This three-tier fallback preserves backward compatibility.
**Warning signs:** Tests for old-format definitions fail after Phase 26 changes.

### Pitfall 5: Orphan Base Table Detection
**What goes wrong:** Validation detects the base table alias as "orphan" because it has no incoming edges.
**Why it happens:** The base table is the root -- it should have in-degree 0 by definition.
**How to avoid:** Explicitly exclude the base table (root) from orphan detection. An orphan is a non-root, non-base table with in-degree 0 AND out-degree 0 (declared but not connected by any relationship).
**Warning signs:** Single-table definitions (no relationships) erroneously fail validation.

### Pitfall 6: Self-Reference Detection vs Cycle Detection
**What goes wrong:** `o(manager_id) REFERENCES o` is a self-reference, not a cycle in the graph sense. Kahn's algorithm may or may not detect it depending on edge representation.
**Why it happens:** A self-loop (edge from node to itself) technically creates a cycle of length 1, but the error message should say "self-reference" not "cycle."
**How to avoid:** Check for self-references explicitly (from_alias == to_alias) BEFORE running cycle detection. Produce the specific error message: "table 'X' cannot reference itself".
**Warning signs:** Self-references get generic "cycle detected" message instead of specific self-reference message.

## Code Examples

### Example 1: Building the Graph from Model

```rust
// Source: project-specific pattern based on model.rs structures
impl RelationshipGraph {
    pub fn from_definition(def: &SemanticViewDefinition) -> Result<Self, String> {
        let root = def.tables.first()
            .ok_or("TABLES clause is empty")?
            .alias.to_ascii_lowercase();

        let all_nodes: HashSet<String> = def.tables.iter()
            .map(|t| t.alias.to_ascii_lowercase())
            .collect();

        let mut edges: HashMap<String, Vec<String>> = HashMap::new();
        let mut reverse: HashMap<String, Vec<String>> = HashMap::new();

        for join in &def.joins {
            if join.fk_columns.is_empty() {
                continue; // Legacy join -- skip graph building
            }
            let from = join.from_alias.to_ascii_lowercase();
            let to = join.table.to_ascii_lowercase();

            // Self-reference check
            if from == to {
                return Err(format!("table '{}' cannot reference itself", join.from_alias));
            }

            edges.entry(from.clone()).or_default().push(to.clone());
            reverse.entry(to).or_default().push(from);
        }

        Ok(Self { edges, reverse, all_nodes, root })
    }
}
```

### Example 2: Complete Validation Sequence

```rust
// Source: project-specific pattern
pub fn validate_graph(
    def: &SemanticViewDefinition,
) -> Result<RelationshipGraph, String> {
    let graph = RelationshipGraph::from_definition(def)?;

    // 1. Check for cycles (Kahn's algorithm)
    let _topo_order = graph.toposort()?;

    // 2. Check for diamonds (multiple parents)
    graph.check_no_diamonds()?;

    // 3. Check for orphan tables (declared but not connected)
    graph.check_no_orphans()?;

    // 4. Check FK count matches PK count for each relationship
    for join in &def.joins {
        if join.fk_columns.is_empty() { continue; }
        let to_alias = join.table.to_ascii_lowercase();
        let to_ref = def.tables.iter()
            .find(|t| t.alias.eq_ignore_ascii_case(&to_alias));
        if let Some(tr) = to_ref {
            if !tr.pk_columns.is_empty() && join.fk_columns.len() != tr.pk_columns.len() {
                return Err(format!(
                    "FK column count ({}) does not match PK column count ({}) on table '{}'",
                    join.fk_columns.len(), tr.pk_columns.len(), join.table
                ));
            }
        }
    }

    // 5. Check all dim/metric source_table aliases are reachable from root
    check_source_tables_reachable(def, &graph)?;

    Ok(graph)
}
```

### Example 3: Updated expand() JOIN Generation (LEFT JOIN + PK/FK ON)

```rust
// Source: adaptation of existing expand.rs lines 420-437
for alias in &ordered_join_aliases {
    let join = def.joins.iter()
        .find(|j| j.table.eq_ignore_ascii_case(alias) || j.from_alias.eq_ignore_ascii_case(alias))
        .expect("join validated at define time");
    let table_ref = def.tables.iter()
        .find(|t| t.alias.eq_ignore_ascii_case(alias))
        .expect("table validated at define time");

    sql.push_str("\n    LEFT JOIN ");
    sql.push_str(&quote_table_ref(&table_ref.table));
    sql.push_str(" AS ");
    sql.push_str(&quote_ident(&table_ref.alias));
    sql.push_str(" ON ");

    // Phase 26: synthesize ON from PK/FK
    if !join.fk_columns.is_empty() {
        sql.push_str(&synthesize_on_clause(join, &def.tables));
    } else if !join.join_columns.is_empty() {
        // Phase 11.1 fallback
        append_legacy_join_on_clause(&mut sql, join, def);
    } else {
        // Phase 10 legacy: raw ON string
        sql.push_str(&join.on);
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Raw ON string in Join.on | Column-pair JoinColumn structs | Phase 11.1 (v0.3.0) | Structured but still base-table-centric |
| JoinColumn (from/to pairs) | PK/FK with from_alias/fk_columns | Phase 24 (v0.5.2) | Enables graph-based resolution |
| Fixed-point ON-substring matching | Graph-based BFS traversal | Phase 26 (this phase) | Correct transitive resolution |
| Declaration-order join emission | Topological sort ordering | Phase 26 (this phase) | Deterministic regardless of declaration order |
| INNER JOIN (bare JOIN) | LEFT JOIN (all joins) | Phase 26 (this phase) | Industry standard: preserves base table rows |

**Deprecated/outdated:**
- `Join.on` (raw string): Still supported for backward compat but not produced by Phase 25 parser
- `Join.join_columns` (Vec<JoinColumn>): Phase 11.1 format, superseded by `fk_columns`/`from_alias`
- `Join.from_cols`: Phase 11 format, superseded by `join_columns` and then by `fk_columns`
- `resolve_joins` fixed-point loop: Replaced by graph-based traversal

## Open Questions

1. **Where exactly should graph validation run in define.rs?**
   - What we know: `DefineFromJsonVTab::bind()` (line 256) deserializes JSON and persists. `DefineSemanticViewVTab::bind()` (line 108) parses args and persists.
   - What's unclear: Validation must run in both paths (JSON and arg-based). Should it be a shared function called from both?
   - Recommendation: Create `validate_definition(def: &SemanticViewDefinition) -> Result<(), String>` in graph.rs, called from both bind functions after parsing but before persisting. For legacy definitions (empty `fk_columns`), skip graph validation gracefully.

2. **Should topological order replace declaration order globally?**
   - What we know: Current tests assert declaration order. Topological sort produces a different order for some inputs.
   - What's unclear: For tree graphs where declaration order IS topological order, is there a visible difference?
   - Recommendation: For new PK/FK definitions, use topological order. For legacy definitions (no `fk_columns`), preserve declaration order. Update tests accordingly.

3. **How to handle mixed old/new definitions?**
   - What we know: Old stored JSON has `join_columns` or `on` strings. New definitions have `fk_columns`/`from_alias`.
   - What's unclear: Should old definitions bypass graph validation entirely?
   - Recommendation: Yes. If all joins have empty `fk_columns`, treat as legacy and skip graph validation. The three-tier fallback in ON clause generation handles this.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + proptest 1.9 |
| Config file | Cargo.toml (dev-dependencies) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| EXP-02 | PK/FK ON clause synthesis | unit | `cargo test --lib graph::tests::pkfk_on_clause -x` | Wave 0 |
| EXP-02 | LEFT JOIN emitted (not INNER) | unit | `cargo test --lib expand::tests::left_join -x` | Wave 0 |
| EXP-02 | Composite PK/FK generates AND-joined ON | unit | `cargo test --lib graph::tests::composite_pkfk -x` | Wave 0 |
| EXP-03 | Topological sort ordering | unit | `cargo test --lib graph::tests::toposort -x` | Wave 0 |
| EXP-03 | Deterministic ordering regardless of declaration order | unit | `cargo test --lib graph::tests::toposort_deterministic -x` | Wave 0 |
| EXP-04 | Transitive A-B-C join inclusion | unit | `cargo test --lib expand::tests::transitive_pkfk -x` | Wave 0 |
| EXP-04 | Only needed joins included (pruning) | unit | `cargo test --lib expand::tests::pruning_pkfk -x` | Wave 0 |
| EXP-06 | Cycle detection at define time | unit | `cargo test --lib graph::tests::cycle_detected -x` | Wave 0 |
| EXP-06 | Diamond detection at define time | unit | `cargo test --lib graph::tests::diamond_detected -x` | Wave 0 |
| EXP-06 | Self-reference detection | unit | `cargo test --lib graph::tests::self_ref -x` | Wave 0 |
| EXP-06 | Orphan table detection | unit | `cargo test --lib graph::tests::orphan_table -x` | Wave 0 |
| EXP-06 | Unreachable source_table detection | unit | `cargo test --lib graph::tests::unreachable_source -x` | Wave 0 |
| EXP-06 | FK count != PK count error | unit | `cargo test --lib graph::tests::fk_pk_count_mismatch -x` | Wave 0 |
| EXP-02 | End-to-end CREATE + query with PK/FK joins | integration (sqllogictest) | `just test-sql` | Wave 0 |
| EXP-04 | End-to-end transitive join through 3 tables | integration (sqllogictest) | `just test-sql` | Wave 0 |
| EXP-06 | End-to-end cycle rejection | integration (sqllogictest) | `just test-sql` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/graph.rs` -- new module with RelationshipGraph, validate_graph(), toposort(), all validation functions + unit tests
- [ ] `test/sql/phase26_join_resolution.test` -- sqllogictest integration tests for PK/FK join synthesis, transitive inclusion, error cases
- [ ] Update `tests/expand_proptest.rs` -- add property tests for PK/FK join definitions

## Sources

### Primary (HIGH confidence)
- Project source code: `src/model.rs`, `src/expand.rs`, `src/body_parser.rs`, `src/ddl/define.rs` -- direct inspection of all Phase 24 model fields, current expansion logic, parser output, and define-time flow
- Project CONTEXT.md -- all locked decisions and constraints
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- design reference for RELATIONSHIPS, PRIMARY KEY, REFERENCES syntax and join synthesis
- [Snowflake semantic view examples](https://docs.snowflake.com/en/user-guide/views-semantic/example) -- multi-table relationship patterns showing transitive join behavior

### Secondary (MEDIUM confidence)
- [Kahn's algorithm - Wikipedia](https://en.wikipedia.org/wiki/Topological_sorting) -- well-established BFS topological sort with natural cycle detection
- [Kahn's algorithm - GeeksforGeeks](https://www.geeksforgeeks.org/dsa/topological-sorting-indegree-based-solution/) -- O(V+E) complexity, queue-based implementation reference

### Tertiary (LOW confidence)
- None -- all findings verified against primary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - stdlib only, no new deps, verified existing Cargo.toml
- Architecture: HIGH - graph.rs module pattern follows project conventions; all integration points inspected line-by-line
- Pitfalls: HIGH - identified from direct code inspection (edge direction, case sensitivity, legacy format collision all visible in current source)
- Validation: HIGH - test patterns verified against existing test infrastructure (expand_proptest.rs, phase25_keyword_body.test)

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable domain -- graph algorithms don't change)
