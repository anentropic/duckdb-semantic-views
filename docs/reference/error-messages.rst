.. meta::
   :description: Reference for all error messages produced by the extension, with causes and fixes for DDL, query, and near-miss detection errors

.. _ref-error-messages:

==============
Error Messages
==============

This page documents the error messages produced by DuckDB Semantic Views, their causes, and how to resolve them.


.. _ref-err-ddl:

DDL Errors (CREATE SEMANTIC VIEW)
=================================

These errors occur at define time when creating or replacing a semantic view.


Missing view name
-----------------

.. code-block:: text

   Missing view name after 'CREATE SEMANTIC VIEW'.

**Cause:** The ``CREATE SEMANTIC VIEW`` statement has no name before ``AS``.

**Fix:** Add a view name: ``CREATE SEMANTIC VIEW my_view AS ...``


Expected AS or FROM YAML
-------------------------

.. code-block:: text

   Expected 'AS' or 'FROM YAML' after view name.

**Cause:** The statement has a view name but is missing the ``AS`` keyword or ``FROM YAML`` keywords before the body.

**Fix:** Use either the keyword body (``CREATE SEMANTIC VIEW my_view AS TABLES (...)``) or the YAML body (``CREATE SEMANTIC VIEW my_view FROM YAML $$ ... $$``).


Missing TABLES clause
---------------------

.. code-block:: text

   Missing required clause 'TABLES'.

**Cause:** The body does not include a ``TABLES`` clause.

**Fix:** Add a ``TABLES`` clause as the first clause in the body.


No DIMENSIONS or METRICS
-------------------------

.. code-block:: text

   At least one of 'DIMENSIONS' or 'METRICS' is required.

**Cause:** The body has a ``TABLES`` clause but neither ``DIMENSIONS`` nor ``METRICS``.

**Fix:** Add at least one ``DIMENSIONS`` or ``METRICS`` clause.


Unknown clause keyword
----------------------

.. code-block:: text

   Unknown clause keyword '<word>'; did you mean '<KEYWORD>'?

**Cause:** A word in the body does not match any known clause keyword (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS, MATERIALIZATIONS).

**Fix:** Check spelling. The error suggests the closest valid keyword.


Duplicate clause
----------------

.. code-block:: text

   Duplicate clause keyword '<KEYWORD>'.

**Cause:** The same clause keyword appears more than once.

**Fix:** Combine entries into a single clause.


Clause out of order
-------------------

.. code-block:: text

   Clause '<KEYWORD>' appears out of order; clauses must appear as: TABLES,
   RELATIONSHIPS (optional), FACTS (optional), DIMENSIONS (optional),
   METRICS (optional), MATERIALIZATIONS (optional).

**Cause:** Clauses are not in the required order.

**Fix:** Reorder clauses to: TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS, MATERIALIZATIONS.


Unclosed parenthesis
--------------------

.. code-block:: text

   Unclosed '(' for clause '<KEYWORD>'.

**Cause:** A clause's opening parenthesis has no matching closing parenthesis.

**Fix:** Check for mismatched parentheses in the clause body.


Graph validation errors
-----------------------

.. code-block:: text

   table '<alias>' cannot reference itself

   Relationship graph contains a cycle: <alias1> -> <alias2> -> ...

   Diamond detected: table '<alias>' is reachable via multiple paths.
   Use named relationships for role-playing dimensions.

**Cause:** The relationship graph violates tree structure requirements.

**Fix:**

- **Self-reference:** A table cannot have a relationship pointing to itself.
- **Cycle:** Follow the chain in the error message to find the circular dependency and remove it.
- **Diamond:** If a table is reachable via multiple paths, give each path a unique relationship name (role-playing pattern) or restructure to remove the duplicate path.


Aggregate in FACTS
------------------

.. code-block:: text

   Fact '<name>' contains aggregate function '<func>'. Facts must be
   row-level expressions. Move aggregation to METRICS.

**Cause:** A fact expression uses an aggregate function like ``SUM``, ``COUNT``, or ``AVG``.

**Fix:** Move the aggregation to the ``METRICS`` clause. Facts are for row-level calculations only.


Circular fact or metric references
----------------------------------

.. code-block:: text

   Circular dependency detected in facts: <name1>, <name2>, ...

   Circular dependency detected in derived metrics: <name1>, <name2>, ...

**Cause:** Facts or derived metrics reference each other in a cycle.

**Fix:** Break the cycle by removing or restructuring the circular reference.


NON ADDITIVE BY dimension not found
------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   NON ADDITIVE BY dimension '<dim>' on metric '<name>' does not match any
   declared dimension. Did you mean '<suggestion>'?

**Cause:** A ``NON ADDITIVE BY`` clause references a dimension name that does not exist in the view's ``DIMENSIONS`` clause.

