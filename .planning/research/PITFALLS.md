# PITFALLS -- Parser Extension Spike (v0.5.0)

**Domain:** Adding DuckDB parser extension hooks to an existing Rust DuckDB extension
**Researched:** 2026-03-07
**Context:** The existing extension is pure Rust with a C API entry point (`semantic_views_init_c_api`). v0.5.0 adds a C++ shim compiled against the DuckDB amalgamation (`duckdb.hpp`) to register parser hooks, switching the extension ABI from `C_STRUCT` to `CPP`.

---

## Critical Pitfalls

Mistakes that cause crashes, link failures, or require rearchitecting.

### P1: ODR (One Definition Rule) Violations -- Two Copies of DuckDB Static Globals

**What goes wrong:**
When `shim.cpp` is compiled against the DuckDB amalgamation (`duckdb.hpp` / `duckdb.cpp`), the resulting object code contains its own copies of DuckDB's inline functions, template instantiations, and -- critically -- static local variables. At runtime, the extension binary contains one copy of these, and the host DuckDB binary contains another. The extension is loaded via `dlopen` with `RTLD_NOW | RTLD_LOCAL` (confirmed in DuckDB's extension loading code), which means:

1. **RTLD_LOCAL prevents symbol interposition**: The extension's copies of DuckDB classes are isolated from the host's copies. They do not collide at the symbol level -- each is resolved within its own translation unit.

2. **Static globals are duplicated**: Any `static` variable inside an inline function (e.g., singleton registries, thread-local storage) will exist in two independent copies -- one in the host, one in the extension. Modifying one does not affect the other.

3. **This is actually the intended architecture**: DuckDB's PR #3783 ("Extension loading by statically linking DuckDB") confirmed that each extension gets its own statically-linked copy of DuckDB. The `RTLD_LOCAL` flag ensures isolation. The parser extension mechanism works because the extension writes to the host's `DBConfig::parser_extensions` vector via inlined accessors that operate on the host-owned `DatabaseInstance` reference passed to the entry point -- not on an extension-local copy.

**Why it works despite ODR concerns:**
The key insight is that `DUCKDB_CPP_EXTENSION_ENTRY(semantic_views, loader)` receives an `ExtensionLoader&` reference to the **host's** object. When the extension calls `loader.GetDatabaseInstance()`, it gets a reference to the host's `DatabaseInstance`. The inlined `DBConfig::GetConfig(db)` compiles into the extension binary but operates on the host's memory. The function body is duplicated (ODR-technically), but it reads/writes the correct host-side data structure because the `this` pointer (or reference) points into the host's address space.

**Where it can still break:**
- **Exception type identity**: If the extension throws a DuckDB exception type (e.g., `ParserException`) and the host tries to catch it, the RTTI type_info objects may not match because they come from different copies. Exceptions must NOT cross the extension/host boundary through DuckDB C++ types. Use error return values or the C API error mechanism.
- **std::string across boundary**: If the extension's copy of `std::string` uses a different allocator or small-string optimization layout than the host's (compiler version mismatch), passing a `std::string` by value across the boundary corrupts memory. Pass `const char*` across the boundary instead.

**Consequences:** Silent memory corruption if exception types or std::string cross the boundary. Difficult to diagnose -- manifests as intermittent crashes or wrong data.

**Prevention:**
- The C++ shim must be kept minimal (~30-50 lines). All logic lives in Rust.
- Never throw C++ exceptions from the shim. Use `try/catch` at every boundary.
- Never pass `std::string` by value across the extension/host boundary. Pass `const char*` and copy immediately.
- The `RTLD_LOCAL` isolation means extension-local DuckDB statics are harmless as long as you only interact with the host's `DatabaseInstance` through the reference provided at init.
- **Confidence:** MEDIUM. Based on DuckDB PR #3783 analysis, dlopen documentation, and C++ ODR/RTTI semantics. Confirmed by prql/duckpgq existence proofs (they compile against amalgamation and work). The exception/string boundary risk is standard C++ FFI knowledge.

**Phase assignment:** Validate in the first build phase. Write a smoke test that loads the extension and runs a basic query.

---

### P2: Dual Entry Point Conflict -- Both `_init` and `_init_c_api` Exported

**What goes wrong:**
The current extension exports `semantic_views_init_c_api` (Rust-generated, C API entry point). Adding the C++ shim introduces `semantic_views_init` (C++ entry point via `DUCKDB_CPP_EXTENSION_ENTRY`). If both are exported, DuckDB's loader behavior depends on the ABI type stamped in the extension footer:

