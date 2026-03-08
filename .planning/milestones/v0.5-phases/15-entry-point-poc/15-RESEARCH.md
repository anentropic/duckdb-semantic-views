# Phase 15: Entry Point POC - Research

**Researched:** 2026-03-07
**Domain:** DuckDB C++ extension entry points, parser hook registration, Rust/C++ FFI via cc crate
**Confidence:** HIGH

## Summary

Phase 15 is a spike to determine whether switching from a C_STRUCT entry point (pure Rust) to a CPP entry point (C++ shim + Rust FFI) enables parser hook registration while preserving existing functionality. The research confirms this is achievable with high confidence.

The CPP entry point pattern is well-established across DuckDB's own extensions (json, parquet, icu, tpch, tpcds, delta, autocomplete) and community extensions (prql, duckpgq). The key insight is that DuckDB's extension loader uses completely separate code paths for CPP vs C_STRUCT ABI types, determined by the 512-byte footer metadata -- so there is no conflict between the two entry point symbols. The C++ shim compiles against the DuckDB v1.4.4 amalgamation header (`duckdb.hpp`, 1.8MB), which contains all necessary types (`ParserExtension`, `DBConfig`, `ExtensionLoader`, `ParserExtensionParseResult`, `ParserExtensionPlanResult`) as inline definitions, avoiding the `-fvisibility=hidden` blocker that killed the Phase 11 approach.

The cc crate handles C++ compilation in `build.rs` with straightforward configuration: `.cpp(true)`, `.std("c++17")`, `.include("cpp/include")`, `.file("cpp/src/shim.cpp")`. Feature-gating the cc compilation behind `CARGO_FEATURE_EXTENSION` ensures `cargo test` remains unaffected.

**Primary recommendation:** Try Option B first (CPP entry point via `DUCKDB_CPP_EXTENSION_ENTRY`). Change the footer ABI type from `C_STRUCT_UNSTABLE` to `CPP` in the Makefile. Export `semantic_views_duckdb_cpp_init` from the C++ shim. Delegate to Rust `init_extension()` via FFI.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Try Option B first (CPP entry via `DUCKDB_CPP_EXTENSION_ENTRY`, delegates to Rust via FFI) -- this is the proven pattern used by prql/duckpgq
- If Option B works and existing tests pass, record the decision and move on -- no need to also prove Option A
- If Option B fails, pivot to Option A (keep C_STRUCT, call C++ helper from Rust init) before declaring no-go
- Only declare no-go if both options fail
- Record decision in both phase verification report AND a dedicated `_notes/entry-point-decision.md` doc with full rationale
- STATE.md accumulated decisions updated as usual
- Vendor `duckdb.hpp` (amalgamation header only, ~1.8MB) in `cpp/include/duckdb.hpp`
- Header only -- no `duckdb.cpp` source file needed for the shim
- Keep header up to date via the existing DuckDB Version Monitor CI action (add a step to re-fetch the header when bumping versions)
- Shim source lives at `cpp/src/shim.cpp`
- This is a spike, not a release -- no new CI tests needed for Phase 15
- Verification: stub `parse_function` returns a no-op statement (e.g., `SELECT 'CREATE SEMANTIC VIEW stub fired'`) proving the full hook chain works: parse -> plan -> execute
- Existing functionality verified by running `just test-all` (full suite: Rust unit, sqllogictest, DuckLake CI)
- Phase 16 adds proper test coverage
- C++ shim compilation feature-gated: build.rs only compiles `shim.cpp` when `CARGO_FEATURE_EXTENSION` is set
- `cc` crate is an optional build-dependency, gated on the `extension` feature -- `cargo test` (bundled mode) never downloads or uses it
- Zero impact on existing developer workflow: `cargo test` remains pure Rust
- Clean break: rewrite init to the from-scratch design where C++ entry is the only DuckDB entry point
- C++ entry owns: DuckDB handshake, parser hook registration, calling Rust init
- Rust init owns: catalog setup, DDL function registration, query function registration (all existing logic)
- No legacy naming artifacts -- the Rust function was never an entry point, just an internal init called by C++
- Work on a feature branch, not main

