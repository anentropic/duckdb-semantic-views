.. meta::
   :description: Feature-by-feature comparison with Snowflake semantic views SQL DDL, covering syntax alignment, key differences, and unsupported features

.. _explanation-snowflake:

======================
Snowflake Comparison
======================

DuckDB Semantic Views is modeled on Snowflake's ``CREATE SEMANTIC VIEW`` SQL DDL interface. If you have used Snowflake semantic views, much of the syntax and concept model will be familiar. This page maps the key concepts and calls out the differences.

.. note::

   Snowflake has two distinct interfaces for semantic views: the SQL DDL (``CREATE SEMANTIC VIEW``)
   and the older YAML spec (``CREATE SEMANTIC VIEW FROM YAML``, designed for Cortex Analyst).
   All comparisons on this page target the SQL DDL interface only. The YAML spec includes
   concepts like ``time_dimensions``, ``custom_instructions``, and ``access_modifier`` that
   exist to serve the AI SQL generation layer and have no equivalent in the SQL DDL.


.. _explanation-sf-concepts:

Concept Mapping
===============

.. list-table::
   :header-rows: 1
   :widths: 30 35 35

   * - Concept
     - Snowflake SQL DDL
     - DuckDB Semantic Views
   * - Define a semantic view
     - ``CREATE SEMANTIC VIEW``
     - ``CREATE SEMANTIC VIEW``
   * - Table declarations
     - ``TABLES`` clause with aliases
     - ``TABLES`` clause with aliases and optional ``PRIMARY KEY``
   * - Relationships
     - ``RELATIONSHIPS`` clause with FK REFERENCES
     - ``RELATIONSHIPS`` clause with FK REFERENCES
   * - Dimensions
     - ``DIMENSIONS`` clause
     - ``DIMENSIONS`` clause
   * - Metrics (measures)
     - ``METRICS`` clause
     - ``METRICS`` clause
   * - Reusable row-level expressions
     - ``FACTS`` clause
     - ``FACTS`` clause (queryable via ``facts := [...]``)
   * - Metric composition
     - Derived metrics (metric referencing other metrics)
     - Derived metrics (same pattern)
   * - Semi-additive metrics
     - ``SEMI ADDITIVE`` / ``NON ADDITIVE BY``
     - ``NON ADDITIVE BY`` (see :ref:`howto-semi-additive`)
   * - Window function metrics
     - ``OVER`` clause with ``PARTITION BY EXCLUDING``
     - ``OVER`` clause with ``PARTITION BY EXCLUDING`` (see :ref:`howto-window-metrics`)
   * - Metadata annotations
     - ``COMMENT``, ``WITH SYNONYMS``
     - ``COMMENT``, ``WITH SYNONYMS`` (see :ref:`howto-metadata-annotations`)
   * - Access modifiers
     - ``PRIVATE`` / ``PUBLIC``
     - ``PRIVATE`` / ``PUBLIC`` on metrics and facts
   * - Materializations / pre-aggregation
     - Not part of Snowflake's ``CREATE SEMANTIC VIEW`` DDL
     - ``MATERIALIZATIONS`` clause for routing to pre-aggregated tables (see :ref:`howto-materializations`)
   * - Query interface
     - Direct SQL with semantic resolution
     - :ref:`semantic_view() <ref-semantic-view-function>` table function
   * - Wildcard selection
     - ``alias.*`` in query parameters
     - ``alias.*`` in ``dimensions``, ``metrics``, ``facts`` parameters (see :ref:`howto-wildcard-selection`)
   * - View inspection
     - ``DESCRIBE SEMANTIC VIEW``
     - ``DESCRIBE SEMANTIC VIEW``
   * - List views
     - ``SHOW SEMANTIC VIEWS``
     - ``SHOW SEMANTIC VIEWS``
   * - Terse view listing
     - ``SHOW TERSE SEMANTIC VIEWS``
     - ``SHOW TERSE SEMANTIC VIEWS``
   * - Column listing
     - ``SHOW COLUMNS IN SEMANTIC VIEW``
     - :ref:`SHOW COLUMNS IN SEMANTIC VIEW <ref-show-columns>`
   * - Filter by scope
     - ``IN SCHEMA`` / ``IN DATABASE``
     - ``IN SCHEMA`` / ``IN DATABASE`` on SHOW commands
   * - Retrieve DDL text
     - ``GET_DDL('SEMANTIC_VIEW', ...)``
     - :ref:`GET_DDL('SEMANTIC_VIEW', ...) <ref-get-ddl>`
   * - Alter a view
     - ``ALTER SEMANTIC VIEW``
     - :ref:`ALTER SEMANTIC VIEW <ref-alter-semantic-view>` (RENAME TO, SET COMMENT, UNSET COMMENT)
   * - Drop a view
     - ``DROP SEMANTIC VIEW``
     - :ref:`DROP SEMANTIC VIEW <ref-drop-semantic-view>`


