.. meta::
   :description: Add COMMENT, WITH SYNONYMS, and PRIVATE/PUBLIC access modifiers to semantic view definitions and inspect them via DESCRIBE and SHOW

.. _howto-metadata-annotations:

=====================================
How to Use Metadata Annotations
=====================================

This guide shows how to annotate dimensions, metrics, facts, and tables with comments, synonyms, and access modifiers in a semantic view definition.

**Prerequisites:**

- A working semantic view with ``TABLES``, ``DIMENSIONS``, and ``METRICS`` (see :ref:`tutorial-multi-table`)
- Familiarity with :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` output


.. _howto-annotations-comment:

Add Comments
============

Comments are human-readable descriptions attached to any entry in the view definition. They appear in :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` output and in the ``comment`` column of ``SHOW`` commands.


View-level comment
------------------

Set a comment on the semantic view itself using :ref:`ALTER <ref-alter-semantic-view>`:

.. code-block:: sql

   ALTER SEMANTIC VIEW sales SET COMMENT = 'Revenue and order analytics for the North America region';

Remove a view-level comment:

.. code-block:: sql

   ALTER SEMANTIC VIEW sales UNSET COMMENT;

.. tip::

   View-level comments appear in the ``comment`` column of :ref:`SHOW SEMANTIC VIEWS <ref-show-semantic-views>` and as a ``SEMANTIC_VIEW`` object kind row in :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>`.


Table-level comment
-------------------

Add a ``COMMENT`` clause after the table declaration:

.. code-block:: sql
   :emphasize-lines: 3,4

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       o AS orders    PRIMARY KEY (id) COMMENT = 'Core order transactions',
       c AS customers PRIMARY KEY (id) COMMENT = 'Customer master data'
   )
   DIMENSIONS (o.region AS o.region)
   METRICS (o.revenue AS SUM(o.amount));


Comments on dimensions, metrics, and facts
-------------------------------------------

Add ``COMMENT`` after the expression on any entry:

.. code-block:: sql
   :emphasize-lines: 6,9,12

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id)
   )
   FACTS (
       li.net_price AS li.extended_price * (1 - li.discount) COMMENT = 'Price after discount'
   )
   DIMENSIONS (
       li.region AS li.region COMMENT = 'Sales region from shipping address'
   )
   METRICS (
       li.total_net AS SUM(li.net_price) COMMENT = 'Net revenue after discounts'
   );


.. _howto-annotations-synonyms:

Add Synonyms
============

Synonyms are alternative names for an entry. They are informational metadata -- they do not affect query resolution, but they appear in :ref:`DESCRIBE <ref-describe-semantic-view>` and ``SHOW`` output for discoverability.

Add ``WITH SYNONYMS`` after the expression (or after the ``COMMENT`` clause if both are present):

.. code-block:: sql
   :emphasize-lines: 6,9

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       o AS orders PRIMARY KEY (id) COMMENT = 'Order data' WITH SYNONYMS = ('transactions', 'purchases')
   )
   DIMENSIONS (
       o.region AS o.region WITH SYNONYMS = ('sales_region', 'territory')
   )
   METRICS (
       o.revenue AS SUM(o.amount) COMMENT = 'Total sales' WITH SYNONYMS = ('total_sales', 'gmv')
   );

Synonyms appear as a JSON array in :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` output (e.g., ``["sales_region","territory"]``) and in the ``synonyms`` column of :ref:`SHOW SEMANTIC DIMENSIONS <ref-show-semantic-dimensions>`.


.. _howto-annotations-access:

Set Access Modifiers (PRIVATE / PUBLIC)
=======================================

Metrics and facts support ``PRIVATE`` and ``PUBLIC`` access modifiers. ``PUBLIC`` is the default. ``PRIVATE`` items cannot be queried directly -- they can only be referenced by derived metric expressions.

.. code-block:: sql
   :emphasize-lines: 6,10,11,12

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       li AS line_items PRIMARY KEY (id)
   )
   FACTS (
       PRIVATE li.raw_margin AS li.price - li.cost
   )
   METRICS (
       li.total_revenue AS SUM(li.price),
       PRIVATE li.total_cost AS SUM(li.cost),
       li.total_margin AS SUM(li.raw_margin),
       profit AS total_revenue - total_cost
   );