### Claude's Discretion
- Exact FFI function signatures between C++ and Rust
- Connection lifetime management (how the C++ entry creates and passes the duckdb_connection to Rust)
- Symbol visibility list updates in build.rs (which symbols to export for CPP vs C_STRUCT)
- Error handling across the C++/Rust FFI boundary
- Whether to use `ExtensionLoader&` (modern API) or `DatabaseInstance&` (older API) in the C++ entry

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| ENTRY-01 | POC Option A -- keep `C_STRUCT` footer, Rust entry initializes normally, then calls a linked C++ function that registers parser hooks using the `duckdb_database` handle | Fallback option; research documents the architecture but recommends trying Option B first per user decision |
| ENTRY-02 | POC Option B -- switch to `CPP` footer, C++ entry via `DUCKDB_CPP_EXTENSION_ENTRY`, delegates to Rust init via FFI, Rust C API stubs are initialized | Primary path; research provides exact macro definition, symbol naming (`semantic_views_duckdb_cpp_init`), ExtensionLoader API, footer ABI type change, and complete code patterns |
| ENTRY-03 | Chosen strategy preserves all existing `semantic_view()` query functionality (existing sqllogictest suite passes) | Research confirms Rust init_extension() is called identically; only the entry point wrapper changes; `just test-all` validates |
| BUILD-01 | C++ shim compiles via `cc` crate against vendored DuckDB amalgamation header (`duckdb.hpp` v1.4.4) | Research confirms amalgamation header (1.8MB from libduckdb-src.zip) contains all needed types; cc crate configuration documented |
| BUILD-02 | Symbol visibility updated in `build.rs` to export the correct entry point symbol(s) for the chosen ABI strategy | Research provides exact symbol name (`semantic_views_duckdb_cpp_init`), macOS/Linux visibility patterns, and build.rs changes needed |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| DuckDB amalgamation header | v1.4.4 | `duckdb.hpp` -- provides all C++ types needed by the shim | Official DuckDB distribution; used by all CPP extensions |
| cc (Rust crate) | latest | Compiles `shim.cpp` from `build.rs` | Standard Rust crate for C/C++ compilation in build scripts |
| libduckdb-sys | =1.4.4 | Rust FFI bindings to DuckDB C API | Already a dependency; provides `duckdb_connection`, `duckdb_database` types |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| duckdb (Rust crate) | =1.4.4 | High-level Rust API (Connection, vtab) | Already a dependency; used by existing init_extension() |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Vendored `duckdb.hpp` | Extract from `libduckdb-sys` tarball at build time | More complex build.rs; header might not be accessible after crate extraction |
| `cc` crate | `cxx` crate | cxx generates safe bindings but adds complexity; cc is sufficient for a thin shim |
| Manual `extern "C"` entry | `duckdb_entrypoint_c_api` macro | The macro is for C_STRUCT ABI; CPP ABI requires `DUCKDB_CPP_EXTENSION_ENTRY` |

**Installation:**
```toml
# Cargo.toml additions
[build-dependencies]
cc = { version = "1", optional = true }

[features]
extension = ["duckdb/loadable-extension", "duckdb/vscalar", "dep:cc"]
```

## Architecture Patterns

### Recommended Project Structure
```
cpp/
  include/
    duckdb.hpp           # Vendored amalgamation header (1.8MB, v1.4.4)
  src/
    shim.cpp             # C++ entry point + parser hook stub (~40-60 lines)
src/
  lib.rs                 # Rust init + FFI exports
  ...                    # Existing Rust codebase unchanged
build.rs                 # Updated: cc crate compilation (feature-gated)
Cargo.toml               # Updated: cc as optional build-dep
Makefile                 # Updated: --abi-type CPP
```

### Pattern 1: CPP Entry Point with Rust FFI Delegation (Option B)

**What:** C++ owns the DuckDB entry point symbol. It registers parser hooks (which require C++ types), then delegates all other initialization to Rust via `extern "C"` FFI.

**When to use:** When the extension needs access to C++ APIs (like `ParserExtension`, `DBConfig`) that are not available through the DuckDB C API.

**Exact entry point symbol:** `semantic_views_duckdb_cpp_init`

The macro `DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader)` expands to:
```cpp
// Source: DuckDB v1.4.4 amalgamation header, line 50212
__attribute__((visibility("default"))) void semantic_views_duckdb_cpp_init(
    duckdb::ExtensionLoader &loader)
```

When wrapped in `extern "C"` (as all official DuckDB extensions do), the symbol has C linkage (no C++ name mangling).

