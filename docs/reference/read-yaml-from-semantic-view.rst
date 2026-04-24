.. meta::
   :description: Syntax reference for READ_YAML_FROM_SEMANTIC_VIEW(), which exports a stored semantic view definition as a YAML string

.. _ref-read-yaml:

================================
READ_YAML_FROM_SEMANTIC_VIEW
================================

Scalar function that returns the YAML representation of a stored semantic view definition. The output is suitable for round-trip import via ``CREATE SEMANTIC VIEW ... FROM YAML``.

.. versionadded:: 0.7.0


.. _ref-read-yaml-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SELECT READ_YAML_FROM_SEMANTIC_VIEW('<view_name>')


.. _ref-read-yaml-params:

Parameters
==========

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Parameter
     - Type
     - Description
   * - ``<view_name>``
     - VARCHAR
     - The name of the semantic view to export. Supports unqualified (``my_view``), schema-qualified (``main.my_view``), and catalog-qualified (``memory.main.my_view``) names. The function resolves the bare view name from the last component.


.. _ref-read-yaml-output:

Output
======

Returns a single VARCHAR value containing the YAML representation of the semantic view definition. The YAML includes all user-declared clauses: tables, relationships, facts, dimensions, metrics, and materializations with their full configuration (comments, synonyms, access modifiers, NON ADDITIVE BY, window specs).


.. _ref-read-yaml-stripping:

Field Stripping
===============

Internal fields populated at define time are stripped from the YAML output before serialization. These fields are repopulated automatically when the definition is imported into a new environment:

.. list-table::
   :header-rows: 1
   :widths: 30 70

   * - Stripped Field
     - Reason
   * - ``column_type_names``
     - Column name list from DDL-time type inference. Regenerated at import time.
   * - ``column_types_inferred``
     - Column type IDs from DDL-time type inference. Regenerated at import time.
   * - ``created_on``
     - Creation timestamp. Set to the import time on re-creation.
   * - ``database_name``
     - Connection-specific database context. Set from the target connection.
   * - ``schema_name``
     - Connection-specific schema context. Set from the target connection.


.. _ref-read-yaml-examples:

Examples
========

**Export a semantic view to YAML:**

.. code-block:: sql

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   );

   SELECT READ_YAML_FROM_SEMANTIC_VIEW('order_metrics');

Sample output:

.. code-block:: yaml

   tables:
     - alias: o
       table: orders
       pk_columns:
         - id
   dimensions:
     - name: region
       expr: o.region
       source_table: o
   metrics:
     - name: revenue
       expr: SUM(o.amount)
       source_table: o

**Save YAML to a file:**

.. code-block:: sql

   COPY (SELECT READ_YAML_FROM_SEMANTIC_VIEW('order_metrics'))
   TO '/path/to/order_metrics.yaml' (FORMAT CSV, HEADER FALSE, QUOTE '');

**Schema-qualified view name:**

.. code-block:: sql

   SELECT READ_YAML_FROM_SEMANTIC_VIEW('main.order_metrics');

**Round-trip (export then import):**

.. code-block:: sql

   -- Export
   COPY (SELECT READ_YAML_FROM_SEMANTIC_VIEW('analytics'))
   TO '/tmp/analytics.yaml' (FORMAT CSV, HEADER FALSE, QUOTE '');

   -- Import into a new view
   CREATE SEMANTIC VIEW analytics_copy FROM YAML FILE '/tmp/analytics.yaml'


.. _ref-read-yaml-errors:

Error Cases
===========

**View does not exist:**

.. code-block:: sql

   SELECT READ_YAML_FROM_SEMANTIC_VIEW('nonexistent');

.. code-block:: text

   Error: semantic view 'nonexistent' does not exist
