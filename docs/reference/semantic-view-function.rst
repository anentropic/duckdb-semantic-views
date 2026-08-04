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
       [ facts := [ '<fact_name>' [, ...] ] , ]
       [ where_clause := '<predicate>' ]
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
     - The name of the semantic view to query. Must match a registered view. The name is folded to lowercase and matched case-insensitively, quoted or not (``'Sales'``, ``'SALES'``, and ``'"sales"'`` all resolve to the same view), following DuckDB's identifier semantics. May carry a ``<schema>.`` (or ``<database>.<schema>.``) qualifier — ``'analytics.sales'`` — which pins the schema. An unqualified name resolves through the session's ``search_path``, exactly as an unqualified table reference does: the first schema on the path holding a view of that name wins. A view that is the only one of its name resolves whether or not its schema is on the path, so a single-schema setup needs no ``search_path`` at all (see :ref:`ref-create-semantic-view`).
   * - ``dimensions``
     - LIST (named)
     - Optional list of dimension names to include in the result. Each name must match a dimension defined in the semantic view. Supports ``alias.*`` wildcard patterns.
   * - ``metrics``
     - LIST (named)
     - Optional list of metric names to include in the result. Each name must match a metric defined in the semantic view. Supports ``alias.*`` wildcard patterns.
   * - ``facts``
     - LIST (named)
     - Optional list of fact names to include in the result. Each name must match a fact defined in the semantic view. Supports ``alias.*`` wildcard patterns.
   * - ``where_clause``
     - VARCHAR (named)
     - Optional predicate applied **before** metrics are aggregated — the equivalent of Snowflake's ``SEMANTIC_VIEW( … WHERE <predicate> )``. See :ref:`ref-sv-pre-agg-filtering`. An omitted, empty, or whitespace-only value is treated as absent.
   * - ``search_path``
     - LIST (named)
     - The session's schema resolution order, used to resolve an unqualified ``<view_name>``. **Supplied automatically** — the extension's parser override injects the caller's search path into every ``semantic_view()`` call it rewrites, because the read side binds on a connection that cannot otherwise see it. Not intended to be written by hand.

At least one of ``dimensions``, ``metrics``, or ``facts`` must be specified. ``where_clause`` alone is not a query.

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

Column types are inferred when the query is bound, by running the generated SQL as a ``LIMIT 0`` probe and reading back the result schema. Every query infers this way — there is no ``CREATE``-time type cache to fall back on, and legacy catalog rows that still carry one are ignored in favour of the probe.

If the probe fails, the query raises ``semantic_view: type inference failed for query …`` with the underlying error. It does not fall back to VARCHAR: a placeholder type would mask a broken ``FACTS`` expression until something downstream tripped over it.

.. versionchanged:: 0.11.0

   A dimension, metric, or fact declared with a double-quoted name (e.g.
   ``"order date"``) now produces an output column named by its **logical
   value** (``order date``), with the quote characters stripped. Previously the
   column was named ``"order date"`` -- quote characters included. A consumer
   selecting the old quote-laden column name must update. Queries over unquoted
   names are unaffected.


.. _ref-sv-filtering:

Filtering
=========

There are two filters, and they run on opposite sides of the aggregation.

Post-aggregation — outer ``WHERE``
----------------------------------

Use standard SQL ``WHERE`` on the outer query to filter the rows the function returns:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   ) WHERE region = 'East';

The ``WHERE`` clause applies to the result set after the semantic view expansion. DuckDB's optimizer pushes predicates down into the generated query where possible.


.. _ref-sv-pre-agg-filtering:

Pre-aggregation — ``where_clause``
----------------------------------

``where_clause := '<predicate>'`` filters the rows the metrics aggregate **over**, before they are aggregated — the equivalent of Snowflake's ``SEMANTIC_VIEW( … WHERE <predicate> )``:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue'],
       where_clause := 'ordered_at >= DATE ''2024-01-01'''
   );

Each region's ``revenue`` is recomputed over only the matching orders. An outer ``WHERE`` cannot express this: by then the aggregation has already run over every row, and ``ordered_at`` is not in the output to filter by.

The parameter is spelled ``where_clause`` rather than ``where`` because DuckDB reserves ``where`` in named-parameter position — ``where := '…'`` is a parse error before the extension is consulted.

**What the predicate may name.** Declared dimensions and facts, referenced by their logical names; each is substituted to its expression, wrapped in parentheses so a member that binds looser than its surrounding context keeps its grouping. Members declared :ref:`LABELS = (FILTER) <howto-annotations-filters>` are the intended case, but any dimension or fact works — the label records intent and does not gate resolution. Naming a **metric** is rejected, matching Snowflake: the filter runs before aggregation, so an aggregate has no value yet.

**Joins and fan-out.** Tables the predicate reaches are joined in and subjected to the same reachability and fan-out checks as a queried dimension's, matching Snowflake's rule that WHERE-clause members participate in the same-logical-table constraint. A member on a role-played table — one reached through two named relationships — is rejected rather than bound to whichever relationship was declared first, since the predicate has no way to say which role is meant.

**Where it is applied.** Before the ``GROUP BY`` on the base-anchored and fact paths, inside each grain CTE for multi-grain queries, inside the snapshot CTE ahead of the ranking for semi-additive metrics, and inside the aggregate CTE ahead of the window function for window metrics — so a filtered number is always recomputed rather than filtered after the fact.

An omitted, empty, or whitespace-only ``where_clause`` is treated as absent. The two filters compose: a pre-aggregation predicate and an outer ``WHERE`` in the same query each do their own job.


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

Dimension, metric, and fact names are resolved case-insensitively, following DuckDB's identifier semantics: matching ignores case whether the reference is written unquoted (``'region'``, ``'REGION'``) or double-quoted (``'"Region"'``) — DuckDB treats double-quoted identifiers as case-insensitive too, so quoting a reference only lets it carry whitespace or special characters, it does not make it case-sensitive. Names can optionally be table-qualified (e.g., ``'o.region'``), which matches against the ``source_table`` alias of the dimension, metric, or fact.

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

   -- Pre-aggregation filtering: revenue per customer, over 2024 orders only
   SELECT * FROM semantic_view('shop',
       dimensions := ['customer'],
       metrics := ['revenue'],
       where_clause := 'ordered_at >= DATE ''2024-01-01'''
   );

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
