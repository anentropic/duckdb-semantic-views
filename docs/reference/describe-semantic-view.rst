.. meta::
   :description: Syntax reference for DESCRIBE SEMANTIC VIEW, which returns the full definition of a view in a property-per-row format showing each object and its properties

.. _ref-describe-semantic-view:

========================
DESCRIBE SEMANTIC VIEW
========================

Returns the definition of a semantic view as a multi-row result set in property-per-row format. Each row represents one property of one object (table, relationship, fact, dimension, metric, or derived metric) in the view definition.


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
     - The type of object: ``TABLE``, ``RELATIONSHIP``, ``FACT``, ``DIMENSION``, ``METRIC``, or ``DERIVED_METRIC``.
   * - ``object_name``
     - VARCHAR
     - The name of the object (table name, relationship name, fact/dimension/metric name).
   * - ``parent_entity``
     - VARCHAR
     - The parent table for this object. Empty string for ``TABLE`` objects and ``DERIVED_METRIC`` objects.
   * - ``property``
     - VARCHAR
     - The property name being described.
   * - ``property_value``
     - VARCHAR
     - The property value.


.. _ref-describe-object-kinds:

Object Kinds and Properties
===========================

Rows appear in definition order: ``TABLE`` objects first, then ``RELATIONSHIP``, ``FACT``, ``DIMENSION``, ``METRIC``, and ``DERIVED_METRIC``.

**TABLE**
   One block per table declared in the ``TABLES`` clause. Each table produces 3 or 4 property rows:

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

**DERIVED_METRIC**
   One block per derived metric (those referencing other metrics rather than a table). Derived metrics have an empty ``parent_entity`` and only 2 property rows:

   .. list-table::
      :header-rows: 1
      :widths: 25 75

      * - Property
        - Description
      * - ``EXPRESSION``
        - The expression composing other metrics.
      * - ``DATA_TYPE``
        - The inferred data type. Empty string if not resolved.


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
   └─────────────┴─────────────┴───────────────┴──────────────────────────┴──────────────────┘

**Multi-table view with relationships:**

.. code-block:: sql

   CREATE SEMANTIC VIEW multi_view AS
   TABLES (
       o AS orders    PRIMARY KEY (id),
       c AS customers PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_to_customer AS o(customer_id) REFERENCES c
   )
   DIMENSIONS (
       o.region        AS o.region,
       c.customer_name AS c.name
   )
   METRICS (
       o.total_revenue AS SUM(o.amount)
   );

   DESCRIBE SEMANTIC VIEW multi_view;

.. code-block:: text

   ┌──────────────┬───────────────────┬───────────────┬──────────────────────────┬──────────────────┐
   │ object_kind  │ object_name       │ parent_entity │ property                 │ property_value   │
   ├──────────────┼───────────────────┼───────────────┼──────────────────────────┼──────────────────┤
   │ TABLE        │ orders            │               │ BASE_TABLE_DATABASE_NAME │ memory           │
   │ TABLE        │ orders            │               │ BASE_TABLE_SCHEMA_NAME   │ main             │
   │ TABLE        │ orders            │               │ BASE_TABLE_NAME          │ orders           │
   │ TABLE        │ orders            │               │ PRIMARY_KEY              │ ["id"]           │
   │ TABLE        │ customers         │               │ BASE_TABLE_DATABASE_NAME │ memory           │
   │ TABLE        │ customers         │               │ BASE_TABLE_SCHEMA_NAME   │ main             │
   │ TABLE        │ customers         │               │ BASE_TABLE_NAME          │ customers        │
   │ TABLE        │ customers         │               │ PRIMARY_KEY              │ ["id"]           │
   │ RELATIONSHIP │ order_to_customer │ orders        │ TABLE                    │ orders           │
   │ RELATIONSHIP │ order_to_customer │ orders        │ REF_TABLE                │ customers        │
   │ RELATIONSHIP │ order_to_customer │ orders        │ FOREIGN_KEY              │ ["customer_id"]  │
   │ RELATIONSHIP │ order_to_customer │ orders        │ REF_KEY                  │ ["id"]           │
   │ DIMENSION    │ region            │ orders        │ TABLE                    │ orders           │
   │ DIMENSION    │ region            │ orders        │ EXPRESSION               │ o.region         │
   │ DIMENSION    │ region            │ orders        │ DATA_TYPE                │                  │
   │ DIMENSION    │ customer_name     │ customers     │ TABLE                    │ customers        │
   │ DIMENSION    │ customer_name     │ customers     │ EXPRESSION               │ c.name           │
   │ DIMENSION    │ customer_name     │ customers     │ DATA_TYPE                │                  │
   │ METRIC       │ total_revenue     │ orders        │ TABLE                    │ orders           │
   │ METRIC       │ total_revenue     │ orders        │ EXPRESSION               │ SUM(o.amount)    │
   │ METRIC       │ total_revenue     │ orders        │ DATA_TYPE                │                  │
   └──────────────┴───────────────────┴───────────────┴──────────────────────────┴──────────────────┘

