---
phase: 16-parser-hook-registration
verified: 2026-03-07T22:40:00Z
status: passed
score: 6/6 must-haves verified
re_verification: false
---

# Phase 16: Parser Hook Registration — Verification Report

**Phase Goal:** DuckDB's parser calls the extension's parse_function for unrecognized statements, and the extension correctly detects `CREATE SEMANTIC VIEW` prefix via Rust FFI
**Verified:** 2026-03-07T22:40:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `detect_create_semantic_view()` correctly identifies CREATE SEMANTIC VIEW with case variations, leading whitespace, and trailing semicolons | VERIFIED | 7 unit tests pass: `test_basic_detection`, `test_case_insensitive` (3 variants), `test_leading_whitespace` (2 variants), `test_trailing_semicolon` (3 variants) |
| 2 | `detect_create_semantic_view()` returns PARSE_NOT_OURS for all non-matching statements including SELECT, CREATE TABLE, CREATE VIEW, empty strings | VERIFIED | `test_non_matching` covers 6 non-matching inputs; `test_too_short` covers the 19-char prefix boundary |
| 3 | `sv_parse_rust` FFI entry point wraps detection in `catch_unwind` so panics cannot cross the FFI boundary | VERIFIED | `src/parse.rs` lines 51-62: `std::panic::catch_unwind(std::panic::AssertUnwindSafe(...)).unwrap_or(PARSE_NOT_OURS)` |
| 4 | C++ `sv_parse_stub` delegates to Rust `sv_parse_rust` instead of doing its own detection | VERIFIED | `cpp/src/shim.cpp` lines 48-57: calls `sv_parse_rust(query.c_str(), query.size())`, old `StringUtil` detection fully removed |
| 5 | Normal SQL statements pass through with zero overhead (DISPLAY_ORIGINAL_ERROR returned immediately) | VERIFIED | `phase16_parser.test`: `SELECT 42` returns 42; `CREATE TABLE parser_test_16` succeeds; `DROP TABLE parser_test_16` succeeds — all via `ParserExtensionParseResult()` empty return |
| 6 | CREATE SEMANTIC VIEW triggers the existing stub plan function and returns "CREATE SEMANTIC VIEW stub fired" | VERIFIED | `phase16_parser.test`: 3 queries (uppercase, lowercase, mixed case) all return `CREATE SEMANTIC VIEW stub fired` from `sv_plan_stub` → `sv_stub_execute` |

**Score:** 6/6 truths verified

---

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/parse.rs` | Pure Rust detection function + feature-gated FFI entry point; exports `detect_create_semantic_view`, `sv_parse_rust`, `PARSE_NOT_OURS`, `PARSE_DETECTED`; min 50 lines | VERIFIED | 153 lines; constants on lines 10-12; `detect_create_semantic_view` lines 22-37; `sv_parse_rust` feature-gated on lines 48-63; 8 unit tests in `mod tests` |
| `src/lib.rs` | Module declaration `pub mod parse` (not feature-gated) | VERIFIED | Line 4: `pub mod parse;` — unconditional, placed after `pub mod model;`, before `#[cfg(feature = "extension")] pub mod query;` |
| `cpp/src/shim.cpp` | C++ trampoline calling Rust via extern C FFI; contains `sv_parse_rust` | VERIFIED | Lines 33-35: `extern "C" { uint8_t sv_parse_rust(const char *query, size_t query_len); }`; lines 48-57: call site in `sv_parse_stub` |

---

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `cpp/src/shim.cpp` | `src/parse.rs` | `extern "C"` FFI call to `sv_parse_rust` | WIRED | `sv_parse_rust` declared at shim.cpp:34 and called at shim.cpp:48-50; linker resolves via `#[no_mangle] pub extern "C" fn sv_parse_rust` in parse.rs:50 |
| `src/parse.rs` (`sv_parse_rust`) | `src/parse.rs` (`detect_create_semantic_view`) | internal function call | WIRED | parse.rs line 60: `detect_create_semantic_view(query)` called inside `sv_parse_rust` |
| `src/lib.rs` | `src/parse.rs` | `pub mod parse;` module declaration | WIRED | lib.rs line 4 |
| `cpp/src/shim.cpp` (`sv_register_parser_hooks`) | `sv_parse_stub` | `ext.parse_function = sv_parse_stub` | WIRED | shim.cpp lines 128: `ext.parse_function = sv_parse_stub;` — hook registered on `DBConfig` |
| `src/lib.rs` (extension init) | `cpp/src/shim.cpp` (`sv_register_parser_hooks`) | `extern "C"` call | WIRED | lib.rs lines 299-301 declare the extern; line 439: `sv_register_parser_hooks(db_handle)` called in `init_extension` |

