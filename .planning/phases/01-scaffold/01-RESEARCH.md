# Phase 1: Scaffold - Research

**Researched:** 2026-02-24
**Domain:** DuckDB Rust extension scaffolding — CMake + Cargo build, multi-platform CI, code quality gates, scheduled version monitoring
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**CI platform matrix:**
- Branch model: `main` / `release/vX.Y` / `feature/*` (git flow)
- Feature branches: Linux x86_64 only — fast feedback per PR
- `main` and `release/*` branches: full 5-platform matrix (Linux x86_64, Linux arm64, macOS x86_64, macOS arm64, Windows x86_64)
- All 5 platforms must pass to merge into main or release branches

**Scheduled DuckDB version monitoring:**
- Weekly cron job polls GitHub API for the latest DuckDB release tag
- On new release + build passes: opens an auto-bump PR with the version update
- On new release + build fails: opens a breakage PR with failure log and `@copilot please update the DuckDB version pin and fix any compilation errors`
- Both success and failure scenarios trigger a PR — version stays current automatically

**Developer experience tooling:**
- Task runner: `just` (Justfile) for common commands — `just build`, `just test`, `just lint`, `just setup`
- Test runner: `cargo-nextest` (faster parallel execution, better output) replaces `cargo test`
- Pre-commit hooks via `cargo-husky`: runs `rustfmt` and `clippy` before each commit
- `just setup` downloads the pinned DuckDB binary locally — ensures local tests use the same version as CI

**Code quality gates:**
- `clippy` pedantic lints + `deny(warnings)` — zero tolerance, all warnings are errors
- `rustfmt` enforced on all code
- `cargo-deny` with `deny.toml` covering: disallowed licenses and known security advisories
- Code coverage gated at 80% minimum — CI fails if coverage drops below threshold
- `CHANGELOG.md` maintained from day 1 (Keep a Changelog format)

### Claude's Discretion
- Exact clippy lint suppressions for known-noisy pedantic rules (e.g., `module_name_repetitions`)
- Coverage tool selection (llvm-cov vs tarpaulin)
- Specific `deny.toml` allowed license list
- Justfile command names and structure
- Pre-commit hook implementation details

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| INFRA-01 | Extension scaffold built using `duckdb/extension-template-rs` with CMake + Cargo build system producing correctly-exported C symbols | Template repo structure, Cargo.toml setup, `duckdb_entrypoint_c_api!` macro, `crate-type = "cdylib"` |
| INFRA-02 | Multi-platform CI build matrix covers Linux x86_64/arm64, macOS x86_64/arm64, and Windows x86_64 | `extension-ci-tools/_extension_distribution.yml` reusable workflow, `exclude_archs` parameter, platform job structure |
| INFRA-03 | Scheduled CI job builds against latest DuckDB release; on failure, opens a GitHub PR mentioning @copilot to investigate | GitHub Actions `schedule:` trigger, `gh api` release polling, `peter-evans/create-pull-request@v7`, conditional success/failure PRs |
| INFRA-04 | CI includes a `LOAD` smoke test (not just `cargo test`) to catch DuckDB ABI version mismatches | DuckDB ABI binding requirement, `USE_UNSTABLE_C_API=1`, binary footer version check, SQLLogicTest `LOAD` pattern |
| STYLE-01 | `rustfmt` configured with project-level `rustfmt.toml`; formatting violations fail CI | `rustfmt.toml` config, `cargo fmt --check` in CI, `edition` and `style_edition` settings |
| STYLE-02 | `clippy` with pedantic lints enforced; lint violations fail CI | `[workspace.lints.clippy]` in Cargo.toml, `#![deny(warnings)]`, known-noisy suppressions, `cargo clippy -- -D warnings` |
</phase_requirements>

---

## Summary

This phase bootstraps the Rust DuckDB extension from nothing to a loadable artifact with passing CI. The technical foundation is the official `duckdb/extension-template-rs` template, which uses a Make + Cargo + Python hybrid build system. The template delegates multi-platform CI to a reusable workflow in `duckdb/extension-ci-tools`, which handles the platform matrix (Linux, macOS, Windows, WASM) — extensions simply call the reusable workflow and exclude unwanted targets.

