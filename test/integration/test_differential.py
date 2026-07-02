#!/usr/bin/env python3
# /// script
# dependencies = ["duckdb==1.5.4"]
# requires-python = ">=3.10"
# ///
"""
Differential-testing harness (TC-2, code-review 2026-07-02).

Compares `semantic_view()` results against independently hand-written
SQL over a seeded, randomized star schema — thousands of rows with NULL
foreign keys, NULL measures, and skewed group sizes. Every bounded
combination of requested dimensions and metrics is executed through both
paths and the result multisets must match exactly (floats within 1e-9
relative tolerance).

This harness is the acceptance net for the Phase R1 expansion-correctness
work: SQL-string shape assertions can't prove the generated SQL computes
the right numbers; this does.

SCOPE (deliberate): the schema and requests stay inside the engine's
*supported core* — all metrics live on the base (fact) table, dimension
tables hang off it via ManyToOne PK relationships, one grain. Scenarios
the 2026-07-02 review identified as broken (multi-grain metric×metric
fan-out SG-1, COUNT(*) on child tables SG-8, semi-additive ties SG-4, …)
are intentionally NOT generated here yet — each R1 fix PR must extend
this harness with the scenario it repairs, using this file's helpers.

Extensions so far:
  - SG-3 (single-pass derived-metric inlining): `net_revenue` is derived
    from metrics whose expressions contain a column (`o.amount`) named
    like another metric (`amount`) — the exact re-scan poison scenario.
  - SG-2 (declaration-order-independent join emission): the `cat_name`
    dimension is two joins away (orders -> products -> categories) with
    the p->cat relationship declared FIRST — pre-fix this emitted a
    forward-referencing join and dropped the o->p connecting join.

Usage:
    uv run test/integration/test_differential.py

Exit codes:
    0 = every combination matched
    1 = at least one mismatch (details printed)
"""

from __future__ import annotations

import itertools
import math
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_ducklake_helpers import get_ext_dir, get_extension_path

SEED = 20260702
N_CUSTOMERS = 50
N_PRODUCTS = 30
N_ORDERS = 4000

# Dimension name -> (semantic dim name, reference SQL expr, table needed)
# `cat_name` lives two joins away (orders -> products -> categories) and its
# relationship is DECLARED FIRST in the view DDL — the SG-2 poison ordering:
# pre-fix, join emission picked the first declared join mentioning an alias,
# emitting `products ON p.category_id = cat.id` as a forward reference and
# dropping the o->p connecting join entirely.
DIMS = {
    "region": ("region", "o.region", None),
    "tier": ("tier", "c.tier", "c"),
    "category": ("category", "p.category", "p"),
    "cat_name": ("cat_name", "cat.cat_name", "cat"),
}
N_CATEGORIES = 8

# Metric name -> reference SQL aggregate (all on the base table — see SCOPE).
# `amount` deliberately shares its name with the orders.amount COLUMN, and
# `net_revenue` is a derived metric whose resolution inlines `tax_total`
# (whose expression contains `o.amount`): the SG-3 poison scenario — pre-fix,
# derived-metric inlining could re-scan inserted text and corrupt this into
# invalid SQL on a hash-seed-dependent fraction of runs.
METRICS = {
    "revenue": "SUM(o.amount)",
    "order_count": "COUNT(*)",
    "avg_amount": "AVG(o.amount)",
    "max_qty": "MAX(o.qty)",
    "min_amount": "MIN(o.amount)",
    "amount": "SUM(o.amount)",
    "tax_total": "SUM(o.amount * 0.1)",
    "net_revenue": "SUM(o.amount) - SUM(o.amount * 0.1)",
}


