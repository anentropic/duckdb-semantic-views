# Architecture Patterns: v0.2.0 Integration Design

**Project:** DuckDB Semantic Views Extension
**Researched:** 2026-02-28
**Scope:** How v0.2.0 features integrate with existing CMake + Rust architecture
**Confidence:** HIGH for build mechanics and C API limits; MEDIUM for C++ shim internals (no shipped Rust+C++ mixed DuckDB extension as reference)

---

## Existing Architecture (Do Not Re-Research)

The v0.1.0 codebase establishes:

- **Build:** `make debug/release` → `cargo build --no-default-features --features extension` → cdylib. No CMake. The `Makefile` includes `extension-ci-tools/makefiles/c_api_extensions/rust.Makefile`, which drives pure Cargo builds. No CMakeLists.txt exists at the repo root.
- **Entry point:** Manual `extern "C" fn semantic_views_init_c_api(info, access)` — hand-written to capture `duckdb_database` handle before Connection wraps it.
- **Catalog:** `Arc<RwLock<HashMap<String, String>>>` (in-memory) + sidecar JSON file for cross-restart persistence. DuckDB SQL writes are blocked during scalar `invoke`.
- **Query interface:** `semantic_query` VTab — expand in bind phase, execute on independent `duckdb_connection` via raw `ffi::duckdb_query`.
- **EXPLAIN interface:** `explain_semantic_view` VTab — runs `EXPLAIN {expanded_sql}` on the same independent connection, returns text rows.
- **Feature split:** `default = [duckdb/bundled]` for `cargo test`; `extension = [duckdb/loadable-extension, duckdb/vscalar]` for cdylib builds.

---

## v0.2.0 Feature Integration Analysis

### Feature 1: C++ Shim Layer

**What it enables:** Parser hooks (`CREATE SEMANTIC VIEW` DDL) and pragma registration (`pragma_query_t` callbacks). Both require `DBConfig` access — specifically `config.parser_extensions.push_back()` and DuckDB's internal `ExtensionUtil::RegisterFunction` / `Catalog::RegisterEntry` for pragma functions. These are C++ SDK APIs only; they are not exposed in the C extension API struct (`duckdb_extension_access`).

**Integration point in the existing build:**

The current build is pure Cargo — no C++ compilation today. Adding C++ requires either:

- **Option A (Cargo `build.rs` + `cc` crate):** Write `src/shim/shim.cpp` and compile it from `build.rs` using the `cc` crate. The `cc` crate links the resulting `.o` directly into the Rust cdylib. This is the standard Rust pattern for embedding a small amount of C or C++. The shim `.cpp` file uses DuckDB headers (from `libduckdb-sys` or downloaded separately) and exports `extern "C"` functions that Rust calls. **This is the recommended approach for this project** — it keeps one build system (Cargo), avoids introducing CMake, and requires minimal tooling changes.

- **Option B (CMake invokes Cargo):** Add a `CMakeLists.txt` at the repo root. CMake compiles the C++ shim into a static library, then invokes `cargo build` as an `ExternalProject` (or via `add_custom_command`), then links shim + cdylib together. This is how the C++ `extension-template` works. The existing `extension-ci-tools/makefiles/c_api_extensions/c_cpp.Makefile` supports this flow. However, it introduces CMake as a new dependency and substantially increases build complexity for what is mostly a Rust project.

**Recommendation:** Option A (Cargo build.rs + cc crate). Reasons:

1. All existing tooling (`just`, `cargo nextest`, `cargo-llvm-cov`, CI workflows) works unchanged — they drive `cargo build`.
2. The shim is small: one `.cpp` file with two functions (`register_parser_extension`, `register_pragma_function`). The cc crate handles cross-compilation targets (osx_arm64, linux_amd64, etc.) automatically.
3. DuckDB headers needed by the shim (`duckdb.hpp` or targeted headers) can be pinned via `libduckdb-sys`'s bundled headers or downloaded at configure time (the `update_duckdb_headers` make target already handles this for the C header).
4. Option B (CMake) would require switching the Makefile include from `rust.Makefile` to `c_cpp.Makefile` and introducing a CMakeLists.txt — viable but disproportionate.

