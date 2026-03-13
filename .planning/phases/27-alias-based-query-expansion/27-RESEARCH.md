# Phase 27: Alias-Based Query Expansion - Research

**Researched:** 2026-03-13
**Domain:** Rust code deletion, qualified column reference verification, sqllogictest integration
**Confidence:** HIGH

## Summary

Phase 27 is primarily a cleanup and verification phase. The core expansion work (EXP-01, alias-based FROM+JOIN) and CTE removal (CLN-02) were completed in Phase 26. Three work items remain: verifying qualified column references work end-to-end (EXP-05), removing the old `:=`/struct-literal DDL body parser from `parse.rs` (CLN-01), and removing the ON-clause substring matching heuristic from `expand.rs` (CLN-03).

The code to be deleted is well-isolated. CLN-01 targets `scan_clause_keywords()` and the paren-body dispatch path in `validate_create_body()` inside `src/parse.rs`. CLN-03 targets the `resolve_joins()` function in `src/expand.rs`. Both deletions have a clear replacement already in place: `parse_keyword_body()` (from `body_parser.rs`) handles all new DDL, and `resolve_joins_pkfk()` handles all new join resolution. The project decision is no backward compatibility for old syntax.

EXP-05 (qualified column references in generated SQL) is already structurally supported by the Phase 26 expansion path: dimensions and metrics declared as `alias.expr` in DDL are stored verbatim in `Dimension.expr` and emitted directly into the SELECT clause. The main verification question is whether `o.amount` in a dimension expression resolves correctly when the FROM clause uses `FROM p26_orders AS o` rather than a CTE. The Phase 26 sqllogictest (`test/sql/phase26_join_resolution.test`) already uses `sum(o.amount)` and `c.name` expressions and they pass — so EXP-05 is likely satisfied but needs an explicit unit test and sqllogictest case.

**Primary recommendation:** Write a Rust unit test confirming `expand()` emits qualified column refs verbatim, add a sqllogictest case using dot-qualified expressions, then delete `resolve_joins()` and `scan_clause_keywords()` (plus the paren-body dispatch in `validate_create_body`).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| EXP-05 | Qualified column references (`alias.column`) work in generated SQL | Already passes through `Dimension.expr` verbatim; needs explicit test |
| CLN-01 | Remove old `:=`/struct literal DDL body parsing (no backward compat) | `scan_clause_keywords()` lines 435-494 in parse.rs; paren-body dispatch in `validate_create_body()` ~lines 689-715 |
| CLN-02 | Remove CTE-based `_base` flattening expansion path | Already complete in Phase 26 |
| CLN-03 | Remove ON-clause substring matching join heuristic | `resolve_joins()` function lines ~169-235 in expand.rs |

Note: CLN-02 is marked complete in REQUIREMENTS.md (Phase 26). It is listed in the phase description but does not require work.
</phase_requirements>

## Standard Stack

No new dependencies. Phase uses existing project tools.

### Core
| Tool | Version | Purpose |
|------|---------|---------|
| cargo test | (project) | Rust unit + integration tests |
| just test-sql | (project) | sqllogictest end-to-end via extension binary |
| just build | (project) | Build extension before running sqllogictest |

### No New Dependencies
The project has a hard constraint: zero new Cargo dependencies (from STATE.md: "Zero new Cargo dependencies -- hand-written parser and graph traversal").

## Architecture Patterns

### How Qualified Column References Flow (EXP-05 path)

The DDL body parser (`body_parser.rs`) parses `alias.dim_name AS sql_expr` entries. The `sql_expr` (right side of `AS`) is stored verbatim in `Dimension.expr` and `Metric.expr`:

```rust
// In body_parser.rs: dimension entry stores raw expression
Dimension {
    name: "customer_name".to_string(),    // left of AS
    expr: "c.name".to_string(),           // right of AS -- stored verbatim
    source_table: Some("c".to_string()),  // alias prefix from left side
    output_type: None,
}
```

