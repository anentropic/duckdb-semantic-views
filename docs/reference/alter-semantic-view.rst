.. meta::
   :description: Syntax reference for ALTER SEMANTIC VIEW RENAME TO, which renames an existing view while preserving its full definition

.. _ref-alter-semantic-view:

======================
ALTER SEMANTIC VIEW
======================

Renames an existing semantic view. The view definition, including all tables, relationships, dimensions, metrics, and facts, is preserved under the new name. Queries using the old name will fail after the rename.

.. note::

   ``ALTER SEMANTIC VIEW`` currently supports only the ``RENAME TO`` operation. Additional alter operations (adding or removing columns, modifying expressions) are not yet available.


.. _ref-alter-syntax:

Syntax
======

.. code-block:: sqlgrammar

   ALTER SEMANTIC VIEW [ IF EXISTS ] <name> RENAME TO <new_name>


.. _ref-alter-variants:

Statement Variants
==================

``ALTER SEMANTIC VIEW <name> RENAME TO <new_name>``
   Renames the semantic view from ``<name>`` to ``<new_name>``. Returns an error if ``<name>`` does not exist or if ``<new_name>`` already exists.

``ALTER SEMANTIC VIEW IF EXISTS <name> RENAME TO <new_name>``
   Renames the semantic view if it exists. If ``<name>`` does not exist, the statement succeeds silently without modifying anything. Returns an error if ``<new_name>`` already exists.


.. _ref-alter-params:

Parameters
==========

``<name>``
   The current name of the semantic view to rename.

``<new_name>``
   The new name for the semantic view. Must not match the name of an existing semantic view.


.. _ref-alter-output:

Output Columns
==============

Returns a single row with 2 columns:

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Column
     - Type
     - Description
   * - ``old_name``
     - VARCHAR
     - The original semantic view name before the rename.
   * - ``new_name``
     - VARCHAR
     - The new semantic view name after the rename.


.. _ref-alter-examples:

Examples
========

**Rename a semantic view:**

.. code-block:: sql

   ALTER SEMANTIC VIEW sales_view RENAME TO revenue_view;

After the rename, queries must use the new name:

.. code-block:: sql

   -- This works
   SELECT * FROM semantic_view('revenue_view',
       dimensions := ['region'],
       metrics := ['total_amount']
   );

   -- This fails: "semantic view 'sales_view' does not exist"
   SELECT * FROM semantic_view('sales_view',
       dimensions := ['region'],
       metrics := ['total_amount']
   );

**Rename with IF EXISTS (safe no-op):**

.. code-block:: sql

   -- Succeeds silently if 'old_reports' does not exist
   ALTER SEMANTIC VIEW IF EXISTS old_reports RENAME TO new_reports;

**Error: target name already exists:**

.. code-block:: sql

   -- Assuming both 'sales' and 'inventory' exist
   ALTER SEMANTIC VIEW sales RENAME TO inventory;

.. code-block:: text

   Error: semantic view 'inventory' already exists

**The statement is case-insensitive:**

.. code-block:: sql

   alter semantic view sales_view rename to revenue_view;

.. warning::

   Snowflake's ``ALTER SEMANTIC VIEW`` supports additional operations beyond ``RENAME TO``, including ``ADD``, ``DROP``, and ``ALTER`` for individual entities (dimensions, metrics, facts, relationships). This extension currently supports only ``RENAME TO``.
