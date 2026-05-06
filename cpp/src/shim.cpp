// C++ helper for the DuckDB semantic_views extension.
//
// The Rust entry point (semantic_views_init_c_api, C_STRUCT ABI) owns the
// DuckDB handshake and function registration. After init, it calls
// sv_register_parser_hooks() here to install a `parser_override` callback —
// the sole DDL entry point as of v0.8.1's full unification (the legacy
// `parse_function` / `sv_ddl_internal` table-function fallback was retired
// once parser_override could route every recognised DDL form including
// DESCRIBE / SHOW SEMANTIC * via pass-through).
//
// All DuckDB C++ symbols are provided by compiling duckdb.cpp (the
// amalgamation source) alongside this file. Symbol visibility on the cdylib
// restricts exports to just the Rust entry point, so these definitions stay
// internal to the binary.
//
// DuckDB 1.5.0 moved the parser extension type declarations from duckdb.hpp
// into duckdb.cpp. The compat header re-declares them so this translation
// unit can use them. See cpp/include/parser_extension_compat.hpp for details.

#include "parser_extension_compat.hpp"
#include <cstdint>
#include <memory>

using namespace duckdb;

// ---------------------------------------------------------------------------
// Rust FFI declarations (defined in src/parse.rs)
// ---------------------------------------------------------------------------
extern "C" {
    // Parser-override DDL rewrite. For recognized semantic-view DDL emits
    // native SQL (INSERT/DELETE/UPDATE on _definitions for write-side DDL,
    // or `SELECT * FROM <read_side_table_function>(...)` pass-through for
    // DESCRIBE/SHOW). Returns:
    //   0 = success: heap-owned (ptr, len) written to *sql_out_ptr/*sql_out_len.
    //                Caller takes ownership and MUST release via sv_free_buffer.
    //                Buffer is NOT NUL-terminated — read exactly *sql_out_len bytes.
    //   1 = error:   message written to error_out (NUL-terminated, capped at
    //                error_out_len-1 bytes); *sql_out_ptr left untouched.
    //                (Currently unused under FALLBACK_OVERRIDE — kept for
    //                Phase 62 Plan 03 once parse_function returns.)
    //   2 = not ours: defer to default parser. Also used for null ctx_ptr.
    //
    // Phase 62: ctx_ptr is an opaque Box<OverrideContext>* produced by
    // sv_make_override_context. It carries the catalog connection +
    // is_file_backed flag for THIS database. The legacy db_token LRU
    // lookup is gone (TECH-DEBT 20).
    uint8_t sv_parser_override_rust(
        const void *ctx_ptr,
        const char *query_ptr, size_t query_len,
        char **sql_out_ptr, size_t *sql_out_len,
        char *error_out, size_t error_out_len);

    // Releases a buffer previously produced by sv_parser_override_rust.
    // Safe to call with a null pointer (no-op). ptr/len must be the exact
    // pair the Rust side returned.
    void sv_free_buffer(char *ptr, size_t len);

    // Phase 62: Box<OverrideContext> ownership FFI. The Rust side allocates
    // a Box<OverrideContext> wrapping the duckdb_connection + is_file_backed
    // flag and returns the leaked raw pointer. The C++ shim stashes the
    // pointer inside SemanticViewsParserInfo::rust_state and hands it back
    // to sv_parser_override_rust on every parse. ~SemanticViewsParserInfo
    // calls sv_drop_override_context to free the Rust allocation.
    //
    // CRITICAL: sv_drop_override_context does NOT call duckdb_disconnect on
    // the inner duckdb_connection — see Phase 62 RESEARCH.md §Q2 for the
    // destruction-order rationale. The Connection object leaks for the
    // remainder of process life (one Connection per DB ever opened).
    void *sv_make_override_context(duckdb_connection conn, bool is_file_backed);
    void  sv_drop_override_context(void *ctx_ptr);
}

// RAII guard for heap-owned buffers returned by the Rust FFI. Ensures the
// buffer is released even if a downstream call (Parser::ParseQuery) throws.
struct SvOwnedBuffer {
    char *ptr = nullptr;
    size_t len = 0;
    SvOwnedBuffer() = default;
    SvOwnedBuffer(const SvOwnedBuffer &) = delete;
    SvOwnedBuffer &operator=(const SvOwnedBuffer &) = delete;
    SvOwnedBuffer(SvOwnedBuffer &&other) noexcept
        : ptr(other.ptr), len(other.len) {
        other.ptr = nullptr;
        other.len = 0;
    }
    SvOwnedBuffer &operator=(SvOwnedBuffer &&other) noexcept {
        if (this != &other) {
            if (ptr) sv_free_buffer(ptr, len);
            ptr = other.ptr;
            len = other.len;
            other.ptr = nullptr;
            other.len = 0;
        }
        return *this;
    }
    ~SvOwnedBuffer() {
        if (ptr) sv_free_buffer(ptr, len);
    }
    string to_string() const {
        return ptr ? string(ptr, len) : string();
    }
};

// Per-extension-load info struct attached to ParserExtension::parser_info.
// Holds an opaque Box<OverrideContext>* (rust_state) that owns the catalog
// connection + is_file_backed flag for THIS database. Lifetime is tied to
// DBConfig, so destruction fires on DB unload — no LRU needed (Phase 62
// resolved TECH-DEBT 20).
struct SemanticViewsParserInfo : public ParserExtensionInfo {
    void *rust_state;  // Box<OverrideContext>* opaque pointer (Rust-owned).
    explicit SemanticViewsParserInfo(void *state) : rust_state(state) {}

