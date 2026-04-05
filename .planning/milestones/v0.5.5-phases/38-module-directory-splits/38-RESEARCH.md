# Phase 38: Module Directory Splits - Research

**Researched:** 2026-04-01
**Domain:** Rust module refactoring -- decomposing large files into module directories with single-responsibility submodules
**Confidence:** HIGH

## Summary

Phase 38 converts the two largest source files (`expand.rs` at 4,299 lines and `graph.rs` at 2,333 lines) into module directories (`src/expand/` and `src/graph/`) with single-responsibility submodules. This is a purely mechanical, behavior-preserving refactoring. No public API changes are made -- `mod.rs` re-exports everything that was previously `pub` or `pub(crate)`.

The expand.rs file contains seven logical clusters: (1) types and error definitions, (2) SQL quoting utilities, (3) request validation/resolution, (4) join resolution, (5) fact inlining, (6) fan trap detection with path helpers, and (7) the main `expand()` SQL generation function. These map cleanly to the submodules specified in REF-01.

The graph.rs file contains five logical clusters: (1) the `RelationshipGraph` struct with builder/toposort/validation methods, (2) fact validation, (3) derived metric validation, (4) USING relationship validation, and (5) shared helpers (cycle path finder, aggregate function detection). These map to the submodules specified in REF-02.

The key risk is Rust's "file vs directory" module resolution: when `src/expand.rs` coexists with `src/expand/`, the compiler emits an ambiguity error. The old files MUST be deleted in the same commit that creates the new directories. Phase 37 already broke circular dependencies, so no new dependency issues arise.

**Primary recommendation:** Split each file into the prescribed submodules, use `mod.rs` with `pub use` re-exports to preserve the exact public API, delete the old files, and verify all 390+ tests pass unchanged.

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass (Rust unit tests + sqllogictest + DuckLake CI)
- **Build:** `just build` for debug extension; `cargo test` for unit tests; `just test-sql` requires `just build` first
- **Snowflake reference:** If in doubt about SQL syntax or behaviour refer to Snowflake semantic views
- **Test completeness:** A phase verification that only runs `cargo test` is incomplete -- sqllogictest covers integration paths that Rust tests do not

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REF-01 | `expand.rs` split into `expand/` module directory with submodules (validation, resolution, facts, fan_trap, role_playing, join_resolver, sql_gen) and `mod.rs` re-exports full prior public API | Detailed function-to-submodule mapping below; 7 public items + 2 pub(crate) items to re-export; all external consumers identified |
| REF-02 | `graph.rs` split into `graph/` module directory with submodules (relationship, facts, derived_metrics, using, toposort) and `mod.rs` re-exports full prior public API | Detailed function-to-submodule mapping below; 7 public items (1 struct + 6 functions) to re-export; all external consumers identified |
| REF-05 | All existing tests pass after refactoring with zero behavior changes | 390 Rust unit tests + sqllogictest + DuckLake CI; test modules move to respective submodules or `test_helpers.rs` files |
</phase_requirements>

## Standard Stack

No new dependencies. This phase is purely internal refactoring within existing Rust code.

### Core (existing, unchanged)
| Library | Version | Purpose | Relevant to Phase |
|---------|---------|---------|-------------------|
| strsim | 0.11 | Levenshtein distance for fuzzy matching | Used by `graph` submodules via `crate::util::suggest_closest` |

### No New Dependencies

This is a code-move refactoring. No new crates, no version changes.

## Architecture Patterns

### Current Module Structure (before)

```
src/
  lib.rs          # pub mod expand; pub mod graph;
  expand.rs       # 4,299 lines -- 7 logical clusters
  graph.rs        # 2,333 lines -- 5 logical clusters
  util.rs         # Shared string utilities (Phase 37)
  errors.rs       # Shared ParseError (Phase 37)
  model.rs        # Data model types
  ...
```

### Target Module Structure (after)

