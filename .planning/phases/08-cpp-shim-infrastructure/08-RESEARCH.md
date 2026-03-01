# Phase 8: C++ Shim Infrastructure - Research

**Researched:** 2026-03-01
**Domain:** Rust + C++ mixed cdylib — `cc` crate, DuckDB C++ header vendoring, symbol visibility, cross-platform build
**Confidence:** HIGH (build mechanics confirmed against live project artifacts; C++ registration patterns confirmed against bundled DuckDB 1.4.4 headers)

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| INFRA-01 | C++ shim compiles via `cc` crate on all 5 CI targets without breaking `cargo test` workflow | Standard stack section confirms `cc` 1.2.56 approach; Architecture section shows feature-gated build.rs pattern; Common Pitfalls section covers all cross-platform concerns |
</phase_requirements>

---

## Summary

Phase 8 validates that a C++ shim can coexist with the existing pure-Rust extension in a single Cargo-primary build. No new user-visible functionality is delivered — the phase is entirely infrastructure. The deliverable is a minimal `shim.cpp` that compiles cleanly on all 5 CI targets (Linux x86_64, Linux arm64, macOS x86_64, macOS arm64, Windows x86_64) and produces a `.duckdb_extension` binary where only the three DuckDB entry point symbols are exported.

The locked architectural decision (from STATE.md, confirmed throughout prior v0.2.0 research) is Cargo-primary with the `cc` crate. CMake is explicitly rejected. This means `build.rs` drives C++ compilation, the existing `rust.Makefile` and CI workflows work unchanged, and developer tooling (`cargo test`, `just`, clippy) continues to function without CMake.

The C++ shim in Phase 8 contains no logic — it is a skeleton that includes `duckdb.hpp`, compiles cleanly, and exposes a stub `extern "C"` function. Logic (parser registration, pragma registration) is added in later phases. This deliberate approach isolates build mechanics from feature complexity.

**Primary recommendation:** Add `cc = "1.2"` to `[build-dependencies]`, vendor `duckdb.hpp` at `duckdb_capi/duckdb.hpp`, add a feature-gated `build.rs`, and create a minimal `src/shim/shim.cpp` skeleton. Validate with `cargo build --no-default-features --features extension` on all CI targets before adding any shim logic.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `cc` crate | 1.2.56 (already transitively present via `libduckdb-sys`) | Compiles `shim.cpp` from `build.rs` into a static archive linked into the cdylib | The universal Rust build-script tool for C/C++ compilation; handles cross-compilation, MSVC, clang, gcc automatically — 575M+ downloads |
| `duckdb.hpp` (vendored) | v1.4.4 (matches pinned `libduckdb-sys = "=1.4.4"`) | Single-header C++ SDK giving `shim.cpp` access to `DBConfig`, `ParserExtension`, `PragmaFunction`, `ExtensionUtil` | DuckDB's official amalgamated C++ header — the only stable way to access C++ internal APIs not in `duckdb.h` |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `duckdb_capi/duckdb.h` (already present in repo) | v1.4.4 | C API header — already vendored | Already exists; no change needed for Phase 8 |
| `duckdb_capi/duckdb_extension.h` (already present in repo) | v1.4.4 | Extension C API header — already vendored | Already exists; no change needed for Phase 8 |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `cc` crate + `build.rs` | CMake-primary with `c_cpp.Makefile` | CMake changes the entire build system, breaks `just`, `cargo nextest`, and all CI workflows; `cc` crate is sufficient for a single `.cpp` file |
| Vendored `duckdb.hpp` | `DEP_DUCKDB_LIB_DIR` from `libduckdb-sys` | `DEP_DUCKDB_LIB_DIR` is only available in bundled mode (`cargo test`), not in extension mode (`--no-default-features --features extension`). Vendoring is the reliable path confirmed against this project's actual build artifacts |
| `cxx` crate for Rust/C++ boundary | Plain `extern "C"` | `cxx` is appropriate for complex bidirectional FFI with owned types; this project's boundary is thin (C-string in, C-string out). `cxx` adds significant build complexity for no benefit |

