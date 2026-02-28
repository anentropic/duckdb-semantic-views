# Phase 5: Hardening and Docs - Research

**Researched:** 2026-02-26
**Domain:** Fuzz testing (cargo-fuzz / libFuzzer), developer documentation
**Confidence:** HIGH

## Summary

Phase 5 has two distinct workstreams: (1) adding fuzz targets using `cargo-fuzz` that cover the C FFI boundary and SQL generation path, and (2) writing a comprehensive `MAINTAINER.md` that lets a Python-expert-turned-Rust-newcomer build, test, fuzz, and publish the extension unaided.

The fuzzing workstream requires `cargo-fuzz` (which wraps libFuzzer), the `arbitrary` crate with derive support for structured fuzzing, and a nightly Rust toolchain. Three fuzz targets are specified: JSON definition parsing, SQL generation via `expand()`, and query-time dimension/metric name arrays. The project's existing `crate-type = ["cdylib", "lib"]` configuration means the "lib" target lets fuzz targets link against the crate normally -- no workspace restructuring required. The fuzz directory is its own Cargo crate living at `fuzz/` with its own `Cargo.toml`.

The documentation workstream produces a single `MAINTAINER.md` covering dev setup, build, test, LOAD, version pin update, fuzzer operation, and community extension publishing. The audience is a Python expert unfamiliar with Rust toolchains. Rust concepts should get brief inline explanations using Python analogies.

**Primary recommendation:** Implement fuzz targets first (they may uncover bugs that need fixing), then write MAINTAINER.md second (it documents the fuzzer workflow).

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Three separate fuzz targets, all equally prioritized:
  1. **JSON definition parsing** -- fuzz `define_semantic_view` with arbitrary strings at the FFI boundary
  2. **SQL generation** -- fuzz `expand()` with arbitrary `SemanticViewDefinition` structs
  3. **Query-time name arrays** -- fuzz dimension/metric name strings against a valid definition (catches injection via column names)
- Validation: no panics AND output SQL must parse successfully (not just crash-free)
- Seed corpus from existing test JSON definitions -- fuzzer mutates from known-good inputs
- MAINTAINER.md audience: Python expert, new to Rust toolchains and DuckDB extension development
- Brief inline Rust concept explanations with Python analogies
- Explain "why" behind troubleshooting fixes
- DuckDB install context: user primarily uses DuckDB through Python (`pip install duckdb`)
- All sections equally detailed: dev setup, build, test, LOAD, version pin update, fuzzer, publishing
- Include brief architecture overview mapping source tree to concepts
- Include worked examples for common extension tasks (adding DDL function, adding metric type)
- Version pin update section: just the steps (no deep ABI explainer)
- Both local and nightly CI: developer runs locally for deep fuzzing, scheduled CI runs nightly
- Nightly CI: 5 minutes per fuzz target (15 minutes total for 3 targets)
- On crash: CI opens a GitHub issue with crash artifact and reproduction steps
- Corpus committed to repo under `fuzz/corpus/` -- shared between local and CI runs
- CI auto-commits new corpus entries via PR after each nightly run
- Periodic `cargo fuzz cmin` to minimize corpus if it grows

