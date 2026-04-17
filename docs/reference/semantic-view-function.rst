.. meta::
   :description: Syntax and parameter reference for semantic_view(), the table function that queries any combination of dimensions, metrics, or facts

.. _ref-semantic-view-function:

=====================
semantic_view()
=====================

Table function that queries a semantic view with a specified combination of dimensions, metrics, or facts. The extension generates the SQL (SELECT, FROM, JOIN, GROUP BY) and returns the result set.


.. _ref-sv-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SELECT * FROM semantic_view(
       '<view_name>',
       [ dimensions := [ '<dim_name>' [, ...] ] , ]
       [ metrics := [ '<metric_name>' [, ...] ] , ]
       [ facts := [ '<fact_name>' [, ...] ] ]
   )


.. _ref-sv-params:

Parameters
==========

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Parameter
     - Type
     - Description
   * - ``<view_name>``
     - VARCHAR (positional)
     - The name of the semantic view to query. Must match a registered view.
   * - ``dimensions``
     - LIST (named)
     - Optional list of dimension names to include in the result. Each name must match a dimension defined in the semantic view. Supports ``alias.*`` wildcard patterns.
   * - ``metrics``
     - LIST (named)
     - Optional list of metric names to include in the result. Each name must match a metric defined in the semantic view. Supports ``alias.*`` wildcard patterns.
   * - ``facts``
     - LIST (named)
     - Optional list of fact names to include in the result. Each name must match a fact defined in the semantic view. Supports ``alias.*`` wildcard patterns.

At least one of ``dimensions``, ``metrics``, or ``facts`` must be specified.

.. warning::

   ``facts`` and ``metrics`` cannot be combined in the same query. Use ``facts := [...]`` or ``metrics := [...]``, not both.


.. _ref-sv-modes:

Query Modes
===========

The function operates in four modes depending on which parameters are provided:

**Dimensions + Metrics** (grouped aggregation):

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region', 'category'],
       metrics := ['revenue', 'order_count']
   );

Generates ``SELECT <dims>, <metrics> FROM ... GROUP BY <dims>``.

**Dimensions only** (distinct values):

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region']
   );

Generates ``SELECT DISTINCT <dims> FROM ...``.

**Metrics only** (grand total, global aggregate):

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       metrics := ['revenue']
   );

Generates ``SELECT <metrics> FROM ...`` with no GROUP BY (returns one row).

**Facts mode** (row-level, no aggregation):

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       facts := ['net_price', 'tax_amount']
   );

Returns one row per source row with the requested fact expressions as columns. No aggregation or GROUP BY is applied. Dimensions can be combined with facts (they appear as columns without triggering grouping).


.. _ref-sv-wildcard:

Wildcard Selection
==================

All three list parameters accept ``alias.*`` patterns that expand to all items scoped to the specified table alias:

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       dimensions := ['o.*'],
       metrics := ['o.*']
   );

``PRIVATE`` metrics and facts are excluded from wildcard expansion. Bare ``*`` (unqualified) is not supported -- all wildcards must be qualified with a table alias.

When an item appears both explicitly and via wildcard expansion, it appears only once in the result (deduplication).


.. _ref-sv-output:

Output
======

Returns a result set with one column per requested dimension, metric, or fact, in the order: dimensions first (in the order requested), then metrics or facts (in the order requested).

Column types are inferred at define time from the underlying table columns. If type inference is not available, columns default to VARCHAR.


.. _ref-sv-filtering:

Filtering
=========

Use standard SQL ``WHERE`` on the outer query to filter results:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   ) WHERE region = 'East';

The ``WHERE`` clause applies to the result set after the semantic view expansion. DuckDB's optimizer pushes predicates down into the generated query where possible.


.. _ref-sv-ordering:

Ordering and Limiting
=====================

Use standard SQL ``ORDER BY`` and ``LIMIT`` on the outer query:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   ) ORDER BY revenue DESC
   LIMIT 10;


.. _ref-sv-name-resolution:

Name Resolution
===============

Dimension, metric, and fact names are resolved case-insensitively. Names can optionally be table-qualified (e.g., ``'o.region'``), which matches against the ``source_table`` alias of the dimension, metric, or fact.

Wildcard patterns (``alias.*``) are expanded before name resolution. The expansion respects ``PRIVATE`` access modifiers -- private items are excluded.

If a name does not match any defined dimension, metric, or fact, the error message lists available names and suggests the closest match (if one exists within 3 edits).


.. _ref-sv-examples:

Examples
========

.. code-block:: sql

   -- All dimensions and metrics
   SELECT * FROM semantic_view('shop',
       dimensions := ['customer', 'product'],
       metrics := ['revenue', 'order_count']
   );

   -- Single dimension, single metric
   SELECT * FROM semantic_view('shop',
       dimensions := ['customer'],
       metrics := ['revenue']
   );

   -- Metrics only (grand total)
   SELECT * FROM semantic_view('shop',
       metrics := ['revenue', 'order_count']
   );

   -- With filtering and ordering
   SELECT * FROM semantic_view('shop',
       dimensions := ['customer'],
       metrics := ['revenue']
   ) WHERE revenue > 100
   ORDER BY revenue DESC;

   -- Facts mode (row-level)
   SELECT * FROM semantic_view('shop',
       facts := ['net_price']
   );

   -- Facts with dimensions
   SELECT * FROM semantic_view('shop',
       dimensions := ['region'],
       facts := ['net_price']
   );

   -- Wildcard selection
   SELECT * FROM semantic_view('shop',
       dimensions := ['o.*'],
       metrics := ['o.*']
   );
