// C++ helper for the DuckDB semantic_views extension.
//
// The Rust entry point (semantic_views_init_c_api, C_STRUCT ABI) owns the
// DuckDB handshake and function registration. After init, it calls
// sv_register_parser_hooks() here to install:
//
//   * parser_override (sv_parser_override) — the success path. Recognised
//     CREATE / DROP / ALTER / DESCRIBE / SHOW DDL is rewritten to native SQL
//     and re-parsed on the caller's connection (transactional behaviour).
//   * parse_function (sv_parse_stub) — Phase 62 Plan 03 error-reporting
//     layer. Called by DuckDB after the default parser fails on an
//     unrecognised prefix; re-runs validation and returns
//     DISPLAY_EXTENSION_ERROR with `error_location` so
//     ParserException::SyntaxError can render `LINE 1: … ^` (caret).
//   * plan_function (sv_plan_unreachable) — required sibling of
//     parse_function; should never fire because sv_parse_stub never returns
//     PARSE_SUCCESSFUL.
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
#include <cstring>
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
    //                (Phase 62: unused — Err branches now return rc=2 and
    //                let parse_function render caret via DISPLAY_EXTENSION_ERROR.)
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

    // Phase 62 Plan 03: parse_function callback. Called by DuckDB after the
    // default parser fails on an unrecognised prefix. Re-runs validation
    // and returns rc + error message + byte-offset position so
    // ParserException::SyntaxError can render `LINE 1: ... ^`.
    // Return codes:
    //   0 = success/unreachable (defensive)
    //   1 = ours-but-invalid (validation error or near-miss). error_buf
    //       gets the message, position_out gets the byte offset (or
    //       UINT32_MAX if no position is available).
    //   2 = not ours; defer (DISPLAY_ORIGINAL_ERROR on the C++ side).
    //   3 = valid DDL but parser_override didn't fire (e.g. override
    //       setting reset by disable_peg_parser). error_buf gets an
    //       actionable hint; position_out gets 0.
    uint8_t sv_parse_function_rust(
        const void *ctx_ptr,
        const char *query_ptr, size_t query_len,
        char *error_buf, size_t error_buf_len,
        uint32_t *position_out);

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

// ---------------------------------------------------------------------------
// SemanticViewParseData — Phase 62 Plan 03
// ---------------------------------------------------------------------------
// Concrete subclass of ParserExtensionParseData required by the
// ParserExtensionParseResult(unique_ptr<...>) constructor. We never return
// PARSE_SUCCESSFUL from sv_parse_stub (its sole purpose is rendering errors
// via DISPLAY_EXTENSION_ERROR), so this type is structurally needed only
// for layout / type-system reasons. If we ever do produce a parse_data,
// sv_plan_unreachable below would fire.
struct SemanticViewParseData : public ParserExtensionParseData {
    string query;
    explicit SemanticViewParseData(string q) : query(std::move(q)) {}

