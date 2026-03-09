# Technology Stack: v0.5.2 SQL DDL & PK/FK Relationships

**Project:** DuckDB Semantic Views Extension
**Researched:** 2026-03-09
**Milestone:** v0.5.2 -- Proper SQL DDL syntax and Snowflake-style PK/FK relationship model
**Scope:** What library/crate additions or changes are needed for SQL DDL body parsing and PK/FK-based join inference

---

## Bottom Line Up Front

**Zero new Cargo dependencies.** The v0.5.2 features -- proper SQL keyword syntax (`TABLES (...)`, `DIMENSIONS (...)`, `METRICS (...)`) and PK/FK-based join inference -- are achievable by extending the existing hand-written parser and expand engine. Neither `sqlparser-rs` nor `petgraph` should be added.

The DDL body is not arbitrary SQL -- it is a small, closed grammar of 4 clause keywords (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS) with structured entries inside parentheses. The existing `scan_clause_keywords` in `parse.rs` already detects these clauses. What is missing is a **body parser** that extracts typed data from each clause into the `SemanticViewDefinition` model, replacing the current pass-through to function-call syntax.

The PK/FK join graph is a small directed acyclic graph (typically 2-8 nodes). Topological sort for join ordering is 30 lines of code using a `HashMap<String, Vec<String>>` adjacency list. `petgraph` (58KB + 3 transitive deps) would add dependency weight for what amounts to a trivial algorithm on trivially small graphs.

---

## Existing Dependency Inventory (Current Cargo.toml)

All dependencies remain sufficient:

| Crate | Version | Sufficient For v0.5.2 | Why |
|-------|---------|----------------------|-----|
| `duckdb` | `=1.4.4` | Yes | VTab, BindInfo -- DDL functions execute via existing table function path |
| `libduckdb-sys` | `=1.4.4` | Yes | Raw FFI unchanged -- same DDL execution path |
| `serde` + `serde_json` | `1` | Yes | `SemanticViewDefinition` gains new fields (PK columns, FK references); serde handles it |
| `strsim` | `0.11` | Yes | "Did you mean" suggestions for clause names, table aliases, column names |
| `cc` | `1` (build-dep, optional) | Yes | C++ shim compilation unchanged |
| `proptest` | `1.9` (dev-dep) | Yes | PBTs for new parser and join graph logic |
| `cargo-husky` | `1` (dev-dep) | Yes | Pre-commit hooks unchanged |

---

## The Two Technical Questions

### Question 1: sqlparser-rs or extend the existing parser?

**Recommendation: Extend the existing hand-written parser. Do NOT add sqlparser-rs.**

**Rationale:**