```
src/
  lib.rs          # pub mod expand; pub mod graph; (UNCHANGED -- Rust resolves to directories)
  expand/
    mod.rs         # Re-exports: pub use types::*; pub use sql_gen::*; etc.
    types.rs       # QueryRequest, ExpandError, Display impl
    resolution.rs  # find_dimension, find_metric, quote_ident, quote_table_ref
    facts.rs       # toposort_facts, inline_facts, toposort_derived, inline_derived_metrics,
                   #   collect_derived_metric_source_tables, collect_derived_metric_using
    fan_trap.rs    # check_fan_traps, ancestors_to_root, path_from_ancestor_to_node,
                   #   check_path_up, check_path_down
    role_playing.rs # relationships_to_table, find_using_context
    join_resolver.rs # synthesize_on_clause, synthesize_on_clause_scoped, resolve_joins_pkfk
    sql_gen.rs     # pub fn expand() -- main SQL generation
    validation.rs  # Request validation (dedup checks, resolution orchestration -- embedded in expand())
  graph/
    mod.rs         # Re-exports: pub use relationship::*; pub use facts::*; etc.
    relationship.rs # RelationshipGraph struct + impl (from_definition, toposort, check_no_diamonds,
                    #   check_no_orphans), validate_graph, validate_fk_references,
                    #   check_source_tables_reachable, find_cycle_path
    facts.rs       # validate_facts, check_fact_source_tables, build_fact_dag,
                   #   check_fact_references_exist, check_fact_cycles, find_fact_references,
                   #   is_word_boundary_byte
    derived_metrics.rs # validate_derived_metrics, check_metric_name_uniqueness,
                       #   check_no_aggregates_in_derived, check_derived_metric_references,
                       #   check_derived_metric_cycles, extract_identifiers,
                       #   is_sql_keyword_or_builtin, contains_aggregate_function,
                       #   AGGREGATE_FUNCTIONS const
    using.rs       # validate_using_relationships
    toposort.rs    # (see note below -- toposort is part of RelationshipGraph impl)
```

**Note on `toposort.rs`:** The requirement lists `toposort` as a separate submodule, but `toposort()` is an `impl RelationshipGraph` method. Splitting an `impl` block across files is idiomatic in Rust (just write `impl RelationshipGraph` in the submodule with `use super::RelationshipGraph`). However, toposort only has 1 method (~50 lines), and `find_cycle_path` (its helper) is also used by `relationship.rs`. The pragmatic choice is to keep `toposort()` inside `relationship.rs` alongside the other `impl RelationshipGraph` methods and make `toposort.rs` either empty/removed or a thin delegation. Given the requirement explicitly names it, the cleanest approach is to put `toposort()` and `find_cycle_path()` in `toposort.rs` as a separate `impl RelationshipGraph` block, re-exported from `mod.rs`.

### Module Dependency Graph (after split)

```
expand/types.rs    -- no intra-crate deps
expand/resolution.rs -- imports crate::model, crate::util
expand/facts.rs    -- imports crate::model, crate::util, super::types (ExpandError not needed here)
expand/fan_trap.rs -- imports crate::model, super::types::ExpandError, super::facts::*
expand/role_playing.rs -- imports crate::model, crate::util, super::types::ExpandError, super::facts
expand/join_resolver.rs -- imports crate::model, crate::graph::RelationshipGraph, super::facts, super::role_playing
expand/sql_gen.rs  -- imports crate::model, crate::util, crate::graph, super::* (the main orchestrator)

graph/relationship.rs -- imports crate::model, crate::util
graph/toposort.rs  -- imports super::relationship::RelationshipGraph
graph/facts.rs     -- imports crate::model, crate::util
graph/derived_metrics.rs -- imports crate::model, crate::util, super::facts::find_fact_references
graph/using.rs     -- imports crate::model, crate::util
```

### Pattern: Rust File-to-Directory Module Conversion

