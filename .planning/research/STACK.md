# Technology Stack: v0.2.0 Additions

**Project:** DuckDB Semantic Views Extension
**Researched:** 2026-03-01
**Milestone:** v0.2.0 — Native DDL + Time Dimensions
**Scope:** What library/crate additions are needed for v0.2.0 features, and what already exists

---

## Bottom Line Up Front

**New Cargo dependency: exactly one.** Add `cc = "1.2"` to `[build-dependencies]` and add a new `build.rs`. Everything else — DuckDB headers, time dimension logic, the Rust/C++ boundary, string handling — is achievable with libraries already in the dependency tree or already present in the build environment. No new `[dependencies]` entries are needed.

---

## Existing Dependency Inventory (v0.1.0 Cargo.toml)

The following are already present and sufficient for the v0.2.0 features they support:

| Crate | Version | Already Covers |
|-------|---------|---------------|
| `duckdb` | `=1.4.4` | All DuckDB Rust extension APIs, VTab, ScalarFunction |
| `libduckdb-sys` | `=1.4.4` | Raw FFI bindings; the `duckdb.tar.gz` bundled source is the DuckDB header source |
| `serde` + `serde_json` | `1` | `TimeGrain` serialization, pragma SQL string construction |
| `arbitrary` | `1`, optional | Already present; no change needed for time dimensions |
| `proptest` | `1.9` | Already covers expansion engine; time dimension tests fit naturally |

---

## New Dependency: `cc` crate (Build-Dependency Only)

### What it is

The `cc` crate is the standard Rust ecosystem tool for compiling C or C++ source files from `build.rs`. It wraps the host C++ compiler (clang++ on macOS, g++ on Linux, MSVC on Windows), handles cross-compilation flag propagation automatically, and links the compiled object into the cdylib.

### Why it is needed

The C++ shim (`src/shim/shim.cpp`) cannot be compiled by Cargo without an explicit build step. The `cc` crate is the only supported mechanism for this in a pure-Cargo build (i.e., without introducing CMake). It is a build-time-only dependency — it does not appear in the compiled extension binary.

### Version

**`cc = "1.2"`** — pin to the minor version, not an exact patch. This tracks the most stable API surface. The crate had 575M+ downloads as of 2026-03; it is the most widely deployed build-time C/C++ compilation tool in the Rust ecosystem.

Current latest: **1.2.45** (confirmed via crates.io, 2026-03-01). The `1.2` range is appropriate because the API is stable across patch versions and no breaking changes have occurred in the 1.x series.

### Where it goes

```toml
# Cargo.toml — NEW section
[build-dependencies]
cc = "1.2"
```

No changes to `[dependencies]` or `[dev-dependencies]`.

### Confidence

HIGH — confirmed via crates.io, official Rust cc crate documentation, and the fact that `libduckdb-sys` itself already uses `cc` internally (visible in the `libduckdb-sys-1.4.4` build output).

---

## DuckDB C++ Headers — No New Dependency, but New Artifact

### The header situation

The C++ shim (`shim.cpp`) needs DuckDB's C++ internal headers — specifically:
- `duckdb/main/config.hpp` (for `DBConfig::GetConfig`, `config.parser_extensions`)
- `duckdb/parser/parser_extension.hpp` (for `ParserExtension`, `parse_function_t`, `plan_function_t`)
- `duckdb/function/pragma_function.hpp` (for `PragmaFunction`, `pragma_query_t`)

These are in `duckdb.hpp`, the amalgamated single-header C++ SDK that DuckDB ships alongside `duckdb.h`.

### What already exists

**During `cargo test` (bundled mode):** `libduckdb-sys` extracts `duckdb.tar.gz` into the build output directory. `duckdb.hpp` is present at `{OUT_DIR}/duckdb/src/include/duckdb.hpp` — verified on this project at `target/debug/build/libduckdb-sys-*/out/duckdb/src/include/duckdb.hpp`. Cargo propagates the `lib_dir` metadata (`cargo:lib_dir=...`) as the `DEP_DUCKDB_LIB_DIR` environment variable to dependent build scripts.

**During `make debug/release` (extension mode, `--no-default-features --features extension`):** `libduckdb-sys` uses the loadable-extension stub path — it does NOT extract `duckdb.tar.gz`. The DuckDB C++ headers are not available in `OUT_DIR`. This is the build that produces the distributable `.duckdb_extension` file.

### Solution: vendor `duckdb.hpp` into the repo

Vendor `duckdb.hpp` at `duckdb_capi/duckdb.hpp` (alongside the existing `duckdb_capi/duckdb.h` and `duckdb_capi/duckdb_extension.h`). This file is:

- Downloaded once from the pinned DuckDB release: `https://github.com/duckdb/duckdb/releases/download/v1.4.4/libduckdb-src.zip` (contains `duckdb.hpp`)
- Committed to the repository (it is ~6MB — large but vendor-appropriate; DuckDB itself recommends this for embedding)
- Version-locked to match `duckdb-rs` and `libduckdb-sys` at `=1.4.4`
- Updated by a `just update-headers` recipe when `TARGET_DUCKDB_VERSION` changes in the Makefile

The `build.rs` uses the vendored copy unconditionally:

```rust
fn main() {
    cc::Build::new()
        .cpp(true)
        .file("src/shim/shim.cpp")
        .include("duckdb_capi/")          // vendored duckdb.hpp lives here
        .flag("-std=c++17")
        .warnings(false)                  // suppress DuckDB's own warnings
        .compile("semantic_views_shim");
}
```

### Why not rely on OUT_DIR for the extension build

`OUT_DIR` from `libduckdb-sys` is available during bundled builds (`cargo test`) but not during extension builds. Trying to detect and switch paths in `build.rs` based on active features is fragile. Vendoring is the reliable, reproducible approach.

### Confidence

HIGH — confirmed by inspecting the actual `libduckdb-sys-1.4.4` build output in this project's `target/` directory. The two-mode behavior (bundled vs loadable-extension) is clearly documented in the project's Makefile and Cargo.toml.

---

## Time Dimensions — No New Crates Needed

### Why no `chrono`

`chrono` is an optional dependency of `duckdb-rs` (enabled by the `modern-full` feature). The project does not enable that feature and should not add it just for time dimensions. The reason: time dimension support in v0.2.0 is **SQL codegen only** — the Rust code generates `date_trunc('month', "order_date")` SQL strings that DuckDB executes. Rust never performs date arithmetic itself. There is no date type to parse, format, or compute — just string construction.

### What is sufficient

`serde_json` (already present) handles serialization of the `TimeGrain` enum. `std::fmt` handles string formatting of the `date_trunc` SQL fragment. No date math library is needed.

The complete time dimension feature adds:
1. A `TimeGrain` enum to `src/model.rs` (derives `Serialize`, `Deserialize`, `Debug`, `Clone` — all from existing `serde` dep)
2. A `granularities: HashMap<String, TimeGrain>` field to `QueryRequest` in `src/expand.rs`
3. A `date_trunc('{grain}', {expr})` wrapper in `build_sql()` for dimensions with a matching entry in `granularities`
4. A `granularities` named parameter parsed in `src/query/table_function.rs`

### Confidence

HIGH — time dimensions are purely additive Rust changes to the existing expansion engine. The existing proptest setup already covers `build_sql()` and will naturally extend to cover time dimension cases.

---

## DuckDB ParserExtension API Version Constraints

### `parse_function_t` and `plan_function_t`

The `ParserExtension` mechanism (`parse_function_t`, `plan_function_t`) is documented in DuckDB's CIDR 2025 paper ("Runtime-Extensible Parsers", Mühleisen & Raasveldt), which describes the mechanism as production-ready. It is present in DuckDB 1.4.x. The mechanism has existed since DuckDB 0.9.x based on community extension usage evidence.

**Key constraint confirmed by ARCHITECTURE.md (first-party research):** `plan_function_t` fires inside DuckDB's normal planner execution path, subject to the same execution lock constraints as the current `invoke`. The pragma_query_t approach sidesteps this by returning a SQL string that DuckDB executes post-lock.

**Version requirement:** No minimum version beyond 1.4.4 is required. The `ParserExtension` API, `pragma_query_t`, and `ExtensionUtil::RegisterFunction` are all present in DuckDB 1.4.4. The project is already pinned to this version.

**ABI stability:** The project's existing CI (DuckDBVersionMonitor) already handles version breakage detection. No change to the CI strategy is needed for v0.2.0.

### Confidence

MEDIUM — `parse_function_t` and `plan_function_t` existence at DuckDB 1.4.4 is confirmed via DuckDB GitHub issue #18485 and the CIDR 2025 paper. The exact function pointer signatures (`string (*)(ClientContext&, const FunctionParameters&)` for `pragma_query_t`) were confirmed against `pragma_function.hpp` in the ARCHITECTURE research. The specific include paths within `duckdb.hpp` need hands-on validation when implementing the shim.

---

## Complete v0.2.0 Cargo.toml Changes

The change is minimal:

```toml
# ADD to Cargo.toml:
[build-dependencies]
cc = "1.2"
```

```toml
# NO CHANGES to [dependencies] — existing deps are sufficient:
# duckdb = { version = "=1.4.4", default-features = false }
# libduckdb-sys = "=1.4.4"
# serde = { version = "1", features = ["derive"] }
# serde_json = "1"
# strsim = "0.11"
# arbitrary = { version = "1", optional = true, features = ["derive"] }
```

