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
    // DDL rewrite: rewrites DDL to function call SQL (does NOT execute)
    // Returns 0 on success (SQL written to sql_out), 1 on failure (error in error_out)
    uint8_t sv_rewrite_ddl_rust(
        const char *query_ptr, size_t query_len,
        char *sql_out, size_t sql_out_len,
        char *error_out, size_t error_out_len);

    // DDL validation with error reporting: 0=success, 1=error, 2=not-ours
    // On error: error message in error_out, position in *position_out.
    // position_out is set to UINT32_MAX when no position is available.
    uint8_t sv_validate_ddl_rust(
        const char *query_ptr, size_t query_len,
        char *sql_out, size_t sql_out_len,
        char *error_out, size_t error_out_len,
        uint32_t *position_out);
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
// statement. Delegates validation to Rust via FFI (sv_validate_ddl_rust) which
// handles case-insensitive prefix matching, clause validation, near-miss
// detection, and error position tracking. Returns one of three outcomes:
//   - PARSE_SUCCESSFUL: DDL detected and validated, carry query forward
//   - DISPLAY_EXTENSION_ERROR: validation error with positioned caret
//   - DISPLAY_ORIGINAL_ERROR: not our statement, let DuckDB show its error
static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo *, const string &query) {
    char sql_buf[4096];
    char error_buf[1024];
    uint32_t position = UINT32_MAX;
    memset(sql_buf, 0, sizeof(sql_buf));
    memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_validate_ddl_rust(
        reinterpret_cast<const char *>(query.c_str()),
        query.size(),
        sql_buf, sizeof(sql_buf),
        error_buf, sizeof(error_buf),
        &position);

    if (rc == 0) {
        // Success: DDL detected and validated -- carry query text forward
        return ParserExtensionParseResult(
            make_uniq<SemanticViewParseData>(query));
    } else if (rc == 1) {
        // Error: validation failed -- return extension error with position
        string err_msg(error_buf);
        ParserExtensionParseResult err_result(err_msg);
        if (position != UINT32_MAX) {
            err_result.error_location = static_cast<idx_t>(position);
        }
        return err_result;
    }
    // rc == 2: not our statement -- let DuckDB show its normal error
    return ParserExtensionParseResult();
}

// ---------------------------------------------------------------------------
// DDL plan function: bind, state, execute, plan
// ---------------------------------------------------------------------------

// Bind data: holds the full result set from executing rewritten DDL SQL.
// Each row is a vector of string values (all columns forwarded as VARCHAR).
struct SvDdlBindData : public FunctionData {
    vector<vector<string>> rows;    // rows[row_idx][col_idx]
    vector<string> col_names;

    SvDdlBindData() = default;

    unique_ptr<FunctionData> Copy() const override {
        auto copy = make_uniq<SvDdlBindData>();
        copy->rows = rows;
        copy->col_names = col_names;
        return copy;
    }
    bool Equals(const FunctionData &other) const override {
        auto &o = other.Cast<SvDdlBindData>();
        return rows == o.rows && col_names == o.col_names;
    }
    // Disable statement caching: the return schema varies per DDL form
    // (CREATE returns 1 column, DESCRIBE returns 6, SHOW returns 2).
    bool SupportStatementCache() const override {
        return false;
    }
};

// Bind callback: extracts query from input, calls Rust FFI to rewrite DDL,
// then executes the rewritten SQL on sv_ddl_conn and captures the full result.
static unique_ptr<FunctionData> sv_ddl_bind(
    ClientContext &, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {

    // The query text is passed as the first (and only) positional parameter
    auto query = StringValue::Get(input.inputs[0]);

    // Step 1: Rewrite DDL to function call SQL via Rust FFI
    char sql_buf[4096];
    char error_buf[1024];
    memset(sql_buf, 0, sizeof(sql_buf));
    memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_rewrite_ddl_rust(
        query.c_str(), query.size(),
        sql_buf, sizeof(sql_buf),
        error_buf, sizeof(error_buf));

    if (rc != 0) {
        throw BinderException("Semantic view DDL failed: %s", error_buf);
    }

    // Step 2: Execute the rewritten SQL on the DDL connection
    duckdb_result result;
    if (duckdb_query(sv_ddl_conn, sql_buf, &result) != DuckDBSuccess) {
        auto err_ptr = duckdb_result_error(&result);
        string err_msg = err_ptr ? string(err_ptr) : "DDL execution failed (unknown error)";
        duckdb_destroy_result(&result);
        throw BinderException("Semantic view DDL failed: %s", err_msg);
    }

    // Step 3: Read result metadata and declare output columns
    auto col_count = duckdb_column_count(&result);
    auto row_count = duckdb_row_count(&result);

    auto bind_data = make_uniq<SvDdlBindData>();

    for (idx_t c = 0; c < col_count; c++) {
        auto col_name = duckdb_column_name(&result, c);
        names.push_back(col_name ? string(col_name) : "col" + to_string(c));
        return_types.push_back(LogicalType::VARCHAR);
        bind_data->col_names.push_back(names.back());
    }

    // Edge case: 0-column result (shouldn't happen but handle gracefully)
    if (col_count == 0) {
        names.push_back("result");
        return_types.push_back(LogicalType::VARCHAR);
        bind_data->col_names.push_back("result");
    }

    // Step 4: Read all result rows using duckdb_value_varchar
    for (idx_t r = 0; r < row_count; r++) {
        vector<string> row;
        for (idx_t c = 0; c < col_count; c++) {
            char *val = duckdb_value_varchar(&result, c, r);
            row.push_back(val ? string(val) : string());
            if (val) {
                duckdb_free(val);
            }
        }
        bind_data->rows.push_back(std::move(row));
    }

    // Step 5: Clean up the result
    duckdb_destroy_result(&result);

    return bind_data;
}

// Global state: tracks the current row offset for emitting result data.
struct SvDdlGlobalState : public GlobalTableFunctionState {
    idx_t offset = 0;
};

static unique_ptr<GlobalTableFunctionState> sv_ddl_init_global(
    ClientContext &, TableFunctionInitInput &) {
    return make_uniq<SvDdlGlobalState>();
}

// Execute callback: emits rows from the stored result data.
// Handles 0, 1, or many rows. Uses offset tracking for chunked emission.
static void sv_ddl_execute(ClientContext &, TableFunctionInput &input,
                           DataChunk &output) {
    auto &state = input.global_state->Cast<SvDdlGlobalState>();
    auto &bind_data = input.bind_data->Cast<SvDdlBindData>();

    auto total_rows = bind_data.rows.size();
    if (state.offset >= total_rows) {
        output.SetCardinality(0);
        return;
    }

    // Emit up to STANDARD_VECTOR_SIZE rows per chunk
    idx_t count = MinValue<idx_t>(STANDARD_VECTOR_SIZE, total_rows - state.offset);
    auto col_count = bind_data.col_names.size();

    for (idx_t r = 0; r < count; r++) {
        auto &row = bind_data.rows[state.offset + r];
        for (idx_t c = 0; c < col_count && c < row.size(); c++) {
            output.SetValue(c, r, Value(row[c]));
        }
    }

    output.SetCardinality(count);
    state.offset += count;
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
