.. meta::
   :description: Query facts directly as row-level columns using the facts parameter in semantic_view(), without aggregation

.. _howto-query-facts:

===========================
How to Query Facts Directly
===========================

This guide shows how to query facts as row-level columns using the ``facts`` parameter in :ref:`semantic_view() <ref-semantic-view-function>`. Unlike metrics, fact queries return individual rows without aggregation.

**Prerequisites:**

- A semantic view with a ``FACTS`` clause (see :ref:`howto-facts`)
- Familiarity with :ref:`semantic_view() <ref-semantic-view-function>` queries


.. _howto-query-facts-basic:

Query Facts
===========

Pass fact names in the ``facts`` parameter to retrieve them as row-level columns:

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       facts := ['net_price', 'tax_amount']
   );

Each row in the result contains the computed fact values -- no aggregation is applied. This is equivalent to a ``SELECT`` with the fact expressions inlined.


.. _howto-query-facts-dims:

Combine Facts with Dimensions
==============================

Add dimensions alongside facts. The dimensions appear as columns but do not trigger ``GROUP BY`` -- the output remains row-level:

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       dimensions := ['region'],
       facts := ['net_price']
   );

This returns one row per source row, with ``region`` and ``net_price`` columns.


.. _howto-query-facts-mutual:

Mutual Exclusion with Metrics
==============================

.. warning::

   Facts and metrics cannot be combined in the same query.

.. code-block:: sql

   -- This fails:
   SELECT * FROM semantic_view('analytics',
       facts := ['net_price'],
       metrics := ['total_revenue']
   );

.. code-block:: text

   semantic view 'analytics': cannot combine facts and metrics in the same query.
   Use facts := [...] OR metrics := [...], not both.

To get both row-level facts and aggregated metrics, run two separate queries.


.. _howto-query-facts-wildcard:

Wildcard Selection for Facts
=============================

Use ``alias.*`` to select all public facts for a table alias:

.. code-block:: sql

   SELECT * FROM semantic_view('analytics',
       facts := ['li.*']
   );

Private facts are excluded from wildcard expansion. See :ref:`howto-wildcard-selection` for details.


.. _howto-query-facts-verify:

Verify with explain_semantic_view()
====================================

Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to see the expanded SQL and query plan for a fact query:

.. code-block:: sql

   SELECT * FROM explain_semantic_view('analytics',
       facts := ['net_price', 'tax_amount']
   );

You can also use :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` to inspect ``FACT`` entries and their expressions.


.. _howto-query-facts-troubleshoot:

Troubleshooting
===============

**Unknown fact name**
   The fact name must match a fact declared in the ``FACTS`` clause. The error message lists available facts and suggests close matches.

**Private fact cannot be queried**
   Facts marked ``PRIVATE`` cannot be queried directly. They can only be referenced in metric expressions. Remove the ``PRIVATE`` keyword to make a fact queryable.

**Incompatible table paths**
   If a fact query combines facts and dimensions from tables that are not on the same root-to-leaf path in the relationship tree, the extension returns an error: ``fact query references objects from incompatible table paths``.
