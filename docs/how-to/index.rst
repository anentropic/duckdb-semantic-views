.. meta::
   :description: Goal-oriented guides for FACTS, derived metrics, role-playing dimensions, fan trap resolution, data source connectivity, metadata annotations, semi-additive metrics, window metrics, wildcard selection, and fact queries

.. _how-to-guides:

=============
How-To Guides
=============

Goal-oriented guides for specific tasks with DuckDB Semantic Views.

- :ref:`howto-facts` -- Define reusable row-level expressions that can be referenced inside metric aggregations.
- :ref:`howto-derived-metrics` -- Compose metrics from other metrics using arithmetic without repeating aggregate logic.
- :ref:`howto-role-playing` -- Join the same physical table multiple times under different aliases for distinct relationships.
- :ref:`howto-fan-traps` -- Understand, detect, and resolve fan traps that inflate aggregation results in multi-table views.
- :ref:`howto-data-sources` -- Connect semantic views to CSV, Parquet, Iceberg, and database tables.
- :ref:`howto-metadata-annotations` -- Add comments, synonyms, and access modifiers to dimensions, metrics, facts, and tables.
- :ref:`howto-semi-additive` -- Define metrics with NON ADDITIVE BY for snapshot data like account balances and inventory levels.
- :ref:`howto-window-metrics` -- Define window function metrics for rolling averages, lag comparisons, and rankings using OVER clauses.
- :ref:`howto-wildcard-selection` -- Select all dimensions, metrics, or facts for a table alias using wildcard patterns in queries.
- :ref:`howto-query-facts` -- Query facts directly as row-level columns without aggregation.

.. toctree::
   :hidden:

   facts
   derived-metrics
   role-playing-dimensions
   fan-traps
   data-sources
   metadata-annotations
   semi-additive-metrics
   window-metrics
   wildcard-selection
   query-facts
