// Phase 1 spike: DuckDB 2.0 GrammarExtension from an extension that carries its own
// statically linked copy of DuckDB (as duckdb-semantic-views does with the amalgamation),
// loaded through the C_STRUCT(_UNSTABLE) entry point.
#include "duckdb.hpp"
#include "duckdb.h"
#include "duckdb/catalog/catalog.hpp"
#include "duckdb/parser/parsed_data/create_table_function_info.hpp"
#include "duckdb/main/capi/capi_internal.hpp"
#include "duckdb/main/connection_manager.hpp"
#include "duckdb/main/client_config.hpp"
#include "duckdb/main/settings.hpp"
#include "duckdb/parser/grammar_extension.hpp"
#include "duckdb/parser/parser.hpp"
#include "duckdb/parser/peg/transformer/peg_transformer.hpp"
#include "duckdb/parser/peg/transformer/parse_result.hpp"
#include "duckdb/parser/statement/multi_statement.hpp"
#include "duckdb/parser/statement/select_statement.hpp"
#include "duckdb/parser/query_node/select_node.hpp"
#include "duckdb/parser/tableref/table_function_ref.hpp"
#include "duckdb/parser/tableref/basetableref.hpp"
#include "duckdb/parser/expression/function_expression.hpp"
#include "duckdb/parser/expression/constant_expression.hpp"
#include "duckdb/parser/expression/star_expression.hpp"
#include "duckdb/planner/extension_callback.hpp"

using namespace duckdb;

static const char *GRAMMAR_NAME = "svspike";

// ---------------------------------------------------------------------------
// helpers

static void Collect(ParseResult &pr, const char *name, vector<reference<ParseResult>> &out) {
	if (StringUtil::CIEquals(pr.Name(), name)) {
		out.push_back(pr);
		return;
	}
	switch (pr.type) {
	case ParseResultType::LIST:
		for (auto &c : pr.Cast<ListParseResult>().GetChildren()) {
			Collect(c.get(), name, out);
		}
		break;
	case ParseResultType::REPEAT:
		for (auto &c : pr.Cast<RepeatParseResult>().GetChildren()) {
			Collect(c.get(), name, out);
		}
		break;
	case ParseResultType::OPTIONAL: {
		auto &opt = pr.Cast<OptionalParseResult>();
		if (opt.HasResult()) {
			Collect(opt.GetResult(), name, out);
		}
		break;
	}
	case ParseResultType::CHOICE:
		Collect(pr.Cast<ChoiceParseResult>().GetResult(), name, out);
		break;
	default:
		break;
	}
}

static ParseResult &First(ParseResult &pr, const char *name) {
	vector<reference<ParseResult>> out;
	Collect(pr, name, out);
	if (out.empty()) {
		throw InternalException("svspike: no '%s' under '%s'", name, pr.Name());
	}
	return out[0];
}

// Rebuild the source text of a parse result from the tokens it spans. The transformer
// has no access to the original query string, only to tokens (text + offset), so
// inter-token gaps are filled with spaces of the original width.
static string SourceText(PEGTransformer &t, ParseResult &pr) {
	auto loc = pr.GetLocation();
	string out;
	idx_t prev_end = loc.offset;
	auto &it = t.token_iterator;
	bool first = true;
	for (idx_t i = 0; i < it.Size(); i++) {
		auto &tok = it.GetToken(i);
		if (tok.offset < loc.offset || tok.offset >= loc.offset + loc.length) {
			continue;
		}
		if (!first) {
			out.append(tok.offset - prev_end, ' ');
		}
		first = false;
		out += tok.text;
		prev_end = tok.offset + tok.length;
	}
	return out;
}

static string Lit(const string &s) {
	return "'" + StringUtil::Replace(s, "'", "''") + "'";
}

// ---------------------------------------------------------------------------
// CREATE SEMANTIC VIEW name AS DIMENSIONS (t.d AS <expr>, ...)
// -> lowered SQL text -> nested Parser -> MultiStatement (runs on the caller's connection)