- **Footer says `CPP`**: DuckDB looks up `semantic_views_init` (the C++ symbol). If found, it calls it. `semantic_views_init_c_api` is ignored. This is the correct path.
- **Footer says `C_STRUCT` or `C_STRUCT_UNSTABLE`**: DuckDB looks up `semantic_views_init_c_api`. It calls the Rust entry point. The C++ init never runs. Parser hooks are never registered.

The footer ABI type is deterministic -- it is set by the `append_extension_metadata.py` script's `--abi-type` argument. Currently the Makefile passes `C_STRUCT_UNSTABLE` (via `USE_UNSTABLE_C_API=1`). After v0.5.0, this must change to `CPP`.

**But there is a deeper problem:**
If the footer says `CPP` and `semantic_views_init` is called, the Rust code's `duckdb_rs_extension_api_init` is never called. This function populates the `AtomicPtr`-based function pointer table that `libduckdb-sys` uses for all C API calls (`ffi::duckdb_query`, `ffi::duckdb_connect`, etc.). Without this initialization, **every Rust C API call is a null pointer dereference**.

The C++ entry point receives an `ExtensionLoader&` (or `DatabaseInstance&`), not the `duckdb_extension_info` + `duckdb_extension_access` pair that `duckdb_rs_extension_api_init` expects. The C++ shim must somehow initialize the Rust function pointer table before calling any Rust code that uses `ffi::*`.

**Consequences:** If the function pointer table is not initialized: immediate SEGFAULT on the first `ffi::duckdb_query` call from Rust. If the footer ABI type is wrong: parser hooks silently not registered, no error.

**Prevention:**
- Switch the footer ABI type from `C_STRUCT_UNSTABLE` to `CPP` by changing the Makefile: remove `USE_UNSTABLE_C_API=1`, add `ABI_TYPE_FLAG=--abi-type CPP`. Update `UNSTABLE_C_API_FLAG` usage.
- The C++ entry point must obtain the `duckdb_extension_access` struct and call `duckdb_rs_extension_api_init` to populate Rust's function pointer table BEFORE calling any Rust init code. This requires the CPP entry point to access the C API initialization mechanism -- investigate whether `ExtensionLoader` provides access to the `duckdb_extension_access` pointer.
- **Alternative approach (simpler):** Keep the footer as `C_STRUCT_UNSTABLE`. Export only `semantic_views_init_c_api`. Have the Rust entry point call into a C++ function (`sv_register_parser_hooks`) that receives the raw `duckdb_database` handle, casts it to `DatabaseInstance*`, and registers the parser hooks. This avoids the dual-entry-point problem entirely. The C++ function is compiled against the amalgamation and linked into the extension, but the entry point remains Rust-owned.

  **Risk with the alternative:** The cast from `duckdb_database` (C API opaque handle) to `DatabaseInstance*` requires knowing the internal layout of the C API wrapper. The existing codebase already does something similar in `lib.rs` line 477: `let db_handle: ffi::duckdb_database = *(*access).get_database.unwrap()(info);` -- but this returns a C API handle, not a C++ reference. The shim would need to dereference the C API handle to get the underlying `DatabaseInstance&`. This is fragile across DuckDB versions.

- **Recommended approach:** Use the CPP entry point (`DUCKDB_CPP_EXTENSION_ENTRY`), which receives `ExtensionLoader&`. From within the C++ entry, obtain a `duckdb_connection` handle using the C++ `Connection` class, then pass it to Rust's init function. The Rust init function must NOT rely on `duckdb_rs_extension_api_init` for its function pointer table -- instead, it must use the raw `duckdb_database` handle obtained from the C++ side to call `ffi::duckdb_connect`, `ffi::duckdb_query`, etc. directly through the amalgamation-linked copies in the extension binary.

  **Wait -- this is the ODR problem again.** If Rust calls `ffi::duckdb_query` through the function pointer table, those pointers must be initialized. But the function pointer table is populated by `duckdb_rs_extension_api_init`, which is only called by DuckDB's C_STRUCT/C_STRUCT_UNSTABLE loading path. Under CPP ABI, this function is never called.

  **Resolution:** The Rust code must either (a) not use `ffi::duckdb_*` stubs at all (call DuckDB through the C++ amalgamation instead -- but this means rewriting all FFI), or (b) manually call `duckdb_rs_extension_api_init` from the C++ entry point by obtaining the `duckdb_extension_access` struct. Option (b) is feasible: `ExtensionLoader` wraps the same `duckdb_extension_info` handle, and the `duckdb_extension_access` is obtainable through it.

- **Simplest viable path:** Investigate whether `duckdb_rs_extension_api_init` can be called from the C++ entry point. The function signature is `fn(info: duckdb_extension_info, access: *const duckdb_extension_access, min_version: &str) -> Result<bool, ...>`. The C++ `ExtensionLoader` must expose the underlying `info` and `access` handles. If it does, the C++ shim calls `duckdb_rs_extension_api_init` first, then calls the Rust init, then registers parser hooks.