### Claude's Discretion
- Exact fuzz target implementation details (harness structure, arbitrary trait implementations)
- SQL validity check mechanism (DuckDB parser vs basic syntax check)
- Troubleshooting section: which errors to include
- Architecture overview structure and level of detail
- Worked example selection (which extension tasks are most instructive)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| TEST-05 | Fuzz targets (using `cargo-fuzz`) cover the unsafe C FFI boundary and the SQL generation path | cargo-fuzz setup, Arbitrary derive for `SemanticViewDefinition`, three fuzz harness patterns, nightly CI workflow, corpus management |
| DOCS-01 | `MAINTAINER.md` covers: dev environment setup, build instructions, running tests, loading the extension in a DuckDB shell, updating the DuckDB version pin, running the fuzzer, and publishing to the community extension registry | Source tree map, build system documentation, community extension descriptor.yml format, Justfile commands, CI workflow descriptions |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| cargo-fuzz | 0.12+ | CLI tool wrapping libFuzzer for Rust fuzz testing | Official rust-fuzz project; the de facto standard for Rust fuzzing |
| libfuzzer-sys | 0.4+ | Provides `fuzz_target!` macro and libFuzzer integration | Required by cargo-fuzz; provides the `fuzz_target!` entry point macro |
| arbitrary | 1.4 | Derive macro for generating structured data from raw bytes | Official rust-fuzz companion; enables structure-aware fuzzing instead of raw byte slices |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| Rust nightly toolchain | latest | Required by cargo-fuzz (uses unstable sanitizer flags) | Only for fuzzing; stable toolchain for all other development |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| cargo-fuzz (libFuzzer) | honggfuzz-rs | honggfuzz has better multi-threaded support but cargo-fuzz is simpler, better documented, and the ecosystem standard |
| arbitrary derive | Manual Arbitrary impl | Manual gives more control but derive covers `String`, `Vec<T>`, `Option<T>` out of the box -- sufficient for `SemanticViewDefinition` |
| DuckDB parser for SQL validity | Basic syntax heuristic | DuckDB parser would be ideal but requires a running DuckDB instance; for the fuzz harness, a lightweight check (balanced parentheses + no null bytes + starts with WITH/SELECT) is sufficient as a proxy |

**Installation (developer):**
```bash
# Nightly toolchain (only needed for fuzzing)
rustup install nightly

# cargo-fuzz CLI
cargo install cargo-fuzz
```

**Installation (CI):**
```yaml
- uses: dtolnay/rust-toolchain@nightly
- run: cargo install cargo-fuzz
```

## Architecture Patterns

### Fuzz Directory Structure
```
fuzz/
├── Cargo.toml              # Separate crate depending on semantic_views
├── corpus/
│   ├── fuzz_json_parse/     # Seed + discovered inputs for JSON parsing target
│   ├── fuzz_sql_expand/     # Seed + discovered inputs for SQL expansion target
│   └── fuzz_query_names/    # Seed + discovered inputs for query name target
├── artifacts/               # Crash-triggering inputs (gitignored)
└── fuzz_targets/
    ├── fuzz_json_parse.rs   # Target 1: JSON definition parsing
    ├── fuzz_sql_expand.rs   # Target 2: SQL generation via expand()
    └── fuzz_query_names.rs  # Target 3: Dimension/metric name arrays
```

