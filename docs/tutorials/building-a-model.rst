.. meta::
   :description: Build a realistic analytics model with facts and derived metrics, learning how to refactor duplicated logic and compose calculations

.. _tutorial-building-model:

=========================
Building a Complete Model
=========================

In this tutorial, you will extend a multi-table semantic view with facts and derived metrics. Starting from a basic view with simple metrics, you will identify duplicated calculations, extract them into reusable facts, and compose higher-level metrics from lower-level ones. By the end, you will understand the full modeling workflow: tables, relationships, facts, base metrics, and derived metrics working together.

**Time:** 15 minutes

**Prerequisites:**

- Completed the :ref:`tutorial-multi-table` tutorial
- Familiarity with row-level vs. aggregate expressions in SQL


.. _tutorial-bm-create-tables:

Create the Schema
=================

Create an e-commerce schema with three tables: ``orders`` (header table), ``customers`` (dimension table), and ``line_items`` (fact table with pricing detail).

.. code-block:: sql

   CREATE TABLE customers (id INTEGER, name VARCHAR, city VARCHAR);
   INSERT INTO customers VALUES
       (1, 'Alice', 'Portland'),
       (2, 'Bob',   'Seattle'),
       (3, 'Carol', 'Portland');

   CREATE TABLE orders (
       id INTEGER, customer_id INTEGER, ordered_at DATE
   );
   INSERT INTO orders VALUES
       (1, 1, '2024-01-15'),
       (2, 1, '2024-01-20'),
       (3, 2, '2024-02-10'),
       (4, 3, '2024-03-01');

   CREATE TABLE line_items (
       id INTEGER, order_id INTEGER,
       price DECIMAL(10,2), quantity INTEGER,
       discount DECIMAL(4,2), unit_cost DECIMAL(10,2)
   );
   INSERT INTO line_items VALUES
       (1, 1, 25.00, 2, 0.00,  10.00),
       (2, 1, 40.00, 1, 0.10,  15.00),
       (3, 2, 50.00, 3, 0.00,  20.00),
       (4, 3, 30.00, 1, 0.20,  12.00),
       (5, 3, 80.00, 2, 0.05,  35.00),
       (6, 4, 60.00, 1, 0.00,  25.00);

The ``line_items`` table has the pricing detail: ``price`` is the unit price, ``quantity`` is the number of units, ``discount`` is a fractional discount (0.10 = 10%), and ``unit_cost`` is the per-unit cost to the business.


.. _tutorial-bm-basic-view:

Start with a Basic View
=======================

Define a semantic view with simple metrics. This is a starting point that you will refactor as patterns emerge.

.. code-block:: sql

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       o  AS orders     PRIMARY KEY (id),
       c  AS customers  PRIMARY KEY (id),
       li AS line_items PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c,
       order_items    AS li(order_id)   REFERENCES o
   )
   DIMENSIONS (
       c.customer AS c.name,
       c.city     AS c.city,
       o.month    AS date_trunc('month', o.ordered_at)
   )
   METRICS (
       li.revenue AS SUM(li.price * li.quantity * (1 - li.discount)),
       li.cost    AS SUM(li.unit_cost * li.quantity)
   );

Query the view to verify it works:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['customer'],
       metrics := ['revenue', 'cost']
   ) ORDER BY revenue DESC;

.. code-block:: text

   ┌──────────┬─────────┬────────┐
   │ customer │ revenue │  cost  │
   ├──────────┼─────────┼────────┤
   │ Bob      │  178.00 │  82.00 │
   │ Alice    │  236.00 │  95.00 │
   │ Carol    │   60.00 │  25.00 │
   └──────────┴─────────┴────────┘

The metrics work, but look at the expressions. ``li.price * li.quantity * (1 - li.discount)`` computes the net line total, and ``li.unit_cost * li.quantity`` computes the total cost. If you added more metrics (average net price, gross total before discount), you would repeat these row-level calculations in each metric expression.


.. _tutorial-bm-add-facts:

Extract Repeated Logic into Facts
==================================

The ``FACTS`` clause lets you name row-level expressions that metrics can reference. A fact computes a value per row, without aggregation. Metrics then aggregate facts instead of repeating the calculation.

Refactor the view to extract two facts:

.. code-block:: sql
   :emphasize-lines: 11-14

   CREATE OR REPLACE SEMANTIC VIEW sales AS
   TABLES (
       o  AS orders     PRIMARY KEY (id),
       c  AS customers  PRIMARY KEY (id),
       li AS line_items PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c,
       order_items    AS li(order_id)   REFERENCES o
   )
   FACTS (
       li.net_total AS li.price * li.quantity * (1 - li.discount),
       li.line_cost AS li.unit_cost * li.quantity
   )
   DIMENSIONS (
       c.customer AS c.name,
       c.city     AS c.city,
       o.month    AS date_trunc('month', o.ordered_at)
   )
   METRICS (
       li.revenue AS SUM(li.net_total),
       li.cost    AS SUM(li.line_cost)
   );

The ``FACTS`` clause appears between ``RELATIONSHIPS`` and ``DIMENSIONS``. Each fact follows the same ``alias.name AS expression`` pattern as dimensions and metrics.

Now the metrics read as ``SUM(li.net_total)`` and ``SUM(li.line_cost)`` instead of embedding the full row-level calculation. When the extension expands a query, it inlines the fact expressions automatically. The query results are identical:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['customer'],
       metrics := ['revenue', 'cost']
   ) ORDER BY revenue DESC;