.. _explanation-sf-syntax:

Syntax Alignment
================

The DDL syntax is intentionally close to Snowflake's. The clause order (``TABLES``, ``RELATIONSHIPS``, ``FACTS``, ``DIMENSIONS``, ``METRICS``) matches Snowflake, and the entry syntax within each clause follows the same pattern.

.. tab-set::
   :sync-group: platform

   .. tab-item:: Snowflake
      :sync: snowflake

      .. code-block:: sql

         CREATE SEMANTIC VIEW analytics AS
         TABLES (
             o AS orders,
             c AS customers
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

   .. tab-item:: DuckDB Semantic Views
      :sync: duckdb

      .. code-block:: sql

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


.. _explanation-sf-differences:

Key Differences
===============

Primary Key Declarations
------------------------

.. note::

   ``PRIMARY KEY`` declarations in the ``TABLES`` clause are optional at the syntax level.
   Whether you need them in practice depends on your data source.

Snowflake resolves PK/FK metadata directly from its catalog, so its SQL DDL does not require
explicit ``PRIMARY KEY`` declarations. DuckDB also has catalog-level PK/FK metadata -- but only
for native DuckDB tables that were created with ``PRIMARY KEY`` constraints.

For external data sources -- Parquet files, CSV, Iceberg REST catalog tables, or any table not
physically defined in DuckDB -- the DuckDB catalog has no PK/FK information to consult. When
you create a semantic view, the extension queries DuckDB's internal ``duckdb_constraints()``
table for each declared table. If a native DuckDB table has a ``PRIMARY KEY`` constraint, the
extension finds it automatically and you can omit ``PRIMARY KEY`` from the ``TABLES`` clause.
If the constraint is not there -- as is the case for Iceberg and other external sources --
you must declare it explicitly.

.. tip::

   Most data engineers using DuckDB with Iceberg tables via the ``iceberg`` extension will
   need to include ``PRIMARY KEY`` declarations in their semantic view DDL. Iceberg's own
   metadata supports primary key fields, but DuckDB does not surface those constraints
   through ``duckdb_constraints()``. Declare them in the ``TABLES`` clause to unlock
   join inference and relationship validation.

The three cases are:

.. list-table::
   :header-rows: 1
   :widths: 40 30 30

   * - Data source
     - Catalog PK available?
     - ``PRIMARY KEY`` in DDL required?
   * - Native DuckDB table with ``PRIMARY KEY`` constraint
     - Yes (main schema only)
     - No (resolved automatically)
   * - Native DuckDB table without ``PRIMARY KEY`` constraint
     - No
     - Yes
   * - External source (Parquet, CSV, Iceberg, Postgres, etc.)
     - No
     - Yes

.. note::

   Automatic PK resolution applies only to tables in the ``main`` schema of the current
   DuckDB database. Tables in other schemas or attached databases are not resolved
   automatically -- declare ``PRIMARY KEY`` explicitly for those tables.

.. code-block:: sql

   -- Native DuckDB table: PRIMARY KEY can be omitted if the table was created with one
   CREATE TABLE orders (id INTEGER PRIMARY KEY, amount DECIMAL);
   CREATE TABLE customers (id INTEGER PRIMARY KEY, name VARCHAR);

   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       o AS orders,         -- PK resolved automatically from DuckDB catalog
       c AS customers       -- PK resolved automatically from DuckDB catalog
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c
   )
   DIMENSIONS (c.name AS c.name)
   METRICS (o.revenue AS SUM(o.amount));

.. code-block:: sql

   -- Iceberg or other external source: PRIMARY KEY must be declared explicitly
   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       o AS orders    PRIMARY KEY (id),
       c AS customers PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c
   )
   DIMENSIONS (c.name AS c.name)
   METRICS (o.revenue AS SUM(o.amount));

If a table involved in a ``RELATIONSHIPS`` entry has no primary key -- neither from the catalog
nor from an explicit declaration -- the extension raises an error at ``CREATE`` time:
``Table 'X' has no PRIMARY KEY. Specify referenced columns explicitly: REFERENCES X(col).``
This prevents the extension from synthesizing an incorrect JOIN ON clause.


Query Interface
---------------

.. warning::

   DuckDB Semantic Views uses a table function for queries, not direct SQL.

In Snowflake, you can write standard SQL against a semantic view and the system resolves dimensions and metrics. In DuckDB, you use the :ref:`semantic_view() <ref-semantic-view-function>` table function with explicit dimension and metric names.

.. code-block:: sql

   -- DuckDB: table function with named lists
   SELECT * FROM semantic_view('analytics',
       dimensions := ['region'],
       metrics := ['revenue']
   );

   -- Snowflake: equivalent SEMANTIC_VIEW clause
   SELECT * FROM SEMANTIC_VIEW('analytics',
       DIMENSIONS 'region'
       METRICS 'revenue'
   );

   -- Snowflake: direct SQL with AGG view-defined aggregate function
   -- (NOT currently supported in duckdb-semantic-views)
   SELECT region, AGG(revenue)
   FROM analytics
   GROUP BY region;


