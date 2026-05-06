.. meta::
   :description: How transactional DDL works in duckdb-semantic-views, and the small set of caveats around read visibility, concurrent CREATE, and DDL across multiple connections

.. _explanation-transactional-ddl:

==========================================
Transactional DDL and Known Limitations
==========================================

.. versionadded:: 0.8.0

In v0.8.0, ``CREATE``, ``DROP``, and ``ALTER SEMANTIC VIEW`` became fully transactional: they participate in your surrounding ``BEGIN`` / ``COMMIT`` / ``ROLLBACK`` block the way ordinary DuckDB DDL does. ADBC, dbt-duckdb, and any other transaction-aware client now behave the way you'd expect.

This page explains what to expect day to day, and a short list of edge cases worth knowing about. Most of the edge cases only surface in unusual situations -- multiple processes touching the same database file at the same time, or scripts that explicitly toggle DuckDB's experimental PEG parser.

If your workload is "open a database, run some DDL at start-up, then query" you can read the next two sections and stop.


.. _explanation-txn-ddl-what-changed:

DDL Now Participates in Your Transaction
=========================================

You can wrap DDL in ``BEGIN`` / ``COMMIT`` and rely on the rollback semantics:

.. code-block:: sql

   BEGIN;
   CREATE SEMANTIC VIEW order_metrics AS
       TABLES (o AS orders) DIMENSIONS (o.region AS o.region);
   ROLLBACK;
   -- order_metrics does not exist; the CREATE was discarded.

   BEGIN;
   DROP SEMANTIC VIEW order_metrics;
   ROLLBACK;
   -- order_metrics is still there; the DROP was discarded.

   BEGIN;
   ALTER SEMANTIC VIEW order_metrics RENAME TO sales_metrics;
   ROLLBACK;
   -- the view is still called order_metrics.

This applies to every ``CREATE`` body variant: the ``AS`` keyword body, inline ``FROM YAML $$ ... $$``, ``FROM YAML FILE '<path>'``, and the ``CREATE OR REPLACE`` / ``IF NOT EXISTS`` modifiers. ``ALTER`` covers ``RENAME TO``, ``SET COMMENT``, and ``UNSET COMMENT``.

Before v0.8.0 these statements committed independently of the surrounding transaction, which meant ``ROLLBACK`` could not undo them. If you wrote DDL with that older behaviour in mind, you can simplify -- the transaction now does what it looks like it does.


.. _explanation-txn-ddl-write-visibility:

Reads Inside an Open Transaction See Committed State
=====================================================

The introspection commands -- ``DESCRIBE SEMANTIC VIEW``, ``SHOW SEMANTIC VIEWS`` (and the other ``SHOW SEMANTIC *`` variants), ``READ_YAML_FROM_SEMANTIC_VIEW``, ``GET_DDL`` -- always read what has been **committed**. They do not see the uncommitted changes from your own open transaction.

So this sequence:

.. code-block:: sql

   BEGIN;
   CREATE SEMANTIC VIEW v AS TABLES (o AS orders) DIMENSIONS (o.r AS o.region);
   SHOW SEMANTIC VIEWS;   -- v is NOT in the result yet
   COMMIT;
   SHOW SEMANTIC VIEWS;   -- now v is listed

is expected. The same applies to in-flight ``DROP`` (the row keeps appearing until commit) and ``ALTER ... RENAME TO`` (the row appears under its old name until commit). If you need ``SHOW`` or ``DESCRIBE`` to reflect a change, commit first.

A related point: when you query a semantic view with ``semantic_view(...)``, that query also reads committed state from your underlying tables. If you've inserted rows into ``orders`` inside an open transaction and then query a semantic view over ``orders`` in the same transaction, those new rows will not be included. Commit the data writes first, or do the data write and the semantic-view query in separate transactions.

This limitation will go away when DuckDB exposes the hook the extension needs; until then, the rule is "commit before introspecting."


.. _explanation-txn-ddl-create-race:

CREATE IF NOT EXISTS Across Multiple Connections
=================================================

.. note::

   This is mostly theoretical for typical DuckDB usage. DuckDB runs as an in-process library and most users have a single program talking to a database file. If that's you, ``CREATE SEMANTIC VIEW IF NOT EXISTS`` behaves exactly the way you'd expect, every time, and you can skip this section.