1. **The grammar is not SQL.** The body of `CREATE SEMANTIC VIEW name (...)` is a domain-specific language with 4 clause keywords, not arbitrary SQL. Consider the target syntax (following Snowflake's `CREATE SEMANTIC VIEW` DDL):

   ```sql
   CREATE SEMANTIC VIEW sales_view (
     TABLES (
       o AS orders PRIMARY KEY (order_id),
       c AS customers PRIMARY KEY (customer_id),
       li AS line_items PRIMARY KEY (line_item_id)
     )
     RELATIONSHIPS (
       li (order_id) REFERENCES o,
       o (customer_id) REFERENCES c
     )
     DIMENSIONS (
       o.order_date AS date_trunc('month', o.order_date),
       c.region AS c.region
     )
     METRICS (
       o.revenue AS sum(li.amount),
       o.order_count AS count(o.order_id)
     )
   )
   ```

   This is a closed, fixed grammar -- not open-ended SQL. A recursive-descent parser for this structure is ~200-300 lines of Rust. `sqlparser-rs` (v0.61.0, ~2MB compiled) cannot parse this grammar out of the box because `CREATE SEMANTIC VIEW` is not in its AST. You would need to either: (a) fork the parser to add a custom `Statement` variant (ongoing merge burden), or (b) use sqlparser only for sub-expression parsing (extracting SQL expressions from dimension/metric definitions), which is a sledgehammer for a nail.

2. **The existing parser is already 80% there.** `scan_clause_keywords` in `parse.rs` (lines 384-441) already:
   - Tracks string literal boundaries (handles escaped quotes)
   - Tracks bracket depth (parentheses, square brackets, curly braces)
   - Identifies clause keywords at depth 0
   - Validates against known keywords with "did you mean" suggestions
   - Reports error positions

   What is missing is the second phase: extracting the content between clause keyword `(` and its matching `)`, then parsing each clause's entry syntax. This is a straightforward extension of the existing scanning logic.

3. **sqlparser-rs would not help with the novel syntax.** The TABLES clause (`alias AS table_name PRIMARY KEY (cols)`) and RELATIONSHIPS clause (`alias (cols) REFERENCES alias`) are semantic-view-specific constructs that do not exist in any SQL dialect's grammar. sqlparser cannot parse them. You would still need a hand-written parser for the clause bodies.

4. **Binary size matters.** The extension already compiles the DuckDB amalgamation (~20MB). Adding sqlparser-rs (~2MB compiled, per Cargo build analysis of comparable projects) increases the non-amalgamation portion of the binary by roughly 15-20%. For a DuckDB community extension, minimal dependencies are a virtue.

**Confidence:** HIGH -- based on direct code inspection of `parse.rs`, review of sqlparser-rs v0.61.0 Statement enum (docs.rs), and analysis of the target grammar against Snowflake's DDL syntax.

### Question 2: What is needed for PK/FK graph traversal and join order computation?

**Recommendation: Hand-written topological sort on a HashMap adjacency list. Do NOT add petgraph.**

**Rationale:**

1. **The graph is tiny.** Semantic views typically have 2-8 tables. Even a complex TPC-H view has 6 tables. The relationship graph (FK edges between table aliases) has at most ~20 edges. Graph library overhead (generic type parameters, trait objects, index types) provides zero benefit at this scale.

2. **The algorithm is trivial.** PK/FK join inference requires:
   - Build an adjacency list from RELATIONSHIPS declarations
   - Given a set of needed tables (from requested dimensions/metrics), find the minimal subgraph connecting them to the base table
   - Topologically sort that subgraph to determine join order

   This is Kahn's algorithm (BFS topological sort) -- ~30 lines of Rust:

   ```rust
   fn topo_sort(adj: &HashMap<String, Vec<String>>, needed: &HashSet<String>) -> Vec<String> {
       let mut in_degree: HashMap<String, usize> = HashMap::new();
       // ... count incoming edges for needed nodes
       // ... BFS from nodes with in_degree 0
       // ... ~25 more lines
   }
   ```

3. **petgraph is heavyweight for this.** petgraph 0.8.3 brings 3 transitive dependencies (`fixedbitset`, `hashbrown`, `indexmap`). Its `DiGraph<N, E>` uses `NodeIndex`/`EdgeIndex` integer handles that require mapping to/from string table names -- adding a layer of indirection that the `HashMap<String, Vec<String>>` approach avoids entirely.

4. **The existing `resolve_joins` already does the hard part.** The current implementation in `expand.rs` (lines 168-234) already:
   - Collects needed tables from dimension/metric `source_table` fields
   - Resolves aliases to physical table names via `def.tables`
   - Performs transitive dependency resolution via a fixed-point loop
   - Filters and orders joins by declaration order

   The v0.5.2 change replaces the ON-clause substring matching (tech debt item 6) with explicit PK/FK graph edges. The algorithm structure stays the same -- only the edge representation changes.

**Confidence:** HIGH -- based on analysis of the existing `resolve_joins` implementation, the PK/FK relationship model from Snowflake's DDL, and evaluation of petgraph 0.8.3 API surface.

---

## Feature-by-Feature Stack Analysis

### 1. SQL Keyword DDL Body Parser

**What exists:** `scan_clause_keywords` detects clause keywords. `rewrite_ddl` passes the body verbatim to function-call syntax (`:=` named parameters with struct/list literals). `validate_clauses` checks structural validity.

**What v0.5.2 changes:** The body parser must understand proper SQL keyword syntax, not function-call syntax. The pipeline becomes:

```
DDL text -> detect_ddl_kind -> parse_create_body -> parse_clause_bodies -> SemanticViewDefinition -> JSON -> create_semantic_view()
```

Instead of the current:
```
DDL text -> detect_ddl_kind -> parse_create_body -> pass body verbatim to function call
```

**New code needed (all in `src/parse.rs` or a new `src/parse/` module):**

| Parser Function | Input | Output | Complexity |
|----------------|-------|--------|------------|
| `extract_clause_body(body, keyword)` | Full body string, "TABLES" | Substring between `TABLES (` and matching `)` | ~30 lines, bracket tracking |
| `parse_tables_clause(text)` | `"o AS orders PRIMARY KEY (order_id), ..."` | `Vec<TableEntry>` with alias, table, pk_cols | ~60 lines |
| `parse_relationships_clause(text)` | `"li (order_id) REFERENCES o, ..."` | `Vec<RelEntry>` with from_alias, fk_cols, to_alias | ~50 lines |
| `parse_dimensions_clause(text)` | `"o.order_date AS expr, ..."` | `Vec<Dimension>` with source_table, name, expr | ~40 lines |
| `parse_metrics_clause(text)` | `"o.revenue AS expr, ..."` | `Vec<Metric>` with source_table, name, expr | ~40 lines |

**Stack requirement:** None new. String slicing, bracket tracking, and comma splitting are all `std` operations. The existing pattern in `scan_clause_keywords` (byte-level scanning with depth tracking) extends naturally.

**Key parsing challenge:** Extracting SQL expressions from dimension/metric clauses. The expression `sum(li.amount * (1 - li.discount))` contains parentheses and commas that must not be treated as clause-level delimiters. The existing bracket depth tracking in `scan_clause_keywords` handles this -- expressions are only split on commas at depth 0.

**Confidence:** HIGH -- the pattern exists in the codebase; this is an extension, not a new approach.

### 2. PK/FK Model Extensions

**What exists in `model.rs`:**
- `TableRef { alias, table }` -- alias-to-table mapping
- `Join { table, on, from_cols, join_columns }` -- mixed legacy/new format
- `JoinColumn { from, to }` -- column pair for FK declarations

**What v0.5.2 adds to the model:**

```rust
/// Extended table entry with PRIMARY KEY declaration.
/// Replaces the simple TableRef for SQL DDL definitions.
pub struct TableEntry {
    pub alias: String,
    pub table: String,
    pub primary_key: Vec<String>,  // NEW: PK column names
}
```

The `Join` struct gains clarity -- `join_columns` becomes the sole path for new definitions. The `on` field remains for backward compatibility with stored JSON but is never written by v0.5.2 DDL.

**Backward compatibility:** Old stored JSON without `primary_key` deserializes with `#[serde(default)]` producing an empty `Vec`. No migration needed.

**Stack requirement:** `serde` (existing) handles the new fields via derive macros. No new crates.

**Confidence:** HIGH -- the serde default pattern is already used for `tables`, `facts`, `join_columns`, and `column_types_inferred` in the current model.

### 3. PK/FK Join Inference Engine

**What exists in `expand.rs`:**
- `resolve_joins` -- collects needed tables, resolves aliases, transitive dependency via ON-clause substring matching
- `append_join_on_clause` -- generates `alias.col = alias.col AND ...` from `join_columns`

**What v0.5.2 changes:**

The transitive dependency resolution (lines 202-227 of `expand.rs`) currently checks if a needed join's ON clause contains another join's table name as a substring. With PK/FK declarations, this becomes explicit graph traversal:

```rust
// Current (v0.5.1): heuristic substring matching
let on_lower = join.on.to_ascii_lowercase();
if on_lower.contains(&other_lower) { ... }

// New (v0.5.2): explicit FK graph edges
// RELATIONSHIPS declares: li (order_id) REFERENCES o
// This means: to reach li, you must first join o
// Graph edge: li -> o (li depends on o)
```

**Algorithm for join ordering:**

1. Build dependency graph from RELATIONSHIPS: for each `from_alias (fk_cols) REFERENCES to_alias`, add edge `from_alias -> to_alias` (from depends on to).
2. Given needed tables (from dimensions/metrics `source_table` fields), find all tables reachable by following dependency edges backward to the base table.
3. Topologically sort the needed subgraph -- tables with no dependencies (closest to base) come first.
4. Emit JOINs in topological order.

**Implementation size:** ~50 lines replacing the existing fixed-point loop in `resolve_joins`. The `HashMap<String, Vec<String>>` adjacency list is built from `def.joins` at expansion time.

**Stack requirement:** None new. `std::collections::HashMap` and `std::collections::HashSet` (already imported in `expand.rs`) are sufficient.

**Confidence:** HIGH -- the existing `resolve_joins` function structure maps directly to this approach. The change is replacing the edge detection mechanism, not the algorithm shape.

### 4. Qualified Column Names in Expressions

**What v0.5.2 enables:** With table aliases declared in TABLES and used as prefixes in DIMENSIONS/METRICS, expressions like `o.order_date` and `li.amount` work naturally because the CTE base query emits `AS "o"` and `AS "li"` aliases.

**What already works:** The `expand` function already emits `AS alias` for base and joined tables when `def.tables` is non-empty (lines 414-434 of `expand.rs`). Qualified column names in expressions resolve correctly because DuckDB's SQL engine handles `alias.column` references.

**What changes:** The DDL body parser must recognize `alias.name AS expr` in DIMENSIONS/METRICS clauses and populate `source_table` from the alias prefix. Currently, `source_table` is set via a separate struct field in function-call syntax. In SQL keyword syntax, it is implicit from the `alias.name` prefix.

**Stack requirement:** None new. String splitting on `.` is `std`.

**Confidence:** HIGH -- code inspection confirms the alias mechanism works. The change is in DDL parsing, not in expansion.

---

## What NOT to Add

| Candidate | Version | Why Not |
|-----------|---------|---------|
| `sqlparser` | 0.61.0 | **~2MB compiled, 1 transitive dep.** Cannot parse `CREATE SEMANTIC VIEW` syntax -- would need custom `Statement` variant (fork burden) or only use for sub-expression parsing (overkill). The DDL body grammar is 4 clause keywords with structured entries -- a hand-written parser is ~200 lines and zero dependencies. The existing `scan_clause_keywords` already handles the hard parts (bracket depth, string literals, keyword detection). |
| `petgraph` | 0.8.3 | **58KB compiled, 3 transitive deps** (fixedbitset, hashbrown, indexmap). The PK/FK graph has 2-8 nodes. Topological sort is ~30 lines of Rust with `HashMap`. petgraph's `NodeIndex`/`EdgeIndex` handles add a mapping layer that is pure overhead at this scale. |
| `nom` | 8.0.0 | Parser combinator framework. The clause body grammar is simple enough that string scanning with `.find()`, `.split()`, and bracket depth tracking is clearer than combinator chains. `nom` adds ~150KB and a learning curve for contributors. |
| `winnow` | 0.7.x | Same category as `nom`. Slightly smaller but same reasoning applies. |
| `pest` | 2.7.x | PEG parser generator. Requires a `.pest` grammar file and build-time code generation. Adds complexity for a grammar that fits in 200 lines of hand-written Rust. |
| `logos` | 0.14.x | Lexer generator. The DDL body has ~10 token types (keywords, identifiers, parens, commas, `AS`, `PRIMARY KEY`, `REFERENCES`). A hand-written scanner is simpler and avoids the proc-macro build cost. |
| `regex` | 1.x | Not needed -- all parsing is structural (bracket depth + keyword detection), not pattern matching. |

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not Alternative |
|----------|-------------|-------------|---------------------|
| DDL body parsing | Hand-written recursive descent (~200 lines) | `sqlparser-rs` 0.61.0 | Cannot parse custom DDL syntax; would need fork or only partial use; ~2MB compiled size |
| DDL body parsing | Hand-written recursive descent | `nom` 8.0 / `winnow` 0.7 | Grammar is too simple for combinator overhead; adds ~150KB + learning curve |
| Join graph | `HashMap<String, Vec<String>>` + Kahn's algorithm (~30 lines) | `petgraph` 0.8.3 | 3 transitive deps for a 2-8 node graph; NodeIndex mapping overhead |
| Join graph | In-place adjacency list | `daggy` 0.8 (DAG-specific petgraph wrapper) | Same dependency chain as petgraph; even more overhead for less flexibility |
| PK column storage | `Vec<String>` on `TableEntry` | `IndexSet<String>` via indexmap | Ordered uniqueness is validated at parse time; `Vec` is sufficient and already in std |

---

## Model Changes (No New Dependencies)

### New fields on `SemanticViewDefinition`

```rust
// In model.rs -- extend TableRef or introduce TableEntry:

/// A table entry with optional PRIMARY KEY declaration.
/// For v0.5.2 SQL DDL syntax, primary_key is populated from TABLES clause.
/// For legacy function-call syntax, primary_key remains empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableRef {
    pub alias: String,
    pub table: String,
    #[serde(default)]
    pub primary_key: Vec<String>,  // NEW
}
```

No new struct types needed. The existing `Join.join_columns: Vec<JoinColumn>` already represents FK declarations. The RELATIONSHIPS clause parser populates `join_columns` from `alias (fk_cols) REFERENCES other_alias`.

### Serde backward compatibility

All new fields use `#[serde(default)]` -- old stored JSON deserializes with empty vecs. No migration path needed. This is the same pattern used for every model extension since v0.2.0.

---

## Complete v0.5.2 Cargo.toml Changes

**None.** The Cargo.toml is unchanged from v0.5.0/v0.5.1:

```toml
# NO CHANGES to [dependencies]:
# duckdb = { version = "=1.4.4", default-features = false }
# libduckdb-sys = "=1.4.4"
# serde = { version = "1", features = ["derive"] }
# serde_json = "1"
# strsim = "0.11"

# NO CHANGES to [build-dependencies]:
# cc = { version = "1", optional = true }

# NO CHANGES to [dev-dependencies]:
# proptest = "1.9"
```

Version bump only: `version = "0.5.0"` -> `version = "0.5.2"` (at milestone completion).

---

## Integration Points

### Where new code touches existing code

| New Feature | Touches | How |
|-------------|---------|-----|
| SQL DDL body parser | `src/parse.rs` (new functions) | Adds `parse_tables_clause`, `parse_relationships_clause`, `parse_dimensions_clause`, `parse_metrics_clause` |
| Clause body extraction | `src/parse.rs` | Adds `extract_clause_body` using existing bracket depth tracking |
| DDL-to-model translation | `src/parse.rs` -> `src/model.rs` | Parser returns `SemanticViewDefinition` directly instead of passing body to function call |
| PK field on TableRef | `src/model.rs` | Add `primary_key: Vec<String>` with `#[serde(default)]` |
| PK/FK join ordering | `src/expand.rs` `resolve_joins` | Replace ON-clause substring matching with explicit FK graph traversal |
| Rewrite pipeline | `src/parse.rs` `rewrite_ddl` | For SQL keyword syntax: serialize `SemanticViewDefinition` to JSON, pass to function as JSON string argument; OR build function-call syntax from parsed model |

### What stays untouched

- `src/ddl/define.rs` -- function implementation unchanged (receives same arguments)
- `src/ddl/drop.rs`, `describe.rs`, `list.rs` -- no changes
- `src/query/` -- query pipeline unchanged
- `src/catalog.rs` -- catalog operations unchanged
- `build.rs` -- build script unchanged
- `cpp/` -- C++ shim unchanged (already routes all DDL text to Rust)

### Two-phase execution strategy

The v0.5.2 parser has two paths:

1. **SQL keyword syntax** (new): `CREATE SEMANTIC VIEW name (TABLES (...) DIMENSIONS (...) METRICS (...))` -- parser extracts clause bodies, builds `SemanticViewDefinition`, serializes to JSON, rewrites to `SELECT * FROM create_semantic_view('name', ...)` using the function-call interface with the structured model data.

2. **Function-call syntax** (existing, preserved): `CREATE SEMANTIC VIEW name (tables := [...], dimensions := [...], metrics := [...])` -- body passes through to function call unchanged, as it does today.

The parser distinguishes between the two by detecting whether the first non-whitespace token after `(` is a clause keyword (TABLES, RELATIONSHIPS, DIMENSIONS, METRICS) or a named parameter (`tables :=`, `dimensions :=`). This detection is already partially implemented in `scan_clause_keywords`.

---

## Confidence Assessment

| Area | Level | Reason |
|------|-------|--------|
| No new deps needed | HIGH | Direct code inspection of parser, model, and expand modules; grammar analysis against target syntax |
| sqlparser-rs rejection | HIGH | Verified v0.61.0 Statement enum lacks custom DDL support; custom grammar analysis shows closed vocabulary |
| petgraph rejection | HIGH | Graph scale analysis (2-8 nodes); algorithm complexity analysis (O(V+E) with V<10 is trivial) |
| Parser extension approach | HIGH | Existing `scan_clause_keywords` pattern directly extensible; bracket depth tracking already handles nested expressions |
| Model changes | HIGH | Existing `#[serde(default)]` backward-compat pattern used 6+ times in current model |
| Join inference changes | HIGH | Existing `resolve_joins` maps directly to FK graph approach; algorithm structure preserved |

---

## Sources

- [sqlparser-rs v0.61.0 -- docs.rs](https://docs.rs/crate/sqlparser/latest) -- version and feature verification (HIGH confidence)
- [sqlparser-rs custom parser docs](https://github.com/sqlparser-rs/sqlparser-rs/blob/main/docs/custom_sql_parser.md) -- extensibility limitations confirmed (MEDIUM confidence, rate-limited during fetch)
- [sqlparser-rs Statement enum -- docs.rs](https://docs.rs/sqlparser/latest/sqlparser/ast/enum.Statement.html) -- no `CREATE SEMANTIC VIEW` variant (HIGH confidence)
- [petgraph v0.8.3 -- docs.rs](https://docs.rs/crate/petgraph/latest) -- version, dependencies, and API surface verified (HIGH confidence)
- [petgraph toposort -- docs.rs](https://docs.rs/petgraph/latest/petgraph/algo/fn.toposort.html) -- algorithm reference (HIGH confidence)
- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- target DDL syntax reference (HIGH confidence)
- [Snowflake semantic view example](https://docs.snowflake.com/en/user-guide/views-semantic/example) -- TPC-H example with TABLES/RELATIONSHIPS/DIMENSIONS/METRICS (HIGH confidence)
- [Snowflake SEMANTIC_VIEW query construct](https://docs.snowflake.com/en/sql-reference/constructs/semantic_view) -- query-time join resolution from RELATIONSHIPS (HIGH confidence)
- Project source: `src/parse.rs`, `src/expand.rs`, `src/model.rs`, `src/ddl/define.rs`, `Cargo.toml` -- first-party code inspection (HIGH confidence)
