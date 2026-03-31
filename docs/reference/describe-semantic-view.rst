.. meta::
   :description: Syntax reference for DESCRIBE SEMANTIC VIEW, which returns the full definition of a view as a single-row JSON result

.. _ref-describe-semantic-view:

========================
DESCRIBE SEMANTIC VIEW
========================

Returns the definition of a semantic view as a single-row result set.


.. _ref-describe-syntax:

Syntax
======

.. code-block:: sqlgrammar

   DESCRIBE SEMANTIC VIEW <name>;


.. _ref-describe-params:

Parameters
==========

``<name>``
   The name of the semantic view to describe. Returns an error if the view does not exist.


.. _ref-describe-output:

Output Columns
==============

The result is a single row with 6 VARCHAR columns:

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Column
     - Type
     - Description
   * - ``name``
     - VARCHAR
     - The semantic view name.
   * - ``base_table``
     - VARCHAR
     - The physical table name of the base (first) table.
   * - ``dimensions``
     - VARCHAR
     - JSON array of dimension definitions. Each entry has ``name``, ``expr``, ``source_table``, and ``output_type`` fields.
   * - ``metrics``
     - VARCHAR
     - JSON array of metric definitions. Each entry has ``name``, ``expr``, ``source_table``, ``output_type``, and optionally ``using_relationships`` fields.
   * - ``joins``
     - VARCHAR
     - JSON array of relationship definitions. Each entry has ``table``, ``from_alias``, ``fk_columns``, ``name``, ``cardinality``, and optionally ``ref_columns`` fields.
   * - ``facts``
     - VARCHAR
     - JSON array of fact definitions. Each entry has ``name``, ``expr``, and ``source_table`` fields.


.. _ref-describe-examples:

Examples
========

.. code-block:: sql

   DESCRIBE SEMANTIC VIEW order_metrics;

.. code-block:: text

   ┌───────────────┬────────────┬─────────────────────────────────────┬─────────────────────┬───────┬───────┐
   │     name      │ base_table │             dimensions              │       metrics       │ joins │ facts │
   ├───────────────┼────────────┼─────────────────────────────────────┼─────────────────────┼───────┼───────┤
   │ order_metrics │ orders     │ [{"expr":"o.region","name":"region" │ [{"expr":"SUM(o.amo │ []    │ []    │
   │               │            │ ,"output_type":null,"source_table": │ unt)","name":"reven │       │       │
   │               │            │ "o"}]                               │ ue",...}]           │       │       │
   └───────────────┴────────────┴─────────────────────────────────────┴─────────────────────┴───────┴───────┘

.. tip::

   Parse the JSON columns in your application code or use DuckDB's JSON functions
   to extract specific fields:

   .. code-block:: sql

      SELECT
          name,
          json_extract(dimensions, '$[*].name') AS dimension_names,
          json_extract(metrics, '$[*].name') AS metric_names
      FROM (DESCRIBE SEMANTIC VIEW order_metrics);
