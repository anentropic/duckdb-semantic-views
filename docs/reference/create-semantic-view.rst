.. meta::
   :description: Full syntax and parameter reference for CREATE SEMANTIC VIEW, covering TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS, and MATERIALIZATIONS clauses, plus FROM YAML and FROM YAML FILE variants

.. _ref-create-semantic-view:

======================
CREATE SEMANTIC VIEW
======================

Creates a semantic view definition, registering dimensions, metrics, relationships, facts, and materializations for on-demand query expansion.


.. _ref-create-syntax:

Syntax
======

**Keyword body (AS):**

.. code-block:: sqlgrammar

   CREATE [ OR REPLACE ] SEMANTIC VIEW [ IF NOT EXISTS ] <name> AS
   TABLES (
       <alias> AS <table_name>
           [ PRIMARY KEY ( <column> [, <column> ...] ) ]
           [ UNIQUE ( <column> [, <column> ...] ) ]
           [ COMMENT = '<text>' ]
           [ WITH SYNONYMS = ( '<synonym>' [, '<synonym>' ...] ) ]
       [, ... ]
   )
   [ RELATIONSHIPS (
       <rel_name> AS <from_alias>( <fk_column> [, <fk_column> ...] )
           REFERENCES <to_alias> [( <ref_column> [, <ref_column> ...] )]
       [, ... ]
   ) ]
   [ FACTS (
       [ PRIVATE ] <alias>.<fact_name> AS <row_level_expression>
           [ COMMENT = '<text>' ]
           [ WITH SYNONYMS = ( '<synonym>' [, '<synonym>' ...] ) ]
       [, ... ]
   ) ]
   [ DIMENSIONS (
       <alias>.<dim_name> AS <expression>
           [ COMMENT = '<text>' ]
           [ WITH SYNONYMS = ( '<synonym>' [, '<synonym>' ...] ) ]
       [, ... ]
   ) ]
   [ METRICS (
       [ PRIVATE ] <alias>.<metric_name>
           [ USING ( <rel_name> [, <rel_name> ...] ) ]
           [ NON ADDITIVE BY ( <dim_name> [ ASC | DESC ] [ NULLS FIRST | NULLS LAST ]
               [, <dim_name> ... ] ) ]
           AS <aggregate_expression>
           [ COMMENT = '<text>' ]
           [ WITH SYNONYMS = ( '<synonym>' [, '<synonym>' ...] ) ]
       [, [ PRIVATE ] <metric_name>
           [ NON ADDITIVE BY ( ... ) ]
           AS <expression> ... ]
       [, [ PRIVATE ] <alias>.<metric_name> AS
           <window_func>( <inner_metric> [, <extra_arg> ...] )
               OVER ( [ PARTITION BY EXCLUDING <dim_name> [, <dim_name> ...] ]
                      | [ PARTITION BY <dim_name> [, <dim_name> ...] ]
                      [ ORDER BY <dim_name> [ ASC | DESC ] [ NULLS FIRST | NULLS LAST ]
                          [, <dim_name> ...] ]
                      [ <frame_clause> ] )
           [ COMMENT = '<text>' ]
           [ WITH SYNONYMS = ( '<synonym>' [, '<synonym>' ...] ) ] ]
   ) ]
   [ MATERIALIZATIONS (
       <mat_name> AS (
           TABLE <table_name>,
           [ DIMENSIONS ( <dim_name> [, <dim_name> ...] ) , ]
           [ METRICS ( <metric_name> [, <metric_name> ...] ) ]
       )
       [, ... ]
   ) ]

**YAML body (FROM YAML):**

.. versionadded:: 0.7.0

.. code-block:: sqlgrammar

   CREATE [ OR REPLACE ] SEMANTIC VIEW [ IF NOT EXISTS ] <name>
       FROM YAML $$ <yaml_content> $$

   CREATE [ OR REPLACE ] SEMANTIC VIEW [ IF NOT EXISTS ] <name>
       FROM YAML FILE '<file_path>'

The ``FROM YAML`` variant accepts a YAML definition in a dollar-quoted string (``$$...$$`` or ``$tag$...$tag$``). The ``FROM YAML FILE`` variant reads the YAML definition from a file at the given path.


.. _ref-create-variants:

Statement Variants
==================

``CREATE SEMANTIC VIEW <name> AS ...``
   Creates a new semantic view. Returns an error if a view with the same name already exists.

``CREATE OR REPLACE SEMANTIC VIEW <name> AS ...``
   Creates or replaces an existing semantic view with the same name. If the view does not exist, creates it. If it does, replaces the definition.

``CREATE SEMANTIC VIEW IF NOT EXISTS <name> AS ...``
   Creates a new semantic view only if no view with the same name exists. If a view with the name already exists, the statement succeeds silently without modifying it.

All three variants work with both the ``AS`` keyword body and the ``FROM YAML`` / ``FROM YAML FILE`` body.

.. note::

   Since v0.8.0 all four ``CREATE`` body variants participate in your surrounding transaction. ``BEGIN ... ROLLBACK`` discards an uncommitted ``CREATE``. See :ref:`explanation-transactional-ddl`.

.. note::

   ``CREATE SEMANTIC VIEW IF NOT EXISTS`` reliably absorbs duplicates within a single process or transaction (re-running a setup script, repeated statements in the same ``BEGIN`` block). It cannot absorb a race between two separate processes that both run ``CREATE IF NOT EXISTS`` against the same database at the same time -- one will succeed and the other will see a constraint error. Concurrent DDL across processes is unusual for typical DuckDB workloads, so most users will never hit this. See :ref:`explanation-txn-ddl-create-race` for the workaround.


.. _ref-create-clauses:

Clauses
=======

Clauses must appear in the following order: ``TABLES``, ``RELATIONSHIPS``, ``FACTS``, ``DIMENSIONS``, ``METRICS``, ``MATERIALIZATIONS``. ``TABLES`` is required. At least one of ``DIMENSIONS`` or ``METRICS`` is required. All other clauses are optional.


.. _ref-create-tables:

TABLES
------

Declares the physical tables available to the semantic view. Each entry assigns an alias and maps it to a physical table. A primary key declaration is optional but recommended for multi-table views (it drives JOIN synthesis and cardinality inference).

.. code-block:: sql

   TABLES (
       o AS orders    PRIMARY KEY (id) COMMENT = 'Order transactions',
       c AS customers PRIMARY KEY (id) WITH SYNONYMS = ('clients', 'buyers')
   )

**Parameters:**

- ``<alias>``, a short name used to reference this table in all other clauses.
- ``<table_name>``, the physical table name. Supports catalog-qualified names (``catalog.schema.table``).
- ``PRIMARY KEY (<column>, ...)``, optional. One or more columns forming the table's primary key. Used for JOIN synthesis and cardinality inference. This is semantic metadata, not a DuckDB constraint. Omit for fact tables that do not need to be join targets.
- ``COMMENT = '<text>'``, optional. A human-readable description of the table.
- ``WITH SYNONYMS = ('<synonym>', ...)``, optional. Alternative names for discoverability. Must come after COMMENT if both are present.

**Optional: UNIQUE constraints:**

.. code-block:: sql

   TABLES (
       o AS orders PRIMARY KEY (id) UNIQUE (email)
   )

``UNIQUE (<column>, ...)`` declares additional unique constraints. Used for cardinality inference: if a relationship's FK columns match a UNIQUE constraint, the relationship is inferred as one-to-one.

The first table in the ``TABLES`` clause is the **base table** (the root of the relationship graph). All other tables must be reachable from the base table through declared relationships.


.. _ref-create-relationships:

RELATIONSHIPS
-------------

Declares FK/PK join paths between tables. Each entry names a relationship, specifies the FK columns on the "from" side, and references the target table alias.

.. code-block:: sql

   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c,
       order_product  AS o(product_id)  REFERENCES p
   )

**Parameters:**

- ``<rel_name>``, a unique name identifying this relationship.
- ``<from_alias>``, the table alias containing the FK columns.
- ``(<fk_column>, ...)``, one or more FK column names on the "from" table.
- ``REFERENCES <to_alias> [(<ref_column>, ...)]``, the target table alias. Optionally specify which columns on the target table to join against. If omitted, the target's ``PRIMARY KEY`` columns are used. The JOIN ON clause is synthesized as ``from_alias.fk_column = to_alias.ref_column``.

