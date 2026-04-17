.. meta::
   :description: Complete SQL syntax reference for all CREATE, ALTER, DROP, DESCRIBE, SHOW, GET_DDL, and query function statements

.. _reference:

=========
Reference
=========

SQL syntax reference for all DuckDB Semantic Views statements and functions.

**DDL statements**

- :ref:`ref-create-semantic-view` -- Create a new semantic view with tables, relationships, dimensions, metrics, and facts.
- :ref:`ref-alter-semantic-view` -- Rename, set, or unset comment on a semantic view.
- :ref:`ref-drop-semantic-view` -- Remove a semantic view from the catalog.
- :ref:`ref-describe-semantic-view` -- Inspect the full definition of a semantic view.
- :ref:`ref-show-semantic-views` -- List all registered semantic views with optional filtering.
- :ref:`ref-show-semantic-dimensions` -- List dimensions across one or all semantic views.
- :ref:`ref-show-semantic-metrics` -- List metrics across one or all semantic views.
- :ref:`ref-show-semantic-facts` -- List facts across one or all semantic views.
- :ref:`ref-show-dims-for-metric` -- List dimensions safe to use with a specific metric (fan trap aware).
- :ref:`ref-show-columns` -- List all queryable columns in a semantic view with types, expressions, and comments.
- :ref:`ref-get-ddl` -- Retrieve the full CREATE DDL text for a stored semantic view.

**Query functions**

- :ref:`ref-semantic-view-function` -- Query a semantic view with any combination of dimensions and metrics.
- :ref:`ref-explain-semantic-view` -- Inspect the SQL generated for a semantic view query.

**Error reference**

- :ref:`ref-error-messages` -- Error messages, causes, and fixes.

.. toctree::
   :hidden:

   create-semantic-view
   alter-semantic-view
   drop-semantic-view
   describe-semantic-view
   show-semantic-views
   show-semantic-dimensions
   show-semantic-metrics
   show-semantic-facts
   show-semantic-dimensions-for-metric
   show-columns-semantic-view
   get-ddl
   semantic-view-function
   explain-semantic-view-function
   error-messages
