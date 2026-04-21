.. meta::
   :description: Syntax reference for SHOW SEMANTIC MATERIALIZATIONS, listing materialization declarations across one or all semantic views

.. _ref-show-semantic-materializations:

===============================
SHOW SEMANTIC MATERIALIZATIONS
===============================

Lists materialization declarations for a specific semantic view or across all registered semantic views.

.. versionadded:: 0.7.0


.. _ref-show-mat-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW SEMANTIC MATERIALIZATIONS [ LIKE '<pattern>' ] IN <view_name>

   SHOW SEMANTIC MATERIALIZATIONS [ LIKE '<pattern>' ]


.. _ref-show-mat-variants:

Statement Variants
==================

``SHOW SEMANTIC MATERIALIZATIONS IN <view_name>``
   Returns materializations declared in the specified semantic view. Returns an error if the view does not exist. Returns an empty result set if the view exists but has no materializations.

``SHOW SEMANTIC MATERIALIZATIONS``
   Returns materializations across all registered semantic views. Returns an empty result set if no views have materializations declared.

Both forms support an optional ``LIKE '<pattern>'`` clause before ``IN`` (or at the end for the cross-view form) to filter materializations by name.


.. _ref-show-mat-params:

Parameters
==========

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Parameter
     - Type
     - Description
   * - ``<view_name>``
     - Name (unquoted)
     - The name of the semantic view to list materializations for. Only used with the ``IN`` form.
   * - ``LIKE '<pattern>'``
     - VARCHAR (optional)
     - Filters materializations by name using SQL ``LIKE`` syntax (``%`` matches any sequence, ``_`` matches one character).


.. _ref-show-mat-output:

Output Columns
==============

Returns one row per materialization with 7 VARCHAR columns:

.. list-table::
   :header-rows: 1
   :widths: 25 12 63

   * - Column
     - Type
     - Description
   * - ``database_name``
     - VARCHAR
     - The DuckDB database containing the semantic view (e.g., ``memory``).
   * - ``schema_name``
     - VARCHAR
     - The DuckDB schema containing the semantic view (e.g., ``main``).
   * - ``semantic_view_name``
     - VARCHAR
     - The name of the semantic view this materialization belongs to.
   * - ``name``
     - VARCHAR
     - The materialization name as declared in the ``MATERIALIZATIONS`` clause.
   * - ``table``
     - VARCHAR
     - The physical table name that the materialization points to.
   * - ``dimensions``
     - VARCHAR
     - JSON array of dimension names covered by this materialization (e.g., ``["region"]``).
   * - ``metrics``
     - VARCHAR
     - JSON array of metric names covered by this materialization (e.g., ``["revenue","order_count"]``).


.. _ref-show-mat-sorting:

Sorting Behavior
================

**Single-view form (IN):** Results are sorted alphabetically by materialization ``name``.

**Cross-view form (no IN):** Results are sorted alphabetically by ``semantic_view_name`` first, then by materialization ``name`` within each view.


.. _ref-show-mat-examples:

Examples
========

**List materializations for a specific view:**

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.revenue     AS SUM(o.amount),
       o.order_count AS COUNT(*)
   )
   MATERIALIZATIONS (
       region_agg AS (
           TABLE daily_revenue_by_region,
           DIMENSIONS (region),
           METRICS (revenue, order_count)
       )
   );

   SHOW SEMANTIC MATERIALIZATIONS IN order_metrics;

.. code-block:: text

   ┌───────────────┬─────────────┬─────────────────────┬────────────┬──────────────────────────────┬────────────┬──────────────────────────────┐
   │ database_name │ schema_name │ semantic_view_name  │ name       │ table                        │ dimensions │ metrics                      │
   ├───────────────┼─────────────┼─────────────────────┼────────────┼──────────────────────────────┼────────────┼──────────────────────────────┤
   │ memory        │ main        │ order_metrics       │ region_agg │ daily_revenue_by_region      │ ["region"] │ ["revenue","order_count"]    │
   └───────────────┴─────────────┴─────────────────────┴────────────┴──────────────────────────────┴────────────┴──────────────────────────────┘

**List materializations across all views:**

.. code-block:: sql

   SHOW SEMANTIC MATERIALIZATIONS;

**Filter by name pattern:**

.. code-block:: sql

   SHOW SEMANTIC MATERIALIZATIONS LIKE 'region%' IN order_metrics;

**View with no materializations (empty result):**

.. code-block:: sql

   SHOW SEMANTIC MATERIALIZATIONS IN simple_view;

Returns an empty result set with the 7-column schema.


.. _ref-show-mat-errors:

Error Cases
===========

**View does not exist:**

.. code-block:: sql

   SHOW SEMANTIC MATERIALIZATIONS IN nonexistent_view;

.. code-block:: text

   Error: semantic view 'nonexistent_view' does not exist
