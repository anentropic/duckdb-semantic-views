.. meta::
   :description: Syntax reference for SHOW SEMANTIC FACTS, which lists named row-level expressions across one or all semantic views with optional filtering

.. _ref-show-semantic-facts:

=====================
SHOW SEMANTIC FACTS
=====================

Lists facts (named row-level expressions) registered in one or all semantic views. Each row describes a single fact with its name, expression, and source table. Views that have no facts defined return no rows.


.. _ref-show-facts-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW SEMANTIC FACTS
       [ LIKE '<pattern>' ]
       [ IN <name> ]
       [ STARTS WITH '<prefix>' ]
       [ LIMIT <rows> ]

All clauses are optional. When multiple clauses appear, they must follow the order shown above.


.. _ref-show-facts-variants:

Statement Variants
==================

``SHOW SEMANTIC FACTS``
   Returns facts across all registered semantic views, sorted by semantic view name and then fact name. Views with no ``FACTS`` clause are omitted from the result.

``SHOW SEMANTIC FACTS IN <name>``
   Returns facts for the specified semantic view only, sorted by fact name. Returns an error if the view does not exist. Returns an empty result if the view has no facts.


.. _ref-show-facts-params:

Parameters
==========

``<name>``
   The name of the semantic view. Required only for the single-view form (``IN`` clause). Returns an error if the view does not exist.


.. _ref-show-facts-filtering:

Optional Filtering Clauses
==========================