In Rust 2021 edition, a module `foo` can be either:
- `src/foo.rs` (file module)
- `src/foo/mod.rs` (directory module)

Both are equivalent from the compiler's perspective. `lib.rs` does NOT need to change -- `pub mod expand;` resolves to `src/expand/mod.rs` automatically.

**Critical constraint:** You cannot have BOTH `src/expand.rs` AND `src/expand/mod.rs`. The compiler will emit:

```
error[E0761]: file for module `expand` found at both "src/expand.rs" and "src/expand/mod.rs"
```

Therefore, the old `.rs` file MUST be deleted before or atomically with creating the directory.

### Pattern: Re-export for API Preservation

```rust
// src/expand/mod.rs
mod types;
mod resolution;
mod facts;
mod fan_trap;
mod join_resolver;
mod role_playing;
mod sql_gen;

// Re-export everything that was previously pub
pub use types::{ExpandError, QueryRequest};
pub use resolution::{quote_ident, quote_table_ref};
pub use sql_gen::expand;

// Re-export pub(crate) items
pub(crate) use facts::{collect_derived_metric_source_tables};
pub(crate) use fan_trap::ancestors_to_root;
```

This ensures all existing `use crate::expand::{expand, QueryRequest}` imports continue to compile without changes.

### Pattern: Test Module Placement

Tests that use `super::*` belong in the submodule they test. For example:
- `expand/resolution.rs` contains `#[cfg(test)] mod tests { ... }` with quote_ident and quote_table_ref tests
- `expand/sql_gen.rs` contains `#[cfg(test)] mod tests { ... }` with expand_tests and phase_* test modules

Shared test helpers (like `make_def()`) should go in a dedicated test helper module:
```rust
// src/expand/test_helpers.rs (or inside mod.rs under #[cfg(test)])
#[cfg(test)]
pub(crate) mod test_helpers {
    // Shared builder functions
}
```

### Anti-Patterns to Avoid

- **Leaving old .rs file alongside new directory:** Compiler error E0761. Delete the old file.
- **Changing public API surface:** This refactoring MUST NOT add, remove, or rename any public items. `mod.rs` re-exports preserve the exact API.
- **Moving tests without `super::*` adjustment:** Tests using `super::*` must import from the correct new `super` context. A test in `expand/fan_trap.rs` that uses `check_fan_traps` can still use `super::*` if it's a private function in that module.
- **Creating circular submodule dependencies:** Submodules should form a DAG. `sql_gen` imports from all siblings; no sibling should import from `sql_gen`.
- **Forgetting `pub(crate)` re-exports:** `ancestors_to_root` and `collect_derived_metric_source_tables` are `pub(crate)`, not `pub`. They must be re-exported with `pub(crate) use` in `mod.rs`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Module re-exports | Manual delegation functions | `pub use submodule::Item` | Rust re-exports are zero-cost and maintain identical API |
| Test helper sharing | Duplicate helpers in each submodule | `#[cfg(test)] mod test_helpers` in mod.rs | Single source of truth, accessible from all submodule tests via `super::test_helpers` |

## Common Pitfalls

### Pitfall 1: File/Directory Ambiguity (E0761)
**What goes wrong:** Compiler refuses to build if both `src/expand.rs` and `src/expand/mod.rs` exist.
**Why it happens:** Rust 2021 resolves `pub mod expand;` by looking for EITHER file, not both.
**How to avoid:** Delete `expand.rs` and `graph.rs` in the same commit that creates the directories. A single `git mv` does not work for file-to-directory conversion; use `git rm` + create new files.
**Warning signs:** E0761 error during `cargo check`.

### Pitfall 2: Visibility Regression
**What goes wrong:** A `pub` item in the old file becomes private in the new submodule, breaking external consumers.
**Why it happens:** Functions that were `pub` in `expand.rs` need to be `pub` in their submodule AND re-exported as `pub` from `mod.rs`.
**How to avoid:** Enumerate ALL public and pub(crate) items before starting. Verify each is re-exported in `mod.rs`. Run `cargo check` after each submodule move.
**Warning signs:** E0603 (private item) or E0432 (unresolved import) errors.