The most critical technical fact: DuckDB extensions are **not ABI-stable across minor versions**. An extension compiled against DuckDB v1.4.4 will not load in DuckDB v1.5.0. This is the fundamental reason the LOAD smoke test in CI (INFRA-04) is essential — `cargo test` never exercises this binding, only a LOAD through the actual DuckDB CLI does. The template currently pins DuckDB v1.4.4 and sets `USE_UNSTABLE_C_API=1`, which means the extension binary is tied to an exact version.

The scheduled version monitoring workflow (INFRA-03) is a custom workflow that must be hand-written. It polls the GitHub API weekly, builds against the latest DuckDB release, and unconditionally opens a PR — a version-bump PR on success, a breakage PR with `@copilot` mention on failure. This is not provided by the template; it is new infrastructure. The `peter-evans/create-pull-request` action is the standard tool for this pattern.

**Primary recommendation:** Clone `duckdb/extension-template-rs` (with submodules), rename the extension, wire in the four custom workflows (PR CI, main CI, scheduled monitor, code quality), configure the Justfile and cargo-husky hooks, and set up clippy via `[workspace.lints.clippy]`. Do not skip the submodule — `extension-ci-tools` is required for the build to work.

## Standard Stack

### Core
| Library/Tool | Version | Purpose | Why Standard |
|-------------|---------|---------|--------------|
| `duckdb/extension-template-rs` | current (pins DuckDB v1.4.4) | Extension scaffold, CMake+Cargo build, CI template | Official DuckDB template; no other Rust scaffold exists |
| `duckdb` crate | 1.4.4 (matches DuckDB pin) | Rust bindings to DuckDB C API | Only official Rust DuckDB binding |
| `libduckdb-sys` | 1.4.4 | Low-level C API FFI | Bundled with `duckdb` crate, provides raw symbols |
| `duckdb/extension-ci-tools` | git submodule (main branch) | Multi-platform build matrix, Makefiles, CI workflows | Required dependency of extension-template-rs |

### Code Quality Tools
| Tool | Version | Purpose | When to Use |
|------|---------|---------|-------------|
| `cargo-nextest` | latest (install via `taiki-e/install-action`) | Faster parallel test runner, better output | Replace `cargo test` everywhere |
| `cargo-husky` | 1.x | Git pre-commit hooks (rustfmt + clippy) | Dev-time quality gate before push |
| `cargo-deny` | latest | License compliance + security advisory checks | CI quality gate |
| `cargo-llvm-cov` | latest | Code coverage (LLVM-based, cross-platform) | CI coverage gate |
| `just` | latest | Task runner (Justfile) | Developer ergonomics |
| `peter-evans/create-pull-request` | v7 | GitHub Action to open PRs from workflows | Scheduled monitor PR creation |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `cargo-llvm-cov` | `tarpaulin` | tarpaulin is Linux-only; llvm-cov works on all 5 target platforms |
| `cargo-husky` | `lefthook`, `pre-commit` | cargo-husky is Rust-native and zero-install (activates on `cargo test`); external tools require separate installation step |
| `just` | `Makefile` | Template already uses Make; `just` adds a clean developer-facing layer with cross-platform behavior |

**Setup:**
```bash
# Install local dev tools
cargo install cargo-nextest --locked
cargo install cargo-deny --locked
cargo install cargo-llvm-cov --locked
cargo install just
# OR use mise/cargo-binstall for faster binary downloads
```

## Architecture Patterns

### Recommended Project Structure