**Cardinality inference:**

The extension infers cardinality from the "from" table's constraints:

- If the FK columns match a ``PRIMARY KEY`` or ``UNIQUE`` constraint on the "from" table, the relationship is **one-to-one**.
- Otherwise, the relationship is **many-to-one** (the default).

Cardinality is used for :ref:`fan trap detection <howto-fan-traps>`.

**Validation rules:**

- The relationship graph must form a tree rooted at the base table.
- Cycles are rejected.
- Diamond patterns (multiple paths to the same table) are rejected unless all paths use named relationships (role-playing pattern).
- Self-references (``from_alias`` equals ``to_alias``) are rejected.
- Orphan tables (declared in ``TABLES`` but not reachable via relationships) are rejected in multi-table views.


.. _ref-create-facts:

FACTS
-----

Declares named row-level expressions. Facts are inlined into metric expressions at expansion time.

.. code-block:: sql

   FACTS (
       li.net_price  AS li.extended_price * (1 - li.discount)
           COMMENT = 'Price after discount',
       PRIVATE li.internal_cost AS li.unit_cost * li.quantity,
       li.tax_amount AS li.net_price * li.tax_rate
           WITH SYNONYMS = ('tax', 'vat')
   )

**Parameters:**

- ``PRIVATE``, optional. When present, the fact cannot be queried directly via ``facts := [...]`` but can still be referenced in metric expressions.
- ``<alias>.<fact_name>``, the table alias and fact name. Facts are scoped to a single table.
- ``<row_level_expression>``, any SQL expression that operates on individual rows. Must not contain aggregate functions.
- ``COMMENT = '<text>'``, optional. A human-readable description.
- ``WITH SYNONYMS = ('<synonym>', ...)``, optional. Alternative names for discoverability.

**Fact chaining:**

Facts can reference other facts by name. The extension resolves dependencies in topological order and inlines them recursively. Circular references are rejected at define time.

**Validation rules:**

- Aggregate functions (``SUM``, ``COUNT``, ``AVG``, ``MIN``, ``MAX``, etc.) in fact expressions are rejected.
- Circular fact references are rejected.


.. _ref-create-dimensions:

DIMENSIONS
----------

Declares named grouping expressions available for queries.

.. code-block:: sql

   DIMENSIONS (
       o.region   AS o.region COMMENT = 'Sales region',
       o.category AS o.category WITH SYNONYMS = ('product_category'),
       o.month    AS date_trunc('month', o.ordered_at)
   )

**Parameters:**

- ``<alias>.<dim_name>``, the table alias and dimension name. The alias indicates which table the dimension comes from (used for join dependency resolution).
- ``<expression>``, any SQL expression. Can be a simple column reference (``o.region``) or a computed expression (``date_trunc('month', o.ordered_at)``).
- ``COMMENT = '<text>'``, optional. A human-readable description.
- ``WITH SYNONYMS = ('<synonym>', ...)``, optional. Alternative names for discoverability.

**Validation rules:**

- Dimension names must be unique within the view (case-insensitive). For example, ``region`` and ``Region`` cannot both appear in the same ``DIMENSIONS`` clause. See :ref:`ref-err-name-uniqueness`.
- A dimension name cannot collide with any metric name (case-insensitive). See :ref:`ref-err-name-uniqueness`.

**Type inference:**

When creating a semantic view on a file-backed database, the extension infers the ``DATA_TYPE`` for each dimension at define time. It executes a ``LIMIT 0`` query against the underlying tables to detect the output type. Most scalar types are inferred, including ``VARCHAR``, ``INTEGER``, ``BIGINT``, ``DOUBLE``, ``DATE``, ``TIMESTAMP``, ``BOOLEAN``, ``FLOAT``, ``UUID``, and others. Types where inference would be lossy, such as ``DECIMAL``, ``LIST``, and ``ARRAY``, show an empty data type.

