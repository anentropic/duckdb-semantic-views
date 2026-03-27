.. meta::
   :description: Full syntax and parameter reference for CREATE SEMANTIC VIEW, covering TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, and METRICS clauses

.. _ref-create-semantic-view:

======================
CREATE SEMANTIC VIEW
======================

Creates a semantic view definition, registering dimensions, metrics, relationships, and facts for on-demand query expansion.


.. _ref-create-syntax:

Syntax
======

.. code-block:: sqlgrammar

   CREATE [ OR REPLACE ] SEMANTIC VIEW [ IF NOT EXISTS ] <name> AS
   TABLES (
       <alias> AS <table_name>
           [ PRIMARY KEY ( <column> [, <column> ...] ) ]
           [ UNIQUE ( <column> [, <column> ...] ) ]
       [, ... ]
   )
   [ RELATIONSHIPS (
       <rel_name> AS <from_alias>( <fk_column> [, <fk_column> ...] )
           REFERENCES <to_alias> [( <ref_column> [, <ref_column> ...] )]
       [, ... ]
   ) ]
   [ FACTS (
       <alias>.<fact_name> AS <row_level_expression>
       [, ... ]
   ) ]
   [ DIMENSIONS (
       <alias>.<dim_name> AS <expression>
       [, ... ]
   ) ]
   [ METRICS (
       <alias>.<metric_name> [ USING ( <rel_name> [, <rel_name> ...] ) ] AS <aggregate_expression>
       [, <metric_name> AS <expression> ... ]
   ) ]


.. _ref-create-variants:

Statement Variants
==================

``CREATE SEMANTIC VIEW <name> AS ...``
   Creates a new semantic view. Returns an error if a view with the same name already exists.

``CREATE OR REPLACE SEMANTIC VIEW <name> AS ...``
   Creates or replaces an existing semantic view with the same name. If the view does not exist, creates it. If it does, replaces the definition.

``CREATE SEMANTIC VIEW IF NOT EXISTS <name> AS ...``
   Creates a new semantic view only if no view with the same name exists. If a view with the name already exists, the statement succeeds silently without modifying it.


.. _ref-create-clauses:

Clauses
=======

Clauses must appear in the following order: ``TABLES``, ``RELATIONSHIPS``, ``FACTS``, ``DIMENSIONS``, ``METRICS``. ``TABLES`` is required. At least one of ``DIMENSIONS`` or ``METRICS`` is required. All other clauses are optional.


.. _ref-create-tables:

TABLES
------

Declares the physical tables available to the semantic view. Each entry assigns an alias and maps it to a physical table. A primary key declaration is optional but recommended for multi-table views (it drives JOIN synthesis and cardinality inference).

.. code-block:: sql

   TABLES (
       o AS orders    PRIMARY KEY (id),
       c AS customers PRIMARY KEY (id)
   )

**Parameters:**

- ``<alias>``, a short name used to reference this table in all other clauses.
- ``<table_name>``, the physical table name. Supports catalog-qualified names (``catalog.schema.table``).
- ``PRIMARY KEY (<column>, ...)``, optional. One or more columns forming the table's primary key. Used for JOIN synthesis and cardinality inference. This is semantic metadata, not a DuckDB constraint. Omit for fact tables that do not need to be join targets.

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
       li.net_price  AS li.extended_price * (1 - li.discount),
       li.tax_amount AS li.net_price * li.tax_rate
   )

**Parameters:**

- ``<alias>.<fact_name>``, the table alias and fact name. Facts are scoped to a single table.
- ``<row_level_expression>``, any SQL expression that operates on individual rows. Must not contain aggregate functions.

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
       o.region   AS o.region,
       o.category AS o.category,
       o.month    AS date_trunc('month', o.ordered_at)
   )

**Parameters:**

- ``<alias>.<dim_name>``, the table alias and dimension name. The alias indicates which table the dimension comes from (used for join dependency resolution).
- ``<expression>``, any SQL expression. Can be a simple column reference (``o.region``) or a computed expression (``date_trunc('month', o.ordered_at)``).


.. _ref-create-metrics:

METRICS
-------

Declares named aggregation expressions and derived metrics.

**Base metrics** (with table alias, containing aggregate functions):

.. code-block:: sql

   METRICS (
       o.revenue     AS SUM(o.amount),
       o.order_count AS COUNT(*)
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

**Parameters:**

- ``<alias>.<metric_name>``, table alias and metric name for base metrics.
- ``<metric_name>``, name only (no alias) for derived metrics.
- ``USING (<rel_name>, ...)``, optional. Specifies which named relationship(s) this metric traverses. Used to disambiguate when a dimension comes from a role-playing table.
- ``<aggregate_expression>``, for base metrics: any expression containing aggregate functions.
- ``<expression>``, for derived metrics: an expression referencing other metric names (no aggregate functions).

**Validation rules:**

- Derived metrics must not contain aggregate functions.
- Circular derived metric references are rejected.
- ``USING`` relationship names must match declared relationships.


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