```
semantic-views/                    # renamed from rusty_quack
├── .cargo/
│   └── config.toml                # toolchain pin, target config
├── .github/
│   └── workflows/
│       ├── MainDistributionPipeline.yml  # PR + main branch build
│       ├── DuckDBVersionMonitor.yml      # weekly scheduled monitor
│       └── CodeQuality.yml              # rustfmt, clippy, deny, coverage
├── .cargo-husky/
│   └── hooks/
│       └── pre-commit             # custom hook: fmt + clippy
├── extension-ci-tools/            # git submodule (do NOT vendor manually)
├── src/
│   └── lib.rs                     # extension entry point
├── test/
│   └── sql/
│       └── semantic_views.test    # SQLLogicTest smoke tests (incl. LOAD)
├── Cargo.toml                     # crate-type cdylib, workspace lints
├── Cargo.lock                     # committed (it's a binary crate)
├── Makefile                       # thin wrapper, includes extension-ci-tools makefiles
├── rustfmt.toml                   # formatting config
├── deny.toml                      # license + advisory config
├── Justfile                       # developer commands
├── CHANGELOG.md                   # Keep a Changelog format
└── README.md
```

### Pattern 1: Extension Entry Point (C API Macro)

**What:** The `#[duckdb_entrypoint_c_api()]` attribute macro handles C FFI bridging automatically. It exports the correct symbol name that DuckDB looks for when loading the extension.
**When to use:** Always — this is the only supported entry point pattern for Rust extensions using the C API.

```rust
// Source: github.com/duckdb/extension-template-rs src/lib.rs
use duckdb::{Connection, Result};
use duckdb_loadable_macros::duckdb_entrypoint_c_api;
use std::error::Error;

#[duckdb_entrypoint_c_api()]
pub unsafe fn extension_entrypoint(con: Connection) -> Result<(), Box<dyn Error>> {
    // Register table functions, scalar functions, etc.
    // con.register_table_function::<MyVTab>("function_name")?;
    Ok(())
}
```

### Pattern 2: Cargo.toml — cdylib + Workspace Lints

**What:** The crate must be a C-compatible dynamic library. Workspace lints configure pedantic clippy at one place and are inherited by all crates.
**When to use:** Set this up at scaffold time; changing crate-type later breaks the build.

```toml
# Source: extension-template-rs Cargo.toml + coreyja.com/til/clippy-pedantic-workspace
[lib]
crate-type = ["cdylib"]

[profile.release]
lto = true
strip = true

[workspace.lints.clippy]
pedantic = "deny"
# Suppress known-noisy pedantic rules
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"

[lints]
workspace = true
```

### Pattern 3: LOAD Smoke Test in SQLLogicTest format

**What:** A `.test` file in `test/sql/` that calls `LOAD` on the built extension binary path. This runs through the actual DuckDB loading mechanism, including ABI version check, binary footer validation, and symbol resolution — none of which `cargo test` exercises.
**When to use:** Mandatory for INFRA-04. Runs as part of `make test_debug` / `make test_release`.

```sql
# Source: DuckDB SQLLogicTest format (test/sql/semantic_views.test)
# This catches ABI mismatches that cargo test cannot detect
require semantic_views

statement ok
SELECT 1;
```

The `require semantic_views` directive causes the test runner to `LOAD` the extension. If the ABI doesn't match the pinned DuckDB CLI binary, the test fails with an error code — exactly what INFRA-04 requires.

### Pattern 4: Scheduled Version Monitor Workflow

**What:** A GitHub Actions workflow on `schedule: cron` that fetches the latest DuckDB release via `gh api`, compares to the pinned version, triggers a build, and opens a PR via `peter-evans/create-pull-request`.
**When to use:** INFRA-03 requirement.