### Pitfall 3: Cross-Submodule Visibility
**What goes wrong:** Private helper functions that were accessible within the single file are now in a different submodule and inaccessible.
**Why it happens:** A function like `synthesize_on_clause()` was private in `expand.rs` but used by both `resolve_joins_pkfk()` and `expand()`. After splitting, if they're in different submodules, the helper needs `pub(super)` or `pub(crate)` visibility.
**How to avoid:** Functions used across submodules within the same module directory should be `pub(super)` (visible to sibling submodules via `super::`). Map all cross-submodule calls before moving.
**Warning signs:** E0603 errors during `cargo check` after splitting.

### Pitfall 4: Test `super::*` Import Breakage
**What goes wrong:** Tests using `use super::*` stop compiling because `super` now refers to the submodule, not the old top-level file.
**Why it happens:** `super::*` in `expand/fan_trap.rs::tests` imports from `fan_trap`, not from `expand`. Private items from other submodules are not visible.
**How to avoid:** Tests that need items from multiple submodules should use `crate::expand::*` or `super::super::*` patterns. Keep tests close to the code they test.
**Warning signs:** Unresolved import errors in test modules.

### Pitfall 5: Doc-test Path Changes
**What goes wrong:** Doc-tests reference `semantic_views::expand::quote_ident` -- this path must still work.
**Why it happens:** Doc-tests use the crate's public API path, not internal module paths.
**How to avoid:** Since `mod.rs` re-exports `pub use resolution::quote_ident`, the external path `semantic_views::expand::quote_ident` is preserved. Verify with `cargo test --doc`.
**Warning signs:** Doc-test failures.

## Detailed Function-to-Submodule Mapping

### expand.rs Decomposition

#### expand/types.rs (lines 1-166 of expand.rs)
```
Imports: std::fmt
Items:
  - pub struct QueryRequest           (line 16)
  - pub enum ExpandError              (line 23)
  - impl fmt::Display for ExpandError (line 64)
```

#### expand/resolution.rs (lines 167-248 of expand.rs)
```
Imports: crate::model::{SemanticViewDefinition, Dimension, Metric}
Items:
  - pub fn quote_ident()              (line 167)  -- pub, has doc-tests
  - pub fn quote_table_ref()          (line 180)  -- pub
  - fn find_dimension()               (line 193)  -- private, used by sql_gen
  - fn find_metric()                  (line 225)  -- private, used by sql_gen
```
Note: `find_dimension` and `find_metric` need `pub(super)` since they're used by `sql_gen.rs`.

#### expand/join_resolver.rs (lines 255-596 of expand.rs)
```
Imports: crate::model::*, crate::graph::RelationshipGraph, super::facts::*, super::role_playing::*
Items:
  - fn synthesize_on_clause()         (line 255)  -- needs pub(super), used by sql_gen
  - fn synthesize_on_clause_scoped()  (line 268)  -- needs pub(super), used by sql_gen
  - fn resolve_joins_pkfk()           (line 464)  -- needs pub(super), used by sql_gen
```

#### expand/facts.rs (lines 605-928 of expand.rs)
```
Imports: crate::model::Fact, crate::util::*
Items:
  - fn toposort_facts()               (line 605)  -- needs pub(super), used by sql_gen
  - fn inline_facts()                 (line 684)  -- needs pub(super), used by sql_gen and inline_derived_metrics
  - fn toposort_derived()             (line 736)  -- private to this module
  - fn inline_derived_metrics()       (line 817)  -- needs pub(super), used by sql_gen
  - pub(crate) fn collect_derived_metric_source_tables() (line 868) -- pub(crate), used by ddl/show_dims_for_metric.rs
  - fn collect_derived_metric_using() (line 391)  -- needs pub(super), used by join_resolver and role_playing
```
Note: `collect_derived_metric_using()` (line 391) is logically a "facts" helper but is also used by role_playing.rs. Placing it in `facts.rs` with `pub(super)` visibility works.

