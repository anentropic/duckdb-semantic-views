# Domain Pitfalls -- Snowflake-Parity & Registry Publishing (v0.5.4)

**Domain:** Removing explicit cardinality keywords in favor of Snowflake-style inference from PK/UNIQUE constraints, supporting multiple DuckDB versions, publishing to the community extension registry, and shipping a documentation site
**Researched:** 2026-03-15
**Context:** The extension has 441 tests, uses C_STRUCT_UNSTABLE ABI, compiles a vendored DuckDB amalgamation via cc crate, and stores semantic view definitions as JSON in `semantic_layer._definitions`. Cardinality (MANY TO ONE / ONE TO ONE / ONE TO MANY) is declared explicitly in RELATIONSHIPS and stored in the `Join.cardinality` field. Fan trap detection in `expand.rs` depends on `Cardinality` enum values to block inflated aggregation.

---

## Critical Pitfalls

Mistakes that cause rewrites, data loss, silent wrong results, or registry rejection.

### C1: Removing Cardinality Keywords Breaks Stored Definitions That Explicitly Declare Them

**What goes wrong:**
Existing semantic view definitions created with v0.5.3 may contain explicit cardinality declarations like `MANY TO ONE` or `ONE TO MANY` in their RELATIONSHIPS clause. These are stored as JSON in `semantic_layer._definitions` with `"cardinality": "OneToMany"` in the `Join` object. The `Cardinality` enum in `model.rs` has three variants: `ManyToOne` (default), `OneToOne`, `OneToMany`.

If the v0.5.4 DDL parser rejects cardinality keywords (because inference replaces them), and a user tries to `CREATE OR REPLACE` a view by copying their existing DDL, the parser errors on `MANY TO ONE`. Worse, if the `Cardinality` enum is removed from the model, old stored JSON with `"cardinality": "OneToMany"` fails to deserialize, making existing views unloadable.

The current serde configuration uses `#[serde(default, skip_serializing_if = "Cardinality::is_default")]` on `Join.cardinality`. This means:
- `ManyToOne` is not serialized (omitted from JSON) -- safe
- `OneToOne` and `OneToMany` ARE serialized -- these will break if the enum is removed

**Why it happens:**
The natural instinct is to remove the old code path when replacing it with inference. But stored JSON persists across extension upgrades. Users who defined views with explicit cardinality in v0.5.3 have JSON containing the enum values.

**Consequences:**
- Extension fails to load existing views on startup (`init_catalog` crashes deserializing old JSON)
- User cannot recreate views from saved DDL that includes cardinality keywords
- Silent data corruption if cardinality defaults to `ManyToOne` when the stored value was `OneToMany` -- fan trap detection stops working for those relationships