The inferred type is visible in :ref:`SHOW SEMANTIC DIMENSIONS <ref-show-semantic-dimensions>` (``data_type`` column) and :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` (``output_type`` field in the JSON).

.. note::

   Type inference requires a file-backed database. In-memory databases skip inference, leaving ``DATA_TYPE`` empty.


.. _ref-create-metrics:

METRICS
-------

Declares named aggregation expressions, derived metrics, semi-additive metrics, and window metrics.

**Base metrics** (with table alias, containing aggregate functions):

.. code-block:: sql

   METRICS (
       o.revenue     AS SUM(o.amount) COMMENT = 'Total revenue',
       o.order_count AS COUNT(*) WITH SYNONYMS = ('num_orders')
   )

**Derived metrics** (no table alias, referencing other metric names):

.. code-block:: sql

   METRICS (
       li.revenue AS SUM(li.net_price),
       li.cost    AS SUM(li.unit_cost),
       profit     AS revenue - cost,
       margin     AS profit / revenue * 100
   )

**USING clause** (for role-playing dimensions):

.. code-block:: sql

   METRICS (
       f.departures USING (dep_airport) AS COUNT(*),
       f.arrivals   USING (arr_airport) AS COUNT(*)
   )

**PRIVATE metrics:**

.. code-block:: sql

   METRICS (
       PRIVATE o.raw_total AS SUM(o.amount),
       net_total AS raw_total * 0.9
   )

**Semi-additive metrics** (with ``NON ADDITIVE BY``):

.. code-block:: sql

   METRICS (
       a.total_balance AS SUM(a.balance)
           NON ADDITIVE BY (report_date DESC NULLS FIRST)
   )

When a query does not include the non-additive dimension, the extension generates a CTE with ``ROW_NUMBER`` to select one snapshot row per group before aggregating. When the non-additive dimension is included in the query, the metric behaves as a standard additive metric.

See :ref:`howto-semi-additive` for details.

**Window metrics** (with ``OVER`` clause):

Window metrics wrap another metric in a SQL window function. The ``OVER`` clause supports two mutually exclusive partitioning modes:

``PARTITION BY EXCLUDING`` computes the partition set dynamically at query time as "all queried dimensions minus the excluded ones." The partition changes depending on which dimensions are requested:

.. code-block:: sql

   METRICS (
       s.total_qty AS SUM(s.quantity),
       s.rolling_avg AS
           AVG(total_qty) OVER (PARTITION BY EXCLUDING date
               ORDER BY date NULLS LAST
               RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)
   )

``PARTITION BY`` (without ``EXCLUDING``) specifies an explicit, fixed set of partition dimensions. The partition set is always exactly the listed dimensions, regardless of what other dimensions are queried:

.. code-block:: sql

   METRICS (
       s.total_qty AS SUM(s.quantity),
       s.store_avg AS
           AVG(total_qty) OVER (PARTITION BY store ORDER BY date NULLS LAST)
   )

See :ref:`howto-window-metrics` for details on both modes.

.. warning::

   ``NON ADDITIVE BY`` and ``OVER`` cannot be combined on the same metric. A metric is either semi-additive or a window metric, not both.

**Parameters:**

- ``PRIVATE``, optional. When present, the metric cannot be queried directly but can be referenced by derived metric expressions.
- ``<alias>.<metric_name>``, table alias and metric name for base metrics.
- ``<metric_name>``, name only (no alias) for derived metrics.
- ``USING (<rel_name>, ...)``, optional. Specifies which named relationship(s) this metric traverses. Used to disambiguate when a dimension comes from a role-playing table.
- ``NON ADDITIVE BY (<dim_name> [ASC|DESC] [NULLS FIRST|NULLS LAST], ...)``, optional. Declares the metric as semi-additive, specifying which dimensions trigger snapshot selection.
- ``<aggregate_expression>``, for base metrics: any expression containing aggregate functions.
- ``<expression>``, for derived metrics: an expression referencing other metric names (no aggregate functions).
- ``<window_func>(<inner_metric>, ...) OVER (...)``, for window metrics: wraps another metric in a window function.
- ``COMMENT = '<text>'``, optional. A human-readable description.
- ``WITH SYNONYMS = ('<synonym>', ...)``, optional. Alternative names for discoverability.

**Validation rules:**

- Metric names must be unique within the view (case-insensitive), across both base and derived metrics. See :ref:`ref-err-name-uniqueness`.
- A metric name cannot collide with any dimension name (case-insensitive). See :ref:`ref-err-name-uniqueness`.
- Derived metrics must not contain aggregate functions.
- Circular derived metric references are rejected.
- ``USING`` relationship names must match declared relationships.
- ``NON ADDITIVE BY`` dimension names must match declared dimensions.
- Window metric inner metric name must match a declared metric.
- Window metric ``EXCLUDING`` dimension names must match declared dimensions.
- Window metric ``PARTITION BY`` dimension names must match declared dimensions.
- Window metric ``ORDER BY`` dimension names must match declared dimensions.
- ``NON ADDITIVE BY`` and ``OVER`` cannot both appear on the same metric.
- ``OVER`` cannot appear on a derived metric (one without a table alias). Only qualified metrics (``alias.name``) can use ``OVER``.


.. _ref-create-materializations:

MATERIALIZATIONS
-----------------

.. versionadded:: 0.7.0

Declares named materializations that map pre-aggregated tables to the dimensions and metrics they cover. When a query's requested dimensions and metrics exactly match a materialization, the extension routes to the pre-aggregated table instead of expanding raw sources.

.. code-block:: sql

   MATERIALIZATIONS (
       region_agg AS (
           TABLE daily_revenue_by_region,
           DIMENSIONS (region),
           METRICS (revenue, order_count)
       ),
       global_agg AS (
           TABLE global_totals,
           METRICS (revenue)
       )
   )

**Parameters:**

- ``<mat_name>``, a unique name identifying this materialization.
- ``TABLE <table_name>``, the physical table containing pre-aggregated data. Supports catalog-qualified names (``catalog.schema.table``). The table is not validated for existence at define time (it may be created later by external tools like dbt).
- ``DIMENSIONS (<dim_name>, ...)``, optional. Dimension names from the view's ``DIMENSIONS`` clause that the materialization table covers.
- ``METRICS (<metric_name>, ...)``, optional. Metric names from the view's ``METRICS`` clause that the materialization table covers.

At least one of ``DIMENSIONS`` or ``METRICS`` must be specified in each materialization entry.

**Routing behavior:**

- Routing uses **exact match**: both the dimension set and metric set must exactly equal the query's requested sets (case-insensitive comparison).
- Materializations are scanned in **definition order**; the **first match wins**.
- Semi-additive metrics (``NON ADDITIVE BY``) and window metrics (``OVER``) are **always excluded** from routing.
- When no match is found, the extension falls back to standard expansion.

See :ref:`howto-materializations` for a detailed guide.

**Validation rules:**

- Materialization names must be unique within a view.
- Dimension names must match declared dimensions in the view's ``DIMENSIONS`` clause.
- Metric names must match declared metrics in the view's ``METRICS`` clause.
- Each materialization must specify at least one of ``DIMENSIONS`` or ``METRICS``.


.. _ref-create-from-yaml:

FROM YAML
---------

.. versionadded:: 0.7.0

Creates a semantic view from a YAML definition instead of the keyword-based ``AS`` body. Two forms are supported:

**Inline YAML (dollar-quoted):**

.. code-block:: duckdb-sql

   CREATE SEMANTIC VIEW order_metrics FROM YAML $$
   tables:
     - alias: o
       table: orders
       pk_columns:
         - id
   dimensions:
     - name: region
       expr: o.region
       source_table: o
   metrics:
     - name: revenue
       expr: SUM(o.amount)
       source_table: o
   $$

The YAML content is enclosed in dollar-quote delimiters. Tagged dollar-quoting (``$yaml$...$yaml$``) is also supported.

**YAML from file:**

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics FROM YAML FILE '/path/to/definition.yaml'

The file path must be single-quoted. DuckDB reads the file contents and parses as YAML.

See :ref:`howto-yaml-definitions` for a detailed workflow guide.

**YAML size limit:** YAML definitions are capped at 1 MiB. Definitions exceeding this limit are rejected with an error.

**Type inference:**

As with dimensions, the extension infers ``DATA_TYPE`` for each metric at define time on file-backed databases. ``COUNT(*)`` infers as ``BIGINT``, ``SUM`` on integer columns infers as ``BIGINT``, ``SUM`` on float/double columns infers as ``DOUBLE``, and so on. The inferred type is visible in :ref:`SHOW SEMANTIC METRICS <ref-show-semantic-metrics>` and :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>`.


