// Parser extension type declarations for DuckDB >= 1.5.0 compatibility.
//
// In DuckDB 1.5.0, the parser extension types (ParserExtensionParseData,
// ParserExtensionParseResult, ParserExtensionPlanResult, ParserExtension,
// ExtensionCallbackManager) were moved from the amalgamation header
// (duckdb.hpp) into the source (duckdb.cpp). Since shim.cpp is compiled
// as a separate translation unit from duckdb.cpp, it no longer sees these
// declarations via `#include "duckdb.hpp"`.
//
// This header re-declares the types needed by shim.cpp. The definitions
// are extracted verbatim from the amalgamation source
// (`duckdb/parser/parser_extension.hpp` section of duckdb.cpp, lines
// 23732-23837, and `duckdb/main/extension_callback_manager.hpp` section).
//
// IMPORTANT: These declarations must EXACTLY match duckdb.cpp to avoid
// ODR (One Definition Rule) violations. When DuckDB is updated, verify
// these declarations still match by:
//   grep -A 120 'duckdb/parser/parser_extension.hpp' cpp/include/duckdb.cpp
//
// As of v0.8.0 the extension only uses parser_override (the legacy
// parse_function/plan_function path was retired in the full unification),
// but the ParserExtension class retains its parse_function and plan_function
// fields under ODR — we keep their typedefs and supporting structs for
// layout compatibility even though shim.cpp never assigns to them.
//
// AllowParserOverride enum, full ParserOptions struct, and minimal Parser
// class declaration are also re-declared so shim.cpp can re-parse
// rewritten SQL produced by the parser_override callback. Definitions
// match duckdb.cpp lines ~23790, ~23800, ~23949.

#pragma once

#include "duckdb.hpp"
#include <cstddef>
#include <type_traits>

namespace duckdb {

// Forward declarations
class ExtensionCallbackManager;
class ParserExtension;

//===--------------------------------------------------------------------===//
// ParserExtensionInfo
//===--------------------------------------------------------------------===//
struct ParserExtensionInfo {
	virtual ~ParserExtensionInfo() {
	}
};

//===--------------------------------------------------------------------===//
// Parse
//===--------------------------------------------------------------------===//
enum class ParserExtensionResultType : uint8_t {
	PARSE_SUCCESSFUL,
	DISPLAY_ORIGINAL_ERROR,
	DISPLAY_EXTENSION_ERROR
};

struct ParserExtensionParseData {
	virtual ~ParserExtensionParseData() {
	}

	virtual unique_ptr<ParserExtensionParseData> Copy() const = 0;
	virtual string ToString() const = 0;
};

struct ParserExtensionParseResult {
	ParserExtensionParseResult() : type(ParserExtensionResultType::DISPLAY_ORIGINAL_ERROR) {
	}
	explicit ParserExtensionParseResult(string error_p)
	    : type(ParserExtensionResultType::DISPLAY_EXTENSION_ERROR), error(std::move(error_p)) {
	}
	explicit ParserExtensionParseResult(unique_ptr<ParserExtensionParseData> parse_data_p)
	    : type(ParserExtensionResultType::PARSE_SUCCESSFUL), parse_data(std::move(parse_data_p)) {
	}

	//! Whether or not parsing was successful
	ParserExtensionResultType type;
	//! The parse data (if successful)
	unique_ptr<ParserExtensionParseData> parse_data;
	//! The error message (if unsuccessful)
	string error;
	//! The error location (if unsuccessful)
	optional_idx error_location;
};

// Phase 62 layout guard — sv_parse_stub writes result.error_location after
// construction (the constructors don't take it as a parameter). If a future
// DuckDB bump moves error_location to private or changes its type from
// optional_idx, this assertion fires before silent breakage. See
// `.planning/phases/62-caret-restoration-lru-removal/62-RESEARCH.md` Risk F.
//
// The size upper bound is intentionally loose to tolerate ABI padding
// differences across compilers — its purpose is to catch a *jump* (e.g. a
// new field added) not to lock the exact byte count. The type-check on
// error_location is the strict guard.
static_assert(sizeof(ParserExtensionParseResult) <= 64,
              "ParserExtensionParseResult layout drift -- re-grep duckdb.cpp parser_extension.hpp");
static_assert(std::is_same<decltype(ParserExtensionParseResult{}.error_location), optional_idx>::value,
              "ParserExtensionParseResult::error_location type drift");

typedef ParserExtensionParseResult (*parse_function_t)(ParserExtensionInfo *info, const string &query);

//===--------------------------------------------------------------------===//
// Plan
//===--------------------------------------------------------------------===//
struct ParserExtensionPlanResult { // NOLINT: work-around bug in clang-tidy
	//! The table function to execute
	TableFunction function;
	//! Parameters to the function
	vector<Value> parameters;
	//! The set of databases that will be modified by this statement (empty for a read-only statement)
	unordered_map<string, StatementProperties::ModificationInfo> modified_databases;
	//! Whether or not the statement requires a valid transaction to be executed
	bool requires_valid_transaction = true;
	//! What type of result set the statement returns
	StatementReturnType return_type = StatementReturnType::NOTHING;
};

typedef ParserExtensionPlanResult (*plan_function_t)(ParserExtensionInfo *info, ClientContext &context,
                                                     unique_ptr<ParserExtensionParseData> parse_data);

//===--------------------------------------------------------------------===//
// Parser override
//===--------------------------------------------------------------------===//
struct ParserOverrideResult {
	explicit ParserOverrideResult() : type(ParserExtensionResultType::DISPLAY_ORIGINAL_ERROR) {};