``LIKE '<pattern>'``
   Filters facts to those whose name matches the pattern. Uses SQL ``LIKE`` pattern syntax: ``%`` matches any sequence of characters, ``_`` matches a single character. Matching is **case-insensitive** (the extension maps ``LIKE`` to DuckDB's ``ILIKE``). The pattern must be enclosed in single quotes.

``STARTS WITH '<prefix>'``
   Filters facts to those whose name begins with the prefix. Matching is **case-sensitive**. The prefix must be enclosed in single quotes.

``LIMIT <rows>``
   Restricts the output to the first *rows* results. Must be a positive integer.

When ``LIKE`` and ``STARTS WITH`` are both present, a fact must satisfy both conditions (they are combined with ``AND``).

.. warning::

   Clause order is enforced. ``LIKE`` must come before ``IN``, and ``STARTS WITH`` must come after ``IN``. Placing clauses out of order produces a syntax error.


.. _ref-show-facts-output:

Output Columns
==============

Returns one row per fact with 4 columns:

.. list-table::
   :header-rows: 1
   :widths: 22 12 66

   * - Column
     - Type
     - Description
   * - ``semantic_view_name``
     - VARCHAR
     - The semantic view this fact belongs to.
   * - ``name``
     - VARCHAR
     - The fact name as declared in the ``FACTS`` clause.
   * - ``expr``
     - VARCHAR
     - The row-level SQL expression defining the fact (e.g., ``li.extended_price * (1 - li.discount)``).
   * - ``source_table``
     - VARCHAR
     - The table alias the fact is scoped to.

.. note::

   Unlike :ref:`SHOW SEMANTIC DIMENSIONS <ref-show-semantic-dimensions>` and :ref:`SHOW SEMANTIC METRICS <ref-show-semantic-metrics>`, the facts output does not include a ``data_type`` column. Facts are row-level expressions that are inlined into metrics at expansion time, so their output type depends on the context in which they are used.


.. _ref-show-facts-examples:

Examples
========

**List facts for a single view:**

Given a semantic view ``orders_sv`` with one fact:

.. code-block:: sql

   SHOW SEMANTIC FACTS IN orders_sv;

.. code-block:: text

   ┌──────────────────────┬────────────┬──────────┬──────────────┐
   │ semantic_view_name   │ name       │ expr     │ source_table │
   ├──────────────────────┼────────────┼──────────┼──────────────┤
   │ orders_sv            │ raw_amount │ o.amount │ o            │
   └──────────────────────┴────────────┴──────────┴──────────────┘

**List facts across all views:**

.. code-block:: sql

   SHOW SEMANTIC FACTS;

Views with no facts are omitted. If only ``orders_sv`` has a ``FACTS`` clause:

.. code-block:: text

   ┌──────────────────────┬────────────┬──────────┬──────────────┐
   │ semantic_view_name   │ name       │ expr     │ source_table │
   ├──────────────────────┼────────────┼──────────┼──────────────┤
   │ orders_sv            │ raw_amount │ o.amount │ o            │
   └──────────────────────┴────────────┴──────────┴──────────────┘

**Filter by pattern with LIKE (case-insensitive):**

Find all facts whose name contains "amount":

.. code-block:: sql

   SHOW SEMANTIC FACTS LIKE '%amount%';

.. code-block:: text

   ┌──────────────────────┬────────────┬──────────┬──────────────┐
   │ semantic_view_name   │ name       │ expr     │ source_table │
   ├──────────────────────┼────────────┼──────────┼──────────────┤
   │ orders_sv            │ raw_amount │ o.amount │ o            │
   └──────────────────────┴────────────┴──────────┴──────────────┘

Because ``LIKE`` is case-insensitive, ``LIKE '%AMOUNT%'`` produces the same results.

**Filter by prefix with STARTS WITH (case-sensitive):**

Find facts whose name starts with "raw":

.. code-block:: sql

   SHOW SEMANTIC FACTS STARTS WITH 'raw';

.. code-block:: text

   ┌──────────────────────┬────────────┬──────────┬──────────────┐
   │ semantic_view_name   │ name       │ expr     │ source_table │
   ├──────────────────────┼────────────┼──────────┼──────────────┤
   │ orders_sv            │ raw_amount │ o.amount │ o            │
   └──────────────────────┴────────────┴──────────┴──────────────┘

``STARTS WITH`` is case-sensitive. ``STARTS WITH 'Raw'`` would return no results because the fact is named ``raw_amount`` (lowercase).

**Limit the number of results:**

.. code-block:: sql

   SHOW SEMANTIC FACTS IN orders_sv LIMIT 1;

.. code-block:: text

   ┌──────────────────────┬────────────┬──────────┬──────────────┐
   │ semantic_view_name   │ name       │ expr     │ source_table │
   ├──────────────────────┼────────────┼──────────┼──────────────┤
   │ orders_sv            │ raw_amount │ o.amount │ o            │
   └──────────────────────┴────────────┴──────────┴──────────────┘

**Chained facts:**

Facts can reference other facts. Consider a view with two chained facts:

.. code-block:: sql

   CREATE SEMANTIC VIEW tpch_analysis AS
   TABLES (
       li AS line_items PRIMARY KEY (id)
   )
   FACTS (
       li.net_price  AS li.extended_price * (1 - li.discount),
       li.tax_amount AS li.net_price * li.tax_rate
   )
   DIMENSIONS (
       li.status AS li.status
   )
   METRICS (
       li.revenue AS SUM(li.net_price)
   );

   SHOW SEMANTIC FACTS IN tpch_analysis;

.. code-block:: text

   ┌──────────────────────┬────────────┬──────────────────────────────────────────┬──────────────┐
   │ semantic_view_name   │ name       │ expr                                     │ source_table │
   ├──────────────────────┼────────────┼──────────────────────────────────────────┼──────────────┤
   │ tpch_analysis        │ net_price  │ li.extended_price * (1 - li.discount)    │ li           │
   │ tpch_analysis        │ tax_amount │ li.net_price * li.tax_rate               │ li           │
   └──────────────────────┴────────────┴──────────────────────────────────────────┴──────────────┘

The ``expr`` column shows each fact's expression as declared. The extension resolves chained references (``li.net_price`` in ``tax_amount``) at query expansion time.

**Filter chained facts with STARTS WITH:**

.. code-block:: sql

   SHOW SEMANTIC FACTS IN tpch_analysis STARTS WITH 'net';

.. code-block:: text

   ┌──────────────────────┬───────────┬───────────────────────────────────────┬──────────────┐
   │ semantic_view_name   │ name      │ expr                                  │ source_table │
   ├──────────────────────┼───────────┼───────────────────────────────────────┼──────────────┤
   │ tpch_analysis        │ net_price │ li.extended_price * (1 - li.discount) │ li           │
   └──────────────────────┴───────────┴───────────────────────────────────────┴──────────────┘

**Error: view does not exist:**

.. code-block:: sql

   SHOW SEMANTIC FACTS IN nonexistent_view;

.. code-block:: text

   Error: semantic view 'nonexistent_view' does not exist