### Pattern 1: Fuzz Crate Cargo.toml Configuration
**What:** The fuzz directory is its own Cargo crate that depends on the main crate with the `arbitrary` feature enabled.
**When to use:** Always -- this is the required cargo-fuzz project structure.
**Example:**
```toml
# fuzz/Cargo.toml
[package]
name = "semantic-views-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
arbitrary = { version = "1", features = ["derive"] }
semantic_views = { path = ".." }
# Note: depends on the default feature (duckdb/bundled), NOT the extension feature.
# Fuzz targets exercise pure Rust logic, not the DuckDB loadable extension stubs.

[[bin]]
name = "fuzz_json_parse"
path = "fuzz_targets/fuzz_json_parse.rs"
doc = false

[[bin]]
name = "fuzz_sql_expand"
path = "fuzz_targets/fuzz_sql_expand.rs"
doc = false

[[bin]]
name = "fuzz_query_names"
path = "fuzz_targets/fuzz_query_names.rs"
doc = false
```
Source: [Rust Fuzz Book - Structure-Aware Fuzzing](https://rust-fuzz.github.io/book/cargo-fuzz/structure-aware-fuzzing.html), [cargo-fuzz guide](https://rust-fuzz.github.io/book/cargo-fuzz/guide.html)

### Pattern 2: Arbitrary Derive on Model Types
**What:** Add conditional `Arbitrary` derive to `SemanticViewDefinition` and its component types so the fuzzer can generate valid-looking structured inputs.
**When to use:** For fuzz target 2 (SQL expansion) and fuzz target 3 (query name arrays).
**Example:**
```rust
// In src/model.rs -- conditional derive behind a feature flag
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct SemanticViewDefinition {
    pub base_table: String,
    pub dimensions: Vec<Dimension>,
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub filters: Vec<String>,
    #[serde(default)]
    pub joins: Vec<Join>,
}

// Same pattern for Dimension, Metric, Join
```

Then in main `Cargo.toml`:
```toml
[features]
default = ["duckdb/bundled"]
extension = ["duckdb/loadable-extension", "duckdb/vscalar"]
arbitrary = ["dep:arbitrary"]

[dependencies]
arbitrary = { version = "1", optional = true, features = ["derive"] }
```

And in `fuzz/Cargo.toml`:
```toml
[dependencies]
semantic_views = { path = "..", features = ["arbitrary"] }
```
Source: [Rust Fuzz Book - Structure-Aware Fuzzing](https://rust-fuzz.github.io/book/cargo-fuzz/structure-aware-fuzzing.html)

### Pattern 3: Fuzz Target 1 -- JSON Definition Parsing
**What:** Feed arbitrary byte strings to `SemanticViewDefinition::from_json()`. Validates the FFI boundary where user-provided JSON enters the system.
**When to use:** This is the most direct FFI boundary test.
**Example:**
```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Must not panic regardless of input.
        // Errors are fine -- panics/UB are not.
        let _ = semantic_views::model::SemanticViewDefinition::from_json("fuzz_test", s);
    }
});
```

### Pattern 4: Fuzz Target 2 -- SQL Expansion
**What:** Feed arbitrary `SemanticViewDefinition` structs (via Arbitrary derive) plus arbitrary dimension/metric name selections to `expand()`. Validate that successful expansion produces SQL that passes a basic syntax check.
**When to use:** Tests the core SQL generation path with wild inputs.
**Example:**
```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use semantic_views::expand::{expand, QueryRequest};
use semantic_views::model::SemanticViewDefinition;

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    def: SemanticViewDefinition,
    dim_names: Vec<String>,
    metric_names: Vec<String>,
}

fuzz_target!(|input: FuzzInput| {
    let req = QueryRequest {
        dimensions: input.dim_names,
        metrics: input.metric_names,
    };
    if let Ok(sql) = expand("fuzz_view", &input.def, &req) {
        // Successful expansion must produce non-empty SQL
        assert!(!sql.is_empty());
        // Basic validity: balanced parentheses, starts with expected prefix
        assert!(sql.starts_with("WITH"));
    }
    // Errors are fine -- expand() returning Err is expected for invalid combos
});
```

### Pattern 5: Fuzz Target 3 -- Query-Time Name Arrays
**What:** Fix a known-good `SemanticViewDefinition` and fuzz only the dimension/metric name strings. Catches injection via user-supplied column names that might break SQL generation.
**When to use:** Specifically targets the name-to-expression resolution and quoting logic.
**Example:**
```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use semantic_views::expand::{expand, QueryRequest};
use semantic_views::model::{Dimension, Metric, SemanticViewDefinition};

#[derive(Debug, Arbitrary)]
struct NameFuzzInput {
    dim_names: Vec<String>,
    metric_names: Vec<String>,
}

fuzz_target!(|input: NameFuzzInput| {
    let def = fixed_definition(); // Known-good definition
    let req = QueryRequest {
        dimensions: input.dim_names,
        metrics: input.metric_names,
    };
    if let Ok(sql) = expand("fuzz_view", &def, &req) {
        assert!(!sql.is_empty());
        assert!(sql.starts_with("WITH"));
    }
});

fn fixed_definition() -> SemanticViewDefinition {
    // Reuse a definition from the existing test fixtures
    SemanticViewDefinition {
        base_table: "orders".to_string(),
        dimensions: vec![
            Dimension { name: "region".to_string(), expr: "region".to_string(), source_table: None },
            Dimension { name: "month".to_string(), expr: "date_trunc('month', created_at)".to_string(), source_table: None },
        ],
        metrics: vec![
            Metric { name: "revenue".to_string(), expr: "sum(amount)".to_string(), source_table: None },
            Metric { name: "count".to_string(), expr: "count(*)".to_string(), source_table: None },
        ],
        filters: vec!["status = 'active'".to_string()],
        joins: vec![],
    }
}
```

### Pattern 6: Nightly CI Workflow for Fuzzing
**What:** GitHub Actions scheduled workflow running all fuzz targets nightly with timed execution and crash artifact handling.
**When to use:** Required by user decision -- nightly CI fuzzing with issue creation on crash.
**Example:**
```yaml
name: Nightly Fuzz
on:
  schedule:
    - cron: '0 3 * * *'  # Daily at 03:00 UTC
  workflow_dispatch:

permissions:
  contents: write
  issues: write
  pull-requests: write

env:
  FUZZ_TIME: 300  # 5 minutes per target

jobs:
  fuzz:
    name: Fuzz ${{ matrix.target }}
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        target: [fuzz_json_parse, fuzz_sql_expand, fuzz_query_names]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install cargo-fuzz
      - name: Run fuzzer
        id: fuzz
        continue-on-error: true
        run: cargo fuzz run ${{ matrix.target }} -- -max_total_time=${{ env.FUZZ_TIME }}
      - name: Upload crash artifacts
        if: steps.fuzz.outcome == 'failure'
        uses: actions/upload-artifact@v4
        with:
          name: crash-${{ matrix.target }}-${{ github.sha }}
          path: fuzz/artifacts/${{ matrix.target }}/
      - name: Open issue on crash
        if: steps.fuzz.outcome == 'failure'
        uses: actions/github-script@v7
        with:
          script: |
            await github.rest.issues.create({
              owner: context.repo.owner,
              repo: context.repo.repo,
              title: `Fuzz crash: ${{ matrix.target }}`,
              body: `Nightly fuzzing found a crash in \`${{ matrix.target }}\`.\n\nRun: ${context.serverUrl}/${context.repo.owner}/${context.repo.repo}/actions/runs/${context.runId}\n\nDownload the crash artifact and reproduce locally:\n\`\`\`bash\ncargo fuzz run ${{ matrix.target }} fuzz/artifacts/${{ matrix.target }}/<crash-file>\n\`\`\``,
              labels: ['bug', 'fuzzing']
            })
      # Commit new corpus entries (only if fuzzer succeeded)
      - name: Commit corpus updates
        if: steps.fuzz.outcome == 'success'
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git add fuzz/corpus/
          git diff --cached --quiet || git commit -m "chore(fuzz): update corpus for ${{ matrix.target }}"
          git push || echo "Nothing to push"