static arena_ptr<TransformResultValue> FinalizeCreate(PEGTransformer &transformer, ParseResult &pr) {
	auto name = SourceText(transformer, First(pr, "SvViewName"));
	vector<reference<ParseResult>> dims;
	Collect(pr, "SvDim", dims);
	string payload;
	for (auto &d : dims) {
		auto member = SourceText(transformer, First(d.get(), "SvMemberName"));
		auto expr = SourceText(transformer, First(d.get(), "SvExpr"));
		if (!payload.empty()) {
			payload += " | ";
		}
		payload += member + " := " + expr;
	}
	// Same shape as native_sql: a guard statement, then the write.
	string sql = "SELECT CASE WHEN EXISTS (SELECT 1 FROM sv_defs WHERE name = " + Lit(name) +
	             ") THEN error('semantic view ' || " + Lit(name) + " || ' already exists') END; " +
	             "INSERT INTO sv_defs VALUES (" + Lit(name) + ", " + Lit(payload) + ")";
	auto parser = Parser::GetBuiltinParser();
	parser.ParseQuery(sql);
	auto multi = make_uniq<MultiStatement>();
	for (auto &stmt : parser.statements) {
		multi->statements.push_back(std::move(stmt));
	}
	return transformer.MakeResult<unique_ptr<SQLStatement>>(std::move(multi));
}

static arena_ptr<TransformProcess> StartCreate(PEGTransformer &transformer, ParseResult &pr) {
	return transformer.MakeProcess<FinalizeTransformProcess>(transformer, pr, FinalizeCreate);
}

// ---------------------------------------------------------------------------
// FROM SEMANTIC_VIEW(name DIMENSIONS a, b) -> stock TableFunctionRef: sv_query('name', ['a','b'])

static arena_ptr<TransformResultValue> FinalizeRef(PEGTransformer &transformer, ParseResult &pr) {
	auto name = SourceText(transformer, First(pr, "SvViewName"));
	vector<reference<ParseResult>> items;
	Collect(pr, "SvItem", items);
	string list;
	for (auto &i : items) {
		list += (list.empty() ? "" : ", ") + Lit(SourceText(transformer, i.get()));
	}
	auto parser = Parser::GetBuiltinParser();
	vector<reference<ParseResult>> alias;
	Collect(pr, "TableAlias", alias);
	string alias_sql = alias.empty() ? "" : " " + SourceText(transformer, alias[0].get());
	parser.ParseQuery("SELECT * FROM sv_query(" + Lit(name) + ", [" + list + "])" + alias_sql);
	auto &select = parser.statements[0]->Cast<SelectStatement>();
	auto ref = std::move(select.node->Cast<SelectNode>().from_table);
	return transformer.MakeResult<unique_ptr<TableRef>>(std::move(ref));
}

static arena_ptr<TransformProcess> StartRef(PEGTransformer &transformer, ParseResult &pr) {
	return transformer.MakeProcess<FinalizeTransformProcess>(transformer, pr, FinalizeRef);
}

// ---------------------------------------------------------------------------
// SHOW SEMANTIC VIEWS -> QueryNode (DescribeStatement child) = SELECT * FROM sv_defs

static arena_ptr<TransformResultValue> FinalizeShow(PEGTransformer &transformer, ParseResult &pr) {
	auto parser = Parser::GetBuiltinParser();
	parser.ParseQuery("SELECT name, payload FROM sv_defs");
	auto node = std::move(parser.statements[0]->Cast<SelectStatement>().node);
	return transformer.MakeResult<unique_ptr<QueryNode>>(std::move(node));
}

static arena_ptr<TransformProcess> StartShow(PEGTransformer &transformer, ParseResult &pr) {
	return transformer.MakeProcess<FinalizeTransformProcess>(transformer, pr, FinalizeShow);
}

class SvGrammar final : public GrammarExtension {
public:
	SvGrammar() : GrammarExtension(GRAMMAR_NAME, "semantic views spike") {
	}
	vector<GrammarChange> GetChanges() const override {
		vector<GrammarChange> c;
		c.push_back(GrammarChange::AddChoice("UnreservedKeyword", "'SEMANTIC'"));
		c.push_back(GrammarChange::AddChoice("UnreservedKeyword", "'DIMENSIONS'"));
		c.push_back(GrammarChange::AddChoice("UnreservedKeyword", "'SEMANTIC_VIEW'"));
		c.push_back(GrammarChange::AddRule(
		    "SvCreateStatement <- 'CREATE' 'SEMANTIC' 'VIEW' SvViewName 'AS' 'DIMENSIONS' '(' SvDim (',' SvDim)* ')'",
		    StartCreate));
		c.push_back(GrammarChange::AddRule("SvViewName <- ColId"));
		c.push_back(GrammarChange::AddRule("SvDim <- SvMemberName 'AS' SvExpr"));
		c.push_back(GrammarChange::AddRule("SvMemberName <- ColId '.' ColId"));
		c.push_back(GrammarChange::AddRule("SvExpr <- Expression"));
		c.push_back(GrammarChange::PrependChoice("Statement", "SvCreateStatement"));

		c.push_back(GrammarChange::AddRule(
		    "SvTableRef <- 'SEMANTIC_VIEW' '(' SvViewName 'DIMENSIONS' SvItem (',' SvItem)* ')' TableAlias?", StartRef));
		c.push_back(GrammarChange::AddRule("SvItem <- ColId"));
		c.push_back(GrammarChange::PrependChoice("InnerTableRef", "SvTableRef"));

		c.push_back(GrammarChange::AddRule("SvShow <- 'SHOW' 'SEMANTIC' 'VIEWS'", StartShow));
		c.push_back(GrammarChange::PrependChoice("DescribeStatement", "SvShow"));
		return c;
	}
};

