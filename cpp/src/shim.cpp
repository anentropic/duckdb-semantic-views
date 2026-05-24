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
