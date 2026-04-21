.. meta::
   :description: Syntax reference for GET_DDL(), which returns the full CREATE DDL text for a semantic view

.. _ref-get-ddl:

=========
GET_DDL
=========

Scalar function that returns the full ``CREATE OR REPLACE SEMANTIC VIEW`` DDL text for a stored semantic view. The output is a syntactically valid DDL statement that can be executed to recreate the view.


.. _ref-get-ddl-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SELECT GET_DDL('<object_type>', '<name>')


.. _ref-get-ddl-params:

Parameters
==========

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Parameter
     - Type
     - Description
   * - ``<object_type>``
     - VARCHAR
     - The object type. Only ``'SEMANTIC_VIEW'`` is supported (case-insensitive).
   * - ``<name>``
     - VARCHAR
     - The name of the semantic view. Returns an error if the view does not exist.


.. _ref-get-ddl-output:

Output
======

Returns a single VARCHAR value containing the full ``CREATE OR REPLACE SEMANTIC VIEW`` DDL statement. The DDL includes all clauses (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS, MATERIALIZATIONS) with all annotations (COMMENT, WITH SYNONYMS, PRIVATE, NON ADDITIVE BY, OVER). The ``MATERIALIZATIONS`` clause is included only when the view has materializations declared; it is omitted for views without materializations.


.. _ref-get-ddl-examples:

Examples
========

**Retrieve DDL for a semantic view:**

.. code-block:: sql

   SELECT GET_DDL('SEMANTIC_VIEW', 'sales');

Sample output:

.. code-block:: text

   CREATE OR REPLACE SEMANTIC VIEW sales AS
   TABLES (
       o AS orders PRIMARY KEY (id) COMMENT = 'Order transactions'
   )
   DIMENSIONS (
       o.region AS o.region COMMENT = 'Sales region'
   )
   METRICS (
       o.revenue AS SUM(o.amount) COMMENT = 'Total revenue'
   )

**Retrieve DDL for a view with materializations:**

.. code-block:: sql

   SELECT GET_DDL('SEMANTIC_VIEW', 'order_metrics');

Sample output:

.. code-block:: text

   CREATE OR REPLACE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount),
       o.order_count AS COUNT(*)
   )
   MATERIALIZATIONS (
       region_agg AS (
           TABLE daily_revenue_by_region,
           DIMENSIONS (region),
           METRICS (revenue, order_count)
       )
   )

**Round-trip verification:**

The DDL output can be executed to recreate the view with identical semantics:

.. code-block:: sql

   -- Save the DDL
   CREATE TABLE ddl_backup AS
   SELECT GET_DDL('SEMANTIC_VIEW', 'sales') AS ddl;

   -- Drop and recreate
   DROP SEMANTIC VIEW sales;

   -- Execute the saved DDL (copy-paste the output)

**Error: unsupported object type:**

.. code-block:: sql

   SELECT GET_DDL('TABLE', 'orders');

.. code-block:: text

   Error: GET_DDL: unsupported object type 'TABLE'. Only 'SEMANTIC_VIEW' is supported.

**Error: view does not exist:**

.. code-block:: sql

   SELECT GET_DDL('SEMANTIC_VIEW', 'nonexistent');

.. code-block:: text

   Error: semantic view 'nonexistent' does not exist