.. code-block:: text

   ┌──────────┬─────────┬────────┐
   │ customer │ revenue │  cost  │
   ├──────────┼─────────┼────────┤
   │ Bob      │  178.00 │  82.00 │
   │ Alice    │  236.00 │  95.00 │
   │ Carol    │   60.00 │  25.00 │
   └──────────┴─────────┴────────┘


.. _tutorial-bm-derived-metrics:

Add Derived Metrics
===================

A derived metric references other metrics by name instead of writing an aggregate expression. Derived metrics have no table alias prefix and no aggregate function. They let you compose calculations like profit and margin without repeating the aggregation logic.

Add ``profit`` and ``margin`` to the view:

.. code-block:: sql
   :emphasize-lines: 24,25

   CREATE OR REPLACE SEMANTIC VIEW sales AS
   TABLES (
       o  AS orders     PRIMARY KEY (id),
       c  AS customers  PRIMARY KEY (id),
       li AS line_items PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c,
       order_items    AS li(order_id)   REFERENCES o
   )
   FACTS (
       li.net_total AS li.price * li.quantity * (1 - li.discount),
       li.line_cost AS li.unit_cost * li.quantity
   )
   DIMENSIONS (
       c.customer AS c.name,
       c.city     AS c.city,
       o.month    AS date_trunc('month', o.ordered_at)
   )
   METRICS (
       li.revenue AS SUM(li.net_total),
       li.cost    AS SUM(li.line_cost),
       profit     AS revenue - cost,
       margin     AS profit / revenue * 100
   );

Notice the pattern:

- ``profit`` references ``revenue`` and ``cost`` by name. It has no alias prefix and no aggregate function.
- ``margin`` references ``profit``, which itself references ``revenue`` and ``cost``. The extension resolves the full chain.

Query the complete model:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['customer'],
       metrics := ['revenue', 'cost', 'profit', 'margin']
   ) ORDER BY margin DESC;

.. code-block:: text

   ┌──────────┬─────────┬────────┬────────┬───────────────────┐
   │ customer │ revenue │  cost  │ profit │      margin       │
   ├──────────┼─────────┼────────┼────────┼───────────────────┤
   │ Alice    │  236.00 │  95.00 │ 141.00 │ 59.74576271186440 │
   │ Bob      │  178.00 │  82.00 │  96.00 │ 53.93258426966292 │
   │ Carol    │   60.00 │  25.00 │  35.00 │ 58.33333333333300 │
   └──────────┴─────────┴────────┴────────┴───────────────────┘

You can query derived metrics with any dimension, just like base metrics. The extension includes all the tables needed to compute the underlying aggregations:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['month'],
       metrics := ['revenue', 'profit', 'margin']
   ) ORDER BY month;

.. code-block:: text

   ┌────────────┬─────────┬────────┬───────────────────┐
   │   month    │ revenue │ profit │      margin       │
   ├────────────┼─────────┼────────┼───────────────────┤
   │ 2024-01-01 │  236.00 │ 141.00 │ 59.74576271186440 │
   │ 2024-02-01 │  178.00 │  96.00 │ 53.93258426966292 │
   │ 2024-03-01 │   60.00 │  35.00 │ 58.33333333333300 │
   └────────────┴─────────┴────────┴───────────────────┘


.. _tutorial-bm-inspect:

Inspect the Generated SQL
=========================

Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to see how facts and derived metrics are expanded:

.. code-block:: sql

   SELECT * FROM explain_semantic_view('sales',
       dimensions := ['customer'],
       metrics := ['revenue', 'profit']
   );

The expanded SQL shows the full chain:

1. **Fact inlining** -- ``li.net_total`` is replaced with ``li.price * li.quantity * (1 - li.discount)`` inside the ``SUM()``.
2. **Derived metric expansion** -- ``profit`` is replaced with ``revenue - cost``, where ``revenue`` and ``cost`` are the aggregate expressions from step 1.
3. **Selective joining** -- only the tables needed for the requested dimensions and metrics are joined. If you remove ``customer`` from the query, the ``customers`` table is dropped from the generated SQL.

This is the core value of the modeling workflow: define facts once, compose metrics from them, and let the extension handle the SQL generation.


.. _tutorial-bm-cleanup:

Clean Up
========

.. code-block:: sql

   DROP SEMANTIC VIEW sales;
   DROP TABLE line_items;
   DROP TABLE orders;
   DROP TABLE customers;


.. _tutorial-bm-summary:

What You Learned
================

You now know how to:

- Define reusable row-level logic in the :ref:`FACTS <ref-create-facts>` clause
- Refactor duplicated calculations from metrics into facts
- Compose metrics from other metrics using :ref:`derived metrics <ref-create-metrics>`
- Stack derived metrics (``margin`` references ``profit`` which references ``revenue`` and ``cost``)
- Inspect fact inlining and metric expansion with :ref:`explain_semantic_view() <ref-explain-semantic-view>`

Next, explore the how-to guides for deeper coverage of these topics and more:

- :ref:`howto-facts` -- fact chaining, querying facts directly, fact metadata
- :ref:`howto-derived-metrics` -- stacking patterns, combining facts and derived metrics
- :ref:`howto-semi-additive` -- metrics for snapshot data like account balances
- :ref:`howto-window-metrics` -- rolling averages, rankings, and lag comparisons
- :ref:`howto-materializations` -- route queries to pre-aggregated tables
