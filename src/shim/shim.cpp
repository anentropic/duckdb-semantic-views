// src/shim/shim.cpp
// Phase 10: pragma_query_t catalog persistence.
// Registers define_semantic_view_internal and drop_semantic_view_internal
// PRAGMAs via ExtensionLoader. Also implements semantic_views_pragma_define
// and semantic_views_pragma_drop for use from Rust scalar invoke (separate
// connection — no deadlock).
//
// Phase 8 skeleton: proved Rust+C++ build boundary (now replaced with real logic).
// Phase 11 will add parser_extension hooks for CREATE SEMANTIC VIEW DDL.

#include "duckdb.hpp"
#include "duckdb/main/config.hpp"
#include "duckdb/parser/parser_extension.hpp"
#include "duckdb/main/extension/extension_loader.hpp"
#include "duckdb/function/pragma_function.hpp"
#include "duckdb/main/capi/capi_internal.hpp"
#include "duckdb/common/string_util.hpp"
#include "shim.h"

using namespace duckdb;

// ---------------------------------------------------------------------------
// PRAGMA callbacks (pragma_query_t) — transaction-aware via returned SQL
// DuckDB executes the returned SQL in the caller's transaction (PERSIST-02).
// ---------------------------------------------------------------------------

// Returns INSERT SQL for DuckDB to execute in the caller's transaction.
// Used by: PRAGMA define_semantic_view_internal('name', 'json')
static string PragmaDefineSemanticView(ClientContext &context,
                                        const FunctionParameters &params) {
    auto name = params.values[0].GetValue<string>();
    auto json = params.values[1].GetValue<string>();
    // Escape single quotes to avoid SQL injection / breakage
    auto safe_name = StringUtil::Replace(name, "'", "''");
    auto safe_json = StringUtil::Replace(json, "'", "''");
    return "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) "
           "VALUES ('" + safe_name + "', '" + safe_json + "')";
}

// Returns DELETE SQL for DuckDB to execute in the caller's transaction.
// Used by: PRAGMA drop_semantic_view_internal('name')
static string PragmaDropSemanticView(ClientContext &context,
                                      const FunctionParameters &params) {
    auto name = params.values[0].GetValue<string>();
    auto safe_name = StringUtil::Replace(name, "'", "''");
    return "DELETE FROM semantic_layer._definitions WHERE name = '" + safe_name + "'";
}

extern "C" {

// ---------------------------------------------------------------------------
// semantic_views_register_shim — called at extension load time (lib.rs)
// Phase 10: registers pragma callbacks.
// Phase 11 will also add parser hooks for CREATE SEMANTIC VIEW DDL.
// ---------------------------------------------------------------------------
void semantic_views_register_shim(void* db_instance_ptr) {
    // Cast chain: void* -> duckdb_database -> DatabaseWrapper -> DatabaseInstance&
    // See capi_internal.hpp for struct definitions:
    //   duckdb_database = struct _duckdb_database { void *internal_ptr; } *
    //   internal_ptr points to DatabaseWrapper { shared_ptr<DuckDB> database; }
    //   DuckDB::instance is shared_ptr<DatabaseInstance>
    auto* db_c = reinterpret_cast<duckdb_database>(db_instance_ptr);
    auto* wrapper = reinterpret_cast<DatabaseWrapper*>(db_c->internal_ptr);
    DatabaseInstance& db_instance = *wrapper->database->instance;

    ExtensionLoader loader(db_instance, "semantic_views");

    // Register PRAGMA define_semantic_view_internal(name VARCHAR, json VARCHAR)
    // pragma_query_t: returned SQL executes in the caller's transaction (PERSIST-02)
    auto define_pragma = PragmaFunction::PragmaCall(
        "define_semantic_view_internal",
        PragmaDefineSemanticView,
        {LogicalType::VARCHAR, LogicalType::VARCHAR}
    );
    loader.RegisterFunction(define_pragma);

    // Register PRAGMA drop_semantic_view_internal(name VARCHAR)
    auto drop_pragma = PragmaFunction::PragmaCall(
        "drop_semantic_view_internal",
        PragmaDropSemanticView,
        {LogicalType::VARCHAR}
    );
    loader.RegisterFunction(drop_pragma);
}

// ---------------------------------------------------------------------------
// semantic_views_pragma_define / _drop — called from Rust scalar invoke
//
// These use a SEPARATE connection (conn) created at init time. Because it is
// a different connection, it has its own transaction and does NOT deadlock with
// the main connection's execution lock. The write is auto-committed.
//
// Limitation: this write is NOT in the user's explicit transaction. The scalar
// define_semantic_view() / drop_semantic_view() is not intended to be wrapped
// in user BEGIN/ROLLBACK. For transactional usage, call
// PRAGMA define_semantic_view_internal(...) directly.
// ---------------------------------------------------------------------------

int32_t semantic_views_pragma_define(
    duckdb_connection conn,
    const char* name,
    const char* json
) {
    if (!conn || !name || !json) return -1;

    string name_str(name);
    string json_str(json);
    // Escape single quotes in values
    auto safe_name = StringUtil::Replace(name_str, "'", "''");
    auto safe_json = StringUtil::Replace(json_str, "'", "''");

    string sql = "INSERT OR REPLACE INTO semantic_layer._definitions (name, definition) "
                 "VALUES ('" + safe_name + "', '" + safe_json + "')";

    duckdb_result result;
    auto state = duckdb_query(conn, sql.c_str(), &result);
    duckdb_destroy_result(&result);
    return (state == DuckDBSuccess) ? 0 : -1;
}

int32_t semantic_views_pragma_drop(
    duckdb_connection conn,
    const char* name
) {
    if (!conn || !name) return -1;

    string name_str(name);
    auto safe_name = StringUtil::Replace(name_str, "'", "''");

    string sql = "DELETE FROM semantic_layer._definitions WHERE name = '" + safe_name + "'";

    duckdb_result result;
    auto state = duckdb_query(conn, sql.c_str(), &result);
    duckdb_destroy_result(&result);
    return (state == DuckDBSuccess) ? 0 : -1;
}

} // extern "C"
