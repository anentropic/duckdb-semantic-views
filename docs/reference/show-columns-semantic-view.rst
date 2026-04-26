.. meta::
   :description: Syntax reference for SHOW COLUMNS IN SEMANTIC VIEW, which lists all queryable columns (dimensions, facts, metrics) with their types, expressions, and comments

.. _ref-show-columns:

=================================
SHOW COLUMNS IN SEMANTIC VIEW
=================================

Lists all queryable columns in a semantic view -- dimensions, facts, and metrics -- with their data types, expressions, kind, and comments. Private items are excluded from the output.


.. _ref-show-columns-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW COLUMNS IN SEMANTIC VIEW <name>


.. _ref-show-columns-params:

Parameters
==========

``<name>``
   The name of the semantic view. Returns an error if the view does not exist.


.. _ref-show-columns-output:

Output Columns
==============

Returns one row per queryable column with 8 columns:

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
     - The semantic view name.
   * - ``column_name``
     - VARCHAR
     - The dimension, fact, or metric name.
   * - ``data_type``
     - VARCHAR
     - The inferred data type. Empty string if not resolved.
   * - ``kind``
     - VARCHAR
     - The column kind: ``DIMENSION``, ``FACT``, ``METRIC``, or ``DERIVED_METRIC``.
   * - ``expression``
     - VARCHAR
     - The SQL expression defining the column.
   * - ``comment``
     - VARCHAR
     - The comment text. Empty string if no comment is set.


.. _ref-show-columns-kind:

Kind Values
===========

.. list-table::
   :header-rows: 1
   :widths: 25 75

   * - Kind
     - Description
   * - ``DIMENSION``
     - A grouping expression from the ``DIMENSIONS`` clause.
   * - ``FACT``
     - A row-level expression from the ``FACTS`` clause.
   * - ``METRIC``
     - A base metric (scoped to a table) from the ``METRICS`` clause.
   * - ``DERIVED_METRIC``
     - A derived metric (no table alias, references other metrics) from the ``METRICS`` clause.


.. _ref-show-columns-private:

PRIVATE Exclusion
=================

Metrics and facts marked ``PRIVATE`` are excluded from the output. Only ``PUBLIC`` items (the default) appear. This matches the behavior of wildcard expansion in :ref:`semantic_view() <ref-semantic-view-function>` queries.


.. _ref-show-columns-sort:

Sort Order
==========

Rows are sorted by ``kind`` (alphabetically: ``DERIVED_METRIC``, ``DIMENSION``, ``FACT``, ``METRIC``) and then by ``column_name`` within each kind.


.. _ref-show-columns-examples:

Examples
========

.. code-block:: sql

   CREATE SEMANTIC VIEW shop AS
   TABLES (o AS orders PRIMARY KEY (id))
   FACTS (o.raw_amount AS o.quantity * o.price COMMENT = 'Line total')
   DIMENSIONS (o.region AS o.region)
   METRICS (
       o.revenue AS SUM(o.quantity * o.price),
       avg_order AS revenue / COUNT(*)
   );

   SHOW COLUMNS IN SEMANTIC VIEW shop;

.. code-block:: text

   ┌───────────────┬─────────────┬────────────────────┬─────────────┬───────────┬────────────────┬───────────────────────────┬────────────┐
   │ database_name │ schema_name │ semantic_view_name │ column_name │ data_type │ kind           │ expression                │ comment    │
   ├───────────────┼─────────────┼────────────────────┼─────────────┼───────────┼────────────────┼───────────────────────────┼────────────┤
   │ memory        │ main        │ shop               │ avg_order   │ DOUBLE    │ DERIVED_METRIC │ revenue / COUNT(*)        │            │
   │ memory        │ main        │ shop               │ region      │ VARCHAR   │ DIMENSION      │ o.region                  │            │
   │ memory        │ main        │ shop               │ raw_amount  │ DOUBLE    │ FACT           │ o.quantity * o.price      │ Line total │
   │ memory        │ main        │ shop               │ revenue     │ BIGINT    │ METRIC         │ SUM(o.quantity * o.price) │            │
   └───────────────┴─────────────┴────────────────────┴─────────────┴───────────┴────────────────┴───────────────────────────┴────────────┘

**Error: view does not exist:**

.. code-block:: sql

   SHOW COLUMNS IN SEMANTIC VIEW nonexistent;

.. code-block:: text

   Error: Semantic view 'nonexistent' not found