**View with facts:**

.. code-block:: sql

   CREATE SEMANTIC VIEW fact_view AS
   TABLES (
       o  AS orders     PRIMARY KEY (id),
       li AS line_items PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       li_to_order AS li(order_id) REFERENCES o
   )
   FACTS (
       li.net_price AS li.price * li.quantity
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.total_net AS SUM(li.net_price)
   );

   DESCRIBE SEMANTIC VIEW fact_view;

.. code-block:: text

   ┌──────────────┬─────────────┬───────────────┬──────────────────────────┬──────────────────────────┐
   │ object_kind  │ object_name │ parent_entity │ property                 │ property_value           │
   ├──────────────┼─────────────┼───────────────┼──────────────────────────┼──────────────────────────┤
   │ TABLE        │ orders      │               │ BASE_TABLE_DATABASE_NAME │ memory                   │
   │ TABLE        │ orders      │               │ BASE_TABLE_SCHEMA_NAME   │ main                     │
   │ TABLE        │ orders      │               │ BASE_TABLE_NAME          │ orders                   │
   │ TABLE        │ orders      │               │ PRIMARY_KEY              │ ["id"]                   │
   │ TABLE        │ line_items  │               │ BASE_TABLE_DATABASE_NAME │ memory                   │
   │ TABLE        │ line_items  │               │ BASE_TABLE_SCHEMA_NAME   │ main                     │
   │ TABLE        │ line_items  │               │ BASE_TABLE_NAME          │ line_items               │
   │ TABLE        │ line_items  │               │ PRIMARY_KEY              │ ["id"]                   │
   │ RELATIONSHIP │ li_to_order │ line_items    │ TABLE                    │ line_items               │
   │ RELATIONSHIP │ li_to_order │ line_items    │ REF_TABLE                │ orders                   │
   │ RELATIONSHIP │ li_to_order │ line_items    │ FOREIGN_KEY              │ ["order_id"]             │
   │ RELATIONSHIP │ li_to_order │ line_items    │ REF_KEY                  │ ["id"]                   │
   │ FACT         │ net_price   │ line_items    │ TABLE                    │ line_items               │
   │ FACT         │ net_price   │ line_items    │ EXPRESSION               │ li.price * li.quantity   │
   │ FACT         │ net_price   │ line_items    │ DATA_TYPE                │                          │
   │ DIMENSION    │ region      │ orders        │ TABLE                    │ orders                   │
   │ DIMENSION    │ region      │ orders        │ EXPRESSION               │ o.region                 │
   │ DIMENSION    │ region      │ orders        │ DATA_TYPE                │                          │
   │ METRIC       │ total_net   │ orders        │ TABLE                    │ orders                   │
   │ METRIC       │ total_net   │ orders        │ EXPRESSION               │ SUM(li.net_price)        │
   │ METRIC       │ total_net   │ orders        │ DATA_TYPE                │                          │
   └──────────────┴─────────────┴───────────────┴──────────────────────────┴──────────────────────────┘