**Prevention:**
- Keep the `Cardinality` enum in `model.rs` with all three variants and the `#[serde(default)]` annotation. Old JSON must always deserialize.
- In the body parser, make cardinality keywords OPTIONAL but still ACCEPTED. Parse them if present, ignore them if absent (inference fills in the value). This is backward compatible: old DDL with `MANY TO ONE` still parses, new DDL without it also parses.
- At define time, if explicit cardinality is provided AND UNIQUE/PK inference would produce a different result, emit a warning (not an error) that the explicit cardinality overrides inference. This catches mismatches without breaking existing definitions.
- Add a new `unique_columns: Vec<String>` field to `TableRef` (with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`) for UNIQUE constraint declarations. Inference uses both `pk_columns` and `unique_columns`.
- Do NOT remove the `parse_cardinality_tokens()` function in `body_parser.rs`. Keep it, but make it a fallback after inference. The parser should try inference first (from PK/UNIQUE on referenced table), then accept explicit override.
- **Confidence:** HIGH. The backward compatibility pattern is well-established in the codebase (see `Join.on`, `Join.from_cols`, `Join.join_columns` -- three generations of join format coexist via serde defaults).

**Detection:** Load a database with v0.5.3-created views containing `OneToMany` cardinality. Verify they deserialize correctly. Verify fan trap detection still works. Test: create a view with `ONE TO MANY` in v0.5.3 format, upgrade extension, verify `DESCRIBE` still shows correct information.

**Phase assignment:** Cardinality inference phase. Must be the FIRST thing addressed before any parser changes.

---

### C2: DuckDB 1.5 Uses Different Amalgamation Layout, Breaking the cc Crate Build

**What goes wrong:**
The extension compiles the DuckDB amalgamation (`duckdb.cpp` + `duckdb.hpp`) via the `cc` crate in `build.rs`. The amalgamation is downloaded from `https://github.com/duckdb/duckdb/releases/download/vX.Y.Z/libduckdb-src.zip`. DuckDB 1.5.0 was released on 2026-03-09.

Between DuckDB minor versions, the amalgamation can change in ways that break the build:
1. **New C++ includes or removed headers** -- the `build.rs` Windows patching (`patch_duckdb_cpp_for_windows`) searches for specific string markers like `#endif // defined(_WIN32)\n\n// Platform-specific helpers`. If DuckDB 1.5 rearranges these markers, the patch silently fails (it prints a `cargo:warning` but continues), and the Windows build breaks with `GetObject` / `interface` macro conflicts.
2. **New system library dependencies** -- DuckDB 1.4.4 required `rstrtmgr.lib` on Windows (added to `build.rs`). DuckDB 1.5 may require additional system libraries.
3. **C++ symbol signature changes** -- `shim.cpp` calls specific DuckDB C++ constructors and methods (`ParserExtension`, `TableFunction`, `CreateScalarFunctionInfo`). If any internal C++ API changes signatures, the shim fails to compile.
4. **The `duckdb-rs` crate may not have a 1.5 release yet** -- as of 2026-03-15, the latest `duckdb` crate on crates.io is 1.4.4. The extension pins `duckdb = "=1.4.4"`. A 1.5 version of the crate must exist before the extension can target DuckDB 1.5.

**Why it happens:**
DuckDB explicitly does NOT guarantee ABI or API stability across minor versions. The amalgamation is a single-file dump of the entire DuckDB codebase (~300K lines). Any internal refactor can change string patterns, add includes, or modify class signatures. The `C_STRUCT_UNSTABLE` ABI type means the extension binary is strictly tied to one DuckDB version.

**Consequences:**
- Build failure on one or more platforms (most likely Windows due to the patching logic)
- Compilation of `shim.cpp` fails with cryptic C++ errors about undefined or changed symbols
- Cargo.toml cannot pin `duckdb = "=1.5.0"` if the crate does not exist yet
- CI turns red on the DuckDB 1.5 branch with no obvious fix

**Prevention:**
- Do NOT attempt to support DuckDB 1.5 until `duckdb-rs` 1.5.x is published on crates.io. Check `https://crates.io/crates/duckdb` before starting.
- Create a separate branch (e.g., `duckdb-1.5`) for the 1.5 port. Keep main on 1.4.4 until 1.5 builds cleanly on all platforms.
- Download the DuckDB 1.5 amalgamation and diff against 1.4.4's `duckdb.hpp` to identify C++ API changes in `ParserExtension`, `TableFunction`, and related classes BEFORE writing any code.
- Test the `patch_duckdb_cpp_for_windows()` function with the 1.5 amalgamation. If markers have moved, update the patch.
- Run `make release` on all three platforms (Linux, macOS, Windows) as the first step of the 1.5 port. Do not write features until the build is green.
- DuckDB 1.5 release notes mention "Parser override functionality with opt-in mechanism" which could affect the parser extension hooks in `shim.cpp`. Investigate `parser_override_function_t` as a possible replacement for or change to the `parse_function` fallback hook.
- **Confidence:** MEDIUM. The specific breakages depend on what changed between DuckDB 1.4.4 and 1.5.0, which requires hands-on investigation. The general risk pattern is HIGH confidence (every DuckDB version bump has broken something historically).

**Detection:** `make release` fails on any platform. `cargo test` fails with link errors or missing symbols. The DuckDB Version Monitor CI workflow opens a PR when 1.5.0 is detected.

**Phase assignment:** Multi-version DuckDB support phase. Must be a standalone phase before any feature work on the 1.5 branch.

---

### C3: Community Extension Registry Requires CMake-Compatible Build, Not Pure Cargo

**What goes wrong:**
The community extension CI builds extensions using the `duckdb/extension-ci-tools` reusable workflows. The build system delegates to Make targets defined in `extension-ci-tools/makefiles/`. For Rust extensions, the `rust.Makefile` delegates compilation to `cargo build` and then runs a post-build script to append a binary footer converting the shared library into a loadable DuckDB extension.

The critical discovery: the community extension registry's CI runs the build using its OWN infrastructure, not the extension's CI. When you submit `description.yml`, the registry CI clones your repo, runs `make release`, and builds for all platforms. If your build has any dependency that is not handled by the standard `make configure && make release` flow, the registry build fails.

Specific risks for this extension:
1. **Amalgamation download** -- the `Makefile` downloads `libduckdb-src.zip` from GitHub Releases. The registry CI may not have network access during build, or the download URL may be blocked. The `ensure_amalgamation` target must succeed.
2. **`UNSTABLE_C_API_FLAG`** -- the Makefile overrides this to `--abi-type C_STRUCT_UNSTABLE`. The registry CI must respect this override. If the registry's build scripts reset it, the extension is built with the wrong ABI type.
3. **Platform exclusions** -- the extension excludes `wasm_mvp`, `wasm_eh`, `wasm_threads`, `windows_amd64_mingw`, `linux_amd64_musl`, and `linux_arm64_musl` in the Build.yml workflow. The `description.yml` must declare the SAME exclusions via `excluded_platforms`, or the registry CI attempts to build for platforms that will fail.
4. **Rust toolchain** -- the registry CI must have Rust installed. The `description.yml` must include `requires_toolchains: [rust, python3]` (matching the `extra_toolchains` in Build.yml and the `rusty_quack` precedent).
5. **`cc` crate compilation time** -- the amalgamation compilation takes 2-5 minutes. The registry CI may have a build timeout that this exceeds, especially on arm64 where compilation is slower.
6. **`extension-ci-tools` version mismatch** -- the submodule in the repo points to v1.4.4. The registry CI may use a different version. The `description.yml` `repo.ref` must point to a commit where the submodule is consistent.

**Why it happens:**
The extension's own CI (Build.yml) is customized with `exclude_archs`, `extra_toolchains`, and specific version pinning. The registry CI uses the `description.yml` to configure these same parameters, but the mapping between Build.yml parameters and description.yml fields is not documented and easy to get wrong.

**Consequences:**
- Registry CI build fails, PR is rejected
- Build succeeds on some platforms but fails on others (e.g., Windows due to `rstrtmgr.lib`, wasm due to Rust)
- Extension is published but crashes on load due to wrong ABI type
- Extension is published for platforms where it was never tested

**Prevention:**
- Study the `rusty_quack` extension's `description.yml` as a template. It uses: `language: rust`, `build: cargo`, `excluded_platforms: [wasm_mvp, wasm_eh, wasm_threads, windows_amd64_rtools, windows_amd64_mingw, linux_amd64_musl]`, `requires_toolchains: [rust, python3]`.
- Mirror the `exclude_archs` from Build.yml into `excluded_platforms` in `description.yml`. Add `windows_arm64` which is in Build.yml but not in rusty_quack's exclusions (this extension has Windows-specific `rstrtmgr` linking that needs testing on arm64).
- Before submitting to the registry, test the full build locally using the same Make targets the registry would use: `make configure && make release && make test_release`.
- The `description.yml` `repo.ref` should be a tagged release commit (e.g., `v0.5.4`), not a branch. Tags are immutable; branches are not.
- Verify the `UNSTABLE_C_API_FLAG` override in the Makefile is applied AFTER the include of `base.Makefile` (it currently is -- line 23). If `extension-ci-tools` changes the include order, this breaks.
- **Confidence:** MEDIUM. The `rusty_quack` precedent proves Rust extensions can be published, but this extension is more complex (C++ shim, amalgamation download, platform-specific linking). The specific failure modes depend on registry CI internals.

**Detection:** Submit a draft PR to `duckdb/community-extensions` with `description.yml` and monitor CI output. Fix failures iteratively. Do NOT wait until all other v0.5.4 work is complete to test the submission -- do a dry run early.

**Phase assignment:** Registry publishing phase. Should be attempted BEFORE the documentation phase so CI issues can be resolved without time pressure.

---

### C4: Cardinality Inference From PK/UNIQUE Produces Wrong Results for Composite Keys and Partial Key References

**What goes wrong:**
Snowflake's cardinality inference works as follows: if the FK references columns that are declared as PRIMARY KEY or UNIQUE on the target table, the relationship is MANY-TO-ONE (many FK rows point to one unique target row). If the FK columns match the PK of the source table AND the target is PK/UNIQUE, the relationship is ONE-TO-ONE.

The inference logic needs to handle several edge cases:

1. **Composite primary keys** -- a table has `PRIMARY KEY (customer_id, product_id)`. A relationship references only `customer_id`. Since `customer_id` alone is NOT the full PK, it is not unique. The relationship should be treated as MANY-TO-MANY (or at least "unknown cardinality"), not MANY-TO-ONE. If the inference naively checks "is `customer_id` part of a PK?" and returns MANY-TO-ONE, fan trap detection is disabled for a relationship that DOES produce fan-out.

2. **UNIQUE subset of PK** -- a table declares `PRIMARY KEY (id)` and also `UNIQUE (email)`. A relationship references `email`. This IS a valid many-to-one relationship (email is unique). The inference must check BOTH pk_columns AND unique_columns, not just pk_columns.

3. **Self-referential relationships** -- the current `RelationshipGraph::from_definition()` rejects self-references (`from == to`). But a table can have a self-referential FK (e.g., `employee(manager_id) REFERENCES employee(id)` for an org chart). If cardinality inference is applied to self-referential relationships, and the extension later supports them, the inference logic must handle the case where from_alias and to_alias refer to the same physical table with different aliases.

4. **No PK or UNIQUE declared on target** -- if the target table has no PK and no UNIQUE columns, cardinality cannot be inferred. The extension must either: (a) default to MANY-TO-ONE (current behavior, matches existing serde default), (b) require explicit cardinality, or (c) reject the relationship. Option (a) is dangerous because MANY-TO-ONE disables fan trap detection for that path -- if the actual cardinality is ONE-TO-MANY, metrics will be silently inflated.

5. **FK columns reference non-PK, non-UNIQUE columns** -- `o(status) REFERENCES s` where `s.status` is neither PK nor UNIQUE. This is a valid join but cardinality is unknown. Should the inference produce an error, a warning, or a default?

**Why it happens:**
Snowflake can get away with simple inference because it validates constraints at query optimization time (with the RELY property). DuckDB actually enforces PK/UNIQUE constraints, but the semantic views extension doesn't query DuckDB's constraint metadata -- it only knows what the user declares in the DDL. The inference operates on DDL declarations, not on actual table metadata.

**Consequences:**
- Silent fan-out: inference says MANY-TO-ONE but actual data has duplicates in the "unique" column
- Fan trap detection disabled for relationships that should trigger it
- Wrong results with no error or warning

**Prevention:**
- Inference rule: a relationship is MANY-TO-ONE if and only if the referenced columns on the target table EXACTLY match either the full `pk_columns` or a declared `unique_columns` set. Partial matches (subset of composite PK) are NOT sufficient.
- Inference rule: a relationship is ONE-TO-ONE if the FK columns on the source table ALSO exactly match the source table's `pk_columns` or a `unique_columns` set, AND the target columns match the target's PK/UNIQUE.
- Inference rule: if neither condition is met, cardinality is UNKNOWN. In this case, require explicit cardinality or default to a safe value. Recommendation: default to MANY-TO-ONE (matches current behavior and Snowflake convention) but emit a define-time warning: "Cardinality of relationship 'X' could not be inferred. Defaulting to MANY TO ONE. Declare UNIQUE on the target table's referenced columns to enable inference."
- Add `unique_columns: Vec<Vec<String>>` to `TableRef` (a list of UNIQUE constraints, each being a list of column names). A single table can have multiple UNIQUE constraints.
- Self-referential relationships: defer to a future milestone. The current rejection in `graph.rs` is correct for tree-structured graphs.
- **Confidence:** HIGH. The inference rules are well-defined. The risk is in incomplete edge case handling.

**Detection:** Test with composite PK where FK references a subset. Verify fan trap detection still triggers. Test with UNIQUE column reference. Test with no PK/UNIQUE on target.

**Phase assignment:** Cardinality inference phase. The inference rules must be defined and tested BEFORE removing the explicit cardinality syntax.

---

## Moderate Pitfalls

### M1: Multi-Version Support Requires Two CI Pipelines, Two Cargo.toml Versions, and Careful Branch Management

**What goes wrong:**
Supporting DuckDB 1.4.x and 1.5.x simultaneously means maintaining two build configurations:
- `duckdb = "=1.4.4"` with amalgamation v1.4.4 on the main/LTS branch
- `duckdb = "=1.5.0"` with amalgamation v1.5.0 on the latest branch

If these are maintained on separate branches, every feature change must be cherry-picked or merged across branches. If maintained with cargo feature flags, `Cargo.toml` becomes complex and `cargo test` can only test one version at a time.

The `extension-ci-tools` repository supports this: "each branch targets a specific version of DuckDB" and "the aim is to support the latest 2 DuckDB versions." The extension submodule `extension-ci-tools` must be updated to the appropriate tag on each branch.

**Why it happens:**
DuckDB's rapid release cycle (1.4.4 in January 2026, 1.5.0 in March 2026) means the extension must track two moving targets. Feature development happens on one branch while the other requires backports.

**Consequences:**
- Features diverge between branches, users get different behavior depending on DuckDB version
- Bug fixes must be applied twice (once per branch)
- CI costs double (two full build matrices)
- Cherry-pick conflicts accumulate over time

**Prevention:**
- Use the `extension-ci-tools` branching convention: `main` branch targets the latest DuckDB version, a long-lived branch (e.g., `andium` or `lts-1.4`) targets the previous version.
- Feature development happens on `main` (latest DuckDB). Backports to the LTS branch are done selectively for bug fixes only, not new features.
- The `Makefile` and `Build.yml` already read `.duckdb-version` as the single source of truth. Each branch sets this file to its target version.
- Do NOT try to support both versions from a single branch with feature flags. The amalgamation is a 300K-line file that differs between versions -- conditional compilation is impractical.
- Pin `extension-ci-tools` submodule to the correct tag on each branch (e.g., `v1.4.4` on LTS, `v1.5.0` on main).
- Consider whether multi-version support is worth the maintenance cost. If the target audience (community extension users) always uses the latest DuckDB, a single-version strategy is simpler. The LTS branch can be created but not actively maintained until there is demand.
- **Confidence:** MEDIUM. The branching strategy is standard, but the execution overhead is significant for a solo-maintained project.

**Detection:** Feature drift between branches. CI failures on the LTS branch after changes to main. Cherry-pick conflicts.

**Phase assignment:** Multi-version DuckDB support phase. Should define the branching strategy BEFORE creating the second branch.

---

### M2: The `description.yml` `build: cargo` vs `build: cmake` Choice Affects Platform Support

**What goes wrong:**
The community extension registry supports two build types: `cmake` (default, used by C++ extensions) and `cargo` (used by Rust extensions like `rusty_quack`). The current extension uses `make release` which delegates to `cargo build`, matching the `cargo` build type.

However, the extension ALSO compiles C++ code via the `cc` crate (the DuckDB amalgamation + shim.cpp). This is unusual for a "Rust" extension -- it is a hybrid. The `cargo` build type in the registry CI may not install C++ build tools on all platforms. If the registry CI for `build: cargo` only provides Rust toolchain and not a C++ compiler, the `cc` crate compilation of `duckdb.cpp` fails.

The `rusty_quack` extension is a pure Rust extension (no C++ shim). This extension is NOT pure Rust -- it has a C++ component compiled via `cc`.

**Why it happens:**
The `cc` crate automatically finds C++ compilers on most platforms (it delegates to the system compiler). But in a CI environment, the compiler must be explicitly installed. The registry CI for `build: cargo` may assume no C++ compilation is needed.

**Consequences:**
- Registry build fails with "C++ compiler not found" on some platforms
- The extension cannot be published despite building locally and in its own CI

**Prevention:**
- Test the registry build early by submitting a draft `description.yml` and monitoring CI.
- If `build: cargo` does not provide C++ tools, investigate whether `build: cmake` is needed instead. This would require adding a `CMakeLists.txt` that invokes `cargo build` and the `cc` crate handles the C++ compilation internally.
- Alternatively, check if `requires_toolchains` can request both `rust` and a C++ toolchain. The `extra_toolchains` parameter in Build.yml already requests `rust;python3`. The registry may support additional toolchains.
- As a last resort, examine whether the C++ shim can be pre-compiled or the amalgamation can be vendored in the repository (rather than downloaded at build time). This trades binary size in the repo for build reliability.
- **Confidence:** LOW. This depends on the registry CI's internal configuration for `build: cargo` extensions. No documentation exists for hybrid Rust+C++ builds. Verification requires actually submitting to the registry.

**Detection:** Registry CI build log shows `cc` crate errors or missing C++ compiler.

**Phase assignment:** Registry publishing phase. Must be validated early.

---

### M3: Zensical Documentation Site Has No `gh-deploy` Command -- Must Use GitHub Actions

**What goes wrong:**
Zensical (the documentation static site generator from the Material for MkDocs team) does NOT have an `gh-deploy` command like MkDocs. MkDocs users are accustomed to running `mkdocs gh-deploy` to push built docs to the `gh-pages` branch. Zensical requires a GitHub Actions workflow that builds the site and deploys via the `actions/deploy-pages` action.

If the deployment workflow is misconfigured:
1. **Missing permissions** -- the workflow needs `pages: write` and `id-token: write` permissions. Without these, the deployment silently fails with a 403.
2. **Base URL mismatch** -- if the site is deployed at `https://user.github.io/repo/` (project page, not user page), the `site_url` in configuration must include the `/repo/` prefix. Without it, all internal links and asset paths are broken (CSS missing, links 404).
3. **Caching issues** -- Zensical's documentation explicitly warns: "Caching on CI systems is not recommended at the moment as the caching functionality will undergo revisions."
4. **Branch configuration** -- GitHub Pages must be configured to use "GitHub Actions" as the source (not a branch). The repository Settings > Pages > Source must be changed from the default.

**Why it happens:**
Zensical is new (first release 2025) and the deployment model differs from MkDocs. Documentation on GitHub Pages deployment is available but sparse. The migration path from MkDocs to Zensical is documented, but this project starts fresh.

**Consequences:**
- Documentation site shows a 404 or blank page
- Internal links broken, CSS missing (base URL issue)
- Deployment workflow runs but does not actually publish
- Site content is stale because caching prevents updates

**Prevention:**
- Start with Zensical's official GitHub Actions workflow template (available in the `zensical new` project scaffolding). Do not write the workflow from scratch.
- Set `site_url` to `https://username.github.io/duckdb-semantic-views/` (with trailing slash and repo name).
- Configure GitHub Pages source to "GitHub Actions" in repository settings BEFORE the first workflow run.
- Do NOT enable caching in the CI workflow (per Zensical's recommendation).
- Test locally with `zensical serve` before pushing. Verify the site renders correctly at `http://localhost:8000/`.
- Verify the deployment by checking the Pages URL immediately after the first workflow run.
- **Confidence:** HIGH. GitHub Pages deployment is well-documented. The pitfalls are all avoidable with correct configuration.

**Detection:** 404 on the Pages URL. Missing CSS (blank white page with unstyled text). Broken internal links.

**Phase assignment:** Documentation site phase. Low risk if the official template is followed.

---

### M4: DuckDB 1.5 Parser Override May Conflict With or Replace the `parse_function` Fallback Hook

**What goes wrong:**
DuckDB 1.5.0 release notes mention "Parser override functionality with opt-in mechanism." The current extension uses `parse_function` (a parser fallback hook) registered in `shim.cpp` to intercept `CREATE SEMANTIC VIEW` statements. If DuckDB 1.5 introduces a new `parser_override_function_t` mechanism that changes how parser hooks work, the existing `parse_function` registration may:
1. Stop being called (if DuckDB changes the fallback hook dispatch)
2. Be called with different arguments or return type expectations
3. Conflict with the new parser override mechanism

The `shim.cpp` C++ code directly references DuckDB internal C++ classes (`ParserExtension`, `ParserExtensionInfo`, etc.). If these classes change in 1.5, the shim fails to compile.

**Why it happens:**
Parser extension hooks are internal DuckDB C++ API, not part of the stable C API. They are accessed via the statically-linked amalgamation, which means the extension is tightly coupled to the exact DuckDB version's internal class layout.

**Consequences:**
- The extension compiles but `CREATE SEMANTIC VIEW` is not recognized (hook not called)
- The extension fails to compile (`shim.cpp` references removed or renamed classes)
- The extension crashes at runtime due to ABI mismatch in parser extension vtable

**Prevention:**
- Before writing any DuckDB 1.5 code, read the 1.5 amalgamation's `ParserExtension` class definition and compare to 1.4.4's. Check if `parse_function` still exists and has the same signature.
- Search the DuckDB 1.5 source for `parser_override` to understand the new mechanism. It may be a C API alternative to the C++ `parse_function` hook -- potentially a path to moving away from the C++ shim entirely.
- If the new parser override is available via the C API, investigate migrating from the C++ `parse_function` hook to the C API `parser_override`. This would eliminate the need for the amalgamation compilation, dramatically reducing build time and binary size.
- If migration is not feasible in v0.5.4, verify the existing hook still works and document the investigation results for a future milestone.
- **Confidence:** MEDIUM. The release notes mention parser override but do not detail whether it replaces or supplements `parse_function`. Investigation required.

**Detection:** `CREATE SEMANTIC VIEW` silently falls through to DuckDB's native parser (error: "Parser Error: syntax error at or near 'SEMANTIC'"). Or: `shim.cpp` fails to compile.

**Phase assignment:** Multi-version DuckDB support phase. Must be investigated as part of the DuckDB 1.5 port.

---

## Minor Pitfalls

### m1: UNIQUE Constraint Syntax Must Be Parsed Without Breaking Existing TABLES Clause

**What goes wrong:**
The current TABLES clause syntax is:
```sql
TABLES (
    o AS orders PRIMARY KEY (id),
    c AS customers PRIMARY KEY (customer_id)
)
```

Adding UNIQUE support extends this to:
```sql
TABLES (
    o AS orders PRIMARY KEY (id) UNIQUE (email),
    c AS customers PRIMARY KEY (customer_id) UNIQUE (customer_id, region)
)
```

The body parser's `parse_tables_clause()` function must handle:
- `PRIMARY KEY` followed by `UNIQUE` (new)
- `PRIMARY KEY` without `UNIQUE` (existing, backward compatible)
- `UNIQUE` without `PRIMARY KEY` (questionable -- should this be allowed?)
- Multiple `UNIQUE` constraints on a single table (e.g., `UNIQUE (email) UNIQUE (phone)`)

If the parser tokenization is not careful, `UNIQUE` could be consumed as part of the PK column list or as a table alias.

**Prevention:**
- After parsing `PRIMARY KEY (...)`, check for the `UNIQUE` keyword. If present, parse `UNIQUE (...)`. If not, continue to the next entry.
- Disallow `UNIQUE` without `PRIMARY KEY`. A table must have a PK for relationship resolution to work. UNIQUE-only tables are an edge case that can be deferred.
- Allow multiple `UNIQUE` clauses per table. Store as `unique_constraints: Vec<Vec<String>>` on `TableRef`.
- Add backward compatibility tests: existing DDL without UNIQUE must parse identically.
- **Confidence:** HIGH. Token-based parsing is an established pattern in `body_parser.rs`.

**Phase assignment:** Cardinality inference phase.

---

### m2: The `description.yml` Extension Name Must Be Lowercase and Match the Binary Name

**What goes wrong:**
The community extension registry requires `extension.name` to be lowercase with only letters, numbers, hyphens, and underscores. The current extension name is `semantic_views` (lowercase with underscore) which is valid. But the name in `description.yml` must EXACTLY match:
- The Makefile's `EXTENSION_NAME=semantic_views`
- The binary output filename (e.g., `semantic_views.duckdb_extension`)
- The `LOAD semantic_views` command users will type

If there is a mismatch (e.g., `description.yml` says `semantic-views` with a hyphen but the binary is `semantic_views` with an underscore), the extension installs but fails to load.

**Prevention:**
- Use `semantic_views` (underscore) consistently everywhere.
- Verify the name in `description.yml`, `Cargo.toml` `[package].name`, `Makefile` `EXTENSION_NAME`, and `build.rs` symbol visibility list all match.
- **Confidence:** HIGH. Simple consistency check.

**Phase assignment:** Registry publishing phase. Validation step in the description.yml creation.

---

### m3: The `duckdb_constraints()` Function May Not Expose Full Constraint Metadata

**What goes wrong:**
If a future version of cardinality inference needs to query ACTUAL database constraints (not just DDL declarations), DuckDB's `duckdb_constraints()` function is the API. However, this function has known limitations: the `constraint_text` column for foreign keys may not include the referenced table or the referenced constraint. Issue #4024 in the DuckDB repo discusses missing FK reference information.

For the v0.5.4 approach (inference from DDL declarations, not from database metadata), this is not a blocker. But if the design later evolves to "infer cardinality by querying the actual database schema," the metadata API may be insufficient.

**Prevention:**
- For v0.5.4, infer cardinality ONLY from DDL declarations (PK/UNIQUE in the TABLES clause). Do not query `duckdb_constraints()` or `information_schema`.
- Document this as a design decision: the semantic view is self-contained. Constraint information comes from the DDL, not from the underlying tables.
- This aligns with Snowflake's approach: "PRIMARY KEY, UNIQUE, and FOREIGN KEY constraints are not enforced and are mainly used for data modeling purposes." The DDL declares intent, not enforcement.
- **Confidence:** HIGH. The DDL-only approach is simpler and more portable.

**Phase assignment:** Cardinality inference phase. Design decision to document.

---

### m4: Documentation Site Content Must Not Duplicate README.md

**What goes wrong:**
The README.md contains DDL syntax reference, worked examples, and a feature overview. The documentation site will contain similar content. If both are maintained separately, they diverge. Users find different information depending on where they look.

**Prevention:**
- Use the documentation site as the canonical source. The README.md should be a brief overview with a link to the documentation site for full reference.
- Alternatively, use Zensical's ability to include markdown files: reference the README.md content from the documentation site to avoid duplication.
- Define a clear boundary: README.md = quick start + installation. Documentation site = full reference + examples + API docs.
- **Confidence:** HIGH. Standard documentation hygiene.

**Phase assignment:** Documentation site phase.

---

### m5: Extension Version String in `description.yml` vs `Cargo.toml`

**What goes wrong:**
The `description.yml` requires an `extension.version` field. The `Cargo.toml` has `version = "0.5.0"`. The DuckDB community extension versioning docs say: "v0.y.z = pre-release/unstable; v1+ = stable API commitment." The extension version in `description.yml` is a freeform string with no enforced scheme.

If the version in `description.yml` does not match `Cargo.toml`, or if the version scheme creates user confusion (is this extension v0.5.4 or v1.0.0?), the registry listing looks unprofessional.

**Prevention:**
- Update `Cargo.toml` version to `0.5.4` when shipping the milestone.
- Set `description.yml` `extension.version` to `0.5.4` (matching Cargo.toml).
- Keep using `0.x.y` versioning until the API is considered stable. The `v0.y.z` convention in the DuckDB ecosystem signals pre-release, which is appropriate.
- **Confidence:** HIGH. Version alignment is a trivial check.

**Phase assignment:** Registry publishing phase. Part of the `description.yml` creation.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Cardinality inference | C1 (stored definition compat), C4 (composite PK edge cases), m1 (parser syntax), m3 (metadata API) | Keep enum + parser backward compat; exact PK/UNIQUE match for inference; DDL-only approach |
| Multi-version DuckDB | C2 (amalgamation build), M1 (branch management), M4 (parser override conflict) | Wait for duckdb-rs 1.5; separate branch; diff amalgamation before coding |
| CE registry publishing | C3 (build system), M2 (cargo vs cmake), m2 (name consistency), m5 (version alignment) | Mirror rusty_quack description.yml; test submission early; verify name consistency |
| Documentation site | M3 (Zensical deployment), m4 (README duplication) | Use official workflow template; set correct base URL; define content boundary |
| All model changes | C1 (serde backward compat) | `#[serde(default)]` on all new fields; keep old enum variants; backward compat tests |

---

## Sources

**Official documentation:**
- [DuckDB: Versioning of Extensions](https://duckdb.org/docs/stable/extensions/versioning_of_extensions) -- C_STRUCT vs C_STRUCT_UNSTABLE ABI types, version compatibility model
- [DuckDB: Community Extension Documentation](https://duckdb.org/community_extensions/documentation) -- description.yml requirements, build types, platform exclusions
- [DuckDB: Community Extension Development](https://duckdb.org/community_extensions/development) -- build system, CI, testing requirements
- [DuckDB: Constraints](https://duckdb.org/docs/stable/sql/constraints) -- PRIMARY KEY, UNIQUE, FOREIGN KEY enforcement and limitations
- [DuckDB: Announcing DuckDB 1.5.0](https://duckdb.org/2026/03/09/announcing-duckdb-150) -- release notes, parser override mention, C API enhancements
- [DuckDB: Announcing DuckDB 1.4.4 LTS](https://duckdb.org/2026/01/26/announcing-duckdb-144) -- LTS release context
- [DuckDB: Release Calendar](https://duckdb.org/release_calendar) -- release cadence

**GitHub sources:**
- [duckdb/community-extensions](https://github.com/duckdb/community-extensions) -- registry repo structure and submission process
- [duckdb/community-extensions#54: Rust extension guidance](https://github.com/duckdb/community-extensions/issues/54) -- pitfalls developing Rust extensions
- [duckdb/extension-template-rs](https://github.com/duckdb/extension-template-rs) -- Rust extension template, Make-based build
- [duckdb/extension-ci-tools](https://github.com/duckdb/extension-ci-tools/) -- reusable CI workflows, multi-version branching
- [duckdb/duckdb#14992: Bump Extension C API to stable](https://github.com/duckdb/duckdb/pull/14992) -- C_STRUCT stabilization, function lifecycle
- [rusty_quack description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) -- reference Rust extension submission (via raw.githubusercontent.com)
- [duckdb/duckdb releases](https://github.com/duckdb/duckdb/releases) -- DuckDB 1.5.0 release details

**Snowflake documentation:**
- [Snowflake: CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- UNIQUE/PK-based cardinality inference, relationship syntax
- [Snowflake: Using SQL commands to create and manage semantic views](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- PK/UNIQUE inference mechanism, relationship types
- [Snowflake: Overview of Constraints](https://docs.snowflake.com/en/sql-reference/constraints-overview) -- constraints as metadata (not enforced), RELY property

**Zensical documentation:**
- [Zensical: Create your site](https://zensical.org/docs/create-your-site/) -- setup process
- [Zensical: Publish your site](https://zensical.org/docs/publish-your-site/) -- GitHub Pages deployment via Actions
- [Zensical: FAQ](https://zensical.org/docs/community/faqs/) -- no gh-deploy command, caching warnings
- [Zensical GitHub repo](https://github.com/zensical/zensical) -- project overview

**Codebase sources:**
- `src/model.rs` -- `Cardinality` enum, `Join.cardinality` field, `TableRef.pk_columns`, serde annotations
- `src/body_parser.rs` -- `parse_cardinality_tokens()`, `CLAUSE_KEYWORDS`, `CLAUSE_ORDER`, `parse_tables_clause()`
- `src/expand.rs` -- `check_fan_traps()`, `card_map` construction from `Join.cardinality`
- `src/graph.rs` -- `RelationshipGraph::from_definition()`, self-reference check, diamond validation
- `build.rs` -- amalgamation compilation, `patch_duckdb_cpp_for_windows()`, symbol visibility
- `Makefile` -- `UNSTABLE_C_API_FLAG`, `ensure_amalgamation`, Make targets
- `.github/workflows/Build.yml` -- `exclude_archs`, `extra_toolchains`, `duckdb_version` pinning
- `Cargo.toml` -- `duckdb = "=1.4.4"`, `cc` optional build dependency
- `TECH-DEBT.md` -- DuckDB version pinning, C_STRUCT_UNSTABLE evaluation, amalgamation compilation
