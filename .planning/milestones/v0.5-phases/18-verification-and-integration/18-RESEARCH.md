# Phase 18: Verification and Integration - Research

**Researched:** 2026-03-08
**Domain:** Branch integration, test suite verification, extension binary validation, version management
**Confidence:** HIGH

## Summary

Phase 18 is the final phase of the v0.5.0 spike milestone. It integrates two divergent branches (`feat/cpp-entry-point` with parser extension code from Phases 15-17, and `gsd/v0.1-milestone` with Phase 17.1 defensive crash fixes), runs the full test suite, adds `test_vtab_crash.py` to the permanent test harness, bumps the version to 0.5.0, evaluates ABI trade-offs, and documents v0.5.0 decisions in TECH-DEBT.md.

Research confirms the cherry-pick from `gsd/v0.1-milestone` to the parser branch will apply cleanly -- all three target files (`src/query/error.rs`, `src/query/table_function.rs`, `test/integration/test_vtab_crash.py`) are unmodified on `feat/cpp-entry-point` relative to the merge base. The ABI type (`C_STRUCT_UNSTABLE`) is correct for the current approach and matches what other Rust community extensions use.

**Primary recommendation:** Create `gsd/v0.5-milestone` from `feat/cpp-entry-point`, cherry-pick the three Phase 17.1 commits cleanly, run `just test-all` as baseline, fix any issues, add vtab crash test to Justfile, bump version, document tech debt, and verify `just test-all` passes at the end.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Create new branch `gsd/v0.5-milestone` from `feat/cpp-entry-point` (has Phase 15-17 parser code)
- Cherry-pick Phase 17.1 defensive fixes from `gsd/v0.1-milestone` (DECIMAL cast skip, runtime type guard)
- Claude assesses cherry-pick compatibility and picks what applies cleanly
- Delete both old branches (`gsd/v0.1-milestone`, `feat/cpp-entry-point`) after new branch is verified
- Do NOT merge to main -- leave that for milestone closure via `/gsd:complete-milestone`
- Existing `phase16_parser.test` satisfies VERIFY-02 (native DDL end-to-end cycle) -- no new sqllogictest tests needed
- `just test-all` is the pass/fail gate (cargo test + sqllogictest + DuckLake CI)
- Also run Python vtab crash script (`test_vtab_crash.py`) against the merged branch
- Add `test_vtab_crash.py` permanently to `just test-all` as a new `just test-vtab-crash` target
- BUILD-04 (cargo test without C++ overhead): Claude verifies structurally
- Binary checks only: verify correct ABI footer, platform symbols, no CMake dependency
- Evaluate C_STRUCT_UNSTABLE vs CPP ABI trade-off and document recommendation in TECH-DEBT.md
- Do NOT investigate community-extensions repo submission requirements or set up publish CI
- Keep current ABI unless evaluation reveals a clear reason to switch
- Bump Cargo.toml version from 0.4.0 to 0.5.0
- Document all v0.5.0 decisions in TECH-DEBT.md
- Phase 18 gets code to a passing state with version bumped and TECH-DEBT updated
- Milestone closure handled separately via `/gsd:complete-milestone`
- Run `just test-all` at START for baseline assessment, then again at END as the pass/fail gate
- Triage failures: simple regressions (< 30 min fix) handled inline; complex regressions get separate phases
- Phase 18 success requires `just test-all` green + `test_vtab_crash.py` green at the end

### Claude's Discretion
- Cherry-pick assessment (which Phase 17.1 fixes apply cleanly to the parser branch)
- Exact sequence of verification steps
- Whether BUILD-04 needs an explicit test or is self-evident from code structure
- How to verify binary format (manual inspection vs automated check)
- Content and structure of TECH-DEBT.md updates