The `expand()` function in `expand.rs` emits `Dimension.expr` directly into the SELECT clause:
```rust
// expand.rs lines ~514-522
let base_expr = dim.expr.clone();
let final_expr = if let Some(ref type_str) = dim.output_type {
    format!("CAST({base_expr} AS {type_str})")
} else {
    base_expr
};
select_items.push(format!("    {} AS {}", final_expr, quote_ident(&dim.name)));
```

For a dimension `c.customer_name AS c.name`, the generated SELECT clause becomes:
```sql
SELECT
    c.name AS "customer_name"
FROM p26_orders AS o
LEFT JOIN p26_customers AS c ON "o"."customer_id" = "c"."id"
```

DuckDB resolves `c.name` against the aliased JOIN target `p26_customers AS c` — this works because Phase 26 removed the CTE wrapper that previously aliased table names differently.

### What CLN-01 Removes (parse.rs paren-body path)

The paren-body path handles DDL of the form:
```sql
-- OLD syntax (no longer supported)
CREATE SEMANTIC VIEW my_view (
    tables := [{alias: 'o', table: 'orders'}],
    dimensions := [{name: 'region', expr: 'region'}],
    metrics := [{name: 'revenue', expr: 'sum(amount)'}]
)
```

The detection is in `scan_clause_keywords()` (parse.rs lines ~435-494): it scans the body for words followed by `:=` or `(`, and returns them as found clause keywords. The function is only called from `validate_clauses()` (line 522), which is only called from the paren-body branch of `validate_create_body()` (line 710).

The AS-body path (new syntax) was added in Phase 25 and is routed at line ~675 in `validate_create_body()`. It goes directly to `rewrite_ddl_keyword_body()` without touching `validate_clauses()` or `scan_clause_keywords()`.

After CLN-01 deletion, the paren-body detection block in `validate_create_body()` (lines ~689-715) and `scan_clause_keywords()` itself are removed. `validate_clauses()` also becomes dead code and should be deleted.

**Key callers to track:**
- `validate_clauses()` is called only from `validate_create_body()` (single call site)
- `scan_clause_keywords()` is called only from `validate_clauses()` (single call site)
- `validate_brackets()` is called from `validate_clauses()` — check if it has any other callers

### What CLN-03 Removes (expand.rs legacy join path)

`resolve_joins()` (expand.rs lines ~169-235) is called only from the `else` branch of the `has_pkfk` condition in `expand()` (line ~572):

```rust
// expand.rs lines ~546-590
if has_pkfk {
    // Phase 26: Graph-based PK/FK join resolution.
    let ordered_aliases = resolve_joins_pkfk(def, &resolved_dims, &resolved_mets);
    // ... emit LEFT JOIN with synthesize_on_clause
} else {
    // Legacy path: resolve_joins + append_join_on_clause.
    let needed_joins = resolve_joins(&def.joins, &resolved_dims, &resolved_mets, def);
    // ... emit LEFT JOIN with append_join_on_clause
}
```

The substring matching heuristic is in the fixed-point loop (lines ~204-228): it detects transitive join dependencies by checking if a needed join's ON clause string contains another join's table name as a substring. This is fragile (e.g., `orders` matches inside `line_orders`).

After CLN-03, the `has_pkfk` conditional is removed and `resolve_joins_pkfk()` is always called. The `resolve_joins()` and `append_join_on_clause()` functions are deleted.

**Dependencies to verify before deletion:**
- `append_join_on_clause()` is called only inside `resolve_joins` loop (same file)
- `resolve_joins()` is called only from the `else` branch
- `synthesize_on_clause()` must remain (used by PK/FK path)

### Anti-Patterns to Avoid