**DuckDB loading flow for CPP ABI:**
```
1. dlopen(extension.duckdb_extension)
2. Read 512-byte footer -> abi_type = "CPP"
3. Construct symbol name: "semantic_views" + "_duckdb_cpp_init"
4. dlsym(dll, "semantic_views_duckdb_cpp_init")
5. Create ExtensionLoader(info)
6. Call init_fun(loader)
7. loader.FinalizeLoad()
```

**Example shim.cpp:**
```cpp
// Source: Pattern derived from DuckDB loadable_extension_demo.cpp
// and official extensions (json, parquet, tpch, etc.)
#include "duckdb.hpp"

using namespace duckdb;

// Forward-declare Rust init function
extern "C" {
    void sv_rust_init(duckdb_database db_handle);
}

// Stub parse function -- proves hook registration works
static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *, const string &query) {
    // For Phase 15 POC: just return DISPLAY_ORIGINAL_ERROR for everything
    // This means "I can't handle this query, let DuckDB try"
    return ParserExtensionParseResult();
    // Default constructor returns DISPLAY_ORIGINAL_ERROR
}

// Stub plan function -- required but won't be called in Phase 15
static ParserExtensionPlanResult sv_plan_stub(
    ParserExtensionInfo *, ClientContext &,
    unique_ptr<ParserExtensionParseData>) {
    throw InternalException("sv_plan_stub should not be called in Phase 15");
}

extern "C" {
DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader) {
    auto &db = loader.GetDatabaseInstance();

    // Register parser extension (stub for Phase 15)
    ParserExtension ext;
    ext.parse_function = sv_parse_stub;
    ext.plan_function = sv_plan_stub;
    auto &config = DBConfig::GetConfig(db);
    config.parser_extensions.push_back(ext);

    // Get raw database handle for Rust FFI
    // ExtensionLoader provides GetDatabaseInstance() which gives DatabaseInstance&
    // We need duckdb_database (C API handle) for Rust
    // duckdb_database is a typedef for DatabaseData* which wraps DatabaseInstance
    // Use reinterpret_cast to pass as opaque pointer
    sv_rust_init(reinterpret_cast<duckdb_database>(&db));
}
}
```

**Critical note on `duckdb_database` from `DatabaseInstance&`:** The C API's `duckdb_database` is a pointer to an internal struct containing a `shared_ptr<DatabaseInstance>`. Getting a `duckdb_database` from a `DatabaseInstance&` is NOT straightforward via cast. The recommended approach is to create a `DuckDB` wrapper or `Connection` object. See "Connection Lifetime Management" pattern below.

### Pattern 2: Connection Lifetime Management

**What:** The C++ entry point creates a `Connection` from the `DatabaseInstance`, extracts the `duckdb_connection` C API handle, and passes it to Rust. Rust uses this to register functions.

**Why it matters:** The existing `init_extension()` in Rust expects a `&Connection` (high-level duckdb-rs type) and a `duckdb_database` handle. The CPP entry receives `ExtensionLoader&` which gives `DatabaseInstance&`. We need to bridge between these.

**Recommended approach:**
```cpp
extern "C" {
DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader) {
    auto &db = loader.GetDatabaseInstance();

    // Register parser hooks (C++ only -- requires C++ types)
    ParserExtension ext;
    ext.parse_function = sv_parse_stub;
    ext.plan_function = sv_plan_stub;
    auto &config = DBConfig::GetConfig(db);
    config.parser_extensions.push_back(ext);

    // Create a connection for Rust init
    // Connection(db) creates a temporary connection to the database
    Connection conn(db);

    // Pass the raw connection handle to Rust
    // Rust will use this to register table functions, init catalog, etc.
    // The Connection object must stay alive for the duration of init
    sv_rust_init(/* needs design -- see discretion area */);
}
}
```

**Rust side:**
```rust
// Source: Existing pattern from src/lib.rs extension module

/// Called by C++ entry point to initialize all Rust-side functionality.
/// Receives an opaque database handle.
///
/// # Safety
/// Called across FFI boundary by C++ shim. db_handle must be valid.
#[no_mangle]
pub unsafe extern "C" fn sv_rust_init(/* params TBD */) {
    // Reuse existing init_extension() logic
    // - Init catalog
    // - Register DDL functions (create/drop/list/describe)
    // - Register query functions (semantic_view, explain_semantic_view)
    // All existing code in init_extension() is reusable
}
```

### Pattern 3: Stub Parser Hook Verification

**What:** The Phase 15 POC stub `parse_function` proves the hook registration chain works by handling a specific test query.