**Shim file placement:**

```
src/
  shim/
    shim.cpp         ← C++ code: registers parser extension + pragma
    shim.h           ← C header: extern "C" declarations called by Rust
build.rs             ← NEW: compiles shim.cpp via cc crate
```

**build.rs pattern:**

```rust
fn main() {
    cc::Build::new()
        .cpp(true)
        .file("src/shim/shim.cpp")
        .include("duckdb_capi/")           // headers already present
        .flag("-std=c++17")
        .compile("semantic_views_shim");   // produces libsemantic_views_shim.a
    // Cargo automatically links the static lib into the cdylib
}
```

The `cc` crate is a standard Rust build dependency; add it to `[build-dependencies]` in `Cargo.toml`.

**C++ header requirement:** The shim needs DuckDB C++ headers — specifically `duckdb/main/config.hpp` and `duckdb/parser/parser_extension.hpp`. These are part of DuckDB's internal SDK, not the public `duckdb.h`. The project already has `duckdb_capi/duckdb.h` and `duckdb.extension.h` from the `extension-template-rs` pattern. The C++ headers are in a different location: either bundle `duckdb.hpp` (the amalgamated single-header C++ SDK, ~6MB) or use the `duckdb` git submodule.

Pragmatic approach: download `duckdb.hpp` from the pinned DuckDB release tag and place it in `duckdb_capi/duckdb.hpp`. Add a `just update-headers` recipe. This is one file, version-pinned alongside the Rust crate.

**Confidence:** HIGH for the `cc` crate / build.rs approach being mechanically correct. MEDIUM for the specific headers needed — the exact include paths for `DBConfig` and `ParserExtension` need validation against DuckDB v1.4.4's amalgam.

---

### Feature 2: Parser Hook — `CREATE SEMANTIC VIEW` DDL

**DuckDB's parser extension API (C++ only):**

From DuckDB's `parser_extension.hpp` and the code pattern seen in real extensions:

```cpp
// Registration (in LoadInternal or called from it):
auto& config = DBConfig::GetConfig(instance);
DuckParserExtension parser_ext;  // custom struct
config.parser_extensions.push_back(parser_ext);
config.operator_extensions.push_back(make_uniq<DuckOperatorExtension>());
```

`ParserExtension` holds three function pointers:
- `parse_function_t` — called when DuckDB's native parser fails; receives the query string, returns `ParserExtensionParseResult`
- `plan_function_t` — called by the planner when it encounters an `ExtensionStatement` (result of a successful parse); returns `ParserExtensionPlanResult` containing a `TableFunction` to execute
- `parser_override_function_t` — optional; can intercept before DuckDB's parser even tries

**How `CREATE SEMANTIC VIEW` flows:**

```
User sends:  CREATE SEMANTIC VIEW my_view (...)
             ↓
DuckDB native parser fails (unknown statement)
             ↓
parse_function_t fires → extension parses DDL text
             ↓ success
Returns ExtensionStatement(parse_data)
             ↓
plan_function_t fires with parse_data
             ↓
Returns ParserExtensionPlanResult { function: semantic_view_ddl_fn, parameters: [...] }
             ↓
DuckDB executes semantic_view_ddl_fn with parameters
             ↓
Rust FFI: semantic_view_ddl_fn calls back to Rust extern "C" fn
             → registers view in catalog
             → writes sidecar (or runs pragma SQL string if pragma_query_t is used)
```

**Critical constraint: `plan_function_t` returns a `TableFunction`, not a SQL string.** The TableFunction's bind/execute phases run inside DuckDB's normal execution path — they have the same execution lock constraints as the current `invoke`. This means the `plan_function_t` path cannot run SQL on the host connection either. Options:

- Use the independent `duckdb_connection` (already created in `init_extension`) for catalog writes from within the TableFunction. This avoids locks.
- Use the `pragma_query_t` pattern (see Feature 3) where the planner callback returns a SQL string that DuckDB executes post-lock. This is cleaner.

**Where the parse logic lives:** The `parse_function_t` in `shim.cpp` parses the DDL syntax. It can delegate to Rust for the heavy lifting via an `extern "C"` function:

```cpp
// shim.cpp
extern "C" {
    // Declared in shim.h, implemented in Rust (src/shim/ffi.rs)
    bool semantic_views_parse_ddl(const char* query, char** out_view_name, char** out_json, char** out_error);
    bool semantic_views_plan_ddl(const char* view_name, const char* json, char** out_error);
}
```

The Rust side implements the actual parsing and catalog registration. The C++ shim is thin — it forwards to Rust and converts DuckDB C++ types to C-compatible strings.

**Confidence:** MEDIUM-HIGH. The `DBConfig::GetConfig + config.parser_extensions.push_back` registration pattern is confirmed by DuckDB GitHub issue #18485. The flow through `plan_function_t` returning a `TableFunction` is documented in `parser_extension.hpp`. The execution lock constraint for the plan phase is inferred from v0.1.0 learnings — needs validation.

---

### Feature 3: `pragma_query_t` — Replacing Sidecar Persistence

**What `pragma_query_t` is:** A function pointer type in DuckDB's pragma system:

```cpp
typedef string (*pragma_query_t)(ClientContext &context, const FunctionParameters &parameters);
```

A pragma registered with `pragma_query_t` returns a SQL string. DuckDB executes that SQL string instead of (or after) the pragma callback. This runs outside execution locks — the SQL executes normally in the DuckDB query pipeline.

**How the FTS extension uses it:** The FTS `PRAGMA create_fts_index(...)` callback builds the index schema as a set of `CREATE TABLE` and `INSERT` SQL statements, returns them as a combined string, and DuckDB executes them. Persistence happens automatically because DuckDB persists the created tables in the `.duckdb` file.

**Integration with existing catalog sync:**

Current flow (v0.1.0):
```
define_semantic_view invoked
  → catalog_insert (HashMap write)
  → write_sidecar (JSON file write, avoids SQL locks)
  → [next load] init_catalog syncs sidecar → semantic_layer._definitions table
```

v0.2.0 target flow with `pragma_query_t`:
```
CREATE SEMANTIC VIEW fires
  → parse_function_t parses DDL (C++ shim → Rust parser)
  → plan_function_t called by planner
  → returns INSERT SQL: "INSERT INTO semantic_layer._definitions VALUES (...)"
  → DuckDB executes INSERT (no locks — runs in normal query pipeline)
  → definition is in semantic_layer._definitions table (persisted by DuckDB)
  → also update in-memory CatalogState HashMap
```

The `semantic_layer._definitions` table already exists and is already loaded at init time. The pragma_query_t approach eliminates the sidecar entirely: the DuckDB table becomes the sole source of truth.

**How to register a pragma from C++:**

```cpp
// shim.cpp
PragmaFunction pf = PragmaFunction::PragmaStatement(
    "create_semantic_view_internal",
    semantic_view_pragma_query  // pragma_query_t function pointer
);
ExtensionUtil::RegisterFunction(instance, pf);
```

`semantic_view_pragma_query` returns a SQL `INSERT` string. DuckDB executes it. No execution locks involved.

**However:** `pragma_query_t` is a C++ API. It requires `duckdb/function/pragma_function.hpp`. If implementing via C++ shim, this is natural. If the shim is thin and just registers the pragma, the actual SQL string construction can happen in Rust via an `extern "C"` call.