Cardinality Inference
---------------------

Both systems infer cardinality from constraints. In DuckDB Semantic Views, cardinality is inferred from ``PRIMARY KEY`` and ``UNIQUE`` declarations in the ``TABLES`` clause:

- If the FK columns on the "from" side match a PK or UNIQUE constraint, the relationship is one-to-one.
- Otherwise, the relationship is many-to-one (the default).

The extension uses inferred cardinality for :ref:`fan trap detection <howto-fan-traps>`.


USING RELATIONSHIPS
-------------------

Both systems support ``USING`` on metrics to select which relationship path a metric traverses. The syntax is identical:

.. code-block:: sql

   METRICS (
       f.departures USING (dep_airport) AS COUNT(*)
   )


Facts Query Mode
----------------

.. versionadded:: 0.6.0

Both systems allow facts to be queried directly as row-level columns. In Snowflake, facts appear in the ``SEMANTIC_VIEW()`` query function. In DuckDB Semantic Views, use the ``facts`` parameter:

.. code-block:: sql

   -- DuckDB: query facts as row-level columns
   SELECT * FROM semantic_view('analytics',
       dimensions := ['region'],
       facts := ['net_price']
   );

.. warning::

   In both systems, facts and metrics cannot be combined in the same query. Use ``facts := [...]`` OR ``metrics := [...]``, not both.


Semi-Additive and Window Metrics
---------------------------------

.. versionadded:: 0.6.0

Both systems support semi-additive metrics (``NON ADDITIVE BY``) and window function metrics (``OVER`` with ``PARTITION BY EXCLUDING``). The syntax is aligned:

.. code-block:: sql

   -- Semi-additive: last balance per account, summed across customers
   METRICS (
       a.balance NON ADDITIVE BY (date_dim) AS SUM(a.amount)
   )

   -- Window: rolling average excluding region from partition
   METRICS (
       o.avg_qty AS AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month)
   )

The behavioral differences are:

- ``NON ADDITIVE BY`` dimensions must be declared in the view's ``DIMENSIONS`` clause. Snowflake validates against its own catalog.
- Window metrics and ``NON ADDITIVE BY`` cannot be combined on the same metric (mutually exclusive).
- Window metrics cannot be mixed with aggregate metrics in the same query.


Materializations
-----------------

.. versionadded:: 0.7.0

Snowflake's ``CREATE SEMANTIC VIEW`` SQL DDL does not include a materializations or pre-aggregation concept. Pre-aggregation in Snowflake is handled through separate materialized views.

DuckDB Semantic Views introduces a ``MATERIALIZATIONS`` clause that declares mappings from pre-aggregated tables to the dimensions and metrics they cover. When a query exactly matches a materialization, the extension routes to the pre-aggregated table instead of expanding raw sources. See :ref:`howto-materializations` for details.


.. _explanation-sf-not-supported:

Features Not Yet Supported
==========================

The following Snowflake ``CREATE SEMANTIC VIEW`` features are not yet implemented in DuckDB Semantic Views:

.. list-table::
   :header-rows: 1
   :widths: 40 60

   * - Snowflake Feature
     - Status
   * - Direct SQL query interface
     - Not planned; :ref:`semantic_view() <ref-semantic-view-function>` table function is the query interface
   * - Column-level security
     - Out of scope; DuckDB handles access control
   * - ``ASOF`` / temporal relationships
     - Not planned; standard equi-joins cover most use cases


.. _explanation-sf-yaml:

A Note on Snowflake's YAML Spec
================================

Snowflake's `YAML-based semantic view definition <https://docs.snowflake.com/en/user-guide/views-semantic/semantic-view-yaml-spec>`_ (``CREATE SEMANTIC VIEW FROM YAML``) is a separate interface designed for Cortex Analyst, Snowflake's AI SQL generation layer. The YAML spec includes concepts that do not exist in the SQL DDL:

- ``time_dimensions`` with granularity controls (the SQL DDL uses regular dimensions with ``date_trunc()``)
- ``custom_instructions`` for AI prompt tuning
- ``access_modifier`` for column-level security
- ``sample_values`` for AI context

DuckDB Semantic Views supports YAML definition import (``FROM YAML``) and export (:ref:`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>`), but these use the extension's own YAML schema -- not Snowflake's Cortex Analyst YAML spec. The DuckDB YAML format is a serialization of the same model used by the SQL DDL (tables, relationships, facts, dimensions, metrics, materializations). It is designed for version control, migration, and sharing -- not for AI prompt tuning. Comparisons against Snowflake YAML-spec-only features remain not applicable.

See :ref:`howto-yaml-definitions` for the DuckDB YAML workflow.