- **Confidence:** LOW. The interaction between CPP entry point and Rust function pointer initialization is not documented. This is the single most dangerous pitfall in v0.5.0 and needs a spike/POC before committing to an approach.

**Phase assignment:** Must be resolved in the very first phase. A POC that loads the extension, initializes both the C++ parser hooks and the Rust function pointer table, and successfully runs a `semantic_view()` query is the minimum viable proof.

---

### P3: C++ ABI Compatibility -- Compiler Version Mismatch

**What goes wrong:**
The CPP ABI type requires that the extension is compiled with the same (or ABI-compatible) compiler as the host DuckDB binary. DuckDB's CPP ABI enforces **exact DuckDB version match** (unlike C_STRUCT which allows cross-version compatibility). But beyond version, the C++ ABI also depends on:

1. **Compiler identity (GCC vs Clang vs MSVC):** Different compilers may use different name mangling, vtable layouts, and exception handling mechanisms. The extension's C++ objects (e.g., `ParserExtension`) must be layout-compatible with the host's.

2. **libstdc++ vs libc++ (Linux/macOS):** On macOS, the system uses `libc++`. On Linux, distributions vary between `libstdc++` (GCC) and `libc++` (Clang). If the extension is compiled with `libc++` but the host DuckDB uses `libstdc++`, passing C++ standard library types across the boundary (especially `std::string`, `std::vector`) will corrupt memory.

3. **GCC 5 std::string ABI break:** GCC 5 changed `std::basic_string` to use small-string optimization. Binaries compiled with GCC < 5 and GCC >= 5 have incompatible `std::string` layouts. Modern systems (GCC >= 5) are past this, but it is a CI concern if building on older toolchains.

**Why this is manageable for this project:**
- The CI uses GitHub Actions runners with known compiler versions.
- The DuckDB community extension CI builds extensions with the same toolchain used to build the DuckDB release.
- The CPP ABI's "exact version match" requirement already forces version-pinning.
- The C++ shim is ~30-50 lines and only passes `const char*` and simple structs across boundaries -- not C++ standard library types.

**Consequences:** If compilers mismatch: immediate crash on first call to any C++ function that passes objects by value. Hard to diagnose -- may look like a memory corruption bug.

**Prevention:**
- Pin the compiler in CI to match the DuckDB release toolchain. For community extensions, DuckDB provides the toolchain via `extension-ci-tools`.
- Never pass `std::string`, `std::vector`, or other C++ standard library types by value across the extension/host boundary. Use `const char*` and `size_t`.
- The `ParserExtension` struct itself is a DuckDB type that both sides agree on (same header, same version). This is safe because the extension and host are built against the same DuckDB version.
- Test on all target platforms in CI. The most common failure mode is "works on macOS (Clang/libc++), breaks on Linux (GCC/libstdc++)".
- **Confidence:** HIGH. C++ ABI compatibility rules are well-established. The mitigation (minimal C++ surface, pinned compiler) is standard practice.

**Phase assignment:** CI setup phase. Ensure the build matrix matches the DuckDB release toolchain.

---

### P4: Rust Function Pointer Table Not Initialized Under CPP Entry

**What goes wrong:**
This is the specific mechanism behind P2's deeper problem. When `duckdb-rs` compiles with `--features loadable-extension`, it replaces all C API function symbols (`duckdb_query`, `duckdb_connect`, `duckdb_open`, etc.) with stub functions that read from global `AtomicPtr` variables. These `AtomicPtr`s are initialized during `duckdb_rs_extension_api_init`, which extracts function pointers from the `duckdb_extension_access` struct passed by DuckDB.

Under the CPP ABI loading path:
1. DuckDB calls `semantic_views_init(DatabaseInstance& db)` (or `DUCKDB_CPP_EXTENSION_ENTRY`)
2. This entry point receives `ExtensionLoader&`, NOT `duckdb_extension_info` + `duckdb_extension_access`
3. `duckdb_rs_extension_api_init` is never called
4. All `AtomicPtr`s remain null
5. The first Rust call to `ffi::duckdb_query(conn, sql, &mut result)` dereferences a null function pointer
6. SEGFAULT

**What specifically breaks:**
- `ffi::duckdb_query` -- used by `execute_sql_raw` in table function execution
- `ffi::duckdb_connect` -- used in `init_extension` to create persist_conn and query_conn
- `ffi::duckdb_data_chunk_get_vector` -- used in zero-copy vector reference pipeline
- Every other `ffi::duckdb_*` call in the extension