### Deferred Ideas (OUT OF SCOPE)
- Full DDL surface (DROP, REPLACE, DESCRIBE, SHOW) -- next milestone after spike validates approach
- User-facing documentation (README, MAINTAINER.md with native DDL syntax) -- future milestone
- Community registry submission -- future milestone after spike proves the approach
- Publish CI pipeline -- future milestone alongside registry submission
- Custom SQL grammar parser -- spike uses statement rewriting; custom grammar deferred
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| VERIFY-01 | `just test-all` passes (Rust unit tests, sqllogictest, DuckLake CI) | Branch integration strategy verified -- cherry-pick applies cleanly. Test infrastructure documented. |
| VERIFY-02 | At least one sqllogictest exercises native `CREATE SEMANTIC VIEW` syntax end-to-end | `phase16_parser.test` already exists on `feat/cpp-entry-point` and covers DDL-01, DDL-02, DDL-03, PARSE-02, PARSE-03. No new test needed. |
| BUILD-04 | `cargo test` (bundled mode) passes without C++ compilation overhead | `build.rs` on feat branch exits immediately when `CARGO_FEATURE_EXTENSION` is not set. `src/parse.rs` detection logic is feature-gate-free. Only FFI entry points are `#[cfg(feature = "extension")]`. Structurally satisfied. |
| BUILD-05 | Extension binary has correct footer ABI type, platform symbols, no CMake dependency | Makefile uses `UNSTABLE_C_API_FLAG=--abi-type C_STRUCT_UNSTABLE`. Build uses `cargo` + `cc` crate (no CMake). Symbol export list restricts to `semantic_views_init_c_api`. Metadata appended by `append_extension_metadata.py`. |
</phase_requirements>

## Architecture Patterns

### Branch Integration Flow

The two branches diverged from a common ancestor (`d47acc6`):

```
d47acc6 (merge base)
├── feat/cpp-entry-point (Phases 15-17: C++ shim, parser hooks, DDL execution)
│   Modified: build.rs, Cargo.toml, Makefile, Justfile, src/lib.rs, src/parse.rs (NEW)
│   Added: cpp/src/shim.cpp, cpp/include/duckdb.{hpp,cpp}, test/sql/phase16_parser.test
│   UNCHANGED: src/query/table_function.rs, src/query/error.rs
│
└── gsd/v0.1-milestone (Phase 17.1: defensive crash fixes)
    Modified: src/query/table_function.rs, src/query/error.rs
    Added: test/integration/test_vtab_crash.py
    UNCHANGED: build.rs, src/lib.rs, src/parse.rs, cpp/*
```

**Key finding:** The branches modify completely disjoint file sets. All three Phase 17.1 commits will cherry-pick cleanly onto `feat/cpp-entry-point`.

### Cherry-Pick Commits (3 commits)

| Commit | Description | Files | Conflict Risk |
|--------|-------------|-------|---------------|
| `b0e57bd` | test(17.1-01): Python crash reproduction script | `test/integration/test_vtab_crash.py` (NEW) | NONE -- file doesn't exist on feat branch |
| `f618eac` | fix(17.1-02): runtime type validation | `src/query/error.rs`, `src/query/table_function.rs` | NONE -- both files unchanged on feat branch |
| `043024b` | fix(17.1-02): skip bare DECIMAL cast | `src/query/table_function.rs` | NONE -- file unchanged on feat branch |

Cherry-pick order: `b0e57bd`, `f618eac`, `043024b` (chronological order, preserving dependency chain).

### Justfile Modification

The Justfile differs between branches only in the `update-headers` recipe (feat branch downloads from GitHub release; v0.1 branch uses cargo build cache). The feat branch version is correct for the v0.5.0 architecture (needs amalgamation from GitHub release for C++ compilation).

Add new target and update `test-all`:

```just
# Run Python vtab crash reproduction tests
test-vtab-crash: build
    uv run test/integration/test_vtab_crash.py

# Updated test-all to include vtab crash tests
test-all: test-rust test-sql test-ducklake-ci test-vtab-crash
```

### Version Bump

Single line change in `Cargo.toml` line 4: `version = "0.4.0"` to `version = "0.5.0"`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| ABI footer verification | Custom binary parser | `append_extension_metadata.py` (already in build) + `strings` or `xxd` on the binary | The metadata script already appends the correct footer; visual inspection confirms |
| Symbol visibility check | nm parsing script | `nm -gU` (macOS) or `nm -D` (Linux) on the built binary | Standard tooling, one command |
| Branch integration | Manual merge with conflict resolution | `git cherry-pick` of individual commits | Disjoint file sets -- no conflict expected |

## Common Pitfalls

