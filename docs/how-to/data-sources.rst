.. meta::
   :description: Connect semantic views to Parquet files, CSV, Apache Iceberg tables, Postgres, and mixed data sources that DuckDB can query

.. _howto-data-sources:

==========================================
How to Use Different Data Sources
==========================================

This guide shows how to define semantic views over tables from various data sources that DuckDB supports. Semantic views work over any table that DuckDB can see: Parquet files, CSV files, Iceberg tables, Postgres tables, or any other source accessible through a DuckDB extension.

**Prerequisites:**

- Completed the :ref:`tutorial-getting-started` tutorial
- DuckDB installed with the relevant data source extensions


.. _howto-ds-parquet:

Parquet Files
=============

Create a DuckDB table from a Parquet file, then define a semantic view over it:

.. code-block:: sql

   CREATE TABLE orders AS SELECT * FROM read_parquet('orders.parquet');
   CREATE TABLE customers AS SELECT * FROM read_parquet('customers.parquet');

   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       o AS orders    PRIMARY KEY (order_id),
       c AS customers PRIMARY KEY (customer_id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c
   )
   DIMENSIONS (
       c.name   AS c.customer_name,
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   );

Alternatively, create a view over the Parquet file and use that in the semantic view:

.. code-block:: sql

   CREATE VIEW orders AS SELECT * FROM read_parquet('orders.parquet');


.. _howto-ds-csv:

CSV Files
=========

.. code-block:: sql

   CREATE TABLE products AS SELECT * FROM read_csv('products.csv',
       auto_detect=true
   );

Then define a semantic view over the ``products`` table as normal.


.. _howto-ds-iceberg:

Iceberg Tables
==============

Load the ``iceberg`` extension and scan Iceberg tables:

.. code-block:: sql

   INSTALL iceberg;
   LOAD iceberg;

   CREATE TABLE orders AS
       SELECT * FROM iceberg_scan('s3://my-bucket/warehouse/orders');

   CREATE SEMANTIC VIEW order_metrics AS
   TABLES (
       o AS orders PRIMARY KEY (order_id)
   )
   DIMENSIONS (
       o.region   AS o.region,
       o.category AS o.category
   )
   METRICS (
       o.revenue     AS SUM(o.amount),
       o.order_count AS COUNT(*)
   );

S3 Credentials
--------------

DuckDB needs S3 credentials to read from cloud storage. Configure them before running ``iceberg_scan``:

.. code-block:: sql

   SET s3_region = 'us-east-1';
   SET s3_access_key_id = 'AKIA...';
   SET s3_secret_access_key = '...';

   -- Or use the httpfs extension's credential chain
   INSTALL httpfs;
   LOAD httpfs;
   SET s3_url_style = 'path';

See the `DuckDB httpfs documentation <https://duckdb.org/docs/extensions/httpfs/s3api>`_ for credential chain options including environment variables and instance profiles.

Iceberg Catalog Types
---------------------

The ``iceberg_scan`` function reads directly from the Iceberg metadata path. If your tables are managed by an Iceberg catalog (Hive metastore, AWS Glue, REST catalog), point to the metadata location that the catalog provides:

.. code-block:: sql

   -- Direct metadata path
   CREATE TABLE orders AS
       SELECT * FROM iceberg_scan('s3://bucket/warehouse/orders/metadata/v3.metadata.json');

   -- Or use the table path if the latest metadata pointer exists
   CREATE TABLE orders AS
       SELECT * FROM iceberg_scan('s3://bucket/warehouse/orders');

Using a View Instead of a Table
-------------------------------

Creating a ``VIEW`` instead of a ``TABLE`` keeps queries reading the latest Iceberg snapshot rather than a static copy:

.. code-block:: sql

   CREATE VIEW orders AS
       SELECT * FROM iceberg_scan('s3://my-bucket/warehouse/orders');

This is useful when the underlying Iceberg table changes frequently. The trade-off is that each :ref:`semantic_view() <ref-semantic-view-function>` query re-scans the Iceberg metadata.

Schema Evolution
----------------

If columns are added or removed from the Iceberg table, update the semantic view to match:

- **New column:** Add a dimension or metric referencing the new column, then ``CREATE OR REPLACE SEMANTIC VIEW`` to update the definition.
- **Removed column:** Any dimension or metric referencing the dropped column will cause a query-time SQL error. Update the semantic view definition to remove those references.

.. tip::

   For a DuckDB + Iceberg + analytics application stack, semantic views provide
   a stable query interface over Iceberg tables. The application queries
   :ref:`semantic_view() <ref-semantic-view-function>` with dimension and metric names, and the extension handles
   schema mapping and join logic.


.. _howto-ds-postgres:

Postgres via postgres_scanner
=============================

Attach a Postgres database and create tables or views from it:

.. code-block:: sql

   INSTALL postgres;
   LOAD postgres;

   ATTACH 'dbname=mydb user=myuser host=localhost' AS pg (TYPE POSTGRES);

   CREATE TABLE orders AS SELECT * FROM pg.public.orders;
   CREATE TABLE customers AS SELECT * FROM pg.public.customers;

Then define a semantic view over the local tables as usual. Alternatively, use the attached tables directly if DuckDB can resolve them.


.. _howto-ds-mixed:

Mixed Sources
=============

Semantic views work with any combination of sources. The only requirement is that each table exists in DuckDB at query time.

.. code-block:: sql

   -- Orders from Iceberg
   CREATE TABLE orders AS
       SELECT * FROM iceberg_scan('s3://bucket/warehouse/orders');

   -- Customers from Postgres
   CREATE TABLE customers AS
       SELECT * FROM pg.public.customers;

   -- Products from a local Parquet file
   CREATE TABLE products AS
       SELECT * FROM read_parquet('products.parquet');

   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       o AS orders    PRIMARY KEY (order_id),
       c AS customers PRIMARY KEY (customer_id),
       p AS products  PRIMARY KEY (product_id)
   )
   RELATIONSHIPS (
       order_customer AS o(customer_id) REFERENCES c,
       order_product  AS o(product_id)  REFERENCES p
   )
   DIMENSIONS (
       c.customer AS c.name,
       p.product  AS p.name,
       o.region   AS o.region
   )
   METRICS (
       o.revenue     AS SUM(o.amount),
       o.order_count AS COUNT(*)
   );


.. _howto-ds-catalog:

Catalog-Qualified Table Names
=============================

If your tables live in a specific catalog or schema, use the fully qualified table name in the ``TABLES`` clause:

.. code-block:: sql

   CREATE SEMANTIC VIEW analytics AS
   TABLES (
       o AS my_catalog.my_schema.orders PRIMARY KEY (order_id)
   )
   DIMENSIONS (
       o.region AS o.region
   )
   METRICS (
       o.revenue AS SUM(o.amount)
   );

The extension quotes each segment of the table name separately (``"my_catalog"."my_schema"."orders"``) in the generated SQL.
