# Phase 23: Parser Proptests and Caret Integration Tests - Research

**Researched:** 2026-03-09
**Domain:** Property-based testing for DDL parser + caret position verification through extension pipeline
**Confidence:** HIGH

## Summary

Phase 23 strengthens test coverage of the parser layer (`src/parse.rs`) with two complementary techniques: (1) property-based tests using `proptest` to fuzz the 7 DDL detection/rewrite/validation functions with randomized inputs, and (2) integration tests that verify DuckDB's caret (`^`) rendering appears at the correct position when errors flow through the full extension load pipeline.

The parser module currently has 79 hand-written unit tests covering detection, rewriting, name extraction, validation, and near-miss detection. These are thorough for the specific cases they test, but they do not exercise the combinatorial space of whitespace variations, case mixing, special characters in view names, unicode, nested brackets, or edge-case lengths. The existing `expand_proptest.rs` (6 properties, simple/joined definitions) and `output_proptest.rs` (36 PBTs for typed output) provide proven patterns for proptest integration in this codebase.

For caret verification: the Phase 21 integration tests (`phase21_error_reporting.test`) verify error *messages* flow through the pipeline, but they explicitly note that sqllogictest `statement error` only matches message substrings -- it cannot assert on the caret line itself. The caret rendering was only verified by unit tests checking that `ParseError.position` is set correctly. There is no end-to-end test that proves the caret actually renders at the right character position in DuckDB output. A Python integration test using `duckdb.connect().execute()` + exception inspection can close this gap.

**Primary recommendation:** Create `tests/parse_proptest.rs` with property-based tests for all public parser functions, plus `test/integration/test_caret_position.py` for end-to-end caret position verification through the loaded extension.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| proptest | 1.9 | Property-based testing for parser functions | Already in `[dev-dependencies]`; proven patterns in expand_proptest.rs and output_proptest.rs |
| duckdb (Python) | latest | End-to-end caret position verification | Already used by test_ducklake_ci.py and test_vtab_crash.py |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| proptest::string::string_regex | (part of proptest) | Generate strings matching DDL prefix patterns | For generating near-miss DDL prefix inputs |
| proptest::sample::subsequence | (part of proptest) | Select random subsets of clause keywords | For generating CREATE body variations |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| proptest | quickcheck | proptest has explicit Strategy objects, better shrinking for complex string inputs, already in the project |
| Python caret test | Node.js/Ruby test | Python already used for integration tests (test_vtab_crash.py, test_ducklake_ci.py), has DuckDB bindings |
| sqllogictest caret assertion | Python exception inspection | sqllogictest cannot match the caret line -- only the error message substring |

## Architecture Patterns

### Recommended Test File Structure
```
tests/
  parse_proptest.rs    # NEW: proptest PBTs for parse.rs functions
  expand_proptest.rs   # Existing: expansion engine PBTs
  output_proptest.rs   # Existing: typed output PBTs
test/
  integration/
    test_caret_position.py   # NEW: Python caret position verification
    test_ducklake_ci.py      # Existing
    test_vtab_crash.py       # Existing
```

### Pattern 1: DDL Detection Properties (proptest)
**What:** Property-based tests that verify `detect_ddl_kind()` and `detect_semantic_view_ddl()` correctly classify inputs regardless of whitespace, case variation, and trailing content.
**When to use:** For all 7 DDL prefixes + non-matching inputs.
**Example:**
```rust
// Source: Follows existing expand_proptest.rs pattern
use proptest::prelude::*;
use semantic_views::parse::*;

/// Strategy: generate random case variation of a DDL prefix.
fn arb_case_variant(prefix: &'static str) -> impl Strategy<Value = String> {
    let chars: Vec<char> = prefix.chars().collect();
    let len = chars.len();
    proptest::collection::vec(proptest::bool::ANY, len).prop_map(move |bools| {
        chars.iter().zip(bools.iter()).map(|(c, &upper)| {
            if upper { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }
        }).collect::<String>()
    })
}

proptest! {
    /// Any case variation of a DDL prefix is detected correctly.
    #[test]
    fn detect_ddl_kind_case_insensitive(
        prefix in arb_case_variant("create semantic view"),
        suffix in " [a-z_]{1,20} \\(.*\\)",
    ) {
        let query = format!("{prefix}{suffix}");
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::Create));
    }
}
```

