# Phase 16: Parser Hook Registration - Research

**Researched:** 2026-03-07
**Domain:** DuckDB parser extension hooks, Rust FFI trampoline, panic safety
**Confidence:** HIGH

## Summary

Phase 16 moves the `CREATE SEMANTIC VIEW` detection logic from the existing C++ stub (`sv_parse_stub` in `shim.cpp`) to Rust via an FFI trampoline. The C++ side becomes a thin caller that invokes a Rust `extern "C"` function, which performs case-insensitive prefix detection, handles semicolons and whitespace, and returns a parse result. The C++ `sv_plan_stub` stays as-is for now (Phase 17 wires it to the catalog).

The existing code already has the full parser hook chain working end-to-end (Phase 15 verified: `sv_parse_stub` -> `sv_plan_stub` -> stub result, including under Python DuckDB). Phase 16's job is purely to replace the C++ detection logic with Rust, add `catch_unwind` panic safety at the FFI boundary, and prove the FFI trampoline works correctly.

**Primary recommendation:** Keep the C++ trampoline minimal (just marshals `const string&` to `const char*` + length, calls Rust, maps the return value). All detection logic lives in a pure Rust function that is independently testable under `cargo test` without the extension feature.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
None -- all implementation decisions were deferred to Claude's discretion.

### Claude's Discretion
- **Plan function behavior** -- Whether sv_plan_stub passes parsed statement data through to Phase 17 or stays as a dummy stub. Claude decides based on what makes Phase 17 easiest to build on.
- **Test strategy** -- What mix of sqllogictest and Rust unit tests to add in Phase 16 vs deferring to Phase 18. Claude decides based on success criteria coverage.
- **Parse result detail** -- Whether Rust parse function just detects the prefix and passes raw text, or also extracts view name/body. Claude decides based on what Phase 17 needs.

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PARSE-01 | `parse_function` registered as fallback hook -- only fires for statements DuckDB's parser cannot handle | Already done in Phase 15 (`sv_register_parser_hooks`). Verified working. No change needed. |
| PARSE-02 | `CREATE SEMANTIC VIEW name (...)` recognized (case-insensitive, leading whitespace, trailing semicolons) | Rust detection function with `trim()`, `to_ascii_lowercase()`, `starts_with()`. Must handle semicollon inconsistency (see Pitfalls). |
| PARSE-03 | Returns `DISPLAY_ORIGINAL_ERROR` for non-semantic-view statements (zero overhead for normal SQL) | C++ trampoline maps Rust return enum to `ParserExtensionParseResult()` default constructor. |
| PARSE-04 | Parse function delegates to Rust via FFI -- C++ trampoline calls `extern "C"` Rust function | C++ calls `sv_parse_rust(query.c_str(), query.size())`, Rust returns enum/struct, C++ maps to `ParserExtensionParseResult`. |
| PARSE-05 | Rust parse function is panic-safe (`catch_unwind`) and thread-safe (no shared mutable state) | `std::panic::catch_unwind(AssertUnwindSafe(|| ...))` wrapping the detection logic. No mutable global state needed. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Rust std | stable | `catch_unwind`, `AssertUnwindSafe`, string operations | Built-in, no dependencies |
| cc | 1.x | Compiles shim.cpp (already present) | Already in build.rs |
| duckdb.hpp | v1.4.4 | DuckDB amalgamation header (already vendored) | Provides `ParserExtensionParseResult`, `StringUtil` |

### Supporting
No new dependencies required. Phase 16 uses only standard library types across the FFI boundary.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Manual `const char*` + len FFI | CString/CStr marshaling | `const char*` + len avoids allocation; CString requires copying. Use pointer+length. |
| Returning C struct from Rust | Returning simple enum (u8) | Simple enum is sufficient for Phase 16 (only 3 states: success+text, display_original, display_extension_error). Avoids complex struct marshaling. |

## Architecture Patterns