    ~SemanticViewsParserInfo() override {
        if (rust_state) {
            sv_drop_override_context(rust_state);
            rust_state = nullptr;
        }
        // CRITICAL — Phase 62 Q2 destruction-order showstopper:
        // We deliberately do NOT call duckdb_disconnect on the
        // duckdb_connection contained within OverrideContext's CatalogReader.
        //
        // By the time this destructor fires, ~DatabaseInstance has already
        // reset connection_manager (duckdb.cpp:276819). ~Connection() would
        // call ConnectionManager::RemoveConnection() on the destroyed
        // manager — use-after-free.
        //
        // The Rust Drop impl on OverrideContext (in src/parse.rs) documents
        // the same constraint. The duckdb_connection object leaks for the
        // remainder of process life — bounded at one Connection per DB ever
        // opened (~few KB each). This matches v0.8.0 commit 680a967 which
        // shipped successfully with this same leak pattern.
        //
        // See .planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md §Q2.
        // Resolves TECH-DEBT item 20 (silent LRU eviction class) by removing the LRU.
    }
};

// ---------------------------------------------------------------------------
// Parser-override hook: sv_parser_override
// ---------------------------------------------------------------------------
// The sole DDL entry point. Runs *before* the default parser. Recognized
// semantic-view DDL is rewritten into native SQL by the Rust side and
// re-parsed via DuckDB's own Parser, producing SQLStatement ASTs that DuckDB
// then plans and executes on the caller's connection — so write-side DDL
// participates in the caller's transaction.
//
// For non-matching queries returns DISPLAY_ORIGINAL_ERROR so DuckDB falls
// through to the default parser.
static ParserOverrideResult sv_parser_override(
    ParserExtensionInfo *info, const string &query, ParserOptions &) {

    // Identify the OverrideContext for this DB's catalog. info is the
    // per-extension-load SemanticViewsParserInfo attached at registration
    // time; if missing or its Rust state is null we cannot route correctly,
    // so defer to the default parser.
    auto *sv_info = dynamic_cast<SemanticViewsParserInfo *>(info);
    if (!sv_info || !sv_info->rust_state) {
        return ParserOverrideResult();
    }

    SvOwnedBuffer sql_buf;
    char error_buf[1024];
    memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_parser_override_rust(
        sv_info->rust_state,
        query.c_str(), query.size(),
        &sql_buf.ptr, &sql_buf.len,
        error_buf, sizeof(error_buf));

    if (rc == 2) {
        // Not our query — let DuckDB's default parser handle it.
        return ParserOverrideResult();
    }

    if (rc == 1) {
        // Validation error or near-miss suggestion — propagate the message
        // via DISPLAY_EXTENSION_ERROR.
        std::runtime_error err(error_buf);
        return ParserOverrideResult(err);
    }

    // rc == 0: native SQL produced. Re-parse via DuckDB's Parser. Use
    // default-constructed ParserOptions so parser_override doesn't recurse
    // (DEFAULT_OVERRIDE skips parser_override hooks entirely). The
    // rewritten SQL is read by exact length, so size is unbounded.
    string native_sql = sql_buf.to_string();
    try {
        Parser parser;
        parser.ParseQuery(native_sql);
        return ParserOverrideResult(std::move(parser.statements));
    } catch (std::exception &e) {
        return ParserOverrideResult(e);
    }
}

// ---------------------------------------------------------------------------
// sv_register_parser_hooks -- called from Rust after C API init
// ---------------------------------------------------------------------------
// Extracts DatabaseInstance& from the C API handle and registers the
// parser_override hook on DBConfig. Phase 62: takes the catalog connection
// + is_file_backed flag and bundles them into an OverrideContext via
// sv_make_override_context. The boxed OverrideContext is owned by the
// SemanticViewsParserInfo we register; lifetime is tied to DBConfig so
// destruction fires on DB unload (no LRU needed — TECH-DEBT 20 resolved).
extern "C" {
    bool sv_register_parser_hooks(duckdb_database db_handle,
                                  duckdb_connection catalog_conn,
                                  bool is_file_backed) {
        try {
            // duckdb_database -> internal_ptr -> DatabaseWrapper ->
            //   shared_ptr<DuckDB> -> shared_ptr<DatabaseInstance>
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            auto &db = *wrapper->database->instance;

            void *rust_state = sv_make_override_context(catalog_conn, is_file_backed);
            if (!rust_state) {
                fprintf(stderr,
                    "sv_register_parser_hooks: sv_make_override_context returned null\n");
                return false;
            }

            ParserExtension ext;
            ext.parser_override = sv_parser_override;
            // duckdb::shared_ptr<T> doesn't have an upcast constructor, so we
            // build the std::shared_ptr<ParserExtensionInfo> first (allocates
            // the SemanticViewsParserInfo and immediately upcasts) then hand
            // it to duckdb::shared_ptr's std-interop constructor.
            std::shared_ptr<ParserExtensionInfo> info_std(
                new SemanticViewsParserInfo(rust_state));
            ext.parser_info = duckdb::shared_ptr<ParserExtensionInfo>(info_std);
            auto &config = DBConfig::GetConfig(db);
            ParserExtension::Register(config, ext);

            // FALLBACK_OVERRIDE: hook runs before default parser; misses
            // fall through cleanly. Caret regression (TECH-DEBT 22) is
            // resolved separately in Phase 62 Plan 03 by re-introducing
            // parse_function as the error-reporting layer.
            config.SetOption("allow_parser_override_extension", Value("FALLBACK"));

            return true;
        } catch (const std::exception &e) {
            fprintf(stderr, "sv_register_parser_hooks failed: %s\n", e.what());
            return false;
        }
    }
}