**Verification approach from CONTEXT.md:** The stub parse_function recognizes `CREATE SEMANTIC VIEW` prefix and returns a rewritten `SELECT` statement to prove the full chain: parse -> plan -> execute.

**How plan_function returns a query result:**
```cpp
// Source: DuckDB loadable_extension_demo.cpp (QuackPlanFunction)
static ParserExtensionPlanResult sv_plan_stub(
    ParserExtensionInfo *info, ClientContext &context,
    unique_ptr<ParserExtensionParseData> parse_data) {

    ParserExtensionPlanResult result;
    // Return a simple SELECT as a TableFunction
    result.function = TableFunction("sv_stub_result", {}, /* execute */, /* bind */);
    result.requires_valid_transaction = false;
    result.return_type = StatementReturnType::QUERY_RESULT;
    return result;
}
```

Alternatively, for the simplest possible POC, the parse_function can rewrite to:
```
SELECT 'CREATE SEMANTIC VIEW stub fired' AS result;
```
by returning a `ParserExtensionParseData` that the plan_function transforms into a table function returning that string.

### Pattern 4: Footer ABI Type Change

**What:** Change the metadata footer from `C_STRUCT_UNSTABLE` to `CPP`.

**Current Makefile:**
```makefile
USE_UNSTABLE_C_API=1
# This adds: --abi-type C_STRUCT_UNSTABLE
```

**Required change:**
```makefile
# Override the UNSTABLE_C_API_FLAG to use CPP instead
UNSTABLE_C_API_FLAG=--abi-type CPP
```

Or more cleanly:
```makefile
# Remove USE_UNSTABLE_C_API=1
# Add explicit ABI type
EXTENSION_ABI_TYPE=CPP
```

The `append_extension_metadata.py` script's `--abi-type` flag directly controls what goes in the footer. DuckDB's loader reads this and decides which symbol to look up:
- `CPP` -> `dlsym("semantic_views_duckdb_cpp_init")`
- `C_STRUCT` / `C_STRUCT_UNSTABLE` -> `dlsym("semantic_views_init_c_api")`

### Anti-Patterns to Avoid

- **Trying to get `duckdb_database` via cast from `DatabaseInstance&`:** The C API handle is a wrapper struct, not a direct pointer to `DatabaseInstance`. Don't use `reinterpret_cast`. Instead, create proper `Connection` objects or find the correct API path.

- **Keeping both `semantic_views_init_c_api` AND `semantic_views_duckdb_cpp_init`:** With CPP ABI footer, only `_duckdb_cpp_init` is called. Having both creates confusion. Remove or stub out the C API entry point.

- **Including `duckdb.cpp` in the build:** Only the header (`duckdb.hpp`) is needed. The shim only uses inline definitions and type declarations from the header. Compiling `duckdb.cpp` (24MB) would be unnecessary and extremely slow.

- **Using `ParserExtension::Register(config, ext)` on v1.4.4:** This static method does NOT exist in v1.4.4. It was added on `main` after v1.4.4. Use `config.parser_extensions.push_back(ext)` directly.

- **Assuming `parser_override` exists in v1.4.4:** It does NOT. The v1.4.4 `ParserExtension` class has only `parse_function` and `plan_function`. This is fine -- `parse_function` (fallback hook) is the correct hook for our use case anyway.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| C++ compilation from Rust | Custom gcc/clang invocation in build.rs | `cc` crate with `.cpp(true)` | Handles compiler detection, flag compatibility, cross-compilation |
| Extension entry point boilerplate | Manual `extern "C"` with `__attribute__` | `DUCKDB_CPP_EXTENSION_ENTRY` macro | Macro generates correct symbol name and visibility attributes |
| Footer metadata stamping | Custom binary footer writer | `append_extension_metadata.py` from extension-ci-tools | Already handles platform detection, version encoding, signature space |
| duckdb_connection from DatabaseInstance | Pointer casting / unsafe reinterpret | `Connection(db)` C++ constructor | Proper initialization, reference counting, connection lifecycle |

**Key insight:** The shim.cpp should be as thin as possible -- just the entry point, parser hook registration, and FFI bridge to Rust. All business logic stays in Rust.

## Common Pitfalls

