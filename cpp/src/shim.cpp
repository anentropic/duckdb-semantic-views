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
#include "shim.hpp"
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

    // Phase 65 Plan 04 (Task 2 Step C) — Rust FFI bridge for the
    // `__sv_compute_create_from_yaml` helper TF. Reads file content from
    // the bind callback (via Connection probe(*context.db) + read_text(?))
    // and parses + enriches the YAML on the Rust side, returning the
    // metadata-less JSON definition as a heap-owned (ptr, len) buffer.
    //
    // The outer parser_override INSERT wraps `new_def` with json_merge_patch
    // to add now()/current_database()/current_schema() on the caller's
    // connection (matching the metadata-via-SQL pattern landed in Plan 03
    // for the inline CREATE path).
    //
    // Parameters:
    //   content_ptr/len — YAML bytes loaded by the C++ bind callback.
    //   name_ptr/len    — view name (bare identifier).
    //   comment_ptr/len — optional COMMENT='...' value; pass len=0 for
    //                     "no comment provided" (the helper leaves
    //                     `def.comment` untouched in that case so the
    //                     YAML's own `comment:` field, if any, survives).
    //   kind            — 0 = CREATE, 1 = OR REPLACE, 2 = IF NOT EXISTS.
    //                     Currently unused by the Rust helper (the outer
    //                     parser_override INSERT shape encodes ON CONFLICT
    //                     behaviour) but threaded for forward compat with
    //                     future variants whose enrichment differs.
    //   out_ptr/out_len — on rc=0, point to a heap-owned UTF-8 buffer
    //                     (NOT NUL-terminated) holding the JSON definition.
    //                     Caller MUST release via `sv_free_buffer` with
    //                     the exact (ptr, len) pair Rust returned.
    //   error_buf       — on rc!=0, gets a NUL-terminated error message
    //                     capped at `error_buf_len-1` bytes. Untouched
    //                     on success.
    //
    // Return codes:
    //   0 — success; (out_ptr, out_len) populated.
    //   1 — YAML parse / size-cap error; error_buf populated.
    //   2 — enrichment / validation error; error_buf populated.
    //   3 — internal error (panic across FFI, allocation failure, etc.);
    //       error_buf populated.
    uint8_t sv_compute_create_from_yaml_rust(
        const uint8_t *content_ptr, size_t content_len,
        const uint8_t *name_ptr, size_t name_len,
        const uint8_t *comment_ptr, size_t comment_len,
        uint8_t kind,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 (Task 1 / Wave 0 bridge spike) — Rust dispatcher for
    // `list_semantic_views()`. The C++ bind callback opens a per-call
    // `Connection probe(*context.db)` and bridges by casting the stack
    // `Connection *` to `duckdb_connection` (cast confirmed by
    // duckdb.cpp:266432-266447 where `duckdb_connect` is literally
    // `reinterpret_cast<duckdb_connection>(new Connection(...))`, so the
    // C-API handle is a borrowed pointer to a C++ `Connection`).
    //
    // The bridge is a BORROW, not a transfer: the Rust dispatcher does NOT
    // call `duckdb_disconnect` (which would `delete` the stack Connection
    // and segfault). When the C++ bind scope ends, the `probe` local's
    // destructor runs and `~Connection()` does the correct teardown.
    //
    // The dispatcher serializes the result rows into a length-prefixed
    // binary buffer (`u32 row_count; for each row: for each of 6 cols: u32
    // byte_len; bytes...`) which the C++ bind parses into BindData. The
    // exec callback then emits rows from BindData. Using a flat binary
    // wire format avoids needing matched struct layouts across the FFI
    // boundary.
    //
    // Return codes:
    //   0 — success; (out_ptr, out_len) populated with the binary buffer.
    //        Caller MUST release via `sv_free_buffer`.
    //   1 — catalog read error; error_buf populated.
    //   2 — internal error (panic across FFI); error_buf populated.
    uint8_t sv_list_semantic_views_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 Task 2 (Wave 1) — Rust dispatcher for the migrated
    // `list_terse_semantic_views()` table function. 5-column subset of
    // list_semantic_views (no `comment`). Same bridge mechanism and borrow
    // contract as the Wave 0 spike.
    uint8_t sv_list_terse_semantic_views_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
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
// sv_register_table_function — Phase 65 Plan 04 (A2 resolution)
// ---------------------------------------------------------------------------
// Reusable C-callable wrapper around the C++ Catalog API table-function
// registration pattern proven by `65-READ-PATH-SPIKE.md`. Bind callbacks
// registered via this path receive a native `ClientContext &` (not the
// duckdb-rs `BindInfo` wrapper which marshals `ClientContext` away), so
// they can open per-call `Connection(*context.db)` for catalog reads and
// YAML parsing without needing a long-lived extension-owned connection.
//
// Plan 04 consumes this to register `__sv_compute_create_from_yaml`. Plan 05
// will consume it (and add a scalar sibling) to migrate the 17 read-side
// callbacks off `query_conn` (H2).
//
// Header declaration in cpp/src/shim.hpp.
extern "C" {
    bool sv_register_table_function(
        duckdb_database db_handle,
        const char *name,
        const LogicalType *arg_types,
        size_t arg_count,
        table_function_bind_t bind_cb,
        table_function_t exec_cb,
        table_function_init_local_t init_cb) {
        try {
            if (db_handle == nullptr || name == nullptr ||
                bind_cb == nullptr || exec_cb == nullptr) {
                fprintf(stderr,
                    "sv_register_table_function: null required argument\n");
                return false;
            }
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            if (wrapper == nullptr) {
                fprintf(stderr,
                    "sv_register_table_function: null DatabaseWrapper\n");
                return false;
            }
            auto &db = *wrapper->database->instance;

            vector<LogicalType> args;
            args.reserve(arg_count);
            for (size_t i = 0; i < arg_count; ++i) {
                args.push_back(arg_types[i]);
            }

            // Six-arg TableFunction ctor: (name, args, function, bind,
            // init_global, init_local). init_global is nullptr; init_local
            // may be null when the helper TF has no per-execution local
            // state (e.g. simple one-row emitters like the YAML helper).
            TableFunction tf(
                std::string(name),
                std::move(args),
                exec_cb,
                bind_cb,
                /*init_global*/ nullptr,
                init_cb);

            CreateTableFunctionInfo info(tf);
            // ALTER_ON_CONFLICT: extension reload (LOAD semantic_views after
            // a previous LOAD in the same process) replaces the registration
            // cleanly instead of throwing on duplicate name.
            info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;

            auto &system_catalog = Catalog::GetSystemCatalog(db);
            auto txn = CatalogTransaction::GetSystemTransaction(db);
            system_catalog.CreateTableFunction(txn, info);
            return true;
        } catch (const std::exception &e) {
            fprintf(stderr,
                "sv_register_table_function('%s') failed: %s\n",
                name ? name : "(null)", e.what());
            return false;
        } catch (...) {
            fprintf(stderr,
                "sv_register_table_function('%s') failed: unknown exception\n",
                name ? name : "(null)");
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// sv_register_scalar_function — Phase 65 Plan 05 (Task 2 Step A)
// ---------------------------------------------------------------------------
// Sibling of `sv_register_table_function`. Registers a scalar function via
// the C++ Catalog API (`Catalog::CreateFunction` on the system catalog with
// a `CreateScalarFunctionInfo`). Consumed by Task 4 / Wave 4 to migrate
// `get_ddl` and `read_yaml_from_semantic_view` off
// `register_scalar_function_with_state`.
//
// Header declaration in cpp/src/shim.hpp.
extern "C" {
    bool sv_register_scalar_function(
        duckdb_database db_handle,
        const char *name,
        const LogicalType *arg_types,
        size_t arg_count,
        LogicalType return_type,
        scalar_function_t exec_cb) {
        try {
            if (db_handle == nullptr || name == nullptr || exec_cb == nullptr) {
                fprintf(stderr,
                    "sv_register_scalar_function: null required argument\n");
                return false;
            }
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            if (wrapper == nullptr) {
                fprintf(stderr,
                    "sv_register_scalar_function: null DatabaseWrapper\n");
                return false;
            }
            auto &db = *wrapper->database->instance;

            vector<LogicalType> args;
            args.reserve(arg_count);
            for (size_t i = 0; i < arg_count; ++i) {
                args.push_back(arg_types[i]);
            }

            ScalarFunction fn(std::string(name), std::move(args),
                              return_type, exec_cb);
            CreateScalarFunctionInfo info(std::move(fn));
            info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;

            auto &system_catalog = Catalog::GetSystemCatalog(db);
            auto txn = CatalogTransaction::GetSystemTransaction(db);
            system_catalog.CreateFunction(txn, info);
            return true;
        } catch (const std::exception &e) {
            fprintf(stderr,
                "sv_register_scalar_function('%s') failed: %s\n",
                name ? name : "(null)", e.what());
            return false;
        } catch (...) {
            fprintf(stderr,
                "sv_register_scalar_function('%s') failed: unknown exception\n",
                name ? name : "(null)");
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// __sv_compute_create_from_yaml — Phase 65 Plan 04 (Task 2 Step B)
// ---------------------------------------------------------------------------
// Helper table function registered via `sv_register_table_function`. The bind
// callback opens `Connection probe(*context.db)`, reads the YAML file via a
// parameterized `read_text(?)` query (path comes through as a typed `Value`,
// so there is no SQL-injection surface), then calls the Rust FFI helper
// `sv_compute_create_from_yaml_rust` to parse + enrich + serialize the YAML
// into a metadata-less JSON definition. The exec callback emits the JSON
// as a single VARCHAR row.
//
// The outer parser_override INSERT (src/parse.rs::rewrite_yaml_file_create,
// landed in Task 4) wraps the helper TF's row via `json_merge_patch(new_def,
// json_object('created_on', strftime(now(), '%Y-%m-%dT%H:%M:%SZ'),
// 'database_name', current_database(), 'schema_name', current_schema()))`
// so metadata fields are populated on the caller's connection, preserving
// the D-21 transactional contract.

struct CreateFromYamlBindData : public TableFunctionData {
    std::string file_path;
    std::string view_name;
    std::string comment;
    int32_t kind = 0;
    std::string new_def;
};

struct CreateFromYamlLocalState : public LocalTableFunctionState {
    bool emitted = false;
};

static unique_ptr<FunctionData> sv_create_from_yaml_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<CreateFromYamlBindData>();
    bd->file_path = input.inputs[0].GetValue<string>();
    bd->view_name = input.inputs[1].GetValue<string>();
    bd->kind      = input.inputs[2].GetValue<int32_t>();
    bd->comment   = input.inputs[3].GetValue<string>();

    // Read the YAML file via a per-call Connection. `read_text(?)` honors
    // DuckDB's `enable_external_access` setting and other path-resolution
    // safeguards — we don't introduce a new file-I/O surface.
    //
    // SQL safety: the path comes through as a typed `Value` (input.inputs[0]
    // is a parameterized VARCHAR bound by the outer parser_override SELECT,
    // not concatenated from user input here). We escape any embedded single
    // quotes when embedding the path into the read_text(...) SQL string so
    // a pathological path string ("a'b.yaml") doesn't break the SELECT — the
    // standard SQL doubling convention. We use Connection::Query here
    // (returning MaterializedQueryResult directly) rather than the Prepare/
    // Execute path because the latter returns a non-materialized QueryResult
    // that triggers an InternalException when down-cast.
    std::string path_escaped = bd->file_path;
    {
        size_t pos = 0;
        while ((pos = path_escaped.find('\'', pos)) != std::string::npos) {
            path_escaped.replace(pos, 1, "''");
            pos += 2;
        }
    }
    Connection probe(*context.db);
    std::string read_sql =
        "SELECT content FROM read_text('" + path_escaped + "')";
    auto result = probe.Query(read_sql);
    if (result->HasError()) {
        // The "FROM YAML FILE failed" prefix matches the v0.9.0 wording
        // pinned by test/sql/phase53_yaml_file.test ("file not found"
        // and "enable_external_access=false" cases).
        throw BinderException(
            "FROM YAML FILE failed: " + result->GetError());
    }
    if (result->RowCount() == 0) {
        throw BinderException(
            "FROM YAML FILE failed: no content returned from '" +
            bd->file_path + "'");
    }
    Value content_val = result->GetValue(0, 0);
    if (content_val.IsNull()) {
        throw BinderException(
            "FROM YAML FILE failed: NULL content from '" +
            bd->file_path + "'");
    }
    std::string yaml_content = content_val.GetValue<string>();

    // Bridge into Rust to parse + enrich + serialize. The Rust side enforces
    // YAML_SIZE_CAP (1 MiB) inside from_yaml_with_size_cap, so we deliberately
    // do NOT pre-check size here — keeps the cap as a single source of truth
    // on the Rust side.
    char *out_ptr = nullptr;
    size_t out_len = 0;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_compute_create_from_yaml_rust(
        reinterpret_cast<const uint8_t *>(yaml_content.data()),
        yaml_content.size(),
        reinterpret_cast<const uint8_t *>(bd->view_name.data()),
        bd->view_name.size(),
        reinterpret_cast<const uint8_t *>(bd->comment.data()),
        bd->comment.size(),
        static_cast<uint8_t>(bd->kind),
        &out_ptr,
        &out_len,
        error_buf,
        sizeof(error_buf));

    if (rc != 0 || out_ptr == nullptr) {
        // Match "FROM YAML FILE failed:" wording for size/parse errors so
        // the legacy phase53 sqllogictest assertions stay green.
        throw BinderException(
            std::string("FROM YAML FILE failed: ") + error_buf);
    }

    // Move the heap-owned UTF-8 buffer into bd->new_def, then release the
    // Rust allocation via sv_free_buffer (the exact (ptr, len) pair Rust
    // returned). The string copy is unavoidable because std::string demands
    // its own allocator-owned storage; for a 1 MiB cap this is bounded.
    bd->new_def.assign(out_ptr, out_len);
    sv_free_buffer(out_ptr, out_len);

    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("new_def");
    return std::move(bd);
}

static unique_ptr<LocalTableFunctionState> sv_create_from_yaml_init_local(
    ExecutionContext & /*context*/,
    TableFunctionInitInput & /*input*/,
    GlobalTableFunctionState * /*global_state*/) {
    return make_uniq<CreateFromYamlLocalState>();
}

static void sv_create_from_yaml_function(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<CreateFromYamlBindData>();
    // Without an init_local callback registered, data_p.local_state is
    // nullptr. Use an instance bit on the bind data instead — but bind
    // data is shared across executions, so for parallel safety we register
    // init_local. (Currently sv_register_parser_hooks passes nullptr for
    // init_cb on this helper; flip the registration to use the init_local
    // path so multiple executions in the same statement don't double-emit.)
    auto *state_p = data_p.local_state.get();
    if (state_p == nullptr) {
        // No local state — fall back to single-emit using a one-shot
        // pattern keyed on output cardinality. The helper TF is bound and
        // executed once per outer INSERT so this is safe in practice for
        // the v0.10.0 surface; if a future caller invokes it inside a
        // streaming context, init_local should be wired in
        // sv_register_parser_hooks.
        output.SetValue(0, 0, Value(bd.new_def));
        output.SetCardinality(1);
        return;
    }
    auto &state = state_p->Cast<CreateFromYamlLocalState>();
    if (state.emitted) {
        output.SetCardinality(0);
        return;
    }
    output.SetValue(0, 0, Value(bd.new_def));
    output.SetCardinality(1);
    state.emitted = true;
}

// ---------------------------------------------------------------------------
// list_semantic_views — Phase 65 Plan 05 (Task 1 / Wave 0 bridge spike)
// ---------------------------------------------------------------------------
// First read-side TF migrated from duckdb-rs `register_table_function_with_
// extra_info` (which marshals `ClientContext &` away — Plan 01 Spike A6) to
// the C++ Catalog API path via `sv_register_table_function` (Plan 04). The
// bind callback opens a per-call `Connection probe(*context.db)` and bridges
// to Rust by casting the stack `Connection *` to `duckdb_connection`. See
// the FFI declaration block above + `65-05-SPIKE-SUMMARY.md` for the bridge
// mechanism design and the LOC extrapolation for the remaining 16 read-side
// migrations.
//
// Wire format from Rust dispatcher: a flat length-prefixed binary buffer:
//   u32 row_count (little-endian)
//   for each row:
//     for each of 6 columns:
//       u32 byte_len (little-endian)
//       byte_len bytes (UTF-8, NOT NUL-terminated)
// Buffer is heap-owned by Rust; C++ parses into BindData and immediately
// releases via `sv_free_buffer`.

struct ListSemanticViewsRow {
    std::string created_on;
    std::string name;
    std::string kind;
    std::string database_name;
    std::string schema_name;
    std::string comment;
};

struct ListSemanticViewsBindData : public TableFunctionData {
    std::vector<ListSemanticViewsRow> rows;
};

struct ListSemanticViewsLocalState : public LocalTableFunctionState {
    bool emitted = false;
};

// Helper: read a little-endian u32 from buf[offset..offset+4] and advance.
// Throws BinderException on out-of-bounds — defensive against an FFI buffer
// truncated by a panic or allocation failure on the Rust side.
static uint32_t sv_read_u32_le(const char *buf, size_t buf_len, size_t &offset) {
    if (offset + 4 > buf_len) {
        throw BinderException(
            "list_semantic_views: FFI buffer truncated (expected u32 at offset " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
    auto p = reinterpret_cast<const unsigned char *>(buf + offset);
    uint32_t v = static_cast<uint32_t>(p[0])
               | (static_cast<uint32_t>(p[1]) << 8)
               | (static_cast<uint32_t>(p[2]) << 16)
               | (static_cast<uint32_t>(p[3]) << 24);
    offset += 4;
    return v;
}

static std::string sv_read_string(const char *buf, size_t buf_len, size_t &offset) {
    uint32_t len = sv_read_u32_le(buf, buf_len, offset);
    if (offset + len > buf_len) {
        throw BinderException(
            "list_semantic_views: FFI buffer truncated (expected " +
            std::to_string(len) + " string bytes at offset " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
    std::string s(buf + offset, len);
    offset += len;
    return s;
}

static unique_ptr<FunctionData> sv_list_semantic_views_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<ListSemanticViewsBindData>();

    // Declare the 6-column schema — must match the v0.9.0 Rust VTab exactly
    // (test/sql/phase42_persistence.test + any other suite that does
    // SELECT * FROM list_semantic_views() relies on byte-identical column
    // names and order).
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("created_on");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("name");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("kind");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("database_name");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("schema_name");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("comment");

    // Open a per-call Connection on the caller's DatabaseInstance. The
    // ctor calls `ConnectionManager::AddConnection` (acquires
    // `connections_lock`) and the matching dtor (RemoveConnection)
    // releases it at end-of-scope. Both confirmed deadlock-free from the
    // bind thread by `65-READ-PATH-SPIKE.md` (READ-BIND-RC0).
    Connection probe(*context.db);

    // Bridge: cast the stack Connection* to duckdb_connection. The C API
    // handle is literally `reinterpret_cast<duckdb_connection>(Connection*)`
    // — confirmed by reading the amalgamation:
    //
    //   // duckdb.cpp:266440-266446
    //   connection = new Connection(*wrapper->database);
    //   *out = reinterpret_cast<duckdb_connection>(connection);
    //
    // This is a BORROW: ownership stays with the C++ `probe` local. The
    // Rust dispatcher MUST NOT call `duckdb_disconnect` on the handle (it
    // would `delete` a stack-allocated Connection — UB). The bind scope's
    // destructor handles teardown correctly.
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_list_semantic_views_bind_rust(
        borrowed,
        &payload.ptr, &payload.len,
        error_buf, sizeof(error_buf));

    if (rc != 0) {
        // Surface as BinderException so the user sees a proper SQL-layer
        // error rather than a process crash. The Rust dispatcher's
        // `catch_unwind` already converts panics into rc=2 + an error
        // message; this branch just re-raises across the C++ boundary.
        throw BinderException(
            std::string("list_semantic_views failed: ") + error_buf);
    }

    if (payload.ptr == nullptr) {
        // Defensive: rc=0 with null buffer means zero rows — bd->rows
        // stays empty. Skip parsing.
        return std::move(bd);
    }

    // Parse the flat binary buffer into ListSemanticViewsRow entries.
    size_t offset = 0;
    uint32_t row_count = sv_read_u32_le(payload.ptr, payload.len, offset);
    bd->rows.reserve(row_count);
    for (uint32_t r = 0; r < row_count; ++r) {
        ListSemanticViewsRow row;
        row.created_on    = sv_read_string(payload.ptr, payload.len, offset);
        row.name          = sv_read_string(payload.ptr, payload.len, offset);
        row.kind          = sv_read_string(payload.ptr, payload.len, offset);
        row.database_name = sv_read_string(payload.ptr, payload.len, offset);
        row.schema_name   = sv_read_string(payload.ptr, payload.len, offset);
        row.comment       = sv_read_string(payload.ptr, payload.len, offset);
        bd->rows.push_back(std::move(row));
    }

    // SvOwnedBuffer destructor releases the Rust-owned buffer via
    // sv_free_buffer with the exact (ptr, len) pair.
    return std::move(bd);
}

static unique_ptr<LocalTableFunctionState> sv_list_semantic_views_init_local(
    ExecutionContext & /*context*/,
    TableFunctionInitInput & /*input*/,
    GlobalTableFunctionState * /*global_state*/) {
    return make_uniq<ListSemanticViewsLocalState>();
}

static void sv_list_semantic_views_function(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<ListSemanticViewsBindData>();
    auto *state_p = data_p.local_state.get();
    if (state_p == nullptr) {
        // Defensive: same pattern as sv_create_from_yaml_function.
        idx_t n = bd.rows.size();
        for (idx_t i = 0; i < n; ++i) {
            output.SetValue(0, i, Value(bd.rows[i].created_on));
            output.SetValue(1, i, Value(bd.rows[i].name));
            output.SetValue(2, i, Value(bd.rows[i].kind));
            output.SetValue(3, i, Value(bd.rows[i].database_name));
            output.SetValue(4, i, Value(bd.rows[i].schema_name));
            output.SetValue(5, i, Value(bd.rows[i].comment));
        }
        output.SetCardinality(n);
        return;
    }
    auto &state = state_p->Cast<ListSemanticViewsLocalState>();
    if (state.emitted) {
        output.SetCardinality(0);
        return;
    }
    idx_t n = bd.rows.size();
    for (idx_t i = 0; i < n; ++i) {
        output.SetValue(0, i, Value(bd.rows[i].created_on));
        output.SetValue(1, i, Value(bd.rows[i].name));
        output.SetValue(2, i, Value(bd.rows[i].kind));
        output.SetValue(3, i, Value(bd.rows[i].database_name));
        output.SetValue(4, i, Value(bd.rows[i].schema_name));
        output.SetValue(5, i, Value(bd.rows[i].comment));
    }
    output.SetCardinality(n);
    state.emitted = true;
}

extern "C" {
    bool sv_register_list_semantic_views(duckdb_database db_handle) {
        // Zero-argument table function — no arg_types array.
        return sv_register_table_function(
            db_handle,
            "list_semantic_views",
            /*arg_types*/ nullptr, /*arg_count*/ 0,
            sv_list_semantic_views_bind,
            sv_list_semantic_views_function,
            sv_list_semantic_views_init_local);
    }
}

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 (Tasks 2-5) — Generic VARCHAR-rows + VARCHAR/BOOL-rows
// helpers for the remaining read-side TF migrations.
// ---------------------------------------------------------------------------
//
// Most of the Phase 65 Plan 05 migrations emit homogeneous VARCHAR result
// columns (the various SHOW SEMANTIC ... / DESCRIBE SEMANTIC VIEW shapes).
// To keep the per-TF migration delta to a thin adapter, we factor the
// payload-parse + bind-data shape + exec emitter into generic helpers here.
//
// Each migrated TF still owns its own bind callback (to declare the right
// column names and call the matching `sv_<name>_bind_rust` dispatcher), but
// the bulk of the bind + the entire exec/init are reused.

struct SvVarcharBindData : public TableFunctionData {
    std::vector<std::vector<std::string>> rows;
    size_t expected_cols = 0;
};

struct SvVarcharLocalState : public LocalTableFunctionState {
    bool emitted = false;
};

// Parse a length-prefixed VARCHAR wire-format payload into bd.rows. The
// expected column count is taken from bd.expected_cols (must be set by the
// caller before invocation). Throws BinderException on truncated payload.
static void sv_parse_varchar_payload(const char *buf, size_t buf_len,
                                     SvVarcharBindData &bd,
                                     const char *fn_name) {
    if (buf == nullptr) {
        return;  // rc=0 with null buffer == zero rows
    }
    size_t offset = 0;
    uint32_t row_count = sv_read_u32_le(buf, buf_len, offset);
    bd.rows.reserve(row_count);
    for (uint32_t r = 0; r < row_count; ++r) {
        std::vector<std::string> row;
        row.reserve(bd.expected_cols);
        for (size_t c = 0; c < bd.expected_cols; ++c) {
            row.push_back(sv_read_string(buf, buf_len, offset));
        }
        bd.rows.push_back(std::move(row));
    }
    if (offset != buf_len) {
        throw BinderException(
            std::string(fn_name) +
            ": FFI buffer has trailing bytes (consumed " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
}

static unique_ptr<LocalTableFunctionState> sv_varchar_init_local(
    ExecutionContext & /*context*/,
    TableFunctionInitInput & /*input*/,
    GlobalTableFunctionState * /*global_state*/) {
    return make_uniq<SvVarcharLocalState>();
}

static void sv_emit_varchar_rows(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<SvVarcharBindData>();
    auto *state_p = data_p.local_state.get();
    bool emitted = false;
    if (state_p) {
        auto &state = state_p->Cast<SvVarcharLocalState>();
        if (state.emitted) {
            output.SetCardinality(0);
            return;
        }
        state.emitted = true;
        emitted = true;
    }
    (void)emitted;  // single-shot semantics — even without local state we emit once per bind
    idx_t n = bd.rows.size();
    for (idx_t i = 0; i < n; ++i) {
        const auto &row = bd.rows[i];
        for (size_t c = 0; c < row.size(); ++c) {
            output.SetValue(c, i, Value(row[c]));
        }
    }
    output.SetCardinality(n);
}

// VARCHAR-rows-with-trailing-BOOL shape (used by
// show_semantic_dimensions_for_metric, which returns 3 VARCHAR + 1 BOOLEAN).
struct SvVarcharBoolBindData : public TableFunctionData {
    std::vector<std::pair<std::vector<std::string>, bool>> rows;
    size_t expected_varchar_cols = 0;  // number of VARCHAR cells per row
};

static void sv_parse_varchar_bool_payload(const char *buf, size_t buf_len,
                                          SvVarcharBoolBindData &bd,
                                          const char *fn_name) {
    if (buf == nullptr) {
        return;
    }
    size_t offset = 0;
    uint32_t row_count = sv_read_u32_le(buf, buf_len, offset);
    bd.rows.reserve(row_count);
    for (uint32_t r = 0; r < row_count; ++r) {
        std::vector<std::string> strs;
        strs.reserve(bd.expected_varchar_cols);
        for (size_t c = 0; c < bd.expected_varchar_cols; ++c) {
            strs.push_back(sv_read_string(buf, buf_len, offset));
        }
        if (offset + 1 > buf_len) {
            throw BinderException(
                std::string(fn_name) +
                ": FFI buffer truncated (expected u8 bool at offset " +
                std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
        }
        bool b = buf[offset] != 0;
        offset += 1;
        bd.rows.emplace_back(std::move(strs), b);
    }
    if (offset != buf_len) {
        throw BinderException(
            std::string(fn_name) +
            ": FFI buffer has trailing bytes (consumed " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
}

static void sv_emit_varchar_bool_rows(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<SvVarcharBoolBindData>();
    auto *state_p = data_p.local_state.get();
    if (state_p) {
        auto &state = state_p->Cast<SvVarcharLocalState>();
        if (state.emitted) {
            output.SetCardinality(0);
            return;
        }
        state.emitted = true;
    }
    idx_t n = bd.rows.size();
    for (idx_t i = 0; i < n; ++i) {
        const auto &strs = bd.rows[i].first;
        for (size_t c = 0; c < strs.size(); ++c) {
            output.SetValue(c, i, Value(strs[c]));
        }
        // BOOLEAN trailing column at index strs.size().
        output.SetValue(strs.size(), i, Value::BOOLEAN(bd.rows[i].second));
    }
    output.SetCardinality(n);
}

// Common idiom for the bind callback: open Connection, run Rust dispatcher
// for the zero-arg case, parse payload into bd. Used by all 5 zero-arg
// migrated TFs (Task 2 / Wave 1) and reused by Task 3 / Wave 2 with extra
// VARCHAR args supplied to the dispatcher.
//
// `dispatcher` returns rc==0 on success (payload populated), rc!=0 with
// error_buf NUL-terminated otherwise. Throws BinderException on rc!=0 with
// the "<fn_name> failed: <error_buf>" message — matches the Wave 0 spike.
template <typename DispatcherFn>
static void sv_run_varchar_bind(ClientContext &context,
                                SvVarcharBindData &bd,
                                size_t expected_cols,
                                const char *fn_name,
                                DispatcherFn &&dispatcher) {
    bd.expected_cols = expected_cols;
    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = dispatcher(borrowed,
                            &payload.ptr, &payload.len,
                            error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException(std::string(fn_name) + " failed: " + error_buf);
    }
    sv_parse_varchar_payload(payload.ptr, payload.len, bd, fn_name);
}

template <typename DispatcherFn>
static void sv_run_varchar_bool_bind(ClientContext &context,
                                     SvVarcharBoolBindData &bd,
                                     size_t expected_varchar_cols,
                                     const char *fn_name,
                                     DispatcherFn &&dispatcher) {
    bd.expected_varchar_cols = expected_varchar_cols;
    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = dispatcher(borrowed,
                            &payload.ptr, &payload.len,
                            error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException(std::string(fn_name) + " failed: " + error_buf);
    }
    sv_parse_varchar_bool_payload(payload.ptr, payload.len, bd, fn_name);
}

// ---------------------------------------------------------------------------
// list_terse_semantic_views — Phase 65 Plan 05 Task 2 (Wave 1)
// ---------------------------------------------------------------------------
// 5-column subset of list_semantic_views — same bridge mechanism, no
// per-function BindData struct (uses generic SvVarcharBindData via the
// sv_run_varchar_bind helper).
//
// Columns: created_on, name, kind, database_name, schema_name.

static unique_ptr<FunctionData> sv_list_terse_semantic_views_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COL_NAMES[] = {
        "created_on", "name", "kind", "database_name", "schema_name",
    };
    for (auto cn : COL_NAMES) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind(
        context, *bd, /*expected_cols*/ 5, "list_terse_semantic_views",
        [](duckdb_connection borrowed, char **out_ptr, size_t *out_len,
           char *error_buf, size_t error_buf_len) {
            return sv_list_terse_semantic_views_bind_rust(
                borrowed, out_ptr, out_len, error_buf, error_buf_len);
        });
    return std::move(bd);
}

extern "C" {
    bool sv_register_list_terse_semantic_views(duckdb_database db_handle) {
        return sv_register_table_function(
            db_handle,
            "list_terse_semantic_views",
            /*arg_types*/ nullptr, /*arg_count*/ 0,
            sv_list_terse_semantic_views_bind,
            sv_emit_varchar_rows,
            sv_varchar_init_local);
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

            // Phase 65 Plan 04 (Task 2 Step B): register the
            // `__sv_compute_create_from_yaml` helper TF via the C++ Catalog
            // API so its bind callback receives a native `ClientContext &`
            // and can open per-call `Connection(*context.db)` to read the
            // YAML file. The outer parser_override INSERT in
            // src/parse.rs::rewrite_yaml_file_create wraps the helper TF's
            // `new_def` row with json_merge_patch + json_object to add
            // now()/current_database()/current_schema() on the caller's
            // connection, preserving D-21 transactional contract.
            {
                LogicalType arg_types[] = {
                    LogicalType::VARCHAR,  // file_path
                    LogicalType::VARCHAR,  // view_name
                    LogicalType::INTEGER,  // kind (0/1/2)
                    LogicalType::VARCHAR,  // comment (empty = none)
                };
                if (!sv_register_table_function(
                        db_handle,
                        "__sv_compute_create_from_yaml",
                        arg_types, 4,
                        sv_create_from_yaml_bind,
                        sv_create_from_yaml_function,
                        sv_create_from_yaml_init_local)) {
                    fprintf(stderr,
                        "sv_register_parser_hooks: failed to register "
                        "__sv_compute_create_from_yaml helper TF\n");
                    return false;
                }
            }

            return true;
        } catch (const std::exception &e) {
            fprintf(stderr, "sv_register_parser_hooks failed: %s\n", e.what());
            return false;
        }
    }
}