#### expand/fan_trap.rs (lines 942-1157 of expand.rs)
```
Imports: crate::model::*, super::types::ExpandError, super::facts::*
Items:
  - fn check_fan_traps()              (line 942)  -- needs pub(super), used by sql_gen
  - pub(crate) fn ancestors_to_root() (line 1044) -- pub(crate), used by ddl/show_dims_for_metric.rs
  - fn path_from_ancestor_to_node()   (line 1056) -- private
  - fn check_path_up()                (line 1077) -- private
  - fn check_path_down()              (line 1124) -- private
```

#### expand/role_playing.rs (lines 303-452 of expand.rs)
```
Imports: crate::model::*, crate::util::*, super::types::ExpandError, super::facts::collect_derived_metric_using
Items:
  - fn relationships_to_table()       (line 303)  -- private
  - fn find_using_context()           (line 323)  -- needs pub(super), used by sql_gen
```

#### expand/sql_gen.rs (lines 1172-1365 of expand.rs)
```
Imports: crate::model::*, crate::util::*, crate::graph::RelationshipGraph,
         super::types::*, super::resolution::*, super::facts::*, super::fan_trap::*,
         super::role_playing::*, super::join_resolver::*
Items:
  - pub fn expand()                   (line 1172) -- pub, the main entry point
```

#### expand/mod.rs -- Re-exports
```rust
mod types;
mod resolution;
mod facts;
mod fan_trap;
mod join_resolver;
mod role_playing;
mod sql_gen;

#[cfg(test)]
mod test_helpers;

// Public API (matches prior expand.rs surface)
pub use types::{ExpandError, QueryRequest};
pub use resolution::{quote_ident, quote_table_ref};
pub use sql_gen::expand;

// Crate-internal API
pub(crate) use facts::collect_derived_metric_source_tables;
pub(crate) use fan_trap::ancestors_to_root;
```

### graph.rs Decomposition

#### graph/relationship.rs (lines 1-366 of graph.rs)
```
Imports: crate::model::SemanticViewDefinition, crate::util::suggest_closest
Items:
  - pub struct RelationshipGraph      (line 21) -- with pub fields
  - impl RelationshipGraph:
    - pub fn from_definition()        (line 39)
    - pub fn check_no_diamonds()      (line 151)
    - pub fn check_no_orphans()       (line 192)
  - fn validate_fk_references()       (line 225)  -- private
  - fn check_source_tables_reachable() (line 287) -- private
  - pub fn validate_graph()           (line 336)
```

#### graph/toposort.rs (lines 90-143 of graph.rs + lines 995-1035)
```
Imports: super::relationship::RelationshipGraph
Items:
  - impl RelationshipGraph:
    - pub fn toposort()               (line 90)
  - fn find_cycle_path()              (line 995)  -- private helper for toposort
```

#### graph/facts.rs (lines 368-586 of graph.rs)
```
Imports: crate::model::SemanticViewDefinition, crate::util::suggest_closest
Items:
  - fn is_word_boundary_byte()        (line 373)  -- private
  - pub fn find_fact_references()     (line 383)
  - pub fn validate_facts()           (line 424)
  - fn check_fact_source_tables()     (line 446)  -- private
  - type FactDag (line 471)           -- private type alias
  - fn build_fact_dag()               (line 474)  -- private
  - fn check_fact_references_exist()  (line 506)  -- private
  - fn check_fact_cycles()            (line 527)  -- private
```

