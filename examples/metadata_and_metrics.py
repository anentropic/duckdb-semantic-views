#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.2"]
# requires-python = ">=3.10"
# ///
"""
uv run examples/metadata_and_metrics.py

Demonstrates v0.6.0 features:
  - COMMENT, SYNONYMS, PRIVATE/PUBLIC metadata annotations
  - ALTER SET/UNSET COMMENT
  - GET_DDL round-trip DDL reconstruction
  - Wildcard selection (table_alias.*)
  - Queryable FACTS (row-level unaggregated mode)
  - Semi-additive metrics (NON ADDITIVE BY)
  - Window function metrics (PARTITION BY EXCLUDING)
  - SHOW TERSE / SHOW COLUMNS / IN SCHEMA scope filtering
"""
import duckdb

con = duckdb.connect(config={"allow_unsigned_extensions": "true"})
con.execute("LOAD 'build/debug/semantic_views.duckdb_extension'")

# ============================================================
# Section 1: Setup -- Create physical tables
# ============================================================

con.execute("""
CREATE TABLE daily_balances (
    account_id INTEGER, balance_date DATE,
    balance DECIMAL(12,2), deposits DECIMAL(12,2)
);
INSERT INTO daily_balances VALUES
    (1, '2026-01-01', 1000.00, 100.00),
    (1, '2026-01-02', 1100.00, 200.00),
    (1, '2026-01-03', 1200.00, 150.00),
    (2, '2026-01-01', 5000.00, 500.00),
    (2, '2026-01-02', 5200.00, 300.00),
    (2, '2026-01-03', 5500.00, 400.00);

CREATE TABLE accounts (id INTEGER, name VARCHAR, region VARCHAR, tier VARCHAR);
INSERT INTO accounts VALUES
    (1, 'Alice', 'East', 'standard'),
    (2, 'Bob',   'West', 'premium');
""")

print("=== Section 1: Physical tables created ===")
print("  daily_balances: 6 rows (daily snapshots per account)")
print("  accounts:       2 rows (account metadata)")

# ============================================================
# Section 2: Metadata annotations -- COMMENT, SYNONYMS, PRIVATE
# ============================================================

print("\n=== Section 2: Metadata annotations ===")

con.execute("""
CREATE SEMANTIC VIEW banking
  COMMENT = 'Daily account balance analytics'
AS
  TABLES (
    b AS daily_balances PRIMARY KEY (account_id, balance_date)
      COMMENT = 'Daily balance snapshots',
    a AS accounts PRIMARY KEY (id)
      COMMENT = 'Account dimension table'
      WITH SYNONYMS = ('acct', 'customer')
  )
  RELATIONSHIPS (
    bal_to_acct AS b(account_id) REFERENCES a
  )
  FACTS (
    PRIVATE b.balance_change AS b.balance - LAG(b.balance) OVER (
        PARTITION BY b.account_id ORDER BY b.balance_date
    )
  )
  DIMENSIONS (
    a.region AS a.region
      COMMENT = 'Geographic region'
      WITH SYNONYMS = ('geo', 'area'),
    a.tier AS a.tier,
    b.balance_date AS b.balance_date
  )
  METRICS (
    b.total_deposits AS SUM(b.deposits)
      COMMENT = 'Sum of all deposits',
    b.latest_balance AS SUM(b.balance)
      NON ADDITIVE BY (b.balance_date DESC NULLS LAST)
      COMMENT = 'Most recent balance (snapshot metric)',
    b.daily_deposit_share AS SUM(b.deposits)
      OVER (PARTITION BY EXCLUDING (a.region))
      COMMENT = 'Deposit share within region partition'
  );
""")

print("  Created semantic view 'banking' with:")
print("  - View-level COMMENT")
print("  - Table-level COMMENTs and SYNONYMS")
print("  - PRIVATE fact (balance_change)")
print("  - Dimension COMMENTs and SYNONYMS")
print("  - Semi-additive metric (latest_balance)")
print("  - Window function metric (daily_deposit_share)")

# ============================================================
# Section 3: SHOW TERSE / SHOW COLUMNS / IN SCHEMA
# ============================================================

print("\n=== Section 3: Introspection enhancements ===")

print("\nSHOW TERSE SEMANTIC VIEWS:")
for row in con.execute("SHOW TERSE SEMANTIC VIEWS").fetchall():
    print(f"  {row}")

print("\nSHOW COLUMNS IN SEMANTIC VIEW banking:")
for row in con.execute("SHOW COLUMNS IN SEMANTIC VIEW banking").fetchall():
    print(f"  kind={row[0]}, name={row[1]}")

print("\nSHOW SEMANTIC VIEWS IN SCHEMA main:")
for row in con.execute("SHOW SEMANTIC VIEWS IN SCHEMA main").fetchall():
    print(f"  {row[0]} (comment: {row[4]})")

# ============================================================
# Section 4: ALTER COMMENT + GET_DDL
# ============================================================

print("\n=== Section 4: ALTER COMMENT + GET_DDL ===")

