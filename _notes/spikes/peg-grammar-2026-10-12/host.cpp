#include "duckdb.hpp"
#include <iostream>
using namespace duckdb;
static void Q(Connection &c, const char *label, const string &sql) {
	auto r = c.Query(sql);
	std::cout << "-- " << label << ": " << sql << "\n" << (r->HasError() ? "ERROR: " + r->GetError() + "\n" : r->ToString());
}
int main() {
	DBConfig config;
	config.SetOptionByName("allow_unsigned_extensions", Value::BOOLEAN(true));
	DuckDB db("host.db", &config);
	Connection pre(db);   // opened BEFORE load
	Connection loader(db);
	Q(loader, "loader", "LOAD './svspike.duckdb_extension'");
	Q(pre, "pre-load conn", "SELECT current_setting('active_grammar_extensions') AS active");
	Q(pre, "pre-load conn", "CREATE SEMANTIC VIEW v9 AS DIMENSIONS (o.a AS a)");
	Connection post(db);  // opened AFTER load
	Q(post, "post-load conn", "SELECT current_setting('active_grammar_extensions') AS active");
	Q(post, "post-load conn", "SELECT count(*) FROM (SHOW SEMANTIC VIEWS)");
	Q(post, "post-load conn", "CREATE VIEW plain AS SELECT * FROM SEMANTIC_VIEW(v9 DIMENSIONS a) AS t");
	Q(post, "user clears list", "SET active_grammar_extensions = []");
	Q(post, "grammar off", "SHOW SEMANTIC VIEWS");
	Q(post, "grammar off, view over construct", "SELECT * FROM plain");
	Q(post, "user re-enables", "SET active_grammar_extensions = ['svspike']");
	Q(post, "re-enabled", "SELECT count(*) FROM (SHOW SEMANTIC VIEWS)");
	Q(post, "reset", "RESET active_grammar_extensions");
	Q(post, "after RESET", "SELECT current_setting('active_grammar_extensions') AS active");
	return 0;
}