#### graph/derived_metrics.rs (lines 588-931 of graph.rs)
```
Imports: crate::model::*, crate::util::suggest_closest, super::facts::find_fact_references
Items:
  - AGGREGATE_FUNCTIONS const         (line 593)  -- private
  - pub fn contains_aggregate_function() (line 641)
  - pub fn validate_derived_metrics() (line 701)
  - fn check_metric_name_uniqueness() (line 733)  -- private
  - fn check_no_aggregates_in_derived() (line 745) -- private
  - fn check_derived_metric_references() (line 759) -- private
  - fn check_derived_metric_cycles()  (line 800)  -- private
  - fn extract_identifiers()          (line 880)  -- private
  - fn is_sql_keyword_or_builtin()    (line 934)  -- private
```

#### graph/using.rs (lines 1037-1106 of graph.rs)
```
Imports: crate::model::SemanticViewDefinition
Items:
  - pub fn validate_using_relationships() (line 1049)
```

#### graph/mod.rs -- Re-exports
```rust
mod relationship;
mod toposort;
mod facts;
mod derived_metrics;
mod using;

#[cfg(test)]
mod test_helpers;

// Public API (matches prior graph.rs surface)
pub use relationship::{RelationshipGraph, validate_graph};
pub use facts::{find_fact_references, validate_facts};
pub use derived_metrics::{contains_aggregate_function, validate_derived_metrics};
pub use using::validate_using_relationships;
```

### Test Distribution

#### expand/ tests (~2,932 lines)
| Test Module | Lines | Destination Submodule |
|-------------|-------|-----------------------|
| quote_ident_tests | ~20 | resolution.rs |
| quote_table_ref_tests | ~40 | resolution.rs |
| expand_tests | ~100 | sql_gen.rs |
| phase11_1_expand_tests | ~100 | sql_gen.rs |
| phase12_cast_tests | ~100 | sql_gen.rs |
| phase26_pkfk_expand_tests | ~150 | sql_gen.rs (exercises join_resolver via expand()) |
| phase27_qualified_refs_tests | ~145 | sql_gen.rs |
| phase29_fact_inlining_tests | ~300 | sql_gen.rs (exercises fact inlining via expand()) |
| phase30_derived_metric_tests | ~475 | sql_gen.rs (exercises derived metrics via expand()) |
| phase31_fan_trap_tests | ~345 | sql_gen.rs (exercises fan trap via expand()) |
| phase32_role_playing_tests | ~485 | sql_gen.rs (exercises role-playing via expand()) |

Most expand/ tests call `expand()` end-to-end, so they belong in `sql_gen.rs` tests. The quote tests are unit tests for `resolution.rs`.

All expand/ test modules share a `make_def()` helper (lines ~1433 onward in the tests). This should live in `test_helpers.rs` and be imported by sibling test modules.

#### graph/ tests (~1,225 lines)
| Test Module | Lines | Destination Submodule |
|-------------|-------|-----------------------|
| graph validation tests (unnamed) | ~380 | relationship.rs |
| phase33_fk_reference_tests | ~215 | relationship.rs |
| validate_facts tests | ~200 | facts.rs |
| find_fact_references tests | ~60 | facts.rs |
| contains_aggregate tests | ~50 | derived_metrics.rs |
| validate_derived_metrics tests | ~150 | derived_metrics.rs |
| diamond/named relationship tests | ~70 | relationship.rs |
| validate_using tests | ~80 | using.rs |

Graph tests also share a `make_def()` helper (lines 1114-1168) and a more specialized `make_def_with_facts()`, `make_def_with_derived_metrics()`, `make_def_with_named_joins()`. These should live in `test_helpers.rs` for the graph module.

### External Consumer Impact (ZERO CHANGES REQUIRED)

All external imports resolve through `mod.rs` re-exports:

| Consumer | Import | Resolved Via |
|----------|--------|-------------|
| `query/explain.rs` | `crate::expand::{expand, QueryRequest}` | `mod.rs` re-exports |
| `query/table_function.rs` | `crate::expand::{expand, QueryRequest}` | `mod.rs` re-exports |
| `query/error.rs` | `crate::expand::ExpandError` | `mod.rs` re-exports |
| `ddl/define.rs` | `crate::expand::QueryRequest`, `crate::expand::expand` | `mod.rs` re-exports |
| `ddl/show_dims_for_metric.rs` | `crate::expand::{ancestors_to_root, collect_derived_metric_source_tables}` | `mod.rs` pub(crate) re-exports |
| `ddl/show_dims_for_metric.rs` | `crate::graph::RelationshipGraph` | `mod.rs` re-exports |
| `ddl/define.rs` | `crate::graph::{validate_graph, validate_facts, validate_derived_metrics, validate_using_relationships}` | `mod.rs` re-exports |
| `expand/` submodules | `crate::graph::RelationshipGraph` | `mod.rs` re-exports |

### Cross-Submodule Dependency Map (expand/)

Functions that cross submodule boundaries need `pub(super)` visibility:

| Function | Defined In | Used By |
|----------|-----------|---------|
| `find_dimension` | resolution | sql_gen |
| `find_metric` | resolution | sql_gen |
| `toposort_facts` | facts | sql_gen |
| `inline_facts` | facts | sql_gen (via inline_derived_metrics in facts) |
| `inline_derived_metrics` | facts | sql_gen |
| `collect_derived_metric_using` | facts | role_playing, join_resolver |
| `collect_derived_metric_source_tables` | facts | fan_trap, join_resolver, mod.rs (pub(crate)) |
| `check_fan_traps` | fan_trap | sql_gen |
| `ancestors_to_root` | fan_trap | mod.rs (pub(crate)) |
| `find_using_context` | role_playing | sql_gen |
| `synthesize_on_clause` | join_resolver | sql_gen |
| `synthesize_on_clause_scoped` | join_resolver | sql_gen |
| `resolve_joins_pkfk` | join_resolver | sql_gen |

### Cross-Submodule Dependency Map (graph/)

| Function | Defined In | Used By |
|----------|-----------|---------|
| `RelationshipGraph` | relationship | toposort, mod.rs (pub) |
| `find_fact_references` | facts | derived_metrics |
| `find_cycle_path` | toposort | (only used by toposort) |

## Code Examples

### Correct mod.rs re-export pattern

```rust
// src/expand/mod.rs
mod types;
mod resolution;
mod facts;
mod fan_trap;
mod join_resolver;
mod role_playing;
mod sql_gen;

// Public API -- matches prior expand.rs surface exactly
pub use types::{ExpandError, QueryRequest};
pub use resolution::{quote_ident, quote_table_ref};
pub use sql_gen::expand;

// Crate-internal API
pub(crate) use facts::collect_derived_metric_source_tables;
pub(crate) use fan_trap::ancestors_to_root;
```

### Correct pub(super) visibility for cross-submodule helpers

```rust
// src/expand/resolution.rs
use crate::model::{Dimension, Metric, SemanticViewDefinition};

/// Quote a SQL identifier.
#[must_use]
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Quote a table reference.
#[must_use]
pub fn quote_table_ref(table: &str) -> String {
    table.split('.').map(quote_ident).collect::<Vec<_>>().join(".")
}

/// Look up a dimension by name (case-insensitive).
/// pub(super) because sql_gen needs it.
pub(super) fn find_dimension<'a>(
    def: &'a SemanticViewDefinition,
    name: &str,
) -> Option<&'a Dimension> {
    // ... existing implementation
}
```

### Correct test helper sharing pattern

```rust
// src/expand/mod.rs (at the bottom)
#[cfg(test)]
pub(super) mod test_helpers;

// src/expand/test_helpers.rs
use crate::model::*;

/// Build a minimal SemanticViewDefinition for testing.
pub(super) fn make_def(
    tables: Vec<(&str, &str, Vec<&str>)>,
    joins: Vec<(&str, &str, Vec<&str>)>,
    dims: Vec<(&str, Option<&str>)>,
    metrics: Vec<(&str, Option<&str>)>,
) -> SemanticViewDefinition {
    // ... existing implementation from expand.rs tests
}
```