- **Deleting `validate_brackets()` without checking all callers.** It may have independent test coverage or callers outside `validate_clauses()`. Check all call sites before deletion.
- **Deleting `parse_create_body()` in parse.rs.** This function handles the paren-body syntax for `rewrite_ddl()`, which is still the rewrite path for non-AS DDL bodies. If the paren-body path is fully removed, `parse_create_body()` becomes dead code too — but only delete it after confirming the old tests in `phase16_parser.test` and `phase19_parser_hook_validation.test` are updated or removed.
- **Assuming old sqllogictest files still pass after deletion.** Some older `.test` files use the paren-body DDL syntax (e.g., `phase16_parser.test`, `phase19_parser_hook_validation.test`). These must be updated or removed as part of CLN-01.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead |
|---------|-------------|-------------|
| Qualified column resolution | Custom column resolver | DuckDB does this natively — just emit `alias.column` in SQL |
| Test setup/teardown | Custom table lifetime tracking | sqllogictest `statement ok` with cleanup at end of file |

## Common Pitfalls

### Pitfall 1: Old sqllogictest files use paren-body syntax
**What goes wrong:** After CLN-01 removes the paren-body dispatch, old `.test` files that use `CREATE SEMANTIC VIEW name (tables := [...])` syntax will fail validation and error at sqllogictest time.
**Why it happens:** Tests written for Phase 16/19/20 predate the AS-body syntax.
**How to avoid:** Audit every `.test` file for paren-body DDL syntax before deleting the paren-body code path. Files to check: `phase16_parser.test`, `phase19_parser_hook_validation.test`, `phase20_extended_ddl.test`.
**Warning signs:** `just test-sql` fails with "Expected '(' after view name" after CLN-01.

### Pitfall 2: Removing `resolve_joins()` breaks legacy stored definitions
**What goes wrong:** Old views stored in the catalog may have `Join` records with empty `fk_columns` (Phase 10/11 format). After CLN-03, the `has_pkfk` branch check is removed, so `resolve_joins_pkfk()` is always called. If the graph is empty (no `fk_columns`), `resolve_joins_pkfk()` returns an empty alias list — which means no JOINs are emitted.
**Why it happens:** `resolve_joins_pkfk()` only processes joins where `!join.fk_columns.is_empty()`.
**How to avoid:** The project decision is no backward compat (STATE.md: "NO backward compatibility needed -- pre-release, old syntax removed entirely"). Legacy stored definitions should not exist in practice. But confirm the behavior: `resolve_joins_pkfk()` returns `Vec::new()` for legacy definitions, which means legacy multi-table views silently become single-table queries. This is acceptable given the no-backward-compat policy, but add a comment noting this.
**Warning signs:** Queries against old views return fewer rows than expected (missing JOINs).

### Pitfall 3: EXP-05 verification is incomplete
**What goes wrong:** The existing Phase 26 tests use qualified column refs in expression strings (`sum(o.amount)`, `c.name`), but they don't explicitly verify that the generated SQL contains the qualified reference as-is. If `expand()` were to strip the qualifier, the tests might still pass for simple cases.
**How to avoid:** Add a Rust unit test that calls `expand()` directly and asserts the generated SQL contains `c.name` (or similar) verbatim in the SELECT clause.

### Pitfall 4: `validate_brackets()` left as dead code
**What goes wrong:** After removing `validate_clauses()`, `validate_brackets()` may become dead code. Clippy will warn or the build will fail if `#[allow(dead_code)]` is not present.
**How to avoid:** Delete `validate_brackets()` if it has no other callers, or add an allow attribute if it's being kept for future use.

## Code Examples

### Verified: Current expand() output with qualified column refs
From Phase 26 sqllogictest (test/sql/phase26_join_resolution.test):

```sql
-- DDL with qualified column refs in expressions
CREATE SEMANTIC VIEW p26_sales AS
  TABLES (
    o AS p26_orders PRIMARY KEY (id),
    c AS p26_customers PRIMARY KEY (id)
  )
  RELATIONSHIPS (
    order_to_customer AS o(customer_id) REFERENCES c
  )
  DIMENSIONS (
    c.customer_name AS c.name      -- expr = "c.name", stored verbatim
  )
  METRICS (
    o.total_amount AS sum(o.amount) -- expr = "sum(o.amount)", stored verbatim
  );

-- This query works and returns correct results:
SELECT * FROM semantic_view('p26_sales',
  dimensions := ['customer_name'],
  metrics := ['total_amount']);
-- Returns: Alice 300.00 / Bob 50.00 / NULL 75.00
```

