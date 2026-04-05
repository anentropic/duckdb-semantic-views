.. meta::
   :description: Syntax reference for SHOW SEMANTIC DIMENSIONS IN ... FOR METRIC, which returns only the dimensions that can be safely combined with a given metric

.. _ref-show-dims-for-metric:

=========================================
SHOW SEMANTIC DIMENSIONS FOR METRIC
=========================================

Returns only the dimensions that can be safely combined with a specific metric, filtering out dimensions that would cause a fan trap. In single-table views, all dimensions are returned. In multi-table views, the extension analyzes the relationship graph and excludes dimensions whose join path to the metric's source table would traverse a one-to-many edge, which would duplicate rows and inflate aggregation results.

For background on what fan traps are and how to resolve them, see :ref:`How to Understand and Avoid Fan Traps <howto-fan-traps>`.


.. _ref-show-dims-for-metric-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW SEMANTIC DIMENSIONS
       [ LIKE '<pattern>' ]
       IN <name>
       FOR METRIC <metric_name>
       [ STARTS WITH '<prefix>' ]
       [ LIMIT <rows> ]

The ``IN`` and ``FOR METRIC`` clauses are required. The remaining clauses are optional. When multiple clauses appear, they must follow the order shown above.


.. _ref-show-dims-for-metric-params:

Parameters
==========

``<name>``
   The name of the semantic view. Returns an error if the view does not exist.

``<metric_name>``
   The name of a metric defined in the semantic view. Returns an error if the metric does not exist. Matching is case-insensitive.

.. tip::

   Both the view name and metric name support fuzzy matching in error messages. If a name is close to an existing name, the error suggests the closest match: ``metric 'totl' not found in semantic view 'orders_sv'. Did you mean 'total'?``


.. _ref-show-dims-for-metric-filtering-clauses:

Optional Filtering Clauses
===========================

