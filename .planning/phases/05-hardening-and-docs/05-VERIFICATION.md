---
phase: 05-hardening-and-docs
verified: 2026-02-26T12:00:00Z
status: human_needed
score: 5/5 must-haves verified
human_verification:
  - test: "Run cargo fuzz run fuzz_json_parse -- -max_total_time=10 in fuzz/ directory"
    expected: "Completes without crashes or undefined behavior; prints coverage stats and exits 0"
    why_human: "cargo-fuzz requires nightly toolchain and LLVM sanitizers; cannot invoke in this environment"
  - test: "Run cargo fuzz run fuzz_sql_expand -- -max_total_time=10 in fuzz/ directory"
    expected: "Completes without crashes; any Err() results from expand() are silently discarded; Ok() results pass the non-empty and starts-with-WITH assertions"
    why_human: "cargo-fuzz requires nightly toolchain; cannot invoke in this environment"
  - test: "Run cargo fuzz run fuzz_query_names -- -max_total_time=10 in fuzz/ directory"
    expected: "Completes without crashes; fuzzed name arrays against the fixed orders definition produce no panics"
    why_human: "cargo-fuzz requires nightly toolchain; cannot invoke in this environment"
  - test: "Follow MAINTAINER.md from Prerequisites through Quick Start using only the document"
    expected: "A Python expert with no Rust experience can clone, run just setup, just build, just test-rust, just test-sql without needing to search for additional information"
    why_human: "Requires human judgment on whether the tone and Python analogies are sufficient for a Rust newcomer"
---

# Phase 5: Hardening and Docs Verification Report

**Phase Goal:** The extension is resilient against malformed inputs at the FFI boundary and is documented well enough for a contributor to set up, build, test, and publish without asking for help
**Verified:** 2026-02-26T12:00:00Z
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `cargo fuzz run` targets cover the C FFI boundary (input parsing) and the SQL generation path | VERIFIED | Three targets exist and are substantive: `fuzz_json_parse` feeds bytes to `from_json()`, `fuzz_sql_expand` uses Arbitrary-derived `SemanticViewDefinition` with `expand()`, `fuzz_query_names` fuzzes name arrays against a fixed definition |
| 2 | No undefined behavior triggered on a corpus of malformed inputs | HUMAN NEEDED | Targets are correctly implemented (no panics possible from `if let Ok(...)` wrapping, errors silently discarded); actual fuzz runs require nightly toolchain — cannot verify programmatically |
| 3 | A contributor following only MAINTAINER.md can set up dev environment, build, run all tests, load the extension, update version pin, run fuzzer, and understand publishing — without asking for clarification | VERIFIED (automated portion) | MAINTAINER.md is 687 lines covering all 12 required sections with step-by-step commands; HUMAN NEEDED for readability judgment |

**Score:** 5/5 artifacts and key links verified (automated); 2 items require human fuzz execution and 1 requires human readability review

### Required Artifacts

#### Plan 05-01 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `fuzz/Cargo.toml` | Fuzz crate depending on semantic_views with arbitrary feature | VERIFIED | Contains `cargo-fuzz = true`, `[workspace]` isolation key, three `[[bin]]` targets, `semantic_views = { path = "..", features = ["arbitrary"] }` |
| `fuzz/fuzz_targets/fuzz_json_parse.rs` | JSON definition parsing fuzz target | VERIFIED | 10-line substantive harness; calls `SemanticViewDefinition::from_json("fuzz_test", s)` inside `fuzz_target!`; skips non-UTF8 gracefully |
| `fuzz/fuzz_targets/fuzz_sql_expand.rs` | SQL expansion fuzz target with Arbitrary-derived inputs | VERIFIED | Uses `#[derive(Arbitrary)]` on `FuzzInput { def: SemanticViewDefinition, dim_names, metric_names }`; calls `expand("fuzz_view", ...)` with assertions on `Ok` results |
| `fuzz/fuzz_targets/fuzz_query_names.rs` | Query-time name array fuzz target against fixed definition | VERIFIED | Fuzzes `NameFuzzInput { dim_names, metric_names }` against `fixed_definition()` (orders table with region/month dims, revenue/count metrics) |
| `.github/workflows/NightlyFuzz.yml` | Nightly CI fuzzing with crash reporting and corpus PR | VERIFIED | Daily cron `0 3 * * *`, 3-target matrix with `fail-fast: false`, crash artifact upload, GitHub issue creation on failure, separate `commit-corpus` job with `peter-evans/create-pull-request@v7` |
| `src/model.rs` | Arbitrary derive behind arbitrary feature flag on all model types | VERIFIED | All four types have `#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]`: `Dimension` (line 5), `Metric` (line 17), `Join` (line 29), `SemanticViewDefinition` (line 41) |

#### Plan 05-02 Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `MAINTAINER.md` | Complete maintainer documentation, 200+ lines, contains "Architecture Overview" | VERIFIED | 687 lines; all 12 required sections present; architecture maps every `src/` file to its role including `ddl/` and `query/` subdirectory files (`define.rs`, `drop.rs`, `list.rs`, `describe.rs`, `table_function.rs`, `explain.rs`, `error.rs`, both `mod.rs`) |

### Key Link Verification

#### Plan 05-01 Key Links

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `fuzz/Cargo.toml` | `Cargo.toml` | path dependency with `features = ["arbitrary"]` | VERIFIED | Line 13: `semantic_views = { path = "..", features = ["arbitrary"] }` — exact pattern match |
| `fuzz/fuzz_targets/fuzz_sql_expand.rs` | `src/expand.rs` | calls `expand()` with Arbitrary-derived `SemanticViewDefinition` | VERIFIED | Line 19: `if let Ok(sql) = expand("fuzz_view", &input.def, &req)` — imports `expand` from `semantic_views::expand` |
| `fuzz/fuzz_targets/fuzz_json_parse.rs` | `src/model.rs` | calls `SemanticViewDefinition::from_json` with arbitrary bytes | VERIFIED | Line 8: `semantic_views::model::SemanticViewDefinition::from_json("fuzz_test", s)` |