Expected generated SQL from `expand()`:
```sql
SELECT
    c.name AS "customer_name",
    sum(o.amount) AS "total_amount"
FROM "p26_orders" AS "o"
LEFT JOIN "p26_customers" AS "c" ON "o"."customer_id" = "c"."id"
GROUP BY
    1
```

### Verified: resolve_joins_pkfk() returns empty for legacy joins
From expand.rs, `resolve_joins_pkfk()` only processes joins with non-empty `fk_columns`:

```rust
// In resolve_joins_pkfk, the graph is built via RelationshipGraph::from_definition
// which only iterates joins where !join.fk_columns.is_empty()
let Ok(graph) = RelationshipGraph::from_definition(def) else {
    return Vec::new();
};
```

### Deletion target: scan_clause_keywords (parse.rs ~lines 435-494)
The entire function can be deleted after verifying it is only called from `validate_clauses()`:

```rust
// DELETE THIS FUNCTION (CLN-01)
fn scan_clause_keywords(body: &str, body_offset: usize) -> Result<Vec<String>, ParseError> {
    // ... detects keyword := or keyword( pattern
}
```

### Deletion target: resolve_joins (expand.rs ~lines 169-235)
The entire function can be deleted after removing the `else` branch in `expand()`:

```rust
// DELETE THIS FUNCTION (CLN-03)
fn resolve_joins<'a>(
    joins: &'a [Join],
    resolved_dims: &[&crate::model::Dimension],
    resolved_mets: &[&crate::model::Metric],
    def: &SemanticViewDefinition,
) -> Vec<&'a Join> {
    // ... substring-matching transitive dependency resolution
}
```

## State of the Art

| Old Approach | Current Approach | When Changed |
|--------------|------------------|--------------|
| Paren-body DDL: `CREATE SEMANTIC VIEW name (tables := [...])` | AS-body DDL: `CREATE SEMANTIC VIEW name AS TABLES (...) ...` | Phase 25 |
| CTE-wrapped `_base` flattening | Direct `FROM table AS alias LEFT JOIN ...` | Phase 26 |
| ON-clause substring matching for transitive joins | Graph-based topological sort with PK/FK declarations | Phase 26 |
| Unqualified column refs in expressions | Qualified `alias.column` refs that resolve against aliased JOINs | Phase 26 (structural support already present) |

**Deprecated/outdated:**
- `scan_clause_keywords()`: superseded by `find_clause_bounds()` in `body_parser.rs`
- `resolve_joins()`: superseded by `resolve_joins_pkfk()` in `expand.rs`
- `append_join_on_clause()`: superseded by `synthesize_on_clause()` in `expand.rs`
- `validate_clauses()`: superseded by validation inside `find_clause_bounds()` in `body_parser.rs`
- `parse_create_body()`: superseded by `parse_keyword_body()` in `body_parser.rs` (for CREATE-with-body; but `parse_ddl_text()` in `parse.rs` still calls `parse_create_body()` — check if `parse_ddl_text()` itself is still needed)

## Open Questions

1. **Are there callers of `validate_clauses()` outside of `validate_create_body()`?**
   - What we know: `validate_clauses()` is called from line 710 in `validate_create_body()`
   - What's unclear: Are there any test files or other modules that call it directly?
   - Recommendation: `grep -r validate_clauses src/` before deletion

2. **Are there callers of `validate_brackets()` outside of `validate_clauses()`?**
   - What we know: It is called at line 521 in `validate_clauses()`
   - What's unclear: Any other callers?
   - Recommendation: `grep -r validate_brackets src/` before deletion

3. **Is `parse_create_body()` or `parse_ddl_text()` still needed after CLN-01?**
   - What we know: `parse_ddl_text()` calls `parse_create_body()` for paren-body parsing; `rewrite_ddl()` also calls `parse_create_body()`
   - What's unclear: After removing the paren-body dispatch, does `rewrite_ddl()` still support paren-body syntax? The answer is yes — `rewrite_ddl()` calls `parse_create_body()` directly. So if CLN-01 only removes the `:=` scanning (clause keyword validation) but keeps the paren-body rewrite path, these functions survive. If CLN-01 removes paren-body entirely, they must go.
   - Recommendation: The REQUIREMENTS say "remove old `:=`/struct literal DDL body parsing". The paren-body DDL body IS the struct literal format (it uses `tables := [{...}]`). Remove `parse_create_body()` and `parse_ddl_text()` too, and update or delete old tests.