con.execute("ALTER SEMANTIC VIEW banking SET COMMENT = 'Updated: banking analytics v2'")
print("After ALTER SET COMMENT:")
for row in con.execute("SHOW SEMANTIC VIEWS LIKE 'banking'").fetchall():
    print(f"  comment: {row[4]}")

con.execute("ALTER SEMANTIC VIEW banking UNSET COMMENT")
print("\nAfter ALTER UNSET COMMENT:")
for row in con.execute("SHOW SEMANTIC VIEWS LIKE 'banking'").fetchall():
    print(f"  comment: {row[4]}")

# Restore comment for GET_DDL demo
con.execute("ALTER SEMANTIC VIEW banking SET COMMENT = 'Daily account balance analytics'")

print("\nGET_DDL round-trip:")
ddl = con.execute("SELECT GET_DDL('SEMANTIC_VIEW', 'banking')").fetchone()[0]
print(f"  (first 120 chars): {ddl[:120]}...")
print(f"  (total length): {len(ddl)} chars")

# ============================================================
# Section 5: Wildcard selection (table_alias.*)
# ============================================================

print("\n=== Section 5: Wildcard selection ===")

print("\nAll account dimensions (a.*):")
for row in con.execute("""
    SELECT * FROM semantic_view('banking',
        dimensions := ['a.*'],
        metrics := ['total_deposits']
    ) ORDER BY region
""").fetchall():
    print(f"  region={row[0]}, tier={row[1]}, total_deposits={row[2]}")

print("\nNote: PRIVATE facts/metrics excluded from wildcard expansion")

# ============================================================
# Section 6: Queryable FACTS (row-level mode)
# ============================================================

print("\n=== Section 6: Queryable FACTS ===")

# Note: balance_change is PRIVATE so we query deposits as a fact-level value
# Facts return unaggregated row-level results
print("\nRow-level fact query (deposits per day per account):")
for row in con.execute("""
    SELECT * FROM semantic_view('banking',
        facts := ['total_deposits'],
        dimensions := ['region', 'balance_date']
    ) ORDER BY region, balance_date
""").fetchall():
    print(f"  region={row[0]}, date={row[1]}, deposits={row[2]}")

# Facts + metrics mutual exclusion
print("\nFacts + metrics mutual exclusion:")
try:
    con.execute("""
        SELECT * FROM semantic_view('banking',
            facts := ['total_deposits'],
            metrics := ['latest_balance']
        )
    """)
except Exception as e:
    print(f"  Error: {e}")

# ============================================================
# Section 7: Semi-additive metrics (NON ADDITIVE BY)
# ============================================================

print("\n=== Section 7: Semi-additive metrics ===")

# latest_balance is NON ADDITIVE BY (balance_date DESC)
# When grouped by region, it takes the most recent balance per account,
# then sums across accounts within each region
print("\nLatest balance by region (snapshot metric):")
for row in con.execute("""
    SELECT * FROM semantic_view('banking',
        dimensions := ['region'],
        metrics := ['latest_balance']
    ) ORDER BY region
""").fetchall():
    print(f"  {row[0]}: latest_balance={row[1]}")

# Mixed: regular + semi-additive in same query
print("\nMixed metrics (regular + semi-additive):")
for row in con.execute("""
    SELECT * FROM semantic_view('banking',
        dimensions := ['region'],
        metrics := ['total_deposits', 'latest_balance']
    ) ORDER BY region
""").fetchall():
    print(f"  {row[0]}: total_deposits={row[1]}, latest_balance={row[2]}")

# ============================================================
# Section 8: Window function metrics (PARTITION BY EXCLUDING)
# ============================================================

print("\n=== Section 8: Window function metrics ===")

# daily_deposit_share: PARTITION BY EXCLUDING (region)
# When querying with dimensions [region, balance_date],
# the window partitions by balance_date only (region excluded)
print("\nDeposit share by region and date (window metric):")
for row in con.execute("""
    SELECT * FROM semantic_view('banking',
        dimensions := ['region', 'balance_date'],
        metrics := ['daily_deposit_share']
    ) ORDER BY balance_date, region
""").fetchall():
    print(f"  {row[0]}, {row[1]}: deposit_share={row[2]}")

# Window + aggregate mixing produces error
print("\nWindow + aggregate mixing:")
try:
    con.execute("""
        SELECT * FROM semantic_view('banking',
            dimensions := ['region'],
            metrics := ['total_deposits', 'daily_deposit_share']
        )
    """)
except Exception as e:
    print(f"  Error: {e}")

# ============================================================
# Section 9: DESCRIBE -- Metadata properties visible
# ============================================================

print("\n=== Section 9: DESCRIBE -- Metadata in output ===")

rows = con.execute("DESCRIBE SEMANTIC VIEW banking").fetchall()
# Show metadata-related properties
for row in rows:
    if row[3] in ('comment', 'synonyms', 'access_modifier'):
        print(f"  {row[0]} {row[1]}: {row[3]} = {row[4]}")

print("\n=== Done ===")