### Recommended Project Structure
```
src/
  lib.rs              # existing -- no changes
  parse.rs            # NEW -- Rust parse detection logic (pure, testable)
cpp/
  src/shim.cpp        # MODIFIED -- sv_parse_stub becomes FFI trampoline to Rust
```

### Pattern 1: FFI Trampoline (C++ -> Rust)
**What:** C++ function receives DuckDB C++ types, extracts raw data, calls Rust `extern "C"`, maps Rust return to C++ type.
**When to use:** Whenever Rust logic needs to be called from C++ code that receives DuckDB internal types.
**Example:**

```c++
// cpp/src/shim.cpp -- C++ trampoline
// Rust FFI declaration
extern "C" {
    // Returns: 0 = DISPLAY_ORIGINAL_ERROR, 1 = PARSE_SUCCESSFUL
    uint8_t sv_parse_rust(const char *query, size_t query_len);
}

static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *, const string &query) {
    uint8_t result = sv_parse_rust(query.c_str(), query.size());
    if (result == 1) {
        // PARSE_SUCCESSFUL -- carry the original query text forward
        return ParserExtensionParseResult(
            make_uniq<SemanticViewParseData>(query));
    }
    // DISPLAY_ORIGINAL_ERROR -- let DuckDB show its normal error
    return ParserExtensionParseResult();
}
```

```rust
// src/parse.rs -- Rust detection logic

/// Result of parse detection.
/// 0 = not our statement (DISPLAY_ORIGINAL_ERROR)
/// 1 = detected CREATE SEMANTIC VIEW (PARSE_SUCCESSFUL)
const PARSE_NOT_OURS: u8 = 0;
const PARSE_SUCCESSFUL: u8 = 1;

/// Detect whether a query is a CREATE SEMANTIC VIEW statement.
///
/// Handles:
/// - Case-insensitive matching
/// - Leading/trailing whitespace
/// - Trailing semicolons (DuckDB inconsistently includes them)
///
/// Returns PARSE_SUCCESSFUL (1) if detected, PARSE_NOT_OURS (0) otherwise.
pub fn detect_create_semantic_view(query: &str) -> u8 {
    let trimmed = query.trim();
    // Strip trailing semicolons (DuckDB SplitQueries re-appends them
    // for middle statements but not the last one -- inconsistent)
    let trimmed = trimmed.trim_end_matches(';').trim();
    if trimmed.len() < 20 {
        return PARSE_NOT_OURS; // "create semantic view" is 20 chars
    }
    if trimmed[..20].eq_ignore_ascii_case("create semantic view") {
        PARSE_SUCCESSFUL
    } else {
        PARSE_NOT_OURS
    }
}

/// FFI entry point -- called from C++ sv_parse_stub.
///
/// # Safety
///
/// `query_ptr` must point to a valid UTF-8 string of `query_len` bytes.
/// The pointer must be valid for the duration of this call.
#[no_mangle]
pub extern "C" fn sv_parse_rust(
    query_ptr: *const u8,
    query_len: usize,
) -> u8 {
    // catch_unwind prevents panics from unwinding across FFI boundary
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if query_ptr.is_null() || query_len == 0 {
            return PARSE_NOT_OURS;
        }
        let query = unsafe {
            std::str::from_utf8_unchecked(
                std::slice::from_raw_parts(query_ptr, query_len)
            )
        };
        detect_create_semantic_view(query)
    }));
    result.unwrap_or(PARSE_NOT_OURS) // on panic, treat as not-ours
}
```

### Pattern 2: Separating Pure Logic from FFI
**What:** The detection logic (`detect_create_semantic_view`) is a pure function that takes `&str` and returns `u8`. The FFI wrapper (`sv_parse_rust`) handles pointer-to-slice conversion and `catch_unwind`. This separation enables unit testing under `cargo test` without the extension feature.
**When to use:** Always -- FFI functions cannot be tested easily; pure Rust functions can.

