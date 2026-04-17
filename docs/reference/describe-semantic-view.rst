.. meta::
   :description: Syntax reference for DESCRIBE SEMANTIC VIEW, which returns the full definition of a view in a property-per-row format showing each object and its properties

.. _ref-describe-semantic-view:

========================
DESCRIBE SEMANTIC VIEW
========================

Returns the definition of a semantic view as a multi-row result set in property-per-row format. Each row represents one property of one object (semantic view, table, relationship, fact, dimension, metric, or derived metric) in the view definition.


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

The result contains multiple rows with 5 VARCHAR columns:

.. list-table::
   :header-rows: 1
   :widths: 18 10 72

   * - Column
     - Type
     - Description
   * - ``object_kind``
     - VARCHAR
     - The type of object: ``SEMANTIC_VIEW``, ``TABLE``, ``RELATIONSHIP``, ``FACT``, ``DIMENSION``, ``METRIC``, or ``DERIVED_METRIC``.
   * - ``object_name``
     - VARCHAR
     - The name of the object (view name, table name, relationship name, fact/dimension/metric name).
   * - ``parent_entity``
     - VARCHAR
     - The parent table for this object. Empty string for ``SEMANTIC_VIEW``, ``TABLE``, and ``DERIVED_METRIC`` objects.
   * - ``property``
     - VARCHAR
     - The property name being described.
   * - ``property_value``
     - VARCHAR
     - The property value.


.. _ref-describe-object-kinds:

Object Kinds and Properties
===========================

Rows appear in definition order: ``SEMANTIC_VIEW`` (when comment is set), then ``TABLE`` objects, then ``RELATIONSHIP``, ``FACT``, ``DIMENSION``, ``METRIC``, and ``DERIVED_METRIC``.

**SEMANTIC_VIEW**
   Emitted only when a view-level comment is set (via :ref:`ALTER SEMANTIC VIEW SET COMMENT <ref-alter-semantic-view>`). Produces one property row:

   .. list-table::
      :header-rows: 1
      :widths: 25 75

      * - Property
        - Description
      * - ``COMMENT``
        - The view-level comment text.

**TABLE**
   One block per table declared in the ``TABLES`` clause. Each table produces 3-6 property rows:

   .. list-table::
      :header-rows: 1
      :widths: 35 65

      * - Property
        - Description
      * - ``BASE_TABLE_DATABASE_NAME``
        - The DuckDB database containing the physical table (e.g., ``memory``).
      * - ``BASE_TABLE_SCHEMA_NAME``
        - The DuckDB schema containing the physical table (e.g., ``main``).
      * - ``BASE_TABLE_NAME``
        - The physical table name.
      * - ``PRIMARY_KEY``
        - JSON array of primary key column names (e.g., ``["id"]``). Only emitted when a primary key is declared.
      * - ``COMMENT``
        - The table comment text. Only emitted when a comment is set.
      * - ``SYNONYMS``
        - JSON array of synonym strings (e.g., ``["transactions","purchases"]``). Only emitted when synonyms are set.

**RELATIONSHIP**
   One block per relationship declared in the ``RELATIONSHIPS`` clause:

   .. list-table::
      :header-rows: 1
      :widths: 25 75

      * - Property
        - Description
      * - ``TABLE``
        - The physical table name on the foreign key side.
      * - ``REF_TABLE``
        - The physical table name on the referenced (primary key) side.
      * - ``FOREIGN_KEY``
        - JSON array of foreign key column names (e.g., ``["customer_id"]``).
      * - ``REF_KEY``
        - JSON array of referenced key column names (e.g., ``["id"]``).

**FACT**
   One block per fact declared in the ``FACTS`` clause:

   .. list-table::
      :header-rows: 1
      :widths: 25 75

      * - Property
        - Description
      * - ``TABLE``
        - The physical table name the fact is scoped to.
      * - ``EXPRESSION``
        - The row-level SQL expression defining the fact.
      * - ``DATA_TYPE``
        - The inferred data type. Empty string if not resolved. Populated when the table contains data.
      * - ``COMMENT``
        - The fact comment text. Only emitted when a comment is set.
      * - ``SYNONYMS``
        - JSON array of synonym strings. Only emitted when synonyms are set.
      * - ``ACCESS_MODIFIER``
        - ``PUBLIC`` or ``PRIVATE``. Always emitted.