### Pitfall 1: Amalgamation Header Size and Compile Time
**What goes wrong:** `duckdb.hpp` is 1.8MB (50,000+ lines). Compiling a `.cpp` file that includes it takes 15-30 seconds even for a trivial shim.
**Why it happens:** The amalgamation is the entire DuckDB codebase in one header.
**How to avoid:** Accept the compile time hit -- it only happens during `extension` feature builds, not `cargo test`. The cc crate caches the compiled object file, so incremental builds are fast unless shim.cpp or the header changes.
**Warning signs:** First full build takes significantly longer than before.

### Pitfall 2: Symbol Visibility Mismatch
**What goes wrong:** The extension loads but DuckDB can't find `semantic_views_duckdb_cpp_init`.
**Why it happens:** The symbol was either not exported (missing from visibility list) or was name-mangled (missing `extern "C"` wrapper).
**How to avoid:**
1. Wrap `DUCKDB_CPP_EXTENSION_ENTRY` in `extern "C" { }` (as all official extensions do)
2. Update `build.rs` symbol visibility lists: macOS `.exp` file gets `_semantic_views_duckdb_cpp_init`, Linux `.dynlist` gets `semantic_views_duckdb_cpp_init;`
3. Verify with `nm -gU` (macOS) or `nm -D` (Linux) after build
**Warning signs:** `LOAD` fails with "Extension entry point not found" or similar.

### Pitfall 3: Connection Lifetime in FFI
**What goes wrong:** Rust holds a `duckdb_connection` that refers to a `Connection` object that has been destroyed.
**Why it happens:** The `Connection conn(db)` in the C++ shim is a stack variable. When the entry function returns, `conn` is destroyed, invalidating any raw pointers passed to Rust.
**How to avoid:** Either (a) complete all Rust init within the C++ entry function's scope (before it returns), or (b) heap-allocate the Connection and ensure its lifetime extends beyond the shim.
**Warning signs:** Segfaults or use-after-free during function registration.

### Pitfall 4: Forgetting to Update Footer ABI Type
**What goes wrong:** Extension binary has `C_STRUCT_UNSTABLE` in its footer but exports `_duckdb_cpp_init`. DuckDB looks for `_init_c_api`, doesn't find it, fails to load.
**Why it happens:** Only updating the C++ code without updating the Makefile's `--abi-type` flag.
**How to avoid:** The Makefile change to `--abi-type CPP` must happen in the same commit as the entry point change.
**Warning signs:** `LOAD 'semantic_views'` fails silently or with "invalid extension" error.

### Pitfall 5: cc Crate Not Feature-Gated
**What goes wrong:** `cargo test` starts compiling C++ code, requiring the vendored `duckdb.hpp`, and fails because the header isn't found or compilation is slow.
**Why it happens:** `cc` crate not gated behind the `extension` feature in `Cargo.toml` or `build.rs`.
**How to avoid:** Make `cc` an optional dependency gated on `extension`. In `build.rs`, check `CARGO_FEATURE_EXTENSION` before any cc compilation. The existing pattern in `build.rs` already gates on this env var.
**Warning signs:** `cargo test` becomes dramatically slower or fails with missing header errors.

### Pitfall 6: C++ Standard Version
**What goes wrong:** Compilation fails with errors about `unique_ptr`, `make_uniq`, or other C++17 features.
**Why it happens:** `duckdb.hpp` requires at least C++17. The cc crate defaults to a lower standard.
**How to avoid:** Explicitly set `.std("c++17")` in the cc crate Build configuration.
**Warning signs:** Template-related compilation errors in `duckdb.hpp`.

## Code Examples

### build.rs -- cc Crate C++ Compilation (Feature-Gated)

```rust
// Source: Pattern from cc crate docs (docs.rs/cc/latest/cc/struct.Build.html)
// + existing build.rs symbol visibility pattern

fn main() {
    if std::env::var("CARGO_FEATURE_EXTENSION").is_err() {
        return;
    }

    // Compile the C++ shim against the vendored DuckDB amalgamation header
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include("cpp/include")      // Where duckdb.hpp lives
        .file("cpp/src/shim.cpp")
        .warnings(false)              // Suppress warnings from duckdb.hpp
        .compile("semantic_views_shim");

    // Symbol visibility (updated for CPP entry point)
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    match target_os.as_str() {
        "linux" => {
            let dynlist_path = format!("{out_dir}/semantic_views.dynlist");
            std::fs::write(
                &dynlist_path,
                "{\n  semantic_views_duckdb_cpp_init;\n};\n",
            )
            .expect("failed to write dynamic list");
            println!("cargo:rustc-link-arg=-Wl,--dynamic-list={dynlist_path}");
        }
        "macos" => {
            let exp_path = format!("{out_dir}/semantic_views.exp");
            std::fs::write(&exp_path, "_semantic_views_duckdb_cpp_init\n")
                .expect("failed to write macOS exported symbols list");
            println!("cargo:rustc-link-arg=-Wl,-exported_symbols_list,{exp_path}");
        }
        _ => {}
    }
}
```

