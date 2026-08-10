.. meta::
   :description: Filter a semantic view query before aggregation with where_clause or after aggregation with an outer WHERE, and know which one a given filter needs

.. _howto-filtering:

=========================================
How to Filter Semantic View Queries
=========================================

This guide shows how to filter a :ref:`semantic_view() <ref-semantic-view-function>`
query, and how to pick the right filter for the job. There are two of them and
they run on opposite sides of the aggregation, so for anything except a filter
on a dimension you are already grouping by, they return different numbers.

**Prerequisites:**

- A working semantic view you can query (see :ref:`tutorial-getting-started`)
- Familiarity with the ``semantic_view()`` query modes (see
  :ref:`ref-semantic-view-function`)


.. _howto-filtering-which:

Which Filter to Use
===================

.. list-table::
   :header-rows: 1
   :widths: 45 27 28

   * - What you want to filter on
     - Use
     - Runs
   * - A dimension that is in the query's output
     - Either
     - Same result
   * - A dimension or fact that is **not** in the output
     - ``where_clause :=``
     - Before aggregation
   * - A date or segment scope for the numbers themselves
     - ``where_clause :=``
     - Before aggregation
   * - A metric value (``revenue > 200``)
     - Outer ``WHERE``
     - After aggregation

The rule behind the table: ``where_clause`` decides **which rows the metrics
aggregate over**, and an outer ``WHERE`` decides **which of the resulting rows
you keep**.


.. _howto-filtering-setup:

Set Up an Example View
======================

Every example below runs against this view. It has two dimensions --
``region``, which the queries group by, and ``ordered_at``, which they mostly do
not -- and two metrics.

.. code-block:: sql

   INSTALL semantic_views FROM community;
   LOAD semantic_views;

   CREATE TABLE orders (
       id INTEGER,
       region VARCHAR,
       amount DECIMAL(10,2),
       ordered_at DATE
   );

   INSERT INTO orders VALUES
       (1, 'East', 100.00, DATE '2023-11-02'),
       (2, 'East', 250.00, DATE '2024-01-15'),
       (3, 'East',  50.00, DATE '2024-02-20'),
       (4, 'West', 400.00, DATE '2023-12-10'),
       (5, 'West', 150.00, DATE '2024-03-05');

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region     AS o.region,
       o.ordered_at AS o.ordered_at
   )
   METRICS (
       o.revenue     AS SUM(o.amount),
       o.order_count AS COUNT(*)
   );

Unfiltered, revenue by region covers all five orders:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue', 'order_count']
   ) ORDER BY region;

.. code-block:: text

   ┌────────┬─────────┬─────────────┐
   │ region │ revenue │ order_count │
   ├────────┼─────────┼─────────────┤
   │ East   │  400.00 │           3 │
   │ West   │  550.00 │           2 │
   └────────┴─────────┴─────────────┘


.. _howto-filtering-outer:

Filter the Result Rows with an Outer WHERE
==========================================

Wrap the function call and add an ordinary SQL ``WHERE``. The predicate may name
any column in the result -- that is, any dimension or metric you asked for.

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   ) WHERE region = 'East';

.. code-block:: text

   ┌────────┬─────────┐
   │ region │ revenue │
   ├────────┼─────────┤
   │ East   │  400.00 │
   └────────┴─────────┘

``region`` is one of the grouped dimensions, so removing the West group after
aggregation and removing the West rows before aggregation come to the same
thing. DuckDB's optimizer pushes such predicates down into the generated query
anyway. This is also the only place a filter on a **metric** can go, because a
metric has no value until it has been aggregated:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   ) WHERE revenue > 500;


.. _howto-filtering-pre-agg:

Filter the Rows Behind a Metric with where_clause
=================================================

Pass ``where_clause := '<predicate>'`` to apply the predicate **before** the
metrics are aggregated. The predicate names declared dimensions and facts by
their logical names, whether or not they appear in the output.

.. code-block:: sql
   :emphasize-lines: 4

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue', 'order_count'],
       where_clause := 'ordered_at >= DATE ''2024-01-01'''
   ) ORDER BY region;

.. code-block:: text

   ┌────────┬─────────┬─────────────┐
   │ region │ revenue │ order_count │
   ├────────┼─────────┼─────────────┤
   │ East   │  300.00 │           2 │
   │ West   │  150.00 │           1 │
   └────────┴─────────┴─────────────┘

Each region's revenue has been recomputed over its 2024 orders only, and the
counts follow. The result still has one row per region -- filtering by
``ordered_at`` did not add it to the output.

.. note::

   The parameter is spelled ``where_clause`` rather than ``where`` because
   DuckDB reserves ``where`` in named-parameter position: ``where := '…'`` fails
   to parse before the extension is ever consulted. Inside the SQL string
   literal, single quotes are doubled -- hence ``DATE ''2024-01-01''``.


.. _howto-filtering-diverge:

Where the Two Filters Diverge
=============================

The 2024 result above is not reachable with an outer ``WHERE``. Two attempts
show why.

**Attempt 1: filter on a column that is not in the output.**

.. code-block:: sql

   -- Fails: `ordered_at` is not a column of the result
   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   ) WHERE ordered_at >= DATE '2024-01-01';

The function returned ``region`` and ``revenue``, so DuckDB raises a binder error
for the unknown column. This failure is loud, which makes it the harmless case.

**Attempt 2: add the column to the output so the filter binds.**

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region', 'ordered_at'],
       metrics := ['revenue']
   ) WHERE ordered_at >= DATE '2024-01-01'
   ORDER BY region, ordered_at;