**DIMENSION**
   One block per dimension declared in the ``DIMENSIONS`` clause:

   .. list-table::
      :header-rows: 1
      :widths: 25 75

      * - Property
        - Description
      * - ``TABLE``
        - The physical table name the dimension is scoped to.
      * - ``EXPRESSION``
        - The SQL expression defining the dimension.
      * - ``DATA_TYPE``
        - The inferred data type. Empty string if not resolved.
      * - ``COMMENT``
        - The dimension comment text. Only emitted when a comment is set.
      * - ``SYNONYMS``
        - JSON array of synonym strings. Only emitted when synonyms are set.

**METRIC**
   One block per base metric (those scoped to a table) declared in the ``METRICS`` clause:

   .. list-table::
      :header-rows: 1
      :widths: 25 75

      * - Property
        - Description
      * - ``TABLE``
        - The physical table name the metric is scoped to.
      * - ``EXPRESSION``
        - The aggregate SQL expression defining the metric.
      * - ``DATA_TYPE``
        - The inferred data type. Empty string if not resolved.
      * - ``COMMENT``
        - The metric comment text. Only emitted when a comment is set.
      * - ``SYNONYMS``
        - JSON array of synonym strings. Only emitted when synonyms are set.
      * - ``ACCESS_MODIFIER``
        - ``PUBLIC`` or ``PRIVATE``. Always emitted.
      * - ``NON_ADDITIVE_BY``
        - Comma-separated list of non-additive dimensions with optional sort/nulls (e.g., ``report_date DESC NULLS FIRST``). Only emitted for semi-additive metrics.
      * - ``WINDOW_SPEC``
        - Reconstructed OVER clause string (e.g., ``AVG(total_qty) OVER (PARTITION BY EXCLUDING date ORDER BY date)``). Only emitted for window metrics.

**DERIVED_METRIC**
   One block per derived metric (those referencing other metrics rather than a table). Derived metrics have an empty ``parent_entity``:

   .. list-table::
      :header-rows: 1
      :widths: 25 75

      * - Property
        - Description
      * - ``EXPRESSION``
        - The expression composing other metrics.
      * - ``DATA_TYPE``
        - The inferred data type. Empty string if not resolved.
      * - ``COMMENT``
        - The derived metric comment text. Only emitted when a comment is set.
      * - ``SYNONYMS``
        - JSON array of synonym strings. Only emitted when synonyms are set.
      * - ``ACCESS_MODIFIER``
        - ``PUBLIC`` or ``PRIVATE``. Always emitted.
      * - ``NON_ADDITIVE_BY``
        - Only emitted for semi-additive derived metrics.
      * - ``WINDOW_SPEC``
        - Only emitted for window-function derived metrics.


.. _ref-describe-examples:

Examples
========

**Simple single-table view:**

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.total AS SUM(o.amount)
   );

   DESCRIBE SEMANTIC VIEW order_metrics;

.. code-block:: text

   ┌─────────────┬─────────────┬───────────────┬──────────────────────────┬──────────────────┐
   │ object_kind │ object_name │ parent_entity │ property                 │ property_value   │
   ├─────────────┼─────────────┼───────────────┼──────────────────────────┼──────────────────┤
   │ TABLE       │ orders      │               │ BASE_TABLE_DATABASE_NAME │ memory           │
   │ TABLE       │ orders      │               │ BASE_TABLE_SCHEMA_NAME   │ main             │
   │ TABLE       │ orders      │               │ BASE_TABLE_NAME          │ orders           │
   │ TABLE       │ orders      │               │ PRIMARY_KEY              │ ["id"]           │
   │ DIMENSION   │ region      │ orders        │ TABLE                    │ orders           │
   │ DIMENSION   │ region      │ orders        │ EXPRESSION               │ o.region         │
   │ DIMENSION   │ region      │ orders        │ DATA_TYPE                │                  │
   │ METRIC      │ total       │ orders        │ TABLE                    │ orders           │
   │ METRIC      │ total       │ orders        │ EXPRESSION               │ SUM(o.amount)    │
   │ METRIC      │ total       │ orders        │ DATA_TYPE                │                  │
   │ METRIC      │ total       │ orders        │ ACCESS_MODIFIER          │ PUBLIC           │
   └─────────────┴─────────────┴───────────────┴──────────────────────────┴──────────────────┘

