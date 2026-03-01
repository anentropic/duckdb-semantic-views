// src/shim/shim.cpp
// Phase 10: pragma_query_t catalog persistence.
// Phase 11: CREATE SEMANTIC VIEW / DROP SEMANTIC VIEW parser extension hook.
//
// Registers define_semantic_view_internal and drop_semantic_view_internal
// PRAGMAs via ExtensionLoader. Also implements semantic_views_pragma_define
// and semantic_views_pragma_drop for use from the C++ parser hook scan function
// (separate connection — no deadlock).
//
// Phase 11: Adds full parser extension:
//   SemanticViewsParserInfo — carries catalog pointer and persist_conn
//   SemanticViewsDDLData — parse result struct (view name, JSON definition, DDL type)
//   SemanticViewsParseFunction — parse_function_t: tokenizes CREATE/DROP SEMANTIC VIEW
//   SemanticViewsPlanFunction — plan_function_t: wraps DDL in a TableFunction plan
//   SemanticViewsDDLBind — bind function: extracts parameters from plan result
//   SemanticViewsDDLScan — scan function: executes the DDL via persist_conn + catalog FFI
//   ParseSemanticViewStatement — hand-written tokenizer for the Snowflake-compatible grammar
//
// Critical constraints:
//   - parse_function_t must return DISPLAY_ORIGINAL_ERROR for all non-matching input (DDL-06)
//   - plan_function_t is called with context_lock held — NEVER call context.Query() (deadlock)
//   - scan function also holds context_lock — use persist_conn for all SQL writes

#include "duckdb.hpp"
#include "duckdb/main/config.hpp"
#include "duckdb/parser/parser_extension.hpp"
#include "duckdb/main/extension/extension_loader.hpp"
#include "duckdb/function/pragma_function.hpp"
#include "duckdb/main/capi/capi_internal.hpp"
#include "duckdb/common/string_util.hpp"
#include "shim.h"

#include <algorithm>
#include <stdexcept>
#include <string>
#include <vector>
#include <set>
#include <sstream>

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

// ---------------------------------------------------------------------------
// Phase 11: CREATE SEMANTIC VIEW / DROP SEMANTIC VIEW parser hook.
// ---------------------------------------------------------------------------

// DDL operation type
enum class SemanticViewsDDLType : int32_t { CREATE = 0, DROP = 1 };

// Parse data — carries fully parsed DDL statement from parse_function_t to plan_function_t
struct SemanticViewsDDLData : ParserExtensionParseData {
    SemanticViewsDDLType ddl_type;
    bool or_replace = false;
    bool if_not_exists = false;
    bool if_exists = false;
    std::string view_name;
    std::string definition_json; // serialized JSON for CREATE; empty for DROP

    unique_ptr<ParserExtensionParseData> Copy() const override {
        return make_unique<SemanticViewsDDLData>(*this);
    }
    std::string ToString() const override {
        return "SemanticViewsDDL:" + view_name;
    }
};

// Parser extension info — carries catalog pointer and persist_conn for scan access
struct SemanticViewsParserInfo : ParserExtensionInfo {
    const void* catalog_ptr;          // Rust CatalogState raw pointer (opaque)
    duckdb_connection persist_conn;   // pre-created separate connection for SQL writes
    SemanticViewsParserInfo(const void* cat, duckdb_connection pc)
        : catalog_ptr(cat), persist_conn(pc) {}
};

// Bind data for the TableFunction scan
struct SemanticViewsDDLBindData : FunctionData {
    SemanticViewsDDLType ddl_type;
    bool or_replace;
    bool if_not_exists;
    bool if_exists;
    std::string view_name;
    std::string definition_json;
    const void* catalog_ptr;
    duckdb_connection persist_conn;

    unique_ptr<FunctionData> Copy() const override {
        return make_unique<SemanticViewsDDLBindData>(*this);
    }
    bool Equals(const FunctionData &other) const override { return true; }
};