---

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| PARSE-01 | 16-01-PLAN.md | `parse_function` registered as fallback hook — only fires for statements DuckDB cannot handle | SATISFIED | `sv_register_parser_hooks` sets `ext.parse_function = sv_parse_stub` and pushes to `config.parser_extensions` (shim.cpp lines 127-131); hook fires only when DuckDB's own parser fails (DuckDB parser extension contract) |
| PARSE-02 | 16-01-PLAN.md | CREATE SEMANTIC VIEW recognized case-insensitively with leading whitespace and trailing semicolons | SATISFIED | `detect_create_semantic_view` uses `eq_ignore_ascii_case` on prefix bytes, `query.trim()` strips whitespace, `trim_end_matches(';').trim()` strips semicolons; confirmed by 7 Rust unit tests and 3 sqllogictest cases |
| PARSE-03 | 16-01-PLAN.md | Parse function returns DISPLAY_ORIGINAL_ERROR for all non-semantic-view statements | SATISFIED | `sv_parse_stub` returns `ParserExtensionParseResult()` (empty = DISPLAY_ORIGINAL_ERROR) when `sv_parse_rust` returns 0; `phase16_parser.test` proves SELECT and CREATE TABLE pass through correctly |
| PARSE-04 | 16-01-PLAN.md | C++ trampoline calls `extern "C"` Rust function | SATISFIED | shim.cpp extern "C" declaration + call site confirmed; end-to-end proven by sqllogictest `CREATE SEMANTIC VIEW test_view` returning "CREATE SEMANTIC VIEW stub fired" |
| PARSE-05 | 16-01-PLAN.md | Rust parse function is panic-safe (`catch_unwind`) and thread-safe (no shared mutable state) | SATISFIED | `sv_parse_rust` wraps in `std::panic::catch_unwind(AssertUnwindSafe(...)).unwrap_or(PARSE_NOT_OURS)` (parse.rs lines 51-62); function reads only the input slice, no mutable shared state |

All 5 requirements claimed by this phase are accounted for and satisfied.

**Orphaned requirements:** None. REQUIREMENTS.md maps PARSE-01 through PARSE-05 to Phase 16 only. No additional Phase 16 requirements appear in REQUIREMENTS.md that are absent from the plan.

---

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | — | — | — | — |

No TODO, FIXME, placeholder, stub-only, or empty-implementation patterns found in any phase-16 modified file. The C++ old detection code (`StringUtil::Trim`, `StringUtil::Lower`, `StringUtil::StartsWith`) was fully removed with no remnants.

---

### Full Test Suite Results

Per CLAUDE.md, all phases must pass `just test-all` before verification is complete.

| Suite | Result | Count |
|-------|--------|-------|
| `cargo nextest run` (Rust unit + proptest + doc tests) | PASS | 137/137 |
| `just test-sql` (sqllogictest via DuckDB runner) | PASS | 4/4 tests (phase2_ddl, semantic_views, phase4_query, phase16_parser) |
| `just test-ducklake-ci` (DuckLake integration) | PASS | 6/6 |

`cargo test parse` specifically: 7/7 unit tests covering all detection behavior specifications.

---

### Commit Verification

All three commits documented in SUMMARY.md exist in the repo:

| Commit | Message | Status |
|--------|---------|--------|
| `b03fb14` | test(16-01): add failing tests for CREATE SEMANTIC VIEW detection | EXISTS |
| `4f5f1d1` | feat(16-01): implement CREATE SEMANTIC VIEW detection in Rust | EXISTS |
| `0194fb8` | feat(16-01): wire C++ trampoline to Rust FFI and add parser sqllogictest | EXISTS |

---

### Human Verification Required

One item cannot be verified programmatically:

**1. Python DuckDB client end-to-end**

**Test:** `python -c "import duckdb; c=duckdb.connect(); c.load_extension('/path/to/semantic_views.duckdb_extension'); print(c.execute('CREATE SEMANTIC VIEW test (tables := [], dimensions := [], metrics := [])').fetchall())"`

**Expected:** Returns `[('CREATE SEMANTIC VIEW stub fired',)]`

**Why human:** Requires the actual Python DuckDB binary (compiled with `-fvisibility=hidden`) to confirm the parser hook fires correctly in that specific host environment. The sqllogictest runner uses the DuckDB CLI, not Python. The VALIDATION.md explicitly lists this as a manual-only verification (PARSE-01 under Python client scenario).

---

### Gaps Summary

No gaps. All 6 observable truths are verified, all 3 artifacts pass all three levels (exists, substantive, wired), all 5 key links are wired, all 5 requirements are satisfied, and the full test suite passes.

The one human verification item (Python client test) is informational — it is not blocking because the DuckDB CLI-based sqllogictest already proves the hook chain works end-to-end, and Phase 15 already validated the `-fvisibility=hidden` compatibility concern via Python client testing.

---

_Verified: 2026-03-07T22:40:00Z_
_Verifier: Claude (gsd-verifier)_
