# Phase 37: Extract Shared Utilities - Research

**Researched:** 2026-04-01
**Domain:** Rust module refactoring -- breaking circular dependencies via leaf module extraction
**Confidence:** HIGH

## Summary

Phase 37 is a purely mechanical, behavior-preserving refactoring. Two specific circular module dependencies need to be broken by extracting shared items into new leaf modules (`util.rs` and `errors.rs`). Neither module currently exists.

The `expand.rs` <-> `graph.rs` circular dependency exists because `graph.rs` imports `expand::suggest_closest` while `expand.rs` imports `graph::RelationshipGraph`. The fix is to extract `suggest_closest`, `replace_word_boundary`, and `is_word_boundary_char` from `expand.rs` into a new `util.rs` leaf module.

The `parse.rs` <-> `body_parser.rs` circular dependency exists because `body_parser.rs` imports `parse::ParseError` while `parse.rs` imports `body_parser::parse_keyword_body`. The fix is to extract `ParseError` from `parse.rs` into a new `errors.rs` leaf module.

**Primary recommendation:** Create `src/util.rs` and `src/errors.rs` as leaf modules with no intra-crate dependencies (only `strsim` for util.rs), move the identified items, update all import paths, and verify zero test regressions.

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass (Rust unit tests + sqllogictest + DuckLake CI + vtab crash + caret position)
- **Build:** `just build` for debug extension; `cargo test` for unit tests; `just test-sql` requires `just build` first
- **Snowflake reference:** If in doubt about SQL syntax or behaviour refer to Snowflake semantic views
- **Test completeness:** A phase verification that only runs `cargo test` is incomplete -- sqllogictest covers integration paths that Rust tests do not

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| REF-03 | `suggest_closest` and `replace_word_boundary` extracted to `util.rs`, breaking expand-graph circular dependency | Dependency analysis below maps all 4 call sites for `suggest_closest` and confirms `replace_word_boundary` + `is_word_boundary_char` must also move. New import path: `crate::util::{suggest_closest, replace_word_boundary, is_word_boundary_char}` |
| REF-04 | `ParseError` extracted to shared `errors.rs`, breaking parse-body_parser circular dependency | `ParseError` is defined in `parse.rs` (line 611) and imported by `body_parser.rs` (line 7). Single-consumer extraction: move struct to `errors.rs`, re-export from `parse.rs` for backward compatibility or update 1 import site. |
</phase_requirements>

## Standard Stack

No new dependencies. This phase is purely internal refactoring within existing Rust code.

### Core (existing, unchanged)
| Library | Version | Purpose | Relevant to Phase |
|---------|---------|---------|-------------------|
| strsim | 0.11 | Levenshtein distance for fuzzy matching | Used by `suggest_closest` in util.rs |

### No New Dependencies

This is a code-move refactoring. No new crates, no version changes.

## Architecture Patterns

### Current Module Dependency Graph (problematic)

```
expand.rs  --->  graph.rs      (imports RelationshipGraph)
graph.rs   --->  expand.rs     (imports suggest_closest)  ** CIRCULAR **

parse.rs        --->  body_parser.rs  (imports parse_keyword_body)
body_parser.rs  --->  parse.rs        (imports ParseError)  ** CIRCULAR **
```

### Target Module Dependency Graph (after extraction)

```
util.rs    (LEAF -- no intra-crate imports, only strsim)
errors.rs  (LEAF -- no intra-crate imports)

expand.rs  --->  graph.rs   (imports RelationshipGraph)
expand.rs  --->  util.rs    (imports suggest_closest, replace_word_boundary, is_word_boundary_char)
graph.rs   --->  util.rs    (imports suggest_closest)

parse.rs        --->  body_parser.rs  (imports parse_keyword_body)
parse.rs        --->  errors.rs       (imports ParseError)
body_parser.rs  --->  errors.rs       (imports ParseError)
```

Note: `expand.rs` -> `graph.rs` is NOT circular because `graph.rs` no longer imports from `expand.rs`.