.. code-block:: text

   ┌────────┬────────────┬─────────┐
   │ region │ ordered_at │ revenue │
   ├────────┼────────────┼─────────┤
   │ East   │ 2024-01-15 │  250.00 │
   │ East   │ 2024-02-20 │   50.00 │
   │ West   │ 2024-03-05 │  150.00 │
   └────────┴────────────┴─────────┘

This runs, but it answers a different question. Adding ``ordered_at`` to the
dimensions changed the grouping, so ``revenue`` is now per region **per day**.
To get 2024 revenue per region you would have to re-aggregate the result
yourself -- and any metric that is not a plain sum (an average, a distinct count,
a semi-additive snapshot) cannot be recovered that way at all.

.. warning::

   This is the failure mode ``where_clause`` exists to prevent. Attempt 2
   produces numbers, and they are wrong for the question asked. Whenever a
   filter scopes *what the metric measures* -- a date range, a customer segment,
   a status -- it belongs in ``where_clause``.


.. _howto-filtering-named:

Reuse a Predicate as a Named Filter
===================================

.. versionadded:: 0.12.0

Rather than repeating a predicate in every call, declare it once as a member
annotated ``LABELS = (FILTER)``. The label records that the member exists to be
filtered on rather than selected:

.. code-block:: sql
   :emphasize-lines: 8

   CREATE OR REPLACE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region     AS o.region,
       o.ordered_at AS o.ordered_at,
       o.is_2024    AS o.ordered_at >= DATE '2024-01-01' LABELS = (FILTER)
   )
   METRICS (
       o.revenue     AS SUM(o.amount),
       o.order_count AS COUNT(*)
   );

Queries then name the filter instead of restating the predicate:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue'],
       where_clause := 'is_2024'
   ) ORDER BY region;

Named filters compose with ordinary SQL operators -- ``where_clause :=
'is_2024 AND region = ''East'''``. Each member's expression is substituted
parenthesized, so a filter defined as an ``OR`` keeps its grouping when you
``AND`` it with something else. See :ref:`howto-annotations-filters` for the
annotation itself.


.. _howto-filtering-combine:

Use Both Filters in One Query
=============================

The two filters compose, each doing its own job: ``where_clause`` scopes the
rows the metrics see, and the outer ``WHERE`` trims the assembled result.

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue'],
       where_clause := 'ordered_at >= DATE ''2024-01-01'''
   ) WHERE revenue > 200
   ORDER BY revenue DESC;

.. code-block:: text

   ┌────────┬─────────┐
   │ region │ revenue │
   ├────────┼─────────┤
   │ East   │  300.00 │
   └────────┴─────────┘

West's 2024 revenue of 150.00 is computed and then discarded by the outer
predicate. Reversing the two filters would give a different answer: ``revenue >
200`` applied to unfiltered totals keeps both regions.


.. _howto-filtering-verify:

Verify Which Rows Were Aggregated
=================================

:ref:`explain_semantic_view() <ref-explain-semantic-view>` accepts
``where_clause`` too, so you can inspect the exact statement an application will
issue before shipping it:

.. code-block:: sql

   SELECT * FROM explain_semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue'],
       where_clause := 'ordered_at >= DATE ''2024-01-01'''
   );

Look for the predicate ahead of the ``GROUP BY``. It is placed before
aggregation on every emission path -- including inside each grain's aggregate in
a :ref:`multi-grain query <explanation-metric-grain>`, inside the snapshot step
of a semi-additive metric so that filtering changes which row wins, and inside
the aggregate step of a window metric so that a filtered window number is
recomputed rather than trimmed afterwards.

.. tip::

   An omitted, empty, or whitespace-only ``where_clause`` is treated as absent.
   Application code that assembles a predicate from optional request parameters
   can therefore pass an empty string for "no filter" without special-casing it.


.. _howto-filtering-troubleshooting:

Troubleshooting
===============

**A predicate on a metric is rejected**
   ``where_clause := 'revenue > 200'`` is refused because the filter runs before
   aggregation, where an aggregate has no value yet. Move that predicate to the
   outer ``WHERE``.

**A binder error names a column that is not in the result**
   The filter is on the outer query but names a member the query did not
   request. Move it into ``where_clause``, where any declared dimension or fact
   resolves whether or not it is in the output.

**A fan trap error names a member you only filtered on**
   Tables the predicate reaches are joined in and checked exactly as a queried
   dimension's are, so a filter can trip the fan-out fence on its own. See
   :ref:`howto-fan-traps` for the fixes, and :ref:`explanation-metric-grain` for
   why the check exists.

**A filter member on a role-played table is rejected**
   When a table is reachable through two named relationships, a predicate has no
   way to say which role it means, so the query errors rather than binding to
   whichever relationship was declared first. Filter on a member of a table that
   is reached one way only. See :ref:`howto-role-playing`.

**The predicate fails on a type error the first time it is used**
   A named filter's ``BOOLEAN`` requirement is enforced by DuckDB's binder when
   the filter is first used, not at ``CREATE``. Check that the member's
   expression really evaluates to a boolean.

**Quoting looks wrong**
   ``where_clause`` is a SQL string literal, so every single quote inside it is
   doubled: ``where_clause := 'region = ''East'''``. Double-quoted identifiers
   inside the predicate need no escaping.


.. _howto-filtering-related:

Related Guides
==============

- :ref:`explanation-metric-grain` -- why a pre-aggregation filter changes the
  numbers and a post-aggregation one cannot.
- :ref:`howto-annotations-filters` -- declaring reusable named filters with
  ``LABELS = (FILTER)``.
- :ref:`ref-sv-pre-agg-filtering` -- the full ``where_clause`` reference,
  including exactly where the predicate is injected on each emission path.
- :ref:`howto-fan-traps` -- resolving the fan-out errors a predicate can trigger.
