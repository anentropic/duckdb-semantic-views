// C++ helper for the DuckDB semantic_views extension (Option A).
//
// The Rust entry point (semantic_views_init_c_api, C_STRUCT ABI) owns the DuckDB
// handshake and function registration. After init, it calls sv_register_parser_hooks()
// here to register the parser extension hooks. This requires C++ types (ParserExtension,
// DBConfig) that are only accessible via the C++ API.
//
// All DuckDB C++ symbols are provided by compiling duckdb.cpp (the amalgamation
// source) alongside this file. Symbol visibility on the cdylib restricts exports
// to just the Rust entry point, so these definitions stay internal to the binary.

#include "duckdb.hpp"

using namespace duckdb;

// ---------------------------------------------------------------------------
// Parser hook: SemanticViewParseData
// ---------------------------------------------------------------------------
// Carries the original query text from parse_function to plan_function.
struct SemanticViewParseData : public ParserExtensionParseData {
    string query;
    explicit SemanticViewParseData(string q) : query(std::move(q)) {}
    unique_ptr<ParserExtensionParseData> Copy() const override {
        return make_uniq<SemanticViewParseData>(query);
    }
    string ToString() const override { return query; }
};

// ---------------------------------------------------------------------------
// Rust FFI declarations (defined in src/parse.rs)
// ---------------------------------------------------------------------------
extern "C" {
    // Parse detection: 0 = not ours, 1 = CREATE SEMANTIC VIEW detected
    uint8_t sv_parse_rust(const char *query, size_t query_len);

    // DDL execution: rewrites DDL to function call, executes via duckdb_query
    // Returns 0 on success (name written to name_out), 1 on failure (error in error_out)
    uint8_t sv_execute_ddl_rust(
        const char *query_ptr, size_t query_len,
        duckdb_connection exec_conn,
        char *name_out, size_t name_out_len,
        char *error_out, size_t error_out_len);
}

// ---------------------------------------------------------------------------
// File-scope static: DDL connection for executing rewritten statements
// ---------------------------------------------------------------------------
// Set by sv_register_parser_hooks, used by sv_ddl_bind.
// This is a separate connection to avoid deadlocking with the main
// connection's ClientContext lock during bind.
static duckdb_connection sv_ddl_conn = nullptr;

// ---------------------------------------------------------------------------
// Parser hook: sv_parse_stub
// ---------------------------------------------------------------------------
// Fallback parse function: only called when DuckDB's own parser fails on a
// statement. Delegates detection to Rust via FFI (sv_parse_rust) which handles
// case-insensitive "CREATE SEMANTIC VIEW" prefix matching, whitespace, and
// semicolon stripping. All other queries fall through to DuckDB's normal
// parser error (DISPLAY_ORIGINAL_ERROR).
static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *, const string &query) {
    // Delegate detection to Rust
    uint8_t result = sv_parse_rust(
        reinterpret_cast<const char *>(query.c_str()),
        query.size());
    if (result == 1) {
        // Rust detected CREATE SEMANTIC VIEW -- carry query text forward
        return ParserExtensionParseResult(
            make_uniq<SemanticViewParseData>(query));
    }
    // Not our statement -- let DuckDB show its normal error
    return ParserExtensionParseResult();
}

// ---------------------------------------------------------------------------
// DDL plan function: bind, state, execute, plan
// ---------------------------------------------------------------------------

// Bind data: holds the view name returned from DDL execution.
struct SvDdlBindData : public FunctionData {
    string view_name;
    explicit SvDdlBindData(string name) : view_name(std::move(name)) {}
    unique_ptr<FunctionData> Copy() const override {
        return make_uniq<SvDdlBindData>(view_name);
    }
    bool Equals(const FunctionData &other) const override {
        auto &o = other.Cast<SvDdlBindData>();
        return view_name == o.view_name;
    }
};

// Bind callback: extracts query from input, calls Rust FFI to execute DDL,
// declares one VARCHAR output column "view_name".
static unique_ptr<FunctionData> sv_ddl_bind(
    ClientContext &, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {

    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("view_name");

    // The query text is passed as the first (and only) positional parameter
    auto query = StringValue::Get(input.inputs[0]);

    // Execute the DDL via Rust FFI
    char name_buf[256];
    char error_buf[1024];
    memset(name_buf, 0, sizeof(name_buf));
    memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_execute_ddl_rust(
        query.c_str(), query.size(),
        sv_ddl_conn,
        name_buf, sizeof(name_buf),
        error_buf, sizeof(error_buf));

    if (rc != 0) {
        throw BinderException("CREATE SEMANTIC VIEW failed: %s", error_buf);
    }

    return make_uniq<SvDdlBindData>(string(name_buf));
}

// Global state: tracks whether the single result row has been emitted.
struct SvDdlGlobalState : public GlobalTableFunctionState {
    bool done = false;
};

static unique_ptr<GlobalTableFunctionState> sv_ddl_init_global(
    ClientContext &, TableFunctionInitInput &) {
    return make_uniq<SvDdlGlobalState>();
}

// Execute callback: returns one row with the view name, then marks done.
static void sv_ddl_execute(ClientContext &, TableFunctionInput &input,
                           DataChunk &output) {
    auto &state = input.global_state->Cast<SvDdlGlobalState>();
    if (state.done) {
        output.SetCardinality(0);
        return;
    }
    state.done = true;
    auto &bind_data = input.bind_data->Cast<SvDdlBindData>();
    output.SetCardinality(1);
    output.SetValue(0, 0, Value(bind_data.view_name));
}

// Plan function: transforms the intercepted CREATE SEMANTIC VIEW statement
// into a DDL-executing TableFunction. The query text is carried from the
// parse phase via SemanticViewParseData.
static ParserExtensionPlanResult sv_plan_function(
    ParserExtensionInfo *, ClientContext &,
    unique_ptr<ParserExtensionParseData> parse_data) {
    auto &sv_data = dynamic_cast<SemanticViewParseData &>(*parse_data);

    ParserExtensionPlanResult result;
    result.function = TableFunction("sv_ddl_internal",
                                    {LogicalType::VARCHAR},
                                    sv_ddl_execute, sv_ddl_bind,
                                    sv_ddl_init_global);
    // Push the raw query text as the VARCHAR parameter
    result.parameters.push_back(Value(sv_data.query));

    result.requires_valid_transaction = true;
    result.return_type = StatementReturnType::QUERY_RESULT;
    return result;
}

// ---------------------------------------------------------------------------
// sv_register_parser_hooks -- called from Rust after C API init
// ---------------------------------------------------------------------------
// Receives a duckdb_database handle (C API) and a duckdb_connection for DDL
// execution. Extracts DatabaseInstance& and registers the parser extension
// hooks on DBConfig.
extern "C" {
    bool sv_register_parser_hooks(duckdb_database db_handle, duckdb_connection ddl_conn) {
        try {
            // Store the DDL connection for use by sv_ddl_bind
            sv_ddl_conn = ddl_conn;

            // Extract DatabaseInstance from the C API handle.
            // duckdb_database -> internal_ptr -> DatabaseWrapper ->
            //   shared_ptr<DuckDB> -> shared_ptr<DatabaseInstance>
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            auto &db = *wrapper->database->instance;

            // Register parser extension
            ParserExtension ext;
            ext.parse_function = sv_parse_stub;
            ext.plan_function = sv_plan_function;
            auto &config = DBConfig::GetConfig(db);
            config.parser_extensions.push_back(ext);

            return true;
        } catch (const std::exception &e) {
            fprintf(stderr, "sv_register_parser_hooks failed: %s\n", e.what());
            return false;
        }
    }
}