// ---------------------------------------------------------------------------
// Hand-written DDL tokenizer — ParseSemanticViewStatement
// ---------------------------------------------------------------------------
//
// Grammar:
//   CREATE [OR REPLACE] SEMANTIC VIEW [IF NOT EXISTS] <name>
//     TABLES ( <alias> AS <table> [PRIMARY KEY (<cols>)] [, ...] )
//     [RELATIONSHIPS ( <from_alias>(<fk_cols>) REFERENCES <ref_alias> [, ...] )]
//     [FACTS ( <alias>.<name> AS <sql_expr> [, ...] )]
//     [DIMENSIONS ( <alias>.<name> AS <sql_expr> [, ...] )]
//     [METRICS ( <alias>.<name> AS <sql_expr> [, ...] )]
//
//   DROP SEMANTIC VIEW [IF EXISTS] <name>

// Tokenizer helpers
static void skip_whitespace(const std::string &s, size_t &pos) {
    while (pos < s.size() && (s[pos] == ' ' || s[pos] == '\t' ||
           s[pos] == '\n' || s[pos] == '\r')) {
        ++pos;
    }
}

// Read the next whitespace-delimited word (uppercased for keyword matching)
static std::string read_word(const std::string &s, size_t &pos) {
    skip_whitespace(s, pos);
    size_t start = pos;
    while (pos < s.size() && s[pos] != ' ' && s[pos] != '\t' &&
           s[pos] != '\n' && s[pos] != '\r' && s[pos] != '(' &&
           s[pos] != ')' && s[pos] != ',' && s[pos] != ';') {
        ++pos;
    }
    std::string word = s.substr(start, pos - start);
    for (auto &c : word) c = (char)toupper((unsigned char)c);
    return word;
}

// Peek at the next non-whitespace character
static char peek_char(const std::string &s, size_t pos) {
    while (pos < s.size() && (s[pos] == ' ' || s[pos] == '\t' ||
           s[pos] == '\n' || s[pos] == '\r')) {
        ++pos;
    }
    return (pos < s.size()) ? s[pos] : '\0';
}

// Read an identifier (may include dots for schema-qualified names)
static std::string read_identifier(const std::string &s, size_t &pos) {
    skip_whitespace(s, pos);
    size_t start = pos;
    // Allow alphanumeric, underscore, dot (for schema-qualified), backtick, quote
    while (pos < s.size()) {
        char c = s[pos];
        if (isalnum((unsigned char)c) || c == '_' || c == '.' || c == '"' || c == '`') {
            ++pos;
        } else {
            break;
        }
    }
    return s.substr(start, pos - start);
}

// Read a SQL expression stopping at comma-at-depth-0 or close-paren-at-depth-0.
// Returns the expression with surrounding whitespace trimmed.
static std::string read_expression(const std::string &s, size_t &pos) {
    skip_whitespace(s, pos);
    size_t start = pos;
    int depth = 0;
    bool in_single_quote = false;
    bool in_double_quote = false;
    while (pos < s.size()) {
        char c = s[pos];
        if (in_single_quote) {
            if (c == '\'' && pos + 1 < s.size() && s[pos+1] == '\'') {
                pos += 2; // escaped quote
                continue;
            }
            if (c == '\'') in_single_quote = false;
        } else if (in_double_quote) {
            if (c == '"') in_double_quote = false;
        } else {
            if (c == '\'') { in_single_quote = true; }
            else if (c == '"') { in_double_quote = true; }
            else if (c == '(') { ++depth; }
            else if (c == ')') {
                if (depth == 0) break; // close of clause — stop
                --depth;
            }
            else if (c == ',' && depth == 0) break; // item separator — stop
        }
        ++pos;
    }
    // Trim trailing whitespace
    size_t end = pos;
    while (end > start && (s[end-1] == ' ' || s[end-1] == '\t' ||
           s[end-1] == '\n' || s[end-1] == '\r')) {
        --end;
    }
    return s.substr(start, end - start);
}

// Read content inside parentheses (not including the parens).
// pos must point at the opening '('.
// Returns content as-is; pos will be after the closing ')'.
static std::string read_paren_content(const std::string &s, size_t &pos) {
    skip_whitespace(s, pos);
    if (pos >= s.size() || s[pos] != '(') {
        throw std::runtime_error("expected '(' in DDL");
    }
    ++pos; // skip '('
    size_t start = pos;
    int depth = 1;
    bool in_single_quote = false;
    bool in_double_quote = false;
    while (pos < s.size() && depth > 0) {
        char c = s[pos];
        if (in_single_quote) {
            if (c == '\'' && pos + 1 < s.size() && s[pos+1] == '\'') {
                pos += 2; continue;
            }
            if (c == '\'') in_single_quote = false;
        } else if (in_double_quote) {
            if (c == '"') in_double_quote = false;
        } else {
            if (c == '\'') in_single_quote = true;
            else if (c == '"') in_double_quote = true;
            else if (c == '(') ++depth;
            else if (c == ')') { --depth; if (depth == 0) { break; } }
        }
        ++pos;
    }
    std::string content = s.substr(start, pos - start);
    if (pos < s.size() && s[pos] == ')') ++pos; // skip closing ')'
    return content;
}