### Recommended Project Structure (after phase)

```
src/
  lib.rs           # Add: pub mod util; pub mod errors;
  util.rs          # NEW: suggest_closest, replace_word_boundary, is_word_boundary_char
  errors.rs        # NEW: ParseError
  expand.rs        # MODIFIED: remove 3 functions, add use crate::util::*
  graph.rs         # MODIFIED: change import from crate::expand to crate::util
  parse.rs         # MODIFIED: remove ParseError struct, add use crate::errors::ParseError
  body_parser.rs   # MODIFIED: change import from crate::parse to crate::errors
  model.rs         # UNCHANGED
  catalog.rs       # UNCHANGED
  ddl/             # MODIFIED: 1 import path change in show_dims_for_metric.rs
  query/           # MODIFIED: 2 import path changes in table_function.rs, explain.rs
```

### Pattern: Leaf Module Extraction

**What:** Move shared utility functions/types into modules with zero intra-crate dependencies (leaf nodes in the dependency graph).

**When to use:** When two modules A and B both need item X, but X currently lives in A, forcing B to import from A. If B also exports something A needs, you get a circular dependency.

**Rust-specific note:** Rust allows intra-crate circular module references (they compile fine). This refactoring is about architectural cleanliness, not fixing compilation errors. The goal is to make the dependency DAG cleaner before Phase 38 (splitting expand.rs and graph.rs into module directories).

### Pattern: Re-export for Backward Compatibility

**What:** After moving `ParseError` to `errors.rs`, optionally add `pub use crate::errors::ParseError;` in `parse.rs` so external consumers (if any) don't break.

**Decision:** NOT recommended for this codebase. This is an internal crate with no external consumers. Directly updating import paths is cleaner and avoids "where does this actually live?" confusion. All import sites are known and enumerated below.

### Anti-Patterns to Avoid
- **Extracting too many items:** Only extract the items named in REF-03 and REF-04. Do NOT extract other shared items (like `quote_ident` or model types) in this phase -- that's scope creep.
- **Changing function signatures:** The extracted functions must have identical signatures. No "while we're at it" improvements.
- **Moving tests to the new modules:** The `replace_word_boundary` unit tests (11 tests) currently live in `expand::tests::phase29_fact_inlining_tests`. They should move to `util::tests` since they test `util` functions. The existing integration/indirect tests in expand, graph, etc. stay where they are.

## Items to Extract: Complete Inventory

### util.rs (REF-03)

#### 1. `suggest_closest` (currently `expand.rs:13`, `pub fn`)

**Signature:**
```rust
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String>
```

**External dependency:** `strsim::levenshtein`

**Current import sites (4 locations that must change):**
| File | Line | Current Import | New Import |
|------|------|---------------|------------|
| `src/graph.rs` | 13 | `use crate::expand::suggest_closest;` | `use crate::util::suggest_closest;` |
| `src/query/table_function.rs` | 13 | `use crate::expand::{expand, suggest_closest, QueryRequest};` | Split: `use crate::util::suggest_closest;` + `use crate::expand::{expand, QueryRequest};` |
| `src/query/explain.rs` | 9 | `use crate::expand::{expand, suggest_closest, QueryRequest};` | Split: `use crate::util::suggest_closest;` + `use crate::expand::{expand, QueryRequest};` |
| `src/ddl/show_dims_for_metric.rs` | 10 | `use crate::expand::{ancestors_to_root, collect_derived_metric_source_tables, suggest_closest};` | Split: `use crate::util::suggest_closest;` + `use crate::expand::{ancestors_to_root, collect_derived_metric_source_tables};` |

**Internal uses in expand.rs (2 call sites, stay as-is but import from `crate::util`):**
- Line 1264: `let suggestion = suggest_closest(name, &available);`
- Line 1287: `let suggestion = suggest_closest(name, &available);`

#### 2. `replace_word_boundary` (currently `expand.rs:629`, `fn` private)

**Signature:**
```rust
pub fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String
```

