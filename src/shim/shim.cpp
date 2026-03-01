// src/shim/shim.cpp
// Phase 8 skeleton: proves Rust+C++ build boundary compiles cleanly.
// No logic — registration callbacks added in Phases 10 (pragma_query_t)
// and 11 (parser_extension).
#include "duckdb.hpp"
#include "duckdb/main/config.hpp"
#include "duckdb/parser/parser_extension.hpp"
#include "duckdb/function/pragma_function.hpp"
#include "shim.h"

using namespace duckdb;

extern "C" {

// Phase 8: intentional no-op.
// db_instance_ptr will be cast to DatabaseInstance* in Phase 10:
//   auto& db = *reinterpret_cast<DatabaseInstance*>(db_instance_ptr);
//   auto& config = DBConfig::GetConfig(db);
//   config.parser_extensions.push_back(...);  // Phase 11
void semantic_views_register_shim(void* /* db_instance_ptr */) {
    // Intentional no-op. Proves C++ compilation and extern "C" boundary work.
}

} // extern "C"