// JSON string escaping: escape backslash and double-quote
static std::string json_escape(const std::string &s) {
    std::string out;
    out.reserve(s.size() + 4);
    for (char c : s) {
        if (c == '\\') out += "\\\\";
        else if (c == '"') out += "\\\"";
        else if (c == '\n') out += "\\n";
        else if (c == '\r') out += "\\r";
        else if (c == '\t') out += "\\t";
        else out += c;
    }
    return out;
}

// Parse a comma-separated list of items inside an already-extracted clause body.
// Each item is read with read_expression, which respects paren depth.
static std::vector<std::string> split_clause_items(const std::string &body) {
    std::vector<std::string> items;
    size_t pos = 0;
    while (pos < body.size()) {
        skip_whitespace(body, pos);
        if (pos >= body.size()) break;
        std::string item = read_expression(body, pos);
        // Trim trailing whitespace from item
        size_t e = item.size();
        while (e > 0 && (item[e-1] == ' ' || item[e-1] == '\t' ||
               item[e-1] == '\n' || item[e-1] == '\r')) --e;
        item = item.substr(0, e);
        if (!item.empty()) items.push_back(item);
        skip_whitespace(body, pos);
        if (pos < body.size() && body[pos] == ',') ++pos; // skip comma
    }
    return items;
}

struct TableEntry {
    std::string alias;
    std::string physical_table;
    std::vector<std::string> pk_cols; // PRIMARY KEY columns (not used in model yet)
};

struct RelationshipEntry {
    std::string from_alias;
    std::vector<std::string> fk_cols;
    std::string ref_alias;
};

struct FieldEntry {
    std::string source_table; // alias
    std::string name;
    std::string expr;
};

// Parse a TABLES clause item: "alias AS physical_table [PRIMARY KEY (cols)]"
static TableEntry parse_table_item(const std::string &item) {
    TableEntry e;
    size_t pos = 0;
    e.alias = read_identifier(item, pos);
    if (e.alias.empty()) throw std::runtime_error("TABLES: expected table alias");

    std::string keyword = read_word(item, pos);
    if (keyword != "AS") throw std::runtime_error("TABLES: expected AS after alias '" + e.alias + "'");

    e.physical_table = read_identifier(item, pos);
    if (e.physical_table.empty()) throw std::runtime_error("TABLES: expected table name after AS");

    skip_whitespace(item, pos);
    if (pos < item.size()) {
        // Optional PRIMARY KEY clause — parse and discard (not in model yet)
        std::string kw = read_word(item, pos);
        if (kw == "PRIMARY") {
            std::string kw2 = read_word(item, pos);
            if (kw2 == "KEY") {
                std::string pk_body = read_paren_content(item, pos);
                // Parse pk_cols but don't use them currently
                (void)pk_body;
            }
        }
    }
    return e;
}

// Parse a RELATIONSHIPS clause item: "from_alias(fk_col, ...) REFERENCES ref_alias"
static RelationshipEntry parse_relationship_item(const std::string &item) {
    RelationshipEntry e;
    size_t pos = 0;
    e.from_alias = read_identifier(item, pos);
    if (e.from_alias.empty()) throw std::runtime_error("RELATIONSHIPS: expected from_alias");

    skip_whitespace(item, pos);
    if (pos >= item.size() || item[pos] != '(') {
        throw std::runtime_error("RELATIONSHIPS: expected '(' after from_alias '" + e.from_alias + "'");
    }
    std::string fk_body = read_paren_content(item, pos);
    // Split FK columns by comma
    size_t fk_pos = 0;
    while (fk_pos < fk_body.size()) {
        skip_whitespace(fk_body, fk_pos);
        std::string col = read_identifier(fk_body, fk_pos);
        if (!col.empty()) e.fk_cols.push_back(col);
        skip_whitespace(fk_body, fk_pos);
        if (fk_pos < fk_body.size() && fk_body[fk_pos] == ',') ++fk_pos;
    }

    std::string kw = read_word(item, pos);
    if (kw != "REFERENCES") {
        throw std::runtime_error("RELATIONSHIPS: expected REFERENCES after FK columns");
    }
    e.ref_alias = read_identifier(item, pos);
    if (e.ref_alias.empty()) throw std::runtime_error("RELATIONSHIPS: expected ref_alias after REFERENCES");
    return e;
}