### Pattern 3: Query Text as Parse Data
**What:** `SemanticViewParseData` carries the raw query text from `parse_function` to `plan_function`. No parsing of the body happens in Phase 16.
**When to use:** Phase 16 (detection only). Phase 17 will extract view name and body from this text.
**Rationale:** Phase 17 needs the original query text to rewrite it as `SELECT * FROM create_semantic_view(...)`. Extracting the view name in Phase 16 would duplicate work that Phase 17 does anyway. Keep Phase 16 minimal -- detect, carry text, done.

### Anti-Patterns to Avoid
- **Parsing the DDL body in Phase 16:** Phase 16 is detection only. Don't try to extract view name, tables, dimensions, etc. That's Phase 17's job (statement rewriting). Over-engineering Phase 16 creates coupling.
- **Returning allocated strings across FFI:** Don't return `char*` from Rust to C++ unless absolutely necessary. For Phase 16, a simple `u8` return code is sufficient. The C++ side already has the query string.
- **Using `CString::new()` for the query:** DuckDB's `string::c_str()` is already null-terminated. Use `const char*` + length, convert to `&str` on the Rust side via `from_raw_parts`. No allocation needed.
- **Mutable global state in parse function:** Parse functions are called from DuckDB's parser, which may run on any thread. Keep the Rust detection function pure (no global state, no side effects).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Case-insensitive prefix match | Manual char-by-char comparison | `str::eq_ignore_ascii_case` on a fixed-length prefix slice | Correct for ASCII SQL keywords, zero-allocation |
| Panic safety at FFI boundary | Manual error code propagation | `std::panic::catch_unwind` | Standard Rust idiom for FFI safety |
| C++ string utilities | Custom trim/lower | DuckDB's `StringUtil::Trim()` / `StringUtil::Lower()` on C++ side | Already used in shim.cpp; but move detection logic to Rust where std `trim()`/`eq_ignore_ascii_case` are cleaner |

**Key insight:** The existing C++ stub already has working detection code. Phase 16 replaces it with equivalent Rust code, not a fundamentally different approach. The value is: testability (Rust unit tests), safety (`catch_unwind`), and preparing for Phase 17 (where Rust parses the body).

## Common Pitfalls