**Installation:**
```toml
# Add to Cargo.toml:
[build-dependencies]
cc = "1.2"
```

```bash
# Download duckdb.hpp to vendor directory:
curl -L https://github.com/duckdb/duckdb/releases/download/v1.4.4/libduckdb-src.zip \
     -o /tmp/libduckdb-src.zip
unzip -j /tmp/libduckdb-src.zip "duckdb.hpp" -d duckdb_capi/
```

---

## Architecture Patterns

### Recommended Project Structure After Phase 8

```
duckdb-semantic-views/
├── build.rs                    # NEW: compiles shim.cpp when extension feature active
├── Cargo.toml                  # MODIFIED: adds cc to [build-dependencies]
├── duckdb_capi/
│   ├── duckdb.h                # already present
│   ├── duckdb_extension.h      # already present
│   └── duckdb.hpp              # NEW: vendored single-header C++ SDK (v1.4.4)
├── src/
│   ├── lib.rs                  # MODIFIED: calls shim init when extension feature active
│   ├── shim/
│   │   ├── mod.rs              # NEW: re-exports shim types (empty in Phase 8)
│   │   ├── shim.cpp            # NEW: C++ skeleton (includes duckdb.hpp, no logic)
│   │   └── shim.h              # NEW: extern "C" boundary declarations
│   └── ...                     # existing files unchanged
└── Justfile                    # MODIFIED: add just update-headers recipe
```

### Pattern 1: Feature-Gated `build.rs`

**What:** Compile `shim.cpp` only when `--features extension` is active. During `cargo test` (default/bundled feature), no C++ compilation occurs.

**When to use:** Always — the shim is irrelevant to unit tests and only needed for the loadable extension binary.

**Why this gating is critical:** Without it, `cargo test` would require a C++ compiler and DuckDB headers. The bundled test workflow must remain dependency-minimal.

**Example:**
```rust
// Source: confirmed pattern from Cargo build script documentation and
// cc-rs documentation (docs.rs/cc/latest/cc/struct.Build.html)
// build.rs — place at repository root

fn main() {
    // Only compile the C++ shim when building the loadable extension.
    // CARGO_FEATURE_EXTENSION is set by Cargo when --features extension is active.
    // During `cargo test` (default/bundled feature), this block is skipped entirely.
    if std::env::var("CARGO_FEATURE_EXTENSION").is_ok() {
        cc::Build::new()
            .cpp(true)                        // Enable C++ mode (uses CXX, not CC)
            .file("src/shim/shim.cpp")
            .include("duckdb_capi/")          // vendored duckdb.hpp lives here
            .flag_if_supported("-std=c++17")  // MSVC uses /std:c++17, not -std=c++17
            .warnings(false)                  // suppress DuckDB's own internal warnings
            .compile("semantic_views_shim");  // produces libsemantic_views_shim.a
        // Cargo automatically links the static archive into the cdylib.
        // No println!("cargo:rustc-link-lib=...") needed — cc crate handles this.
    }
}
```

### Pattern 2: Minimal C++ Shim Skeleton (Phase 8 Only)

**What:** A skeleton `.cpp` that proves the build works before any logic is added.

**When to use:** Phase 8 only — this becomes more complex in Phases 10 and 11.

**Example:**
```cpp
// Source: DuckDB extension pattern — confirmed against bundled DuckDB 1.4.4 headers
// src/shim/shim.cpp

#include "duckdb.hpp"
#include "duckdb/main/config.hpp"
#include "duckdb/parser/parser_extension.hpp"
#include "duckdb/function/pragma_function.hpp"
#include "shim.h"

using namespace duckdb;

// Phase 8: skeleton only — registration functions added in Phases 10 and 11.
// The shim compiles cleanly and the extension loads unchanged.

// Forward declarations ensure the extern "C" boundary is visible to Rust
// even though the implementations are stubs in Phase 8.
extern "C" {
    void semantic_views_register_shim(void* /* db_instance_ptr */) {
        // Phase 8: intentional no-op. Proves the C++ compilation works and
        // the extern "C" boundary is correct. Phases 10/11 add logic here.
    }
}
```