### Cargo.toml -- cc as Optional Build Dependency

```toml
# Source: Existing Cargo.toml pattern + cc crate

[build-dependencies]
cc = { version = "1", optional = true }

[features]
extension = ["duckdb/loadable-extension", "duckdb/vscalar", "dep:cc"]
```

### Makefile -- ABI Type Change

```makefile
# Source: Existing Makefile + extension-ci-tools append_extension_metadata.py

# Remove: USE_UNSTABLE_C_API=1
# Override the UNSTABLE_C_API_FLAG to CPP
UNSTABLE_C_API_FLAG=--abi-type CPP
```

### Verifying Symbol Export After Build

```bash
# macOS: Check exported symbols
nm -gU target/debug/libsemantic_views.dylib | grep semantic_views

# Expected output (CPP entry):
# _semantic_views_duckdb_cpp_init

# Linux: Check dynamic symbols
nm -D target/debug/libsemantic_views.so | grep semantic_views

# Expected output (CPP entry):
# T semantic_views_duckdb_cpp_init
```

### ParserExtension API Types (from v1.4.4 amalgamation)

```cpp
// Source: DuckDB v1.4.4 duckdb.hpp lines 32912-32978

enum class ParserExtensionResultType : uint8_t {
    PARSE_SUCCESSFUL,
    DISPLAY_ORIGINAL_ERROR,
    DISPLAY_EXTENSION_ERROR
};

// Parse function receives the query string that DuckDB's parser couldn't handle
typedef ParserExtensionParseResult (*parse_function_t)(
    ParserExtensionInfo *info, const string &query);

// Plan function transforms parse_data into a TableFunction + parameters
typedef ParserExtensionPlanResult (*plan_function_t)(
    ParserExtensionInfo *info, ClientContext &context,
    unique_ptr<ParserExtensionParseData> parse_data);

class ParserExtension {
public:
    parse_function_t parse_function;    // Fallback: called when main parser fails
    plan_function_t plan_function;      // Converts parse result to executable plan
    shared_ptr<ParserExtensionInfo> parser_info;  // Optional context data
    // NOTE: no parser_override field in v1.4.4
    // NOTE: no Register() static method in v1.4.4
};

// Parse result: success carries parse_data, failure carries error or DISPLAY_ORIGINAL_ERROR
struct ParserExtensionParseResult {
    ParserExtensionParseResult()
        : type(ParserExtensionResultType::DISPLAY_ORIGINAL_ERROR) {}
    explicit ParserExtensionParseResult(string error_p)
        : type(ParserExtensionResultType::DISPLAY_EXTENSION_ERROR),
          error(std::move(error_p)) {}
    explicit ParserExtensionParseResult(unique_ptr<ParserExtensionParseData> parse_data_p)
        : type(ParserExtensionResultType::PARSE_SUCCESSFUL),
          parse_data(std::move(parse_data_p)) {}

    ParserExtensionResultType type;
    unique_ptr<ParserExtensionParseData> parse_data;
    string error;
    optional_idx error_location;
};

// Plan result: specifies the TableFunction to execute and its parameters
struct ParserExtensionPlanResult {
    TableFunction function;
    vector<Value> parameters;
    // ... additional fields for transaction/modification tracking
    bool requires_valid_transaction = true;
    StatementReturnType return_type = StatementReturnType::NOTHING;
};
```

### DuckDB Parse Fallback Flow

