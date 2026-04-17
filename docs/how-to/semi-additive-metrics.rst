.. meta::
   :description: Define semi-additive metrics with NON ADDITIVE BY for snapshot aggregation patterns like account balances and inventory levels

.. _howto-semi-additive:

===================================
How to Use Semi-Additive Metrics
===================================

This guide shows how to define metrics with ``NON ADDITIVE BY`` to handle snapshot data -- values that should not be summed across certain dimensions like time, but can be summed across others like customer or region.

**Prerequisites:**

- A working semantic view with ``TABLES``, ``DIMENSIONS``, and ``METRICS`` (see :ref:`tutorial-multi-table`)


.. _howto-semi-additive-snapshot:

Snapshot Data
=============

Semi-additive metrics solve a specific problem with snapshot data -- tables where each row records a point-in-time measurement rather than an event. For example, an ``accounts`` table might record daily balances:

.. code-block:: text

   ┌────────────┬─────────────┬─────────┐
   │ report_date│ customer_id │ balance │
   ├────────────┼─────────────┼─────────┤
   │ 2026-04-10 │ ACME        │ 500     │
   │ 2026-04-10 │ Globex      │ 300     │
   │ 2026-04-11 │ ACME        │ 550     │
   │ 2026-04-11 │ Globex      │ 280     │
   └────────────┴─────────────┴─────────┘

If you query ``SUM(balance)`` grouped by ``customer_id`` across both dates, you get 1050 for ACME (500 + 550) -- but that is double-counting. The real current balance is 550. Summing across customers makes sense (ACME + Globex = 830 on April 11), but summing the same customer across dates does not.

``NON ADDITIVE BY`` tells the extension to pick one snapshot row per group (e.g., the latest ``report_date``) before aggregating, so you get correct totals without manual filtering.


.. _howto-semi-additive-define:

Define a Semi-Additive Metric
=============================

Add ``NON ADDITIVE BY (<dimension>)`` to a metric to declare which dimensions it should not be summed across. The extension selects the most recent (or earliest) snapshot row before aggregating.

.. code-block:: sql
   :emphasize-lines: 11

   CREATE SEMANTIC VIEW account_metrics AS
   TABLES (
       a AS accounts PRIMARY KEY (id)
   )
   DIMENSIONS (
       a.customer_id AS a.customer_id,
       a.report_date AS a.report_date
   )
   METRICS (
       a.total_balance AS SUM(a.balance)
           NON ADDITIVE BY (report_date DESC NULLS FIRST)
   );

This declares that ``total_balance`` is non-additive by ``report_date``. When a query requests ``total_balance`` grouped by ``customer_id`` (without ``report_date``), the extension selects the latest snapshot row per customer before summing.


.. _howto-semi-additive-sort:

Sort Order and NULLS Placement
==============================

Each dimension in ``NON ADDITIVE BY`` accepts an optional sort order and NULLS placement:

- ``ASC`` (default) -- selects the earliest snapshot row
- ``DESC`` -- selects the latest snapshot row
- ``NULLS FIRST`` -- NULL dimension values are treated as highest priority
- ``NULLS LAST`` (default) -- NULL dimension values are treated as lowest priority

.. code-block:: sql

   -- Latest balance (most recent report_date wins)
   a.total_balance AS SUM(a.balance) NON ADDITIVE BY (report_date DESC NULLS FIRST)

   -- Earliest balance (oldest report_date wins)
   a.opening_balance AS SUM(a.balance) NON ADDITIVE BY (report_date ASC NULLS LAST)


.. _howto-semi-additive-multiple:

Multiple Non-Additive Dimensions
=================================

A metric can be non-additive by more than one dimension. Each gets its own sort specification:

.. code-block:: sql

   a.snapshot_balance AS SUM(a.balance)
       NON ADDITIVE BY (report_date DESC NULLS FIRST, fiscal_period DESC NULLS FIRST)


.. _howto-semi-additive-behavior:

Snapshot Behavior
=================

The semi-additive expansion depends on whether the non-additive dimensions are present in the query:

**Non-additive dimension NOT in query (active):**
   The extension generates a CTE with ``ROW_NUMBER() OVER (PARTITION BY <queried dims> ORDER BY <NA dims>)`` to select one snapshot row per group, then aggregates over the filtered rows. This is the snapshot selection behavior.

.. code-block:: sql

   -- report_date not in query -> snapshot selection activated
   SELECT * FROM semantic_view('account_metrics',
       dimensions := ['customer_id'],
       metrics := ['total_balance']
   );

**Non-additive dimension in query (effectively regular):**
   When all non-additive dimensions are included in the query, the metric behaves as a standard additive metric -- no CTE, no snapshot selection. This matches Snowflake's behavior: "When the non-additive dimension is included in the query, the metric is calculated as a standard additive metric."

.. code-block:: sql

   -- report_date in query -> standard aggregation, no CTE
   SELECT * FROM semantic_view('account_metrics',
       dimensions := ['customer_id', 'report_date'],
       metrics := ['total_balance']
   );

**Mixed regular and semi-additive metrics:**
   Regular metrics and semi-additive metrics can coexist in the same query. The CTE includes both, but only the semi-additive metrics get the ``CASE WHEN __sv_rn = 1`` conditional aggregation. Regular metrics aggregate over all rows.


.. _howto-semi-additive-verify:

Verify the Generated SQL
=========================

Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to inspect the CTE expansion:

.. code-block:: sql

   SELECT * FROM explain_semantic_view('account_metrics',
       dimensions := ['customer_id'],
       metrics := ['total_balance']
   );

The ``sql`` column shows the generated query:

.. code-block:: sql

   WITH __sv_snapshot AS (
       SELECT
           "accounts"."customer_id",
           "accounts"."balance",
           ROW_NUMBER() OVER (
               PARTITION BY "accounts"."customer_id"
               ORDER BY "accounts"."report_date" DESC NULLS FIRST
           ) AS __sv_rn
       FROM "accounts"
   )
   SELECT
       "customer_id",
       SUM(CASE WHEN __sv_rn = 1 THEN "balance" END) AS "total_balance"
   FROM __sv_snapshot
   GROUP BY "customer_id"

The CTE assigns a row number per ``customer_id`` ordered by ``report_date DESC``, so row 1 is the latest snapshot. The outer query then aggregates only those latest rows via ``CASE WHEN __sv_rn = 1``.


.. _howto-semi-additive-restrictions:

Restrictions
============

.. warning::

   ``NON ADDITIVE BY`` and ``OVER`` (window function) cannot be combined on the same metric. A metric is either semi-additive or a window metric, not both. Attempting to use both produces a define-time error.


.. _howto-semi-additive-troubleshoot:

Troubleshooting
===============

**NON ADDITIVE BY dimension not found**
   The dimension name in ``NON ADDITIVE BY`` must match a declared dimension in the view. The error message identifies which dimension name is unrecognized: ``NON ADDITIVE BY dimension 'X' on metric 'Y' does not match any declared dimension``.

**Unexpected aggregation results**
   Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to verify whether the CTE is generated. If all Non-additive dimensions are in the query, the metric behaves as a regular additive metric and no CTE is produced. Remove the Non-additive dimension from the query to activate snapshot selection.

**Performance with multiple Non-Additive dimension sets**
   When multiple semi-additive metrics have different ``NON ADDITIVE BY`` dimensions, each gets its own ``ROW_NUMBER`` column in the CTE (``__sv_rn_1``, ``__sv_rn_2``, etc.). This is functionally correct but adds window function overhead.
