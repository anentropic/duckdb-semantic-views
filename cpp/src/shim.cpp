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
#include <atomic>
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
    //   2 = not ours: defer to default parser.
    //
    // `db_token` identifies which database's catalog connection to use for
    // existence checks and CREATE-time enrichment. Each extension load is
    // assigned a unique token (see sv_register_parser_hooks) so multi-DB
    // processes (e.g. Python tests opening successive databases) don't share
    // a stale catalog connection.
    uint8_t sv_parser_override_rust(
        uint64_t db_token,
        const char *query_ptr, size_t query_len,
        char **sql_out_ptr, size_t *sql_out_len,
        char *error_out, size_t error_out_len);

    // Releases a buffer previously produced by sv_parser_override_rust.
    // Safe to call with a null pointer (no-op). ptr/len must be the exact
    // pair the Rust side returned.
    void sv_free_buffer(char *ptr, size_t len);
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
// `db_token` selects the right catalog connection on the Rust side; we
// generate a fresh token on every sv_register_parser_hooks call so each
// loaded database has an isolated entry in the Rust-side connection map.
struct SemanticViewsParserInfo : public ParserExtensionInfo {
    uint64_t db_token;
    explicit SemanticViewsParserInfo(uint64_t token) : db_token(token) {}
};

static std::atomic<uint64_t> sv_next_db_token{1};

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

    // Identify which DB's catalog connection this query should use. info is
    // the per-extension-load SemanticViewsParserInfo we attached at
    // registration time; if missing we cannot route correctly, so defer.
    auto *sv_info = dynamic_cast<SemanticViewsParserInfo *>(info);
    if (!sv_info) {
        return ParserOverrideResult();
    }

    SvOwnedBuffer sql_buf;
    char error_buf[1024];
    memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_parser_override_rust(
        sv_info->db_token,
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
// parser_override hook on DBConfig. Allocates a fresh `db_token` so the
// Rust side can route per-database catalog reads correctly.
extern "C" {
    bool sv_register_parser_hooks(duckdb_database db_handle,
                                  uint64_t *out_db_token) {
        try {
            // duckdb_database -> internal_ptr -> DatabaseWrapper ->
            //   shared_ptr<DuckDB> -> shared_ptr<DatabaseInstance>
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            auto &db = *wrapper->database->instance;

            uint64_t token = sv_next_db_token.fetch_add(1, std::memory_order_relaxed);
            if (out_db_token) {
                *out_db_token = token;
            }

            ParserExtension ext;
            ext.parser_override = sv_parser_override;
            // duckdb::shared_ptr<T> doesn't have an upcast constructor, so we
            // build the std::shared_ptr<ParserExtensionInfo> first (allocates
            // the SemanticViewsParserInfo and immediately upcasts) then hand
            // it to duckdb::shared_ptr's std-interop constructor.
            std::shared_ptr<ParserExtensionInfo> info_std(
                new SemanticViewsParserInfo(token));
            ext.parser_info = duckdb::shared_ptr<ParserExtensionInfo>(info_std);
            auto &config = DBConfig::GetConfig(db);
            ParserExtension::Register(config, ext);

            // Enable parser_override dispatch in FALLBACK mode so our hook
            // runs *before* the default parser for every query; a miss
            // (DISPLAY_ORIGINAL_ERROR) cleanly falls through to it. Validation
            // errors are surfaced via a synthesised SELECT error('...') from
            // the Rust side because FALLBACK silently drops DISPLAY_EXTENSION_ERROR
            // (verified in v1.5.2 amalgamation ParseInternal). STRICT does
            // honour DISPLAY_EXTENSION_ERROR but its Throw() path loses the
            // ParserException::SyntaxError formatting (LINE/^ caret) that
            // legacy parse_function used, so STRICT doesn't actually win us
            // anything user-visible — see TECH-DEBT item 22.
            config.SetOption("allow_parser_override_extension", Value("FALLBACK"));

            return true;
        } catch (const std::exception &e) {
            fprintf(stderr, "sv_register_parser_hooks failed: %s\n", e.what());
            return false;
        }
    }
}