4. **Does `build_execution_sql()` wrapping work with direct FROM+JOIN SQL?**
   - What we know: From STATE.md: "Research flag: verify `build_execution_sql` type-cast wrapper works with direct FROM+JOIN SQL (spike before Phase 27)". The wrapper is `SELECT {casts} FROM ({expanded_sql}) __sv_inner`.
   - What's unclear: Can DuckDB resolve `c.name` inside `__sv_inner` when the subquery uses `FROM orders AS c`?
   - Answer from code review: YES. The expansion emits flat SQL (`FROM t1 AS a LEFT JOIN t2 AS b ON ...`) — the alias `c` is in scope within the subquery. DuckDB can reference `c.name` inside the inner subquery just fine. The `build_execution_sql()` outer wrapper adds a `SELECT {casts} FROM (...) __sv_inner`, and at that outer level the qualified names become just column names. The `column_names` used in the casts are the output alias names (e.g., `"customer_name"`), not the original qualified expressions — so there is no conflict.
   - Recommendation: Confidence HIGH — this concern from STATE.md is resolved. The Phase 26 test already exercises this path.

5. **Which older `.test` files use paren-body DDL syntax and need to be updated?**
   - Files to audit: `phase16_parser.test`, `phase19_parser_hook_validation.test`, `phase20_extended_ddl.test`, `semantic_views.test`, `phase2_ddl.test`, `phase4_query.test`
   - Recommendation: Planner should include a task to audit and update/remove all paren-body DDL usages in `.test` files as part of CLN-01.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` + `sqllogictest` runner |
| Config file | `justfile` (project root) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| EXP-05 | Qualified column refs (`alias.column`) emit verbatim in SQL | unit | `cargo test -- expand` | Partially (Phase 26 tests use them but don't assert SQL text) |
| CLN-01 | Old paren-body `:=` DDL no longer accepted | unit + slt | `cargo test -- parse` and `just test-sql` | ❌ New tests needed |
| CLN-03 | `resolve_joins()` removed; PK/FK path is the only join resolver | unit | `cargo test -- expand` | ❌ New tests needed |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just build && just test-sql`
- **Phase gate:** `just test-all` green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase27_qualified_refs.test` — sqllogictest for EXP-05: dot-qualified expressions in SELECT, GROUP BY correctness
- [ ] Unit test in `src/expand.rs` asserting `expand()` output contains `c.name` verbatim for a qualified-expression dimension
- [ ] Audit + update old `.test` files that use paren-body DDL syntax (prerequisite for CLN-01)

## Sources

### Primary (HIGH confidence)
- Direct codebase inspection — `src/parse.rs`, `src/expand.rs`, `src/body_parser.rs`, `src/model.rs`, `src/graph.rs`
- `test/sql/phase26_join_resolution.test` — verified EXP-05 works in practice
- `.planning/REQUIREMENTS.md` — requirement definitions
- `.planning/STATE.md` — project decisions and blockers

### Secondary (MEDIUM confidence)
- DuckDB SQL semantics: table aliases in FROM/JOIN clauses are in scope for the full SELECT (standard SQL; DuckDB-specific behavior confirmed by Phase 26 test passing)

## Metadata

**Confidence breakdown:**
- Deletion targets (CLN-01, CLN-03): HIGH — code read directly, single call-site verified
- EXP-05 verification: HIGH — Phase 26 tests already exercise qualified refs; main work is adding explicit assertion
- Old test file impact: MEDIUM — audit not yet performed (filenames known, contents partially read)
- build_execution_sql wrapper compatibility: HIGH — resolved via code review and Phase 26 passing tests

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable codebase, no external dependencies changing)
