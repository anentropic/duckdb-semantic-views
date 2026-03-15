# Phase 34: DuckDB 1.5 Upgrade & LTS Branch - Research

**Researched:** 2026-03-15
**Domain:** DuckDB version upgrade, multi-version branching, CI infrastructure
**Confidence:** MEDIUM-HIGH

## Summary

This phase upgrades the extension from DuckDB 1.4.4 to DuckDB 1.5.0 ("Variegata") on main, while creating a `duckdb/1.4.x` branch that maintains LTS compatibility. The upgrade is non-trivial because the extension uses C_STRUCT_UNSTABLE ABI with a C++ amalgamation shim -- both the Rust dependencies AND the C++ compilation must be updated in lockstep.

DuckDB 1.5.0 was released on 2026-03-09. The duckdb-rs crate adopted a new versioning scheme: `1.10500.0` (encoding the DuckDB version as `1.MAJOR_MINOR_PATCH.x`). This means Cargo.toml pins change from `=1.4.4` to `=1.10500.0` for both `duckdb` and `libduckdb-sys`. The extension-ci-tools repo uses branch `v1.5.0` (not a tag). DuckDB 1.5 introduces a PEG parser (opt-in), but existing Bison-based parser hooks should continue to work since PEG is disabled by default. The C API added several new features but the core extension patterns (VTab, TableFunction, ParserExtension) are expected to be source-compatible, with potential minor API surface changes.

**Primary recommendation:** Upgrade main to DuckDB 1.5.0 first (get all tests green), then create the `duckdb/1.4.x` branch from the pre-upgrade commit, then update CI for dual-branch support, and finally update the Version Monitor for dual-track checking.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Main-first development: all new features land on `main` (targeting DuckDB 1.5.x)
- LTS branch named `duckdb/1.4.x` (not `andium`) -- enables `duckdb/*` CI pattern matching
- Best-effort cherry-pick of features from main to `duckdb/1.4.x` -- don't block main if cherry-picks are messy
- `duckdb/1.4.x` gets a suffixed Cargo.toml version (e.g., `0.5.4+duckdb1.4`) to distinguish from main
- Push through breaking changes -- fix whatever DuckDB 1.5 breaks in the C++ shim, build.rs, duckdb-rs API, or amalgamation
- Update Windows patches in build.rs for DuckDB 1.5 amalgamation layout -- fix, don't skip
- Update extension-ci-tools submodule to v1.5.x tag on main; `duckdb/1.4.x` keeps v1.4.4 pin
- Ignore PEG parser as default behavior (Bison hooks should still work), but add a test that loads extension with PEG enabled to document compatibility status
- Branch-based CI: Build.yml on main builds against 1.5.x, Build.yml on `duckdb/1.4.x` builds against 1.4.x
- Use `duckdb/*` branch pattern in workflow triggers where appropriate
- Full `just test-all` quality gate on both branches
- No matrix -- the branch IS the version selector
- Single DuckDBVersionMonitor.yml with two jobs: `check-latest` (bumps main) and `check-lts` (bumps `duckdb/1.4.x`)
- Dual tags: `v0.5.4` on main (DuckDB 1.5.x), `v0.5.4-duckdb1.4` on `duckdb/1.4.x`
- CE `description.yml` uses commit hashes: `ref` from main, `andium` from `duckdb/1.4.x` branch

### Claude's Discretion
- Exact duckdb-rs version for 1.5.x (likely `=1.5.0` or `=1.10500.0` -- determine from crates.io)
- Whether build.rs Windows patches need updating or can be removed for 1.5
- Specific shim.cpp changes needed for DuckDB 1.5 API
- How to structure the PEG compatibility test
- Order of operations for the upgrade (Cargo.toml first vs amalgamation first vs CI first)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DKDB-01 | Extension builds and all tests pass against DuckDB 1.5.x (latest) | duckdb-rs v1.10500.0 pins, amalgamation download from v1.5.0 release, extension-ci-tools v1.5.0 branch, Windows patch verification, shim.cpp C++ API compat check |
| DKDB-02 | Extension builds and all tests pass against DuckDB 1.4.x (Andium LTS) | No code changes needed -- current code already passes on 1.4.4; `duckdb/1.4.x` branch preserves this state |
| DKDB-03 | `duckdb/1.4.x` branch maintained for 1.4.x LTS compatibility | Branch creation from pre-upgrade commit, Cargo.toml version suffix `0.5.4+duckdb1.4`, CI triggers for `duckdb/*` pattern |
| DKDB-04 | Build.yml runs CI for both DuckDB versions | Branch-based CI strategy: each branch has its own Build.yml with correct version pins; `duckdb/*` branch pattern in workflow triggers |
| DKDB-05 | `.duckdb-version` on main tracks latest; `.duckdb-version` on `duckdb/1.4.x` tracks LTS | `.duckdb-version` updated to `v1.5.0` on main; stays `v1.4.4` on LTS branch |
| DKDB-06 | DuckDB Version Monitor updated to check both latest and LTS releases | Dual-job workflow: `check-latest` targets main, `check-lts` targets `duckdb/1.4.x` |
</phase_requirements>