.. _ref-create-examples:

Examples
========

**Single-table view:**

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   );

**Multi-table star schema:**

.. code-block:: sql

   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       o AS orders    PRIMARY KEY (id),
       c AS customers PRIMARY KEY (id),
       p AS products  PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c,
       order_product  AS o(product_id)  REFERENCES p
   )
   DIMENSIONS (
       c.customer AS c.name,
       p.product  AS p.name,
       o.region   AS o.region
   )
   METRICS (
       o.revenue     AS SUM(o.amount),
       o.order_count AS COUNT(*)
   );

**Full feature set (facts, derived metrics, USING):**

.. code-block:: sql

   CREATE SEMANTIC VIEW flight_analytics AS
   TABLES (
       f AS flights  PRIMARY KEY (flight_id),
       a AS airports PRIMARY KEY (airport_code)
   )
   RELATIONSHIPS (
       dep_airport AS f(departure_code) REFERENCES a,
       arr_airport AS f(arrival_code)   REFERENCES a
   )
   FACTS (
       f.is_international AS CASE WHEN f.departure_country != f.arrival_country
                             THEN 1 ELSE 0 END
   )
   DIMENSIONS (
       a.city    AS a.city,
       f.carrier AS f.carrier
   )
   METRICS (
       f.departures    USING (dep_airport) AS COUNT(*),
       f.arrivals      USING (arr_airport) AS COUNT(*),
       total_flights   AS departures + arrivals
   );