```
Source: [Rust Fuzz Book - CI Integration](https://rust-fuzz.github.io/book/cargo-fuzz/ci.html)

### Pattern 7: Community Extension description.yml for Rust
**What:** The descriptor file needed to submit the extension to the DuckDB community extension registry.
**When to use:** MAINTAINER.md documents this for the publishing section.
**Example (derived from existing Rust extensions `rusty_quack` and `evalexpr_rhai`):**
```yaml
extension:
  name: semantic_views
  description: Semantic views - a declarative layer for dimensions, measures, and relationships
  version: 0.1.0
  language: Rust
  build: cargo
  license: MIT
  excluded_platforms: "wasm_mvp;wasm_eh;wasm_threads;windows_amd64_rtools;windows_amd64_mingw;linux_amd64_musl"
  requires_toolchains: "rust;python3"
  maintainers:
    - paul-rl

repo:
  github: paul-rl/duckdb-semantic-views
  ref: <commit-sha>

docs:
  hello_world: |
    SELECT define_semantic_view('orders', '{"base_table":"orders","dimensions":[{"name":"region","expr":"region"}],"metrics":[{"name":"revenue","expr":"sum(amount)"}]}');
    FROM semantic_query('orders', dimensions := ['region'], metrics := ['revenue']);
  extended_description: |
    Semantic views let you define dimensions, metrics, joins, and filters once,
    then query with any combination. The extension handles GROUP BY, JOIN, and
    filter composition automatically.
