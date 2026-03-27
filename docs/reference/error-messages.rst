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


Expected AS keyword
-------------------

.. code-block:: text

   Expected 'AS' keyword at start of semantic view body.

**Cause:** The statement has a view name but is missing the ``AS`` keyword before the body clauses.

**Fix:** Add ``AS`` after the view name: ``CREATE SEMANTIC VIEW my_view AS TABLES (...)``


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

**Cause:** A word in the body does not match any known clause keyword (TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS).

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
   RELATIONSHIPS (optional), FACTS (optional), DIMENSIONS (optional), METRICS (optional).

**Cause:** Clauses are not in the required order.

**Fix:** Reorder clauses to: TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS.


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

   semantic view '<name>': specify at least dimensions := [...] or metrics := [...].
   Run FROM describe_semantic_view('<name>') to see available dimensions and metrics.

**Cause:** Neither ``dimensions`` nor ``metrics`` was specified in the query.

**Fix:** Add at least one of ``dimensions := [...]`` or ``metrics := [...]``.


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


SQL execution failed
--------------------

.. code-block:: text

   SQL execution failed: <duckdb_error>
   Expanded SQL:
   <generated_sql>

**Cause:** The generated SQL failed when DuckDB executed it. Common causes include dropped tables, column name changes, or type incompatibilities.

**Fix:** Check that the underlying tables still exist and their schemas match the semantic view definition. Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to inspect the generated SQL.


.. _ref-err-near-miss:

Near-Miss DDL Detection
========================

The extension detects near-miss DDL statements and provides helpful suggestions:

.. code-block:: text

   Did you mean 'CREATE SEMANTIC VIEW'?

   Did you mean 'DROP SEMANTIC VIEW'?

This triggers when the input is close to a valid semantic view DDL prefix but contains a typo (e.g., ``CREAT SEMANTIC VIEW`` or ``DROP SEMANTC VIEW``). The detection uses Levenshtein distance with a threshold of 3 edits.