**C header for the boundary:**
```c
// src/shim/shim.h
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// Called from Rust init_extension to wire up C++ hooks.
// Phase 8: no-op stub. Phases 10/11 add parser and pragma registration.
void semantic_views_register_shim(void* db_instance_ptr);

#ifdef __cplusplus
}
#endif
```

**Rust call site (src/lib.rs, inside the `extension` module):**
```rust
// Source: existing lib.rs pattern, extended with shim call
// Add to the extension module, gated by the extension feature already in place

#[cfg(feature = "extension")]
extern "C" {
    fn semantic_views_register_shim(db_instance_ptr: *mut std::ffi::c_void);
}

// In init_extension(), after the catalog is initialized:
// unsafe { semantic_views_register_shim(db_handle as *mut std::ffi::c_void); }
```

### Pattern 3: Symbol Visibility Control

**What:** Restrict the exported symbols of the cdylib to exactly the three DuckDB entry points. Without this, the Rust standard library symbols leak into the binary.

**When to use:** Extension builds only (when `--features extension` is active).

**The three symbols DuckDB expects (confirmed from PITFALLS.md research):**
1. `semantic_views_init_c_api` — already exported by lib.rs via `#[no_mangle] pub unsafe extern "C" fn`
2. `semantic_views_version` — exported by the extension footer metadata system (appended by the CI pipeline's `extension-ci-tools` Python post-build script)
3. `semantic_views_storage_init` — only needed if the extension uses DuckDB's storage system (this extension does not)

**Linux (ELF) — version script approach:**
```
# build/semantic_views.map
{
  global:
    semantic_views_init_c_api;
    semantic_views_version;
  local: *;
};
```

```rust
// In build.rs, after cc::Build::new()...compile():
if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let map_file = format!("{}/semantic_views.map", out_dir);
    std::fs::write(&map_file, "{\n  global:\n    semantic_views_init_c_api;\n    semantic_views_version;\n  local: *;\n};\n").unwrap();
    println!("cargo:rustc-link-arg=-Wl,--version-script={}", map_file);
}
```

**macOS — exported symbols list approach:**
```
# build/semantic_views.exp
_semantic_views_init_c_api
_semantic_views_version
```

```rust
if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let exp_file = format!("{}/semantic_views.exp", out_dir);
    std::fs::write(&exp_file, "_semantic_views_init_c_api\n_semantic_views_version\n").unwrap();
    println!("cargo:rustc-link-arg=-Wl,-exported_symbols_list,{}", exp_file);
}
```

**Windows:** The existing `#[no_mangle] pub unsafe extern "C" fn` with `__declspec(dllexport)` in the generated code handles visibility correctly for MSVC targets. No additional linker flags needed.

**Verification command:**
```bash
# On Linux — should return zero lines if visibility is correct:
nm -D target/release/libsemantic_views.so | grep ' T ' | grep -v 'semantic_views_'
# On macOS:
nm -gU target/release/libsemantic_views.dylib | grep ' T ' | grep -v '_semantic_views_'
```

### Anti-Patterns to Avoid

- **CMake-primary build:** Do not add `CMakeLists.txt` at the repo root or switch to `c_cpp.Makefile`. The `cc` crate handles all cross-compilation within Cargo. CMake would invalidate `just`, `cargo nextest`, and all CI workflows.
- **Fat shim:** Do not put parsing logic, DDL validation, or catalog operations in `shim.cpp`. The C++ side should be <100 lines of registration + forwarding. All logic lives in Rust.
- **Relying on `DEP_DUCKDB_LIB_DIR` for headers:** This env var is only set in bundled mode (`cargo test`), not in extension mode (`--no-default-features`). Always use the vendored copy.
- **Calling C++ catalog APIs from Rust via FFI:** DuckDB's C++ catalog types (`Schema`, `Catalog`) are not in `duckdb.h`. Their ABI changes across patch versions. Use SQL strings (the `pragma_query_t` pattern, Phases 10-11).
- **Unconditional `build.rs` C++ compilation:** Without the `CARGO_FEATURE_EXTENSION` guard, every `cargo test` run triggers C++ compilation, slowing developer feedback and adding toolchain requirements.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| C++ file compilation from Rust | Custom `cc` invocations or shell scripts | `cc` crate in `build.rs` | `cc` handles MSVC vs clang vs gcc flags, cross-compilation targets, `AR` archiver, `-fPIC`, debug vs release flags — all automatically |
| C++ stdlib linking on macOS vs Linux | Platform-conditional linker flags by hand | `cc` crate `.cpp(true)` | `cc` emits the correct `rustc-link-lib=c++` (macOS) or `rustc-link-lib=stdc++` (Linux) automatically |
| Finding DuckDB headers in extension mode | Runtime header discovery or download-at-build | Vendor `duckdb.hpp` at `duckdb_capi/` | Extension mode builds have no bundled DuckDB headers; vendoring is the only reliable approach |
| Cross-platform linker flags for symbol visibility | Conditional shell scripts | Inline in `build.rs` using `CARGO_CFG_TARGET_OS` | `build.rs` has access to all target info from Cargo; no external scripts needed |

**Key insight:** The `cc` crate eliminates nearly all C++ toolchain friction. The only project-specific work is header vendoring and symbol visibility control.

---

## Common Pitfalls

### Pitfall 1: C++ Header Absence in Extension Mode Builds

**What goes wrong:** `build.rs` tries to include DuckDB headers from `libduckdb-sys`'s `OUT_DIR` using the `DEP_DUCKDB_LIB_DIR` environment variable. This works during `cargo test` but fails during `make debug` / `make release` (which use `--no-default-features --features extension`), because in extension mode `libduckdb-sys` does NOT unpack its bundled source tarball. The headers do not exist.

**Why it happens:** Two different code paths in `libduckdb-sys`: the bundled feature unpacks `duckdb.tar.gz`; the loadable-extension feature does not. Verified against this project's actual `target/debug/build/libduckdb-sys-*/` directories.

**How to avoid:** Vendor `duckdb.hpp` at `duckdb_capi/duckdb.hpp`. Point `cc::Build.include()` at the vendored directory unconditionally. Never reference `DEP_DUCKDB_LIB_DIR` or `OUT_DIR` for C++ headers.

**Warning signs:** `error[E0425]: cannot find value 'DEP_DUCKDB_LIB_DIR'` or `fatal error: 'duckdb.hpp' file not found` during `make debug`.

---

### Pitfall 2: Rust Standard Library Symbol Leakage into the cdylib

**What goes wrong:** When `libsemantic_views.a` (a Rust staticlib) is linked into the shared library, the Rust compiler exports all `pub extern "C" fn` symbols AND the Rust standard library symbols (`_ZN3std...` mangled names) as globally visible ELF/Mach-O symbols. The final `.duckdb_extension` may export hundreds or thousands of symbols where only two or three are expected.

**Why it happens:** Cargo's `cdylib` crate type strips many symbols but not all when a Rust staticlib is involved. The linker needs explicit direction (version script on Linux, exported symbols list on macOS) to restrict the export table.

**How to avoid:** Add a `build.rs` step that generates and applies a version script (Linux) or exported symbols list (macOS) restricting exports to `semantic_views_init_c_api` and `semantic_views_version`. See the Symbol Visibility Control pattern above.

**Warning signs:** `nm -D libsemantic_views.duckdb_extension | grep ' T ' | wc -l` returns hundreds or thousands of lines. Extension binary is 5-10x larger than expected (~50KB for this extension; Rust stdlib symbols can add 2-5MB).

---

### Pitfall 3: Rust Panics Crossing the C++ Boundary

**What goes wrong:** A Rust function called from C++ (via `extern "C"`) panics. Under `extern "C"` ABI, this is undefined behavior — the panic unwind mechanism attempts to cross the FFI boundary into C++ stack frames, which is not supported. On Linux this typically causes `SIGABRT`; on macOS it may silently terminate the process; on Windows it causes a crash with exit code 0xC0000409 (STATUS_STACK_BUFFER_OVERRUN).

**Why it happens:** Phase 8's shim is a no-op so this cannot occur yet. However, the pattern must be established in Phase 8 so it is in place before Phases 10/11 add logic.

**How to avoid:** Wrap every `extern "C"` function body exported from Rust in `std::panic::catch_unwind`. Convert any caught panic to a return code. Never allow a Rust panic to propagate into C++ stack frames.

```rust
// Pattern to establish in Phase 8's ffi.rs, even for stub functions:
#[no_mangle]
pub extern "C" fn sv_stub_function() -> bool {
    std::panic::catch_unwind(|| {
        // actual logic here in later phases
        true
    }).unwrap_or(false)
}
```

**Warning signs:** `SIGABRT` or silent process termination on the first call through the C++ boundary. Crash shows no meaningful Rust backtrace.

---

### Pitfall 4: `-std=c++17` Flag Incompatibility on Windows MSVC

**What goes wrong:** `.flag("-std=c++17")` is a GCC/Clang flag. MSVC uses `/std:c++17`. Passing the GCC flag to MSVC causes a compilation warning (treated as error with `/WX`) or the flag is silently ignored, and C++17 features in `duckdb.hpp` fail to compile.

**Why it happens:** The `cc` crate passes flags literally to the compiler; it does not translate between GCC and MSVC flag syntax.

**How to avoid:** Use `.flag_if_supported("-std=c++17")` which probes the compiler before adding the flag. The `cc` crate will skip the flag on MSVC and leave the default C++ standard in place (MSVC defaults to C++14 in older versions but C++17 is the default in recent Visual Studio). Alternatively, check `CARGO_CFG_TARGET_ENV` and conditionally apply the correct flag.

**Warning signs:** Compilation errors on the Windows CI target involving C++17 syntax from `duckdb.hpp` (`std::optional`, structured bindings, `if constexpr`).

---

### Pitfall 5: duckdb.hpp Include Path Requires Subdirectory Headers

**What goes wrong:** `duckdb.hpp` is not truly self-contained — it `#include`s other headers from `duckdb/` subdirectories. When only `duckdb.hpp` is vendored but the `duckdb/` subdirectory headers are missing, compilation fails with a cascade of missing header errors.

**Why it happens:** The `libduckdb-src.zip` release archive contains `duckdb.hpp` as an amalgam of the header tree. The amalgam may OR may not include all necessary headers in-line depending on the DuckDB version.

**How to avoid:** Verify that `duckdb.hpp` from the v1.4.4 `libduckdb-src.zip` is truly amalgamated (all `#include` paths resolved in-line). If not, vendor the entire `duckdb/src/include/` directory tree (already available in `target/debug/build/libduckdb-sys-*/out/duckdb/src/include/`). The verified path in this project's build artifacts shows `duckdb.hpp` at `target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/include/duckdb.hpp` with subdirectory headers present at the same level.

**Warning signs:** `fatal error: 'duckdb/common/common.hpp' file not found` during shim compilation despite the `duckdb.hpp` include succeeding.

---

### Pitfall 6: Build System Inversion Temptation

**What goes wrong:** After struggling with header paths or symbol visibility, it becomes tempting to switch to CMake (which handles these issues differently). Switching to CMake-primary requires:
1. Changing Makefile includes from `rust.Makefile` to `c_cpp.Makefile`
2. Adding `CMakeLists.txt` with ExternalProject for Cargo
3. Updating all CI workflows to use CMake toolchain setup
4. Losing `cargo test`, `cargo nextest`, `just`, and `cargo-llvm-cov` as first-class tools

**Why it happens:** The CMake path is well-documented in DuckDB's C++ extension template, and there are more examples of it. The Cargo + `cc` path for a Rust-primary project with a C++ shim has fewer examples.

**How to avoid:** Commit to the decision before starting. The `cc` crate approach is correct and sufficient. STATE.md decision: "Build strategy is Cargo-primary with `cc` crate — never introduce CMakeLists.txt."

**Warning signs:** Temptation to add `CMakeLists.txt` appears when header paths are confusing. Resolve the header problem by vendoring correctly, not by switching build systems.

---

## Code Examples

Verified patterns from official sources:

### Complete `build.rs` for Phase 8

```rust
// Source: cc crate docs (docs.rs/cc/latest/cc/struct.Build.html),
// Cargo build script reference (doc.rust-lang.org/cargo/reference/build-script-examples.html),
// confirmed CARGO_FEATURE_* env var pattern from Cargo reference.
// build.rs — place at repository root

fn main() {
    // Only compile the C++ shim when building the loadable extension binary.
    // CARGO_FEATURE_EXTENSION is set by Cargo when `--features extension` is passed.
    // During `cargo test` (uses default/bundled feature), this block is skipped.
    if std::env::var("CARGO_FEATURE_EXTENSION").is_err() {
        return;
    }

    cc::Build::new()
        .cpp(true)
        .file("src/shim/shim.cpp")
        .include("duckdb_capi/")           // vendored duckdb.hpp directory
        .flag_if_supported("-std=c++17")   // safe on GCC/clang; skipped on MSVC
        .warnings(false)                   // suppress DuckDB's internal warnings
        .compile("semantic_views_shim");   // links static archive into cdylib

    // Symbol visibility: restrict exported symbols to DuckDB entry points only.
    // This prevents Rust stdlib symbols from leaking into the extension binary.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    match target_os.as_str() {
        "linux" => {
            let map_path = format!("{}/semantic_views.map", out_dir);
            std::fs::write(&map_path,
                "{\n  global:\n    semantic_views_init_c_api;\n    semantic_views_version;\n  local: *;\n};\n"
            ).expect("failed to write version script");
            println!("cargo:rustc-link-arg=-Wl,--version-script={}", map_path);
        }
        "macos" => {
            let exp_path = format!("{}/semantic_views.exp", out_dir);
            std::fs::write(&exp_path,
                "_semantic_views_init_c_api\n_semantic_views_version\n"
            ).expect("failed to write exported symbols list");
            println!("cargo:rustc-link-arg=-Wl,-exported_symbols_list,{}", exp_path);
        }
        _ => {
            // Windows: __declspec(dllexport) on the entry point function handles visibility.
        }
    }
}
```

### C++ Shim Skeleton (Phase 8)

```cpp
// Source: DuckDB 1.4.4 bundled headers — confirmed include paths via
// target/debug/build/libduckdb-sys-*/out/duckdb/src/include/
// src/shim/shim.cpp

#include "duckdb.hpp"
#include "duckdb/main/config.hpp"
#include "duckdb/parser/parser_extension.hpp"
#include "duckdb/function/pragma_function.hpp"
#include "shim.h"

using namespace duckdb;

extern "C" {

// Phase 8: no-op skeleton. Proves that:
// 1. duckdb.hpp and all C++ headers compile cleanly
// 2. The extern "C" boundary compiles and links correctly
// 3. The entry point can be called from Rust without UB
//
// Phases 10 and 11 add parser_extensions.push_back() and
// PragmaFunction registration here.
void semantic_views_register_shim(void* /* db_instance_ptr */) {
    // Intentional no-op. db_instance_ptr would be cast to DatabaseInstance*
    // in later phases: auto& db = *reinterpret_cast<DatabaseInstance*>(db_instance_ptr);
    // auto& config = DBConfig::GetConfig(db);
    // config.parser_extensions.push_back(...);  // Phase 11
}

} // extern "C"
```

### DuckDB C++ API — Key Types for Later Phases (Reference)

```cpp
// Source: DuckDB 1.4.4 headers — confirmed from bundled include directory
// duckdb/main/config.hpp — DBConfig access pattern (Phase 10/11)
auto& config = DBConfig::GetConfig(*reinterpret_cast<DatabaseInstance*>(db_ptr));
config.parser_extensions.push_back(my_parser_ext);   // Phase 11

// duckdb/parser/parser_extension.hpp — ParserExtension structure (Phase 11)
// typedef ParserExtensionParseResult (*parse_function_t)(ParserExtensionInfo *info, const string &query);
// typedef ParserExtensionPlanResult (*plan_function_t)(ParserExtensionInfo *info, ClientContext &context,
//                                                      unique_ptr<ParserExtensionParseData> parse_data);

// duckdb/function/pragma_function.hpp — PragmaFunction registration (Phase 10)
// typedef string (*pragma_query_t)(ClientContext &context, const FunctionParameters &parameters);
// PragmaFunction::PragmaStatement("name", my_pragma_query_fn)

// duckdb/main/extension/extension_loader.hpp — ExtensionLoader for function registration
// ExtensionLoader has a DatabaseInstance& — accessible via loader.GetDatabaseInstance()
// (Only available from within DUCKDB_CPP_EXTENSION_ENTRY, not from the C API entry point)
```

### Verification Commands

```bash
# 1. Verify cargo test still passes (no C++ involved):
cargo test

# 2. Verify extension build compiles with C++:
cargo build --no-default-features --features extension

# 3. Verify symbol count (macOS):
nm -gU target/debug/libsemantic_views.dylib | grep ' T '
# Expected: exactly 1 line: _semantic_views_init_c_api

# 4. Full CI build (Linux x86_64 target — via Makefile):
make debug

# 5. SQLLogicTest smoke test:
just test
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Rust extensions required C++ + CMake as primary build | `cc` crate in `build.rs` allows Cargo-primary with embedded C++ shim | DuckDB extension C API (v1.0.0+, 2024) | Pure-Rust extensions are now standard; small C++ shims can be added without CMake |
| Extensions registered via Connection object | Extensions registered via `ExtensionLoader` (newer C++ API) | DuckDB 1.4.x | FTS and other first-party extensions use `ExtensionLoader`; this project uses the C API entry point directly, which predates `ExtensionLoader` |
| `#[duckdb_entrypoint_c_api]` macro for entry point | Manual `extern "C" fn semantic_views_init_c_api` (already in this project) | Project v0.1.0 | Manual entry point captures `duckdb_database` handle before `Connection` wraps it — needed for independent query connection |

**Note on `ExtensionLoader`:** DuckDB 1.4.4 introduced `ExtensionLoader` (seen in `extension_loader.hpp`). This C++ class provides a higher-level API for registering functions. It is not available from the C extension API (`duckdb_extension_access`). This project's entry point is via `semantic_views_init_c_api` (C API), not via `DUCKDB_CPP_EXTENSION_ENTRY` (C++ macro). For Phase 8, the shim calls `semantic_views_register_shim` from the existing Rust entry point — we do NOT use `DUCKDB_CPP_EXTENSION_ENTRY`. This ensures the project stays on the C API path and avoids mixing two entry point mechanisms.

**Deprecated/outdated:**
- CMake-primary build for Rust extensions: Still used by some older extensions, but the `cc` crate + Cargo approach is now the recommended pattern for Rust-primary extensions with small C++ shims.

---

## Open Questions

1. **Is `duckdb.hpp` from the v1.4.4 release archive truly self-contained?**
   - What we know: The bundled `libduckdb-sys` source includes subdirectory headers alongside `duckdb.hpp` in `duckdb/src/include/`
   - What's unclear: Whether the `libduckdb-src.zip` release archive ships a single-file amalgam or requires the full header tree
   - Recommendation: If `duckdb.hpp` is not self-contained, vendor the entire `duckdb/src/include/` directory tree from the project's existing `target/debug/build/libduckdb-sys-*/out/duckdb/src/include/`. This directory is already present on the developer's machine. The `just update-headers` recipe should copy this tree rather than just `duckdb.hpp`.

2. **Symbol visibility on Windows: is it already correct?**
   - What we know: Windows cdylib exports are controlled by `__declspec(dllexport)` on individual functions; `DUCKDB_EXTENSION_API` in `duckdb.h` expands to `__declspec(dllexport)` on Windows
   - What's unclear: Whether the Rust compiler's Windows cdylib output leaks Rust std symbols without a `.def` file
   - Recommendation: Run `nm` (or `dumpbin /EXPORTS` on Windows) on a Windows CI artifact after the first build to verify the export table. If symbols leak, add a `.def` file restricting exports (analogous to the Linux version script).

3. **Does `semantic_views_register_shim` need to be called before or after existing function registration?**
   - What we know: Phase 8's shim is a no-op; order doesn't matter yet
   - What's unclear: Phases 10/11 will push to `config.parser_extensions` — this must happen before any user queries arrive, which is satisfied by calling it during `init_extension`
   - Recommendation: Call `semantic_views_register_shim` as the last step in `init_extension`, after all Rust functions are registered. This ensures the catalog is initialized before any C++ callback could fire.

---

## Sources

### Primary (HIGH confidence)

- `target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/include/duckdb/parser/parser_extension.hpp` — Confirmed `ParserExtension`, `parse_function_t`, `plan_function_t` structures in DuckDB 1.4.4
- `target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/include/duckdb/function/pragma_function.hpp` — Confirmed `pragma_query_t` type signature and `PragmaFunction::PragmaStatement` API
- `target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/include/duckdb/main/config.hpp` — Confirmed `vector<ParserExtension> parser_extensions` field on `DBConfig`
- `target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/include/duckdb/main/extension/extension_loader.hpp` — Confirmed `ExtensionLoader` C++ API including `DUCKDB_CPP_EXTENSION_ENTRY` macro
- `target/debug/build/libduckdb-sys-f84b3ff5ceb8e9ff/out/duckdb/src/include/duckdb_extension.h` — Confirmed `DUCKDB_EXTENSION_ENTRYPOINT` macros, `duckdb_ext_api_v1` struct, and `DUCKDB_EXTENSION_GLOBAL` pattern
- `/Users/paul/Documents/Dev/Personal/duckdb-semantic-views/Cargo.lock` — Confirmed `cc` version 1.2.56 already present as transitive dependency of `libduckdb-sys`
- `target/debug/libsemantic_views.dylib` + `nm -gU` — Confirmed current extension exports exactly one symbol: `_semantic_views_init_c_api` (debug/bundled build)
- `.planning/research/STACK.md` — First-party research confirming `cc = "1.2"`, vendoring approach, and build.rs pattern (HIGH — project-specific, verified against actual artifacts)
- `.planning/research/ARCHITECTURE.md` — First-party research confirming Cargo-primary approach, `src/shim/` layout, and Rust/C++ boundary design (HIGH — project-specific)
- `.planning/research/PITFALLS.md` — First-party research cataloguing P1.1 (build system inversion), P1.2 (Rust symbol leakage), P1.3 (TLS/unwinding), P6.1 (memory ownership) (HIGH — project-specific, confirmed from multiple sources)

### Secondary (MEDIUM confidence)

- [cc-rs docs.rs](https://docs.rs/cc/latest/cc/struct.Build.html) — `cpp()`, `include()`, `flag_if_supported()`, `warnings()`, `compile()` API confirmed
- [cc-rs GitHub (rust-lang/cc-rs)](https://github.com/rust-lang/cc-rs) — Current version 1.2.56 confirmed as of 2026-02-13
- [Cargo build script reference](https://doc.rust-lang.org/cargo/reference/build-script-examples.html) — `CARGO_FEATURE_*` env var pattern confirmed
- [DuckDB FTS extension analysis](https://github.com/duckdb/duckdb-fts) — Confirmed `PragmaFunction::PragmaCall` + `ExtensionLoader::RegisterFunction` pattern in a real C++ DuckDB extension

### Tertiary (LOW confidence — needs validation)

- Symbol visibility behavior on Windows MSVC without `.def` file — inferred from Rust `cdylib` documentation and PITFALLS.md; needs hands-on CI verification

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — `cc` crate confirmed in lock file; DuckDB headers confirmed in build artifacts
- Architecture: HIGH — `build.rs` pattern confirmed against Cargo docs; C++ header paths confirmed against live project build output; shim layout from ARCHITECTURE.md (first-party, HIGH)
- Pitfalls: HIGH for build mechanics and symbol leakage (confirmed from multiple sources); MEDIUM for Windows symbol visibility (needs CI verification)

**Research date:** 2026-03-01
**Valid until:** 2026-04-01 (DuckDB 1.4.4 is pinned; `cc` crate API is stable)