```yaml
# Source: GitHub Actions docs + peter-evans/create-pull-request docs
name: DuckDB Version Monitor
on:
  schedule:
    - cron: '0 9 * * 1'   # weekly, Monday 09:00 UTC
  workflow_dispatch:         # allow manual trigger

jobs:
  check-and-update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - name: Get latest DuckDB release
        id: latest
        run: |
          LATEST=$(gh api repos/duckdb/duckdb/releases/latest --jq '.tag_name')
          CURRENT=$(grep 'TARGET_DUCKDB_VERSION' Makefile | cut -d= -f2)
          echo "latest=$LATEST" >> $GITHUB_OUTPUT
          echo "current=$CURRENT" >> $GITHUB_OUTPUT
          echo "is_new=$([ "$LATEST" != "$CURRENT" ] && echo true || echo false)" >> $GITHUB_OUTPUT
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}

      - name: Update version pin (if new release)
        if: steps.latest.outputs.is_new == 'true'
        run: |
          sed -i "s/TARGET_DUCKDB_VERSION=.*/TARGET_DUCKDB_VERSION=${{ steps.latest.outputs.latest }}/" Makefile
          sed -i 's/duckdb_version: .*/duckdb_version: ${{ steps.latest.outputs.latest }}/' .github/workflows/MainDistributionPipeline.yml

      - name: Build and test (if new version)
        if: steps.latest.outputs.is_new == 'true'
        id: build
        run: make configure && make test_release
        continue-on-error: true

      - name: Open success PR (build passed)
        if: steps.latest.outputs.is_new == 'true' && steps.build.outcome == 'success'
        uses: peter-evans/create-pull-request@v7
        with:
          title: "chore: bump DuckDB to ${{ steps.latest.outputs.latest }}"
          body: "Automated version bump to ${{ steps.latest.outputs.latest }}. Build passed."
          branch: "chore/duckdb-bump-${{ steps.latest.outputs.latest }}"
          commit-message: "chore: bump DuckDB pin to ${{ steps.latest.outputs.latest }}"

      - name: Open breakage PR (build failed)
        if: steps.latest.outputs.is_new == 'true' && steps.build.outcome == 'failure'
        uses: peter-evans/create-pull-request@v7
        with:
          title: "fix: DuckDB ${{ steps.latest.outputs.latest }} broke the build"
          body: |
            Build against DuckDB ${{ steps.latest.outputs.latest }} failed.

            @copilot please update the DuckDB version pin and fix any compilation errors.

            Build log: ${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}
          branch: "fix/duckdb-breakage-${{ steps.latest.outputs.latest }}"
          commit-message: "chore: attempt bump to DuckDB ${{ steps.latest.outputs.latest }}"
```

### Pattern 5: PR-Scoped CI (Linux x86_64 Only)

**What:** A separate lighter workflow triggers on pull requests and runs only Linux x86_64 to give fast feedback, while the full 5-platform matrix runs only on pushes to `main` and `release/*`.
**When to use:** Feature branch workflow to keep PR turnaround fast.

```yaml
# Separate workflow: .github/workflows/PullRequestCI.yml
name: Pull Request CI
on:
  pull_request:
    branches: [main, 'release/*']

jobs:
  linux-fast-check:
    uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@main
    with:
      duckdb_version: v1.4.4
      extension_name: semantic_views
      extra_toolchains: rust;python3
      # Only build Linux x86_64 — exclude everything else
      exclude_archs: 'wasm_mvp;wasm_eh;wasm_threads;linux_amd64_musl;linux_arm64;osx_amd64;osx_arm64;windows_amd64'
```

### Pattern 6: Justfile Structure

**What:** A `just` Justfile wraps Make targets and adds developer-friendly commands.

```makefile
# Justfile
# Source: just.systems/man/en + project conventions

# Show available commands
default:
    @just --list

# Set up complete local dev environment (one-time)
setup:
    @echo "Installing dev tools..."
    cargo install cargo-nextest --locked
    cargo install cargo-deny --locked
    cargo install cargo-llvm-cov --locked
    make configure

# Build debug extension
build:
    make debug

# Build release extension
build-release:
    make release

# Run tests (uses cargo-nextest via make)
test:
    make test_debug

# Run all lints
lint:
    cargo fmt --check
    cargo clippy -- -D warnings
    cargo deny check

# Format code
fmt:
    cargo fmt

# Check code coverage
coverage:
    cargo llvm-cov --lcov --output-path lcov.info
    cargo llvm-cov report --fail-under-lines 80

# Clean build artifacts
clean:
    make clean
```

### Pattern 7: cargo-husky Pre-commit Hook

**What:** cargo-husky installs a Git pre-commit hook automatically when `cargo test` is run. Use the `user-hooks` feature with a custom script to run `rustfmt --check` and `clippy`.

```toml
# In Cargo.toml [dev-dependencies]
[dev-dependencies.cargo-husky]
version = "1"
default-features = false
features = ["user-hooks"]
```

