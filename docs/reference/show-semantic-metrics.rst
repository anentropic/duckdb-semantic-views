.. meta::
   :description: Syntax reference for SHOW SEMANTIC METRICS, which lists both base and derived metrics across one or all semantic views with optional filtering

.. _ref-show-semantic-metrics:

========================
SHOW SEMANTIC METRICS
========================

Lists metrics registered in one or all semantic views. Each row describes a single metric with its name, source table, and inferred data type. Both base metrics and derived metrics are included.


.. _ref-show-metrics-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW SEMANTIC METRICS
       [ LIKE '<pattern>' ]
       [ IN <name> ]
       [ STARTS WITH '<prefix>' ]
       [ LIMIT <rows> ]

All clauses are optional. When multiple clauses appear, they must follow the order shown above.


.. _ref-show-metrics-variants:

Statement Variants
==================

``SHOW SEMANTIC METRICS``
   Returns metrics across all registered semantic views, sorted by semantic view name and then metric name.

``SHOW SEMANTIC METRICS IN <name>``
   Returns metrics for the specified semantic view only, sorted by metric name. Returns an error if the view does not exist.


.. _ref-show-metrics-params:

Parameters
==========

``<name>``
   The name of the semantic view. Required only for the single-view form (``IN`` clause). Returns an error if the view does not exist.


.. _ref-show-metrics-filtering:

Optional Filtering Clauses
==========================

``LIKE '<pattern>'``
   Filters metrics to those whose name matches the pattern. Uses SQL ``LIKE`` pattern syntax: ``%`` matches any sequence of characters, ``_`` matches a single character. Matching is **case-insensitive** (the extension maps ``LIKE`` to DuckDB's ``ILIKE``). The pattern must be enclosed in single quotes.

``STARTS WITH '<prefix>'``
   Filters metrics to those whose name begins with the prefix. Matching is **case-sensitive**. The prefix must be enclosed in single quotes.

``LIMIT <rows>``
   Restricts the output to the first *rows* results. Must be a positive integer.

When ``LIKE`` and ``STARTS WITH`` are both present, a metric must satisfy both conditions (they are combined with ``AND``).

.. warning::

   Clause order is enforced. ``LIKE`` must come before ``IN``, and ``STARTS WITH`` must come after ``IN``. Placing clauses out of order produces a syntax error.


.. _ref-show-metrics-output:

Output Columns
==============

Returns one row per metric with 6 columns:

.. list-table::
   :header-rows: 1
   :widths: 22 12 66

   * - Column
     - Type
     - Description
   * - ``database_name``
     - VARCHAR
     - The DuckDB database containing the semantic view.
   * - ``schema_name``
     - VARCHAR
     - The DuckDB schema containing the semantic view.
   * - ``semantic_view_name``
     - VARCHAR
     - The semantic view this metric belongs to.
   * - ``table_name``
     - VARCHAR
     - The physical table name the metric is scoped to. Empty string for derived metrics (which reference other metrics rather than a specific table).
   * - ``name``
     - VARCHAR
     - The metric name as declared in the ``METRICS`` clause.
   * - ``data_type``
     - VARCHAR
     - Reserved for future use. Currently always an empty string for metrics.


.. _ref-show-metrics-examples:

Examples
========

**List metrics for a single view:**

Given a semantic view ``orders_sv`` with two base metrics:

.. code-block:: sql

   SHOW SEMANTIC METRICS IN orders_sv;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬──────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name         │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼──────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ orders     │ order_count  │           │
   │ memory        │ main        │ orders_sv            │ orders     │ total_amount │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴──────────────┴───────────┘

**List metrics across all views:**

.. code-block:: sql

   SHOW SEMANTIC METRICS;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬──────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name         │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼──────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ orders     │ order_count  │           │
   │ memory        │ main        │ orders_sv            │ orders     │ total_amount │           │
   │ memory        │ main        │ products_sv          │ products   │ avg_price    │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴──────────────┴───────────┘

Results are sorted by ``semantic_view_name`` then ``name``.

**Filter by pattern with LIKE (case-insensitive):**

Find all metrics whose name contains "amount":

.. code-block:: sql

   SHOW SEMANTIC METRICS LIKE '%amount%';

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬──────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name         │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼──────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ orders     │ total_amount │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴──────────────┴───────────┘

Because ``LIKE`` is case-insensitive, ``LIKE '%AMOUNT%'`` produces the same results.

**Filter by prefix with STARTS WITH (case-sensitive):**

Find metrics whose name starts with "order":

.. code-block:: sql

   SHOW SEMANTIC METRICS STARTS WITH 'order';

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬─────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name        │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼─────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ orders     │ order_count │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴─────────────┴───────────┘

``STARTS WITH`` is case-sensitive. ``STARTS WITH 'Order'`` would return no results because the metric is named ``order_count`` (lowercase).

**Limit the number of results:**

.. code-block:: sql

   SHOW SEMANTIC METRICS IN orders_sv LIMIT 1;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬─────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name        │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼─────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ orders     │ order_count │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴─────────────┴───────────┘

**Derived metrics appear with an empty table_name:**

Derived metrics reference other metrics rather than a specific physical table. They are distinguished from base metrics by their empty ``table_name``:

.. code-block:: sql

   CREATE SEMANTIC VIEW profit_analysis AS
   TABLES (
       li AS line_items PRIMARY KEY (id)
   )
   DIMENSIONS (
       li.product AS li.product
   )
   METRICS (
       li.revenue AS SUM(li.extended_price),
       li.cost    AS SUM(li.unit_cost),
       profit     AS revenue - cost,
       margin     AS profit / revenue * 100
   );

   SHOW SEMANTIC METRICS IN profit_analysis;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬─────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name    │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼─────────┼───────────┤
   │ memory        │ main        │ profit_analysis      │ line_items │ cost    │           │
   │ memory        │ main        │ profit_analysis      │            │ margin  │           │
   │ memory        │ main        │ profit_analysis      │            │ profit  │           │
   │ memory        │ main        │ profit_analysis      │ line_items │ revenue │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴─────────┴───────────┘

Base metrics (``revenue``, ``cost``) show their physical table name. Derived metrics (``profit``, ``margin``) show an empty ``table_name`` because they reference other metrics rather than a specific table.

**Error: view does not exist:**

.. code-block:: sql

   SHOW SEMANTIC METRICS IN nonexistent_view;

.. code-block:: text

   Error: semantic view 'nonexistent_view' does not exist