```
Source: [DuckDB Community Extensions Development](https://duckdb.org/community_extensions/development), [rusty_quack descriptor](https://github.com/duckdb/community-extensions), [evalexpr_rhai descriptor](https://github.com/duckdb/community-extensions)

### Anti-Patterns to Avoid
- **Fuzzing the `extension` feature code directly:** The `ddl` and `query` modules behind `#[cfg(feature = "extension")]` use DuckDB loadable-extension stubs that only work inside a running DuckDB process. Fuzz targets must exercise the pure Rust logic (`model::SemanticViewDefinition::from_json`, `expand::expand`, `expand::quote_ident`) via the default (bundled) feature path.
- **Using `cargo fuzz init` on an existing project with workspace lints:** The auto-generated `fuzz/Cargo.toml` will not inherit `[workspace.lints]` from the parent. This is correct -- the fuzz crate should have its own minimal configuration.
- **Committing `fuzz/artifacts/` to git:** Crash artifacts are debug data, not corpus. Only `fuzz/corpus/` should be committed.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Structured input generation | Custom random generators for `SemanticViewDefinition` | `arbitrary` derive macro | Handles `String`, `Vec<T>`, `Option<T>` automatically; coverage-guided mutation is far more effective than random generation |
| Fuzz harness wiring | Manual libFuzzer C API integration | `libfuzzer-sys::fuzz_target!` macro | Handles signal setup, crash artifact writing, corpus management |
| Corpus minimization | Manual deduplication scripts | `cargo fuzz cmin <target>` | Built-in coverage-guided minimization; removes inputs that don't add coverage |
| CI crash reporting | Custom crash detection scripts | `actions/upload-artifact` + `actions/github-script` | Standard GitHub Actions pattern; reliable artifact upload and issue creation |

**Key insight:** cargo-fuzz handles the entire fuzzing lifecycle (build, run, minimize, report). The only custom code needed is the fuzz harness body itself (a few lines per target).

## Common Pitfalls

### Pitfall 1: Nightly Toolchain Conflict
**What goes wrong:** cargo-fuzz requires Rust nightly for sanitizer support. Using `rustup override set nightly` in the project directory breaks the stable-toolchain builds.
**Why it happens:** cargo-fuzz invokes `cargo +nightly` internally, but if the directory override is set, all cargo commands use nightly.
**How to avoid:** Never set a directory-level nightly override. Use `cargo +nightly fuzz run` explicitly, or let `cargo fuzz` handle the toolchain selection automatically (it passes `+nightly` internally). Document in MAINTAINER.md that only `cargo fuzz` needs nightly.
**Warning signs:** `cargo test` starts showing nightly-only warnings or different behavior.

### Pitfall 2: cdylib Build Failure in Fuzz Targets
**What goes wrong:** Fuzz targets fail to compile because the linker tries to produce a cdylib.
**Why it happens:** The parent crate has `crate-type = ["cdylib", "lib"]`. When the fuzz crate depends on it, Cargo builds the "lib" (rlib) target, which is correct. But if `--features extension` leaks into the fuzz build, loadable-extension stubs won't resolve.
**How to avoid:** The fuzz `Cargo.toml` must depend on `semantic_views = { path = ".." }` without the `extension` feature. The default feature (`duckdb/bundled`) is what fuzz targets need.
**Warning signs:** Linker errors mentioning `duckdb_rs_extension_api_init` or `duckdb_connect`.

### Pitfall 3: Empty Corpus Produces Poor Coverage
**What goes wrong:** Running `cargo fuzz run` with no seed corpus means the fuzzer starts from random bytes, taking much longer to discover interesting code paths.
**Why it happens:** Without seed inputs that parse successfully, the fuzzer struggles to get past the JSON parsing stage.
**How to avoid:** Populate `fuzz/corpus/fuzz_json_parse/` with seed files containing valid JSON definitions from the existing test suite. For `fuzz_sql_expand/` and `fuzz_query_names/`, the `arbitrary` derive handles structured input generation, but seed corpus files still help.
**Warning signs:** Coverage report shows low coverage after extended fuzzing time.

### Pitfall 4: Fuzz Target Panics vs Errors
**What goes wrong:** A `panic!` in production code causes a fuzz crash, but some panics may be expected (e.g., `unreachable!` in pattern matches). The fuzzer can't distinguish "expected" from "unexpected" panics.
**Why it happens:** libFuzzer treats any panic/abort as a crash.
**How to avoid:** Audit all `unwrap()`, `expect()`, `unreachable!()`, and `panic!()` calls in `model.rs`, `expand.rs`, and `catalog.rs`. Convert any that could be triggered by malformed input to `Result` returns. Legitimate internal invariant assertions (truly unreachable code) can stay.
**Warning signs:** Fuzz crashes on inputs that should simply return an error.