**With metadata annotations:**

.. code-block:: sql

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id) COMMENT = 'Transaction line items'
   )
   FACTS (
       li.net_price AS li.extended_price * (1 - li.discount)
           COMMENT = 'Price after discount' WITH SYNONYMS = ('discounted_price')
   )
   DIMENSIONS (
       li.region AS li.region COMMENT = 'Sales territory'
   )
   METRICS (
       li.total_net AS SUM(li.net_price) COMMENT = 'Net revenue'
   );

**Semi-additive metric:**

.. code-block:: sql

   CREATE SEMANTIC VIEW account_metrics AS
   TABLES (
       a AS accounts PRIMARY KEY (id)
   )
   DIMENSIONS (
       a.customer_id AS a.customer_id,
       a.report_date AS a.report_date
   )
   METRICS (
       a.total_balance AS SUM(a.balance)
           NON ADDITIVE BY (report_date DESC NULLS FIRST)
   );

**Window metric with PARTITION BY EXCLUDING:**

.. code-block:: sql

   CREATE SEMANTIC VIEW store_analytics AS
   TABLES (
       s AS sales PRIMARY KEY (id)
   )
   DIMENSIONS (
       s.store AS s.store,
       s.date  AS s.sale_date
   )
   METRICS (
       s.total_qty AS SUM(s.quantity),
       s.rolling_avg AS
           AVG(total_qty) OVER (PARTITION BY EXCLUDING date
               ORDER BY date NULLS LAST)
   );

**Window metric with explicit PARTITION BY:**

.. code-block:: sql

   CREATE SEMANTIC VIEW store_analytics AS
   TABLES (
       s AS sales PRIMARY KEY (id)
   )
   DIMENSIONS (
       s.store  AS s.store,
       s.date   AS s.sale_date,
       s.region AS s.region
   )
   METRICS (
       s.total_qty AS SUM(s.quantity),
       s.store_avg AS
           AVG(total_qty) OVER (PARTITION BY store
               ORDER BY date NULLS LAST)
   );

**With materializations:**

.. versionadded:: 0.7.0

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region,
       o.status AS o.status
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
       ),
       region_status_agg AS (
           TABLE revenue_by_region_status,
           DIMENSIONS (region, status),
           METRICS (revenue)
       )
   );

**From inline YAML:**

.. versionadded:: 0.7.0

.. code-block:: duckdb-sql

   CREATE SEMANTIC VIEW order_metrics FROM YAML $$
   tables:
     - alias: o
       table: orders
       pk_columns:
         - id
   dimensions:
     - name: region
       expr: o.region
       source_table: o
   metrics:
     - name: revenue
       expr: SUM(o.amount)
       source_table: o
   $$

**From YAML file:**

.. versionadded:: 0.7.0

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics FROM YAML FILE '/path/to/definition.yaml'