def seed_schema(conn) -> None:
    rng = random.Random(SEED)
    conn.execute("CREATE TABLE customers (id INTEGER PRIMARY KEY, name VARCHAR, tier VARCHAR)")
    conn.execute("CREATE TABLE categories (id INTEGER PRIMARY KEY, cat_name VARCHAR)")
    conn.execute(
        "CREATE TABLE products (id INTEGER PRIMARY KEY, name VARCHAR, "
        "category VARCHAR, category_id INTEGER)"
    )
    conn.execute(
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, "
        "product_id INTEGER, region VARCHAR, amount DECIMAL(10,2), qty INTEGER)"
    )

    tiers = ["gold", "silver", "bronze", None]
    for i in range(N_CUSTOMERS):
        conn.execute(
            "INSERT INTO customers VALUES (?, ?, ?)",
            [i, f"cust_{i}", rng.choice(tiers)],
        )
    for i in range(N_CATEGORIES):
        conn.execute("INSERT INTO categories VALUES (?, ?)", [i, f"cat_{i}"])
    cats = ["alpha", "beta", "gamma"]
    for i in range(N_PRODUCTS):
        cat_id = None if rng.random() < 0.10 else rng.randrange(N_CATEGORIES)
        conn.execute(
            "INSERT INTO products VALUES (?, ?, ?, ?)",
            [i, f"prod_{i}", rng.choice(cats), cat_id],
        )

    regions = ["east", "west", "north", "south"]
    rows = []
    for i in range(N_ORDERS):
        cust = None if rng.random() < 0.10 else rng.randrange(N_CUSTOMERS)
        prod = None if rng.random() < 0.10 else rng.randrange(N_PRODUCTS)
        amount = None if rng.random() < 0.08 else round(rng.uniform(1, 500), 2)
        rows.append((i, cust, prod, rng.choice(regions), amount, rng.randrange(1, 20)))
    conn.executemany("INSERT INTO orders VALUES (?, ?, ?, ?, ?, ?)", rows)


CREATE_VIEW = """
CREATE SEMANTIC VIEW diff_sv AS
  TABLES (
    o AS orders PRIMARY KEY (id),
    c AS customers PRIMARY KEY (id),
    p AS products PRIMARY KEY (id),
    cat AS categories PRIMARY KEY (id)
  )
  RELATIONSHIPS (
    product_category AS p(category_id) REFERENCES cat,
    order_customer AS o(customer_id) REFERENCES c,
    order_product AS o(product_id) REFERENCES p
  )
  DIMENSIONS (
    o.region AS o.region,
    c.tier AS c.tier,
    p.category AS p.category,
    cat.cat_name AS cat.cat_name
  )
  METRICS (
    o.revenue AS SUM(o.amount),
    o.order_count AS COUNT(*),
    o.avg_amount AS AVG(o.amount),
    o.max_qty AS MAX(o.qty),
    o.min_amount AS MIN(o.amount),
    o.amount AS SUM(o.amount),
    o.tax_total AS SUM(o.amount * 0.1),
    net_revenue AS revenue - tax_total
  )
"""


def reference_sql(dims: list[str], mets: list[str]) -> str:
    """Independent hand-written equivalent of the semantic_view() request."""
    select_items = [DIMS[d][1] for d in dims] + [METRICS[m] for m in mets]
    joins = []
    needed = {DIMS[d][2] for d in dims} - {None}
    if "cat" in needed:
        needed.add("p")  # categories hangs off products
    if "c" in needed:
        joins.append("LEFT JOIN customers c ON o.customer_id = c.id")
    if "p" in needed:
        joins.append("LEFT JOIN products p ON o.product_id = p.id")
    if "cat" in needed:
        joins.append("LEFT JOIN categories cat ON p.category_id = cat.id")
    sql = f"SELECT {'DISTINCT ' if not mets else ''}{', '.join(select_items)}\n"
    sql += "FROM orders o\n" + "\n".join(joins)
    if dims and mets:
        sql += f"\nGROUP BY {', '.join(str(i + 1) for i in range(len(dims)))}"
    return sql


def _is_num(v) -> bool:
    from decimal import Decimal

    return isinstance(v, (int, float, Decimal)) and not isinstance(v, bool)