**Must become `pub`** (currently private). Only used within `expand.rs` production code (6 call sites, lines 769, 772, 789, 792, 926, 1333) and 11 unit tests. After extraction, `expand.rs` imports it from `crate::util`.

#### 3. `is_word_boundary_char` (currently `expand.rs:662`, `fn` private)

**Signature:**
```rust
pub fn is_word_boundary_char(b: u8) -> bool
```

**Must become `pub`**. Used by:
- `replace_word_boundary` (moving to util.rs -- internal dependency satisfied)
- `toposort_facts` in expand.rs (lines 703, 705) -- needs `use crate::util::is_word_boundary_char;`
- `collect_derived_metric_source_tables` in expand.rs (lines 460, 462)
- `resolve_metric_expr_fully` in expand.rs (lines 836, 838)
- `resolve_joins_pkfk` scoped alias logic in expand.rs (lines 981, 983)

### errors.rs (REF-04)

#### 1. `ParseError` (currently `parse.rs:611`)

**Definition:**
```rust
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the original query string.
    pub position: Option<usize>,
}
```

**No external dependencies.** No trait implementations beyond `Debug` (derived).

**Current import sites (1 location that must change):**
| File | Line | Current Import | New Import |
|------|------|---------------|------------|
| `src/body_parser.rs` | 7 | `use crate::parse::ParseError;` | `use crate::errors::ParseError;` |

**Internal uses in parse.rs (many -- stays as-is but imports from `crate::errors`):**
- `ParseError` is constructed ~20 times in parse.rs
- `detect_near_miss` returns `Option<ParseError>`
- `validate_and_rewrite` returns `Result<..., ParseError>`
- Various helper functions return `Result<..., ParseError>`

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Fuzzy string matching | Custom edit distance | `strsim::levenshtein` | Already in use, proven correct |
| Module re-exports | Complex pub use chains | Direct import path updates | Small codebase, no external consumers |

## Common Pitfalls

### Pitfall 1: Forgetting to Update lib.rs

**What goes wrong:** New modules exist as files but aren't declared in `lib.rs`, causing "unresolved import" errors.
**Why it happens:** Creating `src/util.rs` and `src/errors.rs` files is not enough -- `pub mod util;` and `pub mod errors;` must be added to `src/lib.rs`.
**How to avoid:** Add module declarations first, before moving any code.
**Warning signs:** `cargo check` fails with "unresolved import `crate::util`".

### Pitfall 2: Visibility Changes on Extracted Functions

**What goes wrong:** `replace_word_boundary` and `is_word_boundary_char` are currently private (`fn`, not `pub fn`) in `expand.rs`. After extraction to `util.rs`, they must be `pub` for `expand.rs` to use them via `use crate::util::*`.
**Why it happens:** Private functions in a module are accessible within that module's `mod tests` block. After moving to a different module, cross-module access requires `pub`.
**How to avoid:** Explicitly mark all extracted functions as `pub fn` in util.rs.
**Warning signs:** "function is private" compiler errors.

### Pitfall 3: Test Module Paths for `replace_word_boundary` Tests

**What goes wrong:** The 11 `replace_word_boundary` tests currently live inside `expand::tests::phase29_fact_inlining_tests` and call `replace_word_boundary` directly (using `super::*` to access private functions). After extraction, these tests either (a) move to `util::tests` or (b) import from `crate::util` in expand's test module.
**Why it happens:** `super::*` in expand's tests gives access to expand's private items. After `replace_word_boundary` moves out, `super::*` won't include it.
**How to avoid:** Move the 11 `replace_word_boundary` tests to a `#[cfg(test)] mod tests {}` block in `util.rs`. The `toposort_facts` tests stay in `expand.rs` since that function stays there.
**Warning signs:** Tests fail to compile with "cannot find function `replace_word_boundary`".

### Pitfall 4: Partial Import Splitting