```cpp
// Source: DuckDB v1.4.4 src/parser/parser.cpp (simplified)
// When the main PostgreSQL parser fails on a statement:

for (auto &ext : options.extensions->ParserExtensions()) {
    if (!ext.parse_function) {
        continue;
    }
    auto result = ext.parse_function(ext.parser_info.get(), query_statement);

    if (result.type == ParserExtensionResultType::PARSE_SUCCESSFUL) {
        // Wrap in ExtensionStatement and continue
        auto statement = make_uniq<ExtensionStatement>(ext, std::move(result.parse_data));
        // ... set location info
        statements.push_back(std::move(statement));
        break;  // First matching extension wins
    } else if (result.type == ParserExtensionResultType::DISPLAY_EXTENSION_ERROR) {
        throw ParserException::SyntaxError(query, result.error, result.error_location);
    }
    // DISPLAY_ORIGINAL_ERROR: try next extension, or fall through to original error
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `ext_name_init(DatabaseInstance&)` | `DUCKDB_CPP_EXTENSION_ENTRY(name, loader)` with `ExtensionLoader&` | DuckDB v1.4.x | ExtensionLoader wraps DatabaseInstance; provides cleaner registration API; `FinalizeLoad()` called automatically |
| `ExtensionUtil::RegisterFunction()` | `loader.RegisterFunction()` or direct `DBConfig` access | DuckDB v1.4.x (ExtensionUtil removed) | ExtensionUtil deprecated per PR #17772; use ExtensionLoader methods or direct config manipulation |
| `config.parser_extensions.push_back()` | `ParserExtension::Register(config, ext)` | DuckDB main (NOT in v1.4.4) | Static method not available in v1.4.4; must use push_back directly |
| `USE_UNSTABLE_C_API=1` (C_STRUCT_UNSTABLE) | `--abi-type CPP` for C++ extensions | Always available | C_STRUCT/C_STRUCT_UNSTABLE for C API extensions; CPP for C++ extensions; determined by entry point strategy |

**Deprecated/outdated:**
- `ExtensionUtil` class: Removed in DuckDB main (PR #17772). v1.4.4 still has it but ExtensionLoader is the replacement path.
- `parser_override` field: Does NOT exist in v1.4.4's `ParserExtension` class. Only `parse_function` (fallback) and `plan_function` are available.
- `ParserExtension::Register()`: Does NOT exist in v1.4.4. Use `config.parser_extensions.push_back()`.

## Open Questions

1. **How to pass `duckdb_database` handle from C++ to Rust**
   - What we know: C++ entry receives `ExtensionLoader&` -> `GetDatabaseInstance()` -> `DatabaseInstance&`. Rust's `init_extension()` needs `duckdb_database` (C API handle) and `&Connection`.
   - What's unclear: The exact conversion from `DatabaseInstance&` to `duckdb_database` is not a simple cast. The C API wraps it in a struct.
   - Recommendation: Create a `Connection` in C++, get its `duckdb_connection`, and pass that to Rust. Rust can extract `duckdb_database` from the connection. Or refactor Rust's `init_extension()` to accept different parameters. This is a discretion area for the implementer.

2. **Rust C API stub initialization with CPP entry**
   - What we know: When using `C_STRUCT` ABI, `duckdb_rs_extension_api_init()` is called to initialize the C API function pointer table. With CPP ABI, this initialization path is skipped.
   - What's unclear: Whether Rust code that calls C API functions (e.g., `duckdb_connect`, `duckdb_query`) still works without the C API stub initialization. The `loadable-extension` feature replaces C API calls with function pointer stubs that must be initialized.
   - Recommendation: This is a critical unknown. The existing Rust code heavily uses the C API (via `libduckdb-sys`). If the stubs aren't initialized, all C API calls will crash. May need to manually call `duckdb_rs_extension_api_init()` from the C++ entry point, or restructure to avoid C API calls. **This is the highest-risk item for Phase 15.**

3. **Whether `loader.RegisterFunction()` works for all existing function registrations**
   - What we know: `ExtensionLoader` has `RegisterFunction(TableFunction)` methods. The existing Rust code uses `con.register_table_function_with_extra_info::<VTab, _>()` which is a duckdb-rs method.
   - What's unclear: Whether the duckdb-rs registration path works when entering via CPP (since the duckdb-rs `Connection` was never the entry point).
   - Recommendation: Keep using the duckdb-rs registration path -- it calls C API functions internally, which loops back to question 2. If C API stubs work, this works too.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest (Python runner) + cargo test (Rust) + DuckLake CI (Python) |
| Config file | `test/sql/TEST_LIST` (sqllogictest), `Cargo.toml` (Rust tests) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| ENTRY-01 | Option A fallback (C_STRUCT + C++ helper) | manual | Only attempted if Option B fails | N/A |
| ENTRY-02 | CPP entry loads, stub parser hook registered | smoke | `just build && duckdb -cmd "LOAD 'build/debug/semantic_views.duckdb_extension'"` | Wave 0 (manual verification in Phase 15, test in Phase 16) |
| ENTRY-03 | Existing semantic_view() queries work | integration | `just test-all` | Existing tests cover this |
| BUILD-01 | shim.cpp compiles via cc crate | build | `cargo build --no-default-features --features extension` | Implicit in build |
| BUILD-02 | Symbol visibility correct | smoke | `nm -gU target/debug/libsemantic_views.dylib \| grep duckdb_cpp_init` | Wave 0 (manual check) |

### Sampling Rate
- **Per task commit:** `cargo test` (fast, Rust-only)
- **Per wave merge:** `just test-all` (full suite)
- **Phase gate:** `just test-all` green + manual verification of parser hook stub

### Wave 0 Gaps
- [ ] No new automated tests required for Phase 15 (spike) per user decision
- [ ] Phase 16 will add sqllogictest coverage for parser hook behavior
- [ ] Manual verification: `LOAD` extension in DuckDB CLI, run `CREATE SEMANTIC VIEW test ...` to trigger stub

*(Phase 15 is a spike -- formal test coverage deferred to Phase 16 per user decision)*

## Sources

### Primary (HIGH confidence)
- DuckDB v1.4.4 amalgamation header (`duckdb.hpp`) -- verified `ParserExtension` class, `DUCKDB_CPP_EXTENSION_ENTRY` macro, `DBConfig`, `ExtensionLoader`, all at exact line numbers
- [DuckDB extension loading source](https://github.com/duckdb/duckdb/blob/main/src/main/extension/extension_load.cpp) -- confirmed CPP vs C_STRUCT symbol resolution paths (`<name>_duckdb_cpp_init` vs `<name>_init_c_api`)
- [DuckDB loadable_extension_demo.cpp](https://github.com/duckdb/duckdb/blob/main/test/extension/loadable_extension_demo.cpp) -- complete working example of ParserExtension with parse_function, plan_function, ParserExtensionParseData subclass
- [DuckDB extension_loader.hpp](https://github.com/duckdb/duckdb/blob/main/src/include/duckdb/main/extension/extension_loader.hpp) -- ExtensionLoader class definition, DUCKDB_CPP_EXTENSION_ENTRY macro definition
- [DuckDB parser.cpp](https://github.com/duckdb/duckdb/blob/main/src/parser/parser.cpp) -- parse_function fallback loop, ExtensionStatement creation
- [cc crate docs](https://docs.rs/cc/latest/cc/struct.Build.html) -- Build::cpp(), std(), include(), file(), compile() methods

### Secondary (MEDIUM confidence)
- [DuckDB extension-ci-tools](https://github.com/duckdb/extension-ci-tools) -- `append_extension_metadata.py` `--abi-type` flag, Makefile patterns
- [prql extension source](https://github.com/ywelsch/duckdb-prql) -- Real-world CPP extension with parser hooks
- [duckpgq extension source](https://github.com/cwida/duckpgq-extension) -- `DUCKDB_CPP_EXTENSION_ENTRY(duckpgq, loader)` pattern
- [DeepWiki DuckDB Extension Loading](https://deepwiki.com/duckdb/duckdb/4.3-extension-loading-and-installation) -- ABI type classification and loading flow
- Project investigation notes (`_notes/parser-extension-investigation.md`) -- Phase 11 post-mortem and proposed architecture

### Tertiary (LOW confidence)
- `DatabaseInstance&` to `duckdb_database` conversion -- no authoritative source found; recommended approach is theoretical (flagged as Open Question 1)
- C API stub initialization under CPP ABI -- no documentation found on whether `duckdb_rs_extension_api_init()` can be called from CPP entry (flagged as Open Question 2)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries verified, versions pinned, patterns confirmed from official DuckDB source
- Architecture: HIGH -- CPP entry pattern verified across 10+ official DuckDB extensions; macro definition, symbol naming, and loading path confirmed from source
- Pitfalls: HIGH -- amalgamation header size measured (1.8MB), v1.4.4 API differences verified (no `Register()`, no `parser_override`), symbol visibility patterns confirmed
- FFI bridge (C++ to Rust): MEDIUM -- the pattern is sound but the exact `duckdb_database` / C API stub initialization question needs empirical validation (this is what makes Phase 15 a spike)

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable -- DuckDB v1.4.4 is a pinned version)
