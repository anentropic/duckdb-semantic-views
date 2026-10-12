LOAD './svspike.duckdb_extension';
CREATE SEMANTIC VIEW v1 AS DIMENSIONS (o.a AS a /* comment */ + 1, o.b AS "Quoted Col" || E'it''s');
SELECT * FROM sv_defs;
SELECT count(*) AS n FROM (SHOW SEMANTIC VIEWS);
WITH s AS (SHOW SEMANTIC VIEWS) SELECT name FROM s WHERE name LIKE 'v%';
SHOW SEMANTIC VIEWS;
SHOW TABLES;
DESCRIBE sv_defs;
-- syntax errors inside our rule
CREATE SEMANTIC VIEW bad AS DIMENSIONS (o.a AS );
CREATE SEMANTIC VIEW bad AS DIMENZIONS (o.a AS a);