### Pattern 2: Rewrite Roundtrip Property
**What:** For any valid DDL input, `rewrite_ddl()` produces output that starts with `SELECT * FROM` and contains the correct function name.
**When to use:** For all 7 DDL forms.
**Example:**
```rust
proptest! {
    /// Rewrite of any CREATE form starts with correct function call.
    #[test]
    fn rewrite_create_starts_with_function(
        name in "[a-z_]{1,30}",
        body in "tables := \\[\\], dimensions := \\[\\]",
    ) {
        let ddl = format!("CREATE SEMANTIC VIEW {name} ({body})");
        let sql = rewrite_ddl(&ddl).unwrap();
        prop_assert!(sql.starts_with("SELECT * FROM create_semantic_view("));
        prop_assert!(sql.contains(&format!("'{name}'")));
    }
}
```

### Pattern 3: Position Invariant Property
**What:** For any DDL input with leading whitespace, `validate_and_rewrite()` error positions always point to the correct byte in the original query string.
**When to use:** For all validation error types.
**Example:**
```rust
proptest! {
    /// Position from validate_and_rewrite points at the right byte in original query.
    #[test]
    fn error_position_accounts_for_leading_whitespace(
        spaces in " {0,20}",
    ) {
        let query = format!("{spaces}CREATE SEMANTIC VIEW x (tbles := [])");
        let err = validate_and_rewrite(&query).unwrap_err();
        let pos = err.position.unwrap();
        // "tbles" must be at the pointed position
        prop_assert_eq!(&query[pos..pos+5], "tbles");
    }
}
```

### Pattern 4: Python Caret Position Verification
**What:** End-to-end test that loads the extension, runs malformed DDL, catches the exception, and verifies the caret appears at the expected character position.
**When to use:** For ERR-02 verification that the caret renders correctly through the full pipeline.
**Example:**
```python
# Source: Follows test_vtab_crash.py pattern for extension loading
import duckdb

def test_caret_points_at_missing_paren():
    """Caret should point where '(' is expected after view name."""
    conn = duckdb.connect()
    conn.execute(f"LOAD '{ext_path}'")
    try:
        conn.execute("CREATE SEMANTIC VIEW x tables := []")
        assert False, "Should have raised"
    except duckdb.ParserException as e:
        msg = str(e)
        # DuckDB renders:
        # LINE 1: CREATE SEMANTIC VIEW x tables := []
        #                               ^
        assert "Expected '('" in msg
        # Find the caret line and verify position
        lines = msg.split('\n')
        caret_line = [l for l in lines if '^' in l and 'LINE' not in l]
        if caret_line:
            caret_pos = caret_line[0].index('^')
            # Verify caret points at the right character
            ...
```

### Anti-Patterns to Avoid
- **Generating arbitrary UTF-8 strings as DDL input:** The parser operates on ASCII DDL text. Generating random UTF-8 could trigger panics in `from_utf8_unchecked` paths, but these are only called from FFI (feature-gated). Under `cargo test` the pure Rust functions handle arbitrary strings safely. Focus proptest strategies on DDL-shaped strings with controlled variation.
- **Testing caret position in sqllogictest:** sqllogictest `statement error` matches error message substrings only. It cannot match the `LINE 1:` or `^` caret lines. Use Python tests instead.
- **Over-constraining proptest strategies:** Use `prop_filter` sparingly. Prefer strategies that generate valid-by-construction inputs. If >50% of generated inputs are filtered out, the strategy is too loose.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Random case variation | Manual case permutation loops | `proptest::collection::vec(bool, len).prop_map(...)` | Proptest handles shrinking automatically |
| Random whitespace injection | Custom whitespace generator | `proptest::string::string_regex("[ \\t\\n]{0,10}")` | Regex strategy covers the space efficiently |
| DuckDB error inspection in Python | String parsing of error output | `duckdb.ParserException` + `str(e)` | Python DuckDB binds exception message as string |

