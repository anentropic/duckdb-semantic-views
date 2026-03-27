.. meta::
   :description: Understand how the extension detects fan traps and restructure queries or views to avoid inflated aggregation results

.. _howto-fan-traps:

==========================================
How to Understand and Avoid Fan Traps
==========================================

This guide explains what fan traps are, how the extension detects them, and how to restructure queries or views to avoid inflated aggregation results.

**Prerequisites:**

- A working multi-table semantic view with relationships (see :ref:`tutorial-multi-table`)
- Understanding of cardinality concepts (one-to-one, many-to-one, one-to-many)


.. _howto-fan-what:

What Is a Fan Trap?
===================

A fan trap occurs when a query aggregates a metric from one table while grouping by a dimension from another table that is on the "many" side of a relationship. The join produces duplicate rows, inflating the aggregate result.

For example, consider orders and line items:

- Each order has many line items (one-to-many from orders to line items).
- ``COUNT(*)`` on orders counts one row per order.
- If you join orders to line items to get a line-item dimension, each order row is duplicated per line item.
- ``COUNT(*)`` on orders now returns the number of line items, not the number of orders.

The extension detects this pattern and raises an error instead of returning incorrect results. For background on the concept, see :ref:`explanation-sv-vs-views`.


.. _howto-fan-detect:

When the Extension Raises a Fan Trap Error
==========================================

The extension infers cardinality from the ``PRIMARY KEY`` and ``UNIQUE`` declarations in the ``TABLES`` clause:

- If the FK columns on the "from" side of a relationship match a PK or UNIQUE constraint on that same table, the relationship is **one-to-one**.
- Otherwise, the relationship is **many-to-one** (the default).

A fan trap error is raised when a metric's source table must traverse a relationship in the reverse direction (one-to-many). Traversing many-to-one is always safe, because each row on the "many" side maps to at most one row on the "one" side.


.. _howto-fan-example:

Example: Fan Trap Detection in Action
======================================

.. code-block:: sql

   CREATE TABLE orders (id INTEGER, region VARCHAR);
   INSERT INTO orders VALUES (1, 'East'), (2, 'West');

   CREATE TABLE line_items (
       id INTEGER, order_id INTEGER,
       extended_price DOUBLE, status VARCHAR
   );
   INSERT INTO line_items VALUES
       (1, 1, 100.00, 'shipped'),
       (2, 1, 200.00, 'pending'),
       (3, 2, 150.00, 'shipped');

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       o  AS orders     PRIMARY KEY (id),
       li AS line_items PRIMARY KEY (id)
   )
   RELATIONSHIPS (
       li_to_order AS li(order_id) REFERENCES o
   )
   DIMENSIONS (
       o.region     AS o.region,
       li.status    AS li.status
   )
   METRICS (
       li.revenue     AS SUM(li.extended_price),
       o.order_count  AS COUNT(*)
   );

**Safe query,** ``li.revenue`` grouped by ``o.region``:

The relationship ``li_to_order`` is many-to-one from ``li`` to ``o``. Traversing this direction is safe because each line item maps to one order.

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['region'],
       metrics := ['revenue']
   );

**Blocked query,** ``o.order_count`` grouped by ``li.status``:

To reach ``li.status``, the extension must traverse from ``o`` to ``li``, the reverse of many-to-one, which is one-to-many. This would duplicate order rows, inflating the count.

.. code-block:: sql

   -- This query is blocked with a fan trap error:
   SELECT * FROM semantic_view('sales',
       dimensions := ['status'],
       metrics := ['order_count']
   );

The error message identifies the metric, dimension, and relationship involved:

.. code-block:: text

   semantic view 'sales': fan trap detected -- metric 'order_count' (table 'o')
   would be duplicated when joined to dimension 'status' (table 'li') via
   relationship 'li_to_order' (many-to-one cardinality, inferred: FK is not
   PK/UNIQUE). This would inflate aggregation results.


.. _howto-fan-fix:

How to Fix Fan Trap Errors
==========================

There are three approaches:

**1. Remove the problematic dimension**

Query ``order_count`` with a dimension from the same table (``o``) or from a table reachable in the safe direction:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['region'],
       metrics := ['order_count']
   );

**2. Use a metric from the same table as the dimension**

Instead of ``o.order_count`` with ``li.status``, use ``li.revenue`` with ``li.status``:

.. code-block:: sql

   SELECT * FROM semantic_view('sales',
       dimensions := ['status'],
       metrics := ['revenue']
   );

**3. Restructure the view**

If you need both ``order_count`` by ``status``, consider creating a separate semantic view scoped to the appropriate table, or pre-aggregating at the line-item level.


.. _howto-fan-onetoone:

One-to-One Relationships
========================

If the FK columns match a PK or UNIQUE constraint on the "from" side, the extension infers one-to-one cardinality. One-to-one relationships can be traversed in either direction without fan-out.

.. code-block:: sql

   CREATE SEMANTIC VIEW order_details AS
   TABLES (
       o  AS orders     PRIMARY KEY (id),
       od AS order_details PRIMARY KEY (order_id) -- order_id is both PK and FK
   )
   RELATIONSHIPS (
       detail_to_order AS od(order_id) REFERENCES o
   )
   ...

Because ``order_id`` is the PK of ``order_details``, the relationship is one-to-one. Metrics from either table can be grouped by dimensions from the other without triggering a fan trap.

.. tip::

   Before writing a query, you can ask the extension which dimensions are safe to combine with a specific metric. :ref:`SHOW SEMANTIC DIMENSIONS … FOR METRIC <ref-show-dims-for-metric>` applies the same reachability rules at inspection time and returns only the dimensions that will not trigger a fan trap:

   .. code-block:: sql

      SHOW SEMANTIC DIMENSIONS IN sales FOR METRIC order_count;