```toml
# NO CHANGES to [dev-dependencies]:
# proptest = "1.9"
```

---

## New File: `build.rs`

This file does not exist today. It must be added at the repository root. It runs only when the `extension` feature is active (guarded by a feature flag check) to avoid adding C++ compilation overhead to `cargo test`:

```rust
fn main() {
    // Only compile the C++ shim when building the loadable extension.
    // During `cargo test` (default/bundled feature), the shim is not needed
    // because parser hooks and pragma registration are C++-only paths that
    // only fire when DuckDB loads the extension — not in unit tests.
    if std::env::var("CARGO_FEATURE_EXTENSION").is_ok() {
        cc::Build::new()
            .cpp(true)
            .file("src/shim/shim.cpp")
            .include("duckdb_capi/")
            .flag("-std=c++17")
            .warnings(false)
            .compile("semantic_views_shim");
    }
}
```

The `CARGO_FEATURE_EXTENSION` environment variable is set by Cargo when `--features extension` is active.

---

## Alternatives Considered and Rejected

| Alternative | Why Rejected |
|-------------|-------------|
| Add `chrono` for time dimensions | Not needed — time dimensions are SQL string codegen only; DuckDB handles all date math |
| Add `sqlparser` for DDL parsing | Not needed — `CREATE SEMANTIC VIEW` DDL parsing is a simple hand-written string matcher in Rust (far simpler grammar than full SQL) |
| Download `duckdb.hpp` at configure time (not vendor) | Fragile — requires network access during `make configure` and introduces a download failure mode in CI |
| Rely on `DEP_DUCKDB_LIB_DIR` for headers | Only works in bundled mode; fails in extension mode (`--no-default-features`) — confirmed by inspecting the project's actual build output |
| Use CMake to compile the shim | Disproportionate — changes the entire build system for ~100 lines of C++; the `cc` crate handles cross-compilation correctly |
| Add `cxx` crate for the Rust/C++ boundary | Overkill for a thin boundary (two C++ registration functions + five `extern "C"` callbacks); `cxx` is appropriate for complex bidirectional FFI, not for forwarding to string-based Rust functions |

---

## Installation Instructions for v0.2.0

### Step 1: Add build dependency

```toml
# Cargo.toml
[build-dependencies]
cc = "1.2"
```

### Step 2: Vendor the C++ header

Download `duckdb.hpp` from the v1.4.4 release into `duckdb_capi/`:

```bash
# Justfile recipe: just update-headers
curl -L https://github.com/duckdb/duckdb/releases/download/v1.4.4/libduckdb-src.zip \
     -o /tmp/libduckdb-src.zip
unzip -j /tmp/libduckdb-src.zip "duckdb.hpp" -d duckdb_capi/
```

Add `duckdb_capi/duckdb.hpp` to version control (gitignore currently excludes nothing from `duckdb_capi/`).

### Step 3: Create `build.rs`

Place at repo root. See the "New File: build.rs" section above.

### Step 4: Create the shim files

Per ARCHITECTURE.md:
- `src/shim/shim.cpp` — C++ registration code (~80 lines)
- `src/shim/shim.h` — `extern "C"` boundary declarations
- `src/shim/ffi.rs` — Rust implementations of `extern "C"` functions

No new crates required for any of these files.

---

## Sources

- [cc crate — crates.io](https://crates.io/crates/cc) — version 1.2.45 confirmed current (HIGH confidence)
- [cc crate — docs.rs](https://docs.rs/cc) — C++ compilation via `cc::Build::new().cpp(true)` (HIGH confidence)
- Project's own `target/debug/build/libduckdb-sys-*/output` — confirmed `cargo:lib_dir` metadata and `duckdb.hpp` presence in bundled mode (HIGH confidence — first-party)
- Project's own `target/release/build/libduckdb-sys-*/output` — confirmed headers absent in extension/loadable mode (HIGH confidence — first-party)
- [duckdb-rs 1.4.4 dependencies — crates.io](https://crates.io/crates/duckdb) — `chrono` is optional, gated by `modern-full` feature; not transitively available with current feature flags (HIGH confidence)
- [DuckDB GitHub issue #18485](https://github.com/duckdb/duckdb/issues/18485) — confirmed `DBConfig::GetConfig` + `config.parser_extensions.push_back()` pattern at DuckDB 1.4.x (HIGH confidence)
- CIDR 2025 paper — "Runtime-Extensible Parsers", Mühleisen & Raasveldt — confirms `parse_function_t` / `plan_function_t` as production API (HIGH confidence)
- ARCHITECTURE.md (this project, 2026-02-28) — confirmed two-mode build behavior, header strategy, shim file layout (HIGH confidence — first-party)