```bash
# .cargo-husky/hooks/pre-commit
#!/bin/sh
set -e
cargo fmt --check
cargo clippy -- -D warnings
```

The hook file must be executable (`chmod +x .cargo-husky/hooks/pre-commit`). Hook installs automatically on first `cargo test` run.

### Anti-Patterns to Avoid

- **Vendoring `extension-ci-tools` manually:** It must be a git submodule. The Makefiles `include` paths are relative to the submodule. Copying files breaks on updates.
- **Setting `crate-type = ["rlib"]`:** Extension must be `"cdylib"` to produce a shared library that DuckDB can load. Using `rlib` produces a Rust-only artifact.
- **Testing only with `cargo test`:** cargo test never exercises the DuckDB loading mechanism. ABI mismatches are completely invisible to cargo test.
- **Using `#![deny(warnings)]` in `lib.rs`:** Put deny(warnings) in CI command (`cargo clippy -- -D warnings`) not in source code — source-level deny(warnings) breaks on stable/nightly Rust toolchain differences.
- **Pinning `extension-ci-tools` to a branch instead of a commit:** The `@main` reference in CI is what the template uses, but for production stability consider pinning to a tagged release or commit SHA.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Multi-platform extension build matrix | Custom Docker files + matrix config | `extension-ci-tools/_extension_distribution.yml` reusable workflow | Handles cross-compilation, extension binary footer injection, artifact naming — hundreds of lines of tested infrastructure |
| Extension binary footer (version metadata) | Custom binary post-processing | `make build_extension_with_metadata_debug/release` (from ci-tools makefiles) | DuckDB refuses to load extensions without the correct binary footer; the format is internal and subject to change |
| C API FFI bridge | Manual `extern "C"` symbol exports | `#[duckdb_entrypoint_c_api()]` macro | Must export correct symbol name, handle panics across FFI boundary, match DuckDB's expected function signature |
| Version comparison and PR creation | Custom shell script + curl | `gh api` + `peter-evans/create-pull-request@v7` | Handle idempotent PR creation (update existing PR vs create new), git authentication, branch management |

**Key insight:** The extension-ci-tools infrastructure exists specifically because the "simple" approach of just building a `.so` file is not sufficient — DuckDB validates a binary footer with platform and version metadata on every LOAD. Hand-rolling this means reverse-engineering DuckDB internals that change with each release.

## Common Pitfalls

### Pitfall 1: Forgetting --recurse-submodules on Clone
**What goes wrong:** Build fails with `Makefile:XX: extension-ci-tools/makefiles/...: No such file or directory` immediately on `make configure`.
**Why it happens:** `extension-ci-tools` is a git submodule. A plain `git clone` without `--recurse-submodules` leaves the directory empty.
**How to avoid:** Clone with `git clone --recurse-submodules` or run `git submodule update --init --recursive` after cloning. Add a check in `just setup`.
**Warning signs:** Empty `extension-ci-tools/` directory; Makefile include errors.

### Pitfall 2: DuckDB ABI Mismatch — Silent Cargo Test Success
**What goes wrong:** All `cargo test` tests pass, but `LOAD 'semantic_views'` in a DuckDB shell fails with `Error: Extension "semantic_views" could not be loaded` or a version mismatch error.
**Why it happens:** `cargo test` compiles and runs Rust code but never invokes DuckDB's extension loader. The ABI check happens only when DuckDB calls `dlopen()` on the built `.duckdb_extension` file.
**How to avoid:** Always run `make test_debug` (which calls the SQLLogicTest runner with `require semantic_views`) in CI. Never accept green `cargo test` as proof the extension loads.
**Warning signs:** CI passes `cargo test` but fails `make test`.

### Pitfall 3: USE_UNSTABLE_C_API Version Lock
**What goes wrong:** The extension only works with the exact DuckDB minor version it was compiled against. It silently fails to load on any other version, including patch releases.
**Why it happens:** `duckdb-rs` currently requires `USE_UNSTABLE_C_API=1` because it uses unstable C API functionality. This means the extension binary is version-locked with no forward compatibility.
**How to avoid:** Accept this constraint for v0.1. The scheduled monitor workflow (INFRA-03) is the mitigation — it detects new releases and triggers a re-build PR before users hit the problem. Document this in MAINTAINER.md.
**Warning signs:** Any time a new DuckDB minor is released; the monitor workflow opens a breakage PR.

