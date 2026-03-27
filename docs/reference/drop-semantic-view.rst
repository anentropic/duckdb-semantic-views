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


.. _ref-drop-params:

Parameters
==========

``<name>``
   The name of the semantic view to drop. Case-sensitive.


.. _ref-drop-examples:

Examples
========

.. code-block:: sql

   -- Drop an existing view
   DROP SEMANTIC VIEW order_metrics;

   -- Drop only if it exists (no error if missing)
   DROP SEMANTIC VIEW IF EXISTS order_metrics;
