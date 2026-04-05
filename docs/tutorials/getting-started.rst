.. meta::
   :description: Install the extension, define your first semantic view, and run queries across three modes in under five minutes

.. _tutorial-getting-started:

===============
Getting Started
===============

In this tutorial, you will install the DuckDB Semantic Views extension, define your first semantic view over a single table, and query it in three different ways. By the end, you will understand the basic workflow: define dimensions and metrics once, then query any combination without writing GROUP BY or JOIN logic.

**Time:** 5 minutes

**Prerequisites:**

- DuckDB installed (CLI or Python package)
- Basic SQL knowledge (SELECT, GROUP BY, aggregate functions)

.. _tutorial-gs-install:

Install the Extension
=====================

.. tab-set::
   :sync-group: client

   .. tab-item:: DuckDB CLI
      :sync: cli

      Start the DuckDB CLI and load the extension:

      .. code-block:: sql

         INSTALL semantic_views FROM community;
         LOAD semantic_views;

   .. tab-item:: Python
      :sync: python

      .. code-block:: python

         import duckdb

         con = duckdb.connect()
         con.execute("INSTALL semantic_views FROM community")
         con.execute("LOAD semantic_views")


.. _tutorial-gs-create-table:

Create Sample Data
==================

Create an ``orders`` table with some sample rows:

.. code-block:: sql

   CREATE TABLE orders (
       id INTEGER,
       region VARCHAR,
       category VARCHAR,
       amount DECIMAL(10,2)
   );

   INSERT INTO orders VALUES
       (1, 'East',  'Hardware', 25.00),
       (2, 'East',  'Software', 50.00),
       (3, 'West',  'Hardware', 25.00),
       (4, 'West',  'Software', 100.00),
       (5, 'East',  'Hardware', 50.00);


.. _tutorial-gs-define-view:

Define a Semantic View
======================

Create a semantic view over the ``orders`` table. The ``TABLES`` clause declares the table with an alias and primary key. The ``DIMENSIONS`` clause names the columns available for grouping. The ``METRICS`` clause names the aggregations available for measurement.

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region,
       o.category AS o.category
   )
   METRICS (
       o.revenue AS SUM(o.amount),
       o.order_count AS COUNT(*)
   );

Each dimension and metric follows the pattern ``alias.name AS expression``:

- ``o.region AS o.region`` creates a dimension called ``region`` from the ``region`` column of the table aliased as ``o``.
- ``o.revenue AS SUM(o.amount)`` creates a metric called ``revenue`` that computes ``SUM(o.amount)``.

Verify the view was created:

.. code-block:: sql

   SHOW SEMANTIC VIEWS;

You should see the view listed with its metadata:

.. code-block:: text

   ┌─────────────────────┬───────────────┬───────────────┬───────────────┬─────────────┐
   │     created_on      │     name      │     kind      │ database_name │ schema_name │
   ├─────────────────────┼───────────────┼───────────────┼───────────────┼─────────────┤
   │ 2026-04-01T12:00:00 │ order_metrics │ SEMANTIC_VIEW │ memory        │ main        │
   └─────────────────────┴───────────────┴───────────────┴───────────────┴─────────────┘


.. _tutorial-gs-query:

Query the Semantic View
=======================

Query the semantic view using the :ref:`semantic_view() <ref-semantic-view-function>` table function. Pick any combination of the dimensions and metrics you defined.

**Dimensions and metrics together** (grouped aggregation):

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region', 'category'],
       metrics := ['revenue', 'order_count']
   );

.. code-block:: text

   ┌────────┬──────────┬─────────┬─────────────┐
   │ region │ category │ revenue │ order_count │
   ├────────┼──────────┼─────────┼─────────────┤
   │ East   │ Hardware │   75.00 │           2 │
   │ East   │ Software │   50.00 │           1 │
   │ West   │ Hardware │   25.00 │           1 │
   │ West   │ Software │  100.00 │           1 │
   └────────┴──────────┴─────────┴─────────────┘

**Dimensions only** (distinct values, no aggregation):

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region']
   );

.. code-block:: text

   ┌────────┐
   │ region │
   ├────────┤
   │ East   │
   │ West   │
   └────────┘

**Metrics only** (grand total, no GROUP BY):

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       metrics := ['revenue', 'order_count']
   );

.. code-block:: text

   ┌─────────┬─────────────┐
   │ revenue │ order_count │
   ├─────────┼─────────────┤
   │  250.00 │           5 │
   └─────────┴─────────────┘

**Filtering** with ``WHERE`` on the outer query:

.. code-block:: sql

   SELECT * FROM semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   ) WHERE region = 'East';

.. code-block:: text

   ┌────────┬─────────┐
   │ region │ revenue │
   ├────────┼─────────┤
   │ East   │  125.00 │
   └────────┴─────────┘


.. _tutorial-gs-inspect:

Inspect the Generated SQL
=========================

Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to see the SQL that the extension generates:

.. code-block:: sql

   SELECT * FROM explain_semantic_view('order_metrics',
       dimensions := ['region'],
       metrics := ['revenue']
   );

The output shows the expanded SQL and the DuckDB query plan. This is useful for verifying that the extension produces the query you expect.


.. _tutorial-gs-cleanup:

Clean Up
========

Drop the semantic view when you are done:

.. code-block:: sql

   DROP SEMANTIC VIEW order_metrics;


.. _tutorial-gs-summary:

What You Learned
================

You now know how to:

- Install and load the DuckDB Semantic Views extension
- Define a semantic view with :ref:`CREATE SEMANTIC VIEW <ref-create-semantic-view>`
- List registered views with :ref:`SHOW SEMANTIC VIEWS <ref-show-semantic-views>`
- Query any combination of dimensions and metrics with :ref:`semantic_view() <ref-semantic-view-function>`
- Use the three query modes: dimensions + metrics, dimensions only, metrics only
- Inspect generated SQL with :ref:`explain_semantic_view() <ref-explain-semantic-view>`
- Drop a semantic view with :ref:`DROP SEMANTIC VIEW <ref-drop-semantic-view>`

Next, learn how to model multiple tables with relationships in the :ref:`tutorial-multi-table` tutorial.