def normalize(rows: list[tuple]) -> list[tuple]:
    """Sort rows with None-safe, numeric-type-agnostic keys so both result
    sets compare positionally even if one side returns e.g. DECIMAL where
    the other returns DOUBLE for the same value."""

    def key(row: tuple):
        out = []
        for v in row:
            if v is None:
                out.append((2, ""))
            elif _is_num(v):
                out.append((0, float(v)))
            else:
                out.append((1, str(v)))
        return tuple(out)

    return sorted(rows, key=key)


def values_equal(a, b) -> bool:
    if a is None or b is None:
        return a is None and b is None
    if _is_num(a) and _is_num(b):
        return math.isclose(float(a), float(b), rel_tol=1e-9, abs_tol=1e-9)
    return a == b


def rows_equal(got: list[tuple], want: list[tuple]) -> bool:
    if len(got) != len(want):
        return False
    return all(
        len(g) == len(w) and all(values_equal(gv, wv) for gv, wv in zip(g, w))
        for g, w in zip(got, want)
    )


def run_harness() -> int:
    import duckdb

    ext_dir = get_ext_dir()
    ext_path = get_extension_path()
    if not ext_path.exists():
        print(f"ERROR: extension not found at {ext_path}")
        print("Run `just build` first.")
        return 1

    conn = duckdb.connect(
        config={
            "allow_unsigned_extensions": "true",
            "extension_directory": ext_dir,
        }
    )
    conn.execute(f"FORCE INSTALL '{ext_path}'")
    conn.execute("LOAD semantic_views")

    seed_schema(conn)
    conn.execute(CREATE_VIEW)

    dim_names = list(DIMS)
    met_names = list(METRICS)

    # Bounded request set: every dims subset (incl. empty) × (each single
    # metric, one representative pair, all metrics, and — for non-empty dims —
    # no metrics at all). ~70 requests.
    metric_choices: list[list[str]] = (
        [[m] for m in met_names] + [["revenue", "order_count"]] + [met_names]
    )
    dim_choices: list[list[str]] = [
        list(c) for r in range(len(dim_names) + 1) for c in itertools.combinations(dim_names, r)
    ]

    total = 0
    failures = 0
    for dims in dim_choices:
        for mets in metric_choices + ([[]] if dims else []):
            if not dims and not mets:
                continue
            total += 1
            dim_arg = ", ".join(f"'{d}'" for d in dims)
            met_arg = ", ".join(f"'{m}'" for m in mets)
            parts = []
            if dims:
                parts.append(f"dimensions := [{dim_arg}]")
            if mets:
                parts.append(f"metrics := [{met_arg}]")
            sv_sql = f"SELECT * FROM semantic_view('diff_sv', {', '.join(parts)})"
            ref_sql = reference_sql(dims, mets)

            try:
                got = normalize(conn.execute(sv_sql).fetchall())
                want = normalize(conn.execute(ref_sql).fetchall())
            except Exception as exc:  # noqa: BLE001 — report and continue
                failures += 1
                print(f"FAIL dims={dims} mets={mets}: query error: {exc}")
                continue

            if not rows_equal(got, want):
                failures += 1
                print(f"FAIL dims={dims} mets={mets}: result mismatch")
                print(f"  semantic_view rows={len(got)}, reference rows={len(want)}")
                for g, w in list(zip(got, want))[:3]:
                    if not (len(g) == len(w) and all(values_equal(a, b) for a, b in zip(g, w))):
                        print(f"  first differing row: got={g!r} want={w!r}")
                        break

    print()
    print(f"Ran {total} dims×metrics combinations over {N_ORDERS} orders "
          f"({N_CUSTOMERS} customers, {N_PRODUCTS} products, seed={SEED})")
    if failures:
        print(f"FAILED: {failures}/{total} combinations mismatched")
        return 1
    print("ALL PASSED — semantic_view() matches hand-written SQL on every combination")
    return 0


if __name__ == "__main__":
    sys.exit(run_harness())