**Consequences:** Immediate crash. The extension cannot function at all.

**Prevention:**
- **Option A (preferred):** Keep the C_STRUCT_UNSTABLE footer. Keep `semantic_views_init_c_api` as the entry point. The Rust entry point calls `duckdb_rs_extension_api_init` as it does today. After Rust init is complete, call a C++ helper function `sv_register_parser_hooks(db_handle)` that receives the raw `duckdb_database` handle, extracts `DatabaseInstance*`, and registers parser hooks. The challenge: the `duckdb_database` to `DatabaseInstance*` cast is an implementation detail of DuckDB's C API wrapper.

- **Option B:** Use CPP entry point. Within the C++ entry, call `duckdb_rs_extension_api_init` by extracting the `duckdb_extension_info` and `duckdb_extension_access` from the `ExtensionLoader`. This requires investigating whether `ExtensionLoader` exposes these handles. If it does not, this option is blocked.

- **Option C (most invasive):** Replace all Rust `ffi::duckdb_*` calls with calls through a manually managed function pointer table that is initialized from the C++ side. The C++ shim would pass the host's DuckDB function addresses to Rust via a struct. This is effectively reimplementing what `duckdb_rs_extension_api_init` does, but controlled by C++ instead of the C API loading path. This is a large refactor.

- **Option D (pragmatic):** Use CPP entry. In the C++ shim, create a `duckdb_database` C API handle from the `DatabaseInstance&` (DuckDB provides `duckdb_database_create_from_existing` or similar), then call the Rust `semantic_views_init_c_api` function directly, passing the handle through the same C API initialization path. This makes the Rust code think it was loaded via the C API, but the actual entry point is C++. **This needs verification that such a bridge function exists in the DuckDB C API.**

- **Confidence:** HIGH that this is a real problem. LOW confidence on which option is viable -- requires a POC spike.

**Phase assignment:** First phase. This is a go/no-go blocker.

---

### P5: Extension Footer ABI Type Must Change

**What goes wrong:**
The extension footer's `abi_type` field determines which entry point DuckDB looks up. The current build stamps `C_STRUCT_UNSTABLE` (line 218-219 of `base.Makefile`). If the v0.5.0 extension exports a C++ entry point (`semantic_views_init`) but the footer still says `C_STRUCT_UNSTABLE`, DuckDB will look for `semantic_views_init_c_api` instead. The C++ parser hook registration code in `semantic_views_init` will never execute.

**The `append_extension_metadata.py` script supports CPP:**
Line 45 of the script: `arg_parser.add_argument('--abi-type', ..., default='C_STRUCT')`. Passing `--abi-type CPP` sets the correct footer.

**But `cargo-duckdb-ext-tools` does NOT support CPP:**
The Rust-only packaging tool only supports `C_STRUCT` and `C_STRUCT_UNSTABLE`. If the build pipeline uses `cargo-duckdb-ext-tools` anywhere, it cannot produce a CPP-stamped extension.

**Consequences:** Parser hooks silently not registered. `CREATE SEMANTIC VIEW` fails with a standard DuckDB parse error. No error message indicates the entry point was wrong.

**Prevention:**
- Update the Makefile: replace `USE_UNSTABLE_C_API=1` with a direct `--abi-type CPP` flag in the `append_extension_metadata.py` invocation.
- Also pass `--duckdb-version` with the exact pinned version (required for CPP ABI, which enforces exact version match).
- Remove any `cargo-duckdb-ext-tools` usage from the build pipeline. Use the Python script exclusively for footer stamping.
- **If staying with Option A from P4 (C_STRUCT_UNSTABLE footer):** No footer change needed. The Rust entry point is called normally, and the C++ parser hook registration happens via a helper function called from Rust. This is the simplest path if it works.
- Add a CI test: after building, verify the footer ABI type matches expectations. A simple `python3 -c "f=open('ext.duckdb_extension','rb'); f.seek(-534,2); ..."` script can check the footer fields.
- **Confidence:** HIGH. The footer mechanism is well-documented and the Python script supports CPP. The risk is forgetting to change the flag.

**Phase assignment:** Build system phase.

---

## Moderate Pitfalls

### P6: Symbol Visibility -- macOS vs Linux Differences for CPP Entry

**What goes wrong:**
The current `build.rs` exports only `_semantic_views_init_c_api` on macOS (via `-exported_symbols_list`) and `semantic_views_init_c_api` on Linux (via `--dynamic-list`). Switching to CPP ABI requires exporting `semantic_views_init` instead (or additionally).