### Pitfall 4: clippy `deny(warnings)` in Source Code
**What goes wrong:** Build breaks on nightly Rust or when Rust adds new compiler warnings in a toolchain update, because `#![deny(warnings)]` in source code treats ALL warnings (including new ones) as errors.
**Why it happens:** Compiler warnings are not stable across Rust versions. A warning added in Rust 1.87 will break a codebase that was clean on 1.86.
**How to avoid:** Pass `-D warnings` only in the CI command: `cargo clippy -- -D warnings`. Do not put `#![deny(warnings)]` in `lib.rs` or `main.rs`. The `[workspace.lints.clippy]` approach (deny pedantic) is fine because it only controls clippy lints, not rustc warnings.
**Warning signs:** Build fails on toolchain update with "warning promoted to error" from a lint you didn't write.

### Pitfall 5: rustfmt.toml Missing `edition`
**What goes wrong:** `cargo fmt` and `rustfmt` format code differently — `cargo fmt` infers the edition from `Cargo.toml`, but standalone `rustfmt` defaults to 2015 edition. This causes CI to fail even when local formatting looks correct.
**Why it happens:** rustfmt defaults `edition = "2015"` unless configured. Cargo overrides this automatically, but CI often calls `cargo fmt --check` which is consistent — the issue appears when developers run `rustfmt` directly.
**How to avoid:** Set `edition = "2021"` (or 2024 when stable) explicitly in `rustfmt.toml`.
**Warning signs:** `cargo fmt --check` passes locally but fails in CI; formatting differences between direct `rustfmt` and `cargo fmt`.

### Pitfall 6: cargo-deny License Allowlist Too Restrictive
**What goes wrong:** `cargo deny check` fails because a transitive dependency uses a license not in the allowlist, blocking the build.
**Why it happens:** DuckDB's dependency tree includes crates with various licenses (MIT, Apache-2.0, BSD-3, ISC, OpenSSL). A too-narrow allowlist blocks common permissive licenses.
**How to avoid:** Run `cargo deny init` to generate a starting `deny.toml`, then run `cargo deny check licenses` and review failures to build the allowlist empirically. At minimum include: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-DFS-2016.
**Warning signs:** First `cargo deny check` run fails on multiple licenses simultaneously.

### Pitfall 7: cargo-husky Hooks Not Installing
**What goes wrong:** Pre-commit hooks don't run because cargo-husky installs on `cargo test`, not on `cargo build`. New contributors who haven't run the tests don't get hooks.
**Why it happens:** cargo-husky's build script only activates during test compilation. If a contributor never runs `cargo test` locally, hooks are never set up.
**How to avoid:** Include `cargo test` (or `cargo nextest run`) in `just setup` explicitly, so running setup installs hooks as a side effect.
**Warning signs:** Code reaches PR review with formatting or clippy violations.

## Code Examples

Verified patterns from official sources:

### Minimal Working Extension Entry Point
```rust
// Source: github.com/duckdb/extension-template-rs/blob/main/src/lib.rs
use duckdb::{Connection, Result};
use duckdb_loadable_macros::duckdb_entrypoint_c_api;
use std::error::Error;

#[duckdb_entrypoint_c_api()]
pub unsafe fn extension_entrypoint(con: Connection) -> Result<(), Box<dyn Error>> {
    // For Phase 1: just load cleanly. No functions registered yet.
    // Phase 2+ will register table functions here.
    Ok(())
}
```

### Cargo.toml — Complete Scaffold Configuration
```toml
# Source: extension-template-rs + coreyja.com/til/clippy-pedantic-workspace
[package]
name = "semantic_views"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
duckdb = { version = "1.4.4", features = ["loadable-extension"] }
libduckdb-sys = "1.4.4"
duckdb_loadable_macros = "0.1"   # provides duckdb_entrypoint_c_api

[dev-dependencies.cargo-husky]
version = "1"
default-features = false
features = ["user-hooks"]

[profile.release]
lto = true
strip = true

[workspace.lints.clippy]
pedantic = "deny"
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"

[lints]
workspace = true
```

