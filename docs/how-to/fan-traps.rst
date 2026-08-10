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

A fan trap error is raised when a metric's source table must traverse a relationship in the reverse direction (one-to-many) to reach a queried **dimension**. Traversing many-to-one is always safe, because each row on the "many" side maps to at most one row on the "one" side.

Metrics that merely sit at *different grains from each other* are not an error: since v0.12.0 they are computed one grain at a time and joined -- see :ref:`howto-fan-per-grain`.


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


.. _howto-fan-per-grain:

Multi-Grain Queries: Each Metric at Its Own Grain
=================================================

.. versionchanged:: 0.12.0

   Queries whose metrics sit at **different grains** are computed per grain
   instead of being rejected. Each metric is aggregated over its own table in a
   separate CTE and the results are joined on the queried dimensions, which is
   how Snowflake computes them.

Three shapes are answered this way. All three used to raise ``fan trap
detected``, because the generated query was always anchored ``FROM <base
table>``:

**A metric on a parent ("one" side) table the base table references.**
   ``SUM(customers.balance)`` in a view whose base table is ``orders``. Anchored
   at ``orders``, each customer row is counted once per order and customers with
   no orders vanish entirely. It is now aggregated over ``customers`` itself, so
   the total is the plain customer-grain total -- queried alone, or alongside
   dimensions at or above the customer grain.

**Metrics at two different grains, queried together.**
   ``order_count`` (on ``orders``) with ``item_qty`` (on ``line_items``), or two
   metrics on different child tables of one parent (a *chasm trap*). Each is
   aggregated separately and the results joined on the shared dimensions.

**A single derived metric that internally fuses two grains.**
   ``avg AS order_total / item_count``. Each component is aggregated at its own
   grain and the expression is evaluated over the two pre-aggregates, so the
   denominator is the true order count rather than the fanned one.

Dimension groups are combined with a NULL-safe ``FULL OUTER JOIN``, so a group
present at one grain but not another is preserved with a ``NULL`` metric -- a
customer region with no orders keeps its balance and reports a ``NULL`` order
count -- rather than being dropped. Queries with no dimensions produce one row
per grain, combined with ``CROSS JOIN``.

A ``COUNT(*)`` metric on a table with no declared ``PRIMARY KEY`` is answerable
on this path too: the table anchors its own CTE, so there are no NULL-extended
rows to over-count and the ``PRIMARY KEY`` requirement that applies to the
base-anchored join does not.

.. note::

   Single-grain queries are unaffected -- they keep the same base-anchored SQL.
   The per-grain path is entered only where the query would otherwise have been
   rejected.

For why a metric's grain belongs to its table rather than to the query, and what
that means when you are designing a model rather than debugging one, see
:ref:`explanation-metric-grain`.

A **window metric** whose inner aggregate lives on a non-base table *is*
computed at its own grain: the ``__sv_agg`` CTE is anchored at that table, so the
inner aggregate sees one row per record there instead of one per base-table row.
The window function itself is unaffected -- it runs over the already-grouped CTE.
Two window metrics whose inner aggregates sit at **different** grains still raise
the fan-trap error, because those grains would have to be joined before the
window runs.

An **active semi-additive metric** (``NON ADDITIVE BY`` with a snapshot dimension
outside the query) is also computed at its own grain: the snapshot CTE is
anchored at the metric's own table, so the ``RANK()`` runs over one row per
record there rather than over a join that has already duplicated them. A
``NON ADDITIVE BY`` dimension declared on a different table is joined into that
CTE so its ordering still resolves.

**Role-playing** is decided per query rather than per view. A query that reaches
a role-played table -- one table reached from the same source through two named
relationships -- is computed per grain when a metric's ``USING`` names which
relationship is meant, and raises the fan-trap error when nothing does. Picking a
role silently would depend on declaration order, which is the mis-binding this
fence exists to prevent.

Two shapes still raise the error. A role-played dimension queried **together
with** an active semi-additive metric declines, because a snapshot group cannot
carry a role: it would join whichever relationship is declared first while a
sibling grain CTE joins the one ``USING`` named, and the outer join would then
compare two different instances of the same dimension -- a wrong answer rather
than an error. And a query that reaches a role-played table with no ``USING`` to
disambiguate it declines for the reason above. Query those at a single grain.

.. versionchanged:: 0.12.0

   Active semi-additive metrics and ``USING``-disambiguated role-playing were
   both previously excluded from the per-grain path and raised the fan-trap
   error in any multi-grain query.


.. _howto-fan-other-shapes:

Other Shapes the Fence Rejects
==============================

.. versionchanged:: 0.11.0

   Query shapes that inflate the same way as the classic fan trap previously
   slipped past the fence and returned silently wrong numbers. They now raise
   the same ``fan trap detected`` error.

**A dimension below a metric's own grain.**
   The classic case above (``order_count`` by ``li.status``), and its
   longer-range forms: ``SUM(customers.balance)`` grouped by an order-grain or
   line-item-grain dimension. Per-grain aggregation does not make these
   answerable -- each customer genuinely fans across their orders' statuses, so
   there is no single correct value per group. Snowflake rejects the same shape:
   its rule is that `the logical table for the dimension must be related to the
   logical table for the metric
   <https://docs.snowflake.com/en/user-guide/views-semantic/querying>`_ and must
   have "an equal or lower level of granularity than the logical table for the
   metric". Fix: use the fixes listed above.

**A dimension on a sibling table.**
   .. versionchanged:: 0.12.0

   When two tables both reference a third -- ``line_items`` and ``shipments``
   both referencing ``orders`` -- a metric on one grouped by a dimension on the
   other is a fan trap: joining both children multiplies each one's rows by the
   other's (an order with 2 line items and 2 shipments produces 4 rows). Neither
   sibling is an ancestor of the other, so this pair used to be skipped by the
   check and the query returned inflated numbers. It is now rejected. Fix: query
   the two sides separately -- their *metrics* together are fine, and are
   computed per grain.

**An active semi-additive metric queried alongside a fanning child dimension.**
   A ``NON ADDITIVE BY`` metric queried together with a dimension on a fanning
   child table ran its snapshot (``RANK``) query over the already-multiplied
   join, where ties across the fanned duplicates of one source row are
   indistinguishable from ties across distinct rows -- so it could
   double-count. Such metrics previously skipped the fan-trap check on the
   assumption that the snapshot neutralised the fan; they now get the same
   check. Fix: snapshot only on safe, root-ward dimensions, or query the
   semi-additive metric without the fanning child dimension.

.. note::

   A semantic view whose ``RELATIONSHIPS`` form a **cycle** (``a`` references
   ``b`` and ``b`` references ``a``) parses successfully but such a definition
   is degenerate. As of v0.11.0 a query against it terminates with an error
   instead of hanging with unbounded memory growth (the fan-trap ancestor walk
   used to loop forever on a cyclic parent map).


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
