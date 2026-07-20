.. meta::
   :description: Syntax reference for DROP SEMANTIC VIEW, which removes a semantic view definition from the catalog

.. _ref-drop-semantic-view:

====================
DROP SEMANTIC VIEW
====================

Removes a semantic view definition from the catalog.


.. _ref-drop-syntax:

Syntax
======

.. code-block:: sqlgrammar

   DROP SEMANTIC VIEW [ IF EXISTS ] <name>


.. _ref-drop-variants:

Statement Variants
==================

``DROP SEMANTIC VIEW <name>``
   Drops the named semantic view. Returns an error if the view does not exist.

``DROP SEMANTIC VIEW IF EXISTS <name>``
   Drops the named semantic view if it exists. If the view does not exist, the statement succeeds silently.

.. note::

   ``DROP`` participates in your surrounding transaction (``BEGIN ... ROLLBACK`` restores the view). ``DROP SEMANTIC VIEW`` (without ``IF EXISTS``) raises ``semantic view '<name>' does not exist`` when the view is absent at check time; ``IF EXISTS`` keeps its silent-no-op behaviour. The existence check and the delete are atomic only inside an explicit transaction -- under autocommit a drop that another process commits in the window between them is not detected. See :ref:`explanation-transactional-ddl` for the guard window and how to close it.

.. note::

   Requires a writable database. On a read-only database this statement fails with DuckDB's standard ``Cannot execute statement of type "..." which is attached in read-only mode!`` error. See :ref:`explanation-txn-ddl-readonly`.


.. _ref-drop-params:

Parameters
==========

``<name>``
   The name of the semantic view to drop. The name is folded to lowercase and
   matched case-insensitively, quoted or not -- ``DROP SEMANTIC VIEW SALES``,
   ``DROP SEMANTIC VIEW sales``, and ``DROP SEMANTIC VIEW "sales"`` all refer to
   the same view, following DuckDB's identifier semantics.

.. note::

   A view created before v0.11.0 via a *quoted* mixed-case identifier
   (e.g. ``CREATE SEMANTIC VIEW "Sales"``) kept its original casing and is no
   longer reachable by any spelling. Drop and recreate it, or rename its stored
   catalog row to lowercase.


.. _ref-drop-examples:

Examples
========

.. code-block:: sql

   -- Drop an existing view
   DROP SEMANTIC VIEW order_metrics;

   -- Drop only if it exists (no error if missing)
   DROP SEMANTIC VIEW IF EXISTS order_metrics;