**Fix:** Check the dimension name against the declared dimensions. The error suggests close matches when available.


Window metric inner metric not found
-------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   Window metric '<name>': inner metric '<inner>' not found in semantic view
   metrics. Did you mean '<suggestion>'?

**Cause:** A window function metric wraps another metric (e.g., ``AVG(total_qty) OVER (...)``), but the inner metric ``total_qty`` does not exist in the ``METRICS`` clause.

**Fix:** Ensure the inner metric is declared before the window metric. The error suggests close matches.


Window metric EXCLUDING dimension not found
--------------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   Window metric '<name>': EXCLUDING dimension '<dim>' not found in semantic
   view dimensions. Did you mean '<suggestion>'?

**Cause:** A window metric's ``PARTITION BY EXCLUDING`` clause references a dimension that does not exist.

**Fix:** Check the dimension name. The error suggests close matches. All dimensions in ``EXCLUDING`` must be declared in the view's ``DIMENSIONS`` clause.


Window metric PARTITION BY dimension not found
-----------------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   Window metric '<name>': PARTITION BY dimension '<dim>' not found in semantic
   view dimensions. Did you mean '<suggestion>'?

**Cause:** A window metric's ``PARTITION BY`` clause (without ``EXCLUDING``) references a dimension that does not exist in the view's ``DIMENSIONS`` clause.

**Fix:** Check the dimension name. The error suggests close matches. All dimensions in ``PARTITION BY`` must be declared in the view's ``DIMENSIONS`` clause.


Window metric ORDER BY dimension not found
-------------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   Window metric '<name>': ORDER BY dimension '<dim>' not found in semantic
   view dimensions. Did you mean '<suggestion>'?

**Cause:** A window metric's ``ORDER BY`` clause references a dimension that does not exist in the view's ``DIMENSIONS`` clause.

**Fix:** Check the dimension name. The error suggests close matches.


OVER and NON ADDITIVE BY conflict
-----------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   Cannot combine OVER clause with NON ADDITIVE BY on metric '<name>'.
   Use one or the other.

**Cause:** A metric definition includes both a window function ``OVER (...)`` clause and a ``NON ADDITIVE BY (...)`` clause. These are mutually exclusive features.

**Fix:** Use either ``NON ADDITIVE BY`` (for semi-additive snapshot aggregation) or ``OVER`` (for window functions), not both on the same metric.


OVER clause not allowed on derived metric
-------------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   OVER clause not allowed on derived metric '<name>'. Only qualified metrics
   (alias.name) can use OVER.

**Cause:** A derived metric (one without a table alias) has an ``OVER`` clause. Window metrics require a qualified name (``alias.metric_name``) because they need a source table for join resolution.

**Fix:** Add a table alias to the metric name: change ``my_metric AS AVG(total) OVER (...)`` to ``alias.my_metric AS AVG(total) OVER (...)``.


.. _ref-err-materialization:

Materialization Errors
-----------------------

.. versionadded:: 0.7.0

These errors occur when validating the ``MATERIALIZATIONS`` clause.


Duplicate materialization name
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. code-block:: text

   Duplicate materialization name '<name>'.

**Cause:** Two or more materializations in the same view share the same name.

**Fix:** Give each materialization a unique name.


Materialization dimension not found
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. code-block:: text

   Materialization '<name>': dimension '<dim>' not found in semantic view
   dimensions. Did you mean '<suggestion>'?

**Cause:** A materialization references a dimension name that does not exist in the view's ``DIMENSIONS`` clause.

**Fix:** Check the dimension name against the declared dimensions. The error suggests close matches.


Materialization metric not found
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. code-block:: text

   Materialization '<name>': metric '<met>' not found in semantic view
   metrics. Did you mean '<suggestion>'?

**Cause:** A materialization references a metric name that does not exist in the view's ``METRICS`` clause.

**Fix:** Check the metric name against the declared metrics. The error suggests close matches.


Empty materialization coverage
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

.. code-block:: text

   Materialization '<name>': must specify at least one of DIMENSIONS or METRICS.

**Cause:** A materialization entry declares only a ``TABLE`` with neither ``DIMENSIONS`` nor ``METRICS``.

**Fix:** Add at least one of ``DIMENSIONS (...)`` or ``METRICS (...)`` to the materialization entry.


.. _ref-err-yaml:

YAML Errors
============

.. versionadded:: 0.7.0

These errors occur when using ``FROM YAML`` or ``FROM YAML FILE`` to create a semantic view, or when exporting with :ref:`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>`.


Dollar-quote errors
--------------------

.. code-block:: text

   Expected '$' to begin dollar-quoted string

   Unterminated dollar-quote opening delimiter

   Unterminated dollar-quoted string (expected closing '<delimiter>')

**Cause:** The ``FROM YAML`` body is not properly enclosed in dollar-quote delimiters.