``LIKE '<pattern>'``
   Filters the returned dimensions to those whose name matches the pattern. Uses SQL ``LIKE`` pattern syntax: ``%`` matches any sequence of characters, ``_`` matches a single character. Matching is **case-insensitive** (the extension maps ``LIKE`` to DuckDB's ``ILIKE``). The pattern must be enclosed in single quotes. ``LIKE`` must appear before ``IN``.

``STARTS WITH '<prefix>'``
   Filters the returned dimensions to those whose name begins with the prefix. Matching is **case-sensitive**. The prefix must be enclosed in single quotes. ``STARTS WITH`` must appear after ``FOR METRIC``.

``LIMIT <rows>``
   Restricts the output to the first *rows* results. Must be a positive integer. ``LIMIT`` must appear last.

When ``LIKE`` and ``STARTS WITH`` are both present, a dimension must satisfy both conditions (they are combined with ``AND``). These filtering clauses are applied after fan trap filtering, so only dimensions that are safe for the specified metric are candidates for name matching.

.. warning::

   Clause order is enforced. The full order is: ``LIKE``, ``IN``, ``FOR METRIC``, ``STARTS WITH``, ``LIMIT``. Placing clauses out of order produces a syntax error.


.. _ref-show-dims-for-metric-output:

Output Columns
==============

Returns one row per reachable dimension with 4 columns:

.. list-table::
   :header-rows: 1
   :widths: 18 12 70

   * - Column
     - Type
     - Description
   * - ``table_name``
     - VARCHAR
     - The physical table name the dimension is scoped to.
   * - ``name``
     - VARCHAR
     - The dimension name.
   * - ``data_type``
     - VARCHAR
     - The inferred data type of the dimension. Empty string if not resolved.
   * - ``required``
     - BOOLEAN
     - Always ``false``. Reserved for future Snowflake parity.


.. _ref-show-dims-for-metric-filtering:

Fan Trap Filtering
==================

The filtering logic determines whether a dimension is reachable from a metric's source table without traversing any edge in the fan-out direction (from the "one" side to the "many" side of a relationship). The rules are:

- **Same table:** If the dimension and metric share the same source table, the dimension is always included.
- **Many-to-one (forward):** Traversing from a table with a foreign key to the referenced table is always safe. Each row on the FK side maps to at most one row on the referenced side.
- **One-to-many (reverse):** Traversing from the referenced table back to the FK table is fan-out. This direction duplicates rows, inflating aggregates. Dimensions reachable only through such an edge are excluded.
- **One-to-one:** If the FK columns match a ``PRIMARY KEY`` or ``UNIQUE`` constraint on the FK table, the relationship is one-to-one. Both directions are safe.
- **Derived metrics:** For derived metrics (those without a table alias), the extension resolves all base metrics they depend on and uses the union of their source tables. A dimension is included if it is reachable from at least one of those source tables without fan-out.


.. _ref-show-dims-for-metric-examples:

Examples
========

**Single-table view (all dimensions reachable):**

.. code-block:: sql

   CREATE SEMANTIC VIEW simple_sales AS
   TABLES (
       s AS sales PRIMARY KEY (id)
   )
   DIMENSIONS (
       s.product AS s.product,
       s.region  AS s.region
   )
   METRICS (
       s.total AS SUM(s.amount)
   );

   SHOW SEMANTIC DIMENSIONS IN simple_sales FOR METRIC total;

.. code-block:: text

   ┌────────────┬─────────┬───────────┬──────────┐
   │ table_name │ name    │ data_type │ required │
   ├────────────┼─────────┼───────────┼──────────┤
   │ sales      │ product │           │ false    │
   │ sales      │ region  │           │ false    │
   └────────────┴─────────┴───────────┴──────────┘

With a single table, there are no joins, so every dimension is safe for every metric.

**Multi-table view with fan trap filtering:**

Consider a three-table schema: ``customers <- orders <- line_items``, where orders has a many-to-one FK to customers, and line_items has a many-to-one FK to orders.

.. code-block:: sql

   CREATE SEMANTIC VIEW star_sv AS
   TABLES (
       c  AS customers  PRIMARY KEY (id),
       o  AS orders     PRIMARY KEY (id),
       li AS line_items PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c,
       item_order     AS li(order_id)   REFERENCES o
   )
   DIMENSIONS (
       c.customer_name    AS c.name,
       c.customer_country AS c.country,
       li.item_qty        AS li.qty
   )
   METRICS (
       o.order_total    AS SUM(o.total),
       li.line_item_sum AS SUM(li.price * li.qty)
   );

For ``order_total`` (source table: ``orders``), the path to ``customers`` is many-to-one (safe), but the path to ``line_items`` is one-to-many (fan-out). The ``item_qty`` dimension from ``line_items`` is excluded:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN star_sv FOR METRIC order_total;

.. code-block:: text

   ┌────────────┬──────────────────┬───────────┬──────────┐
   │ table_name │ name             │ data_type │ required │
   ├────────────┼──────────────────┼───────────┼──────────┤
   │ customers  │ customer_country │           │ false    │
   │ customers  │ customer_name    │           │ false    │
   └────────────┴──────────────────┴───────────┴──────────┘

For ``line_item_sum`` (source table: ``line_items``), the path from ``line_items`` to ``orders`` to ``customers`` is all many-to-one (safe), and ``item_qty`` is on the same table. All three dimensions are included:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN star_sv FOR METRIC line_item_sum;

.. code-block:: text

   ┌────────────┬──────────────────┬───────────┬──────────┐
   │ table_name │ name             │ data_type │ required │
   ├────────────┼──────────────────┼───────────┼──────────┤
   │ customers  │ customer_country │           │ false    │
   │ customers  │ customer_name    │           │ false    │
   │ line_items │ item_qty         │           │ false    │
   └────────────┴──────────────────┴───────────┴──────────┘

**Filter safe dimensions with LIKE (case-insensitive):**

After fan trap filtering, narrow results further by name pattern:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS LIKE '%gio%' IN filter_sv FOR METRIC total_amount;

.. code-block:: text

   ┌────────────┬────────┬───────────┬──────────┐
   │ table_name │ name   │ data_type │ required │
   ├────────────┼────────┼───────────┼──────────┤
   │ customers  │ region │           │ false    │
   └────────────┴────────┴───────────┴──────────┘

**Filter safe dimensions with STARTS WITH (case-sensitive):**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN filter_sv FOR METRIC total_amount STARTS WITH 'customer';

.. code-block:: text

   ┌────────────┬───────────────┬───────────┬──────────┐
   │ table_name │ name          │ data_type │ required │
   ├────────────┼───────────────┼───────────┼──────────┤
   │ customers  │ customer_name │           │ false    │
   └────────────┴───────────────┴───────────┴──────────┘

**Limit safe dimensions:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN filter_sv FOR METRIC total_amount LIMIT 3;

.. code-block:: text

   ┌────────────┬───────────────┬───────────┬──────────┐
   │ table_name │ name          │ data_type │ required │
   ├────────────┼───────────────┼───────────┼──────────┤
   │ customers  │ customer_name │           │ false    │
   │ customers  │ region        │           │ false    │
   │ orders     │ order_date    │           │ false    │
   └────────────┴───────────────┴───────────┴──────────┘

**Derived metrics inherit source tables:**

.. code-block:: sql

   CREATE SEMANTIC VIEW derived_sv AS
   TABLES (
       c AS customers PRIMARY KEY (id),
       o AS orders    PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c
   )
   DIMENSIONS (
       c.customer_name AS c.name
   )
   METRICS (
       o.order_total AS SUM(o.total),
       double_total  AS order_total * 2
   );

   SHOW SEMANTIC DIMENSIONS IN derived_sv FOR METRIC double_total;

.. code-block:: text

   ┌────────────┬───────────────┬───────────┬──────────┐
   │ table_name │ name          │ data_type │ required │
   ├────────────┼───────────────┼───────────┼──────────┤
   │ customers  │ customer_name │           │ false    │
   └────────────┴───────────────┴───────────┴──────────┘

The derived metric ``double_total`` depends on ``order_total`` (source: ``orders``). The extension traces this dependency and applies the same reachability rules as if querying ``order_total`` directly.

**Error: metric not found:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN star_sv FOR METRIC nonexistent;

.. code-block:: text

   Error: metric 'nonexistent' not found in semantic view 'star_sv'

**Error: view does not exist:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN nonexistent_sv FOR METRIC total;

.. code-block:: text

   Error: semantic view 'nonexistent_sv' does not exist
