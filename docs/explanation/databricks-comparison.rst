.. meta::
   :description: Feature-by-feature comparison with Databricks metric views, covering concept mapping, syntax differences, and feature coverage

.. _explanation-databricks:

=======================
Databricks Comparison
=======================

Databricks offers `Metric Views <https://docs.databricks.com/aws/en/metric-views/>`_ as part of its semantic layer. If you have used Databricks metric views, this page maps the key concepts to DuckDB Semantic Views, highlights the differences, and identifies features unique to each system.

.. note::

   Databricks metric views were announced in 2024 and are evolving rapidly. This comparison
   reflects Databricks' documented surface as of early 2026. Feature availability may vary by
   Databricks runtime version and workspace configuration.


.. _explanation-db-concepts:

Concept Mapping
===============

.. list-table::
   :header-rows: 1
   :widths: 30 35 35

   * - Concept
     - Databricks Metric Views
     - DuckDB Semantic Views
   * - Define a semantic layer
     - ``CREATE METRIC VIEW``
     - ``CREATE SEMANTIC VIEW``
   * - Table declarations
     - ``FROM`` clause with a single source table or subquery
     - ``TABLES`` clause with aliases, optional ``PRIMARY KEY``
   * - Multi-table relationships
     - Join logic embedded in the ``FROM`` clause (explicit JOINs)
     - ``RELATIONSHIPS`` clause with FK REFERENCES (join synthesis)
   * - Dimensions
     - ``DIMENSIONS`` clause
     - ``DIMENSIONS`` clause
   * - Metrics (measures)
     - ``MEASURES`` clause
     - ``METRICS`` clause
   * - Reusable row-level expressions
     - Not directly supported; use subqueries or pre-computed columns
     - ``FACTS`` clause (queryable via ``facts := [...]``)
   * - Metric composition
     - Measures can reference other measures
     - Derived metrics (metric referencing other metrics)
   * - Semi-additive metrics
     - Not directly supported
     - ``NON ADDITIVE BY`` (see :ref:`howto-semi-additive`)
   * - Window function metrics
     - Not directly supported
     - ``OVER`` clause with ``PARTITION BY EXCLUDING`` (see :ref:`howto-window-metrics`)
   * - Metadata annotations
     - ``COMMENT``
     - ``COMMENT``, ``WITH SYNONYMS``
   * - Access modifiers
     - Column masking and row filters (workspace-level)
     - ``PRIVATE`` / ``PUBLIC`` on metrics and facts
   * - Materializations
     - Not part of metric views (handled by Delta Lake materialized views separately)
     - ``MATERIALIZATIONS`` clause for routing to pre-aggregated tables (see :ref:`howto-materializations`)
   * - YAML definitions
     - Not supported for metric views
     - ``FROM YAML`` import and :ref:`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>` export (see :ref:`howto-yaml-definitions`)
   * - Query interface
     - Standard SQL against the metric view name
     - :ref:`semantic_view() <ref-semantic-view-function>` table function
   * - View inspection
     - ``DESCRIBE`` / ``SHOW``
     - :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>`, :ref:`SHOW SEMANTIC VIEWS <ref-show-semantic-views>`
   * - DDL retrieval
     - ``SHOW CREATE TABLE``
     - :ref:`GET_DDL('SEMANTIC_VIEW', ...) <ref-get-ddl>`


.. _explanation-db-syntax:

Syntax Comparison
=================

Databricks metric views use a different structural approach from DuckDB Semantic Views. Databricks embeds the source query in a ``FROM`` clause and separates output columns into ``DIMENSIONS`` and ``MEASURES``. DuckDB Semantic Views declare tables, relationships, and column definitions in distinct clauses.

.. tab-set::
   :sync-group: platform

   .. tab-item:: Databricks
      :sync: databricks

      .. code-block:: sql

         CREATE METRIC VIEW revenue_by_region AS
         FROM orders
         DIMENSIONS (
             region
         )
         MEASURES (
             revenue AS SUM(amount)
         );

   .. tab-item:: DuckDB Semantic Views
      :sync: duckdb

      .. code-block:: sql

         CREATE SEMANTIC VIEW revenue_by_region AS
         TABLES (
             o AS orders PRIMARY KEY (id)
         )
         DIMENSIONS (
             o.region AS o.region
         )
         METRICS (
             o.revenue AS SUM(o.amount)
         );


.. _explanation-db-differences:

Key Differences
===============

Multi-Table Handling
--------------------

Databricks metric views support a single ``FROM`` clause that can contain explicit JOINs:

.. code-block:: sql

   -- Databricks: joins are explicit in FROM
   CREATE METRIC VIEW analytics AS
   FROM orders o
   JOIN customers c ON o.customer_id = c.id
   DIMENSIONS (
       c.name AS customer_name,
       o.region
   )
   MEASURES (
       revenue AS SUM(o.amount)
   );

DuckDB Semantic Views declare tables separately and let the extension synthesize JOINs based on declared relationships:

.. code-block:: sql

   -- DuckDB: joins are synthesized from relationships
   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       o AS orders    PRIMARY KEY (id),
       c AS customers PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c
   )
   DIMENSIONS (
       c.customer_name AS c.name,
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   );

The DuckDB approach means the extension joins **only the tables needed** for each query. If a query requests only ``region`` and ``revenue`` (both from the ``orders`` table), the ``customers`` table is never joined. In Databricks, the ``FROM`` clause always includes all declared joins.


Query Interface
---------------

.. warning::

   DuckDB Semantic Views uses a table function for queries, not direct SQL.

Databricks metric views are queried with standard SQL, as if querying a regular table or view:

.. code-block:: sql

   -- Databricks: standard SQL
   SELECT region, revenue
   FROM revenue_by_region;

DuckDB Semantic Views uses the :ref:`semantic_view() <ref-semantic-view-function>` table function with explicit dimension and metric names:

.. code-block:: sql

   -- DuckDB: table function with named lists
   SELECT * FROM semantic_view('revenue_by_region',
       dimensions := ['region'],
       metrics := ['revenue']
   );


Keyword: MEASURES vs METRICS
-----------------------------

Databricks uses ``MEASURES`` for aggregate columns. DuckDB Semantic Views uses ``METRICS``, following Snowflake's naming convention. The concept is the same: named aggregate expressions.


Dimension Expressions
---------------------

In Databricks, dimensions can be simple column references (``region``) without explicit expressions. In DuckDB Semantic Views, every dimension requires an explicit expression with a table-alias prefix: ``o.region AS o.region``. Computed dimensions use any SQL expression: ``o.month AS date_trunc('month', o.order_date)``.


.. _explanation-db-unique-duckdb:

Features in DuckDB Semantic Views Not in Databricks
====================================================

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Feature
     - Description
   * - ``FACTS`` clause
     - Named row-level expressions, queryable directly (``facts := [...]``) and reusable in metrics. Databricks has no equivalent.
   * - ``NON ADDITIVE BY``
     - Semi-additive metric support for snapshot data. Databricks requires manual CTE or subquery logic.
   * - Window metrics (``OVER``)
     - Declarative window function metrics with ``PARTITION BY EXCLUDING``. Databricks metric views do not support window functions as measures.
   * - ``MATERIALIZATIONS``
     - Automatic routing to pre-aggregated tables when dims/metrics exactly match. Databricks handles materialization through separate Delta Lake materialized views.
   * - ``WITH SYNONYMS``
     - Alternative names for discoverability on tables, dimensions, metrics, and facts.
   * - ``PRIVATE`` / ``PUBLIC``
     - Access modifiers on metrics and facts at the semantic view level.
   * - ``RELATIONSHIPS``
     - Declarative FK/PK relationships with automatic join synthesis and cardinality inference. Databricks uses explicit JOINs.
   * - Fan trap detection
     - Automatic detection of one-to-many join traversals that would inflate aggregation results.
   * - Role-playing dimensions
     - ``USING`` clause on metrics to disambiguate multiple join paths to the same table.
   * - YAML import/export
     - ``FROM YAML`` definition import and :ref:`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>` export for version control and migration.
   * - :ref:`explain_semantic_view() <ref-explain-semantic-view>`
     - Inspect the generated SQL and query plan before execution.


.. _explanation-db-unique-databricks:

Features in Databricks Not in DuckDB Semantic Views
====================================================

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Feature
     - Description
   * - Direct SQL query interface
     - Query metric views with standard ``SELECT`` SQL. DuckDB uses a table function.
   * - Unity Catalog integration
     - Metric views are first-class catalog objects with lineage tracking, access control, and governance.
   * - Row-level security / column masking
     - Databricks provides fine-grained access control at the workspace level. DuckDB defers access control to DuckDB's own mechanisms.
   * - AI/BI integration
     - Metric views power Databricks AI/BI dashboards and natural-language queries through Genie.
   * - Delta Lake materialized views
     - Managed materialized views that automatically refresh when underlying data changes. Separate from metric views but complementary.


.. _explanation-db-choosing:

Choosing Between Them
=====================

Databricks metric views are purpose-built for the Databricks ecosystem. They integrate with Unity Catalog, AI/BI dashboards, and the broader Databricks workspace. If your data already lives in Databricks and your team uses the Databricks platform, metric views fit naturally into the workflow.

DuckDB Semantic Views targets a different use case: lightweight, local-first analytics with an open-source, embeddable engine. It is designed for data engineers who want a semantic layer that runs anywhere DuckDB runs -- inside an application server, in a notebook, against Iceberg tables, or on a developer laptop -- without depending on a cloud platform.

The two systems are not interchangeable. They solve the same conceptual problem (define metrics once, query flexibly) but for different deployment models and ecosystems.