### rustfmt.toml — Minimal Correct Configuration
```toml
# Source: github.com/rust-lang/rustfmt Configurations.md
edition = "2021"
# style_edition defaults to match edition when using cargo fmt
max_width = 100
```

### deny.toml — Starting Allowlist
```toml
# Source: cargo deny init template + common permissive licenses
[licenses]
allow = [
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-DFS-2016",
    "OpenSSL",
]
confidence-threshold = 0.8

[advisories]
db-path = "~/.cargo/advisory-db"
db-urls = ["https://github.com/rustsec/advisory-db"]
vulnerability = "deny"
unmaintained = "warn"
yanked = "warn"

[bans]
multiple-versions = "warn"
```

### SQLLogicTest Smoke Test (INFRA-04)
```sql
-- Source: DuckDB SQLLogicTest format docs
-- File: test/sql/semantic_views.test
-- This test exercises the actual DuckDB LOAD mechanism, catching ABI mismatches

require semantic_views

# Basic smoke: extension loaded and DuckDB is functional
query I
SELECT 42;
----
42
```

### GitHub Actions — Code Quality Workflow
```yaml
# Source: GitHub Actions docs + cargo tool documentation
name: Code Quality
on:
  push:
    branches: [main, 'release/*']
  pull_request:

jobs:
  quality:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Check formatting
        run: cargo fmt --check

      - name: Clippy (pedantic, deny warnings)
        run: cargo clippy -- -D warnings

      - name: cargo-deny
        uses: EmbarkStudios/cargo-deny-action@v2

      - name: Install cargo-llvm-cov
        uses: taiki-e/install-action@cargo-llvm-cov

      - name: Install cargo-nextest
        uses: taiki-e/install-action@nextest

      - name: Coverage check
        run: cargo llvm-cov nextest --fail-under-lines 80
```

### GitHub Actions — Referencing extension-ci-tools for Full Platform Matrix
```yaml
# Source: github.com/duckdb/extension-template-rs .github/workflows/MainDistributionPipeline.yml
name: Main Extension Distribution Pipeline
on:
  push:
    branches: [main, 'release/*']
  workflow_dispatch:

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}-${{ github.head_ref || '' }}-${{ github.base_ref || '' }}-${{ github.ref != 'refs/heads/main' || github.sha }}
  cancel-in-progress: true

jobs:
  duckdb-stable-build:
    name: Build extension binaries
    uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@main
    with:
      duckdb_version: v1.4.4
      ci_tools_version: main
      extension_name: semantic_views
      extra_toolchains: rust;python3
      # Exclude WASM and musl targets; keep Linux x86_64, Linux arm64, macOS x86_64, macOS arm64, Windows x86_64
      exclude_archs: 'wasm_mvp;wasm_eh;wasm_threads;linux_amd64_musl'
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| C++ extension template only | Rust template (`extension-template-rs`) available | 2024 | Can write extensions entirely in Rust; no C++ required |
| Manual cross-compilation setup | `extension-ci-tools` reusable workflow | 2023-2024 | Platform matrix managed centrally; extensions just declare exclusions |
| `cargo test` as primary validation | SQLLogicTest via Make + `require` directive | Ongoing | Actual DuckDB LOAD exercised in CI |
| `cargo test` sequential execution | `cargo-nextest` parallel execution | 2022-present | Up to 3x faster, better output |
| Workspace-level lint config via `lib.rs` | `[workspace.lints.clippy]` in Cargo.toml | Rust 1.74 (2023) | Clippy config is now first-class in Cargo, not scattered through source |

**Deprecated/outdated:**
- Direct `.cargo/config.toml` lint configuration: Replaced by `[workspace.lints]` since Rust 1.74. Use Cargo.toml approach.
- `cargo test --jobs` for parallelism: cargo-nextest handles this more ergonomically with better output.

## Open Questions

1. **Exact `exclude_archs` value for the PR CI (feature branch)**
   - What we know: Feature branches should build Linux x86_64 only. The `exclude_archs` parameter accepts semicolon-separated architecture names.
   - What's unclear: The exact architecture string identifiers for all non-Linux-x86_64 targets (need to verify names from `extension-ci-tools` distribution matrix config).
   - Recommendation: Check `extension-ci-tools/config/distribution_matrix.json` (in the submodule) for exact architecture names after cloning with submodules. Safe default: start with template's `exclude_archs` and add others.

2. **`duckdb_loadable_macros` crate name and version**
   - What we know: The template uses a `#[duckdb_entrypoint_c_api()]` macro. The `duckdb` crate is `1.4.4`.
   - What's unclear: Whether `duckdb_loadable_macros` is a separate crate or re-exported from `duckdb`. Need to verify in template's Cargo.lock.
   - Recommendation: Inspect the template's `Cargo.lock` and `Cargo.toml` precisely before writing the project's `Cargo.toml`. The macro may be part of the `duckdb` crate itself.