// Parse a DIMENSIONS/METRICS/FACTS clause item: "alias.name AS sql_expr"
static FieldEntry parse_field_item(const std::string &item, const std::string &clause_name) {
    FieldEntry e;
    size_t pos = 0;

    // Read "alias.name" — may be "alias.field_name" or just "field_name"
    std::string full_name = read_identifier(item, pos);
    if (full_name.empty()) throw std::runtime_error(clause_name + ": expected field name");

    size_t dot = full_name.find('.');
    if (dot != std::string::npos) {
        e.source_table = full_name.substr(0, dot);
        e.name = full_name.substr(dot + 1);
    } else {
        e.name = full_name;
    }

    std::string kw = read_word(item, pos);
    if (kw != "AS") {
        throw std::runtime_error(clause_name + ": expected AS after field name '" + full_name + "'");
    }

    skip_whitespace(item, pos);
    e.expr = item.substr(pos);
    // Trim trailing whitespace
    size_t e_end = e.expr.size();
    while (e_end > 0 && (e.expr[e_end-1] == ' ' || e.expr[e_end-1] == '\t' ||
           e.expr[e_end-1] == '\n' || e.expr[e_end-1] == '\r')) --e_end;
    e.expr = e.expr.substr(0, e_end);
    if (e.expr.empty()) throw std::runtime_error(clause_name + ": expected SQL expression after AS");
    return e;
}