In this example:

- ``raw_margin`` is a private fact -- it can be referenced by metrics (like ``total_margin``) but cannot be queried via ``facts := ['raw_margin']``.
- ``total_cost`` is a private metric -- it can be referenced by derived metrics (like ``profit``) but cannot be queried via ``metrics := ['total_cost']``.
- ``total_margin`` and ``profit`` are public (default) and use the private items to compute their values.

.. warning::

   ``PRIVATE`` is placed before the table alias (``PRIVATE li.total_cost``), not after the expression. Dimensions do not support access modifiers.


.. _howto-annotations-inspect:

Inspect Annotations
===================

Via DESCRIBE
------------

:ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` shows annotation properties as additional rows:

.. code-block:: sql

   DESCRIBE SEMANTIC VIEW sales;

Look for these property rows:

- ``COMMENT`` -- the comment text (conditional, only when set)
- ``SYNONYMS`` -- JSON array of synonyms (conditional, only when set)
- ``ACCESS_MODIFIER`` -- ``PUBLIC`` or ``PRIVATE`` (always emitted for facts and metrics)
- ``NON_ADDITIVE_BY`` -- non-additive dimension list (conditional, only for semi-additive metrics)
- ``WINDOW_SPEC`` -- reconstructed OVER clause (conditional, only for window metrics)

Via SHOW commands
-----------------

The :ref:`SHOW SEMANTIC DIMENSIONS <ref-show-semantic-dimensions>`, :ref:`SHOW SEMANTIC METRICS <ref-show-semantic-metrics>`, and :ref:`SHOW SEMANTIC FACTS <ref-show-semantic-facts>` commands include ``synonyms`` and ``comment`` columns in their output:

.. code-block:: sql

   SHOW SEMANTIC DIMENSIONS IN sales;

.. code-block:: text

   ┌───────────────┬─────────────┬────────────────────┬────────────┬────────┬───────────┬──────────────────────────────┬──────────────────────────────────────┐
   │ database_name │ schema_name │ semantic_view_name │ table_name │ name   │ data_type │ synonyms                     │ comment                              │
   ├───────────────┼─────────────┼────────────────────┼────────────┼────────┼───────────┼──────────────────────────────┼──────────────────────────────────────┤
   │ memory        │ main        │ sales              │ orders     │ region │           │ ["sales_region","territory"] │ Sales region from shipping address   │
   └───────────────┴─────────────┴────────────────────┴────────────┴────────┴───────────┴──────────────────────────────┴──────────────────────────────────────┘

.. tip::

   Private items are excluded from :ref:`SHOW COLUMNS IN SEMANTIC VIEW <ref-show-columns>` and from wildcard expansion (``alias.*``). They only appear in :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>`.


.. _howto-annotations-troubleshoot:

Troubleshooting
===============

**Comment not appearing in SHOW SEMANTIC VIEWS**
   Only view-level comments appear in :ref:`SHOW SEMANTIC VIEWS <ref-show-semantic-views>`. Table/dimension/metric/fact comments appear in :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` and in the ``comment`` column of :ref:`SHOW SEMANTIC DIMENSIONS <ref-show-semantic-dimensions>`, :ref:`SHOW SEMANTIC METRICS <ref-show-semantic-metrics>`, and :ref:`SHOW SEMANTIC FACTS <ref-show-semantic-facts>`.

**Cannot query a private metric or fact**
   Private items return an error when queried directly. Use them only in derived metric expressions. To make an item queryable again, recreate the view without the ``PRIVATE`` keyword.

**Synonyms not affecting query resolution**
   Synonyms are informational metadata only. They do not expand the set of names recognized by :ref:`semantic_view() <ref-semantic-view-function>` or :ref:`explain_semantic_view() <ref-explain-semantic-view>`. Use the declared name to query an item.

**COMMENT and WITH SYNONYMS order**
   When both are present on the same entry, ``COMMENT`` must come before ``WITH SYNONYMS``. The reverse order produces a parse error.