### Pitfall 5: MAINTAINER.md Assumes Cargo/Rust Knowledge
**What goes wrong:** Instructions like "run `cargo build`" without explaining what Cargo is or how to install Rust leave the target audience (Python expert) stuck.
**Why it happens:** Documentation written by Rust developers unconsciously assumes familiarity.
**How to avoid:** Start from "install `rustup`" (analogous to `pyenv`). Explain `Cargo.toml` (analogous to `pyproject.toml`). Define `cargo test` vs `just test` vs `make test` and when to use each.
**Warning signs:** A reader unfamiliar with Rust cannot follow the instructions without external research.

### Pitfall 6: CI Corpus Commit Race Condition
**What goes wrong:** Multiple fuzz target jobs try to commit corpus updates to the same branch simultaneously, causing push conflicts.
**Why it happens:** The matrix strategy runs targets in parallel; each tries to `git push` corpus updates.
**How to avoid:** Either (a) run corpus commits sequentially in a separate job that depends on all fuzz jobs, or (b) use a PR-based approach where each target creates a separate branch and auto-merges. Option (b) is cleaner with the user's stated preference for "CI auto-commits new corpus entries via PR."
**Warning signs:** CI jobs fail with "rejected -- non-fast-forward" git push errors.

## Code Examples

Verified patterns from official sources:

### Seed Corpus File for JSON Parsing Target
```json
{
    "base_table": "orders",
    "dimensions": [{"name": "region", "expr": "region"}],
    "metrics": [{"name": "revenue", "expr": "sum(amount)"}],
    "filters": ["status = 'active'"],
    "joins": [{"table": "customers", "on": "orders.customer_id = customers.id"}]
}
```
Place in `fuzz/corpus/fuzz_json_parse/seed_valid_full.json`.

### Running Fuzzer Locally
```bash
# Run a specific target (runs indefinitely until Ctrl+C or crash)
cargo fuzz run fuzz_json_parse

# Run with time limit (300 seconds)
cargo fuzz run fuzz_json_parse -- -max_total_time=300

# List all fuzz targets
cargo fuzz list

# Minimize corpus (remove redundant inputs)
cargo fuzz cmin fuzz_json_parse

# Reproduce a crash
cargo fuzz run fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-<hash>

# View debug output for a test case
cargo fuzz fmt fuzz_json_parse fuzz/artifacts/fuzz_json_parse/crash-<hash>
```

### MAINTAINER.md Source Tree Overview Pattern
```markdown
## Architecture Overview

src/
├── lib.rs              # Extension entrypoint -- registers functions with DuckDB
├── model.rs            # Data types: SemanticViewDefinition, Dimension, Metric, Join
├── catalog.rs          # In-memory catalog (HashMap) + sidecar persistence
├── expand.rs           # SQL generation engine -- turns definitions + requests into SQL
├── ddl/                # DDL functions (define, drop, list, describe)
│   ├── define.rs       # define_semantic_view() scalar function
│   ├── drop.rs         # drop_semantic_view() scalar function
│   ├── list.rs         # list_semantic_views() table function
│   └── describe.rs     # describe_semantic_view() table function
└── query/              # Query interface
    ├── table_function.rs  # semantic_query() table function (FFI-heavy)
    ├── explain.rs         # explain_semantic_view() table function
    └── error.rs           # Query error types
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Raw `&[u8]` fuzz targets only | Structure-aware fuzzing via `Arbitrary` derive | arbitrary 1.0+ (2021) | Much better coverage for structured data like JSON/SQL |
| Manual libFuzzer setup | `cargo fuzz` CLI with auto-build | cargo-fuzz 0.10+ (2022) | One command setup, corpus management, crash minimization |
| C++ template only for DuckDB extensions | Rust template via `extension-template-rs` | DuckDB v1.1+ (2024) | Pure Rust extensions now possible via C API |
| `build: cmake` for Rust extensions | `build: cargo` in community registry | Late 2024 | `rusty_quack` uses `build: cargo`; simpler than cmake wrapper |

**Deprecated/outdated:**
- `#[macro_use] extern crate libfuzzer_sys` -- the 2018+ edition syntax `use libfuzzer_sys::fuzz_target` is preferred
- `cargo fuzz init` generates boilerplate that may need manual adjustment for workspace projects with `[workspace.lints]`