The C++ entry point generated by `DUCKDB_CPP_EXTENSION_ENTRY` is an `extern "C"` function, so it has C linkage (no name mangling). But its exact symbol name depends on the macro expansion -- it may be `semantic_views_init` or `semantic_views_duckdb_cpp_init` or similar, depending on the DuckDB version's macro definition.

**Platform-specific gotchas:**
- **macOS:** `-exported_symbols_list` requires underscore prefix (`_semantic_views_init`). Missing the underscore silently hides the symbol.
- **Linux:** `--dynamic-list` does not use underscore prefix. Additionally, the existing `build.rs` uses `--dynamic-list` specifically because Linux's `rustc` already generates a `--version-script` for cdylib targets, and GNU ld rejects two version scripts with "anonymous version tag cannot be combined." The `--dynamic-list` cooperates with rustc's version script.
- **Windows:** No change needed -- `__declspec(dllexport)` handles visibility.

**The cc crate complication:** When `shim.cpp` is compiled via the `cc` crate and linked into the Rust cdylib, the C++ object is part of the same link unit. The `build.rs` symbol visibility rules apply to ALL exported symbols, including those from the C++ object. If `build.rs` only exports `semantic_views_init_c_api`, the C++ `semantic_views_init` function is hidden. The extension loads, but DuckDB cannot find the C++ entry point.

**Prevention:**
- Update `build.rs` to export both `semantic_views_init_c_api` (Rust) and `semantic_views_init` (C++) -- or only the one that matches the footer ABI type.
- Determine the exact symbol name from the `DUCKDB_CPP_EXTENSION_ENTRY` macro expansion before writing the export list. Check with `nm -gU` on the built binary.
- On macOS: add `_semantic_views_init` to the exported symbols file.
- On Linux: add `semantic_views_init` to the dynamic list file.
- Add a post-build verification step in CI: `nm -gU *.duckdb_extension | grep semantic_views_init` must show the expected symbol(s).
- **Confidence:** HIGH. Build system change, well-understood mechanism. The existing `build.rs` already handles this for `semantic_views_init_c_api`.

**Phase assignment:** Build system phase, same as P5.

---

### P7: Thread Safety of `parse_function` Callback

**What goes wrong:**
DuckDB's parser can be invoked from multiple threads concurrently. Each connection has its own parser, and DuckDB supports multiple concurrent connections. The `parse_function` callback registered via `cfg.parser_extensions.push_back(ext)` may be called from any thread at any time.

If the Rust `sv_parse` function (called from the C++ trampoline) accesses shared mutable state -- e.g., the in-memory catalog (`Arc<RwLock<HashMap>>`) -- without proper synchronization, data races occur.

**DuckDB's threading model:**
- DuckDB uses a single writer, multiple reader model with MVCC
- Multiple connections can parse queries simultaneously
- The parser itself has no global lock -- each connection parses independently
- `parse_function` receives the query string as `const std::string&` -- this is thread-safe (read-only)
- `parse_function` must return a `ParserExtensionParseResult` -- this is a per-call allocation, no shared state

**For this extension specifically:**
The `parse_function` only needs to: (1) check if the query starts with `CREATE SEMANTIC VIEW`, (2) if yes, extract the view name and parameter text. It does NOT need to access the catalog, execute SQL, or modify any shared state. The actual DDL execution happens later in `plan_function` / the table function execution path, which already handles concurrency through the existing `Arc<RwLock<...>>` catalog.

**Where it can still break:**
- If `sv_parse` in Rust allocates a `String` and returns a `*const c_char` pointer to C++, the `String` must outlive the C++ code's use of the pointer. If the `String` is stack-allocated in the Rust function and the C++ code stores the pointer, use-after-free.
- If the Rust parse function panics, it must not unwind through the C++ stack (undefined behavior under `extern "C"`).

**Prevention:**
- The `sv_parse` function must be pure: no shared mutable state, no global variables, no logging that writes to shared buffers.
- Wrap the Rust parse function body in `std::panic::catch_unwind`. Return an error result if a panic occurs.
- Allocate the `ExtensionStatement` (or equivalent return data) on the heap. Transfer ownership to C++ explicitly. Do not return pointers to stack-allocated Rust data.
- Mark the function `unsafe extern "C"` with a safety comment documenting: "Called from any DuckDB parser thread. Must be reentrant and panic-safe."
- **Confidence:** HIGH. Standard Rust FFI thread safety requirements. The parse function's stateless nature makes this straightforward.

**Phase assignment:** Parser hook implementation phase. Establish the pattern in the first `sv_parse` implementation.

---

### P8: Memory Ownership of ExtensionStatement Across FFI

**What goes wrong:**
When `parse_function` succeeds, it must return a `ParserExtensionParseResult` containing a `unique_ptr<ExtensionStatement>`. The `ExtensionStatement` carries the parsed information (statement type, original SQL text, extension-specific data).

