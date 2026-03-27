.. meta::
   :description: Syntax reference for SHOW SEMANTIC METRICS, which lists both base and derived metrics across one or all semantic views with optional filtering

.. _ref-show-semantic-metrics:

========================
SHOW SEMANTIC METRICS
========================

Lists metrics registered in one or all semantic views. Each row describes a single metric with its name, aggregate expression, source table, and inferred data type. Both base metrics and derived metrics are included.


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

Returns one row per metric with 5 columns:

.. list-table::
   :header-rows: 1
   :widths: 22 12 66

   * - Column
     - Type
     - Description
   * - ``semantic_view_name``
     - VARCHAR
     - The semantic view this metric belongs to.
   * - ``name``
     - VARCHAR
     - The metric name as declared in the ``METRICS`` clause.
   * - ``expr``
     - VARCHAR
     - The SQL expression defining the metric (e.g., ``SUM(o.amount)`` for base metrics, ``revenue - cost`` for derived metrics).
   * - ``source_table``
     - VARCHAR
     - The table alias the metric is scoped to. Empty string for derived metrics (which have no table alias).
   * - ``data_type``
     - VARCHAR
     - The inferred data type of the metric. Empty string if the type has not been resolved.


.. _ref-show-metrics-examples:

Examples
========

**List metrics for a single view:**

Given a semantic view ``orders_sv`` with two base metrics:

.. code-block:: sql

   SHOW SEMANTIC METRICS IN orders_sv;

.. code-block:: text

   ┌──────────────────────┬──────────────┬────────────────┬──────────────┬───────────┐
   │ semantic_view_name   │ name         │ expr           │ source_table │ data_type │
   ├──────────────────────┼──────────────┼────────────────┼──────────────┼───────────┤
   │ orders_sv            │ order_count  │ COUNT(o.id)    │ o            │           │
   │ orders_sv            │ total_amount │ SUM(o.amount)  │ o            │           │
   └──────────────────────┴──────────────┴────────────────┴──────────────┴───────────┘

**List metrics across all views:**

.. code-block:: sql

   SHOW SEMANTIC METRICS;

.. code-block:: text

   ┌──────────────────────┬──────────────┬────────────────┬──────────────┬───────────┐
   │ semantic_view_name   │ name         │ expr           │ source_table │ data_type │
   ├──────────────────────┼──────────────┼────────────────┼──────────────┼───────────┤
   │ orders_sv            │ order_count  │ COUNT(o.id)    │ o            │           │
   │ orders_sv            │ total_amount │ SUM(o.amount)  │ o            │           │
   │ products_sv          │ avg_price    │ AVG(p.price)   │ p            │           │
   └──────────────────────┴──────────────┴────────────────┴──────────────┴───────────┘

Results are sorted by ``semantic_view_name`` then ``name``.

**Filter by pattern with LIKE (case-insensitive):**

Find all metrics whose name contains "amount":

.. code-block:: sql

   SHOW SEMANTIC METRICS LIKE '%amount%';

.. code-block:: text

   ┌──────────────────────┬──────────────┬───────────────┬──────────────┬───────────┐
   │ semantic_view_name   │ name         │ expr          │ source_table │ data_type │
   ├──────────────────────┼──────────────┼───────────────┼──────────────┼───────────┤
   │ orders_sv            │ total_amount │ SUM(o.amount) │ o            │           │
   └──────────────────────┴──────────────┴───────────────┴──────────────┴───────────┘

Because ``LIKE`` is case-insensitive, ``LIKE '%AMOUNT%'`` produces the same results.

**Filter by prefix with STARTS WITH (case-sensitive):**

Find metrics whose name starts with "order":

.. code-block:: sql

   SHOW SEMANTIC METRICS STARTS WITH 'order';

.. code-block:: text

   ┌──────────────────────┬─────────────┬─────────────┬──────────────┬───────────┐
   │ semantic_view_name   │ name        │ expr        │ source_table │ data_type │
   ├──────────────────────┼─────────────┼─────────────┼──────────────┼───────────┤
   │ orders_sv            │ order_count │ COUNT(o.id) │ o            │           │
   └──────────────────────┴─────────────┴─────────────┴──────────────┴───────────┘

``STARTS WITH`` is case-sensitive. ``STARTS WITH 'Order'`` would return no results because the metric is named ``order_count`` (lowercase).

**Limit the number of results:**

.. code-block:: sql

   SHOW SEMANTIC METRICS IN orders_sv LIMIT 1;

.. code-block:: text

   ┌──────────────────────┬─────────────┬─────────────┬──────────────┬───────────┐
   │ semantic_view_name   │ name        │ expr        │ source_table │ data_type │
   ├──────────────────────┼─────────────┼─────────────┼──────────────┼───────────┤
   │ orders_sv            │ order_count │ COUNT(o.id) │ o            │           │
   └──────────────────────┴─────────────┴─────────────┴──────────────┴───────────┘

**Derived metrics appear with an empty source_table:**

For a view with derived metrics:

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

   ┌──────────────────────┬─────────┬─────────────────────────┬──────────────┬───────────┐
   │ semantic_view_name   │ name    │ expr                    │ source_table │ data_type │
   ├──────────────────────┼─────────┼─────────────────────────┼──────────────┼───────────┤
   │ profit_analysis      │ cost    │ SUM(li.unit_cost)       │ li           │           │
   │ profit_analysis      │ margin  │ profit / revenue * 100  │              │           │
   │ profit_analysis      │ profit  │ revenue - cost          │              │           │
   │ profit_analysis      │ revenue │ SUM(li.extended_price)  │ li           │           │
   └──────────────────────┴─────────┴─────────────────────────┴──────────────┴───────────┘

Base metrics (``revenue``, ``cost``) show their source table alias. Derived metrics (``profit``, ``margin``) show an empty ``source_table`` because they reference other metrics rather than a specific table.

**Filter derived and base metrics with LIKE:**

.. code-block:: sql

   SHOW SEMANTIC METRICS LIKE '%pro%' IN profit_analysis;

This returns both ``profit`` (derived) and ``product``-related metrics whose name contains "pro".

**Error: view does not exist:**

.. code-block:: sql

   SHOW SEMANTIC METRICS IN nonexistent_view;

.. code-block:: text

   Error: semantic view 'nonexistent_view' does not exist
