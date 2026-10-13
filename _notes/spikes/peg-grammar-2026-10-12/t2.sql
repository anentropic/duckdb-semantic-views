LOAD './svspike.duckdb_extension';
-- table ref
SELECT * FROM SEMANTIC_VIEW(v1 DIMENSIONS region, yr) AS t;
-- in a join, CTE and subquery
WITH c AS (SELECT * FROM SEMANTIC_VIEW(v1 DIMENSIONS region)) SELECT count(*) AS n FROM c JOIN SEMANTIC_VIEW(v1 DIMENSIONS region) s USING (item);
-- view over it (must survive without the grammar)
CREATE VIEW plain AS SELECT * FROM SEMANTIC_VIEW(v1 DIMENSIONS a, b);
SELECT sql FROM duckdb_views() WHERE view_name = 'plain';
-- deprecated string form falls through to an ordinary table function call
SELECT * FROM semantic_view('v1', dimensions := ['x']);
