.. meta::
   :description: Syntax reference for GET_DDL(), which returns the full CREATE DDL text for a semantic view

.. _ref-get-ddl:

=========
GET_DDL
=========

Scalar function that returns the full ``CREATE OR REPLACE SEMANTIC VIEW`` DDL text for a stored semantic view. The output is a syntactically valid DDL statement that can be executed to recreate the view.


.. _ref-get-ddl-syntax:

Syntax
======

.. code-block:: sqlgrammar

   SELECT GET_DDL('<object_type>', '<name>' [, <use_fully_qualified_names>])


.. _ref-get-ddl-params:

Parameters
==========

.. list-table::
   :header-rows: 1
   :widths: 20 15 65

   * - Parameter
     - Type
     - Description
   * - ``<object_type>``
     - VARCHAR
     - The object type. Only ``'SEMANTIC_VIEW'`` is supported (case-insensitive).
   * - ``<name>``
     - VARCHAR
     - The name of the semantic view, optionally schema-qualified (``analytics.sales``). An unqualified name resolves to the unique view of that name; if several schemas hold one, the result is an ambiguity error naming them, not a ``search_path`` walk (see :ref:`ref-get-ddl-resolution`). Returns an error if the view does not exist.
   * - ``<use_fully_qualified_names>``
     - BOOLEAN
     - Optional. When ``true``, the rendered ``CREATE`` name carries the view's own schema, so re-running the output recreates the view in that schema. Defaults to ``false`` (a bare name), matching Snowflake. A ``NULL`` returns ``NULL``.


.. _ref-get-ddl-output:

Output
======

Returns a single VARCHAR value containing the full ``CREATE OR REPLACE SEMANTIC VIEW`` DDL statement. The DDL includes all clauses (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS, MATERIALIZATIONS) with all annotations (COMMENT, WITH SYNONYMS, PRIVATE, NON ADDITIVE BY, OVER). The ``MATERIALIZATIONS`` clause is included only when the view has materializations declared; it is omitted for views without materializations.

The rendered DDL re-parses to the same definition. In particular:

- A relationship declared against a ``UNIQUE`` key (rather than the primary key) renders its ``REFERENCES <target>(<columns>)`` column list, so re-parsing keeps the join wired to the unique key instead of silently falling back to the primary key.
- A view name that needs quoting (embedded whitespace or non-ASCII characters) is quoted in the rendered ``CREATE OR REPLACE SEMANTIC VIEW`` header. (Mixed-case names are never quoted for case: names fold to lowercase — see :ref:`ref-create-semantic-view`.)
- With ``<use_fully_qualified_names>`` set to ``true``, the schema and the view name are quoted independently — ``"my schema"."my view"``, never ``"my schema.my view"`` — so the header re-parses as a schema-qualified reference rather than as one name containing a dot.

The schema rendered is where the view actually lives, not how the lookup was spelled: ``GET_DDL('SEMANTIC_VIEW', 'sales', true)`` on a view that lives in ``analytics`` renders ``analytics.sales``.

If the definition records no schema — possible only for a catalog row written before semantic views were schema-scoped and never migrated — asking for a fully-qualified name is an error rather than a silent fall back to the bare name, which would relocate the view on restore.


.. _ref-get-ddl-resolution:

How an unqualified name resolves
================================

``GET_DDL`` is a **scalar** function, and scalar functions have no named parameters, so the extension cannot hand it the session's ``search_path`` the way it does for the ``semantic_view()`` table function. An unqualified name therefore resolves to the **unique** view of that name, wherever it lives:

- exactly one schema holds it — that view is returned, whether or not its schema is on ``search_path``;
- several schemas hold it — an error naming them, so the answer is never a silent pick;
- none holds it — the usual ``does not exist`` error.

``SET search_path`` does **not** disambiguate here. Qualify the name instead:

.. code-block:: sql

   SET search_path = 'analytics';
   SELECT GET_DDL('SEMANTIC_VIEW', 'sales');    -- still an error if staging.sales exists too
   SELECT GET_DDL('SEMANTIC_VIEW', 'analytics.sales');  -- unambiguous

``READ_YAML_FROM_SEMANTIC_VIEW`` is a scalar too and follows the same rule. The DDL statements and ``semantic_view()`` **do** follow ``search_path`` — see :ref:`ref-create-semantic-view`.


.. _ref-get-ddl-examples:

Examples
========

**Retrieve DDL for a semantic view:**

.. code-block:: sql

   SELECT GET_DDL('SEMANTIC_VIEW', 'sales');

Sample output:

.. code-block:: text

   CREATE OR REPLACE SEMANTIC VIEW sales AS
   TABLES (
       o AS orders PRIMARY KEY (id) COMMENT = 'Order transactions'
   )
   DIMENSIONS (
       o.region AS o.region COMMENT = 'Sales region'
   )
   METRICS (
       o.revenue AS SUM(o.amount) COMMENT = 'Total revenue'
   )

**Retrieve DDL for a view with materializations:**

.. code-block:: sql

   SELECT GET_DDL('SEMANTIC_VIEW', 'order_metrics');

Sample output:

.. code-block:: text

   CREATE OR REPLACE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount),
       o.order_count AS COUNT(*)
   )
   MATERIALIZATIONS (
       region_agg AS (
           TABLE daily_revenue_by_region,
           DIMENSIONS (region),
           METRICS (revenue, order_count)
       )
   )

**Relationship against a UNIQUE key:**

When a relationship references a target's ``UNIQUE`` key rather than its primary key, the rendered ``REFERENCES`` carries the explicit column list so the round-trip preserves the join:

.. code-block:: text

   CREATE OR REPLACE SEMANTIC VIEW sales AS
   TABLES (
       o AS orders   PRIMARY KEY (id),
       c AS customers PRIMARY KEY (id) UNIQUE (email)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_email) REFERENCES c(email)
   )
   DIMENSIONS (
       c.customer AS c.name
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   )

**Round-trip verification:**

The DDL output can be executed to recreate the view with identical semantics:

.. code-block:: sql

   -- Save the DDL
   CREATE TABLE ddl_backup AS
   SELECT GET_DDL('SEMANTIC_VIEW', 'sales') AS ddl;

   -- Drop and recreate
   DROP SEMANTIC VIEW sales;

   -- Execute the saved DDL (copy-paste the output)

**Dump and restore across schemas:**

The default renders a bare name, so replaying it recreates the view in whatever schema the executing session is in — fine when there is one schema, wrong for a backup that must restore in place. Pass ``true`` to pin each view to its own schema:

.. code-block:: sql

   SELECT GET_DDL('SEMANTIC_VIEW', 'analytics.sales', true);

.. code-block:: text

   CREATE OR REPLACE SEMANTIC VIEW analytics.sales AS
   TABLES (
       o AS orders PRIMARY KEY (id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   )

Replayed from any session, that statement puts ``sales`` back in ``analytics``. To dump every view at once, build each lookup name with both parts quoted so a schema or view name containing whitespace or a dot survives the round trip:

.. code-block:: sql

   SELECT GET_DDL(
            'SEMANTIC_VIEW',
            '"' || replace(schema_name, '"', '""') || '"."'
                || replace(name, '"', '""') || '"',
            true) AS ddl
   FROM list_semantic_views();

**Error: unsupported object type:**

.. code-block:: sql

   SELECT GET_DDL('TABLE', 'orders');

.. code-block:: text

   Error: GET_DDL: unsupported object type 'TABLE'. Only 'SEMANTIC_VIEW' is supported.

**Error: view does not exist:**

.. code-block:: sql

   SELECT GET_DDL('SEMANTIC_VIEW', 'nonexistent');

.. code-block:: text

   Error: semantic view 'nonexistent' does not exist
