# Phase 50: Code Quality & Test Coverage - Research

**Researched:** 2026-04-14
**Domain:** Rust code quality -- unit testing, deduplication, newtypes, dead code removal
**Confidence:** HIGH

## Summary

Phase 50 is a pure code-quality phase with no new features. It addresses six concrete improvements: adding unit tests to three untested expand modules (`join_resolver.rs`, `fan_trap.rs`, `facts.rs`), deduplicating resolution loops in `expand()` and `expand_facts()`, introducing `DimensionName`/`MetricName` newtypes to centralize case-insensitive comparison, replacing a tuple type with a named struct in `semi_additive.rs`, removing dead code in `model.rs`, and migrating brittle exact-string tests to structural property assertions in `sql_gen.rs`.

All changes are internal refactors with no DDL syntax changes, no new SQL expansion paths, and no persistence format changes. The primary risk is regression in the 576+ existing unit tests and 42+ proptests if refactoring breaks subtle behavior. The test infrastructure is mature (cargo test + sqllogictest + DuckLake CI + integration tests), so regressions will be caught by `just test-all`.

**Primary recommendation:** Split into two plans: (1) Tests-first plan covering QUAL-01 and QUAL-06 (new unit tests + test improvements), then (2) Refactoring plan covering QUAL-02 through QUAL-05 (deduplication, newtypes, named struct, dead code removal). Tests-first ensures we have baseline coverage before refactoring.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| QUAL-01 | `expand/join_resolver.rs`, `expand/fan_trap.rs`, and `expand/facts.rs` each have unit tests covering normal paths and edge cases (empty inputs, circular graphs, missing references) | See "QUAL-01: Unit Test Coverage" section -- identified 8 untested functions across 3 files, existing test helpers support all needed fixtures |
| QUAL-02 | Dimension/metric/fact resolution loops in `expand()` and `expand_facts()` are deduplicated into a shared generic helper | See "QUAL-02: Resolution Loop Deduplication" section -- identified identical patterns at sql_gen.rs lines 61-79 and 244-263 |
| QUAL-03 | `DimensionName` and `MetricName` newtypes replace bare `String` in `QueryRequest` and resolution code, with case-insensitive comparison consolidated in one place | See "QUAL-03: Domain Newtypes" section -- 189 case-insensitive comparisons across 20 files, newtype consolidates this |
| QUAL-04 | Named `NaGroup` struct replaces tuple `Vec<(Vec<String>, Vec<NonAdditiveDim>, Vec<usize>)>` in `semi_additive.rs` | See "QUAL-04: Named NaGroup Struct" section -- single function `collect_na_groups` at line 312 |
| QUAL-05 | Dead `parse_constraint_columns()` in `model.rs` is removed | See "QUAL-05: Dead Code Removal" section -- function at model.rs:421, already has `#[allow(dead_code)]` |
| QUAL-06 | `sql_gen.rs` tests use structural property assertions instead of exact string equality where appropriate | See "QUAL-06: Test Assertion Improvements" section -- 5 exact-match tests identified, 12 already use property assertions |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Quality gate**: `just test-all` must pass (Rust unit tests + SQL logic tests + DuckLake CI + integration tests)
- **Build commands**: `just build` for debug, `cargo test` for unit tests, `just test-sql` requires fresh `just build`
- **Test completeness**: A phase verification that only runs `cargo test` is INCOMPLETE -- sqllogictest covers integration paths Rust tests do not

## Architecture Patterns

### Existing Test Module Pattern

Tests in this codebase follow a consistent pattern. Each source file with tests has a `#[cfg(test)] mod tests { ... }` block at the bottom. Submodules within `mod tests` are used for grouping (e.g., `mod expand_tests`, `mod error_tests`). [VERIFIED: codebase grep]

```rust
// Source: src/expand/sql_gen.rs (existing pattern)
#[cfg(test)]
mod tests {
    use crate::expand::{expand, ExpandError, QueryRequest};

    mod expand_tests {
        use super::*;
        use crate::expand::test_helpers::{minimal_def, orders_view, TestFixtureExt};
        // ...tests...
    }
}
```

