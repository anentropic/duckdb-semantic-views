# Technology Stack: v0.5.4 Additions

**Project:** DuckDB Semantic Views
**Researched:** 2026-03-15
**Focus:** Multi-version DuckDB support, CE registry publishing, Zensical docs, duckdb-rs upgrade

## Recommended Stack Changes

### 1. DuckDB Version Support (1.4.x LTS + 1.5.x)

| Item | Current (v0.5.3) | v0.5.4 Target | Why |
|------|-------------------|---------------|-----|
| DuckDB | v1.4.4 only | v1.5.0 (main) + v1.4.4 (andium) | 1.5.0 released 2026-03-09; 1.4 LTS supported through 2026-09 |
| extension-ci-tools | `@v1.4.4` | `@v1.5.0` (main) + `@v1.4.4` (andium) | Dual workflow strategy per UPDATING.md |
| .duckdb-version | v1.4.4 | v1.5.0 | Main branch tracks latest stable |

**Confidence:** HIGH -- verified via GitHub API (extension-ci-tools has both `v1.5.0` tag and `v1.5-variegata` branch).

#### DuckDB 1.5.0 "Variegata" -- Key Changes for This Extension

**Released:** 2026-03-09 ([announcement](https://duckdb.org/2026/03/09/announcing-duckdb-150))

Changes relevant to this extension:

1. **New `parser_override_function_t` hook** ([PR #19126](https://github.com/duckdb/duckdb/pull/19126)): DuckDB 1.5 introduces a new parser extension hook that runs *before* the built-in parser (unlike our `parse_function` which runs *after* the built-in parser fails). This is opt-in via `SET allow_parser_override_extension = true` ([PR #19181](https://github.com/duckdb/duckdb/pull/19181)). **Our extension does NOT need this** -- our existing `parse_function` fallback mechanism is preserved and unchanged. The new hook is used by the PEG parser, not by statement-intercepting extensions.

2. **Experimental PEG parser** (disabled by default): A new parser based on Parser Expression Grammars is available opt-in. When disabled (the default), the existing YACC parser + `parse_function` fallback chain works identically to 1.4. **No impact on our extension.**

3. **C API additions**: New C API functions for file system access ([PR #19086](https://github.com/duckdb/duckdb/pull/19086)), config options ([PR #19473](https://github.com/duckdb/duckdb/pull/19473)), and table descriptions ([PR #19334](https://github.com/duckdb/duckdb/pull/19334)). None of these affect our existing API surface.

4. **VARIANT type**: Native support for semi-structured data. Not relevant to semantic views.

5. **ABI unchanged**: `C_STRUCT_UNSTABLE` still works the same way -- binaries are tied to exact DuckDB version. No ABI type changes.

**Bottom line:** DuckDB 1.5.0 is expected to be a **low-risk upgrade** for this extension. The parser hook mechanism we use (`parse_function` fallback) is unchanged. The main work is version-bumping, rebuilding the amalgamation, testing, and potentially fixing any C++ API changes in `shim.cpp` (internal symbol names, include paths, struct layouts).

**Confidence:** MEDIUM -- the `parse_function` fallback mechanism is well-established and unlikely to break, but the amalgamation compilation (`duckdb.cpp`) may have internal structural changes that affect our Windows `build.rs` patches (line number offsets, `#include` reorganization). Manual testing is required.

#### Multi-Version Build Strategy

Per [UPDATING.md](https://github.com/duckdb/community-extensions/blob/main/UPDATING.md), the standard pattern for supporting two DuckDB versions is:

```yaml
# Build.yml -- add a second job for andium (LTS)
duckdb-stable-build:
  uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.5.0
  with:
    duckdb_version: v1.5.0
    ci_tools_version: v1.5.0
    extension_name: semantic_views
    extra_toolchains: 'rust;python3'

duckdb-andium-build:
  uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.4.4
  with:
    duckdb_version: v1.4.4
    ci_tools_version: v1.4.4
    extension_name: semantic_views
    extra_toolchains: 'rust;python3'
```

**Branch strategy options:**
- **Option A (recommended): Single branch, dual CI.** Main branch compiles against both versions. `.duckdb-version` tracks latest (v1.5.0). CI downloads the v1.4.4 amalgamation separately for the andium build. This matches the extension template pattern.
- **Option B: Two branches.** Separate `main` (1.5.x) and `v1.4-andium` branches. More maintenance burden. Only needed if APIs diverge significantly.

Recommend **Option A** because the `parse_function` API is unchanged between 1.4 and 1.5.

### 2. duckdb-rs Crate Upgrade

| Item | Current | v0.5.4 Target | Why |
|------|---------|---------------|-----|
| `duckdb` crate | `=1.4.4` | `=1.10500.0` (for 1.5.0 build) | New versioning scheme |
| `libduckdb-sys` | `=1.4.4` | `=1.10500.0` or remove | Must match duckdb crate |

**CRITICAL: duckdb-rs changed its versioning scheme.** ([Release notes](https://github.com/duckdb/duckdb-rs/releases/tag/v1.10500.0), published 2026-03-11)

Starting with DuckDB 1.5.0, the crate version encodes the DuckDB version in the second semver component: `1.MAJOR_MINOR_PATCH.x`. DuckDB v1.5.0 maps to crate version `1.10500.x`.

**Key changes in duckdb-rs 1.10500.0:**
- Upgraded to Rust edition 2024, Arrow 57, bundled DuckDB v1.5.0
- `rust_decimal::Decimal` support for `FromSql`, `ToSql`, and `Appender`
- `Params` implemented for tuples (up to arity 16)
- Loadable extensions need only a single `duckdb` crate dependency (eliminates `libduckdb-sys` as separate dep)
- Fix: cloned database handles keep original `db` handle alive
- Fix: chrono datetime writes normalized to UTC

**Impact on our Cargo.toml:**
```toml
# For v1.5.0 builds:
[dependencies]
duckdb = { version = "=1.10500.0", default-features = false }
# libduckdb-sys may no longer be needed as a direct dependency
```

**For dual-version support**, the Cargo.toml must track the version being built. Since `C_STRUCT_UNSTABLE` binaries are version-pinned anyway, and CI builds are separate jobs, the simplest approach is:
- Main branch Cargo.toml targets `=1.10500.0` (DuckDB 1.5.0)
- The andium CI job uses extension-ci-tools `@v1.4.4` which handles the amalgamation download independently
- `cargo test` (bundled mode) tests against whichever version Cargo.toml specifies

**Confidence:** HIGH -- verified via [GitHub releases](https://github.com/duckdb/duckdb-rs/releases) and [crates.io](https://crates.io/crates/duckdb).

### 3. Community Extension Registry (description.yml)

| Item | Value | Source |
|------|-------|--------|
| Registry repo | `duckdb/community-extensions` | [GitHub](https://github.com/duckdb/community-extensions) |
| Submission | PR adding `extensions/semantic_views/description.yml` | [Docs](https://duckdb.org/community_extensions/documentation) |
| Reference impl | `rusty_quack` (Rust extension, `build: cargo`) | [description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) |

**Exact description.yml format for this extension:**

```yaml
extension:
  name: semantic_views
  description: Declarative semantic layer for dimensions, metrics, and relationships in DuckDB
  version: 0.5.4
  language: Rust
  build: cargo
  license: MIT
  excluded_platforms: "wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl"
  requires_toolchains: "rust;python3"
  maintainers:
    - <github-username>

repo:
  github: <owner>/duckdb-semantic-views
  andium: <commit-hash-for-v1.4.4-build>
  ref: <commit-hash-for-v1.5.0-build>

docs:
  hello_world: |
    CREATE SEMANTIC VIEW sales_metrics (
      TABLES (
        orders AS o PRIMARY KEY (order_id)
      )
      DIMENSIONS (
        region := o.region,
        order_date := o.order_date
      )
      METRICS (
        total_revenue := SUM(o.amount),
        order_count := COUNT(*)
      )
    );
    FROM semantic_view('sales_metrics',
      dimensions := ['region'],
      metrics := ['total_revenue', 'order_count']
    );
  extended_description: |
    The semantic_views extension implements a declarative semantic layer for DuckDB.
    Define dimensions, metrics, relationships, facts, and hierarchies once with
    `CREATE SEMANTIC VIEW` DDL, then query with any combination using the
    `semantic_view()` table function. The extension handles GROUP BY, JOINs from
    PK/FK declarations, fan trap detection, and typed output automatically.
```

**Field reference (verified from [rusty_quack](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) and [PRQL](https://github.com/duckdb/community-extensions/blob/main/extensions/prql/description.yml)):**

| Field | Required | Notes |
|-------|----------|-------|
| `extension.name` | Yes | Lowercase, only `[a-z0-9_-]` |
| `extension.description` | Yes | Short one-liner |
| `extension.version` | Yes | Freeform string (SemVer recommended) |
| `extension.language` | Yes | `Rust` for us (not `C++`) |
| `extension.build` | Yes | `cargo` for Rust extensions |
| `extension.license` | Yes | SPDX identifier |
| `extension.excluded_platforms` | Recommended | Semicolon-separated platform list |
| `extension.requires_toolchains` | Recommended | `"rust;python3"` for Rust+amalgamation builds |
| `extension.maintainers` | Yes | List of GitHub usernames |
| `repo.github` | Yes | `owner/repo` format |
| `repo.ref` | Yes | Commit hash for latest stable build (v1.5.0) |
| `repo.andium` | Recommended | Commit hash for LTS build (v1.4.x) |
| `docs.hello_world` | Recommended | SQL example for auto-generated docs page |
| `docs.extended_description` | Recommended | Markdown description for docs page |

**Key insight from rusty_quack:** The `andium` field (named after the v1.4 LTS codename) is used instead of `ref_next` for LTS-version pinning. The `ref` field points to the commit for the latest stable (1.5.x), while `andium` points to the commit compatible with 1.4.x LTS. When the next LTS ships, this field name will change to the new codename.

**Confidence:** HIGH -- directly verified from two real Rust extension descriptors in the community-extensions repo.

### 4. Zensical Documentation Site

| Item | Value | Source |
|------|-------|--------|
| Tool | Zensical | [GitHub](https://github.com/zensical/zensical) |
| Version | 0.0.27 (latest as of 2026-03-13) | [Releases](https://github.com/zensical/zensical/releases) |
| Install | `pip install zensical` | [PyPI](https://pypi.org/project/zensical/) |
| Config | `zensical.toml` (TOML, not YAML) | [Docs](https://zensical.org/docs/setup/basics/) |
| Content | Markdown in `docs/` directory | Standard SSG pattern |
| Build | `zensical build --clean` | Outputs to `site/` directory |
| Deploy | GitHub Actions to GitHub Pages | Built-in bootstrap workflow |

**What Zensical is:** A modern static site generator built by the Material for MkDocs team (squidfunk). It replaces MkDocs with a Rust+Python hybrid that understands `mkdocs.yml` config but uses TOML natively. It provides the same Material for MkDocs look and feel with better performance, TOML configuration (no indentation errors), and an upcoming module system for extensibility.

**Project structure for this extension:**

```
docs/
  index.md              # Landing page
  getting-started.md    # Installation + first semantic view
  ddl-reference.md      # Full DDL syntax reference
  query-reference.md    # semantic_view() function reference
  examples/             # Worked examples
  changelog.md          # Version history
zensical.toml           # Site configuration
```

**Minimal zensical.toml:**

```toml
[project]
site_name = "DuckDB Semantic Views"
site_description = "Declarative semantic layer for DuckDB"
site_url = "https://<owner>.github.io/duckdb-semantic-views/"
copyright = "Copyright &copy; 2026 The authors"

[project.theme]
language = "en"
features = [
    "content.code.copy",
    "content.code.annotate",
    "navigation.footer",
    "navigation.indexes",
    "navigation.instant",
    "navigation.sections",
    "navigation.top",
    "search.highlight",
]

[[project.theme.palette]]
scheme = "default"
toggle.icon = "lucide/sun"
toggle.name = "Switch to dark mode"

[[project.theme.palette]]
scheme = "slate"
toggle.icon = "lucide/moon"
toggle.name = "Switch to light mode"
```

**GitHub Pages deployment workflow (from Zensical's official bootstrap template):**

```yaml
# .github/workflows/Docs.yml
name: Documentation
on:
  push:
    branches: [main]
permissions:
  contents: read
  pages: write
  id-token: write
jobs:
  deploy:
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/configure-pages@v5
      - uses: actions/checkout@v5
      - uses: actions/setup-python@v5
        with:
          python-version: 3.x
      - run: pip install zensical
      - run: zensical build --clean
      - uses: actions/upload-pages-artifact@v4
        with:
          path: site
      - uses: actions/deploy-pages@v4
        id: deployment
```

This is the official Zensical bootstrap workflow from the [project template](https://github.com/zensical/zensical/blob/master/python/zensical/bootstrap/.github/workflows/docs.yml). Alternatively, the community [cssnr/zensical-action@v1](https://github.com/cssnr/zensical-action) wraps this into a single step but adds an unnecessary abstraction layer.

**Confidence:** HIGH -- verified from Zensical's own bootstrap template and official documentation.

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Doc site | Zensical | MkDocs Material | Zensical is the successor by the same team; TOML config is cleaner; actively developed |
| Doc site | Zensical | mdBook | Rust ecosystem default, but less polished for user docs; no search, no dark mode toggle, weaker nav |
| Doc deploy | GitHub Pages (built-in) | Cloudflare Pages | Unnecessary complexity for an OSS project already on GitHub |
| CE registry | Direct PR to duckdb/community-extensions | Self-hosted distribution | CE registry is the standard; self-hosting fragments the ecosystem |
| Multi-version | Single branch + dual CI | Two branches | APIs are compatible; two branches doubles maintenance for no benefit |
| Doc deploy action | Official bootstrap workflow | cssnr/zensical-action@v1 | Community action adds unnecessary indirection; official workflow is 8 lines |

## What NOT to Add

| Item | Why Not |
|------|---------|
| `duckdb-extension-framework` crate | Experimental, not used by rusty_quack or our extension; adds unnecessary abstraction over working FFI |
| `quack-rs` crate | Utility crate for Rust extensions; our extension already has a working FFI layer |
| Stable C API migration (`C_STRUCT` from `C_STRUCT_UNSTABLE`) | Our extension uses C++ parser hooks (`ParserExtension`, `DBConfig`) which are not exposed through the stable C API. We MUST use `C_STRUCT_UNSTABLE` until DuckDB stabilizes parser extension hooks in the C API. |
| PEG parser `parser_override_function_t` | Our fallback `parse_function` works correctly; `parser_override_function_t` is designed for full parser replacements (like PEG), not for intercepting specific DDL forms |
| Separate `libduckdb-sys` dep (for 1.5 build) | duckdb-rs 1.10500.0 eliminates the need for separate `libduckdb-sys` in loadable extensions |
| Rust edition 2024 upgrade | duckdb-rs 1.10500.0 uses edition 2024, but our crate can stay on 2021 -- editions are per-crate and interoperate. Upgrade is optional, not required. |

## Version Matrix

| Component | v1.4.x (andium/LTS) | v1.5.x (main) |
|-----------|---------------------|---------------|
| DuckDB | v1.4.4 | v1.5.0 |
| extension-ci-tools workflow tag | `@v1.4.4` | `@v1.5.0` |
| extension-ci-tools branch | `v1.4-andium` | `v1.5-variegata` |
| duckdb-rs crate | `=1.4.4` | `=1.10500.0` |
| libduckdb-sys | `=1.4.4` | (bundled in duckdb crate) |
| Amalgamation source | DuckDB v1.4.4 release | DuckDB v1.5.0 release |
| ABI | C_STRUCT_UNSTABLE | C_STRUCT_UNSTABLE |
| Rust edition | 2021 | 2021 (keep; 2024 optional) |

## CI Workflow Changes Required

### Build.yml

Add a second job for the andium LTS build:

```yaml
duckdb-stable-build:
  name: Build extension binaries (v1.5.0)
  uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.5.0
  with:
    duckdb_version: v1.5.0
    ci_tools_version: v1.5.0
    extension_name: semantic_views
    extra_toolchains: 'rust;python3'
    exclude_archs: 'linux_amd64_musl;linux_arm64_musl;windows_arm64;windows_amd64_mingw;wasm_mvp;wasm_eh;wasm_threads'

duckdb-andium-build:
  name: Build extension binaries (v1.4.4 LTS)
  uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.4.4
  with:
    duckdb_version: v1.4.4
    ci_tools_version: v1.4.4
    extension_name: semantic_views
    extra_toolchains: 'rust;python3'
    exclude_archs: 'linux_amd64_musl;linux_arm64_musl;windows_arm64;windows_amd64_mingw;wasm_mvp;wasm_eh;wasm_threads'
```

### PullRequestCI.yml

Add andium build to PR checks (can be Linux-only for speed):

```yaml
linux-fast-check:
  name: Build and test (v1.5.0, Linux x86_64)
  uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.5.0
  with:
    duckdb_version: v1.5.0
    ci_tools_version: v1.5.0
    # ... same as current but with v1.5.0

linux-andium-check:
  name: Build and test (v1.4.4 LTS, Linux x86_64)
  uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.4.4
  with:
    duckdb_version: v1.4.4
    ci_tools_version: v1.4.4
    # ... same as current
```

### DuckDBVersionMonitor.yml

Needs updating to handle the multi-version world:
- Track latest stable (1.5.x) AND latest LTS patch (1.4.x)
- The `repos/duckdb/duckdb/releases/latest` API now returns v1.5.0
- Need a separate check for LTS releases (filter by `v1.4.*` pattern)

### New: Docs.yml

Add the Zensical documentation deployment workflow (see section 4 above).

## DuckDB Version Monitor Update

The existing `DuckDBVersionMonitor.yml` workflow checks `repos/duckdb/duckdb/releases/latest` which now returns v1.5.0. This workflow needs updating to:
1. Track both the latest stable AND the latest LTS patch (v1.4.x)
2. Not trigger a bump PR when the "latest" changes from 1.4.4 to 1.5.0 (that is an expected migration, not a breakage)
3. Check for LTS patches via `gh api repos/duckdb/duckdb/releases --jq '[.[] | select(.tag_name | startswith("v1.4"))] | first | .tag_name'`

## Cargo.toml Changes

```toml
# Cargo.toml for v0.5.4 (targeting DuckDB 1.5.0 as primary)
[package]
name = "semantic_views"
version = "0.5.4"
edition = "2021"

[dependencies]
duckdb = { version = "=1.10500.0", default-features = false }
# libduckdb-sys dropped as direct dependency (bundled in duckdb 1.10500.0)
serde = { version = "1", features = ["derive"] }
serde_json = "1"
strsim = "0.11"
arbitrary = { version = "1", optional = true, features = ["derive"] }

[build-dependencies]
cc = { version = "1", optional = true }

[dev-dependencies]
proptest = "1.9"
```

**Note:** The andium CI build (v1.4.4) may need a way to override the duckdb crate version. Options:
- **Cargo.toml patch section** in CI (fragile)
- **Conditional compilation** via feature flag (complex)
- **Simplest: let extension-ci-tools handle it** -- the CE registry build system uses its own amalgamation download and does not depend on the duckdb crate version for extension builds. The `duckdb` crate is only used for `cargo test` (bundled mode), not for cdylib builds. The CI extension build uses `--no-default-features --features extension` which uses `loadable-extension` stubs, not bundled DuckDB.

This means the Cargo.toml duckdb version only affects `cargo test`, and CI extension builds work regardless of the pinned crate version. **The andium build should work without Cargo.toml changes** because extension-ci-tools downloads its own DuckDB source.

**Confidence:** MEDIUM -- this needs validation. The `libduckdb-sys` crate version may matter for type definitions even in `loadable-extension` mode. If so, a feature-flag approach would be needed.

## Sources

- [DuckDB 1.5.0 "Variegata" announcement](https://duckdb.org/2026/03/09/announcing-duckdb-150) -- HIGH confidence
- [DuckDB v1.5.0 GitHub release](https://github.com/duckdb/duckdb/releases/tag/v1.5.0) -- HIGH confidence
- [extension-ci-tools releases](https://github.com/duckdb/extension-ci-tools/releases) -- HIGH confidence (verified v1.5.0 tag + v1.5-variegata branch exist via GitHub API)
- [duckdb-rs v1.10500.0 release notes](https://github.com/duckdb/duckdb-rs/releases/tag/v1.10500.0) -- HIGH confidence
- [rusty_quack description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/rusty_quack/description.yml) -- HIGH confidence (canonical Rust CE example, fetched raw)
- [PRQL description.yml](https://github.com/duckdb/community-extensions/blob/main/extensions/prql/description.yml) -- HIGH confidence (Rust toolchain CE example, fetched raw)
- [Community Extensions UPDATING.md](https://github.com/duckdb/community-extensions/blob/main/UPDATING.md) -- HIGH confidence (fetched raw, dual-version strategy documented)
- [Community Extensions documentation](https://duckdb.org/community_extensions/documentation) -- HIGH confidence
- [DuckDB versioning of extensions](https://duckdb.org/docs/stable/extensions/versioning_of_extensions) -- HIGH confidence
- [Zensical GitHub](https://github.com/zensical/zensical) -- HIGH confidence
- [Zensical PyPI](https://pypi.org/project/zensical/) -- HIGH confidence
- [Zensical documentation](https://zensical.org/docs/) -- HIGH confidence
- [Zensical bootstrap workflow](https://github.com/zensical/zensical/blob/master/python/zensical/bootstrap/.github/workflows/docs.yml) -- HIGH confidence (fetched raw)
- [Zensical bootstrap config](https://github.com/zensical/zensical/blob/master/python/zensical/bootstrap/zensical.toml) -- HIGH confidence (fetched raw)
- [cssnr/zensical-action](https://github.com/cssnr/zensical-action) -- MEDIUM confidence (community action, not official)
- [parser_override_function_t PR](https://github.com/duckdb/duckdb/pull/19126) -- HIGH confidence (read PR description)
- [Parser override opt-in PR](https://github.com/duckdb/duckdb/pull/19181) -- HIGH confidence (read PR description)
- [DuckDB release cycle](https://duckdb.org/docs/stable/dev/release_cycle) -- HIGH confidence
- [Guidance on Rust extensions (Issue #54)](https://github.com/duckdb/community-extensions/issues/54) -- MEDIUM confidence
