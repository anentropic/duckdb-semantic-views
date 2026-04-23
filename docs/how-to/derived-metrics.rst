.. meta::
   :description: Compose metrics from other metrics using arithmetic expressions so you can compute profit, margin, and similar calculations without repeating aggregate logic

.. _howto-derived-metrics:

=============================================
How to Compose Metrics with Derived Metrics
=============================================

This guide shows how to define derived metrics, which combine other metrics by name rather than writing full aggregate expressions. Derived metrics enable calculations like profit (revenue minus cost) and margin (profit divided by revenue) without repeating the underlying aggregation logic.

**Prerequisites:**

- A working multi-table semantic view (see :ref:`tutorial-multi-table`, or :ref:`tutorial-building-model` for a guided introduction to derived metrics)
- Understanding of the ``FACTS`` clause (see :ref:`howto-facts`)


.. _howto-derived-basic:

Define a Derived Metric
=======================

A derived metric references other metric names instead of table columns. It has no table alias prefix and no aggregate function.

.. code-block:: sql
   :emphasize-lines: 13,14

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id),
       o  AS orders      PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       li_to_order AS li(order_id) REFERENCES o
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       li.revenue AS SUM(li.extended_price),
       li.cost    AS SUM(li.unit_cost),
       profit     AS revenue - cost,
       margin     AS profit / revenue * 100
   );

The key distinction:

- **Base metrics** (``li.revenue``, ``li.cost``) have a table alias prefix and contain aggregate functions.
- **Derived metrics** (``profit``, ``margin``) have no table alias prefix and reference other metric names.


.. _howto-derived-query:

Query Derived Metrics
=====================

Derived metrics work like any other metric in :ref:`semantic_view() <ref-semantic-view-function>`:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['region'],
       metrics := ['revenue', 'cost', 'profit']
   ) ORDER BY region;

.. code-block:: text

   ┌────────┬─────────┬────────┬────────┐
   │ region │ revenue │  cost  │ profit │
   ├────────┼─────────┼────────┼────────┤
   │ East   │  300.00 │ 130.00 │ 170.00 │
   │ West   │  150.00 │  60.00 │  90.00 │
   └────────┴─────────┴────────┴────────┘

You can also query a derived metric on its own. The extension resolves all underlying base metrics it depends on:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['region'],
       metrics := ['margin']
   ) ORDER BY region;


.. _howto-derived-stacking:

Stack Derived Metrics
=====================

Derived metrics can reference other derived metrics. The extension resolves the full dependency chain:

.. code-block:: sql

   METRICS (
       li.revenue AS SUM(li.extended_price),
       li.cost    AS SUM(li.unit_cost),
       profit     AS revenue - cost,
       margin     AS profit / revenue * 100
   );

Here ``margin`` references ``profit``, which in turn references ``revenue`` and ``cost``. The extension expands the chain:

- ``profit`` becomes ``SUM(li.extended_price) - SUM(li.unit_cost)``
- ``margin`` becomes ``(SUM(li.extended_price) - SUM(li.unit_cost)) / SUM(li.extended_price) * 100``


.. _howto-derived-with-facts:

Combine Facts and Derived Metrics
=================================

Facts and derived metrics work together. Facts provide reusable row-level expressions, base metrics aggregate them, and derived metrics compose the aggregates.

.. code-block:: sql

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id),
       o  AS orders      PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       li_to_order AS li(order_id) REFERENCES o
   )
   FACTS (
       li.net_price AS li.extended_price * (1 - li.discount)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       li.revenue AS SUM(li.net_price),
       li.cost    AS SUM(li.unit_cost),
       profit     AS revenue - cost,
       margin     AS profit / revenue * 100
   );

The full resolution chain:

1. ``net_price`` (fact) = ``li.extended_price * (1 - li.discount)``
2. ``revenue`` (base metric) = ``SUM(li.extended_price * (1 - li.discount))``
3. ``profit`` (derived) = ``SUM(...) - SUM(li.unit_cost)``
4. ``margin`` (derived) = ``(SUM(...) - SUM(...)) / SUM(...) * 100``


.. _howto-derived-errors:

Troubleshooting
===============

**Aggregate function in a derived metric**
   Derived metrics must not contain aggregate functions. The expression ``profit AS SUM(revenue)``
   is rejected at define time because ``SUM()`` is an aggregate. The correct form is
   ``profit AS revenue - cost`` (referencing metric names, not aggregating them).

**Circular derived metric references**
   Derived metrics that reference each other in a cycle cause a define-time error. For
   example, ``a AS b + 1`` and ``b AS a + 1`` is rejected.

**Derived metric references unknown name**
   If a derived metric references a name that is not a defined metric, the extension
   treats it as a column reference. If that column does not exist, the query fails at
   execution time.