	explicit ParserOverrideResult(vector<unique_ptr<SQLStatement>> statements_p)
	    : type(ParserExtensionResultType::PARSE_SUCCESSFUL), statements(std::move(statements_p)) {};

	explicit ParserOverrideResult(std::exception &error_p)
	    : type(ParserExtensionResultType::DISPLAY_EXTENSION_ERROR), error(error_p) {};

	ParserExtensionResultType type;
	vector<unique_ptr<SQLStatement>> statements;
	ErrorData error;
};

//===--------------------------------------------------------------------===//
// AllowParserOverride
//===--------------------------------------------------------------------===//
enum class AllowParserOverride : uint8_t { DEFAULT_OVERRIDE, FALLBACK_OVERRIDE, STRICT_OVERRIDE };

//===--------------------------------------------------------------------===//
// ParserOptions
//===--------------------------------------------------------------------===//
struct ParserOptions {
	bool preserve_identifier_case = true;
	bool integer_division = false;
	idx_t max_expression_depth = 1000;
	optional_ptr<const ExtensionCallbackManager> extensions;
	AllowParserOverride parser_override_setting = AllowParserOverride::DEFAULT_OVERRIDE;
};

// Guard against silent layout drift between this redeclaration and duckdb.cpp.
// On DuckDB v1.5.2 (duckdb-rs crate `=1.10502.0`, pinned in Cargo.toml) the layout
// is { bool, bool, idx_t, optional_ptr<...>, AllowParserOverride } and packs
// to 32 bytes on a 64-bit target (alignof 8 forces 6B pad after the bools,
// 7B trailing pad after the enum). If a DuckDB bump changes the field set
// this assert fires; re-grep parser.hpp / parser_options.hpp in duckdb.cpp
// and adjust both the struct and this constant in lockstep. Truncating
// ParserOptions previously caused garbage parse errors like
// `syntax error at or near "" position 0` (see milestone v0.8.0 commit 55ddcda).
static_assert(sizeof(ParserOptions) == 32,
              "ParserOptions layout drift -- re-grep duckdb.cpp parser_options.hpp");

typedef ParserOverrideResult (*parser_override_function_t)(ParserExtensionInfo *info, const string &query,
                                                           ParserOptions &options);

//===--------------------------------------------------------------------===//
// ParserExtension
//===--------------------------------------------------------------------===//
class ParserExtension {
public:
	//! The parse function of the parser extension.
	//! Takes a query string as input and returns ParserExtensionParseData (on success) or an error
	parse_function_t parse_function = nullptr;

	//! The plan function of the parser extension
	//! Takes as input the result of the parse_function, and outputs various properties of the resulting plan
	plan_function_t plan_function = nullptr;

	//! Override the current parser with a new parser and return a vector of SQL statements
	parser_override_function_t parser_override = nullptr;

	//! Additional parser info passed to the parse function
	shared_ptr<ParserExtensionInfo> parser_info;

	static void Register(DBConfig &config, ParserExtension extension);
};

//===--------------------------------------------------------------------===//
// ExtensionCallbackManager
//===--------------------------------------------------------------------===//
class ExtensionCallbackManager {
public:
	static ExtensionCallbackManager &Get(DatabaseInstance &db);
	void Register(ParserExtension extension);
};

//===--------------------------------------------------------------------===//
// Parser (minimal — only the surface shim.cpp uses to re-parse rewritten SQL)
//===--------------------------------------------------------------------===//
//
// Layout MUST mirror duckdb::Parser in the linked amalgamation exactly,
// including the trailing private `options` field. Truncating it makes
// `Parser parser;` allocate too little storage and the constructor / ParseQuery
// will write past the object — observed as garbage parse errors like
// `syntax error at or near "" position 0`. Keep this in sync with
// duckdb.cpp's class Parser declaration when bumping DuckDB.
class Parser {
public:
	explicit Parser(ParserOptions options = ParserOptions());

	vector<unique_ptr<SQLStatement>> statements;

public:
	void ParseQuery(const string &query);

private:
	ParserOptions options;
};

} // namespace duckdb