The ownership chain is: Rust parses the SQL -> creates result data -> passes to C++ shim -> C++ wraps in `ExtensionStatement` -> transfers via `unique_ptr` to DuckDB. DuckDB owns the `ExtensionStatement` and frees it when done.

**Memory management hazards:**
1. **Rust String -> C++ std::string:** If Rust's `sv_parse` returns a `*const c_char` pointing to the parsed statement text, the C++ trampoline must copy it into a `std::string` before Rust's `CString` is dropped. If the C++ code stores the `*const c_char` pointer directly, it dangles when Rust's function returns.

2. **ExtensionStatement allocation:** `ExtensionStatement` must be allocated with C++ `new` (not Rust's allocator). The C++ trampoline should: (a) call Rust's parse function to get the parsed data as plain C types, (b) construct the `ExtensionStatement` using `new` in C++, (c) return `make_unique<ExtensionStatement>(...)`.

3. **plan_function state stash:** Following the prql pattern, `plan_function` may stash state in `context.registered_state`. This state must be C++-allocated and C++-freed. If Rust-allocated data is stashed, it must be behind a C-compatible wrapper with explicit free functions.

**Prevention:**
- In the C++ trampoline (`sv_parse_trampoline`):
  ```cpp
  auto result = sv_parse(query.c_str(), query.size());
  if (!result.success) return ParserExtensionParseResult(); // not handled
  auto stmt = make_unique<ExtensionStatement>(
      extension, std::string(result.statement_text));
  return ParserExtensionParseResult(std::move(stmt));
  ```
- The Rust `sv_parse` function returns a simple C struct: `{ success: bool, statement_text: *const c_char, statement_text_len: usize }`. The `statement_text` points to a `CString` that Rust keeps alive by leaking it (via `CString::into_raw`). The C++ trampoline copies the text into a `std::string`, then calls `sv_parse_free(result.statement_text)` to reclaim the Rust allocation.
- Alternative: the Rust function writes into a caller-provided buffer. Simpler lifetime management but requires size negotiation.
- **Confidence:** HIGH. Standard C/C++/Rust FFI memory ownership patterns. The `CString::into_raw` + explicit free pattern is documented in the Rustonomicon.

**Phase assignment:** Parser hook implementation phase.

---

### P9: Semicolon Inconsistency in parse_function Input

**What goes wrong:**
DuckDB issue #18485 documents that the query string passed to `parse_function` has inconsistent semicolon handling:
- CLI: trailing semicolon sometimes included, sometimes not
- Python API: trailing semicolon sometimes stripped
- DuckDB UI: trailing semicolon stripped
- The behavior differs between `CREATE` statements and other statement types

The extension's prefix-matching logic (`query.starts_with("CREATE SEMANTIC VIEW")`) is not affected by trailing semicolons. But if the parser extracts the view body from the query string by taking "everything after the view name," a trailing semicolon becomes part of the view definition text.

**Prevention:**
- Always strip trailing semicolons and whitespace from the query string before parsing. This is a one-line normalization step.
- Do not rely on the semicolon to determine statement boundaries -- DuckDB's statement splitter handles this before `parse_function` is called.
- Test from all interfaces: CLI (`duckdb -c "CREATE SEMANTIC VIEW ...;"`), Python (`conn.execute("CREATE SEMANTIC VIEW ...;")`), and sqllogictest runner.
- **Confidence:** HIGH. Confirmed in DuckDB issue #18485. The normalization is trivial.

**Phase assignment:** Parser hook implementation phase.

---

### P10: Double Parser Hook Registration on Extension Reload

**What goes wrong:**
If the extension is loaded twice (e.g., `LOAD 'semantic_views'; LOAD 'semantic_views';`), the parser hook is registered twice. Both hooks fire for every query. The second hook attempts to parse statements that the first hook already handled, potentially causing:
- Duplicate `ExtensionStatement` construction
- Double execution of DDL operations (defining a view twice)
- Confusing error messages

DuckDB's extension loader should prevent double-loading, but if it does not, the parser hook vector (`cfg.parser_extensions`) will contain two entries for the same extension.

**Prevention:**
- Use a static `std::once_flag` (or equivalent) in the C++ shim to ensure `push_back(ext)` is called exactly once:
  ```cpp
  static std::once_flag parser_registered;
  std::call_once(parser_registered, [&]() {
      cfg.parser_extensions.push_back(ext);
  });
  ```
- Alternative: check if the parser hook is already registered by iterating `cfg.parser_extensions` and checking for the extension's `parse_fun` pointer.
- **Confidence:** MEDIUM. DuckDB likely prevents double-loading, but the guard is cheap insurance.

**Phase assignment:** C++ shim implementation phase.

---

### P11: Panic Across FFI Boundary in Parse Callback

**What goes wrong:**
If the Rust `sv_parse` function panics (e.g., due to an unexpected input, unwrap on None, index out of bounds), the panic unwinds through the C++ trampoline and into DuckDB's parser. Under `extern "C"` calling convention, unwinding across FFI is undefined behavior. The typical result: immediate abort with no useful error message, or memory corruption.

This is especially dangerous because `parse_function` is called for every query that DuckDB's parser cannot handle. A bug in the Rust parse logic would crash every query, not just `CREATE SEMANTIC VIEW` queries.

**Prevention:**
- Wrap the entire Rust `sv_parse` body in `std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))`. Convert panics to error returns.
- The C++ trampoline checks the error return and returns `ParserExtensionParseResult()` (not handled) or sets an error.
- The `sv_parse` function must never `unwrap()`, `expect()`, or use `[]` indexing on untrusted input. Use `match`, `if let`, and `.get()`.
- Add negative tests: malformed `CREATE SEMANTIC VIEW` statements that exercise all error paths.
- **Confidence:** HIGH. Standard Rust FFI practice. RFC 2945 (`extern "C-unwind"`) would allow safe unwinding, but DuckDB's callback signatures use `extern "C"`.

**Phase assignment:** Parser hook implementation phase. This pattern must be established in the very first `sv_parse` implementation.

---

## Minor Pitfalls

### P12: Stale Build Artifacts from cc Crate

**What goes wrong:**
The `cc` crate caches compiled C++ objects. When `shim.cpp` is modified, the cached object may not be recompiled if the `cc` crate's change detection does not notice the modification (e.g., the header it depends on changed, but `cc` only tracks the `.cpp` file).

Phase 11 hit this: after editing `shim.cpp`, `nm -u` still showed old symbols from a cached `libsemantic_views_shim.a`. The fix was `cargo clean -p semantic_views`.

**Prevention:**
- Add `println!("cargo:rerun-if-changed=src/shim/shim.cpp");` and `println!("cargo:rerun-if-changed=src/shim/duckdb.hpp");` in `build.rs`. The `cc` crate should handle this automatically, but explicit `rerun-if-changed` directives are insurance.
- After any change to the C++ shim or amalgamation header: `cargo clean -p semantic_views && cargo build`.
- **Confidence:** HIGH. Documented cc crate behavior. Previously hit in this project.

**Phase assignment:** Build system phase.

---

### P13: Amalgamation Header Version Must Match DuckDB Pin

**What goes wrong:**
The `shim.cpp` includes `duckdb.hpp` (the amalgamation header). This header defines the C++ class layouts, vtable orderings, and function signatures. If the amalgamation header version does not exactly match the DuckDB runtime version (`v1.4.4`), the extension's C++ objects will have wrong layouts. Calling virtual methods through mismatched vtables causes crashes.

The DuckDB CI workflow (`DuckDBVersionMonitor.yml`) already detects version mismatches for the Rust `duckdb-rs` crate. It must also update the amalgamation header.

**Prevention:**
- Fetch the amalgamation from the same DuckDB release as the `duckdb-rs` pin. The download URL is deterministic: `https://github.com/duckdb/duckdb/releases/download/v1.4.4/libduckdb-src.zip`.
- Pin the amalgamation download in `build.rs` using the same `TARGET_DUCKDB_VERSION` constant.
- The `DuckDBVersionMonitor.yml` workflow should update both `Cargo.toml` (duckdb-rs version) and the amalgamation download URL when a new version is detected.
- Add a build-time assertion: compare a version string from the amalgamation header against the Cargo.toml duckdb version.
- **Confidence:** HIGH. Version mismatch causes obvious crashes. The mitigation is a build system check.

**Phase assignment:** Build system phase.

---

### P14: `parse_function` vs `parser_override` -- Use parse_function (Fallback)

**What goes wrong:**
There are two parser extension hooks:
- `parser_override`: Called BEFORE DuckDB's parser, for every query. Must handle the full SQL grammar if it claims a statement.
- `parse_function`: Called AFTER DuckDB's parser fails. Only receives statements that DuckDB cannot parse.

`CREATE SEMANTIC VIEW ...` will fail DuckDB's parser at the `SEMANTIC` keyword (unrecognized after `CREATE`), triggering `parse_function`. This is the correct hook.

Using `parser_override` would require the extension to parse EVERY query (even `SELECT 1`) and return "not handled" for non-semantic-view queries. This adds overhead to every query and is unnecessary.

The v0.2.0 PITFALLS.md (P2.1) incorrectly recommended `parser_override` for some scenarios. The investigation doc correctly identifies `parse_function` as the right choice.

**Prevention:**
- Register `parse_function`, not `parser_override`.
- The first line of the parse function: check if the query starts with `CREATE SEMANTIC VIEW` (case-insensitive, after stripping whitespace/comments). If not, return "not handled" immediately.
- The performance impact of `parse_function` is zero for normal queries (it is only called when DuckDB's parser fails).
- **Confidence:** HIGH. Confirmed by prql extension source code (uses `parse_function`, not `parser_override`) and the DuckDB extensible parsers blog post.

**Phase assignment:** Parser hook implementation phase.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Build system setup | P5 (footer ABI type), P6 (symbol visibility), P12 (stale artifacts), P13 (amalgamation version) | Update Makefile, build.rs, and add CI verification steps |
| C++ shim scaffold | P2 (dual entry point), P4 (Rust function pointers), P10 (double registration) | POC spike to prove entry point strategy works end-to-end |
| Parser hook impl | P7 (thread safety), P8 (memory ownership), P9 (semicolons), P11 (panic safety), P14 (hook type) | Stateless parse function, catch_unwind, normalize input |
| Integration testing | P1 (ODR at runtime), P3 (compiler ABI) | Test on macOS + Linux, verify no crashes under load |
| C API bridge | P4 (function pointer table init) | Critical spike -- prove Rust ffi::* works under CPP entry |

---

## Research Notes

**Confidence assessment:**

| Area | Confidence | Basis |
|------|------------|-------|
| ODR behavior (P1) | MEDIUM | DuckDB PR #3783, dlopen RTLD_LOCAL documentation, prql/duckpgq existence proofs |
| Entry point conflict (P2, P4) | LOW | Novel problem -- no documented Rust+C++ DuckDB extension exists. Requires POC. |
| C++ ABI (P3) | HIGH | Well-established C++ ABI rules, DuckDB community extension CI practices |
| Footer stamping (P5) | HIGH | Directly read `append_extension_metadata.py` source -- confirmed `--abi-type CPP` support |
| Symbol visibility (P6) | HIGH | Existing build.rs already handles this; extending to include C++ symbol is mechanical |
| Thread safety (P7) | HIGH | DuckDB threading model documented; parse function is stateless |
| Memory ownership (P8) | HIGH | Standard Rust FFI patterns, Rustonomicon |
| Semicolons (P9) | HIGH | Confirmed in DuckDB issue #18485 |
| Double registration (P10) | MEDIUM | DuckDB likely prevents double-load, but guard is cheap |
| Panic safety (P11) | HIGH | Standard Rust FFI, RFC 2945 |

**Sources consulted:**
- [DuckDB PR #3783: Extension loading by statically linking DuckDB](https://github.com/duckdb/duckdb/pull/3783) -- RTLD_LOCAL, static linking per-extension
- [DuckDB PR #12682: C API extensions](https://github.com/duckdb/duckdb/pull/12682) -- C_STRUCT ABI, function pointer struct
- [DuckDB issue #18485: Inconsistent semicolon handling](https://github.com/duckdb/duckdb/issues/18485) -- parser extension semicolons
- [DuckDB extension-ci-tools `append_extension_metadata.py`](https://github.com/duckdb/extension-ci-tools/) -- footer format, ABI type parameter
- [DuckDB extension loading (DeepWiki)](https://deepwiki.com/duckdb/duckdb/4.3-extension-loading-and-installation) -- ABI types, entry point selection
- [DuckDB extension system (DeepWiki)](https://deepwiki.com/duckdb/duckdb/3-extension-system) -- extension architecture
- [duckdb-prql (GitHub)](https://github.com/ywelsch/duckdb-prql) -- existence proof for parser extension compiled against amalgamation
- [duckdb-rs issue #370: Rust extensions via C API](https://github.com/duckdb/duckdb-rs/issues/370) -- function pointer mechanism
- [DuckDB community extension development docs](https://duckdb.org/community_extensions/development) -- CI and build requirements
- [C++ dlopen duplicate symbols](https://linuxvox.com/blog/loading-two-instances-of-a-shared-library/) -- RTLD_LOCAL isolation semantics
- [Rust FFI (Rustonomicon)](https://doc.rust-lang.org/nomicon/ffi.html) -- thread safety, memory ownership, panic safety
- This project's `_notes/parser-extension-investigation.md` -- prior investigation
- This project's `build.rs` -- current symbol visibility setup
- This project's `src/lib.rs` -- current entry point and Rust FFI surface
- This project's phase 11 summary (`11-04-SUMMARY.md`) -- prior failure analysis