## Standard Stack

### Core Version Pins (main branch, DuckDB 1.5.x)

| Dependency | Current | Target | Purpose |
|------------|---------|--------|---------|
| `duckdb` (crate) | `=1.4.4` | `=1.10500.0` | Rust bindings for DuckDB |
| `libduckdb-sys` (crate) | `=1.4.4` | `=1.10500.0` | Raw FFI bindings |
| `.duckdb-version` | `v1.4.4` | `v1.5.0` | Amalgamation download version |
| extension-ci-tools | branch `v1.4.4` | branch `v1.5.0` | CI build infrastructure |
| Python `duckdb` | `1.4.4` | `1.5.0` | Integration test runtime |

**Confidence: HIGH** -- duckdb-rs v1.10500.0 confirmed on GitHub releases page (released 2026-03-11, bundles DuckDB v1.5.0). The new versioning scheme encodes DuckDB version in the second semver component: `1.MAJOR_MINOR_PATCH.x` where DuckDB 1.5.0 = crate 1.10500.0.

### LTS Branch Pins (duckdb/1.4.x branch)

| Dependency | Value | Purpose |
|------------|-------|---------|
| `duckdb` (crate) | `=1.4.4` | Unchanged |
| `libduckdb-sys` (crate) | `=1.4.4` | Unchanged |
| `.duckdb-version` | `v1.4.4` | Unchanged |
| extension-ci-tools | branch `v1.4.4` | Unchanged |
| Python `duckdb` | `1.4.4` | Unchanged |
| Cargo.toml `version` | `0.5.4+duckdb1.4` | Suffixed to distinguish from main |