// ---------------------------------------------------------------------------
// sv_query(name, items) table function: returns one row per item

struct SvQueryBind : public TableFunctionData {
	vector<string> rows;
	idx_t pos = 0;
};

static unique_ptr<FunctionData> SvQueryBindFn(ClientContext &, TableFunctionBindInput &input,
                                              vector<LogicalType> &types, vector<Identifier> &names) {
	auto res = make_uniq<SvQueryBind>();
	auto view = input.inputs[0].GetValue<string>();
	for (auto &v : ListValue::GetChildren(input.inputs[1])) {
		res->rows.push_back(view + "." + v.GetValue<string>());
	}
	types.push_back(LogicalType::VARCHAR);
	names.emplace_back("item");
	return std::move(res);
}

static void SvQueryFn(ClientContext &, TableFunctionInput &data, DataChunk &output) {
	auto &b = (SvQueryBind &)*data.bind_data;
	auto &state = const_cast<SvQueryBind &>(b);
	idx_t n = 0;
	while (state.pos < state.rows.size() && n < STANDARD_VECTOR_SIZE) {
		output.SetValue(0, n++, Value(state.rows[state.pos++]));
	}
	output.SetCardinality(n);
}

// ---------------------------------------------------------------------------
// activation

static void Activate(ClientContext &context) {
	auto &config = ClientConfig::GetConfig(context);
	vector<Value> names;
	for (auto &e : config.active_grammar_extensions) {
		if (StringUtil::CIEquals(e, GRAMMAR_NAME)) {
			return;
		}
		names.push_back(Value(e));
	}
	names.push_back(Value(GRAMMAR_NAME));
	ActiveGrammarExtensionsSetting::SetLocal(context, Value::LIST(LogicalType::VARCHAR, std::move(names)));
}

class SvCallback final : public ExtensionCallback {
public:
	void OnConnectionOpened(ClientContext &context) override {
		Activate(context);
	}
};

extern "C" {
DUCKDB_EXTENSION_API bool svspike_init_c_api(duckdb_extension_info info, duckdb_extension_access *access) {
	try {
		auto *db_handle = access->get_database(info);
		auto *wrapper = reinterpret_cast<DatabaseWrapper *>((*db_handle)->internal_ptr);
		auto &db = *wrapper->database->instance;
		auto &config = DBConfig::GetConfig(db);

		GrammarExtension::Register(db, make_shared_ptr<SvGrammar>());
		ExtensionCallback::Register(config, make_shared_ptr<SvCallback>());

		{
			Connection con(db);
			con.Query("CREATE TABLE IF NOT EXISTS sv_defs(name VARCHAR, payload VARCHAR)");
			TableFunction tf("sv_query", {LogicalType::VARCHAR, LogicalType::LIST(LogicalType::VARCHAR)}, SvQueryFn,
			                 SvQueryBindFn);
			CreateTableFunctionInfo tf_info(tf);
			auto &catalog = Catalog::GetSystemCatalog(db);
			con.BeginTransaction();
			catalog.CreateTableFunction(*con.context, tf_info);
			con.Commit();
		}
		// Activate on every connection that already exists (incl. the one running LOAD).
		for (auto &ctx : db.GetConnectionManager().GetConnectionList()) {
			Activate(*ctx);
		}
		return true;
	} catch (std::exception &e) {
		ErrorData err(e);
		access->set_error(info, err.Message().c_str());
		return false;
	}
}
}
