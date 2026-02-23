# Stack Research: DuckDB Semantic Views Extension in Rust

**Date:** 2026-02-23
**Research type:** Project — Stack dimension
**Milestone:** Greenfield setup
**Status:** Draft — see Verification Gaps section

---

## Research Scope

This document answers: what is the standard 2026 stack for building a DuckDB extension in Rust, specifically for a project that needs custom DDL (`CREATE SEMANTIC VIEW`), parser interception, catalog integration, table function syntax, and community extension distribution.

---

## Bottom Line Up Front

The DuckDB Rust extension ecosystem is functional but immature. There is an official extension template for C++, a Rust variant of that template maintained semi-officially, and the `duckdb-rs` crate which provides both a client driver and (as of ~0.10) extension-development bindings. The build system is CMake-wrapped Cargo — you cannot escape CMake entirely. The community extension registry (extensions.duckdb.org) provides distribution. The largest gap for this project is **parser hooks**: DuckDB does not expose a stable Rust API for intercepting DDL or hooking into the parser; this likely requires unsafe C FFI calls against internal DuckDB APIs.

---

## Decisions

| Area | Decision | Confidence |
|------|----------|-----------|
| Extension bindings | `duckdb` crate (duckdb-rs) with `extensions` feature | High |
| Build system | CMake + Cargo (via official Rust extension template) | High |
| Template | `duckdb/extension-template-rs` | High |
| Distribution | DuckDB community extension registry | High |
| Parser hooks | C FFI against DuckDB internals — no stable Rust API | High |
| DDL registration | Via `CreateStatement` hook or custom parser extension | Medium |
| Catalog integration | DuckDB catalog C++ API via FFI | Medium |
| SQL AST | `sqlparser-rs` for parsing user-supplied SQL expressions in definitions | Medium |
| Serialization | `serde` + `serde_json` for semantic view catalog storage | High |
| Testing | `duckdb-rs` in-process DuckDB + standard Rust test harness | High |

---

## Core Bindings and SDK

### `duckdb` crate (duckdb-rs)

**Crate:** `duckdb`
**Repo:** https://github.com/duckdb/duckdb-rs
**Version (as of Aug 2025):** ~0.10.x — verify on crates.io before pinning
**License:** MIT

The canonical Rust crate for DuckDB. Originally a community crate, it moved under the `duckdb` GitHub org indicating semi-official maintenance. It serves two roles:

1. **Client driver** — connects to DuckDB databases, executes queries, retrieves results. Modeled after `rusqlite`.
2. **Extension development** — exposes a `TableFunction`, `ScalarFunction`, and related types that allow registering extension functionality from Rust.

**Rationale:** This is the only maintained Rust crate in this space. There is no alternative. The `libduckdb-sys` sub-crate provides the raw C bindings; the `duckdb` crate builds on top with a safe Rust API layer.

**Relevant features:**
- `extensions` feature flag — enables extension development types
- `bundled` feature — bundles DuckDB source (avoids system DuckDB dependency, but balloons compile time; use for CI/distribution, disable for local dev)
- `vtab` feature — enables virtual table support (needed for table function query syntax)

**Confidence: High** — this is the only real option; the question is which version.

**Verification needed:** Pin to a specific version by checking crates.io for the latest `duckdb` release and confirming it tracks the DuckDB version you are targeting (DuckDB C API versioning matters for extension ABI compatibility).

---

### Official Extension Template: `duckdb/extension-template-rs`

**Repo:** https://github.com/duckdb/extension-template-rs
**Status:** Official (under DuckDB org)

DuckDB maintains a C++ extension template (`duckdb/extension-template`) and a Rust variant (`duckdb/extension-template-rs`). The Rust template is the prescribed starting point.

**What the template provides:**
- `Makefile` + `CMakeLists.txt` wrapping Cargo
- GitHub Actions CI that builds for Linux, macOS, and Windows
- Community extension signing workflow (required for registry submission)
- Extension entrypoint wiring (`extern "C" fn extension_init_db`, `extern "C" fn extension_version`)
- Basic example of registering a scalar function from Rust