### Pitfall 1: Stale Extension Binary
**What goes wrong:** `just test-sql` and `test-ducklake-ci` use the binary at `build/debug/semantic_views.duckdb_extension`. If the binary is from a previous build, tests pass or fail against old code.
**Why it happens:** `just test-sql` depends on `build`, but `test-ducklake-ci` does NOT auto-build.
**How to avoid:** Run `just build` before `test-ducklake-ci` and `test-vtab-crash`. The `test-all` recipe has `test-sql` first (which triggers `build`), so subsequent targets use the fresh binary.
**Warning signs:** Tests pass but expected new behavior isn't observed.

### Pitfall 2: Cherry-Pick Ordering
**What goes wrong:** If `043024b` (DECIMAL fix) is applied before `f618eac` (type validation), the DECIMAL fix won't apply cleanly because it modifies code introduced by the type validation commit.
**Why it happens:** `043024b` patches code added by `f618eac`.
**How to avoid:** Cherry-pick in chronological order: `b0e57bd`, `f618eac`, `043024b`.

### Pitfall 3: Makefile Divergence
**What goes wrong:** The `gsd/v0.1-milestone` Makefile uses `USE_UNSTABLE_C_API=1` (old pattern). The `feat/cpp-entry-point` Makefile uses `UNSTABLE_C_API_FLAG=--abi-type C_STRUCT_UNSTABLE` (explicit override after include). If the old Makefile is used, the ABI footer may be wrong.
**Why it happens:** The feat branch Makefile overrides `UNSTABLE_C_API_FLAG` AFTER the `base.Makefile` include to ensure `C_STRUCT_UNSTABLE` is set. The old pattern relies on base.Makefile's `ifeq` conditional.
**How to avoid:** The new branch is created from `feat/cpp-entry-point`, which already has the correct Makefile. No action needed as long as the Makefile is not overwritten by cherry-pick (it won't be -- the cherry-pick commits don't touch Makefile).

### Pitfall 4: test_vtab_crash.py Extension Path
**What goes wrong:** `test_vtab_crash.py` looks for the extension at `build/debug/semantic_views.duckdb_extension` via `SEMANTIC_VIEWS_EXTENSION_PATH` env var or the helpers' default path.
**Why it happens:** The script uses the same helpers as `test_ducklake_ci.py` -- it calls `get_extension_path()` from `test_ducklake_helpers.py`.
**How to avoid:** Verify the script uses the correct default path. Since the Justfile `build` target produces the binary at `build/debug/semantic_views.duckdb_extension`, and the `test-vtab-crash` recipe depends on `build`, this should work automatically.

### Pitfall 5: Justfile update-headers Divergence
**What goes wrong:** The `gsd/v0.1-milestone` branch changed `update-headers` to use cargo build cache (`duckdb_capi/` directory). The `feat/cpp-entry-point` branch uses GitHub release download to `cpp/include/`. The feat branch version is correct for v0.5.0 (needs the amalgamation source files `duckdb.hpp` + `duckdb.cpp`).
**How to avoid:** Since cherry-pick commits don't touch Justfile, the feat branch version is preserved. No conflict.

## Code Examples

### Cherry-Pick Sequence
```bash
# Create new branch from feat/cpp-entry-point
git checkout feat/cpp-entry-point
git checkout -b gsd/v0.5-milestone

# Cherry-pick Phase 17.1 commits (chronological order)
git cherry-pick b0e57bd   # test_vtab_crash.py
git cherry-pick f618eac   # runtime type validation
git cherry-pick 043024b   # DECIMAL cast skip
```

### Justfile test-vtab-crash Target
```just
# Run Python vtab crash reproduction tests against the built extension.
# Exercises 5 crash vectors (13 tests) for type mismatch, connection lifetime,
# bind-time execution, chunking, and pointer stability.
test-vtab-crash: build
    uv run test/integration/test_vtab_crash.py

# Run all tests: Rust unit tests + SQL logic tests + DuckLake CI + vtab crash
test-all: test-rust test-sql test-ducklake-ci test-vtab-crash
```

