.. meta::
   :description: Select all dimensions, metrics, or facts for a table alias using wildcard patterns (alias.*) in semantic_view() queries

.. _howto-wildcard-selection:

================================
How to Use Wildcard Selection
================================

This guide shows how to use the ``alias.*`` wildcard pattern to select all dimensions, metrics, or facts belonging to a specific table alias in a single expression, instead of listing each name individually.

**Prerequisites:**

- A working semantic view with multiple dimensions and/or metrics (see :ref:`tutorial-multi-table`)
- Familiarity with :ref:`semantic_view() <ref-semantic-view-function>` queries


.. _howto-wildcard-syntax:

Wildcard Syntax
===============

Use ``table_alias.*`` in any of the three list parameters (``dimensions``, ``metrics``, ``facts``) to expand to all items scoped to that table alias:

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       dimensions := ['o.*'],
       metrics := ['o.*']
   );

This expands ``o.*`` to all dimensions scoped to table alias ``o`` and all metrics scoped to ``o``.


.. _howto-wildcard-private:

PRIVATE Item Exclusion
======================

Wildcard expansion excludes ``PRIVATE`` metrics and facts. Only ``PUBLIC`` items (the default) are included in the expanded list.

Given a view with both public and private metrics:

.. code-block:: sql

   CREATE SEMANTIC VIEW sales AS
   TABLES (o AS orders PRIMARY KEY (id))
   DIMENSIONS (o.region AS o.region)
   METRICS (
       o.revenue AS SUM(o.amount),
       PRIVATE o.internal_cost AS SUM(o.cost),
       profit AS revenue - internal_cost
   );

.. code-block:: sql

   -- o.* expands to ['revenue'] only -- internal_cost is PRIVATE
   SELECT * FROM semantic_view('sales',
       dimensions := ['region'],
       metrics := ['o.*']
   );


.. _howto-wildcard-dedup:

Deduplication
=============

When an item appears both explicitly and via a wildcard, it appears only once in the expanded list:

.. code-block:: sql

   -- 'region' is listed explicitly AND is part of o.*
   -- Result: ['region', 'status'] (region appears once)
   SELECT * FROM semantic_view('sales',
       dimensions := ['region', 'o.*']
   );


.. _howto-wildcard-bare:

Bare Wildcard Rejection
========================

.. warning::

   Unqualified ``*`` (bare wildcard) is not supported. All wildcards must be qualified with a table alias.

.. code-block:: sql

   -- This fails:
   SELECT * FROM semantic_view('sales',
       dimensions := ['*']
   );

.. code-block:: text

   unqualified wildcard '*' is not supported. Use table_alias.* to select all items
   for a specific table.


.. _howto-wildcard-facts:

Wildcards for Facts
===================

The ``facts`` parameter also supports wildcard expansion:

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       facts := ['li.*']
   );

This expands to all public facts scoped to table alias ``li``.


.. _howto-wildcard-troubleshoot:

Troubleshooting
===============

**Unknown table alias in wildcard**
   The table alias must match an alias declared in the ``TABLES`` clause. The error lists available aliases: ``unknown table alias 'x' in wildcard 'x.*'. Available aliases: [o, c, li]``.

**Empty expansion**
   If the wildcard expands to an empty list (no items scoped to that alias, or all are private), the query may fail with an empty request error. Verify that the alias has public items defined.