**What goes wrong:** Some files import `suggest_closest` alongside other `expand` items in a single `use` statement (e.g., `use crate::expand::{expand, suggest_closest, QueryRequest};`). After extraction, this must be split into two `use` statements.
**Why it happens:** Forgetting to split compound imports leaves a dangling `suggest_closest` in the `crate::expand::` import.
**How to avoid:** Search for all `suggest_closest` references and update each one. The complete list is in the "Items to Extract" section above.
**Warning signs:** "unresolved import `crate::expand::suggest_closest`".

### Pitfall 5: Forgetting `is_word_boundary_char` Direct Uses

**What goes wrong:** `is_word_boundary_char` is used directly in 4 places in expand.rs (not just via `replace_word_boundary`). Forgetting to add `use crate::util::is_word_boundary_char;` to expand.rs breaks compilation.
**Why it happens:** It's easy to assume `is_word_boundary_char` is only a helper for `replace_word_boundary`, but expand.rs uses it independently in toposort_facts, collect_derived_metric_source_tables, resolve_metric_expr_fully, and resolve_joins_pkfk.
**How to avoid:** The complete inventory in this research doc covers all usage sites.
**Warning signs:** "cannot find function `is_word_boundary_char` in this scope".

## Code Examples

### util.rs -- Complete New Module

```rust
//! Shared string utilities for fuzzy matching and word-boundary replacement.
//!
//! Extracted from `expand.rs` to break the expand <-> graph circular dependency.
//! Both `expand` and `graph` modules import from here.

/// Suggest the closest matching name from `available` using Levenshtein distance.
///
/// Returns `Some(name)` (with original casing) if the best match has an edit
/// distance of 3 or fewer characters. Returns `None` if no candidate is close
/// enough. Both the query and candidates are lowercased for comparison.
#[must_use]
pub fn suggest_closest(name: &str, available: &[String]) -> Option<String> {
    let query = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for candidate in available {
        let dist = strsim::levenshtein(&query, &candidate.to_ascii_lowercase());
        if dist <= 3 {
            if let Some((best_dist, _)) = best {
                if dist < best_dist {
                    best = Some((dist, candidate));
                }
            } else {
                best = Some((dist, candidate));
            }
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// Replace occurrences of `needle` in `haystack` with `replacement`, but only
/// at word boundaries. A word boundary is any position adjacent to a non-
/// alphanumeric or underscore. This prevents `net_price` from matching inside
/// `net_price_total` or `my_net_price`.
///
/// The matching is case-sensitive (fact names are identifiers).
pub fn replace_word_boundary(haystack: &str, needle: &str, replacement: &str) -> String {
    // ... (exact copy from expand.rs:629-658)
}

/// Check if a byte is a word-boundary character (NOT alphanumeric or underscore).
pub fn is_word_boundary_char(b: u8) -> bool {
    !b.is_ascii_alphanumeric() && b != b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    // Move 11 replace_word_boundary tests here from expand::tests::phase29_fact_inlining_tests
}
```

### errors.rs -- Complete New Module

```rust
//! Shared error types for the semantic views parser pipeline.
//!
//! Extracted from `parse.rs` to break the parse <-> body_parser circular dependency.
//! Both `parse` and `body_parser` modules import from here.

/// An error produced during DDL parsing with an optional byte-offset position in the
/// original query string (before any trimming). `DuckDB` uses this to render
/// a caret (`^`) under the error location.
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the original query string.
    pub position: Option<usize>,
}
```

### Import Updates in Existing Files

