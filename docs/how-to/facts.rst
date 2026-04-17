.. meta::
   :description: Define named row-level expressions in the FACTS clause that metrics can reference, query facts directly, and annotate facts with metadata

.. _howto-facts:

==============================================
How to Use FACTS for Reusable Row-Level Logic
==============================================

This guide shows how to use the ``FACTS`` clause to define reusable row-level expressions that metrics can reference. FACTS eliminate duplicated calculations across metrics and support chaining (one fact referencing another). Facts can also be queried directly as row-level columns and annotated with comments, synonyms, and access modifiers.

**Prerequisites:**

- A working semantic view with ``TABLES``, ``DIMENSIONS``, and ``METRICS`` (see :ref:`tutorial-multi-table`)
- Understanding of aggregate vs. row-level expressions in SQL


.. _howto-facts-basic:

Define a Basic Fact
===================

A fact is a named row-level expression scoped to a table alias. Unlike metrics, facts do not contain aggregate functions. They compute a value for each row.

.. code-block:: sql
   :emphasize-lines: 8

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id)
   )
   FACTS (
       li.net_price AS li.extended_price * (1 - li.discount)
   )
   DIMENSIONS (
       li.region AS li.region
   )
   METRICS (
       li.total_net AS SUM(li.net_price)
   );

The metric ``total_net`` references the fact ``net_price``. At expansion time, the extension inlines the fact expression into the metric: ``SUM(li.extended_price * (1 - li.discount))``.


.. _howto-facts-chain:

Chain Facts Together
====================

Facts can reference other facts. The extension resolves them in dependency order (topological sort) and inlines them recursively.

.. code-block:: sql
   :emphasize-lines: 6,7

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id)
   )
   FACTS (
       li.net_price  AS li.extended_price * (1 - li.discount),
       li.tax_amount AS li.net_price * li.tax_rate
   )
   DIMENSIONS (
       li.region AS li.region
   )
   METRICS (
       li.total_net AS SUM(li.net_price),
       li.total_tax AS SUM(li.tax_amount)
   );

Here ``tax_amount`` references ``net_price``. The extension resolves the chain:

1. ``net_price`` = ``li.extended_price * (1 - li.discount)``
2. ``tax_amount`` = ``(li.extended_price * (1 - li.discount)) * li.tax_rate``

Both metrics receive the fully inlined expressions.


.. _howto-facts-multi-table:

Use Facts in Multi-Table Views
==============================

Facts are scoped to their table alias. In a multi-table view, each fact references columns from its own table.

.. code-block:: sql

   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       li AS line_items PRIMARY KEY (id),
       o  AS orders      PRIMARY KEY (id),
       c  AS customers   PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       li_to_order       AS li(order_id)    REFERENCES o,
       order_to_customer AS o(customer_id)  REFERENCES c
   )
   FACTS (
       li.net_price  AS li.extended_price * (1 - li.discount),
       li.tax_amount AS li.net_price * li.tax_rate
   )
   DIMENSIONS (
       o.region  AS o.region,
       c.country AS c.country
   )
   METRICS (
       li.total_net AS SUM(li.net_price),
       li.total_tax AS SUM(li.tax_amount)
   );

The facts are still scoped to ``li`` (line_items), but the dimensions come from ``o`` (orders) and ``c`` (customers). The extension joins all necessary tables based on what the query requests.


.. _howto-facts-query:

Query Facts Directly
====================

.. versionadded:: 0.6.0

Facts can be queried as row-level columns using the ``facts`` parameter in :ref:`semantic_view() <ref-semantic-view-function>`. Unlike metric queries, fact queries return individual rows without aggregation.

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       facts := ['net_price', 'tax_amount']
   );

Each row in the result contains the computed fact values. Dimensions can be included alongside facts -- they appear as columns but do not trigger ``GROUP BY``:

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       dimensions := ['region'],
       facts := ['net_price']
   );

.. warning::

   Facts and metrics cannot be combined in the same query. Use ``facts := [...]`` OR ``metrics := [...]``, not both. Attempting to mix them produces an error.

For a complete guide to fact queries, including wildcard selection and troubleshooting, see :ref:`howto-query-facts`.


.. _howto-facts-annotations:

Annotate Facts with Metadata
==============================

.. versionadded:: 0.6.0

Facts support the same metadata annotations as dimensions and metrics: ``COMMENT``, ``WITH SYNONYMS``, and ``PRIVATE``/``PUBLIC`` access modifiers.

.. code-block:: sql
   :emphasize-lines: 6,7

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id)
   )
   FACTS (
       li.net_price  AS li.extended_price * (1 - li.discount) COMMENT = 'Price after discount',
       PRIVATE li.raw_margin AS li.price - li.cost WITH SYNONYMS = ('margin', 'gross_margin')
   )
   DIMENSIONS (
       li.region AS li.region
   )
   METRICS (
       li.total_net AS SUM(li.net_price),
       profit_margin AS total_net - SUM(li.raw_margin)
   );

- ``COMMENT`` adds a human-readable description, visible in ``DESCRIBE SEMANTIC VIEW`` and ``SHOW SEMANTIC FACTS`` output.
- ``WITH SYNONYMS`` adds informational alternative names for discoverability.
- ``PRIVATE`` prevents a fact from being queried directly via ``facts := [...]``, while still allowing it to be referenced in metric expressions. Private facts are also excluded from wildcard expansion (``alias.*``).

For more on metadata annotations, see :ref:`howto-metadata-annotations`.


.. _howto-facts-verify:

Verify the Inlined SQL
======================

Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to confirm that fact expressions are inlined correctly:

.. code-block:: sql

   SELECT * FROM explain_semantic_view('analytics',
       dimensions := ['region'],
       metrics := ['total_net']
   );

The expanded SQL shows the fully inlined expression in the SELECT clause, with no reference to the fact name.


.. _howto-facts-errors:

Troubleshooting
===============

**Circular fact references**
   Facts that reference each other in a cycle cause a define-time error. The extension
   detects cycles during ``CREATE SEMANTIC VIEW`` and reports which facts are involved.

**Aggregate functions in facts**
   Facts must be row-level expressions. Using an aggregate function like ``SUM()`` or
   ``COUNT()`` in a fact expression causes a define-time error. Aggregation belongs in
   the ``METRICS`` clause.

**Fact name not found**
   If a metric references a fact name that does not exist, the extension treats it as a
   regular column reference. If the column also does not exist, the query fails with a
   DuckDB column-not-found error. Double-check fact names match exactly.

**Private fact cannot be queried**
   Facts marked ``PRIVATE`` cannot be queried via ``facts := [...]``. They return an
   error: ``fact '<name>' is private and cannot be queried directly``. Remove the
   ``PRIVATE`` keyword to make a fact queryable.