**View with metadata annotations:**

.. code-block:: sql

   CREATE SEMANTIC VIEW annotated AS
   TABLES (
       o AS orders PRIMARY KEY (id) COMMENT = 'Order data'
   )
   DIMENSIONS (
       o.region AS o.region COMMENT = 'Sales region' WITH SYNONYMS = ('territory')
   )
   METRICS (
       o.revenue AS SUM(o.amount) COMMENT = 'Total revenue'
   );

   ALTER SEMANTIC VIEW annotated SET COMMENT = 'Revenue analytics';

   DESCRIBE SEMANTIC VIEW annotated;

.. code-block:: text

   ┌───────────────┬─────────────┬───────────────┬──────────────────────────┬──────────────────────┐
   │ object_kind   │ object_name │ parent_entity │ property                 │ property_value       │
   ├───────────────┼─────────────┼───────────────┼──────────────────────────┼──────────────────────┤
   │ SEMANTIC_VIEW │ annotated   │               │ COMMENT                  │ Revenue analytics    │
   │ TABLE         │ orders      │               │ BASE_TABLE_DATABASE_NAME │ memory               │
   │ TABLE         │ orders      │               │ BASE_TABLE_SCHEMA_NAME   │ main                 │
   │ TABLE         │ orders      │               │ BASE_TABLE_NAME          │ orders               │
   │ TABLE         │ orders      │               │ PRIMARY_KEY              │ ["id"]               │
   │ TABLE         │ orders      │               │ COMMENT                  │ Order data           │
   │ DIMENSION     │ region      │ orders        │ TABLE                    │ orders               │
   │ DIMENSION     │ region      │ orders        │ EXPRESSION               │ o.region             │
   │ DIMENSION     │ region      │ orders        │ DATA_TYPE                │                      │
   │ DIMENSION     │ region      │ orders        │ COMMENT                  │ Sales region         │
   │ DIMENSION     │ region      │ orders        │ SYNONYMS                 │ ["territory"]        │
   │ METRIC        │ revenue     │ orders        │ TABLE                    │ orders               │
   │ METRIC        │ revenue     │ orders        │ EXPRESSION               │ SUM(o.amount)        │
   │ METRIC        │ revenue     │ orders        │ DATA_TYPE                │                      │
   │ METRIC        │ revenue     │ orders        │ COMMENT                  │ Total revenue        │
   │ METRIC        │ revenue     │ orders        │ ACCESS_MODIFIER          │ PUBLIC               │
   └───────────────┴─────────────┴───────────────┴──────────────────────────┴──────────────────────┘

.. tip::

   Filter by ``object_kind`` to extract specific parts of the view definition:

   .. code-block:: sql

      -- All dimensions in the view:
      SELECT object_name, property, property_value
      FROM (DESCRIBE SEMANTIC VIEW order_metrics)
      WHERE object_kind = 'DIMENSION';

      -- All relationships:
      SELECT object_name, property, property_value
      FROM (DESCRIBE SEMANTIC VIEW multi_view)
      WHERE object_kind = 'RELATIONSHIP';

      -- Count objects by kind:
      SELECT object_kind, COUNT(DISTINCT object_name) AS object_count
      FROM (DESCRIBE SEMANTIC VIEW multi_view)
      GROUP BY object_kind;

**Error: view does not exist:**

.. code-block:: sql

   DESCRIBE SEMANTIC VIEW nonexistent_view;

.. code-block:: text

   Error: semantic view 'nonexistent_view' does not exist

**The statement is case-insensitive:**

.. code-block:: sql

   describe semantic view order_metrics;