```rust
// src/lib.rs -- add two new module declarations
pub mod errors;    // NEW
pub mod util;      // NEW
pub mod body_parser;
pub mod catalog;
pub mod expand;
pub mod graph;
pub mod model;
pub mod parse;

// src/graph.rs -- line 13
// BEFORE: use crate::expand::suggest_closest;
// AFTER:
use crate::util::suggest_closest;

// src/expand.rs -- top of file
// BEFORE: (suggest_closest, replace_word_boundary, is_word_boundary_char defined locally)
// AFTER:
use crate::util::{is_word_boundary_char, replace_word_boundary, suggest_closest};

// src/body_parser.rs -- line 7
// BEFORE: use crate::parse::ParseError;
// AFTER:
use crate::errors::ParseError;

// src/parse.rs -- add import, remove struct definition
// ADD:
use crate::errors::ParseError;

// src/query/table_function.rs -- line 13
// BEFORE: use crate::expand::{expand, suggest_closest, QueryRequest};
// AFTER:
use crate::expand::{expand, QueryRequest};
use crate::util::suggest_closest;

// src/query/explain.rs -- line 9
// BEFORE: use crate::expand::{expand, suggest_closest, QueryRequest};
// AFTER:
use crate::expand::{expand, QueryRequest};
use crate::util::suggest_closest;

// src/ddl/show_dims_for_metric.rs -- line 10
// BEFORE: use crate::expand::{ancestors_to_root, collect_derived_metric_source_tables, suggest_closest};
// AFTER:
use crate::expand::{ancestors_to_root, collect_derived_metric_source_tables};
use crate::util::suggest_closest;
```

## State of the Art

Not applicable -- this is an internal Rust module refactoring, not dependent on external ecosystem changes.

## Open Questions

1. **Should `parse.rs` re-export `ParseError` for backward compatibility?**
   - What we know: Only `body_parser.rs` imports `ParseError` from `parse.rs`. No external consumers exist.
   - What's unclear: Whether future phases (38-41) will add more `ParseError` consumers.
   - Recommendation: Do NOT re-export. Update the 1 import site directly. If Phase 38+ needs `ParseError`, they import from `crate::errors`. Cleaner, no confusion about canonical location.

2. **Should `expand.rs` re-export `suggest_closest` for backward compatibility?**
   - What we know: 4 external files import `suggest_closest` from `crate::expand`. All are internal.
   - Recommendation: Do NOT re-export. Update all 4 import sites. Phase 38 will split `expand.rs` into a module directory anyway -- better to fix paths now.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + sqllogictest (Python runner) + integration scripts |
| Config file | `Cargo.toml` (Rust), `Justfile` (task runner), `Makefile` (build) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| REF-03 | `suggest_closest` importable from `crate::util` | unit (compile check) | `cargo test` | Wave 0: move 11 `replace_word_boundary` tests to `util::tests` |
| REF-03 | `replace_word_boundary` importable from `crate::util` | unit | `cargo test` | Wave 0: same as above |
| REF-03 | expand-graph circular dependency broken | structural | `cargo test` (compiles = passes) | N/A -- compiler enforces |
| REF-04 | `ParseError` importable from `crate::errors` | unit (compile check) | `cargo test` | Exists implicitly via body_parser tests |
| REF-04 | parse-body_parser circular dependency broken | structural | `cargo test` (compiles = passes) | N/A -- compiler enforces |
| REF-03+04 | All 482+ existing tests pass unchanged | regression | `just test-all` | All existing tests |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** `just test-all` green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/util.rs` -- new file with `#[cfg(test)] mod tests` containing 11 `replace_word_boundary` tests moved from `expand::tests::phase29_fact_inlining_tests`
- [ ] `src/errors.rs` -- new file (no dedicated tests needed; `ParseError` is a simple struct tested indirectly through parse and body_parser tests)

## Sources

### Primary (HIGH confidence)
- Direct source code analysis of `src/expand.rs` (4440 lines), `src/graph.rs` (2333 lines), `src/parse.rs` (2131 lines), `src/body_parser.rs` (1882 lines)
- `src/lib.rs` module declarations
- `Cargo.toml` dependency list
- grep/search of all import paths across entire `src/` tree

### Secondary (MEDIUM confidence)
- None needed -- this is an internal refactoring with no external technology research required

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, purely internal
- Architecture: HIGH - complete inventory of all items to move and all import sites
- Pitfalls: HIGH - exhaustive code analysis, all edge cases documented (visibility, test moves, compound imports)

**Research date:** 2026-04-01
**Valid until:** 2026-05-01 (stable -- internal refactoring, no external dependency risk)