**Build system:** CMake calls Cargo. You run `make` or `cmake --build`; the CMake layer handles DuckDB header injection and symbol export; Cargo handles your Rust code. There is no pure-Cargo path that satisfies DuckDB's extension loading requirements — the CMake layer is mandatory.

**Confidence: High** — template is official and actively maintained.

**Verification needed:** Check the template's README for the current DuckDB version it targets; ensure your DuckDB version matches what the template pins.

---

## Build System

### CMake + Cargo (no pure-Cargo alternative)

DuckDB extensions are loaded via `LOAD 'extension.duckdb_extension'` — a platform-specific shared library with specific exported C symbols. DuckDB's build infrastructure (vcpkg, cmake, extension signing) assumes CMake.

**The stack is:**
1. `Makefile` calls `cmake`
2. CMake configures the DuckDB headers and linker flags, then calls `cargo build --release`
3. Cargo produces a `.dylib` / `.so` / `.dll`
4. CMake post-processes it (symbol export, signing stub)

**Local development:** `cargo build` alone works for running tests that load the extension in-process via `duckdb-rs`. You only need the full CMake path for producing the distributable `.duckdb_extension` file.

**Rationale for not pursuing pure-Cargo:** The DuckDB extension ABI requires specific C symbol names and a signing header appended to the binary. Replicating this in Cargo alone is possible in principle but would require maintaining a build.rs that duplicates what the official CMake infrastructure does — creating maintenance burden against DuckDB version updates with no benefit.

**Confidence: High**

---

## DuckDB APIs for Key Features

### Table Functions (query syntax: `FROM my_view(...)`)

**API:** `duckdb::TableFunction` (in `duckdb-rs`)

DuckDB's table function API allows registering functions that appear in `FROM` clauses and return result sets. This is the mechanism for `FROM SEMANTIC_VIEW(...)` or `FROM my_view(DIMENSIONS ... METRICS ...)`.

**Rust support:** `duckdb-rs` exposes `TableFunction`, `BindInfo`, `InitInfo`, `FunctionInfo` — enough to register a named table function with parameters.

**Confidence: High** — table functions are the most mature part of the Rust extension API.

**Relevant example:** The `duckdb/extension-template-rs` includes a working table function example. Community extensions like `duckdb-httpfs` (C++) demonstrate the pattern in C++.

---

### Parser Hooks and Custom DDL (`CREATE SEMANTIC VIEW`)

This is the hardest part of the stack.

**The problem:** DuckDB does not expose a stable, documented Rust API for hooking into the parser or registering custom DDL statement types. `CREATE SEMANTIC VIEW` is not standard SQL — it requires either:

1. **A parser extension** — DuckDB supports parser extension callbacks (C++ `ParserExtension`) that receive the raw SQL string when the built-in parser fails. The extension can parse and return a custom statement type.
2. **Custom statement types** — Requires implementing `CustomStatement` C++ class and wiring it through DuckDB's planner/executor hooks.

**Rust exposure:** As of mid-2025, `duckdb-rs` does not expose `ParserExtension` or `CustomStatement` at the Rust level. This means:

- You must call into DuckDB's C API using `libduckdb-sys` (unsafe FFI)
- Or you write a thin C++ shim that bridges the parser extension callback into a Rust function pointer

**Practical approach (two options):**

**Option A — Parser extension via C FFI:** Use `libduckdb-sys` to call `duckdb_add_extension_option` and related parser hook APIs directly. Wrap in a `unsafe` block with a Rust-safe interface. This is what several community extensions do.

**Option B — Table function as DDL workaround:** Instead of `CREATE SEMANTIC VIEW`, implement `CREATE_SEMANTIC_VIEW(name, ...)` as a scalar or table function that writes to a metadata table. The user syntax is less clean but the implementation is entirely in stable `duckdb-rs` APIs. **Recommended for v0.1** given the complexity of Option A.