**Fix:** Ensure the YAML content starts with ``$$`` (or ``$tag$``) and ends with the matching closing delimiter.


Trailing content after dollar-quote
-------------------------------------

.. code-block:: text

   Unexpected content after closing dollar-quote: '<text>'

**Cause:** Extra text appears after the closing ``$$`` delimiter.

**Fix:** Remove any content between the closing ``$$`` and the statement terminator.


Empty file path
----------------

.. code-block:: text

   File path cannot be empty.

**Cause:** ``FROM YAML FILE`` was used with an empty single-quoted string.

**Fix:** Provide a valid file path: ``FROM YAML FILE '/path/to/file.yaml'``


Missing file path quotes
--------------------------

.. code-block:: text

   Expected single-quoted file path after FILE keyword.

**Cause:** The file path after ``FROM YAML FILE`` is not enclosed in single quotes.

**Fix:** Use single quotes around the file path: ``FROM YAML FILE '/path/to/file.yaml'``


YAML size limit exceeded
--------------------------

.. code-block:: text

   YAML definition for semantic view '<name>' exceeds size limit
   (<size> bytes > <cap> bytes).

**Cause:** The YAML content exceeds the 1 MiB (1,048,576 bytes) size cap.

**Fix:** Reduce the YAML definition size, or split the semantic view into multiple smaller views.


YAML parsing error
-------------------

.. code-block:: text

   YAML deserialization error: <details>

**Cause:** The YAML content is not valid YAML or does not match the expected semantic view definition schema.

**Fix:** Check the YAML syntax and structure. The definition must include ``tables``, and at least one of ``dimensions`` or ``metrics``.


.. _ref-err-query:

Query Errors (semantic_view)
============================

These errors occur at query time when calling :ref:`semantic_view() <ref-semantic-view-function>` or :ref:`explain_semantic_view() <ref-explain-semantic-view>`.


View not found
--------------

.. code-block:: text

   Semantic view '<name>' not found. Did you mean '<suggestion>'?
   Available views: [<list>].
   Run FROM list_semantic_views() to see all registered views.

**Cause:** No semantic view with the given name exists.

**Fix:** Check the view name. The error shows available views and suggests close matches.


Empty request
-------------

.. code-block:: text

   semantic view '<name>': specify at least dimensions := [...], metrics := [...],
   or facts := [...].
   Run DESCRIBE SEMANTIC VIEW <name> to see available dimensions, metrics, and facts.

**Cause:** Neither ``dimensions``, ``metrics``, nor ``facts`` was specified in the query.

**Fix:** Add at least one of ``dimensions := [...]``, ``metrics := [...]``, or ``facts := [...]``.


Unknown dimension
-----------------

.. code-block:: text

   semantic view '<view>': unknown dimension '<name>'. Available: [<list>].
   Did you mean '<suggestion>'?

**Cause:** A requested dimension name does not match any dimension in the view.