### Pitfall 1: Semicolon Inconsistency in parse_function Input
**What goes wrong:** DuckDB's `SplitQueries()` re-appends `;` to statements that precede another semicolon, but the LAST statement (or sole statement) does NOT get a semicolon appended. Result: the same query `CREATE SEMANTIC VIEW test (...)` may arrive with or without a trailing semicolon depending on whether it's the only statement or part of a multi-statement script.
**Why it happens:** Confirmed bug/inconsistency: [DuckDB issue #18485](https://github.com/duckdb/duckdb/issues/18485). The `SplitQueries` function in `src/parser/parser.cpp` (DuckDB v1.4.4) splits on `;` tokens and re-appends `;` to each segment EXCEPT the final trailing segment.
**How to avoid:** Always strip trailing semicolons before prefix matching: `query.trim().trim_end_matches(';').trim()`. This normalizes both cases.
**Warning signs:** Tests pass in CLI but fail in Python client (or vice versa).
**Confidence:** HIGH -- verified by reading DuckDB v1.4.4 source code of `SplitQueries`.

### Pitfall 2: Panic Unwinding Across FFI Boundary
**What goes wrong:** If the Rust parse function panics (e.g., due to unexpected input, assertion failure, or OOM), the panic unwinds across the `extern "C"` boundary into C++ code. This is undefined behavior and typically causes a segfault or corrupted stack.
**Why it happens:** Rust panics use a different unwinding mechanism than C++ exceptions. Crossing the boundary is UB per the Rust reference.
**How to avoid:** Wrap the entire Rust function body in `std::panic::catch_unwind()`. On panic, return `PARSE_NOT_OURS` (safe fallback -- DuckDB shows its normal error).
**Warning signs:** Crashes on malformed input that would cause Rust panics (out-of-bounds, unwrap failures).
**Confidence:** HIGH -- well-documented Rust FFI requirement.

### Pitfall 3: UTF-8 Assumption on Query Text
**What goes wrong:** `std::str::from_utf8()` panics or returns Err on non-UTF-8 input. DuckDB query strings are typically ASCII/UTF-8, but the C++ `string` type has no encoding guarantee.
**Why it happens:** DuckDB SQL is ASCII-based, but string literals can contain arbitrary bytes. The prefix `CREATE SEMANTIC VIEW` is always ASCII, so the detection only needs to inspect the first ~25 bytes.
**How to avoid:** Use `from_utf8_unchecked` (the prefix is ASCII), OR use `from_utf8` with a fallback to `PARSE_NOT_OURS` on error. Since `catch_unwind` wraps everything, even `from_utf8_unchecked` on invalid UTF-8 would be caught if it caused a panic downstream. Pragmatically, `from_utf8_unchecked` is fine because DuckDB query text is always valid UTF-8 (it rejects non-UTF-8 at the client layer).
**Warning signs:** None in practice -- SQL statements are always ASCII.
**Confidence:** HIGH.

### Pitfall 4: Feature Gate Confusion
**What goes wrong:** The `sv_parse_rust` FFI function is compiled into the cdylib when `--features extension` is used. If it's placed in a module that's also compiled under `cargo test` (default features), the linker can't find the C++ symbols it tries to call, or the symbol conflicts with something else.
**Why it happens:** The project uses `#[cfg(feature = "extension")]` to gate extension-only code. The pure Rust detection function should NOT be gated (so `cargo test` can test it), but the `#[no_mangle] extern "C"` FFI entry point MUST be gated.
**How to avoid:** Split into two parts:
  1. `pub fn detect_create_semantic_view(query: &str) -> u8` -- NOT feature-gated, testable
  2. `#[cfg(feature = "extension")] #[no_mangle] pub extern "C" fn sv_parse_rust(...)` -- feature-gated
**Warning signs:** `cargo test` fails with linker errors about missing C++ symbols.
**Confidence:** HIGH -- established pattern in this project.

### Pitfall 5: sv_plan_stub Data Flow
**What goes wrong:** If Phase 16 changes `sv_parse_stub` to not create `SemanticViewParseData` (or changes its structure), `sv_plan_stub` may crash because it receives unexpected parse data.
**Why it happens:** The parse and plan stubs are coupled: plan_function receives the `unique_ptr<ParserExtensionParseData>` returned by parse_function. Both must agree on the data type.
**How to avoid:** Keep `SemanticViewParseData` as-is. The Rust FFI trampoline returns a signal (u8), and the C++ trampoline still creates `SemanticViewParseData(query)` when the Rust side signals success. Plan stub remains unchanged.
**Warning signs:** Crash in `sv_plan_stub` when trying to cast `ParserExtensionParseData` to `SemanticViewParseData`.
**Confidence:** HIGH.

## Code Examples

### Complete Rust Parse Module (src/parse.rs)

```rust
// Source: Project-specific implementation based on DuckDB v1.4.4 parser extension API

/// Parse detection for `CREATE SEMANTIC VIEW` statements.
///
/// This module provides two layers:
/// 1. A pure detection function (`detect_create_semantic_view`) that is
///    testable under `cargo test` without the extension feature.
/// 2. An FFI entry point (`sv_parse_rust`) that wraps the detection in
///    `catch_unwind` for panic safety, feature-gated on `extension`.

/// Not our statement -- return DISPLAY_ORIGINAL_ERROR.
pub const PARSE_NOT_OURS: u8 = 0;
/// Detected CREATE SEMANTIC VIEW -- return PARSE_SUCCESSFUL.
pub const PARSE_DETECTED: u8 = 1;

/// Detect whether a query is a `CREATE SEMANTIC VIEW` statement.
///
/// Handles case variations, leading/trailing whitespace, and trailing
/// semicolons (DuckDB inconsistently includes them per issue #18485).
///
/// This function is pure and allocation-free for the common case
/// (non-matching queries). It performs no heap allocation.
pub fn detect_create_semantic_view(query: &str) -> u8 {
    let trimmed = query.trim();
    let trimmed = trimmed.trim_end_matches(';').trim();
    let prefix = "create semantic view";
    if trimmed.len() < prefix.len() {
        return PARSE_NOT_OURS;
    }
    // Compare only the prefix bytes, case-insensitively
    if trimmed.as_bytes()[..prefix.len()]
        .eq_ignore_ascii_case(prefix.as_bytes())
    {
        PARSE_DETECTED
    } else {
        PARSE_NOT_OURS
    }
}

/// FFI entry point called from C++ `sv_parse_stub`.
///
/// Wraps detection in `catch_unwind` for panic safety at the FFI boundary.
/// On any panic, returns `PARSE_NOT_OURS` (DuckDB shows its normal error).
///
/// # Safety
///
/// `query_ptr` must point to a valid byte sequence of `query_len` bytes.
/// The pointer must remain valid for the duration of this call.
#[cfg(feature = "extension")]
#[no_mangle]
pub extern "C" fn sv_parse_rust(query_ptr: *const u8, query_len: usize) -> u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if query_ptr.is_null() || query_len == 0 {
            return PARSE_NOT_OURS;
        }
        // SAFETY: DuckDB query strings are always valid UTF-8 (ASCII SQL text).
        // Even if not, we only inspect ASCII prefix bytes.
        let query = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(query_ptr, query_len))
        };
        detect_create_semantic_view(query)
    }))
    .unwrap_or(PARSE_NOT_OURS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_detection() {
        assert_eq!(detect_create_semantic_view("CREATE SEMANTIC VIEW test (...)"), PARSE_DETECTED);
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(detect_create_semantic_view("create semantic view test"), PARSE_DETECTED);
        assert_eq!(detect_create_semantic_view("Create Semantic View test"), PARSE_DETECTED);
        assert_eq!(detect_create_semantic_view("CREATE semantic VIEW test"), PARSE_DETECTED);
    }

    #[test]
    fn test_leading_whitespace() {
        assert_eq!(detect_create_semantic_view("  CREATE SEMANTIC VIEW test"), PARSE_DETECTED);
        assert_eq!(detect_create_semantic_view("\n\tCREATE SEMANTIC VIEW test"), PARSE_DETECTED);
    }

    #[test]
    fn test_trailing_semicolon() {
        assert_eq!(detect_create_semantic_view("CREATE SEMANTIC VIEW test;"), PARSE_DETECTED);
        assert_eq!(detect_create_semantic_view("CREATE SEMANTIC VIEW test ;"), PARSE_DETECTED);
        assert_eq!(detect_create_semantic_view("CREATE SEMANTIC VIEW test ;\n"), PARSE_DETECTED);
    }

    #[test]
    fn test_non_matching() {
        assert_eq!(detect_create_semantic_view("SELECT 1"), PARSE_NOT_OURS);
        assert_eq!(detect_create_semantic_view("CREATE TABLE test"), PARSE_NOT_OURS);
        assert_eq!(detect_create_semantic_view("CREATE VIEW test"), PARSE_NOT_OURS);
        assert_eq!(detect_create_semantic_view(""), PARSE_NOT_OURS);
        assert_eq!(detect_create_semantic_view(";"), PARSE_NOT_OURS);
        assert_eq!(detect_create_semantic_view("CREATE"), PARSE_NOT_OURS);
    }

    #[test]
    fn test_too_short() {
        assert_eq!(detect_create_semantic_view("create semantic vie"), PARSE_NOT_OURS);
    }
}
```

### Modified C++ Trampoline (cpp/src/shim.cpp changes)

```c++
// Source: Project-specific, based on existing shim.cpp pattern

// Rust FFI -- parse detection
extern "C" {
    uint8_t sv_parse_rust(const char *query, size_t query_len);
}

static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *, const string &query) {
    // Delegate detection to Rust
    uint8_t result = sv_parse_rust(
        reinterpret_cast<const char *>(query.c_str()),
        query.size());
    if (result == 1) {
        // Rust detected CREATE SEMANTIC VIEW -- carry query text forward
        return ParserExtensionParseResult(
            make_uniq<SemanticViewParseData>(query));
    }
    // Not our statement -- let DuckDB show its normal error
    return ParserExtensionParseResult();
}
```

### sqllogictest for Parser Hook (test/sql/phase16_parser.test)

```sql
# Phase 16 parser hook integration test.
# Exercises that the parser extension hook fires for CREATE SEMANTIC VIEW
# and the stub plan function returns a result.

require semantic_views

# Normal SQL still works (PARSE-03: zero overhead for non-extension queries)
query I
SELECT 42;
----
42

# Parser hook fires for CREATE SEMANTIC VIEW (PARSE-01, PARSE-02)
# The stub plan function returns "CREATE SEMANTIC VIEW stub fired"
query T
CREATE SEMANTIC VIEW test_view (tables := [], dimensions := [], metrics := []);
----
CREATE SEMANTIC VIEW stub fired

# Case insensitive detection (PARSE-02)
query T
create semantic view lower_test (tables := [], dimensions := [], metrics := []);
----
CREATE SEMANTIC VIEW stub fired

# Normal CREATE TABLE still works (PARSE-03)
statement ok
CREATE TABLE parser_test (id INTEGER);

statement ok
DROP TABLE parser_test;
```

## State of the Art

| Old Approach (Phase 15) | Current Approach (Phase 16) | Impact |
|--------------------------|----------------------------|--------|
| C++ detection logic in `sv_parse_stub` | Rust detection via FFI trampoline | Testable under `cargo test`, panic-safe |
| No `catch_unwind` | `catch_unwind` wrapper on FFI boundary | Prevents UB on malformed input |
| No Rust unit tests for parse | Pure function testable without extension | Catches regressions early |

**Unchanged:**
- `sv_plan_stub` -- stays as C++ dummy stub (Phase 17 changes it)
- `sv_register_parser_hooks` -- stays as-is (already working)
- `SemanticViewParseData` -- stays as-is (carries query text)

## Discretionary Decisions (Recommendations)

### 1. sv_plan_stub: Keep as Dummy Stub
**Recommendation:** Leave `sv_plan_stub` unchanged. It returns a single row `"CREATE SEMANTIC VIEW stub fired"`. Phase 17 replaces it with statement rewriting to `create_semantic_view(...)`.
**Rationale:** Changing it now adds risk with no benefit. Phase 17 needs the raw query text (already in `SemanticViewParseData.query`), not pre-parsed components.

### 2. Test Strategy: Unit Tests Now, sqllogictest Now
**Recommendation:** Add both in Phase 16:
- **Rust unit tests** for `detect_create_semantic_view()` covering all case variations, whitespace, semicolons, and non-matching queries. These run under `cargo test` (no extension feature needed).
- **One sqllogictest** (`phase16_parser.test`) proving the hook chain fires and the stub result appears. This proves the FFI trampoline works end-to-end.
**Rationale:** Success criteria 1 and 4 require end-to-end verification. Deferring ALL tests to Phase 18 would mean Phase 16 ships untested. The unit tests cost nothing (pure Rust), and one sqllogictest covers the integration path.

### 3. Parse Result Detail: Raw Text Only
**Recommendation:** The Rust parse function detects the prefix and returns a simple `u8` code. It does NOT extract the view name or parse the body. The C++ trampoline creates `SemanticViewParseData(query)` carrying the raw query text.
**Rationale:** Phase 17 needs to rewrite the query to `SELECT * FROM create_semantic_view('name', tables := [...], ...)`. Extracting the name in Phase 16 would mean doing it twice or creating a more complex FFI return type. Keep Phase 16 minimal.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust std test + DuckDB sqllogictest runner |
| Config file | Cargo.toml `[dev-dependencies]` + `test/sql/TEST_LIST` |
| Quick run command | `cargo test parse` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PARSE-01 | parse_function fires for unrecognized statements | integration (sqllogictest) | `just test-sql` | Wave 0 |
| PARSE-02 | Case-insensitive detection with whitespace/semicolons | unit (Rust) | `cargo test parse` | Wave 0 |
| PARSE-03 | DISPLAY_ORIGINAL_ERROR for normal SQL | integration (sqllogictest) | `just test-sql` | Wave 0 |
| PARSE-04 | Rust callable from C++ via FFI trampoline | integration (sqllogictest) | `just test-sql` | Wave 0 |
| PARSE-05 | Panic-safe (`catch_unwind`), thread-safe (no shared state) | unit (Rust) + code review | `cargo test parse` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test parse`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/parse.rs` -- new module with `detect_create_semantic_view()` + FFI entry point
- [ ] `test/sql/phase16_parser.test` -- sqllogictest exercising parser hook chain
- [ ] `test/sql/TEST_LIST` -- add `test/sql/phase16_parser.test` entry
- [ ] `src/lib.rs` -- add `pub mod parse;` declaration

## Open Questions

1. **Semicolon behavior may change in future DuckDB versions**
   - What we know: DuckDB v1.4.4 has inconsistent semicolon inclusion (issue #18485, open)
   - What's unclear: Whether DuckDB will fix this by always including or always stripping semicolons
   - Recommendation: Strip semicolons defensively. This handles both current and any future fix.

2. **Should sv_plan_stub carry parsed data forward for Phase 17?**
   - What we know: Phase 17 needs the raw query text, which is already in `SemanticViewParseData.query`
   - What's unclear: Whether Phase 17 will want additional pre-parsed fields
   - Recommendation: No -- raw text is sufficient. Phase 17 can parse what it needs from the text.

## Sources

### Primary (HIGH confidence)
- DuckDB v1.4.4 amalgamation header (`cpp/include/duckdb.hpp`) -- `ParserExtensionParseResult`, `ParserExtensionPlanResult`, `ParserExtension` class definitions (lines 32902-32979)
- DuckDB v1.4.4 source `src/parser/parser.cpp` -- `SplitQueries` function, extension fallback iteration, result type handling
- Existing codebase: `cpp/src/shim.cpp` -- working Phase 15 implementation
- Existing codebase: `src/lib.rs` -- FFI pattern, feature gating conventions
- Rust std docs: `std::panic::catch_unwind` -- FFI panic safety

### Secondary (MEDIUM confidence)
- [DuckDB issue #18485](https://github.com/duckdb/duckdb/issues/18485) -- semicolon inconsistency confirmed, open
- [Rust Nomicon: Unwinding](https://doc.rust-lang.org/nomicon/unwinding.html) -- FFI unwinding is UB
- `_notes/parser-extension-investigation.md` -- project investigation notes, cross-verified with source code

### Tertiary (LOW confidence)
- None -- all findings verified against source code

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, using established project patterns
- Architecture: HIGH -- FFI trampoline pattern mirrors existing `sv_register_parser_hooks` bridge
- Pitfalls: HIGH -- semicolon issue verified against DuckDB v1.4.4 source; FFI panic safety is well-documented Rust idiom
- Code examples: HIGH -- based on existing working code in shim.cpp and lib.rs

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable -- DuckDB v1.4.4 pinned, no moving parts)