If two separate processes (or two separate connections from the same program) both run ``CREATE SEMANTIC VIEW IF NOT EXISTS my_view ...`` against the same database at the same time, and neither has committed yet when the other starts, both will try to create the view. One will win. The other will see:

.. code-block:: text

   Constraint Error: Duplicate key "name: my_view" violates primary key constraint

This is the same error a plain ``CREATE SEMANTIC VIEW`` would produce in the same race. ``IF NOT EXISTS`` reliably absorbs duplicates within a single process or single transaction; it cannot absorb two processes that both genuinely thought the view didn't exist.

If you do run parallel bootstrap scripts -- multi-worker container start-up, parallel test set-up, that kind of thing -- catch the constraint error on your view name and treat it as success. Something like:

.. code-block:: python

   try:
       conn.execute("CREATE SEMANTIC VIEW IF NOT EXISTS my_view ...")
   except duckdb.ConstraintException as e:
       if 'name: my_view' not in str(e):
           raise
       # someone else created it first; that's fine.

The first writer wins, the second writer sees a clear error rather than silent corruption, and after the catch both processes are in the same state.


.. _explanation-txn-ddl-drop-alter-race:

DROP and ALTER Without IF EXISTS Detect Concurrent Drops
=========================================================

If you run ``DROP SEMANTIC VIEW my_view`` (without ``IF EXISTS``) or any ``ALTER SEMANTIC VIEW my_view ...`` form (without ``IF EXISTS``), and another process drops the view at the same time, you'll see:

.. code-block:: text

   Invalid Input Error: semantic view 'my_view' was concurrently dropped

instead of a silent success. This is intentional -- you asked for an operation on a specific view, the view was there when the extension checked, and then it wasn't. Surfacing the race is more useful than pretending the operation succeeded.

The ``IF EXISTS`` variants (``DROP SEMANTIC VIEW IF EXISTS my_view``, ``ALTER SEMANTIC VIEW IF EXISTS my_view ...``) keep their silent-no-op contract by design: you opted in to "do nothing if the view isn't there", and that's what they do.

In a single-process workload there's no race window to worry about. This only surfaces if multiple processes are issuing DDL against the same database file at the same time.


.. _explanation-txn-ddl-peg:

DuckDB's Experimental PEG Parser
=================================

DuckDB ships an experimental alternative grammar called the "PEG parser" alongside its default parser. The extension supports both, so semantic-view DDL works either way.

There is one quirk worth knowing about. If you turn the PEG parser **off** mid-session with ``CALL disable_peg_parser()``, that pragma also resets a related setting that the extension depends on. Subsequent semantic-view DDL on the same connection will then fail with:

.. code-block:: text

   Parser Error: syntax error at or near "SEMANTIC"

Restore the setting in one statement:

.. code-block:: sql

   CALL disable_peg_parser();
   SET allow_parser_override_extension = 'FALLBACK';

If you don't touch ``disable_peg_parser`` you'll never see this. The extension installs the right setting at load time and keeps it that way.


.. _explanation-txn-ddl-summary:

Summary
========

For most users, the everyday-visible v0.8.x changes are:

- ``BEGIN ... ROLLBACK`` now genuinely rolls back ``CREATE``, ``DROP``, and ``ALTER SEMANTIC VIEW``. This is the headline improvement.

The other items on this page only matter in specific situations:

- Introspection inside an open transaction shows committed state -- commit before ``SHOW`` / ``DESCRIBE`` if you need to see your own pending changes.
- Concurrent ``CREATE IF NOT EXISTS`` from two processes can produce a constraint error on one of them; catch it and treat as success.
- Toggling ``disable_peg_parser`` requires re-setting one parser option afterwards.

See also:

- :ref:`ref-create-semantic-view` -- syntax for all four ``CREATE`` body forms.
- :ref:`ref-drop-semantic-view` -- ``DROP`` and ``DROP IF EXISTS``.
- :ref:`ref-alter-semantic-view` -- ``ALTER`` variants.
- :ref:`ref-error-messages` -- error catalogue.