**Fix:** Check the dimension name. Use :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` to see all available dimensions.


Unknown metric
--------------

.. code-block:: text

   semantic view '<view>': unknown metric '<name>'. Available: [<list>].
   Did you mean '<suggestion>'?

**Cause:** A requested metric name does not match any metric in the view.

**Fix:** Check the metric name. Use :ref:`DESCRIBE SEMANTIC VIEW <ref-describe-semantic-view>` to see all available metrics.


Duplicate dimension or metric
-----------------------------

.. code-block:: text

   semantic view '<view>': duplicate dimension '<name>'

   semantic view '<view>': duplicate metric '<name>'

**Cause:** The same dimension or metric name appears more than once in the request.

**Fix:** Remove the duplicate from the ``dimensions`` or ``metrics`` list.


Fan trap detected
-----------------

.. code-block:: text

   semantic view '<view>': fan trap detected -- metric '<metric>' (table '<table>')
   would be duplicated when joined to dimension '<dim>' (table '<table>') via
   relationship '<rel>' (many-to-one cardinality, inferred: FK is not PK/UNIQUE).
   This would inflate aggregation results. Remove the dimension, use a metric from
   the same table, or restructure the relationship.

**Cause:** The query would traverse a one-to-many join boundary, inflating aggregate results.

**Fix:** See :ref:`howto-fan-traps` for detailed solutions: remove the problematic dimension, use a metric from the same table as the dimension, or restructure the view.


Ambiguous dimension path
-------------------------

.. code-block:: text

   semantic view '<view>': dimension '<dim>' is ambiguous -- table '<table>'
   is reached via multiple relationships: [<rel1>, <rel2>]. Specify a metric
   with USING to disambiguate, or use a dimension from a non-ambiguous table.

**Cause:** A dimension comes from a table reachable via multiple named relationships (role-playing pattern), and no co-queried metric has a ``USING`` clause that selects one path.

**Fix:** See :ref:`howto-role-playing`. Add a metric with ``USING (<rel_name>)`` to disambiguate, or use a dimension from a table that is not a role-playing target.


Private metric
--------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': metric '<name>' is private and cannot be queried
   directly. Private metrics can only be used in derived metric expressions.

**Cause:** A metric marked ``PRIVATE`` was requested in the ``metrics := [...]`` list.

**Fix:** Private metrics exist for internal composition only. Query a public derived metric that uses the private metric, or recreate the view without the ``PRIVATE`` keyword. See :ref:`howto-metadata-annotations`.


Private fact
------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': fact '<name>' is private and cannot be queried
   directly. Private facts can only be used in derived expressions.

**Cause:** A fact marked ``PRIVATE`` was requested in the ``facts := [...]`` list.

**Fix:** Remove the ``PRIVATE`` keyword from the fact to make it queryable, or reference the fact only from metric expressions.


Cannot combine facts and metrics
----------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': cannot combine facts and metrics in the same query.
   Use facts := [...] OR metrics := [...], not both.

**Cause:** The query includes both ``facts := [...]`` and ``metrics := [...]``. Facts are row-level expressions; metrics are aggregated. These modes are mutually exclusive.

**Fix:** Run two separate queries -- one for facts and one for metrics.


Unknown fact
------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': unknown fact '<name>'. Available: [<list>].
   Did you mean '<suggestion>'?

**Cause:** A requested fact name does not match any fact in the view's ``FACTS`` clause.

**Fix:** Check the fact name against declared facts. The error suggests close matches.


Duplicate fact
--------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': duplicate fact '<name>'

**Cause:** The same fact name appears more than once in the ``facts := [...]`` list.

**Fix:** Remove the duplicate from the ``facts`` list.


Incompatible table paths for facts
------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': fact query references objects from incompatible
   table paths -- tables '<table_a>' and '<table_b>' are not on the same
   root-to-leaf path in the relationship tree

**Cause:** A fact query combines facts and dimensions from tables that are on different branches of the relationship tree. All objects in a fact query must be on the same root-to-leaf path.

**Fix:** Restrict the query to facts and dimensions from tables that share a direct path in the relationship tree.


Window and aggregate metric mixing
------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': cannot mix window function metrics [<window_metrics>]
   with aggregate metrics [<aggregate_metrics>] in the same query

**Cause:** The query requests both window function metrics and standard aggregate metrics. These produce different result shapes (row-level vs. grouped) and cannot be combined.

**Fix:** Run separate queries for window metrics and aggregate metrics.


Window metric required dimension missing
------------------------------------------

.. versionadded:: 0.6.0

.. code-block:: text

   semantic view '<view>': window function metric '<metric>' requires
   dimension '<dim>' to be included in the query (used in <reason>)

**Cause:** A window function metric references a dimension in its ``PARTITION BY EXCLUDING``, ``PARTITION BY``, or ``ORDER BY`` clause, but that dimension was not included in the query's ``dimensions := [...]`` list. The ``<reason>`` value indicates which clause requires the dimension (``PARTITION BY EXCLUDING``, ``PARTITION BY``, or ``ORDER BY``).

**Fix:** Add the required dimension to the query. Use :ref:`SHOW SEMANTIC DIMENSIONS FOR METRIC <ref-show-dims-for-metric>` to see which dimensions are required (``required = TRUE``) for a window metric.


.. _ref-err-wildcard:

Wildcard Errors
================

.. versionadded:: 0.6.0

These errors occur when using ``alias.*`` wildcard patterns in ``dimensions``, ``metrics``, or ``facts`` parameters.


Unqualified wildcard
--------------------

.. code-block:: text

   unqualified wildcard '*' is not supported. Use table_alias.* to select
   all items for a specific table.

**Cause:** A bare ``*`` was used without a table alias prefix.

**Fix:** Use ``alias.*`` instead of ``*``. For example: ``dimensions := ['o.*']`` to select all dimensions from the ``o`` table alias.


Unknown table alias in wildcard
---------------------------------

.. code-block:: text

   unknown table alias '<alias>' in wildcard '<alias>.*'. Available aliases:
   [<list>]

**Cause:** The table alias in a wildcard expression does not match any alias declared in the view's ``TABLES`` clause.

**Fix:** Check the alias name against the declared table aliases. The error lists all available aliases.


.. _ref-err-near-miss:

Near-Miss DDL Detection
========================

The extension detects near-miss DDL statements and provides helpful suggestions:

.. code-block:: text

   Did you mean 'CREATE SEMANTIC VIEW'?

   Did you mean 'DROP SEMANTIC VIEW'?

This triggers when the input is close to a valid semantic view DDL prefix but contains a typo (e.g., ``CREAT SEMANTIC VIEW`` or ``DROP SEMANTC VIEW``). The detection uses Levenshtein distance with a threshold of 3 edits.
