#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2"]
# requires-python = ">=3.10"
# ///
"""
uv run examples/type_inference.py

Demonstrates v0.7.1 features:
  - DDL-time type inference for dimensions and metrics
  - Inferred DATA_TYPE in DESCRIBE, SHOW SEMANTIC DIMENSIONS/METRICS, SHOW COLUMNS
  - DECIMAL avoidance (parameterized types left empty to avoid lossy CAST)
  - In-memory vs file-backed behavior

Requires a debug build: just build
"""
import duckdb
import os
import tempfile

# ============================================================
# File-backed database (type inference active)
# ============================================================

tmpdir = tempfile.mkdtemp()
db_path = os.path.join(tmpdir, "type_inference_demo.duckdb")
con = duckdb.connect(db_path, config={"allow_unsigned_extensions": "true"})
con.execute("LOAD 'build/debug/semantic_views.duckdb_extension'")

con.execute("""
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    region VARCHAR,
    ordered_at DATE,
    quantity INTEGER,
    unit_price DOUBLE,
    total DECIMAL(10,2)
);
INSERT INTO orders VALUES
    (1, 'US',  '2024-01-15', 3,  25.00, 75.00),
    (2, 'EU',  '2024-01-20', 1,  50.00, 50.00),
    (3, 'US',  '2024-02-10', 2, 100.00, 200.00),
    (4, 'EU',  '2024-02-14', 5,  10.00, 50.00);
""")

con.execute("""
CREATE SEMANTIC VIEW sales AS
TABLES (
    o AS orders PRIMARY KEY (id)
)
DIMENSIONS (
    o.region    AS o.region,
    o.order_day AS o.ordered_at
)
METRICS (
    o.order_count AS COUNT(*),
    o.avg_price   AS AVG(o.unit_price),
    o.total_qty   AS SUM(o.quantity),
    o.gross_total AS SUM(o.total)
);
""")

# DESCRIBE shows DATA_TYPE for each dimension and metric
print("=== DESCRIBE SEMANTIC VIEW (file-backed) ===")
for row in con.execute("DESCRIBE SEMANTIC VIEW sales").fetchall():
    kind, name, parent, prop, val = row
    if prop == "DATA_TYPE":
        print(f"  {kind:12s} {name:14s} DATA_TYPE = {val or '(empty)'}")

# SHOW SEMANTIC DIMENSIONS with inferred data_type
print("\n=== SHOW SEMANTIC DIMENSIONS ===")
for row in con.execute("SHOW SEMANTIC DIMENSIONS IN sales").fetchall():
    print(f"  {row[4]:14s} data_type={row[5] or '(empty)'}")

# SHOW SEMANTIC METRICS with inferred data_type
print("\n=== SHOW SEMANTIC METRICS ===")
for row in con.execute("SHOW SEMANTIC METRICS IN sales").fetchall():
    print(f"  {row[4]:14s} data_type={row[5] or '(empty)'}")

# SHOW COLUMNS for a unified view
print("\n=== SHOW COLUMNS IN SEMANTIC VIEW ===")
for row in con.execute("SHOW COLUMNS IN SEMANTIC VIEW sales").fetchall():
    print(f"  {row[3]:14s} data_type={row[4] or '(empty)':10s} kind={row[5]}")

# Note: SUM(o.total) where total is DECIMAL(10,2) produces empty data_type
# because DECIMAL is a parameterized type and bare "DECIMAL" would lose
# the precision/scale, causing a lossy CAST. This is intentional.

con.close()

# ============================================================
# In-memory database (no type inference)
# ============================================================

con_mem = duckdb.connect(config={"allow_unsigned_extensions": "true"})
con_mem.execute("LOAD 'build/debug/semantic_views.duckdb_extension'")

con_mem.execute("""
CREATE TABLE orders (id INTEGER, region VARCHAR, amount DOUBLE);
INSERT INTO orders VALUES (1, 'US', 100), (2, 'EU', 200);

CREATE SEMANTIC VIEW sales_mem AS
TABLES (o AS orders PRIMARY KEY (id))
DIMENSIONS (o.region AS o.region)
METRICS (o.revenue AS SUM(o.amount));
""")

print("\n=== SHOW SEMANTIC DIMENSIONS (in-memory, no inference) ===")
for row in con_mem.execute("SHOW SEMANTIC DIMENSIONS IN sales_mem").fetchall():
    print(f"  {row[4]:14s} data_type={row[5] or '(empty)'}")

print("\n=== SHOW SEMANTIC METRICS (in-memory, no inference) ===")
for row in con_mem.execute("SHOW SEMANTIC METRICS IN sales_mem").fetchall():
    print(f"  {row[4]:14s} data_type={row[5] or '(empty)'}")

con_mem.close()

# Cleanup
import shutil
shutil.rmtree(tmpdir)