### Correct split impl block for toposort

```rust
// src/graph/toposort.rs
use std::collections::{HashMap, HashSet, VecDeque};
use super::relationship::RelationshipGraph;

impl RelationshipGraph {
    /// Topological sort via Kahn's algorithm.
    pub fn toposort(&self) -> Result<Vec<String>, String> {
        // ... existing implementation
    }
}

/// Find a cycle path among unvisited nodes.
fn find_cycle_path(
    edges: &HashMap<String, Vec<String>>,
    visited: &HashSet<&str>,
    all_nodes: &HashSet<String>,
) -> String {
    // ... existing implementation
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Flat files (expand.rs, graph.rs) | Module directories (expand/, graph/) | Phase 38 (now) | Better navigability, single-responsibility submodules |
| Circular dep: graph imports from expand | Phase 37 extracted to util.rs | Phase 37 | Enables clean split without dependency cycles |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test framework + proptest + sqllogictest |
| Config file | Cargo.toml (test config), .sqllogictest/ directory |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REF-01 | expand.rs split into expand/ module directory | compilation | `cargo check` | N/A (structural) |
| REF-02 | graph.rs split into graph/ module directory | compilation | `cargo check` | N/A (structural) |
| REF-05 | All existing tests pass with zero behavior changes | unit + integration | `just test-all` | Existing tests (390+ unit, sqllogictest, DuckLake CI) |

### Sampling Rate
- **Per task commit:** `cargo test` (fast feedback on compilation + unit tests)
- **Per wave merge:** `just test-all` (full suite including sqllogictest and DuckLake CI)
- **Phase gate:** `just test-all` green before `/gsd:verify-work`

### Wave 0 Gaps
None -- existing test infrastructure covers all phase requirements. This phase moves tests between files but does not need new test files. The 390+ existing tests serve as the regression suite.

## Open Questions

1. **Validation submodule for expand/**
   - What we know: The requirement lists "validation" as a submodule. The request validation logic (empty check, duplicate detection, resolution) is embedded in the `expand()` function itself (lines 1177-1228).
   - What's unclear: Whether to extract these ~50 lines into a separate `validation.rs` or keep them in `sql_gen.rs`.
   - Recommendation: Keep validation logic inline in `sql_gen.rs` since it's tightly coupled to the expand flow (returns early with `ExpandError`). Creating a `validation.rs` with 2-3 tiny functions adds file overhead without clarity gain. The planner should decide based on the "single responsibility" judgment call. If the requirement mandates it, extract `validate_request()` as a helper.

2. **Test helper module naming**
   - What we know: Graph tests have multiple helper variants (`make_def`, `make_def_with_facts`, `make_def_with_derived_metrics`, `make_def_with_named_joins`). Expand tests have their own `make_def` with a different signature.
   - What's unclear: Whether to consolidate into a crate-wide test helper or keep per-module.
   - Recommendation: Keep per-module test helpers (`expand/test_helpers.rs` and `graph/test_helpers.rs`) since the `make_def` signatures are different and domain-specific.

## Sources

### Primary (HIGH confidence)
- Direct source code analysis of `src/expand.rs` (4,299 lines) and `src/graph.rs` (2,333 lines)
- Rust Reference on module system: https://doc.rust-lang.org/reference/items/modules.html
- Phase 37 research and completed work (established patterns for this codebase)

### Secondary (MEDIUM confidence)
- Rust 2021 edition module resolution rules (file vs directory)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, pure code move
- Architecture: HIGH - direct source code analysis, all function signatures and dependencies mapped
- Pitfalls: HIGH - well-known Rust module system constraints, verified against compiler behavior

**Research date:** 2026-04-01
**Valid until:** 2026-05-01 (stable -- internal refactoring, no external dependency risk)