**Confidence for Option A:** Medium — it works but requires knowledge of DuckDB internal C APIs that are not versioned separately from DuckDB itself.
**Confidence for Option B:** High — completely within documented territory.

**Recommendation:** Start with Option B (table function DDL) to validate the semantic view expansion logic quickly. Implement proper `CREATE SEMANTIC VIEW` syntax (Option A) in a subsequent milestone once the core is proven.

---

### Catalog Integration (persisting semantic view definitions)

**The problem:** Semantic view definitions must survive `INSTALL`/`LOAD`, be stored somewhere durable, and be queryable.

**Options:**

1. **DuckDB catalog objects** — Register semantic views as first-class catalog entries (like views or macros). Requires `CatalogEntry` C++ integration — no Rust API available.

2. **Internal DuckDB tables** — Store definitions in a regular DuckDB table (e.g., `_semantic_views` in a hidden schema). The extension creates this table on `LOAD` if absent. Queries read from it at expansion time. Simple, entirely within `duckdb-rs` APIs.

3. **External JSON/Parquet file** — Store definitions in a sidecar file. Fragile (file location coupling) and not recommended.

**Recommendation:** Option 2 (internal DuckDB tables). Simple, persistent, transactional, and inspectable with standard SQL. This is how several community extensions handle persistent extension state.

**Schema sketch:**
```sql
CREATE TABLE IF NOT EXISTS _duckdb_semantic_views.definitions (
    view_name VARCHAR PRIMARY KEY,
    definition JSON NOT NULL,
    created_at TIMESTAMP DEFAULT current_timestamp
);
```

**Confidence: High**

---

### DDL Registration (hooks for `CREATE`/`DROP`/`ALTER`)

If implementing proper `CREATE SEMANTIC VIEW` syntax (Option A above), you need DuckDB's DDL hooks.

**DuckDB extension hooks available (C++ / C API):**
- `ParserExtension` — intercept SQL that the parser cannot parse
- `OptimizerExtension` — intercept the query plan for rewriting
- `PragmaFunction` — register PRAGMA-based extension commands
- Custom statement types via `ClientContext` hooks

**Rust exposure:** None stable. Use `libduckdb-sys` FFI.

**Confidence: Medium** — APIs exist; Rust bindings require manual FFI work.

---

## Community Extension Distribution

### DuckDB Community Extension Registry

**URL:** https://community-extensions.duckdb.org / https://extensions.duckdb.org

DuckDB maintains a community extension registry separate from its core extensions. Extensions in the registry are installable via:

```sql
INSTALL my_extension FROM community;
LOAD my_extension;
```

**Submission process:**
1. Fork the `duckdb/community-extensions` repository
2. Add a descriptor file (`my_extension.yaml`) declaring your extension
3. The registry CI builds your extension against supported DuckDB versions/platforms
4. Extensions are signed with DuckDB's extension signing key

**Signing:** DuckDB extensions are signed. The community registry handles signing as part of its CI. Self-distributed extensions require either being unsigned (users must set `SET allow_unsigned_extensions = true`) or obtaining a signing key.

**Platform matrix:** The registry CI builds for:
- Linux x86_64, Linux aarch64
- macOS x86_64, macOS arm64
- Windows x86_64

**DuckDB version pinning:** Each submission must declare which DuckDB version(s) it targets. The ABI is version-specific — an extension built for DuckDB 1.0 will not load in DuckDB 1.1 without a rebuild.

**Confidence: High**

---

## Supporting Crates

### `sqlparser` (sqlparser-rs)

**Crate:** `sqlparser`
**Repo:** https://github.com/apache/datafusion-sqlparser-rs (now under Apache)
**Version (as of Aug 2025):** ~0.47.x — verify on crates.io
**License:** Apache 2.0