// Main tokenizer: parses the full CREATE or DROP statement and returns SemanticViewsDDLData
static unique_ptr<SemanticViewsDDLData> ParseSemanticViewStatement(const std::string &query) {
    auto data = make_unique<SemanticViewsDDLData>();
    std::string upper = StringUtil::Upper(query);
    size_t pos = 0;

    // Parse CREATE or DROP
    std::string first_kw = read_word(upper, pos);
    if (first_kw == "DROP") {
        data->ddl_type = SemanticViewsDDLType::DROP;

        std::string kw2 = read_word(upper, pos);
        if (kw2 != "SEMANTIC") throw std::runtime_error("expected SEMANTIC after DROP");
        std::string kw3 = read_word(upper, pos);
        if (kw3 != "VIEW") throw std::runtime_error("expected VIEW after DROP SEMANTIC");

        // Optional IF EXISTS
        size_t save_pos = pos;
        std::string kw4 = read_word(upper, pos);
        if (kw4 == "IF") {
            std::string kw5 = read_word(upper, pos);
            if (kw5 != "EXISTS") throw std::runtime_error("expected EXISTS after DROP SEMANTIC VIEW IF");
            data->if_exists = true;
        } else {
            pos = save_pos; // not IF, backtrack
        }

        // View name (read from original query to preserve case)
        size_t name_start = pos;
        skip_whitespace(query, name_start);
        data->view_name = read_identifier(query, name_start);
        if (data->view_name.empty()) throw std::runtime_error("expected view name after DROP SEMANTIC VIEW");

        return data;
    }

    // CREATE branch
    if (first_kw != "CREATE") {
        throw std::runtime_error("expected CREATE or DROP");
    }
    data->ddl_type = SemanticViewsDDLType::CREATE;

    // Optional OR REPLACE
    size_t save_pos = pos;
    std::string kw2 = read_word(upper, pos);
    if (kw2 == "OR") {
        std::string kw3 = read_word(upper, pos);
        if (kw3 != "REPLACE") throw std::runtime_error("expected REPLACE after OR");
        data->or_replace = true;
        kw2 = read_word(upper, pos);
    }

    if (kw2 != "SEMANTIC") throw std::runtime_error("expected SEMANTIC after CREATE");
    std::string kw3 = read_word(upper, pos);
    if (kw3 != "VIEW") throw std::runtime_error("expected VIEW after CREATE SEMANTIC");

    // Optional IF NOT EXISTS
    save_pos = pos;
    std::string kw4 = read_word(upper, pos);
    if (kw4 == "IF") {
        std::string kw5 = read_word(upper, pos);
        std::string kw6 = read_word(upper, pos);
        if (kw5 != "NOT" || kw6 != "EXISTS") {
            throw std::runtime_error("expected NOT EXISTS after CREATE SEMANTIC VIEW IF");
        }
        data->if_not_exists = true;
    } else {
        pos = save_pos;
    }

    // View name (read from original query at the same position to preserve case)
    size_t name_start = pos;
    skip_whitespace(query, name_start);
    data->view_name = read_identifier(query, name_start);
    // Also advance pos in upper
    skip_whitespace(upper, pos);
    read_identifier(upper, pos); // skip the name in upper too
    if (data->view_name.empty()) throw std::runtime_error("expected view name after CREATE SEMANTIC VIEW");

    // Parse clauses. Read the rest of the query from original (not upper) for expressions.
    // We'll work from the original query from here, using upper for keyword matching.
    // pos is now at the position after the view name in both strings.

    std::vector<TableEntry> tables;
    std::vector<RelationshipEntry> relationships;
    std::vector<FieldEntry> facts;
    std::vector<FieldEntry> dimensions;
    std::vector<FieldEntry> metrics;

    bool found_tables = false;

    while (pos < upper.size()) {
        skip_whitespace(upper, pos);
        if (pos >= upper.size() || upper[pos] == ';') break;

        size_t clause_pos = pos;
        std::string clause_kw = read_word(upper, pos);

        if (clause_kw == "TABLES") {
            found_tables = true;
            size_t orig_pos = clause_pos + (pos - clause_pos); // align with upper
            // Find '(' in original query at this position
            size_t body_start = pos;
            skip_whitespace(query, body_start);
            // read_paren_content needs pos pointing at '(' in ORIGINAL query
            // We've been tracking pos in upper (same offsets since upper has same length)
            std::string body = read_paren_content(query, pos);
            auto items = split_clause_items(body);
            for (auto &item : items) {
                tables.push_back(parse_table_item(item));
            }
        } else if (clause_kw == "RELATIONSHIPS") {
            std::string body = read_paren_content(query, pos);
            auto items = split_clause_items(body);
            for (auto &item : items) {
                relationships.push_back(parse_relationship_item(item));
            }
        } else if (clause_kw == "FACTS") {
            std::string body = read_paren_content(query, pos);
            auto items = split_clause_items(body);
            for (auto &item : items) {
                facts.push_back(parse_field_item(item, "FACTS"));
            }
        } else if (clause_kw == "DIMENSIONS") {
            std::string body = read_paren_content(query, pos);
            auto items = split_clause_items(body);
            for (auto &item : items) {
                dimensions.push_back(parse_field_item(item, "DIMENSIONS"));
            }
        } else if (clause_kw == "METRICS") {
            std::string body = read_paren_content(query, pos);
            auto items = split_clause_items(body);
            for (auto &item : items) {
                metrics.push_back(parse_field_item(item, "METRICS"));
            }
        } else if (clause_kw.empty()) {
            break;
        } else {
            throw std::runtime_error("unexpected keyword '" + clause_kw + "' in CREATE SEMANTIC VIEW");
        }
    }

    if (!found_tables) {
        throw std::runtime_error("CREATE SEMANTIC VIEW requires a TABLES clause");
    }
    if (tables.empty()) {
        throw std::runtime_error("TABLES clause must declare at least one table");
    }

    // Collect declared aliases for validation
    std::set<std::string> declared_aliases_upper;
    for (auto &t : tables) {
        std::string a = t.alias;
        for (auto &c : a) c = (char)toupper((unsigned char)c);
        declared_aliases_upper.insert(a);
    }

    // Validate that all source_table aliases in fields are declared in TABLES
    auto validate_alias = [&](const std::string &alias, const std::string &ctx) {
        if (!alias.empty()) {
            std::string ua = alias;
            for (auto &c : ua) c = (char)toupper((unsigned char)c);
            if (declared_aliases_upper.find(ua) == declared_aliases_upper.end()) {
                throw std::runtime_error(ctx + ": table alias '" + alias + "' not declared in TABLES clause");
            }
        }
    };
    for (auto &f : facts) validate_alias(f.source_table, "FACTS");
    for (auto &d : dimensions) validate_alias(d.source_table, "DIMENSIONS");
    for (auto &m : metrics) validate_alias(m.source_table, "METRICS");
    for (auto &r : relationships) {
        validate_alias(r.from_alias, "RELATIONSHIPS");
        validate_alias(r.ref_alias, "RELATIONSHIPS");
    }

    // Determine base_table:
    // Base = table alias NOT referenced as the target of any REFERENCES clause.
    // If no RELATIONSHIPS, first declared table is base.
    // If all tables are referenced (circular), use first declared table.
    std::set<std::string> ref_targets_upper;
    for (auto &r : relationships) {
        std::string ua = r.ref_alias;
        for (auto &c : ua) c = (char)toupper((unsigned char)c);
        ref_targets_upper.insert(ua);
    }

    std::string base_alias;
    std::string base_physical;
    for (auto &t : tables) {
        std::string ua = t.alias;
        for (auto &c : ua) c = (char)toupper((unsigned char)c);
        if (ref_targets_upper.find(ua) == ref_targets_upper.end()) {
            base_alias = t.alias;
            base_physical = t.physical_table;
            break;
        }
    }
    if (base_alias.empty()) {
        // All tables referenced (circular) — use first
        base_alias = tables[0].alias;
        base_physical = tables[0].physical_table;
    }

    // Build joins (non-base tables)
    // For each non-base table: determine from_cols from RELATIONSHIPS where from_alias = this table
    std::vector<std::pair<std::string, std::vector<std::string>>> join_entries;
    for (auto &t : tables) {
        std::string ua = t.alias;
        for (auto &c : ua) c = (char)toupper((unsigned char)c);
        std::string base_ua = base_alias;
        for (auto &c : base_ua) c = (char)toupper((unsigned char)c);
        if (ua == base_ua) continue; // skip base table

        std::vector<std::string> from_cols;
        for (auto &r : relationships) {
            std::string rua = r.from_alias;
            for (auto &c : rua) c = (char)toupper((unsigned char)c);
            if (rua == ua) {
                from_cols = r.fk_cols;
                break;
            }
        }
        join_entries.push_back({t.physical_table, from_cols});
    }

    // Build JSON definition string
    // Format: {"base_table":"...","dimensions":[...],"metrics":[...],"filters":[],"joins":[...],"facts":[...]}
    std::string json = "{";
    json += "\"base_table\":\"" + json_escape(base_physical) + "\"";

    // dimensions
    json += ",\"dimensions\":[";
    for (size_t i = 0; i < dimensions.size(); ++i) {
        if (i > 0) json += ",";
        auto &d = dimensions[i];
        json += "{\"name\":\"" + json_escape(d.name) + "\"";
        json += ",\"expr\":\"" + json_escape(d.expr) + "\"";
        if (!d.source_table.empty()) {
            json += ",\"source_table\":\"" + json_escape(d.source_table) + "\"";
        }
        json += "}";
    }
    json += "]";

    // metrics
    json += ",\"metrics\":[";
    for (size_t i = 0; i < metrics.size(); ++i) {
        if (i > 0) json += ",";
        auto &m = metrics[i];
        json += "{\"name\":\"" + json_escape(m.name) + "\"";
        json += ",\"expr\":\"" + json_escape(m.expr) + "\"";
        if (!m.source_table.empty()) {
            json += ",\"source_table\":\"" + json_escape(m.source_table) + "\"";
        }
        json += "}";
    }
    json += "]";

    // filters (always empty from DDL — FILTERS clause not in grammar)
    json += ",\"filters\":[]";

    // joins
    json += ",\"joins\":[";
    for (size_t i = 0; i < join_entries.size(); ++i) {
        if (i > 0) json += ",";
        auto &je = join_entries[i];
        json += "{\"table\":\"" + json_escape(je.first) + "\"";
        json += ",\"from_cols\":[";
        for (size_t k = 0; k < je.second.size(); ++k) {
            if (k > 0) json += ",";
            json += "\"" + json_escape(je.second[k]) + "\"";
        }
        json += "]}";
    }
    json += "]";

    // facts
    json += ",\"facts\":[";
    for (size_t i = 0; i < facts.size(); ++i) {
        if (i > 0) json += ",";
        auto &f = facts[i];
        json += "{\"name\":\"" + json_escape(f.name) + "\"";
        json += ",\"expr\":\"" + json_escape(f.expr) + "\"";
        if (!f.source_table.empty()) {
            json += ",\"source_table\":\"" + json_escape(f.source_table) + "\"";
        }
        json += "}";
    }
    json += "]";

    json += "}";

    data->definition_json = json;
    return data;
}

