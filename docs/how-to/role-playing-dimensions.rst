.. meta::
   :description: Join the same table under multiple relationship aliases and use USING on metrics to resolve which path a query should traverse

.. _howto-role-playing:

============================================
How to Model Role-Playing Dimensions
============================================

This guide shows how to handle the role-playing dimension pattern, where the same table is joined via multiple relationships. A common example is an ``airports`` table joined to ``flights`` as both the departure airport and the arrival airport. The ``USING`` keyword on metrics tells the extension which join path to use.

**Prerequisites:**

- A working multi-table semantic view with relationships (see :ref:`tutorial-multi-table`)
- Understanding of how ``RELATIONSHIPS`` maps to JOIN clauses


.. _howto-rp-problem:

The Problem
===========

When the same table appears as the target of multiple relationships, dimensions from that table are ambiguous. The extension cannot determine which join path to use unless a co-queried metric specifies a ``USING`` clause.

Consider flights with departure and arrival airports:

.. code-block:: sql

   CREATE TABLE airports (airport_code VARCHAR, city VARCHAR, country VARCHAR);
   CREATE TABLE flights (
       flight_id INTEGER,
       departure_code VARCHAR,
       arrival_code VARCHAR,
       carrier VARCHAR
   );

Both ``departure_code`` and ``arrival_code`` point to ``airports``. A dimension like ``city`` from ``airports`` is ambiguous: is it the departure city or the arrival city?


.. _howto-rp-define:

Define a Role-Playing View
==========================

Declare two named relationships to the same target table. Then use ``USING`` on metrics to select which relationship path each metric traverses.

.. code-block:: sql
   :emphasize-lines: 8,9,16,17

   CREATE SEMANTIC VIEW flight_analytics AS
   TABLES (
       f AS flights  PRIMARY KEY (flight_id),
       a AS airports PRIMARY KEY (airport_code)
   )
   RELATIONSHIPS (
       dep_airport AS f(departure_code) REFERENCES a,
       arr_airport AS f(arrival_code)   REFERENCES a
   )
   DIMENSIONS (
       a.city    AS a.city,
       a.country AS a.country,
       f.carrier AS f.carrier
   )
   METRICS (
       f.departure_count USING (dep_airport) AS COUNT(*),
       f.arrival_count   USING (arr_airport) AS COUNT(*)
   );

The ``USING (dep_airport)`` clause tells the extension: when this metric is queried alongside a dimension from the ``airports`` table, use the ``dep_airport`` relationship to resolve the join path.


.. _howto-rp-query:

Query with USING Context
========================

**Departures by city:** the ``departure_count`` metric's USING context resolves ``city`` via ``dep_airport``:

.. code-block:: sql

   SELECT * FROM semantic_view('flight_analytics',
       dimensions := ['city'],
       metrics := ['departure_count']
   ) ORDER BY city;

**Arrivals by city:** the ``arrival_count`` metric's USING context resolves ``city`` via ``arr_airport``:

.. code-block:: sql

   SELECT * FROM semantic_view('flight_analytics',
       dimensions := ['city'],
       metrics := ['arrival_count']
   ) ORDER BY city;

**Non-ambiguous dimensions:** the ``carrier`` dimension comes from the ``flights`` table (not ``airports``), so it works with any metric without ambiguity:

.. code-block:: sql

   SELECT * FROM semantic_view('flight_analytics',
       dimensions := ['carrier'],
       metrics := ['departure_count', 'arrival_count']
   ) ORDER BY carrier;


.. _howto-rp-derived:

Derived Metrics with Role-Playing
=================================

Derived metrics that reference USING-annotated metrics inherit the USING context transitively:

.. code-block:: sql

   METRICS (
       f.departure_count USING (dep_airport) AS COUNT(*),
       f.arrival_count   USING (arr_airport) AS COUNT(*),
       total_flights     AS departure_count + arrival_count
   );

``total_flights`` depends on both ``dep_airport`` and ``arr_airport``. Querying ``total_flights`` with a non-ambiguous dimension like ``carrier`` works:

.. code-block:: sql

   SELECT * FROM semantic_view('flight_analytics',
       dimensions := ['carrier'],
       metrics := ['total_flights']
   ) ORDER BY carrier;

However, querying ``total_flights`` with the ambiguous ``city`` dimension fails. The extension cannot determine which single USING path should resolve ``city``.


.. _howto-rp-inspect:

Inspect the Scoped Aliases
==========================

Use :ref:`explain_semantic_view() <ref-explain-semantic-view>` to see how the extension creates scoped aliases for role-playing joins:

.. code-block:: sql

   SELECT * FROM explain_semantic_view('flight_analytics',
       dimensions := ['city'],
       metrics := ['departure_count']
   );

The expanded SQL shows the airports table joined with a scoped alias (e.g., ``a__dep_airport``) that reflects which relationship path was used.


.. _howto-rp-errors:

Troubleshooting
===============

**"dimension is ambiguous" error**
   This occurs when a dimension comes from a role-playing table and no co-queried metric
   provides a single USING path to disambiguate. Solutions:

   - Add a metric with ``USING`` that targets the desired relationship.
   - Use a dimension from a non-ambiguous table (like the base table).

**Multiple USING paths for the same table**
   If two co-queried metrics have different USING paths that both target the dimension's
   table, the extension raises an ambiguity error. Query only one USING-scoped metric at
   a time alongside the ambiguous dimension.

**Dimension on a descendant of a role-playing table**
   .. versionchanged:: 0.11.0

   A dimension on a table reached *through* a role-playing table -- for
   example ``region_name`` on ``regions`` where ``airports`` references
   ``regions`` -- is ambiguous, because which role's rows it groups by depends
   on which role you traversed. Such a query is now rejected (previously it
   silently used the first-declared relationship). Unlike a dimension directly
   on the role-playing table, a descendant **cannot** be disambiguated by a
   metric's ``USING``: give the target a distinct alias per role, or query it
   through a non-role-playing table.

**Facts on a role-playing table**
   .. versionchanged:: 0.11.0

   A fact sourced on a role-playing table (or a descendant of one) is now
   rejected. Facts carry no ``USING`` context, so the role was always
   unresolvable -- previously such a fact silently read the first-declared
   relationship's rows.

**Semi-additive metric snapshotting on a role-playing dimension**
   .. versionchanged:: 0.11.0

   A metric written ``m USING (<relationship>) NON ADDITIVE BY (<dim on the
   role-played table>)`` now selects the snapshot for the role its ``USING``
   names, instead of an arbitrary one. A semi-additive metric that snapshots on
   a role-playing dimension but has no ``USING`` to disambiguate the role is
   now rejected as ambiguous at query time -- the same error a directly-queried
   role-playing dimension raises. See :ref:`howto-semi-additive`.