Needed for parsing user-supplied SQL expressions inside semantic view definitions (dimension expressions, metric expressions, filter clauses). DuckDB's own parser is C++ and not accessible from Rust; `sqlparser-rs` provides a pure-Rust SQL parser.

**Caveat:** `sqlparser-rs` uses a generic SQL dialect model. DuckDB's dialect has extensions (e.g., `EXCLUDE`, `REPLACE`, positional column references) that may not parse correctly. Use `sqlparser::dialect::GenericDialect` or `DuckDbDialect` if available; test DuckDB-specific syntax carefully.

**Confidence: Medium** — the crate is mature but DuckDB dialect coverage may have gaps for advanced syntax.

**Alternative:** Do not parse SQL expressions at all in v0.1 — store them as opaque strings in the definition, interpolate them into the expanded SQL at query time, and let DuckDB validate them. This avoids the `sqlparser-rs` dependency entirely and reduces scope.

---

### `serde` + `serde_json`

**Crates:** `serde` (~1.0), `serde_json` (~1.0)
**Confidence: High**

For serializing/deserializing semantic view definitions stored as JSON in the DuckDB catalog table. Stable, mature, ubiquitous.

---

### `thiserror`

**Crate:** `thiserror` (~1.x or ~2.x — check current version)
**Confidence: High**

Extension error types. `thiserror` for library-style error definitions; prefer it over `anyhow` for extension code where errors propagate to DuckDB as error messages.

---

### `uuid`

**Crate:** `uuid` (~1.x)
**Confidence: High**

If semantic view definitions need stable identifiers for catalog references.

---

## What NOT to Use

### Pure Rust DuckDB bindings (not `duckdb-rs`)

There is no viable alternative to `duckdb-rs` for DuckDB extension development. Do not attempt to build against DuckDB's C API manually without `libduckdb-sys` as the base — it exists and is the right foundation.

### `arrow-rs` for in-extension computation

The `duckdb-rs` extension API provides `DataChunk` access for passing data between DuckDB and your extension. You do not need `arrow-rs` unless you are computing derived values in Rust. For this project (which is a preprocessor — SQL expansion only), you output SQL strings to DuckDB, not Arrow data. Avoid the `arrow-rs` dependency.

### `egg` / e-graph rewriting

As established in the design doc: `egg` solves the problem of recognizing and rewriting arbitrary SQL from diverse clients into a canonical form. This project has structured input (`SEMANTIC_VIEW(...)` with explicit dimensions/metrics) and deterministic expansion. `egg` is not needed and would be heavy over-engineering.

### `datafusion`

Apache DataFusion is a query engine. This project is a preprocessor for DuckDB — DuckDB is the engine. DataFusion would introduce a competing query engine as a dependency. Not appropriate.

### `nom` or `pest` for SQL parsing

Unless you need to parse a custom mini-language (e.g., the `DIMENSIONS ... METRICS ...` clause syntax inside the table function call), avoid writing a custom SQL parser. DuckDB parses the outer SQL; your extension receives structured parameters via the table function `BindInfo` API. If you do need light parsing, `nom` is fine; avoid `pest` (heavier, code-gen step).

---

## Known Gaps and Rough Edges

### 1. No stable Rust API for parser hooks

**Gap:** The biggest limitation. `CREATE SEMANTIC VIEW` requires either parser extension hooks or a workaround. The `duckdb-rs` crate does not expose these. Implementing them requires unsafe C FFI or a C++ shim.

**Impact on this project:** High. If you want clean DDL syntax for v0.1, expect to write unsafe code. The workaround (table function as DDL) defers this.

**Workaround for v0.1:** Use a scalar function or table function to create/drop semantic views instead of DDL syntax.

### 2. DuckDB ABI is not stable across minor versions

**Gap:** Extensions are compiled against a specific DuckDB version and will not load in other versions. The community registry handles multi-version builds, but local development and early testing require version discipline.

**Impact:** You must pin `duckdb` crate version to a specific DuckDB version and keep them in sync. Upgrades require a full rebuild and potentially API changes.

