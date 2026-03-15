# Deferred Items - Phase 32

## Pre-existing: Proptest identifier collision with AS keyword

**Discovered during:** 32-01 execution
**Scope:** Out of scope (pre-existing, not caused by Phase 32 changes)
**Description:** Two parse proptests (`relationship_cardinality_keyword_variants` and `relationship_no_cardinality_defaults`) fail when the generated relationship name starts with `as_`. The `as_` identifier matches the `AS` keyword via `find_keyword_ci` word-boundary detection, causing the RELATIONSHIPS parser to misinterpret the entry.
**Impact:** Low -- only affects proptest with adversarial inputs, not real user DDL
**Suggested fix:** Either exclude identifiers starting with reserved keywords from proptest strategy, or improve `find_keyword_ci` to not match identifiers where the boundary is an underscore.