// ---------------------------------------------------------------------------
// Parser extension functions
// ---------------------------------------------------------------------------

// parse_function_t — called for every statement DuckDB's native parser fails.
// Must return DISPLAY_ORIGINAL_ERROR for non-matching input (DDL-06).
static ParserExtensionParseResult SemanticViewsParseFunction(
    ParserExtensionInfo *info, const std::string &query) {
    // Fast keyword check — return DISPLAY_ORIGINAL_ERROR for non-matching SQL
    std::string upper = StringUtil::Upper(query);
    bool is_create = (upper.find("CREATE") != std::string::npos &&
                      upper.find("SEMANTIC") != std::string::npos);
    bool is_drop   = (upper.find("DROP") != std::string::npos &&
                      upper.find("SEMANTIC") != std::string::npos);
    if (!is_create && !is_drop) {
        return ParserExtensionParseResult(); // DISPLAY_ORIGINAL_ERROR — fall through
    }
    try {
        auto data = ParseSemanticViewStatement(query);
        return ParserExtensionParseResult(std::move(data));
    } catch (const std::exception &e) {
        return ParserExtensionParseResult(std::string(e.what()));
    }
}

// Forward declarations
static unique_ptr<FunctionData> SemanticViewsDDLBind(
    ClientContext &context, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names);