**Key insight:** The parser functions in `parse.rs` are all pure functions (no FFI, no side effects, no external state). This makes them ideal proptest targets -- any input can be tested without needing a running DuckDB instance.

## Common Pitfalls

### Pitfall 1: proptest Regressions from Persistence Files
**What goes wrong:** proptest persists failing inputs to `proptest-regressions/` files. These are read on subsequent runs and can cause confusing failures if the code changes semantics.
**Why it happens:** Proptest stores minimized failing inputs to ensure reproducibility. If the function behavior changes intentionally, old regression files trigger stale failures.
**How to avoid:** Commit regression files to git (proptest recommends this). When behavior changes intentionally, delete the stale regression file.
**Warning signs:** Test fails immediately without running random cases, with a message like "Regression test failed."

### Pitfall 2: Slow Tests from Unconstrained String Generation
**What goes wrong:** Using `any::<String>()` generates very long strings, making tests slow (especially with Levenshtein distance computation in `detect_near_miss` which is O(n*m)).
**Why it happens:** Default string strategy generates strings up to ~64 bytes. Near-miss detection runs Levenshtein on the entire prefix slice.
**How to avoid:** Constrain string length: `"[a-zA-Z ]{1,100}"` or `proptest::string::string_regex("[\\x20-\\x7E]{1,100}")`. The parser functions operate on DDL text, so ASCII printable characters are the relevant domain.
**Warning signs:** `cargo test` takes >30 seconds for parse proptests (should be <5 seconds).

### Pitfall 3: Python Test Extension Path
**What goes wrong:** Python caret test fails to load extension because it uses wrong path.
**Why it happens:** Extension binary path varies between build modes (debug vs release) and platforms.
**How to avoid:** Use `SEMANTIC_VIEWS_EXTENSION_PATH` env var (same pattern as `test_vtab_crash.py` and `test_ducklake_ci.py`). Default to `build/debug/` convention.
**Warning signs:** `ModuleNotFoundError` or `LOAD` failure in Python test.

### Pitfall 4: Caret Line Parsing in DuckDB Output
**What goes wrong:** Python test incorrectly parses the caret position from DuckDB error output because the format includes ANSI codes or varies across DuckDB versions.
**Why it happens:** DuckDB error formatting includes `LINE N:` prefix before the query line, and the caret line is indented to match.
**How to avoid:** Match the `^` character position relative to the query line in the error output. The format is stable across DuckDB 1.x: `LINE 1: <query>\n         ^`. Count whitespace/indentation to determine caret offset.
**Warning signs:** Caret position assertion fails despite error message being correct.

### Pitfall 5: Prefix Overlap in Detection Properties
**What goes wrong:** proptest generates "CREATE SEMANTIC VIEW IF NOT EXISTS" and expects `DdlKind::Create` instead of `DdlKind::CreateIfNotExists`.
**Why it happens:** Strategy generates the shorter prefix "CREATE SEMANTIC VIEW" and appends "IF NOT EXISTS..." as suffix.
**How to avoid:** Be careful with suffix strategies for CREATE form tests. Either test the full prefix explicitly, or test detection for all 7 forms together and assert the correct mapping. The detection function uses longest-first ordering to handle this -- but the test strategy must understand which DdlKind to expect.
**Warning signs:** Property test fails on inputs where a longer prefix matches a different DdlKind.

## Code Examples

Verified patterns from the codebase and proptest docs.