### Sources
- [duckdb-rs releases](https://github.com/duckdb/duckdb-rs/releases) -- v1.10500.0 confirmed (HIGH confidence)
- [duckdb-rs Cargo.toml](https://raw.githubusercontent.com/duckdb/duckdb-rs/main/Cargo.toml) -- workspace version `1.10500.0`, both duckdb and libduckdb-sys (HIGH confidence)
- extension-ci-tools remote branches: `origin/v1.5.0` and `origin/v1.5-variegata` confirmed via `git branch -r` (HIGH confidence)

## Architecture Patterns

### Recommended Order of Operations

The upgrade should proceed in this sequence to minimize debugging complexity:

```
1. Update Cargo.toml pins (duckdb, libduckdb-sys) to =1.10500.0
2. Update .duckdb-version to v1.5.0
3. Download new amalgamation (just update-headers)
4. Run cargo test (bundled feature -- catches Rust API breaks)
5. Run just build (extension feature -- catches C++ compilation breaks)
6. Fix any shim.cpp / build.rs issues
7. Run just test-all (full quality gate)
8. Fix any integration test issues (Python duckdb pin, test expectations)
9. Create duckdb/1.4.x branch from pre-upgrade commit
10. Update CI workflows on main (extension-ci-tools, workflow triggers)
11. Update CI workflows on duckdb/1.4.x (add duckdb/* trigger pattern)
12. Update DuckDB Version Monitor for dual-track
13. Add PEG compatibility test
```

**Rationale:** Cargo.toml first because `cargo test` uses the bundled feature (compiles DuckDB from source in duckdb-rs). This isolates Rust API changes from C++ amalgamation issues. If `cargo test` passes but `just build` fails, the problem is in the amalgamation/shim, not the Rust code.

### Files That Need Updating (main branch)

```
Version pins:
├── .duckdb-version                          # v1.4.4 -> v1.5.0
├── Cargo.toml                               # =1.4.4 -> =1.10500.0 (both deps)
├── .github/workflows/Build.yml              # @v1.4.4 -> @v1.5.0, duckdb_version/ci_tools_version
├── .github/workflows/PullRequestCI.yml      # @v1.4.4 -> @v1.5.0, duckdb_version/ci_tools_version
├── .github/workflows/DuckDBVersionMonitor.yml  # Rewrite for dual-track

Python PEP 723 headers (duckdb==X.Y.Z):
├── examples/basic_ddl_and_query.py          # duckdb==1.4.4 -> duckdb==1.5.0
├── examples/advanced_features.py            # duckdb==1.4.4 -> duckdb==1.5.0
├── test/integration/test_caret_position.py  # duckdb==1.4.4 -> duckdb==1.5.0
├── test/integration/test_ducklake_ci.py     # duckdb==1.4.4 -> duckdb==1.5.0
├── test/integration/test_vtab_crash.py      # duckdb==1.4.4 -> duckdb==1.5.0
├── test/integration/test_ducklake.py        # duckdb==1.4.4 -> duckdb==1.5.0
├── configure/setup_ducklake.py              # duckdb==1.4.4 -> duckdb==1.5.0

Build infrastructure:
├── extension-ci-tools (submodule)           # Update to v1.5.0 branch
├── build.rs                                 # Windows patches -- verify markers exist in new amalgamation
├── cpp/src/shim.cpp                         # Verify C++ API compat (compile test)

CI workflows:
├── .github/workflows/Build.yml              # Add duckdb/* to branch triggers
├── .github/workflows/PullRequestCI.yml      # Add duckdb/* to PR base branch triggers
├── .github/workflows/CodeQuality.yml        # Add duckdb/* to branch triggers
├── .github/workflows/Fuzz.yml               # Consider: add duckdb/* trigger
├── .github/workflows/DuckDBVersionMonitor.yml  # Dual-job rewrite
```

### Branch Strategy Pattern

```
main (DuckDB 1.5.x)
├── .duckdb-version = v1.5.0
├── Cargo.toml: version = "0.5.4", duckdb = "=1.10500.0"
├── Build.yml: @v1.5.0, duckdb_version: v1.5.0
└── Tags: v0.5.4

duckdb/1.4.x (DuckDB 1.4.x LTS)
├── .duckdb-version = v1.4.4
├── Cargo.toml: version = "0.5.4+duckdb1.4", duckdb = "=1.4.4"
├── Build.yml: @v1.4.4, duckdb_version: v1.4.4
└── Tags: v0.5.4-duckdb1.4
```

### Anti-Patterns to Avoid
- **Matrix-based CI for versions:** Don't use a build matrix with DuckDB version as a variable. The branch IS the version selector. This keeps CI simple and avoids the complexity of parameterizing Cargo.toml pins at CI time.
- **Shared branch with conditional compilation:** Don't try to support both versions on a single branch with `#[cfg]` flags. The amalgamation, Cargo.toml pins, and CI tools version are all hard-pinned.
- **Rebasing LTS onto main:** Don't rebase `duckdb/1.4.x` onto main. Cherry-pick individual commits instead. The branches diverge at the version pin level and should stay diverged.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DuckDB version detection | Custom version parsing | `.duckdb-version` file + `$(shell cat ...)` | Established pattern, consumed by Makefile, justfile, CI |
| Multi-version CI | Custom matrix logic | Branch-based CI + `duckdb/*` trigger pattern | GitHub Actions naturally runs the workflow from the branch being pushed, inheriting that branch's version pins |
| Version monitor dual-track | Two separate workflows | Single workflow with two jobs | Avoids duplication, easier to maintain |
| Amalgamation download | Custom download script | `just update-headers` recipe | Already reads `.duckdb-version` and downloads correct release |

## Common Pitfalls

### Pitfall 1: duckdb-rs Version Scheme Change
**What goes wrong:** Setting Cargo.toml to `duckdb = "=1.5.0"` instead of `"=1.10500.0"` -- the crate version changed to encode the DuckDB version differently.
**Why it happens:** Previous versions used simple version mirroring (1.4.4 = 1.4.4). The new scheme uses `1.MAJOR_MINOR_PATCH.x`.
**How to avoid:** Use `duckdb = "=1.10500.0"` and `libduckdb-sys = "=1.10500.0"` in Cargo.toml.
**Warning signs:** `cargo update` or `cargo build` fails with "no matching package" errors.

### Pitfall 2: Windows Patch Markers in build.rs
**What goes wrong:** The build.rs Windows patches use string markers to locate insertion points in duckdb.cpp. DuckDB 1.5.0 may have moved or changed these markers, causing patches to silently skip.
**Why it happens:** The patch function `patch_duckdb_cpp_for_windows()` uses exact string matching against specific code patterns in duckdb.cpp (e.g., `#endif // defined(_WIN32)\n\n// Platform-specific helpers`). The amalgamation layout changes between versions.
**How to avoid:** After downloading the 1.5.0 amalgamation, check if the patch markers still exist. If not, find the new marker locations and update build.rs.
**Warning signs:** Build succeeds but `cargo:warning=duckdb.cpp Win32 patch N skipped` appears in build output. Windows CI builds fail with `GetObject` or `interface` macro conflicts.

### Pitfall 3: extension-ci-tools Uses Branches, Not Tags
**What goes wrong:** Using `@v1.5.0` as a tag reference in `uses:` when it's actually a branch.
**Why it happens:** Confusing GitHub Actions tag refs with branch refs. In extension-ci-tools, `v1.5.0` is a branch name.
**How to avoid:** GitHub Actions `uses:` syntax works with both branch names and tags -- `@v1.5.0` will resolve the branch. Verify with `git branch -r` on the submodule. The submodule should be updated to track the `v1.5.0` branch.
**Warning signs:** CI fails with "ref not found" errors.

### Pitfall 4: C++ API Changes in DuckDB 1.5
**What goes wrong:** shim.cpp fails to compile against the new amalgamation due to C++ API changes.
**Why it happens:** DuckDB 1.5.0 made several internal changes: "Encapsulate scalar/aggregate function callbacks", "Encapsulate `BaseScalarFunction` properties", new `parser_override_function_t`, and storage restructuring. While our shim uses a narrow API surface (ParserExtension, DBConfig, FunctionData, TableFunction, DataChunk), any of these could have signature changes.
**How to avoid:** Compile first (`just build`), read error messages, fix one at a time. The shim.cpp file uses: `ParserExtension`, `ParserExtensionParseResult`, `ParserExtensionPlanResult`, `ParserExtensionParseData`, `FunctionData`, `TableFunction`, `DataChunk`, `DBConfig`, `DatabaseWrapper`, `LogicalType`, `Value`. Check each against the new amalgamation header.
**Warning signs:** C++ compilation errors in the `cc` crate build step.

### Pitfall 5: DatabaseWrapper Reinterpret Cast
**What goes wrong:** The `sv_register_parser_hooks` function in shim.cpp reinterpret_casts `duckdb_database->internal_ptr` to `DatabaseWrapper*`. If DuckDB 1.5 changed the internal structure of `duckdb_database`, this cast produces UB.
**Why it happens:** This is an implementation-detail cast, not using public API. It bypasses the C API abstraction to get at the C++ `DatabaseInstance`.
**How to avoid:** This is the highest-risk area. Check if the `duckdb_database` internal structure changed. If it did, find the new way to extract `DatabaseInstance&` from a C API handle. Test with `just build && just test-sql` -- if parser hooks fail to register, this is likely the cause.
**Warning signs:** Extension loads but `CREATE SEMANTIC VIEW` fails silently or crashes.

### Pitfall 6: PEG Parser Breaking Parser Hooks
**What goes wrong:** With `CALL enable_peg_parser()`, the extension's Bison-based parser hooks may not fire because the PEG parser takes a different code path.
**Why it happens:** The PEG parser is a completely different parsing pipeline. Parser extensions registered via `DBConfig::parser_extensions` may only be consulted by the Bison parser.
**How to avoid:** This is expected behavior per the CONTEXT.md decision ("Ignore PEG parser as default behavior"). Add a test that documents the behavior: load extension, enable PEG parser, try CREATE SEMANTIC VIEW, document whether it works or not.
**Warning signs:** Parser hooks silently ignored when PEG is enabled.

### Pitfall 7: Python Test Version Mismatch
**What goes wrong:** Integration tests (test_vtab_crash.py, test_caret_position.py, etc.) fail because they install duckdb==1.4.4 via PEP 723 headers while the extension is built against 1.5.0.
**Why it happens:** All Python files have hardcoded `# dependencies = ["duckdb==1.4.4"]` in their PEP 723 metadata.
**How to avoid:** Update ALL Python files' PEP 723 headers to `duckdb==1.5.0` as part of the version bump. Use: `find . -name '*.py' -exec grep -l 'duckdb==' {} \;` to find them all.
**Warning signs:** "Catalog Error: Failed to load extension" in Python integration tests.

## Code Examples

### Cargo.toml Version Pin Update (main)
```toml
# Before (DuckDB 1.4.4):
duckdb = { version = "=1.4.4", default-features = false }
libduckdb-sys = "=1.4.4"

# After (DuckDB 1.5.0):
duckdb = { version = "=1.10500.0", default-features = false }
libduckdb-sys = "=1.10500.0"
```
Source: [duckdb-rs main Cargo.toml](https://raw.githubusercontent.com/duckdb/duckdb-rs/main/Cargo.toml), workspace version confirmed as `1.10500.0`.

### Cargo.toml Version Suffix (duckdb/1.4.x branch)
```toml
[package]
version = "0.5.4+duckdb1.4"
# ... duckdb and libduckdb-sys remain at =1.4.4
```
Note: The `+` suffix in semver is build metadata -- it's ignored by Cargo for dependency resolution but distinguishes the builds.

### Build.yml Update (main)
```yaml
# Before:
uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.4.4
with:
  duckdb_version: v1.4.4
  ci_tools_version: v1.4.4

# After:
uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.5.0
with:
  duckdb_version: v1.5.0
  ci_tools_version: v1.5.0
```

### Build.yml Branch Triggers (both branches)
```yaml
on:
  push:
    branches:
      - main
      - 'release/*'
      - 'gsd/*'
      - 'milestone/*'
      - 'duckdb/*'         # <-- add for LTS branch CI
```

### DuckDB Version Monitor Dual-Track Pattern
```yaml
jobs:
  check-latest:
    name: Check for new DuckDB latest release
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: main
          submodules: recursive
      - name: Get latest DuckDB release
        id: latest
        run: |
          LATEST=$(gh api repos/duckdb/duckdb/releases/latest --jq '.tag_name')
          CURRENT=$(cat .duckdb-version)
          echo "latest=$LATEST" >> $GITHUB_OUTPUT
          echo "current=$CURRENT" >> $GITHUB_OUTPUT
          echo "is_new=$( [ "$LATEST" != "$CURRENT" ] && echo true || echo false )" >> $GITHUB_OUTPUT
      # ... bump, build, test, PR (targeting main)

  check-lts:
    name: Check for new DuckDB LTS release
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: duckdb/1.4.x
          submodules: recursive
      - name: Get latest DuckDB 1.4.x release
        id: lts
        run: |
          # Query all releases, filter for v1.4.* tags
          LATEST_LTS=$(gh api repos/duckdb/duckdb/releases --jq '[.[] | select(.tag_name | startswith("v1.4")) | .tag_name] | first')
          CURRENT=$(cat .duckdb-version)
          echo "latest=$LATEST_LTS" >> $GITHUB_OUTPUT
          echo "current=$CURRENT" >> $GITHUB_OUTPUT
          echo "is_new=$( [ "$LATEST_LTS" != "$CURRENT" ] && echo true || echo false )" >> $GITHUB_OUTPUT
      # ... bump, build, test, PR (targeting duckdb/1.4.x)
```

### PEG Compatibility Test (sqllogictest)
```
# Test PEG parser compatibility
# This test documents whether parser hooks work with PEG enabled

require semantic_views

statement ok
CALL enable_peg_parser();

# Try creating a semantic view with PEG parser active
# Expectation: This may fail since PEG parser uses a different code path
# for parser extensions. Document the actual behavior.
statement ok
CREATE TABLE peg_test (x INTEGER, y DOUBLE);

statement error
CREATE SEMANTIC VIEW peg_demo (
    TABLES (
        base_table peg_test PRIMARY KEY (x)
    )
    DIMENSIONS (
        dim_x x
    )
    METRICS (
        total_y SUM(y)
    )
);
----
# Expected error pattern depends on PEG parser behavior
# If PEG ignores parser extensions, DuckDB will show its own parse error
```

### CE description.yml with andium field
```yaml
# Source: https://github.com/duckdb/community-extensions/blob/main/extensions/yaml/description.yml
repo:
  github: paul-rl/duckdb-semantic-views
  ref: <commit-sha-from-main>         # Built against DuckDB latest (1.5.x)
  andium: <commit-sha-from-duckdb/1.4.x>  # Built against DuckDB LTS (1.4.x)
```
Source: Confirmed via yaml extension's description.yml -- `andium` field sits alongside `ref` in the `repo` section. Both are commit SHA hashes.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| duckdb-rs versioning mirrors DuckDB (1.4.4 = 1.4.4) | duckdb-rs encodes DuckDB version (1.5.0 = 1.10500.0) | duckdb-rs v1.10500.0, March 2026 | Cargo.toml pins use new scheme |
| Single DuckDB version track | LTS (Andium) + Latest dual track | DuckDB 1.4.0, September 2025 | Extensions need dual-version support |
| Bison parser only | Bison + PEG parser (opt-in) | DuckDB 1.5.0, March 2026 | Parser hooks may need PEG compat eventually |
| extension-ci-tools tags | extension-ci-tools branches | Always branches (verified) | CI `uses:` references are branch refs |
| `duckdb_entrypoint_c_api` macro had `.unwrap()` panics | Improved error handling in macro | duckdb-rs v1.10500.0 | Our manual FFI entry point is unaffected |

**Deprecated/outdated:**
- DuckDB 1.4.x enters end-of-life September 2026 (1 year from LTS announcement)
- Python 3.9 support dropped in DuckDB 1.5.0 (minimum is now Python 3.10)

## Open Questions

1. **Will shim.cpp compile against DuckDB 1.5.0 amalgamation?**
   - What we know: DuckDB 1.5.0 "encapsulated" scalar/aggregate function callbacks and BaseScalarFunction properties. Our shim uses ParserExtension, FunctionData, TableFunction, DataChunk, DBConfig, DatabaseWrapper.
   - What's unclear: Whether any of these types had signature or layout changes. The release notes mention changes but don't specify which types are affected.
   - Recommendation: Build-first approach -- download 1.5.0 amalgamation and attempt compilation. Fix errors as they arise. This is the fastest path to certainty.
   - **Confidence: LOW** -- cannot verify without building

2. **Has DatabaseWrapper internal layout changed?**
   - What we know: `sv_register_parser_hooks` reinterpret_casts `db_handle->internal_ptr` to `DatabaseWrapper*` then accesses `->database->instance`. This is an undocumented internal cast.
   - What's unclear: Whether the layout of this internal chain changed in 1.5.0.
   - Recommendation: If parser hooks fail silently after upgrading, this cast is the first place to investigate. Test with a simple `CREATE SEMANTIC VIEW` after `LOAD`.
   - **Confidence: LOW** -- internal implementation detail, not in any changelog

3. **Will PEG parser completely ignore Bison-based parser extension hooks?**
   - What we know: PEG is opt-in via `CALL enable_peg_parser()`. DuckDB 1.5.0 introduced `parser_override_function_t` for PEG-based grammar extension. Our extension uses Bison's `ParserExtension` + `parse_function`/`plan_function` hooks.
   - What's unclear: Whether the PEG parser has any fallback mechanism to consult `parser_extensions` registered via the old mechanism.
   - Recommendation: Add a test that enables PEG and tries CREATE SEMANTIC VIEW. Document the result (pass/fail) without trying to fix it -- PEG migration is future work (PEG-01).
   - **Confidence: MEDIUM** -- PEG is explicitly opt-in, our hooks should work with default Bison parser

4. **Do Windows patch markers in build.rs still exist in DuckDB 1.5.0 amalgamation?**
   - What we know: Patch 1 looks for `#endif // defined(_WIN32)\n\n// Platform-specific helpers`. Patch 2 looks for `#undef CreateDirectory\n#undef MoveFile\n#undef RemoveDirectory`. These are tied to specific code in duckdb.cpp around lines 25363-38094.
   - What's unclear: Whether the new amalgamation has the same structure at these locations.
   - Recommendation: Download 1.5.0 amalgamation and grep for patch markers. If missing, find new locations and update. If markers moved significantly, build.rs already has warning fallbacks (`cargo:warning=...patch N skipped`).
   - **Confidence: MEDIUM** -- DuckDB tends to maintain similar patterns but line numbers shift

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust) + sqllogictest (Python runner) + uv-run integration tests |
| Config file | Cargo.toml (Rust), test/sql/TEST_LIST (sqllogictest), justfile (orchestration) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DKDB-01 | Build + test on DuckDB 1.5.x | integration | `just test-all` (on main after upgrade) | N/A -- existing tests validate |
| DKDB-02 | Build + test on DuckDB 1.4.x | integration | `just test-all` (on duckdb/1.4.x) | N/A -- existing tests validate |
| DKDB-03 | LTS branch exists and works | smoke | `git branch -r \| grep duckdb/1.4.x` + `just test-all` on branch | N/A -- manual verification |
| DKDB-04 | CI runs both versions | smoke | Push to both branches, verify CI runs | N/A -- CI pipeline verification |
| DKDB-05 | `.duckdb-version` correct on each branch | unit | `cat .duckdb-version` on each branch | N/A -- file content check |
| DKDB-06 | Version monitor checks both tracks | smoke | Manual `workflow_dispatch` trigger of DuckDBVersionMonitor.yml | N/A -- workflow verification |

### PEG Compatibility Test
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| (DKDB-01 sub) | PEG parser compat documented | smoke | `just test-sql` (includes PEG test file) | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test` (quick, catches Rust API breaks)
- **Per wave merge:** `just test-all` (full quality gate)
- **Phase gate:** Full `just test-all` on BOTH branches before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/peg_compat.test` -- PEG parser compatibility smoke test
- [ ] Update `test/sql/TEST_LIST` to include PEG compat test (if created as separate file)

## Sources

### Primary (HIGH confidence)
- [duckdb-rs GitHub releases](https://github.com/duckdb/duckdb-rs/releases) -- v1.10500.0 = DuckDB 1.5.0, new versioning scheme confirmed
- [duckdb-rs main Cargo.toml](https://raw.githubusercontent.com/duckdb/duckdb-rs/main/Cargo.toml) -- workspace version 1.10500.0 for duckdb, libduckdb-sys, duckdb-loadable-macros
- [extension-ci-tools remote branches](verified via local git fetch) -- `origin/v1.5.0` branch exists
- [DuckDB v1.5.0 release notes](https://github.com/duckdb/duckdb/releases/tag/v1.5.0) -- C API additions, parser changes, breaking changes
- [yaml extension description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/yaml/description.yml) -- `repo.andium` field confirmed alongside `repo.ref`
- Project source files (Cargo.toml, build.rs, shim.cpp, Build.yml, DuckDBVersionMonitor.yml, justfile, Makefile) -- current state verified

### Secondary (MEDIUM confidence)
- [DuckDB 1.5.0 announcement blog](https://duckdb.org/2026/03/09/announcing-duckdb-150) -- PEG parser opt-in, Andium LTS through September 2026, Python 3.9 dropped
- [DuckDB PEG parser discussion](https://github.com/duckdb/duckdb/discussions/20618) -- PEG is opt-in in v1.5.0, will replace Bison parser "completely for every SQL query" when enabled
- [DuckDB Community Extensions documentation](https://duckdb.org/community_extensions/documentation) -- description.yml format
- [DeepWiki: extension template maintenance](https://deepwiki.com/duckdb/extension-template/5-maintenance-and-updates) -- 5-step version update process

### Tertiary (LOW confidence)
- C++ API compatibility of ParserExtension/TableFunction/FunctionData in DuckDB 1.5.0 -- inferred from release notes listing "encapsulate" changes, but not verified against actual header diffs. Must build to confirm.
- DatabaseWrapper internal layout stability -- no documentation, only verified by building and testing.

## Metadata

**Confidence breakdown:**
- Standard stack (versions, pins): **HIGH** -- crate versions confirmed from GitHub, extension-ci-tools branches verified locally
- Architecture (branch strategy, CI): **HIGH** -- well-defined by CONTEXT.md decisions, follows established patterns
- Upgrade compatibility (shim.cpp, build.rs): **LOW** -- cannot verify without building against 1.5.0 amalgamation. DuckDB C++ API is internal and changes between versions.
- Pitfalls: **MEDIUM-HIGH** -- based on concrete analysis of current code and DuckDB 1.5.0 changes, but some are speculative until build is attempted

**Research date:** 2026-03-15
**Valid until:** 2026-04-15 (stable -- DuckDB 1.5.x is released, duckdb-rs 1.10500.0 is released)