**Does pragma_query_t replace the define_semantic_view scalar function?** For file-backed databases, yes — `CREATE SEMANTIC VIEW` + pragma becomes the canonical path. The function-based DDL (`define_semantic_view`) can remain as a compatibility alias or be deprecated in v0.2.0. The sidecar code in `catalog.rs` becomes dead code once pragma_query_t is the persistence path.

**Confidence:** HIGH for `pragma_query_t` returning SQL that DuckDB executes. HIGH that this runs outside the execution lock (that's the entire point of the pattern). MEDIUM for the exact C++ registration path — needs validation against DuckDB 1.4.4's `ExtensionUtil` API.

---

### Feature 4: EXPLAIN Hook

**Current state (v0.1.0):** `explain_semantic_view('view', dimensions := [...], metrics := [...])` is a VTab that:
1. Calls `expand()` to get the expanded SQL
2. Runs `EXPLAIN {expanded_sql}` on the independent connection
3. Returns text rows with metadata header, expanded SQL, and DuckDB plan

This already works. The v0.2.0 "native EXPLAIN" goal is to make `EXPLAIN FROM semantic_query(...)` show the expanded SQL in the standard DuckDB EXPLAIN output (i.e., without needing a separate `explain_semantic_view` call).

**What native EXPLAIN interception requires:**

DuckDB's EXPLAIN processing happens in the planner after the logical plan is built. An extension hook that intercepts EXPLAIN output would need access to the `OptimizerExtension` or the logical plan tree after the table function's bind phase. The bind phase already has the expanded SQL — the EXPLAIN text just needs to be captured and inserted into the plan tree.

**How the hook would work:**

Option A: `OptimizerExtension` — fires after logical planning. The callback can inspect the plan for `SemanticViewVTab` nodes and annotate them with the expanded SQL string. This is the cleaner approach but requires C++ SDK access.

Option B: Modify `SemanticViewVTab.bind()` to store the expanded SQL in a side-channel (already done — `SemanticViewBindData.expanded_sql` exists). DuckDB's physical plan for the table function already includes this data. The gap is surfacing it in `EXPLAIN` output. DuckDB's `PhysicalTableFunction::GetOperatorName()` or similar virtual method could be overridden, but this requires C++ subclassing.

Option C (pragmatic, no C++ shim needed): Keep `explain_semantic_view` as the EXPLAIN interface and document it. Defer true EXPLAIN interception. This is what v0.1.0 already does — `TECH-DEBT.md` item QUERY-V2-03 notes this requires a C++ shim.

**Recommendation for v0.2.0:** Keep the existing `explain_semantic_view` VTab as-is. It already runs `EXPLAIN {expanded_sql}` and returns the DuckDB plan. The only missing UX is that `EXPLAIN FROM semantic_query(...)` doesn't automatically call it. This gap is a documentation issue, not an architecture gap. Intercepting EXPLAIN itself is complex C++ work with unclear benefit given the existing workaround.

If the C++ shim is being built anyway for the parser hook, adding an `OptimizerExtension` that prints expanded SQL when it sees a `SemanticViewVTab` node is feasible — but not required for the core v0.2.0 goals.

**Confidence:** HIGH that the existing `explain_semantic_view` VTab is sufficient for practical use. MEDIUM that an OptimizerExtension approach would work for native EXPLAIN interception — not validated against 1.4.4.

---

### Feature 5: Time Dimensions

**No C++ required.** Time dimension support is purely Rust expansion engine work. Integration points:

**a. Model change (`src/model.rs`):**

`Dimension` needs a new optional field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dimension {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub source_table: Option<String>,
    // NEW for v0.2.0:
    #[serde(default)]
    pub time_grain: Option<TimeGrain>,   // if Some, this is a time dimension
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimeGrain {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}
```

Because `SemanticViewDefinition` uses `#[serde(deny_unknown_fields)]`, adding `time_grain` with `#[serde(default)]` is backward-compatible — existing definitions without the field deserialize with `time_grain: None`.

**b. Expansion engine change (`src/expand.rs`):**

The `QueryRequest` gains an optional `granularity` override per dimension:

```rust
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
    // NEW: per-dimension granularity override
    pub granularities: HashMap<String, TimeGrain>,  // dim_name → requested grain
}
```

In `build_sql()`, when a dimension has `time_grain: Some(grain)`, the SQL expression wraps the dimension expr in `date_trunc`:

```sql
-- grain=Month:
date_trunc('month', "order_date") AS "order_date"
-- grain=Week:
date_trunc('week', "order_date") AS "order_date"
```

DuckDB's `date_trunc(precision, timestamp)` handles all granularities: `'day'`, `'week'`, `'month'`, `'quarter'`, `'year'`.

**c. Table function interface change (`src/query/table_function.rs`):**

The `semantic_query` VTab gains an optional named parameter `granularities` (a MAP or STRUCT of dimension-name to grain string). The bind phase parses this into `QueryRequest.granularities`.

Alternatively, granularity can be encoded in the dimension name: `'order_date__month'` → dimension `order_date` at `month` grain. This avoids changing the function signature but is less clean.

**d. The CTE-based expansion (`src/expand.rs`) is unchanged:** Time dimensions are just a modified expression in the SELECT and GROUP BY. The flat `_base` CTE namespace still works — `date_trunc('month', order_date)` is valid in the CTE scope.

**Backward compatibility:** Existing `SemanticViewDefinition` JSON (no `time_grain` field) deserializes correctly because the field defaults to `None`. No migration needed.

**Confidence:** HIGH. `date_trunc` is standard DuckDB SQL. The model and expansion engine changes are additive. The only design decision is how to express granularity in `semantic_query` parameters — the chosen approach (new named parameter `granularities`) is straightforward.

---

## Component Map: New vs. Modified

| Component | Status | File(s) | What Changes |
|-----------|--------|---------|--------------|
| `shim.cpp` | NEW | `src/shim/shim.cpp` | C++ code: parser extension registration, pragma registration |
| `shim.h` | NEW | `src/shim/shim.h` | `extern "C"` declarations for Rust↔C++ boundary |
| `src/shim/ffi.rs` | NEW | `src/shim/ffi.rs` (or `src/lib.rs`) | Rust `extern "C"` implementations called by shim |
| `build.rs` | NEW | `build.rs` | Compiles `shim.cpp` via `cc` crate when `extension` feature active |
| `Cargo.toml` | MODIFIED | `Cargo.toml` | Add `cc` to `[build-dependencies]` |
| `duckdb_capi/duckdb.hpp` | NEW | `duckdb_capi/duckdb.hpp` | Amalgamated C++ SDK header (downloaded, version-pinned) |
| `Justfile` | MODIFIED | `Justfile` | Add `just update-headers` recipe for `duckdb.hpp` |
| `src/model.rs` | MODIFIED | `src/model.rs` | Add `TimeGrain` enum, `time_grain` field to `Dimension` |
| `src/expand.rs` | MODIFIED | `src/expand.rs` | Add granularity coarsening via `date_trunc` in SQL builder |
| `src/query/table_function.rs` | MODIFIED | `src/query/table_function.rs` | Add `granularities` named parameter to `semantic_query` |
| `src/catalog.rs` | MODIFIED | `src/catalog.rs` | Remove sidecar logic once pragma_query_t is the write path (or keep as fallback) |
| `src/lib.rs` | MODIFIED | `src/lib.rs` | Call C++ shim registration from `init_extension` |
| `src/query/explain.rs` | UNCHANGED | `src/query/explain.rs` | Already works; no changes needed |
| `src/ddl/define.rs` | UNCHANGED or DEPRECATED | `src/ddl/define.rs` | Function-based DDL stays as compatibility shim; sidecar write path may be removed |

---

## Rust ↔ C++ Boundary

The shim is thin by design. The C++ side only:
1. Holds DuckDB C++ types (`DatabaseInstance&`, `DBConfig&`, `ParserExtension`)
2. Registers hooks with DuckDB
3. Forwards to Rust via `extern "C"` for all logic

```c
// shim.h — the complete boundary
#ifdef __cplusplus
extern "C" {
#endif

// Called from Rust init_extension to wire up C++ hooks
void semantic_views_register_parser(void* db_instance_ptr);
void semantic_views_register_pragma(void* db_instance_ptr);

// Called by DuckDB from within parse_function_t, forwarded to Rust
// Returns 1 on success, 0 on failure. Caller frees out_* with sv_free_str().
int sv_parse_ddl(const char* query,
                 char** out_view_name,
                 char** out_json,
                 char** out_error);

// Called by DuckDB from within pragma_query_t
// Returns SQL string to execute (caller frees with sv_free_str())
char* sv_make_define_sql(const char* view_name, const char* json);
char* sv_make_drop_sql(const char* view_name);
void sv_free_str(char* s);

#ifdef __cplusplus
}
#endif
```

The Rust side exports these via:

```rust
// src/shim/ffi.rs
#[no_mangle]
pub extern "C" fn sv_parse_ddl(...) { ... }

#[no_mangle]
pub extern "C" fn sv_make_define_sql(...) -> *mut c_char { ... }
```

The C++ shim calls these Rust functions. The Rust functions have no DuckDB C++ dependency — they work on strings and call into existing Rust catalog logic.

**No Rust→C++ calls for catalog operations.** The C++ side of the boundary only registers hooks; it never calls back into C++ catalog APIs. The pragma_query_t callback returns a SQL string; DuckDB executes it. Clean separation.

---

## Build Order for Implementation

Dependencies flow is:

```
duckdb.hpp download      → shim.cpp compilation
build.rs                 → shim compilation (cc crate)
shim.h / ffi.rs          → Rust/C++ boundary
model.rs (TimeGrain)     → expand.rs (date_trunc) → table_function.rs (parameter)
shim registration        → parser hook → catalog (pragma_query_t replaces sidecar)
```

Recommended implementation sequence:

**Step 1: Build infrastructure** — Add `build.rs` + `cc` crate + `duckdb.hpp`. Compile a minimal `shim.cpp` that just includes the headers and compiles cleanly. Verify the extension still loads in DuckDB. This validates that the C++ compilation works before adding any logic.

**Step 2: Time dimensions** — Add `TimeGrain` to model, `date_trunc` wrapping in expand, `granularities` parameter in table_function. This is pure Rust, testable with existing `cargo test`. No C++ shim needed. Fuzz targets and proptest cover the expansion engine.

**Step 3: Pragma registration** — Implement `sv_make_define_sql` and `sv_make_drop_sql` in Rust ffi.rs. Implement `semantic_views_register_pragma` in shim.cpp. Test that `PRAGMA define_semantic_view_internal(...)` executes an INSERT and survives restart without sidecar.

**Step 4: Parser hook** — Implement `sv_parse_ddl` in Rust. Implement `semantic_views_register_parser` in shim.cpp with `parse_function_t` and `plan_function_t`. Test `CREATE SEMANTIC VIEW my_view (...)` end-to-end. This is the most complex step.

**Step 5: Sidecar removal** — Once Step 4 validates persistence via pragma_query_t, remove sidecar logic from `catalog.rs` and `define.rs`. Keep the function-based DDL (`define_semantic_view`) as a compatibility path for in-memory databases (where pragma_query_t inserts into a non-persisted table, which is fine).

**Step 6: Integration tests** — Update SQLLogicTest files for native DDL syntax. Verify that existing `define_semantic_view` function still works (backward compat). Add tests for time dimension granularity coarsening.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Making the C++ Shim Fat

**What goes wrong:** Putting parsing logic, validation, or catalog access in `shim.cpp`. Rust code is well-tested and has ownership semantics; C++ code in this project will not. The shim should be 50-100 lines of registration + forwarding.

**Instead:** Parse DDL in Rust (reuse `SemanticViewDefinition::from_json` or extend it). Build SQL strings in Rust. The C++ shim only translates between DuckDB C++ types and the `extern "C"` boundary.

### Anti-Pattern 2: Using CMake When cc Crate Suffices

**What goes wrong:** Adding CMakeLists.txt forces all existing CI workflows, developer tools, and the `just` task runner to work through a CMake layer. The whole build system changes for a 50-line C++ file.

**Instead:** `build.rs` + `cc` crate. Cargo knows how to compile C++, link the result, and propagate platform flags. The existing `rust.Makefile` and CI continue to work unchanged.

### Anti-Pattern 3: Calling C++ Catalog APIs from Rust

**What goes wrong:** Trying to call `Catalog::CreateEntry` or `Schema::CreateEntry` from Rust via FFI. DuckDB's C++ catalog types are complex, and their ABI is not stable. Any C++ API not in `duckdb.h` can change across patch versions.

**Instead:** Use SQL strings returned from `pragma_query_t`. DuckDB executes them in the normal pipeline. The `semantic_layer._definitions` table (already exists from v0.1.0) is the persistence target.

### Anti-Pattern 4: Registering Pragma Without Needing Sidecar Removal

**What goes wrong:** Implementing `pragma_query_t` alongside the sidecar, creating two persistence paths that can diverge.

**Instead:** Remove the sidecar entirely in the same phase that activates `pragma_query_t`. Two persistence paths is worse than one.

### Anti-Pattern 5: Time Dimension Granularity in Dimension Name Encoding

**What goes wrong:** Encoding granularity in the dimension name string (`order_date__month`) rather than as a separate parameter. Users then need string hacking to reference dimensions by name in filters or other functions.

**Instead:** Add `granularities` as a proper named parameter to `semantic_query`. The expand engine receives a clean `HashMap<String, TimeGrain>` and applies `date_trunc` to matching dimensions.

---

## Scalability Considerations

This extension is a preprocessor — DuckDB handles all execution. Scale concerns are:

| Concern | Current (v0.1.0) | v0.2.0 |
|---------|------------------|--------|
| Catalog reads | RwLock on in-memory HashMap — reads non-blocking | Unchanged |
| Catalog writes (define/drop) | Sidecar file write (~microseconds) | pragma_query_t → SQL INSERT (DuckDB lock, safe outside invoke) |
| Parser hook overhead | N/A | One `str.contains("SEMANTIC VIEW")` check per statement that native parser fails — negligible |
| Time dimension expansion | N/A | One `date_trunc` wrapper per time dimension — zero runtime overhead |
| C++ shim binary size | N/A | ~10KB static lib linked into cdylib — negligible |

---

## Sources

- DuckDB `pragma_function.hpp` (raw.githubusercontent.com) — confirmed `pragma_query_t` type signature (HIGH confidence)
- DuckDB GitHub issue #18485 — confirmed `DBConfig::GetConfig(instance)` + `config.parser_extensions.push_back()` pattern (HIGH confidence)
- DuckDB `parser_extension.hpp` (deepwiki summary) — confirmed `parse_function_t`, `plan_function_t`, `ParserExtensionPlanResult` structure (MEDIUM-HIGH confidence)
- `duckdb_extension.h` v1.4.4 — confirmed parser hooks and pragma registration are NOT in `duckdb_extension_access` struct (HIGH confidence)
- TECH-DEBT.md (v0.1.0) — confirmed sidecar approach, execution lock constraints, deferred items (HIGH confidence — first-party)
- CIDR 2025 paper (Mühleisen & Raasveldt) — confirmed DuckDB extensible parser mechanism exists at C++ level
- DuckDB FTS source analysis — confirmed schema-based persistence pattern (FTS index = DuckDB tables in schema) (MEDIUM confidence)
- Rust `cc` crate documentation — standard approach for C/C++ compilation in Rust build scripts (HIGH confidence)
