.. meta::
   :description: Syntax reference for SHOW SEMANTIC DIMENSIONS, which lists dimensions across one or all semantic views with optional filtering

.. _ref-show-semantic-dimensions:

==========================
SHOW SEMANTIC DIMENSIONS
==========================

Lists dimensions registered in one or all semantic views. Each row describes a single dimension with its name, source table, inferred data type, synonyms, and comment.


.. _ref-show-dims-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW SEMANTIC DIMENSIONS
       [ LIKE '<pattern>' ]
       [ IN <name> ]
       [ IN SCHEMA <schema_name> | IN DATABASE <database_name> ]
       [ STARTS WITH '<prefix>' ]
       [ LIMIT <rows> ]

All clauses are optional. When multiple clauses appear, they must follow the order shown above.


.. _ref-show-dims-variants:

Statement Variants
==================

``SHOW SEMANTIC DIMENSIONS``
   Returns dimensions across all registered semantic views, sorted by semantic view name and then dimension name.

``SHOW SEMANTIC DIMENSIONS IN <name>``
   Returns dimensions for the specified semantic view only, sorted by dimension name. Returns an error if the view does not exist.


.. _ref-show-dims-params:

Parameters
==========

``<name>``
   The name of the semantic view. Required only for the single-view form (``IN`` clause). Returns an error if the view does not exist.


.. _ref-show-dims-filtering:

Optional Filtering Clauses
==========================

``LIKE '<pattern>'``
   Filters dimensions to those whose name matches the pattern. Uses SQL ``LIKE`` pattern syntax: ``%`` matches any sequence of characters, ``_`` matches a single character. Matching is **case-insensitive** (the extension maps ``LIKE`` to DuckDB's ``ILIKE``). The pattern must be enclosed in single quotes.

``IN SCHEMA <schema_name>``
   Filters dimensions to those in semantic views belonging to the specified schema.

``IN DATABASE <database_name>``
   Filters dimensions to those in semantic views belonging to the specified database.

``STARTS WITH '<prefix>'``
   Filters dimensions to those whose name begins with the prefix. Matching is **case-sensitive**. The prefix must be enclosed in single quotes.

``LIMIT <rows>``
   Restricts the output to the first *rows* results. Must be a positive integer.

When ``LIKE`` and ``STARTS WITH`` are both present, a dimension must satisfy both conditions (they are combined with ``AND``).

.. warning::

   Clause order is enforced. ``LIKE`` must come before ``IN``, and ``STARTS WITH`` must come after ``IN``. Placing clauses out of order produces a syntax error. For example, ``SHOW SEMANTIC DIMENSIONS IN my_view LIKE '%x%'`` is not valid.


.. _ref-show-dims-output:

Output Columns
==============

Returns one row per dimension with 8 columns:

.. list-table::
   :header-rows: 1
   :widths: 22 12 66

   * - Column
     - Type
     - Description
   * - ``database_name``
     - VARCHAR
     - The DuckDB database containing the semantic view.
   * - ``schema_name``
     - VARCHAR
     - The DuckDB schema containing the semantic view.
   * - ``semantic_view_name``
     - VARCHAR
     - The semantic view this dimension belongs to.
   * - ``table_name``
     - VARCHAR
     - The physical table name the dimension is scoped to. Empty string if no source table is associated.
   * - ``name``
     - VARCHAR
     - The dimension name as declared in the ``DIMENSIONS`` clause.
   * - ``data_type``
     - VARCHAR
     - The inferred data type. Empty string if not resolved.
   * - ``synonyms``
     - VARCHAR
     - JSON array of synonym strings (e.g., ``["territory","sales_region"]``). Empty string if no synonyms are set.
   * - ``comment``
     - VARCHAR
     - The dimension comment text. Empty string if no comment is set.


.. _ref-show-dims-examples:

Examples
========

**List dimensions for a single view:**

Given a semantic view ``orders_sv`` with three dimensions:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN orders_sv;

.. code-block:: text

   ┌───────────────┬─────────────┬────────────────────┬────────────┬───────────────┬───────────┬──────────┬─────────┐
   │ database_name │ schema_name │ semantic_view_name │ table_name │ name          │ data_type │ synonyms │ comment │
   ├───────────────┼─────────────┼────────────────────┼────────────┼───────────────┼───────────┼──────────┼─────────┤
   │ memory        │ main        │ orders_sv          │ customers  │ customer_name │ VARCHAR   │          │         │
   │ memory        │ main        │ orders_sv          │ orders     │ order_date    │ DATE      │          │         │
   │ memory        │ main        │ orders_sv          │ customers  │ region        │ VARCHAR   │          │         │
   └───────────────┴─────────────┴────────────────────┴────────────┴───────────────┴───────────┴──────────┴─────────┘

The ``table_name`` column shows the actual physical table name, not the alias used in the DDL.

**List dimensions across all views:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS;

Results are sorted by ``semantic_view_name`` then ``name``.

**Filter by pattern with LIKE (case-insensitive):**

Find all dimensions whose name contains "name", across all views:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS LIKE '%name%';

Because ``LIKE`` is case-insensitive, ``LIKE '%NAME%'`` produces the same results.

**Filter by schema:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN SCHEMA main;

**Filter by pattern within a specific view:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS LIKE '%NAME%' IN orders_sv;

**Error: view does not exist:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN nonexistent_view;

.. code-block:: text

   Error: semantic view 'nonexistent_view' does not exist

**The statement is case-insensitive:**

.. code-block:: sql

   show semantic dimensions in orders_sv;

.. tip::

   To see only the dimensions that are safe to use with a specific metric (avoiding fan traps in multi-table views), use :ref:`SHOW SEMANTIC DIMENSIONS ... FOR METRIC <ref-show-dims-for-metric>` instead.