### Test Helpers (test_helpers.rs)

The expand module has a shared `test_helpers.rs` providing: [VERIFIED: codebase read]
- `orders_view()` -- base fixture with 2 dims, 2 metrics, no joins
- `minimal_def(base_table, dim_name, dim_expr, metric_name, metric_expr)` -- minimal fixture
- `TestFixtureExt` trait -- builder-style chaining (`.with_dimension()`, `.with_metric()`, `.with_table()`, `.with_pkfk_join()`, `.with_fact()`, `.with_non_additive_by()`, `.with_window_spec()`, etc.)

These helpers are sufficient for all QUAL-01 test scenarios. No new test infrastructure needed.

### Newtype Pattern in Rust

Standard Rust newtype for domain strings with custom `Eq`/`Hash`: [VERIFIED: Rust reference]

```rust
// Newtype wrapping String with case-insensitive comparison
#[derive(Debug, Clone)]
pub struct DimensionName(String);

impl DimensionName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for DimensionName {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for DimensionName {}

impl std::hash::Hash for DimensionName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the lowercased form for consistency with Eq
        for byte in self.0.bytes() {
            byte.to_ascii_lowercase().hash(state);
        }
    }
}
```

### Resolution Loop Generic Helper Pattern

The duplicated loops in `expand()` and `expand_facts()` follow this shape: [VERIFIED: codebase read]

```rust
let mut resolved: Vec<&T> = Vec::with_capacity(names.len());
let mut seen = HashSet::new();
for name in names {
    if !seen.insert(name.to_ascii_lowercase()) {
        return Err(duplicate_error(name));
    }
    let item = find_by_name(def, name)
        .ok_or_else(|| not_found_error(name))?;
    if item.is_private() {
        return Err(private_error(name));
    }
    resolved.push(item);
}
```

This can be extracted into a generic function parameterized by the item type, finder function, and error constructors.

### Anti-Patterns to Avoid

- **Breaking public API during refactor**: `QueryRequest` is used in `sql_gen::expand()` which is a public API. If `DimensionName`/`MetricName` newtypes replace `Vec<String>`, the public API changes. Consider whether newtypes should live in `QueryRequest` (breaking change) or only in internal resolution code (non-breaking). The success criteria says "replace bare `String` in `QueryRequest`", so the public API change is intentional. [ASSUMED]
- **Over-abstracting**: The resolution helper should be a simple function, not a trait or macro. The three loops (dim, metric, fact) have slightly different error types and privacy checks but the same structure.
- **Changing test behavior during test improvement**: QUAL-06 says "where appropriate" -- some tests (like `test_basic_single_dimension_single_metric`) are regression anchors that SHOULD use exact string equality to catch unintended changes.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Case-insensitive string comparison | `to_ascii_lowercase()` at every call site | `DimensionName`/`MetricName` newtype with `Eq` impl | 189 call sites, centralized logic prevents bugs |
| Test deduplication verification | Manual before/after diffing | `cargo test` -- existing 576+ tests serve as regression suite | Any behavioral change will be caught |

## Detailed Analysis per Requirement

### QUAL-01: Unit Test Coverage

**Current state**: [VERIFIED: codebase grep]
- `join_resolver.rs`: 0 unit tests. 3 functions (`synthesize_on_clause`, `synthesize_on_clause_scoped`, `resolve_joins_pkfk`)
- `fan_trap.rs`: 0 unit tests. 5 functions (`check_fan_traps`, `ancestors_to_root`, `validate_fact_table_path`, `check_path_up`, `check_path_down`, `path_from_ancestor_to_node`)
- `facts.rs`: Has 7 tests but only covers `toposort_derived`, `inline_derived_metrics`, and `MAX_DERIVATION_DEPTH`. Missing tests for: `collect_derived_metric_using`, `toposort_facts`, `inline_facts`, `collect_derived_metric_source_tables`

**Tests needed**:

For `join_resolver.rs`:
1. `synthesize_on_clause` -- normal PK/FK pair, composite keys, empty fk_columns
2. `synthesize_on_clause_scoped` -- scoped alias (role-playing), ref_columns fallback to pk_columns
3. `resolve_joins_pkfk` -- single join, transitive joins, scoped aliases, no joins needed, derived metric USING

For `fan_trap.rs`:
1. `check_fan_traps` -- no joins (trivially ok), ManyToOne safe direction, ManyToOne fan-out direction, OneToOne safe, semi-additive skip, window skip, derived metric traversal
2. `ancestors_to_root` -- root node, single parent, multi-level chain
3. `validate_fact_table_path` -- single table (trivially ok), ancestor/descendant pair ok, divergent tables error, no joins
4. `path_from_ancestor_to_node` -- ancestor is target (single node path), normal path, ancestor not in chain (fallback)

For `facts.rs` (additional tests):
1. `collect_derived_metric_using` -- base metric with USING, derived metric with transitive USING, no USING
2. `toposort_facts` -- empty facts, single fact, chain dependencies, independent facts, cycle detection
3. `inline_facts` -- no facts (passthrough), single fact substitution, qualified form (source_table.name), chained facts in topo order
4. `collect_derived_metric_source_tables` -- base metric (direct source), derived metric (transitive sources), cycle handling

**Test helpers available**: `TestFixtureExt` already has `.with_table()`, `.with_pkfk_join()`, `.with_fact()`, `.with_metric()`, `.with_dimension()` -- all needed builders exist. [VERIFIED: codebase read]

### QUAL-02: Resolution Loop Deduplication

**Current state**: [VERIFIED: codebase read]

The dimension resolution loop appears identically in:
- `expand_facts()` at lines 60-79 (resolves `req.dimensions`)
- `expand()` at lines 243-263 (resolves `req.dimensions`)

The metric resolution loop at `expand()` lines 265-295 has the same structure but with:
- Different finder function (`find_metric` vs `find_dimension`)
- Different error variants (`DuplicateMetric`/`UnknownMetric` vs `DuplicateDimension`/`UnknownDimension`)
- Additional `PrivateMetric` check

The fact resolution loop at `expand_facts()` lines 26-57 also follows the pattern with `PrivateFact` check.

**Recommended approach**: Extract a generic helper function that takes:
- The name list (`&[String]`)
- A finder closure `Fn(&str) -> Option<&T>`
- Error constructors for duplicate, not-found, and optional private check

```rust
fn resolve_items<'a, T>(
    names: &[String],
    view_name: &str,
    find_fn: impl Fn(&str) -> Option<&'a T>,
    access_check: impl Fn(&T, &str) -> Option<ExpandError>,
    make_dup_err: impl Fn(&str) -> ExpandError,
    make_not_found_err: impl Fn(&str) -> ExpandError,
) -> Result<Vec<&'a T>, ExpandError>
```

This reduces ~60 lines of duplicated code across 3 call sites to ~20 lines total (function + 3 one-liner calls).

### QUAL-03: Domain Newtypes

**Current state**: [VERIFIED: codebase grep]
- 189 occurrences of `eq_ignore_ascii_case` or `to_ascii_lowercase` across 20 source files
- `QueryRequest` uses `Vec<String>` for `dimensions`, `metrics`, and `facts`
- Resolution functions use `name.to_ascii_lowercase()` or `name.eq_ignore_ascii_case()` ad hoc

**Scope**: The success criteria specifies `DimensionName` and `MetricName` newtypes in `QueryRequest` and resolution code. This means:
1. Define newtypes in `model.rs` (or a new `names.rs` module)
2. Change `QueryRequest.dimensions` from `Vec<String>` to `Vec<DimensionName>`
3. Change `QueryRequest.metrics` from `Vec<String>` to `Vec<MetricName>`
4. Update resolution code (`find_dimension`, `find_metric`) to accept newtypes
5. `Display`, `Deref`, `From<String>`, `From<&str>` impls for ergonomic use