**View with derived metrics:**

Derived metrics appear as ``DERIVED_METRIC`` with an empty ``parent_entity`` and only ``EXPRESSION`` and ``DATA_TYPE`` properties (no ``TABLE`` property):

.. code-block:: sql

   CREATE SEMANTIC VIEW derived_view AS
   TABLES (o AS orders PRIMARY KEY (id))
   DIMENSIONS (o.region AS o.region)
   METRICS (
       o.revenue AS SUM(o.amount),
       profit AS revenue * 0.3
   );

   DESCRIBE SEMANTIC VIEW derived_view;

.. code-block:: text

   ┌────────────────┬─────────────┬───────────────┬──────────────────────────┬──────────────────┐
   │ object_kind    │ object_name │ parent_entity │ property                 │ property_value   │
   ├────────────────┼─────────────┼───────────────┼──────────────────────────┼──────────────────┤
   │ TABLE          │ orders      │               │ BASE_TABLE_DATABASE_NAME │ memory           │
   │ TABLE          │ orders      │               │ BASE_TABLE_SCHEMA_NAME   │ main             │
   │ TABLE          │ orders      │               │ BASE_TABLE_NAME          │ orders           │
   │ TABLE          │ orders      │               │ PRIMARY_KEY              │ ["id"]           │
   │ DIMENSION      │ region      │ orders        │ TABLE                    │ orders           │
   │ DIMENSION      │ region      │ orders        │ EXPRESSION               │ o.region         │
   │ DIMENSION      │ region      │ orders        │ DATA_TYPE                │                  │
   │ METRIC         │ revenue     │ orders        │ TABLE                    │ orders           │
   │ METRIC         │ revenue     │ orders        │ EXPRESSION               │ SUM(o.amount)    │
   │ METRIC         │ revenue     │ orders        │ DATA_TYPE                │                  │
   │ DERIVED_METRIC │ profit      │               │ EXPRESSION               │ revenue * 0.3    │
   │ DERIVED_METRIC │ profit      │               │ DATA_TYPE                │                  │
   └────────────────┴─────────────┴───────────────┴──────────────────────────┴──────────────────┘

**Table without PRIMARY KEY:**

When a table is declared without ``PRIMARY KEY``, the ``PRIMARY_KEY`` property row is omitted:

.. code-block:: sql

   CREATE SEMANTIC VIEW no_pk_view AS
   TABLES (o AS orders)
   DIMENSIONS (o.region AS o.region)
   METRICS (o.total AS SUM(o.amount));

   DESCRIBE SEMANTIC VIEW no_pk_view;

.. code-block:: text

   ┌─────────────┬─────────────┬───────────────┬──────────────────────────┬──────────────────┐
   │ object_kind │ object_name │ parent_entity │ property                 │ property_value   │
   ├─────────────┼─────────────┼───────────────┼──────────────────────────┼──────────────────┤
   │ TABLE       │ orders      │               │ BASE_TABLE_DATABASE_NAME │ memory           │
   │ TABLE       │ orders      │               │ BASE_TABLE_SCHEMA_NAME   │ main             │
   │ TABLE       │ orders      │               │ BASE_TABLE_NAME          │ orders           │
   │ DIMENSION   │ region      │ orders        │ TABLE                    │ orders           │
   │ DIMENSION   │ region      │ orders        │ EXPRESSION               │ o.region         │
   │ DIMENSION   │ region      │ orders        │ DATA_TYPE                │                  │
   │ METRIC      │ total       │ orders        │ TABLE                    │ orders           │
   │ METRIC      │ total       │ orders        │ EXPRESSION               │ SUM(o.amount)    │
   │ METRIC      │ total       │ orders        │ DATA_TYPE                │                  │
   └─────────────┴─────────────┴───────────────┴──────────────────────────┴──────────────────┘

The ``TABLE`` block has only 3 rows instead of 4.

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