### Existing proptest Pattern in This Codebase (expand_proptest.rs)
```rust
// Source: tests/expand_proptest.rs lines 151-166
fn arb_query_request(def: &SemanticViewDefinition) -> impl Strategy<Value = QueryRequest> {
    let dim_names: Vec<String> = def.dimensions.iter().map(|d| d.name.clone()).collect();
    let met_names: Vec<String> = def.metrics.iter().map(|m| m.name.clone()).collect();

    let dim_strategy = proptest::sample::subsequence(dim_names, 0..=def.dimensions.len());
    let met_strategy = proptest::sample::subsequence(met_names, 0..=def.metrics.len());

    (dim_strategy, met_strategy)
        .prop_filter("at least one dimension or metric", |(dims, mets)| {
            !dims.is_empty() || !mets.is_empty()
        })
        .prop_map(|(dims, mets)| QueryRequest {
            dimensions: dims,
            metrics: mets,
        })
}
```

### proptest String Regex Strategy
```rust
// Source: proptest docs (https://docs.rs/proptest/1.9/proptest/string/fn.string_regex.html)
use proptest::string::string_regex;

// Generate strings matching a regex
let strategy = string_regex("[a-zA-Z_][a-zA-Z0-9_]{0,29}").unwrap();
// This produces valid identifier-like strings for view names
```

### Python DuckDB Exception Inspection Pattern
```python
# Source: Follows test_vtab_crash.py pattern (test/integration/test_vtab_crash.py)
import duckdb, os

ext_path = os.environ.get(
    "SEMANTIC_VIEWS_EXTENSION_PATH",
    "build/debug/semantic_views.duckdb_extension"
)

def get_conn():
    conn = duckdb.connect()
    conn.execute(f"LOAD '{ext_path}'")
    return conn

def test_caret_positioned_error():
    conn = get_conn()
    try:
        conn.execute("CREATE SEMANTIC VIEW x tables := []")
        assert False, "Expected ParserException"
    except duckdb.ParserException as e:
        error_text = str(e)
        assert "Expected '('" in error_text
        # Verify caret line exists (DuckDB renders it)
        assert "^" in error_text
```

### Parser Public API Surface (Functions to Test)
```rust
// Source: src/parse.rs -- all public functions testable under cargo test
pub fn detect_ddl_kind(query: &str) -> Option<DdlKind>;          // 7 patterns + None
pub fn detect_semantic_view_ddl(query: &str) -> u8;               // PARSE_DETECTED / PARSE_NOT_OURS
pub fn rewrite_ddl(query: &str) -> Result<String, String>;        // DDL -> function call SQL
pub fn extract_ddl_name(query: &str) -> Result<Option<String>, String>; // DDL -> view name
pub fn validate_and_rewrite(query: &str) -> Result<Option<String>, ParseError>; // Full validation
pub fn validate_clauses(body: &str, body_offset: usize, query: &str) -> Result<(), ParseError>;
pub fn detect_near_miss(query: &str) -> Option<ParseError>;       // Fuzzy prefix matching
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| 79 hand-written unit tests in parse.rs | Same + proptest PBTs | Phase 23 | Covers combinatorial space (case, whitespace, special chars) |
| Caret rendering verified by unit tests only | Same + Python end-to-end verification | Phase 23 | Closes the gap between "position is set" and "caret renders correctly" |
| sqllogictest for error integration tests | Same (kept) + Python for caret | Phase 23 | sqllogictest still tests message content; Python tests caret position |

## Open Questions

1. **proptest Case Count Configuration**
   - What we know: Default is 256 cases per property. The parser functions are fast (microseconds per call).
   - What's unclear: Whether 256 is enough to discover edge cases in the near-miss detection (Levenshtein threshold boundary).
   - Recommendation: Use default (256). If specific properties need more, annotate with `#[proptest(cases = 1000)]`. The functions are pure and fast -- even 1000 cases adds <1 second.