**Impact scope**: This changes the public API of `QueryRequest`. All callers (test code, `table_function.rs`, proptests) need updating. The `facts` field is NOT mentioned in success criteria, so it stays as `Vec<String>`. [ASSUMED]

**Case-insensitive PartialEq/Hash**: The newtype should implement `PartialEq` and `Hash` using ASCII-lowercased form. This automatically makes `HashSet::insert` and `HashMap::get` work correctly without explicit `to_ascii_lowercase()` at call sites.

**Risk**: Serde compatibility -- `QueryRequest` is not serialized (it's a runtime struct created from VTab parameters), so no backward-compat concern. [VERIFIED: codebase read -- QueryRequest is constructed in table_function.rs from VARCHAR parameters]

### QUAL-04: Named NaGroup Struct

**Current state**: [VERIFIED: codebase read]

In `semi_additive.rs`, `collect_na_groups` returns `Vec<(Vec<NonAdditiveDim>, Vec<usize>)>` and internally uses `Vec<(Vec<String>, Vec<NonAdditiveDim>, Vec<usize>)>`. This triple-tuple is hard to read.

The function is at lines 312-344. Consumers access fields via `.0`, `.1`, `.2` pattern matching (lines 117, 392).

**Recommended struct**:

```rust
/// A group of metrics sharing the same NON ADDITIVE BY dimension set.
struct NaGroup {
    /// Lowercased dimension name keys (used for grouping equality check)
    key: Vec<String>,
    /// The actual NonAdditiveDim entries for this group
    na_dims: Vec<NonAdditiveDim>,
    /// Indices into resolved_mets that belong to this group
    metric_indices: Vec<usize>,
}
```

**Scope**: 1 struct definition, 1 function return type change, ~6 access sites in `expand_semi_additive` and `get_rn_column_for_metric`.

### QUAL-05: Dead Code Removal

**Current state**: [VERIFIED: codebase read]

`parse_constraint_columns()` at `model.rs:421` has `#[allow(dead_code)]` annotation and 5 associated tests (lines 510-541). It was used in early phases for parsing DuckDB constraint output but is no longer called anywhere.

**Action**: Remove the function and its tests. Remove the `#[allow(dead_code)]` annotation. Straightforward deletion, no callers to update.

### QUAL-06: Test Assertion Improvements

**Current state**: [VERIFIED: codebase read]

In `sql_gen.rs` tests:
- 5 tests use `assert_eq!(sql, expected)` with full exact string comparison
- 12 tests use `assert!(sql.contains(...))` structural assertions
- 95 total `#[test]` functions in the file

The 5 exact-match tests:
1. `test_basic_single_dimension_single_metric` (line 525)
2. `test_multiple_dimensions_multiple_metrics` (line 547)
3. `test_global_aggregate_no_dimensions` (line 563)
4. `test_dimensions_only_generates_distinct` (line 631)
5. `test_metrics_only_still_works` (line 648)

**Recommendation**: Keep 1-2 "golden file" exact-match tests as regression anchors (e.g., `test_basic_single_dimension_single_metric`). Convert the rest to property assertions that check:
- Contains `SELECT` / `SELECT DISTINCT`
- Contains expected dimension/metric aliases
- Contains `FROM "table"` with correct table
- Contains `GROUP BY` or doesn't (structural property)
- Correct number of GROUP BY ordinals

This makes tests resilient to whitespace/formatting changes while still verifying correctness.

## Common Pitfalls

### Pitfall 1: Newtype Breaks Downstream Callers
**What goes wrong:** Changing `QueryRequest.dimensions` from `Vec<String>` to `Vec<DimensionName>` breaks every test and call site that constructs a `QueryRequest`.
**Why it happens:** There are ~95 test functions in sql_gen.rs alone that build `QueryRequest`, plus table_function.rs, plus proptests.
**How to avoid:** Implement `From<String>` and `From<&str>` for the newtypes so `.into()` or `.map(DimensionName::new)` works with minimal churn. Consider providing a `QueryRequest::new()` constructor that accepts `Vec<String>` and converts internally.
**Warning signs:** Compilation errors in 50+ locations after the type change.

### Pitfall 2: Refactoring Changes Observable Behavior
**What goes wrong:** The generic resolution helper subtly changes error message content, ordering, or which error is returned first.
**Why it happens:** Different error handling paths (duplicate vs not-found vs private) may execute in different order.
**How to avoid:** Write QUAL-01 tests FIRST (before refactoring). Ensure the new generic helper preserves exact error variant selection. Run `just test-all` after each change.
**Warning signs:** Test failures in error message content or error variant matching.

### Pitfall 3: Hash Inconsistency with Eq
**What goes wrong:** `DimensionName` implements case-insensitive `Eq` but case-sensitive `Hash`, causing `HashSet`/`HashMap` to break.
**Why it happens:** Rust requires `Hash` to be consistent with `Eq`: `a == b` implies `hash(a) == hash(b)`.
**How to avoid:** Hash the lowercased bytes, not the original bytes. Test explicitly that `DimensionName("Foo")` and `DimensionName("foo")` produce the same hash.
**Warning signs:** Duplicate entries in HashSets, missing lookups in HashMaps.

### Pitfall 4: Removing Dead Code That Has Hidden Users
**What goes wrong:** `parse_constraint_columns()` might be used via `#[cfg(feature = "extension")]` or integration tests.
**Why it happens:** The `#[allow(dead_code)]` annotation masks compiler warnings.
**How to avoid:** Grep the entire codebase (not just `src/`) for `parse_constraint_columns` before removing.
**Warning signs:** Compilation errors under `--features extension`.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + proptest + sqllogictest |
| Config file | Cargo.toml (test config), justfile (task runner) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| QUAL-01 | join_resolver.rs unit tests | unit | `cargo test expand::join_resolver --lib` | No -- Wave 0 |
| QUAL-01 | fan_trap.rs unit tests | unit | `cargo test expand::fan_trap --lib` | No -- Wave 0 |
| QUAL-01 | facts.rs additional unit tests | unit | `cargo test expand::facts --lib` | Partial -- existing tests cover derived metrics only |
| QUAL-02 | Resolution loop deduplication | unit (regression) | `cargo test expand::sql_gen --lib` | Yes -- 95 existing tests |
| QUAL-03 | DimensionName/MetricName newtypes | unit | `cargo test model --lib` | No -- Wave 0 for newtype tests |
| QUAL-04 | NaGroup named struct | unit (regression) | `cargo test expand::semi_additive --lib` | Yes -- 10 existing tests |
| QUAL-05 | Dead code removal | unit (regression) | `cargo test model --lib` | Yes -- verify removed tests don't break |
| QUAL-06 | sql_gen.rs property assertions | unit | `cargo test expand::sql_gen --lib` | Yes -- modify existing |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `join_resolver.rs` `#[cfg(test)] mod tests` -- covers QUAL-01 (join resolver tests)
- [ ] `fan_trap.rs` `#[cfg(test)] mod tests` -- covers QUAL-01 (fan trap tests)
- [ ] Additional tests in `facts.rs` -- covers QUAL-01 (fact helper tests)
- [ ] Newtype unit tests -- covers QUAL-03 (Eq, Hash, Display consistency)

*(All new tests are part of the implementation -- no separate framework install needed)*

## Code Examples

### Property Assertion Pattern for QUAL-06

```rust
// Source: recommended pattern based on existing codebase conventions
#[test]
fn test_multiple_dimensions_multiple_metrics() {
    let def = orders_view();
    let req = QueryRequest {
        facts: vec![],
        dimensions: vec!["region".to_string(), "status".to_string()],
        metrics: vec!["total_revenue".to_string(), "order_count".to_string()],
    };
    let sql = expand("orders", &def, &req).unwrap();

    // Structural properties instead of exact string match
    assert!(sql.starts_with("SELECT\n"), "Should start with SELECT");
    assert!(sql.contains("region AS \"region\""), "Should include region dim");
    assert!(sql.contains("status AS \"status\""), "Should include status dim");
    assert!(sql.contains("sum(amount) AS \"total_revenue\""), "Should include revenue metric");
    assert!(sql.contains("count(*) AS \"order_count\""), "Should include count metric");
    assert!(sql.contains("FROM \"orders\""), "Should reference orders table");
    assert!(sql.contains("GROUP BY\n    1,\n    2"), "Should group by 2 dims");
}
```

### Generic Resolution Helper for QUAL-02

```rust
// Source: recommended pattern based on codebase analysis
fn resolve_names<'a, T>(
    names: &[String],
    view_name: &str,
    find_fn: impl Fn(&str) -> Option<&'a T>,
    access_check: Option<&dyn Fn(&T) -> bool>,  // returns true if private
    dup_err: impl Fn(String, String) -> ExpandError,
    not_found_err: impl Fn(String, String) -> ExpandError,
    private_err: impl Fn(String, String) -> ExpandError,
) -> Result<Vec<&'a T>, ExpandError> {
    let mut resolved = Vec::with_capacity(names.len());
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(dup_err(view_name.to_string(), name.clone()));
        }
        let item = find_fn(name)
            .ok_or_else(|| not_found_err(view_name.to_string(), name.clone()))?;
        if let Some(check) = access_check {
            if check(item) {
                return Err(private_err(view_name.to_string(), name.clone()));
            }
        }
        resolved.push(item);
    }
    Ok(resolved)
}
```

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The public API change to QueryRequest (Vec<String> -> Vec<DimensionName>) is intentional per success criteria | QUAL-03 | Medium -- if API stability is preferred, newtypes could be internal-only |
| A2 | `facts` field in QueryRequest stays as Vec<String> (not mentioned in success criteria) | QUAL-03 | Low -- could add FactName newtype if desired |
| A3 | parse_constraint_columns has no hidden callers under cfg(feature = "extension") | QUAL-05 | Low -- compiler will catch if wrong |

## Open Questions

1. **Newtype scope: QueryRequest only, or deeper?**
   - What we know: Success criteria says "in QueryRequest and resolution code"
   - What's unclear: Should newtypes propagate into SemanticViewDefinition (Dimension.name, Metric.name)?
   - Recommendation: Keep newtypes in QueryRequest and resolution code only. Model types stay as String to avoid serde complexity. Can expand later.

2. **How many exact-match tests to keep?**
   - What we know: 5 tests use exact match, success criteria says "where appropriate"
   - What's unclear: Which tests are the right "golden file" regression anchors
   - Recommendation: Keep `test_basic_single_dimension_single_metric` as the single golden-file anchor. Convert the other 4 to property assertions.

## Sources

### Primary (HIGH confidence)
- Codebase analysis: All 12 expand/ source files read and analyzed
- `src/expand/join_resolver.rs` -- 0 tests confirmed, 3 public functions identified
- `src/expand/fan_trap.rs` -- 0 tests confirmed, 6 functions identified
- `src/expand/facts.rs` -- 7 existing tests, 4 untested functions identified
- `src/expand/sql_gen.rs` -- 95 tests, 5 exact-match tests identified
- `src/expand/semi_additive.rs` -- tuple type at line 316, 6 access sites
- `src/model.rs` -- dead `parse_constraint_columns` at line 421 with `#[allow(dead_code)]`
- `src/expand/types.rs` -- QueryRequest definition with `Vec<String>` fields

### Secondary (MEDIUM confidence)
- Rust newtype pattern: standard Rust idiom, well-documented

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- pure Rust refactoring, no external dependencies
- Architecture: HIGH -- all code analyzed, patterns clear
- Pitfalls: HIGH -- common Rust refactoring pitfalls well-understood
- Test coverage gaps: HIGH -- confirmed via codebase grep

**Research date:** 2026-04-14
**Valid until:** 2026-05-14 (stable -- internal refactoring, no external dependency drift)