### 3. Extension signing for local distribution

**Gap:** Before community registry acceptance, your extension will be unsigned. Users must run `SET allow_unsigned_extensions = true` to load it. This is fine for development but is a friction point for early adopters.

### 4. `duckdb-rs` extension API surface is small

**Gap:** The `duckdb-rs` extension API covers table functions and scalar functions reasonably well. Anything beyond that (optimizer hooks, parser hooks, custom types, catalog entries) requires dropping into `libduckdb-sys` FFI. The safe Rust layer is thin.

**Impact:** Expect to write `unsafe` code for DDL hooks and any catalog-level integration. Budget time for this.

### 5. `sqlparser-rs` DuckDB dialect gaps

**Gap:** DuckDB's SQL dialect has extensions (`EXCLUDE columns`, `REPLACE`, struct access via `.`, list slicing) that `sqlparser-rs` may not fully support. If semantic view definitions can contain DuckDB-specific SQL expressions, parsing them in Rust may be unreliable.

**Workaround:** Treat SQL expressions in definitions as opaque strings; do not parse them in Rust. Interpolate them directly into expanded SQL and let DuckDB validate.

### 6. No mature DuckDB Rust extension examples with DDL

**Gap:** Most existing Rust DuckDB extensions are simple (scalar functions, basic table functions). There are no well-documented examples of parser extension hooks implemented in Rust. The C++ ecosystem has more examples.

**Implication:** You may need to study C++ community extension code and translate patterns to Rust FFI.

---

## Recommended Starter Stack for v0.1

```toml
[dependencies]
duckdb = { version = "0.10", features = ["extensions", "vtab"] }

serde = { version = "1", features = ["derive"] }
serde_json = "1"

thiserror = "2"

# Optional: only if parsing SQL expressions in definitions
# sqlparser = { version = "0.47", features = [] }

[build-dependencies]
# none — CMake template handles build configuration
```

**Template:** Start from `duckdb/extension-template-rs` on GitHub. Clone it, rename, and build on top.

**v0.1 DDL approach:** Implement `CREATE_SEMANTIC_VIEW(name, definition_json)` as a scalar function that inserts into an internal catalog table. Use `DROP_SEMANTIC_VIEW(name)` similarly. Ship real `CREATE SEMANTIC VIEW` syntax in v0.2 once the expansion logic is proven.

---

## Verification Checklist

The following items were identified during research as requiring live verification (training data cutoff: August 2025):

- [ ] **`duckdb` crate current version** — check crates.io for latest `duckdb` version and the DuckDB version it bundles
- [ ] **`extension-template-rs` current state** — check `duckdb/extension-template-rs` README for DuckDB version it targets and any changes to build system since mid-2025
- [ ] **`sqlparser-rs` current version** — check crates.io for `sqlparser`; confirm DuckDB dialect support status
- [ ] **Community extension registry process** — check `duckdb/community-extensions` repo for current submission requirements and CI matrix
- [ ] **Parser extension Rust exposure** — check `duckdb-rs` changelog/issues for any new `ParserExtension` bindings added after August 2025
- [ ] **DuckDB version to target** — determine which DuckDB stable version to build against (1.0.x, 1.1.x, etc.) and align crate version

---

## References

- `duckdb/duckdb-rs` — https://github.com/duckdb/duckdb-rs
- `duckdb/extension-template-rs` — https://github.com/duckdb/extension-template-rs
- `duckdb/extension-template` (C++ reference) — https://github.com/duckdb/extension-template
- `duckdb/community-extensions` — https://github.com/duckdb/community-extensions
- DuckDB extension docs — https://duckdb.org/docs/dev/extensions/creating_extensions.html
- `apache/datafusion-sqlparser-rs` — https://github.com/apache/datafusion-sqlparser-rs
- DuckDB parser extension API (C++) — https://duckdb.org/docs/dev/extensions/parser_extensions.html