static void SemanticViewsDDLScan(
    ClientContext &context, TableFunctionInput &data, DataChunk &output);

// plan_function_t — converts parse data into a ParserExtensionPlanResult (TableFunction).
// Called with ClientContext lock held — NEVER call context.Query() here.
static ParserExtensionPlanResult SemanticViewsPlanFunction(
    ParserExtensionInfo *info_base, ClientContext &context,
    unique_ptr<ParserExtensionParseData> parse_data) {

    auto &info = *static_cast<SemanticViewsParserInfo*>(info_base);
    auto &ddl = *static_cast<SemanticViewsDDLData*>(parse_data.get());

    // Build TableFunction with empty output (DDL returns no rows)
    TableFunction func("semantic_view_ddl_exec", {}, SemanticViewsDDLScan,
                       SemanticViewsDDLBind);

    ParserExtensionPlanResult result;
    result.function = func;

    // Pass all DDL data + catalog/connection pointers as parameters.
    // Bind function extracts them by index. Order must match extraction in SemanticViewsDDLBind.
    result.parameters.push_back(Value(ddl.view_name));              // 0: view_name
    result.parameters.push_back(Value(ddl.definition_json));        // 1: definition_json
    result.parameters.push_back(Value((int32_t)ddl.ddl_type));      // 2: ddl_type
    result.parameters.push_back(Value((bool)ddl.or_replace));       // 3: or_replace
    result.parameters.push_back(Value((bool)ddl.if_not_exists));    // 4: if_not_exists
    result.parameters.push_back(Value((bool)ddl.if_exists));        // 5: if_exists
    result.parameters.push_back(Value::POINTER((uintptr_t)info.catalog_ptr));  // 6: catalog_ptr
    result.parameters.push_back(Value::POINTER((uintptr_t)info.persist_conn)); // 7: persist_conn

    result.return_type = StatementReturnType::NOTHING;
    result.modified_databases["main"] = {};
    result.requires_valid_transaction = true;
    return result;
}

// Bind function — extracts parameters from the plan result into bind data.
static unique_ptr<FunctionData> SemanticViewsDDLBind(
    ClientContext &context, TableFunctionBindInput &input,
    vector<LogicalType> &return_types, vector<string> &names) {

    auto bind_data = make_unique<SemanticViewsDDLBindData>();
    bind_data->view_name        = input.inputs[0].GetValue<string>();
    bind_data->definition_json  = input.inputs[1].GetValue<string>();
    bind_data->ddl_type         = (SemanticViewsDDLType)input.inputs[2].GetValue<int32_t>();
    bind_data->or_replace       = input.inputs[3].GetValue<bool>();
    bind_data->if_not_exists    = input.inputs[4].GetValue<bool>();
    bind_data->if_exists        = input.inputs[5].GetValue<bool>();
    bind_data->catalog_ptr      = (const void*)input.inputs[6].GetPointer();
    bind_data->persist_conn     = (duckdb_connection)input.inputs[7].GetPointer();

    // DDL returns no output columns
    return_types.clear();
    names.clear();
    return std::move(bind_data);
}

