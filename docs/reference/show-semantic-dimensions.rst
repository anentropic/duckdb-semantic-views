.. meta::
   :description: Syntax reference for SHOW SEMANTIC DIMENSIONS, which lists dimensions across one or all semantic views with optional filtering

.. _ref-show-semantic-dimensions:

==========================
SHOW SEMANTIC DIMENSIONS
==========================

Lists dimensions registered in one or all semantic views. Each row describes a single dimension with its name, source table, and inferred data type.


.. _ref-show-dims-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SHOW SEMANTIC DIMENSIONS
       [ LIKE '<pattern>' ]
       [ IN <name> ]
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

Returns one row per dimension with 6 columns:

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
     - Reserved for future use. Currently always an empty string for dimensions.


.. _ref-show-dims-examples:

Examples
========

**List dimensions for a single view:**

Given a semantic view ``orders_sv`` with three dimensions:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN orders_sv;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────────┬───────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name     │ name          │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────────┼───────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ customers      │ customer_name │           │
   │ memory        │ main        │ orders_sv            │ orders         │ order_date    │           │
   │ memory        │ main        │ orders_sv            │ customers      │ region        │           │
   └───────────────┴─────────────┴──────────────────────┴────────────────┴───────────────┴───────────┘

The ``table_name`` column shows the actual physical table name, not the alias used in the DDL.

**List dimensions across all views:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────────┬───────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name     │ name          │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────────┼───────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ customers      │ customer_name │           │
   │ memory        │ main        │ orders_sv            │ orders         │ order_date    │           │
   │ memory        │ main        │ orders_sv            │ customers      │ region        │           │
   │ memory        │ main        │ products_sv          │ products       │ product_name  │           │
   └───────────────┴─────────────┴──────────────────────┴────────────────┴───────────────┴───────────┘

Results are sorted by ``semantic_view_name`` then ``name``.

**Filter by pattern with LIKE (case-insensitive):**

Find all dimensions whose name contains "name", across all views:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS LIKE '%name%';

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────────┬───────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name     │ name          │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────────┼───────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ customers      │ customer_name │           │
   │ memory        │ main        │ products_sv          │ products       │ product_name  │           │
   └───────────────┴─────────────┴──────────────────────┴────────────────┴───────────────┴───────────┘

Because ``LIKE`` is case-insensitive, ``LIKE '%NAME%'`` produces the same results.

**Filter by pattern within a specific view:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS LIKE '%NAME%' IN orders_sv;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬───────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name          │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼───────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ customers  │ customer_name │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴───────────────┴───────────┘

**Filter by prefix with STARTS WITH (case-sensitive):**

Find dimensions whose name starts with "customer":

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS STARTS WITH 'customer';

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬───────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name          │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼───────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ customers  │ customer_name │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴───────────────┴───────────┘

``STARTS WITH`` is case-sensitive. ``STARTS WITH 'Customer'`` would return no results because the dimension is named ``customer_name`` (lowercase).

**Limit the number of results:**

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN orders_sv LIMIT 2;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────────┬───────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name     │ name          │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────────┼───────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ customers      │ customer_name │           │
   │ memory        │ main        │ orders_sv            │ orders         │ order_date    │           │
   └───────────────┴─────────────┴──────────────────────┴────────────────┴───────────────┴───────────┘

**Combine multiple clauses:**

All optional clauses can be combined, following the required order (``LIKE``, ``IN``, ``STARTS WITH``, ``LIMIT``):

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS LIKE '%name%' IN orders_sv STARTS WITH 'cust' LIMIT 10;

.. code-block:: text

   ┌───────────────┬─────────────┬──────────────────────┬────────────┬───────────────┬───────────┐
   │ database_name │ schema_name │ semantic_view_name   │ table_name │ name          │ data_type │
   ├───────────────┼─────────────┼──────────────────────┼────────────┼───────────────┼───────────┤
   │ memory        │ main        │ orders_sv            │ customers  │ customer_name │           │
   └───────────────┴─────────────┴──────────────────────┴────────────┴───────────────┴───────────┘

The dimension ``customer_name`` matches both ``LIKE '%name%'`` (contains "name") and ``STARTS WITH 'cust'`` (begins with "cust").

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
