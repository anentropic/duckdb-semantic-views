.. meta::
   :description: Syntax reference for SHOW SEMANTIC VIEWS, including optional LIKE, STARTS WITH, and LIMIT filtering clauses

.. _ref-show-semantic-views:

====================
SHOW SEMANTIC VIEWS
====================

Lists all registered semantic views, with optional filtering by name pattern, prefix, or row count.


.. _ref-show-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW SEMANTIC VIEWS
       [ LIKE '<pattern>' ]
       [ STARTS WITH '<prefix>' ]
       [ LIMIT <rows> ]

All clauses are optional. When multiple clauses appear, they must follow the order shown above.


.. _ref-show-filtering:

Optional Filtering Clauses
==========================

``LIKE '<pattern>'``
   Filters views to those whose name matches the pattern. Uses SQL ``LIKE`` pattern syntax: ``%`` matches any sequence of characters, ``_`` matches a single character. Matching is **case-insensitive** (the extension maps ``LIKE`` to DuckDB's ``ILIKE``). The pattern must be enclosed in single quotes.

``STARTS WITH '<prefix>'``
   Filters views to those whose name begins with the prefix. Matching is **case-sensitive**. The prefix must be enclosed in single quotes.

``LIMIT <rows>``
   Restricts the output to the first *rows* results. Must be a positive integer.

When ``LIKE`` and ``STARTS WITH`` are both present, a view must satisfy both conditions (they are combined with ``AND``).

.. warning::

   Clause order is enforced. ``LIKE`` must come before ``STARTS WITH``, and ``STARTS WITH`` must come before ``LIMIT``. Placing clauses out of order produces a syntax error.


.. _ref-show-output:

Output Columns
==============

Returns one row per registered semantic view with 5 columns:

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Column
     - Type
     - Description
   * - ``created_on``
     - VARCHAR
     - Timestamp when the semantic view was created.
   * - ``name``
     - VARCHAR
     - The semantic view name.
   * - ``kind``
     - VARCHAR
     - Always ``SEMANTIC_VIEW``.
   * - ``database_name``
     - VARCHAR
     - The DuckDB database containing the view (e.g., ``memory``).
   * - ``schema_name``
     - VARCHAR
     - The DuckDB schema containing the view (e.g., ``main``).


.. _ref-show-examples:

Examples
========

**List all semantic views:**

The ``created_on`` column contains a non-deterministic timestamp. To get deterministic output, select specific columns from the underlying table function:

.. code-block:: sql

   SELECT name, kind, database_name, schema_name
   FROM (SHOW SEMANTIC VIEWS);

.. code-block:: text

   ┌─────────────────┬───────────────┬───────────────┬─────────────┐
   │ name            │ kind          │ database_name │ schema_name │
   ├─────────────────┼───────────────┼───────────────┼─────────────┤
   │ order_metrics   │ SEMANTIC_VIEW │ memory        │ main        │
   │ sales_analytics │ SEMANTIC_VIEW │ memory        │ main        │
   └─────────────────┴───────────────┴───────────────┴─────────────┘

If no semantic views are registered, the result set is empty.

**Filter by pattern with LIKE (case-insensitive):**

Find all views whose name contains "order":

.. code-block:: sql

   SHOW SEMANTIC VIEWS LIKE '%order%';

.. code-block:: text

   ┌─────────────────────┬───────────────┬───────────────┬───────────────┬─────────────┐
   │ created_on          │ name          │ kind          │ database_name │ schema_name │
   ├─────────────────────┼───────────────┼───────────────┼───────────────┼─────────────┤
   │ 2026-04-02 10:30:00 │ order_metrics │ SEMANTIC_VIEW │ memory        │ main        │
   └─────────────────────┴───────────────┴───────────────┴───────────────┴─────────────┘

Because ``LIKE`` is case-insensitive, ``LIKE '%ORDER%'`` produces the same results.

**Filter by prefix with STARTS WITH (case-sensitive):**

Find views whose name starts with "sales":

.. code-block:: sql

   SHOW SEMANTIC VIEWS STARTS WITH 'sales';

.. code-block:: text

   ┌─────────────────────┬─────────────────┬───────────────┬───────────────┬─────────────┐
   │ created_on          │ name            │ kind          │ database_name │ schema_name │
   ├─────────────────────┼─────────────────┼───────────────┼───────────────┼─────────────┤
   │ 2026-04-02 10:35:00 │ sales_analytics │ SEMANTIC_VIEW │ memory        │ main        │
   └─────────────────────┴─────────────────┴───────────────┴───────────────┴─────────────┘

``STARTS WITH`` is case-sensitive. ``STARTS WITH 'Sales'`` would return no results because the view is named ``sales_analytics`` (lowercase).

**Limit the number of results:**

.. code-block:: sql

   SHOW SEMANTIC VIEWS LIMIT 1;

.. code-block:: text

   ┌─────────────────────┬───────────────┬───────────────┬───────────────┬─────────────┐
   │ created_on          │ name          │ kind          │ database_name │ schema_name │
   ├─────────────────────┼───────────────┼───────────────┼───────────────┼─────────────┤
   │ 2026-04-02 10:30:00 │ order_metrics │ SEMANTIC_VIEW │ memory        │ main        │
   └─────────────────────┴───────────────┴───────────────┴───────────────┴─────────────┘

**Combine multiple clauses:**

All optional clauses can be combined, following the required order (``LIKE``, ``STARTS WITH``, ``LIMIT``):

.. code-block:: sql

   SHOW SEMANTIC VIEWS LIKE '%a%' STARTS WITH 'sales' LIMIT 10;

.. code-block:: text

   ┌─────────────────────┬─────────────────┬───────────────┬───────────────┬─────────────┐
   │ created_on          │ name            │ kind          │ database_name │ schema_name │
   ├─────────────────────┼─────────────────┼───────────────┼───────────────┼─────────────┤
   │ 2026-04-02 10:35:00 │ sales_analytics │ SEMANTIC_VIEW │ memory        │ main        │
   └─────────────────────┴─────────────────┴───────────────┴───────────────┴─────────────┘

The view ``sales_analytics`` matches both ``LIKE '%a%'`` (contains "a") and ``STARTS WITH 'sales'`` (begins with "sales").

**Select specific columns to skip the timestamp:**

.. code-block:: sql

   SELECT name, kind, database_name, schema_name
   FROM (SHOW SEMANTIC VIEWS)
   WHERE name ILIKE '%ics%';

.. code-block:: text

   ┌─────────────────┬───────────────┬───────────────┬─────────────┐
   │ name            │ kind          │ database_name │ schema_name │
   ├─────────────────┼───────────────┼───────────────┼─────────────┤
   │ order_metrics   │ SEMANTIC_VIEW │ memory        │ main        │
   │ sales_analytics │ SEMANTIC_VIEW │ memory        │ main        │
   └─────────────────┴───────────────┴───────────────┴─────────────┘

**The statement is case-insensitive:**

.. code-block:: sql

   show semantic views like '%order%';