2. **Scope of Caret Position Verification**
   - What we know: 6 error types produce positioned errors (clause typo, empty body, missing tables, missing paren, missing name, unbalanced brackets). Near-miss also produces positioned errors.
   - What's unclear: How many of these 6+ error types need Python caret verification vs. unit test coverage being sufficient.
   - Recommendation: Test 3 representative cases in Python (structural error, clause error, near-miss). If all 3 render correctly, the others follow from the same code path (all use `ParseError.position` -> `error_location` -> `ParserException::SyntaxError`).

3. **proptest Regression File Handling**
   - What we know: proptest creates `proptest-regressions/` directory for failing cases.
   - What's unclear: Whether the project .gitignore excludes these.
   - Recommendation: Add proptest-regressions files to git (proptest's recommended practice). Check/update .gitignore if needed.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | proptest 1.9 (Rust) + Python duckdb (caret tests) |
| Config file | `tests/parse_proptest.rs` (new), `test/integration/test_caret_position.py` (new) |
| Quick run command | `cargo test parse_proptest` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map

Phase 23 has no formal requirement IDs (TBD in ROADMAP.md). The phase addresses test coverage gaps identified in Phase 21:

| Gap | Behavior | Test Type | Automated Command | File Exists? |
|-----|----------|-----------|-------------------|-------------|
| GAP-01 | DDL detection is case-insensitive for all 7 forms under random case variation | proptest | `cargo test parse_proptest::detect` | Wave 0 |
| GAP-02 | DDL rewrite produces correct function call for all 7 forms with random view names | proptest | `cargo test parse_proptest::rewrite` | Wave 0 |
| GAP-03 | Validation error positions always point to correct byte in original query regardless of whitespace | proptest | `cargo test parse_proptest::position` | Wave 0 |
| GAP-04 | Near-miss detection does not false-positive on normal SQL | proptest | `cargo test parse_proptest::near_miss` | Wave 0 |
| GAP-05 | Bracket validation handles nested structures with strings correctly | proptest | `cargo test parse_proptest::brackets` | Wave 0 |
| GAP-06 | Caret renders at correct position in DuckDB error output through full extension pipeline | integration (Python) | `uv run test/integration/test_caret_position.py` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test parse_proptest`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `tests/parse_proptest.rs` -- proptest PBTs for parser functions
- [ ] `test/integration/test_caret_position.py` -- Python caret position verification
- [ ] Update `justfile` if needed to include caret test in `test-all`

*(Existing test infrastructure: 79 unit tests in parse.rs, phase21_error_reporting.test for error message integration)*

## Sources

### Primary (HIGH confidence)
- Project source code: `src/parse.rs` (1734 lines), `tests/expand_proptest.rs`, `tests/output_proptest.rs`, `test/sql/phase21_error_reporting.test`, `cpp/src/shim.cpp`, `Cargo.toml` -- direct code reading
- Phase 21 research and summaries -- documented caret rendering architecture, FFI patterns, sqllogictest limitations
- DuckDB amalgamation source (vendored at `cpp/include/duckdb.hpp`) -- `ParserExtensionParseResult.error_location` confirmed

### Secondary (MEDIUM confidence)
- [proptest docs](https://docs.rs/proptest/1.9/proptest/) -- string_regex strategy, Strategy trait, proptest! macro
- [proptest GitHub](https://github.com/proptest-rs/proptest) -- regression file handling, best practices

### Tertiary (LOW confidence)
- None. All findings verified against project source code and official docs.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - proptest 1.9 already in dev-dependencies, Python duckdb already used for integration tests
- Architecture: HIGH - all patterns follow existing codebase conventions (expand_proptest.rs, test_vtab_crash.py)
- Pitfalls: HIGH - based on direct code reading of parse.rs and Phase 21 caret rendering implementation

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable -- proptest API and parse.rs unlikely to change)