    unique_ptr<ParserExtensionParseData> Copy() const override {
        return make_uniq<SemanticViewParseData>(query);
    }
    string ToString() const override {
        return query;
    }
};

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
        // Phase 62 Plan 03: defer to default parser. parse_function
        // (registered as sv_parse_stub) re-runs validation and returns
        // DISPLAY_EXTENSION_ERROR with caret position. The Rust side
        // currently always returns rc=2 on the error path, so this
        // branch is defensive — kept to match the documented contract.
        return ParserOverrideResult();
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
// Parse-function hook: sv_parse_stub  (Phase 62 Plan 03)
// ---------------------------------------------------------------------------
// Called by DuckDB's Parser::ParseQuery after the default parser fails on
// an unrecognised prefix. Sole purpose is rendering caret-aware errors via
// ParserException::SyntaxError(query, msg, error_location).
//
// parser_override remains the success path — it rewrites recognized DDL
// to native SQL, re-parses on the caller's connection, and returns
// PARSE_SUCCESSFUL (transactional behaviour preserved). For error cases
// parser_override now ALWAYS returns DISPLAY_ORIGINAL_ERROR (rc=2 from
// Rust), letting the default parser fail and DuckDB call this stub.
//
// We re-run validation through sv_parse_function_rust which returns:
//   0 — defensive internal error; render as DISPLAY_EXTENSION_ERROR
//   1 — ours-but-invalid: error_buf populated; position is byte offset
//   2 — not ours: DISPLAY_ORIGINAL_ERROR (let the default parser's error stand)
//   3 — valid-but-override-disabled: actionable hint; position=0
static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *info, const string &query) {
    auto *sv_info = dynamic_cast<SemanticViewsParserInfo *>(info);
    // ctx_ptr is unused by sv_parse_function_rust today (validation does
    // not need the catalog), so a null sv_info / rust_state is OK — we
    // still get correct rc/error/position. Pass through whatever we have.
    const void *ctx = (sv_info != nullptr) ? sv_info->rust_state : nullptr;

    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint32_t position = UINT32_MAX;

    uint8_t rc = sv_parse_function_rust(
        ctx,
        query.c_str(), query.size(),
        error_buf, sizeof(error_buf),
        &position);

    switch (rc) {
        case 2:
            // Not ours — DISPLAY_ORIGINAL_ERROR (default parser's error
            // text + caret stands).
            return ParserExtensionParseResult();

        case 1:
        case 3: {
            // ours-but-invalid OR valid-but-override-disabled. Both want
            // DISPLAY_EXTENSION_ERROR with caret position, just different
            // message text. Construct the std::string explicitly first to
            // dodge C++'s "most vexing parse" — `ParserExtensionParseResult
            // result(string(error_buf));` would otherwise be parsed as a
            // function declaration.
            string msg(error_buf);
            ParserExtensionParseResult result(std::move(msg));
            if (position != UINT32_MAX) {
                result.error_location = optional_idx(position);
            }
            return result;
        }

        case 0: {
            // Defensive — sv_parse_function_rust currently never returns 0.
            // Map to DISPLAY_EXTENSION_ERROR with an internal-error message
            // rather than letting a silent default-parser error escape.
            return ParserExtensionParseResult(string(
                "semantic_views: internal error — sv_parse_function_rust "
                "returned rc=0 (please report this bug)"));
        }

        default: {
            // Defensive — unknown rc. Same handling as rc=0.
            return ParserExtensionParseResult(string(
                "semantic_views: internal error — unknown rc from "
                "sv_parse_function_rust"));
        }
    }
}

// ---------------------------------------------------------------------------
// Plan-function hook: sv_plan_unreachable  (Phase 62 Plan 03)
// ---------------------------------------------------------------------------
// ParserExtension carries a sibling `plan_function` pointer alongside
// `parse_function`. The plan function only fires when parse_function returns
// PARSE_SUCCESSFUL, which sv_parse_stub never does — every code path goes
// through DISPLAY_EXTENSION_ERROR or DISPLAY_ORIGINAL_ERROR. Provide a
// hard-fail stub so a contract violation surfaces loudly rather than silently.
//
// Signature must match plan_function_t in parser_extension_compat.hpp:
//   ParserExtensionPlanResult (*)(ParserExtensionInfo *, ClientContext &,
//                                  unique_ptr<ParserExtensionParseData>);
static ParserExtensionPlanResult sv_plan_unreachable(
    ParserExtensionInfo * /*info*/,
    ClientContext & /*context*/,
    unique_ptr<ParserExtensionParseData> /*parse_data*/) {
    throw InternalException(
        "semantic_views: sv_plan_unreachable called — sv_parse_stub never "
        "returns PARSE_SUCCESSFUL (please report this bug)");
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
            // Phase 62 Plan 03: parse_function is the error-reporting layer.
            // parser_override owns the success path (rewrite + re-parse on the
            // caller's connection — transactional). When parser_override
            // defers (rc=2), the default parser fails on the unrecognised
            // prefix and DuckDB calls sv_parse_stub, which re-runs validation
            // and returns DISPLAY_EXTENSION_ERROR with `error_location` so
            // ParserException::SyntaxError renders `LINE 1: … ^` (caret).
            // sv_plan_unreachable is the required sibling — sv_parse_stub
            // never returns PARSE_SUCCESSFUL so it should never fire.
            ext.parse_function  = sv_parse_stub;
            ext.plan_function   = sv_plan_unreachable;
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
            // resolved by parse_function above — sv_parse_stub renders
            // `LINE 1: ... ^` for every CREATE/DROP/ALTER validation error.
            config.SetOption("allow_parser_override_extension", Value("FALLBACK"));

            return true;
        } catch (const std::exception &e) {
            fprintf(stderr, "sv_register_parser_hooks failed: %s\n", e.what());
            return false;
        }
    }
}