### Binary ABI Verification
```bash
# Check ABI type in extension footer (last 256 bytes contain metadata)
strings build/debug/semantic_views.duckdb_extension | grep -E "C_STRUCT|CPP"
# Expected: C_STRUCT_UNSTABLE

# Check exported symbols (macOS)
nm -gU build/debug/libsemantic_views.dylib | grep -v "^$"
# Expected: only _semantic_views_init_c_api (plus standard symbols)

# Verify no CMake dependency
ls CMakeLists.txt 2>/dev/null && echo "CMake found -- unexpected" || echo "No CMake -- correct"
```

### Version Bump
```toml
# Cargo.toml line 4
version = "0.5.0"
```

## ABI Evaluation: C_STRUCT_UNSTABLE vs CPP

### Current State
The extension uses `C_STRUCT_UNSTABLE` ABI with the Rust entry point (`semantic_views_init_c_api`). The C++ shim is compiled alongside via the `cc` crate with the DuckDB amalgamation source.

### C_STRUCT_UNSTABLE (Current)
- **What it means:** Extension uses the C API function pointer struct, including unstable (unreleased) parts. Binary is pinned to an exact DuckDB version.
- **Why this project uses it:** `duckdb-rs` crate relies on unstable C API functionality (noted in Makefile comment). The extension template uses it by default.
- **Registry compatibility:** The `rusty_quack` community extension (official Rust template example) uses `build: cargo` and is published. The ABI type is appended by `append_extension_metadata.py` automatically.
- **Trade-off:** Version-pinned (must rebuild per DuckDB release). Already mitigated by `DuckDBVersionMonitor.yml` CI workflow.

### CPP ABI (Alternative)
- **What it means:** Extension is compiled against C++ API, tightly coupled to a specific DuckDB version.
- **Phase 15 finding:** CPP entry point (`DUCKDB_CPP_EXTENSION_ENTRY`) failed because `ExtensionLoader` referenced non-inlined C++ symbols not available under Python DuckDB's `-fvisibility=hidden`. The extension compiled but crashed at load time.
- **Why NOT suitable:** The Rust entry point works; switching to CPP entry would require abandoning the `duckdb-rs` C API initialization flow and would break Python compatibility.

### Recommendation
**Keep `C_STRUCT_UNSTABLE`.** The CPP ABI is not viable for this project's architecture (Rust entry point + C++ helper). `C_STRUCT_UNSTABLE` is functionally equivalent in version-pinning behavior and compatible with the community extension registry. The version pinning is already handled by the DuckDB Version Monitor CI workflow. No reason to switch.

## TECH-DEBT.md Update Structure

The following v0.5.0 decisions need to be documented:

### 1. Statement Rewrite Approach (not custom grammar)
- Native DDL is implemented via parser hook fallback + statement rewriting
- `CREATE SEMANTIC VIEW name (...)` is rewritten to `SELECT * FROM create_semantic_view('name', ...)`
- The rewritten SQL executes on a dedicated DDL connection to avoid ClientContext lock deadlock
- Custom grammar deferred to future milestone

### 2. DDL Connection Isolation Pattern
- Parser hook plan function executes rewritten DDL on a separate `duckdb_connection`
- This avoids deadlocking the main connection's ClientContext lock held during bind
- The DDL connection is created at extension init and stored as a file-scope static in shim.cpp

### 3. Amalgamation Compilation Trade-offs
- `duckdb.cpp` (23MB, ~300K lines) is compiled alongside `shim.cpp` via the `cc` crate
- First build takes ~2.5 minutes; cached on subsequent builds
- Provides ALL DuckDB C++ symbols (constructors, RTTI, vtables) eliminating manual stubs
- Symbol visibility restricts exports to entry point only (no ODR conflicts with host)
- Must be version-pinned to match `TARGET_DUCKDB_VERSION`

### 4. C_STRUCT_UNSTABLE ABI (evaluated, kept)
- Evaluated CPP ABI as alternative; rejected due to Phase 15 failure (non-inlined symbol resolution under -fvisibility=hidden)
- C_STRUCT_UNSTABLE pins binary to exact DuckDB version (same as CPP in practice)
- Compatible with community extension registry (rusty_quack uses same approach)
- DuckDB Version Monitor CI mitigates the version-pinning cost

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | `cargo test` (Rust), `sqllogictest` (Python runner), `uv` (Python scripts) |
| Config file | `test/sql/TEST_LIST` (sqllogictest file list), `Cargo.toml` (Rust tests) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| VERIFY-01 | Full test suite passes | integration | `just test-all` | Yes (Justfile) |
| VERIFY-02 | Native DDL sqllogictest | sqllogictest | `just test-sql` (includes `phase16_parser.test`) | Yes (on feat branch) |
| BUILD-04 | cargo test without C++ | unit/structural | `cargo test` (verify no C++ compilation in output) | Yes (build.rs feature gate) |
| BUILD-05 | Correct ABI footer, symbols, no CMake | manual/smoke | `strings build/debug/*.duckdb_extension \| grep C_STRUCT_UNSTABLE` | Yes (build system) |

