.. meta::
   :description: A declarative DuckDB extension that lets you define dimensions and metrics once, then query any combination without writing JOIN or GROUP BY logic

.. _overview:

========================
DuckDB Semantic Views
========================

Define dimensions and metrics once, then query them in any combination. The extension writes the GROUP BY and JOIN logic for you.

DuckDB Semantic Views is a loadable DuckDB extension that implements a declarative semantic layer. You declare tables, relationships, dimensions, and metrics using native SQL DDL, then query any combination with a table function. The extension generates the correct SQL (SELECT, FROM, JOIN, GROUP BY) and DuckDB executes it.

.. grid:: 1 2 3 3
   :gutter: 3

   .. grid-item-card:: Getting Started
      :link: tutorial-getting-started
      :link-type: ref

      Install the extension, create your first semantic view, and run your first query in 5 minutes.

   .. grid-item-card:: Multi-Table Semantic Views
      :link: tutorial-multi-table
      :link-type: ref

      Learn to model relationships between tables and query across them.

   .. grid-item-card:: DDL Reference
      :link: ref-create-semantic-view
      :link-type: ref

      Full syntax reference for ``CREATE SEMANTIC VIEW`` and all DDL statements.

.. grid:: 1 2 3 3
   :gutter: 3

   .. grid-item-card:: How-To Guides
      :link: how-to-guides
      :link-type: ref

      Task-oriented guides for FACTS, derived metrics, role-playing dimensions, fan traps, and data sources.

   .. grid-item-card:: Concepts
      :link: explanation
      :link-type: ref

      Understand how semantic views differ from regular views and how they compare to Snowflake.

   .. grid-item-card:: Query Reference
      :link: ref-semantic-view-function
      :link-type: ref

      Full reference for the ``semantic_view()`` and ``explain_semantic_view()`` query functions.


.. toctree::
   :hidden:

   tutorials/index
   how-to/index
   explanation/index
   reference/index