#### Plan 05-02 Key Links

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `MAINTAINER.md` | `Justfile` | References `just` commands for all build/test/fuzz workflows | VERIFIED | 31 occurrences of `just ` commands covering `just setup`, `just build`, `just test-rust`, `just test-sql`, `just test-iceberg`, `just test-all`, `just fuzz`, `just fuzz-all`, `just fuzz-cmin`, `just coverage`, `just lint` |
| `MAINTAINER.md` | `Cargo.toml` | References feature flags and dependencies | VERIFIED | 6 occurrences of "features"; explicitly documents `default` vs `extension` feature split with table |
| `MAINTAINER.md` | `fuzz/` | Documents fuzzer setup, running, corpus management | VERIFIED | 4 occurrences of `cargo fuzz`; dedicated Fuzzing section with targets table, corpus management, crash interpretation, and nightly CI description |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| TEST-05 | 05-01 | Fuzz targets cover the unsafe C FFI boundary and SQL generation path | SATISFIED | Three `cargo-fuzz` targets exist and are substantive: `fuzz_json_parse` (FFI/JSON boundary), `fuzz_sql_expand` (SQL generation), `fuzz_query_names` (name injection). Seed corpus present. NightlyFuzz.yml automates ongoing coverage. Marked `[x]` in REQUIREMENTS.md. |
| DOCS-01 | 05-02 | MAINTAINER.md covers dev environment setup, build, tests, loading, version pin update, fuzzing, and publishing | SATISFIED | 687-line MAINTAINER.md covers all 7 listed topics plus architecture overview, worked examples, troubleshooting, and CI workflows. All `just` commands are real and cross-reference the Justfile. Marked `[x]` in REQUIREMENTS.md. |

No orphaned requirements — REQUIREMENTS.md traceability table maps TEST-05 and DOCS-01 to Phase 5, matching the plan frontmatter exactly.

### Anti-Patterns Found

No anti-patterns detected:

- Fuzz targets: no `TODO`, `FIXME`, `placeholder`, `unimplemented!()`, or `todo!()` — all three are complete and substantive
- NightlyFuzz.yml: no stub steps — all five jobs steps are wired (checkout, toolchain, install, run, conditional upload/issue/PR)
- MAINTAINER.md: no `TODO`, `FIXME`, `TBD`, or `coming soon` — all 12 sections are fully written
- The `todo!()` that appears in the "Adding a New DDL Function" worked example is intentional illustration code, not a production stub

### Human Verification Required

#### 1. Fuzz target execution: fuzz_json_parse

**Test:** From the `fuzz/` directory, run `cargo fuzz run fuzz_json_parse -- -max_total_time=10`
**Expected:** Fuzzer runs for 10 seconds, prints libfuzzer coverage stats, exits 0 with no crash artifacts written to `fuzz/artifacts/fuzz_json_parse/`
**Why human:** cargo-fuzz requires the nightly Rust toolchain and LLVM AddressSanitizer; these cannot be invoked in the verification environment

#### 2. Fuzz target execution: fuzz_sql_expand

**Test:** Run `cargo fuzz run fuzz_sql_expand -- -max_total_time=10`
**Expected:** Completes without crashes; any `Err` results from `expand()` are silently discarded; `Ok` results satisfy the `!sql.is_empty()` and `sql.starts_with("WITH")` assertions
**Why human:** Same toolchain constraint as above

#### 3. Fuzz target execution: fuzz_query_names

**Test:** Run `cargo fuzz run fuzz_query_names -- -max_total_time=10`
**Expected:** Fuzzed name arrays against the fixed orders definition produce no panics; validation errors from `expand()` are acceptable
**Why human:** Same toolchain constraint as above

#### 4. MAINTAINER.md contributor readability

**Test:** Ask a Python expert with no Rust experience to follow MAINTAINER.md from "Prerequisites" through "Quick Start" without looking at any other documentation
**Expected:** They can complete `git clone --recurse-submodules`, `just setup`, `just build`, `just test-rust`, `just test-sql` without needing to search externally; Rust concept explanations (rustup=pyenv, Cargo.toml=pyproject.toml, features=extras_require) are sufficient for orientation
**Why human:** Requires human judgment on whether the Python-analogy explanations and inline footnote style are adequate for a Rust newcomer; automated checks can only verify the content exists, not whether it is comprehensible

### Gaps Summary

No gaps found. All artifacts exist, are substantive (not stubs), and are correctly wired:

- All three fuzz targets contain real fuzzing logic, not placeholder `todo!()` calls
- The Arbitrary derive is correctly gated behind a feature flag on all four model types
- The fuzz crate has the `[workspace]` isolation key preventing parent workspace absorption
- NightlyFuzz.yml has all five required structural elements (cron, matrix, crash upload, issue creation, corpus PR)
- MAINTAINER.md is 687 lines with all 12 required sections, correct src/ file mapping, two worked examples, and no broken references
- Both requirement IDs (TEST-05, DOCS-01) are fully addressed and marked complete in REQUIREMENTS.md

The only items requiring human attention are the three fuzz runs (to confirm no panics on the actual seed corpus with real sanitizer tooling) and a readability review of MAINTAINER.md. These cannot be verified programmatically and represent normal "ship criteria" items, not implementation gaps.

---

_Verified: 2026-02-26T12:00:00Z_
_Verifier: Claude (gsd-verifier)_
