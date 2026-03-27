.. meta::
   :description: Goal-oriented guides for FACTS, derived metrics, role-playing dimensions, fan trap resolution, and data source connectivity

.. _how-to-guides:

=============
How-To Guides
=============

Goal-oriented guides for specific tasks with DuckDB Semantic Views.

- :ref:`howto-facts` — Define reusable row-level expressions that can be referenced inside metric aggregations.
- :ref:`howto-derived-metrics` — Compose metrics from other metrics using arithmetic without repeating aggregate logic.
- :ref:`howto-role-playing` — Join the same physical table multiple times under different aliases for distinct relationships.
- :ref:`howto-fan-traps` — Understand, detect, and resolve fan traps that inflate aggregation results in multi-table views.
- :ref:`howto-data-sources` — Connect semantic views to CSV, Parquet, Iceberg, and database tables.

.. toctree::
   :hidden:

   facts
   derived-metrics
   role-playing-dimensions
   fan-traps
   data-sources