// Scan function — executes the DDL operation via persist_conn and catalog FFI.
// Called with ClientContext lock held — NEVER call context.Query() here.
static void SemanticViewsDDLScan(
    ClientContext &context, TableFunctionInput &data_in, DataChunk &output) {

    auto &bind_data = data_in.bind_data->Cast<SemanticViewsDDLBindData>();
    output.SetCardinality(0);

    if (bind_data.ddl_type == SemanticViewsDDLType::CREATE) {
        // 1. Persist: write to semantic_layer._definitions via persist_conn
        //    (separate connection — does not hold context_lock, no deadlock)
        int persist_rc = semantic_views_pragma_define(
            bind_data.persist_conn,
            bind_data.view_name.c_str(),
            bind_data.definition_json.c_str());

        // For OR REPLACE: ignore persist failure (INSERT OR REPLACE handles duplicates)
        // For plain CREATE: a persist failure is a real error
        if (persist_rc != 0 && !bind_data.or_replace) {
            throw std::runtime_error("Failed to persist semantic view '" +
                                     bind_data.view_name + "' to catalog table");
        }

        // 2. Update in-memory catalog via Rust FFI
        int cat_rc;
        if (bind_data.or_replace) {
            cat_rc = semantic_views_catalog_upsert(
                bind_data.catalog_ptr,
                bind_data.view_name.c_str(),
                bind_data.definition_json.c_str());
        } else {
            cat_rc = semantic_views_catalog_insert(
                bind_data.catalog_ptr,
                bind_data.view_name.c_str(),
                bind_data.definition_json.c_str());
        }

        if (cat_rc != 0) {
            // IF NOT EXISTS: silently succeed when view already exists
            if (bind_data.if_not_exists) {
                output.SetCardinality(0);
                return;
            }
            throw std::runtime_error("Semantic view '" + bind_data.view_name +
                                     "' already exists");
        }
    } else {
        // DROP branch
        // 1. Persist: remove from semantic_layer._definitions
        semantic_views_pragma_drop(
            bind_data.persist_conn,
            bind_data.view_name.c_str());

        // 2. Update in-memory catalog
        if (bind_data.if_exists) {
            semantic_views_catalog_delete_if_exists(
                bind_data.catalog_ptr,
                bind_data.view_name.c_str());
        } else {
            int cat_rc = semantic_views_catalog_delete(
                bind_data.catalog_ptr,
                bind_data.view_name.c_str());
            if (cat_rc != 0) {
                throw std::runtime_error("Semantic view '" + bind_data.view_name +
                                         "' does not exist");
            }
        }
    }
}

extern "C" {

// ---------------------------------------------------------------------------
// semantic_views_register_shim — called at extension load time (lib.rs)
// Phase 10: registers pragma callbacks.
// Phase 11: also registers parser hooks for CREATE/DROP SEMANTIC VIEW DDL.
// ---------------------------------------------------------------------------
void semantic_views_register_shim(
    void* db_instance_ptr,
    const void* catalog_raw_ptr,
    duckdb_connection persist_conn_param
) {
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

    // Register parser extension for CREATE/DROP SEMANTIC VIEW (Phase 11)
    auto &config = DBConfig::GetConfig(db_instance);
    ParserExtension parser_ext;
    parser_ext.parse_function = SemanticViewsParseFunction;
    parser_ext.plan_function = SemanticViewsPlanFunction;
    parser_ext.parser_info = make_shared_ptr<SemanticViewsParserInfo>(
        catalog_raw_ptr, persist_conn_param);
    config.parser_extensions.push_back(parser_ext);
}

// ---------------------------------------------------------------------------
// semantic_views_pragma_define / _drop — called from the C++ parser hook scan function
//
// These use a SEPARATE connection (conn) created at init time. Because it is
// a different connection, it has its own transaction and does NOT deadlock with
// the main connection's execution lock (context_lock is non-reentrant std::mutex).
//
// Limitation: writes via this connection are NOT in the user's explicit transaction.
// For transactional usage, call PRAGMA define_semantic_view_internal(...) directly.
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