### Sampling Rate
- **Per task commit:** `cargo test` (quick, no extension build needed)
- **Per wave merge:** `just test-all` (full suite including sqllogictest + DuckLake CI + vtab crash)
- **Phase gate:** `just test-all` green + `test_vtab_crash.py` green

### Wave 0 Gaps
- [ ] `test-vtab-crash` Justfile target -- needs to be added to `test-all`
- [ ] `test/sql/TEST_LIST` -- needs `phase16_parser.test` entry (already present on feat branch in TEST_LIST)
- [ ] No new test files needed -- existing tests cover all requirements

## Open Questions

1. **DuckLake CI test compatibility with parser branch**
   - What we know: `test_ducklake_ci.py` uses `create_semantic_view()` function-based DDL, which still works (DDL-03 confirmed by `phase16_parser.test`)
   - What's unclear: Whether the C++ amalgamation compilation introduces any subtle ABI differences that affect the DuckLake extension interaction
   - Recommendation: Running `just test-all` as baseline will immediately reveal any issues. LOW risk.

2. **cargo nextest vs cargo test**
   - What we know: Justfile uses `cargo nextest run` for `test-rust`, but `just test-all` depends on `test-rust` which uses nextest
   - What's unclear: Whether nextest is installed in the current environment
   - Recommendation: If nextest is not available, fall back to `cargo test`. The Justfile could be updated to use `cargo test` if nextest is unavailable.

## Sources

### Primary (HIGH confidence)
- Git branch analysis: `git diff`, `git log`, `git diff <merge-base>..branch` -- verified file disjointness between branches
- `feat/cpp-entry-point` branch code: `build.rs`, `Makefile`, `Justfile`, `src/lib.rs`, `src/parse.rs`, `cpp/src/shim.cpp`, `test/sql/phase16_parser.test`
- `gsd/v0.1-milestone` branch code: `src/query/table_function.rs`, `src/query/error.rs`, `test/integration/test_vtab_crash.py`
- `extension-ci-tools/makefiles/c_api_extensions/base.Makefile` -- ABI metadata construction
- `_notes/entry-point-decision.md` -- Phase 15 Option A/B evaluation results

### Secondary (MEDIUM confidence)
- [DuckDB community-extensions `rusty_quack` description.yml](https://raw.githubusercontent.com/duckdb/community-extensions/main/extensions/rusty_quack/description.yml) -- confirmed `build: cargo`, `language: Rust` for registry
- [DuckDB community extensions FAQ](https://duckdb.org/community_extensions/faq) -- extension submission overview
- [DuckDB extension-template-rs](https://github.com/duckdb/extension-template-rs) -- Rust extension template status (experimental)

### Tertiary (LOW confidence)
- [DuckDB community-extensions Issue #54](https://github.com/duckdb/community-extensions/issues/54) -- Rust extension guidance (still maturing)
- [DuckDB PR #14992](https://github.com/duckdb/duckdb/pull/14992) -- C API stability bump (context for ABI types)

## Metadata

**Confidence breakdown:**
- Branch integration: HIGH -- git diff analysis confirms zero file overlap between cherry-pick targets and feat branch modifications
- Test infrastructure: HIGH -- existing Justfile, TEST_LIST, and test scripts are well-documented
- ABI evaluation: HIGH -- Phase 15 entry-point-decision.md provides thorough evaluation; community extension registry examples confirm compatibility
- TECH-DEBT content: MEDIUM -- content is clear from project history, but exact wording is at Claude's discretion

**Research date:** 2026-03-08
**Valid until:** 2026-04-08 (stable -- no external dependency changes expected)
