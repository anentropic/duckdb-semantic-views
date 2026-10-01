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


.. _howto-rp-diamond:

Role-Playing vs an Ambiguous Diamond
====================================

.. versionchanged:: 0.11.0

   A table reached from two *different* source tables -- a join "diamond" -- is
   rejected at ``CREATE`` time. Earlier versions accepted it and joined the
   shared table through whichever relationship was declared first, which gave
   wrong numbers when the two paths lead to different rows.

Role-playing and a diamond both reach one table by two routes, but only one of
them tells the extension which route a query means:

- **Role-playing (supported):** several named relationships from **one** source
  table to one target, like ``dep_airport`` and ``arr_airport`` from
  ``flights`` to ``airports`` above. ``USING`` on a metric picks the route.
- **Diamond (rejected):** one target reached from two **different** source
  tables. In the view below, ``regions`` is reached from both ``customers`` and
  ``stores``, so a ``region`` dimension could mean the customer's region or the
  store's:

.. code-block:: sql

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       o AS orders    PRIMARY KEY (order_id),
       c AS customers PRIMARY KEY (customer_id),
       s AS stores    PRIMARY KEY (store_id),
       r AS regions   PRIMARY KEY (region_id)
   )
   RELATIONSHIPS (
       order_customer  AS o(customer_id) REFERENCES c,
       order_store     AS o(store_id)    REFERENCES s,
       customer_region AS c(region_id)   REFERENCES r,
       store_region    AS s(region_id)   REFERENCES r
   )
   DIMENSIONS (
       r.region AS r.name
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   );

.. code-block:: text

   diamond: 'r' is reachable from multiple tables ('c', 's'); the join path is
   ambiguous. Declare the target under a second table alias so it is joined once
   per path.

Fix it as the message says: declare ``regions`` once per path, under two
aliases, and give each alias its own dimension. Each route then has its own
table and there is nothing left to choose between:

.. code-block:: sql
   :emphasize-lines: 6,7,12,13,16,17

   CREATE SEMANTIC VIEW sales AS
   TABLES (
       o  AS orders    PRIMARY KEY (order_id),
       c  AS customers PRIMARY KEY (customer_id),
       s  AS stores    PRIMARY KEY (store_id),
       cr AS regions   PRIMARY KEY (region_id),
       sr AS regions   PRIMARY KEY (region_id)
   )
   RELATIONSHIPS (
       order_customer  AS o(customer_id) REFERENCES c,
       order_store     AS o(store_id)    REFERENCES s,
       customer_region AS c(region_id)   REFERENCES cr,
       store_region    AS s(region_id)   REFERENCES sr
   )
   DIMENSIONS (
       cr.customer_region AS cr.name,
       sr.store_region    AS sr.name
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   );

A query can now group by ``customer_region``, ``store_region`` or both.

Two relationships from one source to two *different* targets, like
``orders → customers`` and ``orders → products`` in :ref:`tutorial-multi-table`,
are neither pattern and need nothing extra.


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


.. _howto-rp-related:

Related
=======

- :ref:`ref-create-relationships` -- ``RELATIONSHIPS`` syntax and validation rules
- :ref:`ref-create-metrics` -- ``USING`` in the metric grammar
- :ref:`howto-fan-traps` -- What a role-played dimension does to multi-grain queries
- :ref:`howto-derived-metrics` -- Composing metrics that carry ``USING`` context