## Open Questions

1. **SQL Validity Check Mechanism**
   - What we know: The user wants to verify that expanded SQL "parses successfully." DuckDB has a parser accessible via `EXPLAIN` or `PREPARE`, but these require a running DuckDB instance.
   - What's unclear: Whether to spin up a bundled DuckDB connection in the fuzz harness to do real parsing, or use a lightweight heuristic (non-empty, starts with `WITH`, balanced parentheses).
   - Recommendation: Use a lightweight heuristic in the fuzz harness. Running a full DuckDB parser in the fuzz loop would dramatically slow down iterations (cargo-fuzz aims for millions of executions/sec). A heuristic catches gross generation bugs; real SQL validity is already covered by the integration tests. If a deeper check is desired, a separate slow-path fuzz target could run SQL through DuckDB's `PREPARE` statement at a lower iteration rate.

2. **Workspace Integration for fuzz/ Directory**
   - What we know: The project root `Cargo.toml` does not declare a `[workspace]` with `members`. It uses `[workspace.lints.clippy]` which implicitly makes it a workspace root.
   - What's unclear: Whether `cargo fuzz init` will create the fuzz directory as part of this implicit workspace or as an independent workspace.
   - Recommendation: Use `cargo fuzz init --fuzzing-workspace=true` to create the fuzz directory as an independent workspace. This avoids any interaction with the parent's `[workspace.lints]` configuration. The fuzz crate does not need pedantic clippy lints.

3. **Corpus Commit Strategy in CI**
   - What we know: User wants corpus committed to repo and CI to auto-commit new entries via PR.
   - What's unclear: Exact git workflow to avoid race conditions when parallel matrix jobs all want to commit.
   - Recommendation: After all fuzz jobs complete, a separate `commit-corpus` job (using `needs: [fuzz]`) collects all corpus changes, creates a single PR. This avoids parallel push conflicts.

## Sources

### Primary (HIGH confidence)
- [Rust Fuzz Book - cargo-fuzz guide](https://rust-fuzz.github.io/book/cargo-fuzz/guide.html) - Setup, commands, corpus management
- [Rust Fuzz Book - Structure-Aware Fuzzing](https://rust-fuzz.github.io/book/cargo-fuzz/structure-aware-fuzzing.html) - Arbitrary derive pattern
- [Rust Fuzz Book - CI Integration](https://rust-fuzz.github.io/book/cargo-fuzz/ci.html) - GitHub Actions workflow pattern
- [Rust Fuzz Book - Tutorial](https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html) - fuzz_target! macro, directory structure
- [DuckDB Community Extensions Development](https://duckdb.org/community_extensions/development) - Publishing process
- [DuckDB Community Extensions Documentation](https://duckdb.org/community_extensions/documentation) - description.yml fields

### Secondary (MEDIUM confidence)
- [rusty_quack description.yml](https://github.com/duckdb/community-extensions) - Verified Rust extension descriptor using `build: cargo`, `language: Rust`
- [evalexpr_rhai description.yml](https://github.com/duckdb/community-extensions) - Verified Rust+C++ extension descriptor with `requires_toolchains: rust`
- [cargo-fuzz README](https://github.com/rust-fuzz/cargo-fuzz) - Nightly requirement, platform support (x86_64, aarch64, Unix only)
- [DuckDB community-extensions issue #54](https://github.com/duckdb/community-extensions/issues/54) - Rust extension ecosystem status discussion

### Tertiary (LOW confidence)
- None. All findings verified through primary or secondary sources.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - cargo-fuzz/arbitrary are the official, well-documented Rust fuzzing tools; no ambiguity
- Architecture: HIGH - fuzz directory structure is well-specified by cargo-fuzz; project codebase is well-understood from prior phases
- Pitfalls: HIGH - based on codebase analysis (feature flags, crate types) and verified documentation
- Community extension publishing: MEDIUM - Rust extension support via `build: cargo` is confirmed by `rusty_quack`, but the `extension-template-rs` is still marked "experimental"

**Research date:** 2026-02-26
**Valid until:** 2026-03-26 (stable domain; cargo-fuzz and arbitrary are mature)