3. **Code coverage for cdylib crates**
   - What we know: `cargo-llvm-cov` is cross-platform and accurate. Coverage for `cdylib` crates requires `--lib` flag or similar.
   - What's unclear: Whether `cargo llvm-cov nextest` works seamlessly with `cdylib` crate types, or whether a separate test harness configuration is needed.
   - Recommendation: Verify during implementation. If coverage on the cdylib doesn't work directly, unit tests can be in a separate `[[test]]` target or `pub(crate)` module that compiles as `rlib`.

## Sources

### Primary (HIGH confidence)
- `github.com/duckdb/extension-template-rs` — repository structure, Makefile, Cargo.toml, src/lib.rs, CI workflow verified by WebFetch
- `github.com/duckdb/extension-ci-tools` — reusable workflow platform matrix (linux, macos, windows, wasm jobs), architecture list verified by WebFetch
- `duckdb.org/docs/stable/extensions/versioning_of_extensions` — ABI version binding requirement verified by WebFetch
- `coreyja.com/til/clippy-pedantic-workspace` — `[workspace.lints.clippy]` syntax and `[lints] workspace = true` pattern verified by WebFetch
- `github.com/peter-evans/create-pull-request` — PR creation action inputs, `body`, `branch`, `commit-message` parameters verified by WebFetch

### Secondary (MEDIUM confidence)
- `nexte.st` — cargo-nextest up to 3x faster, per-test-binary parallel execution, separate process model; verified by official nextest docs via WebSearch
- `github.com/rhysd/cargo-husky` — installation mechanism (activates on `cargo test`), `user-hooks` feature, custom hook directory `.cargo-husky/hooks/`; verified by WebFetch
- `cargo-deny` setup (`cargo deny init`, `deny.toml` sections) — verified via multiple WebSearch results referencing official cargo-deny docs
- `github.com/rust-lang/rustfmt Configurations.md` — `edition` and `style_edition` config options; WebSearch confirmed official source
- GitHub Actions `schedule: cron` syntax and `peter-evans/create-pull-request` for automated PRs — verified by WebFetch

### Tertiary (LOW confidence)
- `duckdb_loadable_macros` as separate crate — inferred from template source code description; actual crate name/version should be verified from template `Cargo.lock` directly
- `cargo llvm-cov nextest` compatibility with cdylib — expected to work based on llvm-cov docs, but untested with this specific crate type

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all tools verified via official repos and docs
- Architecture patterns: HIGH — entry point pattern verified from template source; CI workflows verified from template YAML
- Pitfalls: HIGH — ABI mismatch risk verified by official DuckDB docs; others from direct observation of template constraints
- Scheduled monitor workflow: MEDIUM — pattern is correct but specific YAML is composed from verified components (gh api, peter-evans action); exact workflow needs testing

**Research date:** 2026-02-24
**Valid until:** 2026-04-24 (DuckDB moves fast; re-verify if DuckDB releases a new minor before planning completes)
